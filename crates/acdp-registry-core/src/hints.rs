//! RFC-ACDP-0018 §8.2–§8.3 change-hint delivery.
//!
//! Delivery state lives on the subscription row, so there is no per-change
//! queue. A pass:
//!
//! 1. claims due subscriptions: active, `next_attempt_at_ms` reached, not
//!    leased, and with feed entries past `last_hinted_seq`. The lease is
//!    taken and committed before any network call, so no transaction is
//!    held across HTTP, and two replicas never claim the same row;
//! 2. evaluates each one's filter as its owner, through the same function
//!    `GET /changes` uses, so a change the owner cannot see never causes
//!    a hint (rec-007 H). With nothing visible, the row's position just
//!    advances;
//! 3. otherwise POSTs one signed, content-free `acdp-change-hint/1`
//!    covering everything up to the head it observed (coalescing);
//! 4. finishes the attempt with a write conditional on its lease: a
//!    subscription deleted or re-claimed meanwhile is left alone.
//!
//! A delivered hint holds the subscription back for [`HINT_MIN_INTERVAL_MS`]
//! so a busy feed sends at most about one hint a second per subscription;
//! the changes in between coalesce into the next one.
//!
//! A failed delivery (non-2xx, a redirect, a timeout, an SSRF refusal)
//! backs off from one minute, doubling to an hourly cap, so retries run
//! well past the hour §8.2 asks for. A subscription failing for
//! [`DISABLE_AFTER_MS`] (72 h) is disabled and `GET` reports it. A secret
//! sealed under a `[changes]` key id this replica does not hold (a
//! rotation, a rollback, one replica on the wrong key) is retried on the
//! same schedule, so restoring the key within 72 h loses nothing; a secret
//! that fails to open under the right key is corrupt and disables at once.
//!
//! Every delivery re-resolves the callback host and refuses the whole
//! answer if any address is outside public unicast (§8.3, DNS rebinding),
//! follows no redirect, bounds connect at 5 s and the whole request at
//! 10 s, and never reads the response body.
//!
//! The clock is read per claim batch (for the lease) and per delivery
//! (for the signature's `t` and the retry schedule), so a long pass
//! neither claims with a lease that has already lapsed nor signs a stale
//! timestamp. Every attempt is bounded by [`ATTEMPT_TIMEOUT`] and a batch
//! by its ceil(`CLAIM_BATCH` / in-flight) rounds of it, which stays under
//! [`LEASE_MS`]: a live worker finishes before its lease lapses.
//!
//! The worker wakes on [`AppStateInner::notify_hints`](crate::state::AppStateInner::notify_hints),
//! called after every commit that may append to the feed, and on a
//! [`POLL_INTERVAL`] timer for retries. On shutdown it stops claiming and
//! lets the batch in flight finish; an attempt cut off anyway only leaves
//! a lease, which lapses after [`LEASE_MS`] and the subscription is tried
//! again.

use std::sync::Arc;
use std::time::Duration;

use acdp::error::AcdpError;
use acdp::types::change::{sign_hint, ChangeHint, SubscriptionStatus};
use acdp_registry_store::{ExtendedRegistryStore, SubscriptionProgress, SubscriptionRecord};
use acdp_registry_types::RegistryError;
use zeroize::Zeroize as _;

use crate::callback::resolve_allowed;
use crate::changes_key::{ChangesKey, SealError};
use crate::handlers::{evaluate_changes, ChangesRequest, Since};
use crate::state::AppState;

