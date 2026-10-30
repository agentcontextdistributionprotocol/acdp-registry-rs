//! `POST/GET /key-attestations` — RFC-ACDP-0021 keyless signing.
//!
//! Both methods answer `not_implemented` (501) unless the state carries a
//! [`KeylessState`], which it does only when `keyless.enabled`. That 501 is
//! decided before anything about the request is read, so a registry
//! without the profile answers exactly as it did before this module did
//! anything else (§5.4 and the profile registry's note). Startup still
//! refuses `keyless.enabled = true`, so in this release only in-process
//! harnesses reach the rest.
//!
//! `POST` runs §5.2 in order and stops at the first failure:
//!
//! 1. the per-IP budget (charged on arrival), the media type and body
//!    size, the closed request schema, then a read-only peek at the
//!    identity's budget;
//! 2. and 3. issuer acceptance and the token ([`crate::keyless::verify`]);
//!    then the identity's budget is charged (only a caller holding a token
//!    that verifies for the identity can spend it) and the token is checked
//!    for single use;
//! 4. and 5. the proof of possession and the algorithm ([`mint`]);
//! 6. the controller's DID document lists the identity, re-fetched once
//!    past the cache before refusing;
//! 7. and 8. the window, then the attestation is published as an ordinary
//!    untenanted body under the registry DID through `commit_proven`, which
//!    writes it, its receipt, its log leaf and its index row in one
//!    transaction (and refuses a token that raced us to it).
//!
//! The token is never logged and never stored: `token_hash` is its trace.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Instant;

use acdp::error::AcdpError;
use acdp::types::keyless::{
    token_hash, KeyAttestationList, KeyAttestationRequest, KeylessReason, ListedIdentities,
    WorkloadIdentity, REQUEST_VERSION,
};
use acdp::types::primitives::CtxId;
use acdp_registry_store::{ExtendedRegistryStore, KeyAttestationFilter};
use acdp_registry_types::RegistryError;
use axum::extract::{FromRequest, FromRequestParts, Request, State};
use axum::http::header::LOCATION;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::extract::{AcdpBytes, AcdpQuery};
use crate::keyless::{mint, reason, with_keyless_reason, KeylessState};
use crate::metrics::{record_rate_limit_rejection, RateLimitScope};
use crate::state::AppState;
use crate::tenant_trust::PeerIp;

/// The list page size when none is asked for, and the largest honoured:
/// search's (RFC-ACDP-0005, §5.4 "paginated like search").
const LIST_LIMIT_DEFAULT: u32 = 20;
const LIST_LIMIT_MAX: u32 = 100;

/// The 501 a registry not advertising the profile answers (§5.4).
fn not_implemented() -> Response {
    RegistryError::Acdp(AcdpError::NotImplemented(
        "this registry does not advertise acdp-registry-keyless \
         (RFC-ACDP-0021: /key-attestations is not implemented)"
            .into(),
    ))
    .into_response()
}

