//! The configured issuers' published keys (RFC-ACDP-0021 §5.2 step 3),
//! fetched over [`crate::outbound`] and cached in process.
//!
//! **Fetch targets.** Only the `[keyless]` config entries, which are
//! exactly the advertised `keyless.accepted_*` lists: an OIDC issuer's
//! configured `jwks_uri`, or else the `jwks_uri` of its
//! `/.well-known/openid-configuration` (whose `issuer` must be the
//! configured one, byte for byte); a SPIFFE trust domain's configured
//! `bundle_endpoint`. The cache's slots are built once from config, so a
//! source not in config is never fetched and the cache never grows.
//!
//! **Freshness.** A copy is used for `jwks_cache_ttl_seconds`, shortened by
//! the response's `Cache-Control: max-age` (floored at 60 s, never above
//! 3600 s). Past that it is refetched; one fetch per source at a time,
//! the rest wait for it (single flight).
//!
//! **Outage (stale-if-error).** When a refetch fails, the old copy is
//! still used for up to `jwks_max_stale_seconds` past its TTL, and the
//! source is not retried for [`FAILURE_BACKOFF_MS`], so an outage does not
//! hold every request for a timeout. With no usable copy the answer is
//! [`IssuerUnreachable`] (`key_resolution_unreachable`, `issuer_unreachable`),
//! and a source with no copy at all is not retried inside that window
//! either: it answers unreachable at once (a negative cache).
//!
//! **Unknown `kid`.** [`JwksCache::refresh_for_unknown_kid`] refetches at
//! most once per source per [`UNKNOWN_KID_REFETCH_MS`], so a token naming
//! keys that do not exist cannot make the registry hammer an issuer.
//!
//! Fetch failures are logged at warn with the source and a failure class
//! only; a token never reaches this module.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use acdp_registry_types::config::KeylessConfig;
use serde::Deserialize;

use super::Clock;
use crate::callback::{CallbackResolver, SystemResolver};
use crate::outbound::{policy_client, read_capped, CappedReadError, ClientBounds};

/// An unknown `kid` forces at most one refetch per source in this window.
pub const UNKNOWN_KID_REFETCH_MS: i64 = 30_000;
/// After a failed fetch, a source is not refetched for this long: a stale
/// copy answers meanwhile, or, with none, [`IssuerUnreachable`] at once.
pub const FAILURE_BACKOFF_MS: i64 = 30_000;
/// The shortest TTL a `Cache-Control: max-age` can impose.
pub const MIN_TTL_SECONDS: u64 = 60;
/// The longest TTL any copy is used for.
pub const MAX_TTL_SECONDS: u64 = 3600;

/// One configured key source, named the way §13.1 advertises it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeySource {
    /// An accepted OIDC issuer, by its issuer URI.
    Oidc(String),
    /// An accepted SPIFFE trust domain.
    Spiffe(String),
}

impl std::fmt::Display for KeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Oidc(i) => write!(f, "oidc:{i}"),
            Self::Spiffe(d) => write!(f, "spiffe:{d}"),
        }
    }
}

/// One published key, as served (RFC 7517 §4). Members this registry does
/// not read are ignored; a key that does not parse is skipped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IssuerJwk {
    pub kty: String,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
    #[serde(default, rename = "use")]
    pub use_: Option<String>,
    #[serde(default)]
    pub crv: Option<String>,
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
}

/// A source's keys at one fetch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeySet {
    pub keys: Vec<IssuerJwk>,
}

/// The issuer's keys could not be fetched and no usable copy is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssuerUnreachable;

/// Where outbound fetches go and how hosts are resolved and trusted.
#[derive(Clone)]
pub struct Outbound {
    /// The RFC-ACDP-0006 §7 policy every address is held to.
    pub policy: acdp::safe_http::SsrfPolicy,
    /// How hosts are resolved.
    pub resolver: Arc<dyn CallbackResolver>,
    /// Root certificates trusted beside the webpki roots (tests only).
    pub extra_roots: Vec<reqwest::Certificate>,
}