/// The header carrying the hint's HMAC (§8.2).
pub const SIGNATURE_HEADER: &str = "ACDP-Signature";
/// Subscriptions claimed per store round trip.
pub const CLAIM_BATCH: u32 = 32;
/// Claim batches one pass works through before yielding to the next tick.
const MAX_BATCHES_PER_PASS: usize = 64;
/// How long a claim is held: longer than a whole batch can take (see
/// [`ATTEMPT_TIMEOUT`]), so a live worker never loses its own lease.
pub const LEASE_MS: i64 = 60_000;
/// Bound on one subscription's attempt, queueing excluded: DNS
/// (`callback::LOOKUP_TIMEOUT`, 5 s) + request ([`REQUEST_TIMEOUT`],
/// 10 s) + evaluation and store writes. An attempt that overruns is
/// abandoned and its lease left to lapse.
pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);
/// First retry delay after a failure.
pub const RETRY_BASE_MS: i64 = 60_000;
/// Retry delay ceiling: past it, a failing subscription is tried hourly.
pub const RETRY_CAP_MS: i64 = 3_600_000;
/// §8.2: a registry MAY disable a subscription that has failed for 72 h.
pub const DISABLE_AFTER_MS: i64 = 72 * 3_600_000;
/// TCP + TLS connect bound (§8.3: at most 10 s).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Whole-request bound (§8.3: at most 10 s).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The bounds one delivery runs under. Production uses the constants
/// ([`callback::LOOKUP_TIMEOUT`](crate::callback::LOOKUP_TIMEOUT),
/// [`CONNECT_TIMEOUT`], [`REQUEST_TIMEOUT`]); a test may shorten them with
/// [`AppStateInner::with_hint_timeouts`](crate::state::AppStateInner::with_hint_timeouts)
/// so a timeout path costs milliseconds, never lengthen them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintTimeouts {
    /// One callback-host DNS lookup, at subscription and at delivery.
    pub lookup: Duration,
    /// TCP + TLS connect.
    pub connect: Duration,
    /// The whole request.
    pub request: Duration,
}

impl Default for HintTimeouts {
    fn default() -> Self {
        Self {
            lookup: crate::callback::LOOKUP_TIMEOUT,
            connect: CONNECT_TIMEOUT,
            request: REQUEST_TIMEOUT,
        }
    }
}

impl HintTimeouts {
    /// Each bound the smaller of `self`'s and the production one's.
    #[must_use]
    pub fn capped(self) -> Self {
        let d = Self::default();
        Self {
            lookup: self.lookup.min(d.lookup),
            connect: self.connect.min(d.connect),
            request: self.request.min(d.request),
        }
    }
}
/// Deliveries in flight at once per replica.
const MAX_IN_FLIGHT: usize = 16;

// A batch runs in ceil(CLAIM_BATCH / MAX_IN_FLIGHT) rounds of at most
// ATTEMPT_TIMEOUT each; all of it must fit inside the lease.
const _: () = assert!(
    ((CLAIM_BATCH as usize).div_ceil(MAX_IN_FLIGHT) as u64) * (ATTEMPT_TIMEOUT.as_millis() as u64)
        < (LEASE_MS as u64)
);
/// After a delivered hint, the subscription is not due again for this long.
pub const HINT_MIN_INTERVAL_MS: i64 = 1_000;
/// The worker's timer between wake-ups (retries become due on it).
pub const POLL_INTERVAL: Duration = Duration::from_secs(15);

/// The delay before retry number `attempts` (1-based): 1 min, doubling,
/// capped at an hour.
pub fn retry_delay_ms(attempts: u32) -> i64 {
    let shift = attempts.saturating_sub(1).min(16);
    (RETRY_BASE_MS << shift).min(RETRY_CAP_MS)
}

/// The outcome of one delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The receiver answered 2xx.
    Delivered,
    /// Anything else: the reason, for the log line.
    Failed(String),
    /// The callback host resolved to a forbidden address, or the URL no
    /// longer passes the policy (§8.3). Retried like a failure.
    Refused(String),
}

/// What one subscription's turn in a pass came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintOutcome {
    /// A hint was accepted (2xx).
    Delivered,
    /// Nothing visible to the owner past its position; it advanced.
    NothingVisible,
    /// The delivery failed and is scheduled for retry.
    Failed,
    /// The SSRF policy refused the callback; scheduled for retry.
    SsrfRefused,
    /// The secret is sealed under a `[changes]` key id this replica does
    /// not hold; nothing sent, scheduled for retry.
    UnknownKey,
    /// The delivery failed and the subscription has now failed for 72 h,
    /// or it can never be delivered (its secret is corrupt, its stored
    /// owner is unreadable, or it has no tenant while
    /// `auth.require_tenant` is set); it is disabled.
    Disabled,
    /// The lease was lost (deleted or re-claimed meanwhile); nothing was
    /// written.
    LeaseLost,
    /// A store or evaluation error, or the attempt overran
    /// [`ATTEMPT_TIMEOUT`]; nothing was written and the lease is left to
    /// lapse, after which the subscription is tried again.
    Error,
}

