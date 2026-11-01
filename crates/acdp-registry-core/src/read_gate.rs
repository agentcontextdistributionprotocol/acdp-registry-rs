//! The data-plane read gate: RFC-ACDP-0008 §6.2 read authentication, then
//! the §4.3 read rate limit, before any read handler runs.
//!
//! Every `GET`/`HEAD` on the `data` group passes through `read_gate`. It
//! resolves the `Authorization` header once, into a [`ReadPrincipal`]
//! request extension the handlers read instead of parsing the header again:
//!
//! * no header, or a scheme other than `Bearer`: anonymous;
//! * an `acdp_rt_` token (`bearer_static`): validated against
//!   `[[auth.read_tokens]]`, never parsed as a JWT;
//! * any other bearer: the registry's own JWT (`bearer_jwt`).
//!
//! A bearer that fails validation is refused `403 not_authorized` here, before
//! the handler looks anything up, so the refusal is the same whether or not
//! the resource exists, and the request never falls back to anonymous
//! (§6.2), not even for a public context. The refusal is charged to the
//! client's anonymous read bucket first, so guessing tokens costs budget; an
//! exhausted bucket answers 429 instead.
//!
//! With `auth.enabled = false` the header is ignored entirely and every read
//! is anonymous, as before this gate existed.
//!
//! Nothing in this module logs, formats or echoes a token.

use std::sync::Arc;

use acdp::types::AgentDid;
use acdp::AcdpError;
use acdp_registry_auth::{extract_bearer, is_read_token};
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryError;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::state::AppState;
use crate::{metrics, rate_limit, tenant_trust};

/// Who a data-plane read is from: `effective_requester_did` and its tenant
/// binding, or nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadPrincipal {
    Anonymous,
    Did {
        did: AgentDid,
        /// The JWT's issuer-signed `tenant` claim, or the static token's
        /// configured binding. Authoritative over `X-Tenant-Id` in
        /// `tenant_for_principal`.
        tenant: Option<String>,
    },
}

impl ReadPrincipal {
    /// `effective_requester_did`, for visibility checks.
    pub fn requester(&self) -> Option<AgentDid> {
        match self {
            Self::Anonymous => None,
            Self::Did { did, .. } => Some(did.clone()),
        }
    }

    /// The tenant the credential itself binds, if any.
    pub fn tenant_claim(&self) -> Option<String> {
        match self {
            Self::Anonymous => None,
            Self::Did { tenant, .. } => tenant.clone(),
        }
    }
}

/// Handlers take the principal the gate resolved. Outside the gate (a route
/// it does not cover, or an embedder calling a handler directly) it is
/// resolved here instead, by the same rules.
impl<S: ExtendedRegistryStore + 'static> FromRequestParts<Arc<AppState<S>>> for ReadPrincipal {
    type Rejection = RegistryError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState<S>>,
    ) -> Result<Self, Self::Rejection> {
        if let Some(p) = parts.extensions.get::<ReadPrincipal>() {
            return Ok(p.clone());
        }
        resolve(state, &parts.headers)
    }
}

/// Resolve the request's read credential. `Err` is the refusal to send.
pub(crate) fn resolve<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    headers: &HeaderMap,
) -> Result<ReadPrincipal, RegistryError> {
    if !state.config.auth.enabled {
        return Ok(ReadPrincipal::Anonymous);
    }
    let mut values = headers.get_all(axum::http::header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(ReadPrincipal::Anonymous);
    };
    if values.next().is_some() {
        // Which one would be "the" credential is ambiguous (RFC 9110 §5.3
        // forbids repeating a singleton field), so neither is used.
        return Err(RegistryError::Acdp(AcdpError::SchemaViolation(
            "more than one Authorization header".into(),
        )));
    }
    let Some(token) = value.to_str().ok().and_then(extract_bearer) else {
        return Ok(ReadPrincipal::Anonymous);
    };
    if is_read_token(token) {
        let p = state
            .read_tokens
            .validate(token, chrono::Utc::now())
            .map_err(|e| RegistryError::AuthToken(e.to_string()))?;
        return Ok(ReadPrincipal::Did {
            did: AgentDid::new(&p.did),
            tenant: p.tenant,
        });
    }
    let claims = state.auth.validate_bearer_claims(token)?;
    Ok(ReadPrincipal::Did {
        did: AgentDid::new(&claims.sub),
        tenant: claims.tenant,
    })
}

/// Whether this registry accepts `bearer_static` (and so advertises it).
pub fn bearer_static_active<S: ExtendedRegistryStore + 'static>(state: &AppState<S>) -> bool {
    state.config.auth.enabled && !state.read_tokens.is_empty()
}