impl Outbound {
    /// The production policy and the system resolver, webpki roots only.
    pub fn production() -> Self {
        Self {
            policy: acdp::safe_http::SsrfPolicy::default(),
            resolver: Arc::new(SystemResolver),
            extra_roots: Vec::new(),
        }
    }

    /// Also trust the PEM certificate `root` (tests: an in-process HTTPS
    /// issuer).
    pub fn with_root_pem(mut self, root: &[u8]) -> Result<Self, reqwest::Error> {
        self.extra_roots.push(reqwest::Certificate::from_pem(root)?);
        Ok(self)
    }
}

/// What a fetch is.
#[derive(Debug, Clone)]
enum Target {
    /// A JWKS (an OIDC `jwks_uri`, or a SPIFFE bundle endpoint).
    Jwks(String),
    /// OIDC discovery for this issuer, then its `jwks_uri`.
    Discovery(String),
}

/// Why a fetch failed. Every one maps to [`IssuerUnreachable`] when no
/// usable copy is held; `label()` is the metric and log class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The URL failed the SSRF policy before any connection.
    Policy,
    /// DNS (including a refused answer), connect, TLS or timeout.
    Transport,
    /// A non-2xx answer, a redirect included.
    Status(u16),
    /// The body exceeded `max_fetch_bytes`.
    TooLarge,
    /// The body is not a JWKS (or not a discovery document).
    BadDocument,
    /// The discovery document's `issuer` or `jwks_uri` is unusable.
    BadDiscovery,
}

impl FetchError {
    /// The `outcome` metric label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Policy => "policy_refused",
            Self::Transport => "transport",
            Self::Status(_) => "bad_status",
            Self::TooLarge => "too_large",
            Self::BadDocument => "bad_document",
            Self::BadDiscovery => "bad_discovery",
        }
    }
}

/// Counters a test (or an operator, through the metrics) can read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Fetch attempts (a discovery and its JWKS count once).
    pub fetches: u64,
    /// Failed fetch attempts.
    pub fetch_failures: u64,
    /// Lookups answered from a fresh copy.
    pub hits: u64,
    /// Lookups answered from a copy past its TTL during an outage.
    pub stale_served: u64,
}

#[derive(Default)]
struct Counters {
    fetches: AtomicU64,
    fetch_failures: AtomicU64,
    hits: AtomicU64,
    stale_served: AtomicU64,
}

struct Entry {
    keys: Arc<KeySet>,
    fetched_at_ms: i64,
    ttl_ms: i64,
}

#[derive(Default)]
struct SlotState {
    entry: Option<Entry>,
    last_forced_ms: Option<i64>,
    failed_at_ms: Option<i64>,
}

struct Slot {
    target: Target,
    /// Held across a fetch: the single-flight lock.
    state: tokio::sync::Mutex<SlotState>,
}

/// The in-process key cache, one slot per configured source.
pub struct JwksCache {
    slots: HashMap<KeySource, Slot>,
    outbound: Outbound,
    bounds: ClientBounds,
    max_bytes: usize,
    ttl_seconds: u64,
    max_stale_ms: i64,
    clock: Clock,
    counters: Counters,
}

