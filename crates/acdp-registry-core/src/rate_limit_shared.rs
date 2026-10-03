//! The layered `/auth/*` limiter: the in-memory limiter (L1) in front of a
//! shared, cluster-wide backend (L2) (FEAT-06 item 3).
//!
//! Two properties make this deployable, and both live here rather than in the
//! backend:
//!
//! * **L1 is a permanent pre-filter.** It is asked first and a denial returns
//!   without touching L2, so the rate of database statements is bounded by the
//!   in-memory limits, not by the attacker. The middleware makes two checks per
//!   admitted request (global, then per-IP), each its own L2 round trip. The
//!   global check reaches L2 only when L1-global allowed, so it costs at most
//!   `global_per_minute` statements a minute; the per-IP check is reached only
//!   after the global check passed, so it also costs at most `global_per_minute`
//!   (and at most `per_ip_per_minute × distinct-IPs-admitted`, whichever is
//!   smaller). Per replica, per minute:
//!
//!   ```text
//!   L2 statements ≤ 2 × global_per_minute          (12 000/min at the defaults)
//!   ```
//!
//!   however large the flood and however many source IPs it rotates through —
//!   **provided `global_per_minute > 0`**. With the global budget off, only the
//!   per-IP term remains, `per_ip_per_minute × distinct-IPs-admitted`, which an
//!   attacker rotating IPs controls; startup validation must therefore refuse a
//!   shared backend with `global_per_minute = 0`.
//!   L1 and L2 carry the same limits, so L1 is never looser than the cluster
//!   bound: enabling the shared backend can only tighten enforcement relative
//!   to per-process limiting, never loosen it.
//! * **The unavailable posture is decided here.** [`LayeredRateLimiter::check`]
//!   never returns [`LimitDecision::Unavailable`]. By default an unreachable L2
//!   degrades to exactly per-process enforcement (L1 already admitted the
//!   request) rather than turning a database blip into a token-issuance outage;
//!   `Deny` flips that for an operator who would rather refuse.
//!
//! A request L2 denies has already spent an L1 unit. That makes L1 strictly
//! tighter, never looser, so it is safe — not a bug to "fix". Likewise the two
//! layers' windows are aligned differently (L1 from process start, L2 to the
//! wall clock), so at a boundary L1 may reject marginally early: today's
//! behaviour, in the safe direction.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use acdp_registry_store::{LimitDecision, SharedLimitScope, SharedRateLimitBackend};
use async_trait::async_trait;

use crate::metrics::{record_shared_rate_limit, SharedOutcome};
use crate::rate_limit::AgentRateLimiter;

/// What to do when the shared backend cannot answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailablePosture {
    /// Admit: fall back to the per-process limit L1 already enforced.
    Allow,
    /// Refuse with a short `Retry-After`.
    Deny,
}

/// `Retry-After` for a request refused because the shared backend is down. Long
/// enough that a retrying client does not itself re-stall every check on the
/// backend timeout, short enough to recover quickly.
const UNAVAILABLE_RETRY_AFTER_SECS: u64 = 5;

/// Minimum seconds between `warn!` lines about an unavailable backend.
const WARN_EVERY_SECS: u64 = 60;

/// L1 (in-memory) → L2 (shared) composition. See the module docs.
pub struct LayeredRateLimiter {
    l1: Arc<AgentRateLimiter>,
    l2: Arc<dyn SharedRateLimitBackend>,
    on_unavailable: UnavailablePosture,
    /// Epoch seconds of the last `warn!`; 0 = never.
    last_warn: AtomicU64,
}

impl LayeredRateLimiter {
    pub fn new(
        l1: Arc<AgentRateLimiter>,
        l2: Arc<dyn SharedRateLimitBackend>,
        on_unavailable: UnavailablePosture,
    ) -> Self {
        Self {
            l1,
            l2,
            on_unavailable,
            last_warn: AtomicU64::new(0),
        }
    }

    /// `warn!` at most once per [`WARN_EVERY_SECS`]; the rest at `debug!`.
    fn note_unavailable(&self, scope: SharedLimitScope) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last = self.last_warn.load(Ordering::Relaxed);
        let due = last == 0 || now.saturating_sub(last) >= WARN_EVERY_SECS;
        let posture = format!("{:?}", self.on_unavailable);
        if due
            && self
                .last_warn
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!(
                scope = scope.label(),
                backend = self.l2.backend_name(),
                posture,
                "shared rate-limit backend unavailable"
            );
        } else {
            tracing::debug!(
                scope = scope.label(),
                backend = self.l2.backend_name(),
                posture,
                "shared rate-limit backend unavailable"
            );
        }
    }
}