impl HintOutcome {
    /// The `outcome` label on the deliveries counter.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::NothingVisible => "nothing_visible",
            Self::Failed => "failed",
            Self::SsrfRefused => "ssrf_refused",
            Self::UnknownKey => "unknown_key",
            Self::Disabled => "disabled",
            Self::LeaseLost => "lease_lost",
            Self::Error => "error",
        }
    }
}

/// Totals for one pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PassStats {
    pub claimed: usize,
    pub delivered: usize,
    pub nothing_visible: usize,
    pub failed: usize,
    pub ssrf_refused: usize,
    pub unknown_key: usize,
    pub disabled: usize,
    pub lease_lost: usize,
    pub errors: usize,
}

impl PassStats {
    fn add(&mut self, o: HintOutcome) {
        self.claimed += 1;
        match o {
            HintOutcome::Delivered => self.delivered += 1,
            HintOutcome::NothingVisible => self.nothing_visible += 1,
            HintOutcome::Failed => self.failed += 1,
            HintOutcome::SsrfRefused => self.ssrf_refused += 1,
            HintOutcome::UnknownKey => self.unknown_key += 1,
            HintOutcome::Disabled => self.disabled += 1,
            HintOutcome::LeaseLost => self.lease_lost += 1,
            HintOutcome::Error => self.errors += 1,
        }
    }
}

/// The worker's clock, in Unix milliseconds. Read once per claim batch
/// and per delivery, never once per pass.
pub type HintClock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The wall clock.
pub fn system_clock() -> HintClock {
    Arc::new(|| chrono::Utc::now().timestamp_millis())
}

/// Run one delivery pass with the clock frozen at `now_ms`. Tests drive
/// this directly; [`spawn_hint_worker`] uses [`run_hint_pass_with`].
pub async fn run_hint_pass<S: ExtendedRegistryStore + 'static>(
    state: &Arc<AppState<S>>,
    now_ms: i64,
) -> Result<PassStats, RegistryError> {
    run_hint_pass_with(state, &(Arc::new(move || now_ms) as HintClock), None).await
}

/// Run one delivery pass on `clock`. When `stop` turns true no further
/// batch is claimed; the batch in flight finishes.
pub async fn run_hint_pass_with<S: ExtendedRegistryStore + 'static>(
    state: &Arc<AppState<S>>,
    clock: &HintClock,
    stop: Option<&tokio::sync::watch::Receiver<bool>>,
) -> Result<PassStats, RegistryError> {
    let mut stats = PassStats::default();
    if state.events_key().is_none() {
        return Ok(stats);
    }
    let store = state.server.store();
    let gate = Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT));
    for _ in 0..MAX_BATCHES_PER_PASS {
        if stop.is_some_and(|s| *s.borrow()) {
            break;
        }
        let head = store.changes_head().await?.head;
        let now_ms = clock();
        let claimed = store
            .claim_due_subscriptions(now_ms, head, now_ms + LEASE_MS, CLAIM_BATCH)
            .await?;
        let n = claimed.len();
        let mut tasks = tokio::task::JoinSet::new();
        for sub in claimed {
            let state = Arc::clone(state);
            let gate = Arc::clone(&gate);
            let clock = Arc::clone(clock);
            tasks.spawn(async move {
                let _permit = gate.acquire_owned().await;
                let id = sub.subscription_id.clone();
                let Some(key) = state.events_key() else {
                    return HintOutcome::Error;
                };
                let attempt = hint_one(&state, key, sub, &clock);
                match tokio::time::timeout(ATTEMPT_TIMEOUT, attempt).await {
                    Ok(Ok(o)) => o,
                    Ok(Err(e)) => {
                        // Nothing written: the lease lapses and the
                        // subscription is tried again after it.
                        tracing::warn!(subscription_id = %id, error = %e, "change hint attempt failed");
                        HintOutcome::Error
                    }
                    Err(_) => {
                        tracing::warn!(subscription_id = %id, "change hint attempt overran its bound; abandoned");
                        HintOutcome::Error
                    }
                }
            });
        }
        while let Some(done) = tasks.join_next().await {
            let o = done.unwrap_or(HintOutcome::Error);
            crate::metrics::record_hint_delivery(o.as_str());
            stats.add(o);
        }
        if n < CLAIM_BATCH as usize {
            break;
        }
    }
    Ok(stats)
}

/// §8.2's MAY: disable once failing for [`DISABLE_AFTER_MS`].
pub fn failed_long_enough(failing_since_ms: i64, now_ms: i64) -> bool {
    now_ms - failing_since_ms >= DISABLE_AFTER_MS
}

