//! Per-agent publish rate limiting (RFC-ACDP-0008 §4.3 REQUIRED).
//!
//! A fixed-window counter keyed by the signing `agent_id`. In-memory and
//! per-process: a horizontally-scaled deployment should additionally bound
//! publishes at a shared layer (gateway / Redis). We deliberately avoid a
//! new dependency (`dashmap`) — a `Mutex<HashMap>` is ample for the single
//! lock-per-publish access pattern, matching the workspace's dep-graph
//! minimization principle.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
/// Opportunistic prune threshold — keeps the agent map from growing without
/// bound when many distinct agents publish once and never return.
const PRUNE_AT: usize = 4096;

struct Bucket {
    window_start: Instant,
    count: u32,
}

/// Fixed-window limiter: at most `limit` publishes per 60s per agent, plus an
/// optional process-global ceiling across ALL keys per 60s.
pub struct AgentRateLimiter {
    limit: u32,
    buckets: Mutex<HashMap<String, Bucket>>,
    /// Process-wide ceiling per window. `u32::MAX` ⇒ effectively disabled.
    /// Defends the unauthenticated `/auth/challenge` endpoint (#24): the
    /// per-agent key is attacker-controlled, so varying `agent_id` bypasses
    /// the per-key bound — the global counter caps total flooding regardless.
    global_limit: u32,
    global: Mutex<Bucket>,
}

impl AgentRateLimiter {
    /// Construct a limiter allowing `limit_per_minute` per agent, with no
    /// global ceiling (used by the publish limiter, keyed by the verified
    /// producer `agent_id`).
    pub fn new(limit_per_minute: u32) -> Self {
        Self::with_global_ceiling(limit_per_minute, u32::MAX)
    }

    /// Like [`new`](Self::new) but also enforces a process-global ceiling of
    /// `global_limit_per_minute` across all keys.
    pub fn with_global_ceiling(limit_per_minute: u32, global_limit_per_minute: u32) -> Self {
        Self {
            limit: limit_per_minute,
            buckets: Mutex::new(HashMap::new()),
            global_limit: global_limit_per_minute,
            global: Mutex::new(Bucket {
                window_start: Instant::now(),
                count: 0,
            }),
        }
    }

    /// Check the process-global ceiling (#24). Call this in addition to
    /// [`check`](Self::check) on unauthenticated endpoints where the per-key
    /// identity is attacker-controlled.
    pub fn check_global(&self) -> Result<(), u64> {
        self.check_global_at(Instant::now())
    }

    fn check_global_at(&self, now: Instant) -> Result<(), u64> {
        if self.global_limit == u32::MAX {
            return Ok(());
        }
        let mut b = self.global.lock().unwrap_or_else(|e| e.into_inner());
        if now.duration_since(b.window_start) >= WINDOW {
            b.window_start = now;
            b.count = 0;
        }
        if b.count >= self.global_limit {
            let elapsed = now.duration_since(b.window_start);
            return Err(WINDOW.saturating_sub(elapsed).as_secs().max(1));
        }
        b.count += 1;
        Ok(())
    }

    /// Read-only budget test for `agent_id`. **Never mutates, never inserts.**
    ///
    /// This is the half of the old `check` that is safe to run on an
    /// **unverified** `agent_id`. `check` combined a read and a write, so
    /// calling it before signature verification let an unauthenticated caller
    /// (a) spend another agent's budget by naming them, and (b) grow the bucket
    /// map without bound by naming a fresh id each time -- the map key was
    /// attacker-controlled. `peek` answers the question the pre-pipeline
    /// rejection actually needs ("is this agent already over budget?") without
    /// either effect.
    ///
    /// An absent bucket and an expired window both read as count 0. The expired
    /// case deliberately does NOT roll the window over: rolling is a write, and
    /// a caller who never earns a charge must leave no trace at all.
    pub fn peek(&self, agent_id: &str) -> Result<(), u64> {
        self.peek_at(agent_id, Instant::now())
    }