impl std::fmt::Debug for JwksCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksCache")
            .field("sources", &self.slots.keys().collect::<Vec<_>>())
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl JwksCache {
    /// A cache for exactly `cfg`'s issuers and trust domains.
    /// A disabled `[keyless]` gets no slots at all, so nothing is ever
    /// fetched whatever its lists say.
    pub fn new(cfg: &KeylessConfig, outbound: Outbound, clock: Clock) -> Self {
        let mut slots = HashMap::new();
        let (oidc, spiffe) = if cfg.enabled {
            (&cfg.oidc_issuers[..], &cfg.spiffe_trust_domains[..])
        } else {
            (&[][..], &[][..])
        };
        for o in oidc {
            let target = match &o.jwks_uri {
                Some(uri) => Target::Jwks(uri.clone()),
                None => Target::Discovery(o.issuer.clone()),
            };
            slots.insert(KeySource::Oidc(o.issuer.clone()), Slot::new(target));
        }
        for d in spiffe {
            slots.insert(
                KeySource::Spiffe(d.trust_domain.clone()),
                Slot::new(Target::Jwks(d.bundle_endpoint.clone())),
            );
        }
        let timeout = Duration::from_millis(cfg.fetch_timeout_ms);
        Self {
            slots,
            outbound,
            bounds: ClientBounds {
                lookup: timeout.min(crate::callback::LOOKUP_TIMEOUT),
                connect: timeout,
                request: timeout,
            },
            max_bytes: usize::try_from(cfg.max_fetch_bytes).unwrap_or(usize::MAX),
            ttl_seconds: cfg.jwks_cache_ttl_seconds.min(MAX_TTL_SECONDS),
            max_stale_ms: secs_to_ms(cfg.jwks_max_stale_seconds),
            clock,
            counters: Counters::default(),
        }
    }

    /// The counters so far.
    pub fn stats(&self) -> CacheStats {
        let c = &self.counters;
        CacheStats {
            fetches: c.fetches.load(Ordering::Relaxed),
            fetch_failures: c.fetch_failures.load(Ordering::Relaxed),
            hits: c.hits.load(Ordering::Relaxed),
            stale_served: c.stale_served.load(Ordering::Relaxed),
        }
    }

    /// `source`'s keys: the cached copy while fresh, else a refetch, else
    /// (the issuer unreachable) a stale copy inside its window.
    pub async fn keys(&self, source: &KeySource) -> Result<Arc<KeySet>, IssuerUnreachable> {
        let slot = self.slots.get(source).ok_or(IssuerUnreachable)?;
        let mut st = slot.state.lock().await;
        let now = (self.clock)();
        let backing_off = st
            .failed_at_ms
            .is_some_and(|f| now < f.saturating_add(FAILURE_BACKOFF_MS));
        match &st.entry {
            Some(e) => {
                if now < e.fetched_at_ms.saturating_add(e.ttl_ms) {
                    self.counters.hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(Arc::clone(&e.keys));
                }
                if backing_off {
                    if let Some(keys) = self.stale(e, now) {
                        return Ok(keys);
                    }
                }
            }
            // Negative cache: a cold source that just failed answers at
            // once, so an outage costs one timeout per window, not one
            // per queued request.
            None if backing_off => return Err(IssuerUnreachable),
            None => {}
        }
        match self.fetch(source, &slot.target).await {
            Ok(fresh) => Ok(self.store(&mut st, fresh, now)),
            Err(()) => {
                st.failed_at_ms = Some(now);
                st.entry
                    .as_ref()
                    .and_then(|e| self.stale(e, now))
                    .ok_or(IssuerUnreachable)
            }
        }
    }

    /// Refetch `source` because a token named a `kid` the held copy lacks.
    /// `Ok(None)`: a forced refetch already ran within
    /// [`UNKNOWN_KID_REFETCH_MS`], so none was made and the held copy
    /// stands. A failed refetch is [`IssuerUnreachable`]: whether the key
    /// exists cannot be told.
    pub async fn refresh_for_unknown_kid(
        &self,
        source: &KeySource,
    ) -> Result<Option<Arc<KeySet>>, IssuerUnreachable> {
        let slot = self.slots.get(source).ok_or(IssuerUnreachable)?;
        let mut st = slot.state.lock().await;
        let now = (self.clock)();
        if st
            .last_forced_ms
            .is_some_and(|t| now < t.saturating_add(UNKNOWN_KID_REFETCH_MS))
        {
            return Ok(None);
        }
        st.last_forced_ms = Some(now);
        match self.fetch(source, &slot.target).await {
            Ok(fresh) => Ok(Some(self.store(&mut st, fresh, now))),
            Err(()) => {
                st.failed_at_ms = Some(now);
                Err(IssuerUnreachable)
            }
        }
    }

    fn stale(&self, e: &Entry, now: i64) -> Option<Arc<KeySet>> {
        let until = e
            .fetched_at_ms
            .saturating_add(e.ttl_ms)
            .saturating_add(self.max_stale_ms);
        (now < until).then(|| {
            self.counters.stale_served.fetch_add(1, Ordering::Relaxed);
            crate::metrics::record_keyless_key_stale_served();
            Arc::clone(&e.keys)
        })
    }

    fn store(
        &self,
        st: &mut SlotState,
        (keys, max_age): (KeySet, Option<u64>),
        now: i64,
    ) -> Arc<KeySet> {
        let ttl = match max_age {
            Some(m) => self
                .ttl_seconds
                .min(m.clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS)),
            None => self.ttl_seconds,
        };
        let keys = Arc::new(keys);
        st.entry = Some(Entry {
            keys: Arc::clone(&keys),
            fetched_at_ms: now,
            ttl_ms: secs_to_ms(ttl),
        });
        st.failed_at_ms = None;
        keys
    }

    /// One fetch, counted and logged. `Err(())` once logged.
    async fn fetch(
        &self,
        source: &KeySource,
        target: &Target,
    ) -> Result<(KeySet, Option<u64>), ()> {
        self.counters.fetches.fetch_add(1, Ordering::Relaxed);
        let got = match target {
            Target::Jwks(url) => self.fetch_jwks(url).await,
            Target::Discovery(issuer) => self.fetch_discovered(issuer).await,
        };
        match got {
            Ok(v) => {
                crate::metrics::record_keyless_key_fetch("ok");
                Ok(v)
            }
            Err(e) => {
                self.counters.fetch_failures.fetch_add(1, Ordering::Relaxed);
                crate::metrics::record_keyless_key_fetch(e.label());
                tracing::warn!(
                    %source,
                    failure = e.label(),
                    status = match e { FetchError::Status(s) => s, _ => 0 },
                    "keyless issuer keys could not be fetched"
                );
                Err(())
            }
        }
    }

    async fn fetch_discovered(&self, issuer: &str) -> Result<(KeySet, Option<u64>), FetchError> {
        // OpenID Connect Discovery 1.0 §4: the issuer with any trailing
        // `/` removed, then the well-known path.
        let url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let (body, _) = self.get(&url).await?;
        #[derive(Deserialize)]
        struct Discovery {
            issuer: String,
            jwks_uri: String,
        }
        let doc: Discovery = serde_json::from_slice(&body).map_err(|_| FetchError::BadDocument)?;
        // Discovery §4.3: the document's issuer MUST be the one asked for.
        let jwks_https = url::Url::parse(&doc.jwks_uri).is_ok_and(|u| u.scheme() == "https");
        if doc.issuer != issuer || !jwks_https {
            return Err(FetchError::BadDiscovery);
        }
        self.fetch_jwks(&doc.jwks_uri).await
    }

    async fn fetch_jwks(&self, url: &str) -> Result<(KeySet, Option<u64>), FetchError> {
        let (body, max_age) = self.get(url).await?;
        #[derive(Deserialize)]
        struct Jwks {
            keys: Vec<serde_json::Value>,
        }
        let set: Jwks = serde_json::from_slice(&body).map_err(|_| FetchError::BadDocument)?;
        let keys = set
            .keys
            .into_iter()
            .filter_map(|k| serde_json::from_value::<IssuerJwk>(k).ok())
            .collect();
        Ok((KeySet { keys }, max_age))
    }

    /// GET `url` under the outbound policy: the body (capped) and the
    /// response's `max-age`.
    async fn get(&self, url: &str) -> Result<(Vec<u8>, Option<u64>), FetchError> {
        // Refuses IP literals, which the client's resolver would never see.
        self.outbound
            .policy
            .check_url(url)
            .map_err(|_| FetchError::Policy)?;
        let client = policy_client(
            &self.outbound.policy,
            &self.outbound.resolver,
            self.bounds,
            &self.outbound.extra_roots,
        )
        .map_err(|_| FetchError::Transport)?;
        let resp = client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| FetchError::Transport)?;
        if !resp.status().is_success() {
            return Err(FetchError::Status(resp.status().as_u16()));
        }
        let max_age = resp
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .and_then(cache_control_max_age);
        let body = read_capped(resp, self.max_bytes)
            .await
            .map_err(|e| match e {
                CappedReadError::TooLarge => FetchError::TooLarge,
                CappedReadError::Transport(_) => FetchError::Transport,
            })?;
        Ok((body, max_age))
    }
}