#[async_trait]
impl SharedRateLimitBackend for LayeredRateLimiter {
    async fn check(&self, scope: SharedLimitScope, key: &str) -> LimitDecision {
        // L1 first: a denial never reaches the database.
        if let denied @ LimitDecision::Deny { .. } =
            SharedRateLimitBackend::check(self.l1.as_ref(), scope, key).await
        {
            return denied;
        }
        let started = Instant::now();
        let answer = self.l2.check(scope, key).await;
        let seconds = started.elapsed().as_secs_f64();
        match answer {
            LimitDecision::Allow => {
                record_shared_rate_limit(scope, SharedOutcome::Allow, seconds);
                LimitDecision::Allow
            }
            deny @ LimitDecision::Deny { .. } => {
                record_shared_rate_limit(scope, SharedOutcome::Deny, seconds);
                deny
            }
            LimitDecision::Unavailable => {
                record_shared_rate_limit(scope, SharedOutcome::Unavailable, seconds);
                self.note_unavailable(scope);
                match self.on_unavailable {
                    UnavailablePosture::Allow => LimitDecision::Allow,
                    UnavailablePosture::Deny => LimitDecision::Deny {
                        retry_after_seconds: UNAVAILABLE_RETRY_AFTER_SECS,
                    },
                }
            }
        }
    }

    fn backend_name(&self) -> &'static str {
        "layered"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    use super::*;

    /// Replays a script of decisions (repeating the last one) and counts calls.
    struct StubBackend {
        script: Mutex<VecDeque<LimitDecision>>,
        last: Mutex<LimitDecision>,
        calls: AtomicUsize,
    }

    impl StubBackend {
        fn new(script: impl IntoIterator<Item = LimitDecision>) -> Arc<Self> {
            let script: VecDeque<_> = script.into_iter().collect();
            let last = *script.back().expect("non-empty script");
            Arc::new(Self {
                script: Mutex::new(script),
                last: Mutex::new(last),
                calls: AtomicUsize::new(0),
            })
        }
        fn always(d: LimitDecision) -> Arc<Self> {
            Self::new([d])
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SharedRateLimitBackend for StubBackend {
        async fn check(&self, _: SharedLimitScope, _: &str) -> LimitDecision {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let next = self.script.lock().unwrap().pop_front();
            match next {
                Some(d) => {
                    *self.last.lock().unwrap() = d;
                    d
                }
                None => *self.last.lock().unwrap(),
            }
        }
        fn backend_name(&self) -> &'static str {
            "stub"
        }
    }

    const DENY_L2: LimitDecision = LimitDecision::Deny {
        retry_after_seconds: 42,
    };

    fn layered(
        l1_per_ip: u32,
        l1_global: u32,
        l2: Arc<StubBackend>,
        posture: UnavailablePosture,
    ) -> LayeredRateLimiter {
        LayeredRateLimiter::new(
            Arc::new(AgentRateLimiter::with_global_ceiling(l1_per_ip, l1_global)),
            l2,
            posture,
        )
    }

    /// The database-amplification bound as an executable assertion: once L1
    /// denies, L2 is no longer asked, however many more requests arrive.
    #[tokio::test]
    async fn l1_denial_short_circuits_before_l2() {
        let l2 = StubBackend::always(LimitDecision::Allow);
        let l = layered(3, u32::MAX, l2.clone(), UnavailablePosture::Allow);
        for _ in 0..3 {
            assert_eq!(
                l.check(SharedLimitScope::PerIp, "1.1.1.1").await,
                LimitDecision::Allow
            );
        }
        assert_eq!(
            l2.calls(),
            3,
            "L2 consulted exactly for the admitted requests"
        );
        for _ in 0..1_000 {
            assert!(matches!(
                l.check(SharedLimitScope::PerIp, "1.1.1.1").await,
                LimitDecision::Deny { .. }
            ));
        }
        assert_eq!(
            l2.calls(),
            3,
            "a flood past L1's limit must cost zero L2 statements"
        );
    }

    #[tokio::test]
    async fn global_scope_is_also_pre_filtered() {
        let l2 = StubBackend::always(LimitDecision::Allow);
        let l = layered(100, 2, l2.clone(), UnavailablePosture::Allow);
        for _ in 0..2 {
            l.check(SharedLimitScope::Global, "").await;
        }
        for _ in 0..50 {
            assert!(matches!(
                l.check(SharedLimitScope::Global, "").await,
                LimitDecision::Deny { .. }
            ));
        }
        assert_eq!(l2.calls(), 2);
    }

    #[tokio::test]
    async fn l2_denial_passes_through_with_its_own_retry_after() {
        let l2 = StubBackend::always(DENY_L2);
        let l = layered(100, u32::MAX, l2.clone(), UnavailablePosture::Allow);
        // 42 is L2's value; L1's would be in 59..=60.
        assert_eq!(l.check(SharedLimitScope::PerIp, "k").await, DENY_L2);
        assert_eq!(l2.calls(), 1);
    }