    fn peek_at(&self, agent_id: &str, now: Instant) -> Result<(), u64> {
        let map = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        // No bucket => nothing spent this window.
        let Some(bucket) = map.get(agent_id) else {
            return Ok(());
        };
        // Expired window => nothing spent, and we do NOT roll it over here.
        if now.duration_since(bucket.window_start) >= WINDOW {
            return Ok(());
        }
        if bucket.count >= self.limit {
            let elapsed = now.duration_since(bucket.window_start);
            // Mirrors `check_at` exactly, including the 1s floor so a client
            // never sees `Retry-After: 0`.
            return Err(WINDOW.saturating_sub(elapsed).as_secs().max(1));
        }
        Ok(())
    }

    /// Charge one unit against `agent_id`'s budget. Infallible: this is the
    /// write half, called only once a publish has actually succeeded and the
    /// `agent_id` is therefore verified.
    ///
    /// **Rolls the window over exactly as `check_at` does.** A "just increment"
    /// implementation would accumulate across windows and permanently trip the
    /// bucket -- the count would never reset, so an agent that published
    /// steadily would eventually be locked out forever. The rollover is not an
    /// optimisation; it is what makes this a fixed-window limiter rather than a
    /// lifetime quota.
    ///
    /// Pruning lives here rather than in `peek` because this is the only entry
    /// point that inserts, so it is the only one that can grow the map.
    pub fn record(&self, agent_id: &str) {
        self.record_at(agent_id, Instant::now());
    }

    fn record_at(&self, agent_id: &str, now: Instant) {
        let mut map = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= PRUNE_AT {
            map.retain(|_, b| now.duration_since(b.window_start) < WINDOW);
        }
        let bucket = map.entry(agent_id.to_string()).or_insert(Bucket {
            window_start: now,
            count: 0,
        });
        if now.duration_since(bucket.window_start) >= WINDOW {
            bucket.window_start = now;
            bucket.count = 0;
        }
        // Saturating rather than wrapping: a wrapped count would silently reset
        // the bucket to 0 and hand the agent a fresh budget.
        bucket.count = bucket.count.saturating_add(1);
    }

    /// Record one publish attempt by `agent_id`. Returns `Err(retry_after_secs)`
    /// when the agent is over budget for the current window, otherwise `Ok`.
    pub fn check(&self, agent_id: &str) -> Result<(), u64> {
        self.check_at(agent_id, Instant::now())
    }

    fn check_at(&self, agent_id: &str, now: Instant) -> Result<(), u64> {
        let mut map = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= PRUNE_AT {
            map.retain(|_, b| now.duration_since(b.window_start) < WINDOW);
        }
        let bucket = map.entry(agent_id.to_string()).or_insert(Bucket {
            window_start: now,
            count: 0,
        });
        if now.duration_since(bucket.window_start) >= WINDOW {
            bucket.window_start = now;
            bucket.count = 0;
        }
        if bucket.count >= self.limit {
            let elapsed = now.duration_since(bucket.window_start);
            // At least 1s so a client never sees `Retry-After: 0`.
            let retry = WINDOW.saturating_sub(elapsed).as_secs().max(1);
            return Err(retry);
        }
        bucket.count += 1;
        Ok(())
    }
}

/// A single CIDR block, used to recognise trusted reverse proxies (FEAT-06).
///
/// Stored as the network base address plus a prefix length so matching is a
/// masked bitwise compare — no allocation, no external `ipnet` dependency
/// (matching the workspace's dep-minimization principle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cidr {
    base: IpAddr,
    prefix_len: u8,
}

