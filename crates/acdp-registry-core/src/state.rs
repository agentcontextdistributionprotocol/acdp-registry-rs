//! Shared HTTP-layer state. Held inside `Arc` by axum.

use std::sync::{Arc, RwLock};

use acdp::client::CrossRegistryResolver;
use acdp::registry::RegistryServer;
use acdp_registry_auth::AuthService;
use acdp_registry_store::{ExtendedRegistryStore, SharedRateLimitBackend};
use acdp_registry_types::config::UnavailablePostureConfig;
use acdp_registry_types::{PlaygroundConfig, RegistryConfig, TenantHeaderTrust};
use acdp_registry_webhook::WebhookEmitter;

use metrics_exporter_prometheus::PrometheusHandle;

use crate::rate_limit::{AgentRateLimiter, TrustedProxies};
use crate::rate_limit_shared::{LayeredRateLimiter, UnavailablePosture};

/// Public alias — the value axum actually carries is `Arc<AppState<S>>`.
pub type AppState<S> = AppStateInner<S>;

pub struct AppStateInner<S: ExtendedRegistryStore> {
    pub server: Arc<RegistryServer<S>>,
    pub auth: Arc<AuthService>,
    pub webhook: Option<WebhookEmitter>,
    pub config: RegistryConfig,
    /// FEAT-01: cross-registry resolver consulted by `retrieve` when the
    /// requested `ctx_id`'s authority differs from this registry's
    /// `config.registry.authority`. `None` disables forwarding.
    pub cross_registry: Option<Arc<CrossRegistryResolver>>,
    /// Live-mutable playground configuration (plan §2: pinned-key
    /// rotation + admin reload). Seeded from `config.playground` at
    /// startup; the `POST /admin/pinned-keys/reload` endpoint re-reads
    /// the on-disk config and atomic-swaps this cell so an operator
    /// can rotate keys without restarting the process.
    ///
    /// Read path holds the lock only long enough to clone the struct
    /// (small + Clone), so no read ever blocks on a writer. Writers
    /// hold the lock for the duration of the env-driven swap.
    pub playground: Arc<RwLock<PlaygroundConfig>>,
    /// REG-P1-3: per-agent `POST /contexts` limiter (RFC-ACDP-0008 §4.3).
    /// `None` when `limits.publish_rate_per_minute == 0` (disabled).
    pub rate_limiter: Option<Arc<AgentRateLimiter>>,
    /// Per-agent `POST /auth/challenge` limiter. The challenge endpoint is
    /// unauthenticated; without this an attacker can flood it to amplify
    /// writes and grow the nonce store. `None` when
    /// `limits.challenge_rate_per_minute == 0` (disabled).
    pub challenge_rate_limiter: Option<Arc<AgentRateLimiter>>,
    /// FEAT-06: per-IP + process-global limiter applied as middleware over the
    /// whole `/auth/*` subrouter, keyed by the resolved client IP. Complements
    /// the per-agent `challenge_rate_limiter` — an unauthenticated attacker
    /// controls `agent_id` and can rotate it, but not their source IP (short
    /// of a botnet, which the global ceiling then bounds). `None` when
    /// `[rate_limit] enabled = false` or both budgets are `0`.
    pub auth_ip_limiter: Option<Arc<AgentRateLimiter>>,
    /// What the `/auth/*` middleware charges. Today the in-memory limiter
    /// above; kept alongside it because a layered backend needs the in-memory
    /// limiter as its first layer.
    pub auth_limit_backend: Option<Arc<dyn SharedRateLimitBackend>>,
    /// FEAT-06: parsed `[rate_limit] trusted_proxies` CIDRs. Empty (the
    /// default) means `X-Forwarded-For` is never trusted and the TCP socket
    /// peer is always used as the client IP.
    pub trusted_proxies: TrustedProxies,
    /// #374: the `auth.tenant_header_trust` mode in effect (the config value,
    /// or `none` when the key is absent, #386).
    pub tenant_header_trust: TenantHeaderTrust,
    /// #374: the same `[rate_limit] trusted_proxies` list, parsed STRICTLY for
    /// the tenant-header trust test. The limiter's copy above is lossy (a bad
    /// entry is dropped and the rest kept); a trust boundary must not be
    /// silently narrowed or reshaped, so one bad entry here empties the list
    /// and trusts no peer. Startup validation refuses a bad entry before this
    /// is ever built; this is the fail-closed backstop for embedders.
    pub tenant_trusted_proxies: TrustedProxies,
    /// FEAT-10: process-global Prometheus recorder handle. `Some` only when
    /// `[metrics] enabled` — the `/metrics` route is mounted iff this is set.
    pub metrics: Option<PrometheusHandle>,
    /// The registry's own `did:web` DID document, served at
    /// `GET /.well-known/did.json` so consumers can resolve the receipt
    /// verification key (RFC-ACDP-0010). Precomputed at startup from
    /// `config.receipt` — `None` when no receipt key is configured (the
    /// endpoint then 404s). Static for the process lifetime: key rotation
    /// is a config change + restart.
    pub registry_did_document: Option<serde_json::Value>,
    /// Transparency-log serving state (RFC-ACDP-0012): the checkpoint
    /// signer (= the RFC-ACDP-0010 receipt signer, §6), the `log_id`,
    /// and the head-root cache. `None` when `log.enabled` is false —
    /// the `/log/*` endpoints then answer 501 `not_implemented`.
    pub log: Option<Arc<crate::log::LogState>>,
    /// RFC-ACDP-0018 change-feed cursor key. `None` when `changes.enabled`
    /// is false or the key fails to load — `GET /changes` then answers
    /// 501 `not_implemented`.
    pub changes: Option<Arc<crate::changes_key::ChangesKey>>,
    /// RFC-ACDP-0006 §7 policy every subscription `callback_url` is held
    /// to, at creation and at each delivery (RFC-ACDP-0018 §8.3). The
    /// production default; tests may loosen it with
    /// [`Self::with_callback_policy`].
    pub callback_policy: acdp::safe_http::SsrfPolicy,
    /// How a callback host is resolved for those checks.
    pub callback_resolver: Arc<dyn crate::callback::CallbackResolver>,
    /// Woken after every commit that appends to the change feed, so the
    /// hint worker runs a pass at once instead of at its next poll.
    pub hints_notify: Arc<tokio::sync::Notify>,
    /// Extra trust anchors for hint delivery, on top of the webpki roots
    /// (tests: a private CA for a local receiver). Empty in production.
    pub hint_extra_roots: Vec<reqwest::Certificate>,
    /// Callback lookup and hint delivery bounds; the production constants
    /// unless a test shortened them with [`Self::with_hint_timeouts`].
    pub hint_timeouts: crate::hints::HintTimeouts,
    /// The smallest store page a `/changes` scan asks for:
    /// [`MIN_SCAN_ROWS`](crate::handlers::MIN_SCAN_ROWS) unless a
    /// test lowered it with [`Self::with_changes_min_scan_rows`].
    pub changes_min_scan_rows: u32,
}