/// `POST /key-attestations` (RFC-ACDP-0021 §5.1-§5.3).
pub async fn create_key_attestation<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    req: Request,
) -> Response {
    let Some(kl) = state.keyless.clone() else {
        return not_implemented();
    };
    match mint_attestation(&state, &kl, req).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// `GET /key-attestations?controller=<did>` (RFC-ACDP-0021 §5.4).
pub async fn list_key_attestations<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    req: Request,
) -> Response {
    if state.keyless.is_none() {
        return not_implemented();
    }
    let (mut parts, _) = req.into_parts();
    let q = match AcdpQuery::<ListQuery>::from_request_parts(&mut parts, &()).await {
        Ok(AcdpQuery(q)) => q,
        Err(rej) => return rej.into_response(),
    };
    match list(&state, q).await {
        Ok(page) => Json(page).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    controller: Option<String>,
    issuer: Option<String>,
    subject: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    q: ListQuery,
) -> Result<KeyAttestationList, RegistryError> {
    let schema = |m: String| RegistryError::Acdp(AcdpError::SchemaViolation(m));
    let controller = q
        .controller
        .ok_or_else(|| schema("the controller query parameter is required".into()))?;
    if !controller.starts_with("did:web:") || acdp::did::did_web_to_url(&controller).is_err() {
        return Err(schema(format!(
            "controller '{controller}' is not a did:web DID"
        )));
    }
    let filter = KeyAttestationFilter {
        issuer: q.issuer,
        subject: q.subject,
    };
    let limit = q
        .limit
        .unwrap_or(LIST_LIMIT_DEFAULT)
        .clamp(1, LIST_LIMIT_MAX);
    let page = state
        .server
        .store()
        .list_key_attestations(&controller, &filter, q.cursor.as_deref(), limit)
        .await?;
    Ok(KeyAttestationList {
        items: page.items,
        next_cursor: page.next_cursor,
    })
}

/// A schema refusal that never echoes a value from the body: serde names
/// unknown and missing members, but its other messages can quote what it
/// read, and that may be the token.
fn parse_error(e: &serde_json::Error) -> RegistryError {
    let m = e.to_string();
    let message = if m.starts_with("unknown field") || m.starts_with("missing field") {
        format!("not an {REQUEST_VERSION} body: {m}")
    } else {
        format!(
            "not an {REQUEST_VERSION} body (line {}, column {})",
            e.line(),
            e.column()
        )
    };
    RegistryError::Acdp(AcdpError::SchemaViolation(message))
}

/// The per-identity limiter key: one bucket per `(type, issuer, subject)`.
fn identity_key(id: &WorkloadIdentity) -> String {
    match id {
        WorkloadIdentity::Oidc { issuer, subject } => format!("id:oidc|{issuer}|{subject}"),
        WorkloadIdentity::Spiffe { subject } => format!("id:spiffe||{subject}"),
    }
}

fn rate_limited(scope: RateLimitScope, retry_after_seconds: u64) -> RegistryError {
    record_rate_limit_rejection(scope);
    RegistryError::RateLimited {
        retry_after_seconds,
    }
}

async fn mint_attestation<S: ExtendedRegistryStore + 'static>(
    state: &Arc<AppState<S>>,
    kl: &KeylessState,
    req: Request,
) -> Result<Response, RegistryError> {
    // The forced DID re-fetch (step 6) is measured against this instant.
    let started = Instant::now();

    // Step 1: the per-IP budget, charged on arrival, before the body is
    // read. The client IP is the auth limiter's: the TCP peer, or the
    // trusted proxy chain's `X-Forwarded-For`.
    if let Some(limiter) = &kl.ip_limiter {
        let peer = req
            .extensions()
            .get::<PeerIp>()
            .and_then(|p| p.0)
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let xff = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());
        let ip = crate::rate_limit::client_ip(peer, xff, &state.trusted_proxies);
        limiter
            .check(&format!("ip:{ip}"))
            .map_err(|s| rate_limited(RateLimitScope::KeylessPerIp, s))?;
    }

    // Step 1: media type, size and the closed schema (§5.1). The request
    // carries no HTTP signature and needs none: the token authenticates it.
    let body = match AcdpBytes::from_request(req, &()).await {
        Ok(AcdpBytes(b)) => b,
        Err(rej) => return Ok(rej.into_response()),
    };
    let request: KeyAttestationRequest =
        serde_json::from_slice(&body).map_err(|e| parse_error(&e))?;
    request.validate().map_err(RegistryError::Acdp)?;
    let identity = request.identity.identity();
    let id_key = identity_key(&identity);
    if let Some(limiter) = &kl.identity_limiter {
        limiter
            .peek(&id_key)
            .map_err(|s| rate_limited(RateLimitScope::KeylessPerIdentity, s))?;
    }

    // Steps 2 and 3.
    let now_s = (kl.clock)().div_euclid(1000);
    let verified = crate::keyless::verify(
        &state.config.keyless,
        &state.keyless_jwks,
        &request.identity,
        &kl.registry_did,
        &request.public_key_jwk,
        now_s,
    )
    .await
    .map_err(crate::keyless::KeylessRefusal::into_error)?;
    if let Some(limiter) = &kl.identity_limiter {
        limiter.record(&id_key);
    }
    debug_assert_eq!(verified.token_hash, token_hash(request.identity.token()));
    let store = state.server.store();
    if store
        .key_attestation_token_seen(&verified.token_hash)
        .await?
    {
        return Err(RegistryError::with_reason(
            acdp_registry_store::key_attestations::token_replayed(&verified.token_hash),
            reason::TOKEN_REPLAYED,
        ));
    }
    tracing::debug!(
        token_hash = %verified.token_hash,
        controller = %request.controller,
        "key attestation request: token verified"
    );

    // Steps 4 and 5.
    let algorithm = mint::verify_pop(&request, &kl.registry_did)?;
    mint::check_algorithm(
        &request,
        algorithm,
        &state.server.capabilities().supported_signature_algorithms,
    )?;

    // Step 6: a listed identity proceeds whether or not the document was
    // re-fetched; an unlisted one is final only against a fresh document.
    let resolution = state
        .auth
        .resolver
        .resolve_until(&request.controller, started, |doc| {
            ListedIdentities::of(doc).find(&identity).is_some()
        })
        .await
        .map_err(RegistryError::Acdp)?;
    let listed = ListedIdentities::of(&resolution.document);
    let entry = match listed.find(&identity) {
        Some(entry) => entry.clone(),
        None if resolution.fresh => {
            return Err(RegistryError::with_reason(
                KeylessReason::IdentityNotAuthorized.error(format!(
                    "{} does not list this workload identity in its AcdpKeylessSigning service",
                    request.controller
                )),
                reason::IDENTITY_NOT_AUTHORIZED,
            ))
        }
        None => {
            return Err(RegistryError::Acdp(AcdpError::KeyResolutionUnreachable(
                format!(
                    "{} does not list this workload identity in a cached DID document, and \
                     re-fetching it is rate-limited; retry later",
                    request.controller
                ),
            )))
        }
    };

    // Step 7.
    let window = mint::window(
        (kl.clock)(),
        request.requested_ttl_seconds,
        state.config.keyless.max_attestation_ttl_seconds,
        verified.exp,
    )
    .map_err(crate::keyless::KeylessRefusal::into_error)?;
    let attestation = mint::attestation(&request, algorithm, &entry, window, verified.token_hash)
        .map_err(RegistryError::Acdp)?;

    // Step 8: the normal publish path, untenanted (plan D18), with no
    // idempotency key: a second request is refused as a replay, never
    // answered from a stored response.
    let publish = mint::build(
        kl.signing_key(),
        &kl.key_id,
        &state.config.registry.authority,
        &attestation,
    )
    .map_err(|e| RegistryError::Internal(format!("building the key attestation: {e}")))?;
    let server = state.server.clone();
    let pinned = kl.public_key_b64.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let proven = server.prove_publish_identity_pinned(&publish, &pinned, "ed25519")?;
        server.commit_proven(proven, None, None)
    })
    .await
    .map_err(|e| RegistryError::Internal(format!("join: {e}")))?
    .map_err(with_keyless_reason)?;
    let response = outcome.into_response();
    crate::metrics::record_receipt_minted();
    if state.log.is_some() {
        crate::metrics::record_log_leaf();
    }

    // The full retrieval representation (§5.2 step 8, RFC-ACDP-0004).
    let server = state.server.clone();
    let ctx_id = CtxId(response.ctx_id.as_str().to_string());
    let lookup = ctx_id.clone();
    let full = tokio::task::spawn_blocking(move || server.retrieve(&lookup, None))
        .await
        .map_err(|e| RegistryError::Internal(format!("join: {e}")))??
        .ok_or_else(|| {
            RegistryError::Internal(format!(
                "minted key attestation '{ctx_id}' is not retrievable"
            ))
        })?;
    tracing::info!(
        ctx_id = %ctx_id,
        controller = %attestation.controller,
        token_hash = %attestation.token_hash,
        "key attestation minted"
    );
    let location = HeaderValue::from_str(&crate::publish_reply::location_for(ctx_id.as_str()))
        .map_err(|e| RegistryError::Internal(format!("key attestation Location header: {e}")))?;
    Ok((StatusCode::CREATED, [(LOCATION, location)], Json(full)).into_response())
}
