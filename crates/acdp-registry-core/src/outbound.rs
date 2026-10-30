//! Outbound HTTPS to hosts the registry does not run: change-hint
//! callbacks (RFC-ACDP-0018 §8.3) and keyless issuer keys (RFC-ACDP-0021
//! §5.2 step 3).
//!
//! Every client built here holds the RFC-ACDP-0006 §7 / RFC-ACDP-0008 §4.8
//! line: https only, no proxy, no redirects (stricter than §4.8's three
//! same-authority hops), no connection pooling, bounded lookup, connect
//! and request times, and every DNS answer checked through an
//! [`SsrfPolicy`](acdp::safe_http::SsrfPolicy) at connect time. One
//! forbidden address refuses the whole answer
//! (`reject_if_any_forbidden`), so a public address cannot smuggle a
//! private one through, and the checked addresses are the ones dialled.
//!
//! A client's resolver sees only hostnames: callers check the URL with
//! `SsrfPolicy::check_url` first, which refuses IP literals.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::callback::{resolve_allowed, CallbackResolver};

/// The bounds one outbound client applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientBounds {
    /// One DNS lookup.
    pub lookup: Duration,
    /// TCP + TLS connect.
    pub connect: Duration,
    /// The whole request, body included.
    pub request: Duration,
}

/// A client for one outbound exchange under `policy`: https only, no
/// proxy, no redirects, no pooling, `bounds`, and every DNS answer from
/// `resolver` checked against `policy` on connect. `extra_roots` are
/// trusted beside the webpki roots (tests: a private CA; empty in
/// production).
pub fn policy_client(
    policy: &acdp::safe_http::SsrfPolicy,
    resolver: &Arc<dyn CallbackResolver>,
    bounds: ClientBounds,
    extra_roots: &[reqwest::Certificate],
) -> Result<reqwest::Client, reqwest::Error> {
    let mut b = reqwest::Client::builder()
        .use_rustls_tls()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(bounds.connect)
        .timeout(bounds.request)
        .pool_max_idle_per_host(0)
        .dns_resolver(Arc::new(PolicyResolver {
            policy: policy.clone(),
            inner: Arc::clone(resolver),
            lookup: bounds.lookup,
        }));
    for root in extra_roots {
        b = b.add_root_certificate(root.clone());
    }
    b.build()
}

/// Why [`read_capped`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CappedReadError {
    /// The body is longer than the cap (by `Content-Length` or as read).
    TooLarge,
    /// The transfer failed or timed out mid-body.
    Transport(String),
}

impl std::fmt::Display for CappedReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("response body exceeds the size cap"),
            Self::Transport(e) => write!(f, "response body: {e}"),
        }
    }
}

/// Read `resp`'s body, refusing more than `max` bytes. A declared
/// `Content-Length` over the cap is refused before any byte is read; a
/// body with none (or a false one) is refused at the first chunk that
/// crosses the cap, so at most `max` bytes plus one chunk are buffered.
pub async fn read_capped(
    mut resp: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, CappedReadError> {
    if resp
        .content_length()
        .is_some_and(|n| n > u64::try_from(max).unwrap_or(u64::MAX))
    {
        return Err(CappedReadError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| CappedReadError::Transport(e.to_string()))?
    {
        if body.len() + chunk.len() > max {
            return Err(CappedReadError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// `reqwest` DNS hook: resolve through the configured
/// [`CallbackResolver`], then refuse the whole answer if any address is
/// forbidden (`reject_if_any_forbidden`; filtering would let one public
/// address smuggle a private one through).
struct PolicyResolver {
    policy: acdp::safe_http::SsrfPolicy,
    inner: Arc<dyn CallbackResolver>,
    lookup: Duration,
}

impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let policy = self.policy.clone();
        let inner = Arc::clone(&self.inner);
        let lookup = self.lookup;
        let host = name.as_str().to_owned();
        Box::pin(async move {
            // Port 0: the connector substitutes the URL's port.
            let addrs: Vec<SocketAddr> = resolve_allowed(&policy, inner.as_ref(), &host, 0, lookup)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            let addrs: reqwest::dns::Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}