impl<S: ExtendedRegistryStore> AppStateInner<S> {
    /// Route the `/auth/*` ceilings through a shared (cluster-wide) backend.
    /// A no-op when `shared` is `None`.
    ///
    /// **This is where both of the layered limiter's safety properties take
    /// effect.** The binary hands over the *bare* shared backend; it cannot
    /// build the layering itself because the in-memory limiter lives inside this
    /// state. So the shared backend is wrapped here, over the very `Arc` the
    /// middleware already used — never installed bare, which would silently drop
    /// the in-memory pre-filter (the database-load bound) and the fail-open
    /// fallback.
    pub fn with_shared_auth_limiter(
        mut self,
        shared: Option<Arc<dyn SharedRateLimitBackend>>,
    ) -> Self {
        let Some(l2) = shared else { return self };
        let Some(l1) = self.auth_ip_limiter.clone() else {
            // Startup validation rules this out; never install an unlayered L2.
            tracing::error!(
                "shared auth rate limiter configured but the in-memory limiter is disabled; \
                 leaving per-process limiting in place"
            );
            return self;
        };
        let posture = match self.config.rate_limit.backend_unavailable {
            UnavailablePostureConfig::Allow => UnavailablePosture::Allow,
            UnavailablePostureConfig::Deny => UnavailablePosture::Deny,
        };
        self.auth_limit_backend = Some(Arc::new(LayeredRateLimiter::new(l1, l2, posture)));
        self
    }

