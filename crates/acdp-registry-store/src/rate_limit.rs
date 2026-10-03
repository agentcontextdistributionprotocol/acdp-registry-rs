//! Seam for the `/auth/*` rate limiter (FEAT-06 item 3).
//!
//! The trait lives in this crate — the one both `acdp-registry-core` (the
//! in-memory implementation and the middleware) and `acdp-registry-pg` (the
//! future shared implementation) already depend on — so neither has to pull
//! the other's dependency tree in.
//!
//! The seam is deliberately as wide as the two `/auth/*` scopes. The per-agent
//! publish and challenge budgets are not routed through it.

use async_trait::async_trait;

/// Which `/auth/*` ceiling a check is charged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedLimitScope {
    /// The per-client-IP budget. The `key` is the canonical client IP.
    PerIp,
    /// The ceiling across every client. The `key` is ignored.
    Global,
}

impl SharedLimitScope {
    /// Byte-identical to the Prometheus `scope` label values in
    /// `acdp-registry-core::metrics` — pinned by a test there, not by
    /// convention.
    pub fn label(self) -> &'static str {
        match self {
            Self::PerIp => "auth_per_ip",
            Self::Global => "auth_global",
        }
    }
}

/// The outcome of one charge against a [`SharedRateLimitBackend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitDecision {
    Allow,
    /// Over budget. `retry_after_seconds` is at least 1, so a client never
    /// sees `Retry-After: 0`.
    Deny {
        retry_after_seconds: u64,
    },
    /// The backend could not answer. NOT a decision: the caller applies its
    /// configured posture. An in-process backend never returns this.
    Unavailable,
}

/// A counter the `/auth/*` middleware charges one request against.
#[async_trait]
pub trait SharedRateLimitBackend: Send + Sync {
    /// Charge one request to `scope`. [`SharedLimitScope::Global`] is checked
    /// with `key = ""`.
    async fn check(&self, scope: SharedLimitScope, key: &str) -> LimitDecision;

    /// Short stable name for logs and metrics: `"memory"`, `"postgres"` or
    /// `"layered"`.
    fn backend_name(&self) -> &'static str;
}