/// One claimed subscription's turn.
async fn hint_one<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    key: &ChangesKey,
    sub: SubscriptionRecord,
    clock: &HintClock,
) -> Result<HintOutcome, RegistryError> {
    let store = state.server.store();
    let now_ms = clock();
    let lease = sub.lease_until_ms.unwrap_or(now_ms + LEASE_MS);
    let sub_id = sub.subscription_id.clone();
    let finish = |p: SubscriptionProgress, o: HintOutcome| async move {
        let kept = store
            .finish_subscription_attempt(&sub_id, lease, &p)
            .await?;
        Ok::<_, RegistryError>(if kept { o } else { HintOutcome::LeaseLost })
    };

    let owner = match acdp::types::primitives::AgentDid::parse(sub.owner_did.clone()) {
        Ok(o) => o,
        Err(e) => {
            // Corrupt row: no retry can ever evaluate it as its owner.
            tracing::warn!(
                subscription_id = %sub.subscription_id,
                error = %e,
                "stored owner DID is unreadable; disabling the subscription"
            );
            let p = SubscriptionProgress::disabled(&sub, now_ms);
            return finish(p, HintOutcome::Disabled).await;
        }
    };
    if state.config.auth.require_tenant && sub.tenant_id.is_none() {
        // Created before tenants were required: its owner's own
        // `GET /changes` is now refused, so evaluating it with no tenant
        // clause would see across every tenant.
        tracing::warn!(
            subscription_id = %sub.subscription_id,
            "subscription has no tenant and auth.require_tenant is set; disabling it"
        );
        let p = SubscriptionProgress::disabled(&sub, now_ms);
        return finish(p, HintOutcome::Disabled).await;
    }
    let mut req = ChangesRequest {
        since: Since::Position(sub.last_hinted_seq),
        limit: 1,
        filter: sub.filter.clone(),
    };
    let head = store.changes_head().await?.head;
    let tenant = sub.tenant_id.as_deref();
    let resp = match evaluate_changes(state, key, Some(&owner), tenant, &req).await {
        Ok((resp, _)) => resp,
        // Its position was pruned (the worker was down past retention):
        // look again from the oldest retained entry, so a hint still
        // only ever follows a change the owner may see.
        Err(RegistryError::Acdp(AcdpError::CursorExpired)) => {
            req.since = Since::Oldest;
            evaluate_changes(state, key, Some(&owner), tenant, &req)
                .await?
                .0
        }
        Err(e) => return Err(e),
    };
    if resp.changes.is_empty() {
        // Nothing the owner may see: advance to where the scan stopped.
        let pos = key.decode_cursor(&resp.next_cursor).map_err(|_| {
            RegistryError::Internal("hint worker: its own cursor does not decode".into())
        })?;
        let p = SubscriptionProgress::healthy(pos, now_ms);
        return finish(p, HintOutcome::NothingVisible).await;
    }

    let mut secret = match key.open_secret(&sub.subscription_id, sub.secret_key_id, &sub.secret_ct)
    {
        Ok(s) => s,
        Err(SealError::UnknownKey) => {
            // Rotated, rolled back, or this replica holds the wrong key:
            // retry, so restoring the key within 72 h loses nothing.
            let p = retry_or_disable(&sub, now_ms, "secret sealed under an unknown changes key");
            let o = if p.status == SubscriptionStatus::Disabled {
                HintOutcome::Disabled
            } else {
                HintOutcome::UnknownKey
            };
            return finish(p, o).await;
        }
        Err(e) => {
            // Tampered or truncated under the right key: never signable.
            tracing::warn!(
                subscription_id = %sub.subscription_id,
                error = %e,
                "change hint secret is corrupt; disabling the subscription"
            );
            let p = SubscriptionProgress::disabled(&sub, now_ms);
            return finish(p, HintOutcome::Disabled).await;
        }
    };
    let hint = ChangeHint::new(
        sub.subscription_id.clone(),
        hint_authority(&state.config.registry.authority),
        key.encode_cursor(head),
    );
    let body = serde_json::to_vec(&hint)
        .map_err(|e| RegistryError::Internal(format!("change hint: {e}")))?;
    let signature = sign_hint(&secret, clock().div_euclid(1000), &body);
    secret.zeroize();

    let delivery = deliver(state, &sub.callback_url, body, &signature).await;
    let now_ms = clock();
    let (why, outcome) = match delivery {
        Delivery::Delivered => {
            let p = SubscriptionProgress::healthy(head, now_ms + HINT_MIN_INTERVAL_MS);
            return finish(p, HintOutcome::Delivered).await;
        }
        Delivery::Failed(why) => (why, HintOutcome::Failed),
        Delivery::Refused(why) => (why, HintOutcome::SsrfRefused),
    };
    let p = retry_or_disable(&sub, now_ms, &why);
    let o = if p.status == SubscriptionStatus::Disabled {
        HintOutcome::Disabled
    } else {
        outcome
    };
    finish(p, o).await
}