    /// The changes key, when this registry serves `acdp-registry-events`:
    /// `None` answers `/subscriptions` with 501. Startup validation
    /// requires `changes.enabled` alongside `events.enabled`, so the key is
    /// present whenever events is configured and loads.
    pub fn events_key(&self) -> Option<&crate::changes_key::ChangesKey> {
        if self.config.events.enabled {
            self.changes.as_deref()
        } else {
            None
        }
    }

    /// Replace the callback resolver (tests).
    pub fn with_callback_resolver(
        mut self,
        resolver: Arc<dyn crate::callback::CallbackResolver>,
    ) -> Self {
        self.callback_resolver = resolver;
        self
    }

    /// Also trust the PEM certificate `root` for hint delivery, beside the
    /// webpki roots (tests: an in-process HTTPS receiver).
    pub fn with_hint_root_pem(mut self, root: &[u8]) -> Result<Self, reqwest::Error> {
        self.hint_extra_roots
            .push(reqwest::Certificate::from_pem(root)?);
        Ok(self)
    }

    /// Wake the hint worker: the change feed has grown. Cheap, and a no-op
    /// beyond a stored permit when no worker is waiting.
    pub fn notify_hints(&self) {
        if self.config.events.enabled {
            self.hints_notify.notify_one();
        }
    }

    /// Shorten the callback lookup and hint delivery bounds (tests: a
    /// timeout path in milliseconds). Each is capped at its production
    /// value, so this can never loosen §8.3.
    #[must_use]
    pub fn with_hint_timeouts(mut self, timeouts: crate::hints::HintTimeouts) -> Self {
        self.hint_timeouts = timeouts.capped();
        self
    }

    /// Lower the smallest `/changes` store page (tests: exhaust the refill
    /// budget with a few rows). Clamped to `1..=MIN_SCAN_ROWS`.
    #[must_use]
    pub fn with_changes_min_scan_rows(mut self, rows: u32) -> Self {
        self.changes_min_scan_rows = rows.clamp(1, crate::handlers::MIN_SCAN_ROWS);
        self
    }

    /// Replace the callback SSRF policy (tests).
    pub fn with_callback_policy(mut self, policy: acdp::safe_http::SsrfPolicy) -> Self {
        self.callback_policy = policy;
        self
    }