impl Cidr {
    /// Parse `"10.0.0.0/8"` / `"fc00::/7"`. A bare IP (`"1.2.3.4"`) is treated
    /// as a `/32` (v4) or `/128` (v6) host route.
    fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (addr_part, prefix_part) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let base: IpAddr = addr_part
            .parse()
            .map_err(|_| format!("invalid IP in CIDR '{s}'"))?;
        let max = if base.is_ipv4() { 32 } else { 128 };
        let prefix_len = match prefix_part {
            Some(p) => p
                .parse::<u8>()
                .map_err(|_| format!("invalid prefix in CIDR '{s}'"))?,
            None => max,
        };
        if prefix_len > max {
            return Err(format!("prefix /{prefix_len} out of range for CIDR '{s}'"));
        }
        Ok(Self { base, prefix_len })
    }

    /// Does `ip` fall within this block? IPv4-mapped IPv6 peers are compared
    /// after canonicalisation (done by the caller), so a v4 CIDR matches a
    /// `::ffff:a.b.c.d` peer.
    fn contains(&self, ip: IpAddr) -> bool {
        match (self.base, ip) {
            (IpAddr::V4(b), IpAddr::V4(o)) => masked_eq(&b.octets(), &o.octets(), self.prefix_len),
            (IpAddr::V6(b), IpAddr::V6(o)) => masked_eq(&b.octets(), &o.octets(), self.prefix_len),
            _ => false,
        }
    }
}

/// Compare the top `prefix_len` bits of two equal-length octet arrays.
fn masked_eq(a: &[u8], b: &[u8], prefix_len: u8) -> bool {
    let mut bits = prefix_len as usize;
    for (x, y) in a.iter().zip(b.iter()) {
        if bits == 0 {
            break;
        }
        let take = bits.min(8);
        let mask = if take == 8 {
            0xFFu8
        } else {
            // top `take` bits set
            !(0xFFu8 >> take)
        };
        if (x & mask) != (y & mask) {
            return false;
        }
        bits -= take;
    }
    true
}

/// Operator-configured set of reverse-proxy CIDRs whose `X-Forwarded-For`
/// header this registry is willing to trust (FEAT-06). Empty = trust none.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxies {
    cidrs: Vec<Cidr>,
}

impl TrustedProxies {
    /// Parse a list of CIDR strings, collecting every parse error so the
    /// caller (startup validation) can reject a misconfiguration up front.
    pub fn parse(entries: &[String]) -> Result<Self, String> {
        let mut cidrs = Vec::with_capacity(entries.len());
        for e in entries {
            cidrs.push(Cidr::parse(e)?);
        }
        Ok(Self { cidrs })
    }

    /// Parse, silently dropping (and logging) invalid entries. Used on the
    /// hot construction path where startup validation has already run.
    pub fn parse_lossy(entries: &[String]) -> Self {
        let cidrs = entries
            .iter()
            .filter_map(|e| match Cidr::parse(e) {
                Ok(c) => Some(c),
                Err(err) => {
                    tracing::warn!(entry = %e, error = %err, "ignoring invalid trusted_proxy CIDR");
                    None
                }
            })
            .collect();
        Self { cidrs }
    }

    pub fn is_empty(&self) -> bool {
        self.cidrs.is_empty()
    }

    fn contains(&self, ip: IpAddr) -> bool {
        self.cidrs.iter().any(|c| c.contains(ip))
    }
}

/// Resolve the effective client IP for rate-limiting (FEAT-06).
///
/// `peer` is the TCP socket peer address (already IPv4-canonicalised by the
/// caller). `xff` is the raw `X-Forwarded-For` header value, if present.
///
/// SECURITY: `X-Forwarded-For` is caller-controlled and is honoured ONLY when
/// `peer` is itself a trusted proxy. In that case the real client is the
/// rightmost XFF entry that is not itself a trusted proxy — i.e. we walk the
/// forwarded chain from the right (nearest hop first), skipping trusted
/// proxies, and take the first address a trusted proxy actually received the
/// request from. If every XFF entry is a trusted proxy (or XFF is
/// absent/garbage), we fall back to `peer`. When `trusted` is empty, XFF is
/// never consulted.
pub fn client_ip(peer: IpAddr, xff: Option<&str>, trusted: &TrustedProxies) -> IpAddr {
    if trusted.is_empty() || !trusted.contains(peer) {
        return peer;
    }
    let Some(xff) = xff else {
        return peer;
    };
    // Right-to-left: the last entry is the address the trusted peer saw.
    for hop in xff.rsplit(',') {
        let hop = hop.trim();
        // XFF entries may carry a port (rare) — strip anything after the IP.
        let candidate = hop.parse::<IpAddr>().ok().map(canonical_ip).or_else(|| {
            hop.parse::<std::net::SocketAddr>()
                .ok()
                .map(|s| canonical_ip(s.ip()))
        });
        match candidate {
            Some(ip) if trusted.contains(ip) => continue, // another trusted hop
            Some(ip) => return ip,                        // first untrusted → the client
            None => return peer,                          // garbage → don't trust the chain
        }
    }
    peer
}