impl Slot {
    fn new(target: Target) -> Self {
        Self {
            target,
            state: tokio::sync::Mutex::new(SlotState::default()),
        }
    }
}

fn secs_to_ms(s: u64) -> i64 {
    i64::try_from(s.saturating_mul(1000)).unwrap_or(i64::MAX)
}

/// The lifetime a `Cache-Control` value allows: `max-age=N`, or 0 for
/// `no-store` / `no-cache` (which the TTL floor then lifts to
/// [`MIN_TTL_SECONDS`]). `None` when it says neither.
fn cache_control_max_age(v: &str) -> Option<u64> {
    let mut max_age = None;
    for d in v.split(',').map(str::trim) {
        let lower = d.to_ascii_lowercase();
        if lower == "no-store" || lower == "no-cache" {
            return Some(0);
        }
        if let Some(n) = lower.strip_prefix("max-age=") {
            max_age = n.trim_matches('"').parse().ok().or(max_age);
        }
    }
    max_age
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_control_max_age_reads_the_directives_that_matter() {
        assert_eq!(cache_control_max_age("public, max-age=120"), Some(120));
        assert_eq!(cache_control_max_age("Max-Age=\"90\""), Some(90));
        assert_eq!(cache_control_max_age("no-store"), Some(0));
        assert_eq!(cache_control_max_age("max-age=600, no-cache"), Some(0));
        assert_eq!(cache_control_max_age("public"), None);
        assert_eq!(cache_control_max_age("max-age=abc"), None);
    }

    /// The configured sources of an enabled section.
    fn two_sources(enabled: bool) -> KeylessConfig {
        KeylessConfig {
            enabled,
            oidc_issuers: vec![acdp_registry_types::config::OidcIssuer {
                issuer: "https://issuer.example".into(),
                jwks_uri: None,
            }],
            spiffe_trust_domains: vec![acdp_registry_types::config::SpiffeDomain {
                trust_domain: "prod.example".into(),
                bundle_endpoint: "https://bundle.example/".into(),
                jwt_issuer: None,
            }],
            ..KeylessConfig::default()
        }
    }

    #[test]
    fn slots_are_exactly_the_configured_sources() {
        let c = JwksCache::new(
            &two_sources(true),
            Outbound::production(),
            super::super::system_clock(),
        );
        let mut got: Vec<String> = c.slots.keys().map(ToString::to_string).collect();
        got.sort();
        assert_eq!(got, ["oidc:https://issuer.example", "spiffe:prod.example"]);
    }

    /// A disabled section builds no slot, whatever its lists hold.
    #[tokio::test]
    async fn a_disabled_section_has_no_slots_and_never_fetches() {
        let c = JwksCache::new(
            &two_sources(false),
            Outbound::production(),
            super::super::system_clock(),
        );
        assert!(c.slots.is_empty());
        let s = KeySource::Oidc("https://issuer.example".into());
        assert_eq!(c.keys(&s).await, Err(IssuerUnreachable));
        assert_eq!(c.stats().fetches, 0);
    }

    /// A source outside config is never fetched: the lookup answers
    /// unreachable without a slot to fetch from.
    #[tokio::test]
    async fn an_unconfigured_source_is_never_fetched() {
        let c = JwksCache::new(
            &KeylessConfig::default(),
            Outbound::production(),
            super::super::system_clock(),
        );
        let s = KeySource::Oidc("https://elsewhere.example".into());
        assert_eq!(c.keys(&s).await, Err(IssuerUnreachable));
        assert_eq!(c.refresh_for_unknown_kid(&s).await, Err(IssuerUnreachable));
        assert_eq!(c.stats().fetches, 0);
    }
}