    /// Build state with a fresh `playground` cell seeded from
    /// `config.playground`. Centralises the lock setup so test
    /// harnesses don't have to know the cell exists.
    pub fn new(
        server: Arc<RegistryServer<S>>,
        auth: Arc<AuthService>,
        webhook: Option<WebhookEmitter>,
        config: RegistryConfig,
        cross_registry: Option<Arc<CrossRegistryResolver>>,
    ) -> Self {
        let playground = Arc::new(RwLock::new(config.playground.clone()));
        let rate_limiter = match config.limits.publish_rate_per_minute {
            0 => None,
            n => Some(Arc::new(AgentRateLimiter::new(n))),
        };
        let challenge_rate_limiter = match config.limits.challenge_rate_per_minute {
            0 => None,
            // #24: `/auth/challenge` is unauthenticated and keyed by the
            // attacker-controlled `agent_id`, so also enforce a process-global
            // ceiling (64× the per-agent budget) — varying `agent_id` can no
            // longer flood the nonce store without bound.
            n => Some(Arc::new(AgentRateLimiter::with_global_ceiling(
                n,
                n.saturating_mul(64),
            ))),
        };
        // Receipt key problems are caught at startup by the binary's
        // validate_config; if the key still fails to load here, serve no
        // DID document rather than panic mid-construction (test harnesses
        // build state without running validate_config first).
        let registry_did_document = if config.receipt.is_configured() {
            crate::receipt::build_did_document(&config.receipt, &config.registry.authority)
                .map_err(|e| tracing::error!(error = %e, "receipt DID document unavailable"))
                .ok()
        } else {
            None
        };
        // Transparency log (RFC-ACDP-0012). Misconfigurations are caught
        // at startup by the binary's validate_config; if construction
        // still fails here, serve 501s rather than panic mid-construction
        // (test harnesses build state without running validate_config).
        let log = crate::log::LogState::from_config(&config)
            .map_err(|e| tracing::error!(error = %e, "transparency log unavailable"))
            .ok()
            .flatten()
            .map(Arc::new);
        // Change feed (RFC-ACDP-0018). Same posture as the log: the binary's
        // validate_config refuses a bad key at startup; here a failure
        // serves 501s rather than panicking.
        let changes = if config.changes.enabled {
            crate::changes_key::load_changes_key(&config.changes, &config.receipt)
                .map_err(|e| tracing::error!(error = %e, "change feed unavailable"))
                .ok()
                .map(Arc::new)
        } else {
            None
        };
        // FEAT-06: per-IP + global `/auth/*` limiter. Reuses the fixed-window
        // `AgentRateLimiter`, keyed by client IP. A `0` global budget maps to
        // the `u32::MAX` "disabled" sentinel; the per-IP check is skipped by
        // the middleware when `per_ip_per_minute == 0`.
        let rl = &config.rate_limit;
        let auth_ip_limiter =
            if rl.enabled && (rl.per_ip_per_minute > 0 || rl.global_per_minute > 0) {
                let global = if rl.global_per_minute == 0 {
                    u32::MAX
                } else {
                    rl.global_per_minute
                };
                Some(Arc::new(AgentRateLimiter::with_global_ceiling(
                    rl.per_ip_per_minute,
                    global,
                )))
            } else {
                None
            };
        let auth_limit_backend: Option<Arc<dyn SharedRateLimitBackend>> = auth_ip_limiter
            .clone()
            .map(|l| l as Arc<dyn SharedRateLimitBackend>);
        let trusted_proxies = TrustedProxies::parse_lossy(&rl.trusted_proxies);
        let tenant_header_trust = config.auth.effective_tenant_header_trust();
        let tenant_trusted_proxies =
            TrustedProxies::parse(&rl.trusted_proxies).unwrap_or_else(|e| {
                tracing::error!(
                    error = %e,
                    "rate_limit.trusted_proxies is invalid; no peer is trusted to \
                     assert X-Tenant-Id"
                );
                TrustedProxies::default()
            });
        // FEAT-10: install the process-global recorder when metrics are
        // enabled. Idempotent — many in-process test harnesses share one.
        let metrics = if config.metrics.enabled {
            Some(crate::metrics::install_recorder(
                &config.metrics.duration_buckets,
            ))
        } else {
            None
        };
        Self {
            server,
            auth,
            webhook,
            config,
            cross_registry,
            playground,
            rate_limiter,
            challenge_rate_limiter,
            auth_ip_limiter,
            auth_limit_backend,
            trusted_proxies,
            tenant_header_trust,
            tenant_trusted_proxies,
            metrics,
            registry_did_document,
            log,
            changes,
            callback_policy: acdp::safe_http::SsrfPolicy::default(),
            callback_resolver: Arc::new(crate::callback::SystemResolver),
            hints_notify: Arc::new(tokio::sync::Notify::new()),
            hint_extra_roots: Vec::new(),
            hint_timeouts: crate::hints::HintTimeouts::default(),
            changes_min_scan_rows: crate::handlers::MIN_SCAN_ROWS,
        }
    }
}