/// Canonicalise an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) to its IPv4
/// form so CIDR matching and bucket keys are stable regardless of the
/// listener's dual-stack representation.
pub fn canonical_ip(ip: IpAddr) -> IpAddr {
    ip.to_canonical()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- A1: peek / record ----------------------------------------------
    //
    // The security property is that a caller who never earns a charge leaves NO
    // TRACE. Each test below pins one half of that, and each is falsifiable on
    // its own -- asserting only "peek doesn't reject" would pass against an
    // implementation that inserted a bucket, which is the actual defect.

    #[test]
    fn peek_does_not_create_a_bucket() {
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        for i in 0..1000 {
            assert!(rl.peek_at(&format!("spoofed-{i}"), t0).is_ok());
        }
        // THE security assertion. `check` would have left 1000 entries keyed by
        // attacker-supplied strings; that unbounded, attacker-keyed growth is
        // the half of A1 that a "does it reject?" test cannot see.
        assert_eq!(
            rl.buckets.lock().unwrap().len(),
            0,
            "peek must not insert: the map key is attacker-controlled",
        );
    }

    #[test]
    fn peek_does_not_charge() {
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        // Peeking is free no matter how often; only `record` spends.
        for _ in 0..50 {
            assert!(rl.peek_at("agent-a", t0).is_ok());
        }
        // The single unit of budget is still there to be spent.
        rl.record_at("agent-a", t0);
        assert!(
            rl.peek_at("agent-a", t0).is_err(),
            "one record against a limit of 1 must exhaust the budget",
        );
    }

    #[test]
    fn peek_sees_another_agents_spent_budget() {
        // Spoofing protection is about WRITES, not reads: peek must still
        // report a genuinely exhausted budget, or the pre-pipeline rejection
        // stops working entirely.
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        rl.record_at("agent-a", t0);
        assert!(rl.peek_at("agent-a", t0).is_err());
        assert!(rl.peek_at("agent-b", t0).is_ok());
    }

    #[test]
    fn record_rolls_the_window_over_rather_than_accumulating() {
        // `record` was described in an early draft as "increment only, never
        // rejects". That draft predicted such an implementation would
        // accumulate and lock an agent out PERMANENTLY -- fail closed.
        //
        // Against this `peek` it does the opposite, and that is worse. `peek`
        // reads an expired window as count 0 on its own, so if `record` never
        // advances `window_start`, every later `peek` sees a window that expired
        // long ago and returns Ok forever. The limiter silently STOPS LIMITING
        // after the first window -- fail open.
        //
        // The first version of this test asserted only that budget was available
        // again after the window turned. That is true under both the correct and
        // the broken implementation, so it caught nothing: the mutation ran green.
        // The assertions below are the ones that separate them.
        let rl = AgentRateLimiter::new(2);
        let t0 = Instant::now();
        rl.record_at("agent-a", t0);
        rl.record_at("agent-a", t0);
        assert!(rl.peek_at("agent-a", t0).is_err(), "budget spent in-window");

        // Spend the FULL budget again inside the NEW window.
        let later = t0 + WINDOW + Duration::from_secs(1);
        rl.record_at("agent-a", later);
        rl.record_at("agent-a", later);
        assert!(
            rl.peek_at("agent-a", later).is_err(),
            "the new window's budget must be enforceable. Under an increment-only \
             `record`, window_start stays at t0, every peek reads the window as \
             expired, and the limiter never rejects again",
        );

        // Directly, so the mechanism is pinned and not merely its symptom.
        let map = rl.buckets.lock().unwrap();
        let b = map.get("agent-a").expect("record created it");
        assert_eq!(
            b.window_start, later,
            "record must ADVANCE window_start into the new window",
        );
        assert_eq!(
            b.count, 2,
            "count must restart from 0 in the new window; an increment-only \
             record leaves 4 here",
        );
    }

    #[test]
    fn peek_does_not_roll_the_window_over() {
        // An expired window reads as count 0, but peek must not WRITE that
        // rollover -- otherwise an unverified caller mutates state again, just
        // more subtly than by inserting.
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        rl.record_at("agent-a", t0);
        let later = t0 + WINDOW + Duration::from_secs(1);
        assert!(rl.peek_at("agent-a", later).is_ok());
        // The stored bucket must be untouched: same window_start, same count.
        let map = rl.buckets.lock().unwrap();
        let b = map.get("agent-a").expect("record created it");
        assert_eq!(b.count, 1, "peek must not reset a stored count");
        assert_eq!(
            b.window_start, t0,
            "peek must not advance a stored window_start",
        );
    }

    #[test]
    fn peek_retry_after_matches_check() {
        // The 429's `Retry-After` is part of the wire contract and must not
        // change just because the read moved out of `check`.
        let a = AgentRateLimiter::new(1);
        let b = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        a.record_at("agent-a", t0);
        assert!(b.check_at("agent-a", t0).is_ok());
        let mid = t0 + Duration::from_secs(20);
        assert_eq!(
            a.peek_at("agent-a", mid).unwrap_err(),
            b.check_at("agent-a", mid).unwrap_err(),
            "peek must compute Retry-After exactly as check does",
        );
    }

    #[test]
    fn allows_up_to_limit_then_rejects() {
        let rl = AgentRateLimiter::new(3);
        let t0 = Instant::now();
        assert!(rl.check_at("agent-a", t0).is_ok());
        assert!(rl.check_at("agent-a", t0).is_ok());
        assert!(rl.check_at("agent-a", t0).is_ok());
        let retry = rl
            .check_at("agent-a", t0)
            .expect_err("4th should be limited");
        assert!(
            (1..=60).contains(&retry),
            "retry-after out of range: {retry}"
        );
    }

    #[test]
    fn separate_agents_have_separate_budgets() {
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        assert!(rl.check_at("agent-a", t0).is_ok());
        assert!(rl.check_at("agent-a", t0).is_err());
        // A different agent is unaffected by agent-a's exhausted budget.
        assert!(rl.check_at("agent-b", t0).is_ok());
    }

    #[test]
    fn global_ceiling_caps_total_across_distinct_keys() {
        // #24: per-agent budget is generous, but the global ceiling caps total
        // flooding even when the attacker rotates agent_id every request.
        let rl = AgentRateLimiter::with_global_ceiling(1_000, 2);
        let t0 = Instant::now();
        assert!(rl.check_global_at(t0).is_ok());
        assert!(rl.check_global_at(t0).is_ok());
        assert!(
            rl.check_global_at(t0).is_err(),
            "global ceiling must reject once exhausted regardless of key"
        );
        // Refreshes next window.
        let later = t0 + Duration::from_secs(61);
        assert!(rl.check_global_at(later).is_ok());
    }

    #[test]
    fn no_global_ceiling_by_default() {
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        for _ in 0..10_000 {
            assert!(
                rl.check_global_at(t0).is_ok(),
                "new() must not impose a global ceiling"
            );
        }
    }

    #[test]
    fn window_resets_after_60s() {
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        assert!(rl.check_at("agent-a", t0).is_ok());
        assert!(rl.check_at("agent-a", t0).is_err());
        let later = t0 + Duration::from_secs(61);
        assert!(
            rl.check_at("agent-a", later).is_ok(),
            "budget should refresh in the next window"
        );
    }

    #[test]
    fn window_boundary_is_inclusive_at_exactly_60s() {
        // The reset is `>= WINDOW`, so the budget refreshes at exactly 60.0s
        // but NOT a tick earlier (59.999s is still the same window).
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        assert!(rl.check_at("a", t0).is_ok());
        assert!(rl.check_at("a", t0).is_err());
        // 59.999s in — still throttled.
        assert!(rl
            .check_at("a", t0 + Duration::from_millis(59_999))
            .is_err());
        // Exactly 60s — new window.
        assert!(rl.check_at("a", t0 + Duration::from_secs(60)).is_ok());
    }

    #[test]
    fn retry_after_never_reports_zero() {
        // Near the very end of a window the remaining seconds floor to 0;
        // the limiter clamps to 1 so a client never sees `Retry-After: 0`.
        let rl = AgentRateLimiter::new(1);
        let t0 = Instant::now();
        assert!(rl.check_at("a", t0).is_ok());
        let retry = rl
            .check_at("a", t0 + Duration::from_millis(59_500))
            .expect_err("still throttled");
        assert_eq!(retry, 1, "sub-second remainder must clamp to 1s");
    }

    /// H-P item 1 — the `Retry-After` **value**, not merely its range.
    ///
    /// **What already existed, and why none of it pinned the number.**
    ///
    /// - `peek_retry_after_matches_check` pins `peek == check`. That is internal
    ///   consistency between two paths, and it is silent if both share one wrong
    ///   arithmetic — which they do: `WINDOW.saturating_sub(elapsed)…max(1)` is
    ///   written out three separate times, in `check_global_at`, `peek_at` and
    ///   `check_at`.
    /// - `allows_up_to_limit_then_rejects` asserts `(1..=60).contains(&retry)`,
    ///   and the HTTP integration test asserts the same range on the real
    ///   header. A range that spans every value the function can return rules
    ///   out nothing.
    /// - `retry_after_never_reports_zero` *does* assert an exact value — but at
    ///   59.5s in, where the expected answer is the clamp floor. A wrong
    ///   mechanism reaches the right number there by accident, so the one exact
    ///   assertion in the module is the one least able to detect a wrong
    ///   mechanism.
    ///
    /// **Measured before writing this.** Replacing `WINDOW.saturating_sub(elapsed)`
    /// with `elapsed.saturating_sub(WINDOW)` at all three sites pins every
    /// `Retry-After` to exactly 1 second, and leaves all 24 tests in this module
    /// green plus the integration assertion. A client throttled for a full
    /// minute is told to come back in one second, so it retries roughly 60×
    /// more often than intended — under precisely the load the limiter exists
    /// to shed. This test fails on that mutation at the first sample.
    ///
    /// Varies `elapsed` (through the explicit `now` argument, so no wall-clock
    /// dependence) and asserts the remaining seconds each time, which is the
    /// value the branch actually computes.
    #[test]
    fn retry_after_reports_the_seconds_actually_remaining() {
        // Derived from the wire contract — a 60-second window — and
        // deliberately NOT from `WINDOW`. Computing the expectation from the
        // same constant the code reads would let a change to it move both
        // sides together and assert nothing.
        const CONTRACT_WINDOW_SECS: u64 = 60;
        assert_eq!(
            WINDOW.as_secs(),
            CONTRACT_WINDOW_SECS,
            "the documented window changed; Retry-After is a wire contract, so \
             update the docs and the expectations below deliberately"
        );

        // (elapsed into the window, seconds a client should be told to wait).
        // Truncating division is intended: 14.5s remaining reports 14.
        let samples: [(u64, u64); 5] = [
            (0, 60),
            (1_000, 59),
            (20_000, 40),
            (45_500, 14),
            // The clamp, kept here so this test covers the floor too — but it
            // is the four rows above that make the test non-vacuous.
            (59_500, 1),
        ];

        for (elapsed_ms, expected) in samples {
            let at = Duration::from_millis(elapsed_ms);

            // Path 1: `check_at` — the write path a real request takes.
            let rl = AgentRateLimiter::new(1);
            let t0 = Instant::now();
            assert!(rl.check_at("a", t0).is_ok(), "first call is within budget");
            let got = rl
                .check_at("a", t0 + at)
                .expect_err("second call is over a limit of 1");
            assert_eq!(
                got, expected,
                "check_at: {elapsed_ms}ms into a {CONTRACT_WINDOW_SECS}s window \
                 a client must be told to wait {expected}s, got {got}s"
            );

            // Path 2: `peek_at` — the read-only path, reached through
            // `record_at` so the bucket exists without `check_at` having run.
            let rl = AgentRateLimiter::new(1);
            let t0 = Instant::now();
            rl.record_at("a", t0);
            let got = rl
                .peek_at("a", t0 + at)
                .expect_err("recorded count is at the limit of 1");
            assert_eq!(
                got, expected,
                "peek_at: {elapsed_ms}ms in must report {expected}s, got {got}s"
            );
        }

        // Path 3: the process-global ceiling. Asserted with a tolerance rather
        // than exactly, because `with_global_ceiling` stamps the global
        // bucket's `window_start` from `Instant::now()` at construction instead
        // of accepting it — so the elapsed this path measures is mine plus an
        // uncontrolled construction delta. A tolerance is honest here; an exact
        // assertion would be a flake waiting for a loaded machine. It still
        // excludes the always-clamp mutation, which is the point.
        let rl = AgentRateLimiter::with_global_ceiling(1_000, 1);
        let t0 = Instant::now();
        assert!(
            rl.check_global_at(t0).is_ok(),
            "first call is within budget"
        );
        let got = rl
            .check_global_at(t0 + Duration::from_secs(20))
            .expect_err("second call is over a global limit of 1");
        assert!(
            (38..=40).contains(&got),
            "check_global_at: 20s into a {CONTRACT_WINDOW_SECS}s window a client \
             must be told to wait ~40s, got {got}s — a value of 1 means the \
             remaining-time arithmetic collapsed to the clamp floor"
        );
    }

    #[test]
    fn prune_evicts_only_stale_buckets() {
        let rl = AgentRateLimiter::new(10);
        let t0 = Instant::now();
        // Fill the map to the prune threshold with buckets in the current window.
        for i in 0..PRUNE_AT {
            assert!(rl.check_at(&format!("agent-{i}"), t0).is_ok());
        }
        // A new key one full window later trips the opportunistic prune; every
        // existing bucket is now stale and must be evicted, leaving just the
        // freshly-inserted key.
        let later = t0 + Duration::from_secs(61);
        assert!(rl.check_at("newcomer", later).is_ok());
        let len = rl.buckets.lock().unwrap().len();
        assert_eq!(len, 1, "stale buckets must be pruned, got {len} entries");
    }

    // ── CIDR + client-IP resolution (FEAT-06) ───────────────────────

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn cidr_matches_v4_and_v6_ranges() {
        let c = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(c.contains(ip("10.1.2.3")));
        assert!(c.contains(ip("10.255.255.255")));
        assert!(!c.contains(ip("11.0.0.1")));
        assert!(!c.contains(ip("192.168.1.1")));

        let c = Cidr::parse("192.168.1.0/24").unwrap();
        assert!(c.contains(ip("192.168.1.42")));
        assert!(!c.contains(ip("192.168.2.42")));

        let c = Cidr::parse("fc00::/7").unwrap();
        assert!(c.contains(ip("fd00::1")));
        assert!(!c.contains(ip("fe80::1")));

        // bare host route
        let c = Cidr::parse("203.0.113.7").unwrap();
        assert!(c.contains(ip("203.0.113.7")));
        assert!(!c.contains(ip("203.0.113.8")));
    }

    #[test]
    fn cidr_rejects_garbage_and_out_of_range() {
        assert!(Cidr::parse("not-an-ip/8").is_err());
        assert!(Cidr::parse("10.0.0.0/33").is_err());
        assert!(Cidr::parse("::/129").is_err());
        assert!(Cidr::parse("10.0.0.0/x").is_err());
    }

    #[test]
    fn client_ip_ignores_xff_when_no_trusted_proxies() {
        // Empty trust set: XFF is never consulted, socket peer wins.
        let trusted = TrustedProxies::default();
        let got = client_ip(ip("203.0.113.9"), Some("1.1.1.1"), &trusted);
        assert_eq!(got, ip("203.0.113.9"));
    }

    #[test]
    fn client_ip_ignores_xff_from_untrusted_peer() {
        // Peer is NOT a trusted proxy, so its XFF is a spoof attempt — ignore.
        let trusted = TrustedProxies::parse(&["10.0.0.0/8".into()]).unwrap();
        let got = client_ip(ip("203.0.113.9"), Some("1.1.1.1"), &trusted);
        assert_eq!(got, ip("203.0.113.9"));
    }

    #[test]
    fn client_ip_honors_xff_from_trusted_peer() {
        let trusted = TrustedProxies::parse(&["10.0.0.0/8".into()]).unwrap();
        // Trusted proxy at 10.0.0.5 forwarded a request from the real client.
        let got = client_ip(ip("10.0.0.5"), Some("198.51.100.7"), &trusted);
        assert_eq!(got, ip("198.51.100.7"));
    }

    #[test]
    fn client_ip_walks_chain_of_trusted_proxies() {
        // client → proxy(10.0.0.9) → proxy(10.0.0.5=peer). XFF lists the
        // client then the first proxy; we skip the trusted hop and return
        // the client.
        let trusted = TrustedProxies::parse(&["10.0.0.0/8".into()]).unwrap();
        let got = client_ip(ip("10.0.0.5"), Some("198.51.100.7, 10.0.0.9"), &trusted);
        assert_eq!(got, ip("198.51.100.7"));
    }

    #[test]
    fn client_ip_falls_back_when_all_hops_trusted() {
        let trusted = TrustedProxies::parse(&["10.0.0.0/8".into()]).unwrap();
        let got = client_ip(ip("10.0.0.5"), Some("10.0.0.9, 10.0.0.8"), &trusted);
        assert_eq!(got, ip("10.0.0.5"));
    }

    #[test]
    fn client_ip_falls_back_on_garbage_xff() {
        let trusted = TrustedProxies::parse(&["10.0.0.0/8".into()]).unwrap();
        let got = client_ip(ip("10.0.0.5"), Some("garbage"), &trusted);
        assert_eq!(got, ip("10.0.0.5"));
    }

    #[test]
    fn client_ip_canonicalises_v4_mapped_peer_against_v4_cidr() {
        // A dual-stack listener may report the peer as ::ffff:10.0.0.5; the
        // caller canonicalises before calling, so simulate that here.
        let trusted = TrustedProxies::parse(&["10.0.0.0/8".into()]).unwrap();
        let peer = canonical_ip(ip("::ffff:10.0.0.5"));
        assert_eq!(peer, ip("10.0.0.5"));
        let got = client_ip(peer, Some("198.51.100.7"), &trusted);
        assert_eq!(got, ip("198.51.100.7"));
    }

    #[test]
    fn global_and_per_agent_counters_are_independent() {
        // Exhausting the per-agent budget must not consume the global counter,
        // and vice-versa — the publish path checks them separately.
        let rl = AgentRateLimiter::with_global_ceiling(1, 1);
        let t0 = Instant::now();
        // Use up the per-agent budget for "a".
        assert!(rl.check_at("a", t0).is_ok());
        assert!(rl.check_at("a", t0).is_err());
        // The global counter is untouched — its first call still succeeds.
        assert!(rl.check_global_at(t0).is_ok());
        assert!(rl.check_global_at(t0).is_err());
        // And a different agent's per-agent budget is likewise untouched.
        assert!(rl.check_at("b", t0).is_ok());
    }
}