/// A failed attempt's progress: backed off, or disabled once failing for
/// [`DISABLE_AFTER_MS`].
fn retry_or_disable(sub: &SubscriptionRecord, now_ms: i64, why: &str) -> SubscriptionProgress {
    let failing_since = sub.failing_since_ms.unwrap_or(now_ms);
    let attempts = sub.attempts.saturating_add(1);
    if failed_long_enough(failing_since, now_ms) {
        tracing::warn!(
            subscription_id = %sub.subscription_id,
            reason = %why,
            "change hints have failed for 72 hours; disabling the subscription"
        );
        return SubscriptionProgress::disabled(sub, now_ms);
    }
    tracing::info!(
        subscription_id = %sub.subscription_id,
        attempts,
        reason = %why,
        "change hint attempt failed; will retry"
    );
    SubscriptionProgress {
        last_hinted_seq: sub.last_hinted_seq,
        status: SubscriptionStatus::Active,
        next_attempt_at_ms: now_ms + retry_delay_ms(attempts),
        failing_since_ms: Some(failing_since),
        attempts,
    }
}

/// `registry_authority` is a hostname (§8.2 schema): drop any port.
fn hint_authority(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && port.bytes().all(|b| b.is_ascii_digit()) => {
            host
        }
        _ => authority,
    }
}

/// POST one hint. Never reads the response body.
async fn deliver<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    url: &str,
    body: Vec<u8>,
    signature: &str,
) -> Delivery {
    // The same URL checks as at creation, again: the URL is stored, but
    // the policy may have tightened since.
    if let Err(e) = state.callback_policy.check_url(url) {
        return Delivery::Refused(format!("callback_url refused: {e}"));
    }
    // Resolve and check here, so a refusal is told apart from a network
    // failure. The client's resolver checks again on connect, so an answer
    // that changes in between is still refused.
    let Some((host, port)) = url::Url::parse(url).ok().and_then(|u| {
        Some((
            u.host_str()?.to_owned(),
            u.port_or_known_default().unwrap_or(443),
        ))
    }) else {
        return Delivery::Refused("callback_url has no host".into());
    };
    if let Err(why) = resolve_allowed(
        &state.callback_policy,
        state.callback_resolver.as_ref(),
        &host,
        port,
        state.hint_timeouts.lookup,
    )
    .await
    {
        return Delivery::Refused(why);
    }
    let client = match hint_client(state) {
        Ok(c) => c,
        Err(e) => return Delivery::Failed(format!("http client: {e}")),
    };
    let sent = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(SIGNATURE_HEADER, signature)
        .body(body)
        .send()
        .await;
    match sent {
        // Dropped unread: the body is never consumed (§8.3).
        Ok(resp) if resp.status().is_success() => Delivery::Delivered,
        Ok(resp) => Delivery::Failed(format!("receiver answered {}", resp.status())),
        Err(e) => Delivery::Failed(format!("{e}")),
    }
}

/// A client for one delivery: https only, no proxy, no redirects, no
/// pooling, bounded timeouts, and every DNS answer checked through the
/// callback policy ([`crate::outbound::policy_client`]).
fn hint_client<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
) -> Result<reqwest::Client, reqwest::Error> {
    crate::outbound::policy_client(
        &state.callback_policy,
        &state.callback_resolver,
        crate::outbound::ClientBounds {
            lookup: state.hint_timeouts.lookup,
            connect: state.hint_timeouts.connect,
            request: state.hint_timeouts.request,
        },
        &state.hint_extra_roots,
    )
}