/// The `data` group's read gate: authentication (403), then the read rate
/// limit (429), then the handler. Writes pass straight through; their
/// bearer-authenticated routes refuse a static token themselves
/// (`caller_from_headers`).
pub(crate) async fn read_gate<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    mut req: Request,
    next: Next,
) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return next.run(req).await;
    }
    // RFC 6750 §2.2: a `bearer_static` token is never read from the query.
    // Refusing the parameter outright (rather than ignoring it) tells the
    // client its credential went nowhere instead of serving it anonymously.
    if bearer_static_active(&state) && query_names_access_token(req.uri()) {
        return RegistryError::Acdp(AcdpError::SchemaViolation(
            "access_token is not accepted in the query; send Authorization: Bearer".into(),
        ))
        .into_response();
    }
    let peer = req
        .extensions()
        .get::<tenant_trust::PeerIp>()
        .and_then(|p| p.0);
    // Resolved up front (not as a closure over `req`): a `&Request` held
    // across the `next.run` await would make this future `!Send`.
    let client = state.client_ip(peer, req.headers());
    let anonymous_key = || rate_limit::read_bucket_key(client);
    let principal = match resolve(&state, req.headers()) {
        Ok(p) => p,
        Err(refusal) => {
            if let Some(limited) = charge(
                state.read_anonymous_limiter.as_deref(),
                &anonymous_key(),
                metrics::RateLimitScope::ReadAnonymous,
            ) {
                return limited;
            }
            return refusal.into_response();
        }
    };
    let limited = match &principal {
        ReadPrincipal::Did { did, .. } => charge(
            state.read_principal_limiter.as_deref(),
            &format!("did:{did}"),
            metrics::RateLimitScope::ReadPrincipal,
        ),
        ReadPrincipal::Anonymous => charge(
            state.read_anonymous_limiter.as_deref(),
            &anonymous_key(),
            metrics::RateLimitScope::ReadAnonymous,
        ),
    };
    if let Some(limited) = limited {
        return limited;
    }
    req.extensions_mut().insert(principal);
    next.run(req).await
}

/// One unit against `key`; `Some(429)` when over budget. No limiter: admit.
fn charge(
    limiter: Option<&rate_limit::AgentRateLimiter>,
    key: &str,
    scope: metrics::RateLimitScope,
) -> Option<Response> {
    let retry_after_seconds = limiter?.check(key).err()?;
    metrics::record_rate_limit_rejection(scope);
    Some(
        RegistryError::RateLimited {
            retry_after_seconds,
        }
        .into_response(),
    )
}

/// Whether the query has an `access_token` parameter, after percent-decoding
/// (so `access%5Ftoken` counts too).
fn query_names_access_token(uri: &Uri) -> bool {
    uri.query().is_some_and(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .any(|(k, _)| k.eq_ignore_ascii_case("access_token"))
    })
}

/// The request URI as it may appear in logs: with any query parameter that
/// could carry a read token (`access_token`, or a value containing the
/// `acdp_rt_` prefix once decoded) replaced by `[redacted]`, and any path
/// segment containing the prefix likewise. Everything else is unchanged, so
/// ordinary request logs stay useful.
pub fn redact_uri(uri: &Uri) -> String {
    const MASK: &str = "[redacted]";
    let leaks = |s: &str| {
        percent_decode(s)
            .to_ascii_lowercase()
            .contains(acdp_registry_auth::READ_TOKEN_PREFIX)
    };
    let path = uri
        .path()
        .split('/')
        .map(|seg| if leaks(seg) { MASK } else { seg })
        .collect::<Vec<_>>()
        .join("/");
    let Some(query) = uri.query() else {
        return path;
    };
    let query = query
        .split('&')
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let key_hits = percent_decode(k).eq_ignore_ascii_case("access_token");
            if key_hits || leaks(k) || leaks(v) {
                format!("{}={MASK}", if leaks(k) { MASK } else { k })
            } else {
                pair.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{path}?{query}")
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={s}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn redact_uri_masks_token_carrying_parameters_only() {
        let t = "acdp_rt_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        for (input, want) in [
            ("/contexts/search?q=x", "/contexts/search?q=x"),
            (
                &format!("/contexts/search?access_token={t}&q=x") as &str,
                "/contexts/search?access_token=[redacted]&q=x",
            ),
            (
                &format!("/changes?cursor=c&access%5Ftoken={t}"),
                "/changes?cursor=c&access%5Ftoken=[redacted]",
            ),
            (&format!("/changes?x={t}"), "/changes?x=[redacted]"),
            ("/changes?x=ACDP%5FRT%5Fabc", "/changes?x=[redacted]"),
            (&format!("/contexts/{t}"), "/contexts/[redacted]"),
            ("/contexts/acdp%5Frt%5Fzz/body", "/contexts/[redacted]/body"),
            (&format!("/x?{t}"), "/x?[redacted]=[redacted]"),
        ] {
            let got = redact_uri(&uri(input));
            assert_eq!(got, want, "{input}");
            assert!(!got.contains("AAAA"), "{got}");
        }
    }

    #[test]
    fn access_token_query_detection_decodes() {
        assert!(query_names_access_token(&uri("/x?access_token=1")));
        assert!(query_names_access_token(&uri("/x?a=1&ACCESS_TOKEN=2")));
        assert!(query_names_access_token(&uri("/x?access%5Ftoken=2")));
        assert!(!query_names_access_token(&uri("/x?token=1")));
        assert!(!query_names_access_token(&uri("/x")));
        assert!(!query_names_access_token(&uri("/x?q=access_token")));
    }
}