    #[tokio::test]
    async fn unavailable_allows_under_allow_posture() {
        let l2 = StubBackend::always(LimitDecision::Unavailable);
        let l = layered(100, u32::MAX, l2, UnavailablePosture::Allow);
        assert_eq!(
            l.check(SharedLimitScope::PerIp, "k").await,
            LimitDecision::Allow
        );
    }

    /// Fail-open is not "no limiting": L1 keeps enforcing while L2 is down.
    #[tokio::test]
    async fn unavailable_under_allow_posture_still_enforces_l1() {
        let l2 = StubBackend::always(LimitDecision::Unavailable);
        let l = layered(2, u32::MAX, l2, UnavailablePosture::Allow);
        assert_eq!(
            l.check(SharedLimitScope::PerIp, "k").await,
            LimitDecision::Allow
        );
        assert_eq!(
            l.check(SharedLimitScope::PerIp, "k").await,
            LimitDecision::Allow
        );
        assert!(matches!(
            l.check(SharedLimitScope::PerIp, "k").await,
            LimitDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn unavailable_denies_under_deny_posture() {
        let l2 = StubBackend::always(LimitDecision::Unavailable);
        let l = layered(100, u32::MAX, l2, UnavailablePosture::Deny);
        match l.check(SharedLimitScope::PerIp, "k").await {
            LimitDecision::Deny {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, UNAVAILABLE_RETRY_AFTER_SECS);
                assert!(retry_after_seconds >= 1);
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn never_surfaces_unavailable() {
        for posture in [UnavailablePosture::Allow, UnavailablePosture::Deny] {
            for stub in [LimitDecision::Allow, DENY_L2, LimitDecision::Unavailable] {
                let l = layered(100, 100, StubBackend::always(stub), posture);
                for scope in [SharedLimitScope::PerIp, SharedLimitScope::Global] {
                    assert_ne!(
                        l.check(scope, "k").await,
                        LimitDecision::Unavailable,
                        "{posture:?}/{stub:?}/{scope:?}"
                    );
                }
            }
        }
    }

    /// Enabling the shared backend can only tighten enforcement: with an L2 that
    /// always allows, the layered limiter admits no more than L1 alone does.
    #[tokio::test]
    async fn layered_is_never_looser_than_l1_alone() {
        let bare = AgentRateLimiter::with_global_ceiling(5, 8);
        let l = layered(
            5,
            8,
            StubBackend::always(LimitDecision::Allow),
            UnavailablePosture::Allow,
        );
        let (mut bare_admitted, mut layered_admitted) = (0, 0);
        for i in 0..60 {
            let ip = format!("10.0.0.{}", i % 4);
            let g_bare = AgentRateLimiter::check_global(&bare).is_ok();
            let b = g_bare && AgentRateLimiter::check(&bare, &ip).is_ok();
            let g_lay = l.check(SharedLimitScope::Global, "").await == LimitDecision::Allow;
            let y = g_lay && l.check(SharedLimitScope::PerIp, &ip).await == LimitDecision::Allow;
            bare_admitted += usize::from(b);
            layered_admitted += usize::from(y);
        }
        assert!(
            layered_admitted <= bare_admitted,
            "{layered_admitted} > {bare_admitted}"
        );
        assert!(bare_admitted > 0, "the comparison must not be vacuous");
    }

    #[test]
    fn backend_name_is_layered() {
        let l = layered(
            1,
            1,
            StubBackend::always(LimitDecision::Allow),
            UnavailablePosture::Allow,
        );
        assert_eq!(l.backend_name(), "layered");
    }

    /// Each L2 answer is counted under its own `outcome`, with the scope label,
    /// through a thread-local recorder (the test runs on one thread).
    #[tokio::test]
    async fn l2_round_trips_are_counted_by_scope_and_outcome() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        for (stub, scope) in [
            (LimitDecision::Allow, SharedLimitScope::PerIp),
            (DENY_L2, SharedLimitScope::Global),
            (LimitDecision::Unavailable, SharedLimitScope::PerIp),
        ] {
            let l = layered(
                100,
                100,
                StubBackend::always(stub),
                UnavailablePosture::Allow,
            );
            l.check(scope, "k").await;
        }
        let text = handle.render();
        for line in [
            r#"acdp_registry_rate_limit_shared_total{scope="auth_per_ip",outcome="allow"} 1"#,
            r#"acdp_registry_rate_limit_shared_total{scope="auth_global",outcome="deny"} 1"#,
            r#"acdp_registry_rate_limit_shared_seconds_count{scope="auth_global"} 1"#,
            r#"acdp_registry_rate_limit_shared_total{scope="auth_per_ip",outcome="unavailable"} 1"#,
        ] {
            assert!(text.contains(line), "missing `{line}` in:\n{text}");
        }
        assert!(
            text.contains("acdp_registry_rate_limit_shared_seconds_count{scope=\"auth_per_ip\"} 2"),
            "{text}"
        );
    }
}