/// Run a pass whenever [`AppStateInner::notify_hints`](crate::state::AppStateInner::notify_hints)
/// fires and at least every `poll`, until `shutdown` turns true (or its
/// sender is dropped). A pass in progress claims no further batch after
/// that, finishes the one in flight, and the task ends. A failed pass is
/// logged and the next wake-up tries again. Also samples the
/// subscription-status and hint-backlog gauges once per wake-up.
pub fn spawn_hint_worker<S: ExtendedRegistryStore + 'static>(
    state: Arc<AppState<S>>,
    poll: Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let clock = system_clock();
    tokio::spawn(async move {
        while !*shutdown.borrow() {
            match run_hint_pass_with(&state, &clock, Some(&shutdown)).await {
                Ok(s) if s.claimed > 0 => tracing::debug!(?s, "change hint pass"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "change hint pass failed"),
            }
            let store = state.server.store();
            if let Ok(head) = store.changes_head().await {
                if let Ok(c) = store.subscription_counts(head.head).await {
                    crate::metrics::record_hint_subscriptions(c.active, c.disabled);
                    crate::metrics::record_hint_backlog(c.oldest_due_at_ms, clock());
                }
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                _ = tokio::time::timeout(poll, state.hints_notify.notified()) => {}
            }
        }
        tracing::info!("change hint worker stopped");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_starts_exactly_at_72_hours() {
        assert!(!failed_long_enough(0, DISABLE_AFTER_MS - 1));
        assert!(failed_long_enough(0, DISABLE_AFTER_MS));
        assert!(failed_long_enough(5, DISABLE_AFTER_MS + 5));
    }

    #[test]
    fn hint_timeouts_shorten_but_never_lengthen_the_bounds() {
        let prod = HintTimeouts::default();
        assert_eq!(prod.lookup, crate::callback::LOOKUP_TIMEOUT);
        assert_eq!(
            (prod.connect, prod.request),
            (CONNECT_TIMEOUT, REQUEST_TIMEOUT)
        );
        let short = HintTimeouts {
            lookup: Duration::from_millis(1),
            connect: Duration::from_millis(2),
            request: Duration::from_millis(3),
        };
        assert_eq!(short.capped(), short);
        let long = HintTimeouts {
            lookup: Duration::from_secs(60),
            connect: Duration::from_secs(60),
            request: Duration::from_secs(60),
        };
        assert_eq!(long.capped(), prod);
    }

    #[test]
    fn every_outcome_has_a_metric_label() {
        for o in [
            HintOutcome::Delivered,
            HintOutcome::NothingVisible,
            HintOutcome::Failed,
            HintOutcome::SsrfRefused,
            HintOutcome::UnknownKey,
            HintOutcome::Disabled,
            HintOutcome::LeaseLost,
            HintOutcome::Error,
        ] {
            assert!(!o.as_str().is_empty());
        }
    }

    #[test]
    fn retry_schedule_doubles_from_a_minute_to_an_hourly_cap() {
        let mins: Vec<i64> = (1..=9).map(|a| retry_delay_ms(a) / 60_000).collect();
        assert_eq!(mins, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(retry_delay_ms(0), RETRY_BASE_MS);
        assert_eq!(retry_delay_ms(u32::MAX), RETRY_CAP_MS);
        // §8.2: retries continue for at least an hour.
        let first_hour: i64 = (1..=6).map(retry_delay_ms).sum();
        assert!(first_hour >= 3_600_000);
    }

    #[test]
    fn hint_authority_drops_a_port_only() {
        assert_eq!(
            hint_authority("registry.example.com"),
            "registry.example.com"
        );
        assert_eq!(
            hint_authority("registry.example.com:8443"),
            "registry.example.com"
        );
        assert_eq!(hint_authority("127.0.0.1:9000"), "127.0.0.1");
    }

    /// The pinned KAT `hint-hmac-001` (rec-007 C).
    #[test]
    fn hint_hmac_001_known_answer() {
        let secret = "q4C3i0gxkC8b7mSx2F5fW1dTnYp9Z0eLrJvH6uK3oAM=";
        let body = br#"{"head_cursor":"c2VxOjEwNTc","hint_version":"acdp-change-hint/1","registry_authority":"registry.example.com","subscription_id":"6f1c2a9e-3b4d-4e5f-8a6b-7c8d9e0f1a2b"}"#;
        assert_eq!(
            sign_hint(secret, 1_790_985_602, body),
            "t=1790985602,v1=a000c589a805e6a5ed4eeb7835a9e8051a879d067c17c86c3de28862f9e489de"
        );
    }
}
