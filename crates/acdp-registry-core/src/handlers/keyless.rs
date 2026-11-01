//! `POST/GET /key-attestations` — RFC-ACDP-0021 keyless signing.
//!
//! Both methods answer `not_implemented` (501) unless the state carries a
//! [`KeylessState`], which it does only when `keyless.enabled`. That 501 is
//! decided before anything about the request is read, so a registry
//! without the profile answers exactly as it did before this module did
//! anything else (§5.4 and the profile registry's note). `keyless.enabled`
//! is off by default.
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
//!
//! [`publish_keyless`] is `POST /contexts` for a keyless-signed body
//! (§6.3), and [`refuse_keyless_event`] refuses a keyless-signed lifecycle
//! event (R13). `handlers/context.rs` only calls them, and only with the
//! profile on.

use std::sync::Arc;
use std::time::Instant;

use acdp::error::AcdpError;
use acdp::registry::PublishValidator;
use acdp::types::keyless::{
    is_keyless_key_id, token_hash, Jwk, KeyAttestation, KeyAttestationList, KeyAttestationRequest,
    KeylessReason, ListedIdentities, WorkloadIdentity, REQUEST_VERSION,
};
use acdp::types::lifecycle::LifecycleEvent;
use acdp::types::primitives::{CtxId, Status};
use acdp::types::publish::{PublishRequest, PublishResponse};
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
use crate::rate_limit::PublishCharge;
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
    // `X-Forwarded-For` hop the `[rate_limit]` policy trusts.
    if let Some(limiter) = &kl.ip_limiter {
        let peer = req.extensions().get::<PeerIp>().and_then(|p| p.0);
        let ip = state.client_ip(peer, req.headers());
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

// ── §6.3 keyless publish and the lifecycle refusal ─────────────────────────

/// The keyless state when `req` is a keyless-signed publish this registry
/// must check itself (§6.3): the profile is on and `signature.key_id` has
/// an `acdp-kl-` fragment. `None` leaves the publish to the other
/// branches, where the SDK refuses a keyless key id at key resolution
/// (kl-003 J).
pub(crate) fn keyless_publish_state<S: ExtendedRegistryStore>(
    state: &AppState<S>,
    req: &PublishRequest,
) -> Option<Arc<KeylessState>> {
    state
        .keyless
        .clone()
        .filter(|_| is_keyless_key_id(&req.signature.key_id))
}

/// R13 (§6.3 "Lifecycle events"): with the profile on, a lifecycle event
/// whose signing key id is keyless is refused before anything about its
/// target is read, so the refusal is the same whatever the target, and
/// whether or not the identity is org-wide (kl-004 F). With the profile off
/// the SDK refuses it at key resolution, with no reason.
pub(crate) fn refuse_keyless_event<S: ExtendedRegistryStore>(
    state: &AppState<S>,
    event: &LifecycleEvent,
) -> Result<(), RegistryError> {
    if state.keyless.is_none() {
        return Ok(());
    }
    match &event.signature {
        Some(sig) if is_keyless_key_id(&sig.key_id) => Err(RegistryError::with_reason(
            AcdpError::KeyResolution(format!(
                "lifecycle event signed with keyless key id '{}': keyless keys do not sign \
                 RFC-ACDP-0013 events in this version (RFC-ACDP-0021 §6.3)",
                sig.key_id
            )),
            reason::KEYLESS_EVENT_UNSUPPORTED,
        )),
        _ => Ok(()),
    }
}

/// A §6.3 refusal with its registered code and `details.reason`.
fn refuse(r: KeylessReason, message: impl std::fmt::Display) -> RegistryError {
    with_keyless_reason(r.error(message))
}

/// §6.3 step 3, bytes differ: `key_resolution_failed` with
/// `attestation_mismatch`. The SDK maps that reason only to
/// `key_not_authorized` (step 4's code), so it is built here.
fn attestation_bytes_differ(message: impl std::fmt::Display) -> RegistryError {
    RegistryError::with_reason(
        AcdpError::KeyResolution(KeylessReason::AttestationMismatch.tag(message)),
        KeylessReason::AttestationMismatch.as_str(),
    )
}

/// The attested key as the standard-base64 bytes
/// `prove_publish_identity_pinned` fingerprints: the raw 32-byte Ed25519
/// key, or the uncompressed SEC1 P-256 point. `Jwk::fingerprint` and this
/// encoding agree on the RFC-ACDP-0010 §6 fingerprint; the caller asserts it.
fn pinned_key_b64(jwk: &Jwk) -> Result<String, RegistryError> {
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use base64::Engine as _;
    let decode = |s: &str| {
        URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|e| RegistryError::Internal(format!("stored attestation JWK: {e}")))
    };
    let raw = match &jwk.y {
        None => decode(&jwk.x)?,
        Some(y) => {
            let mut sec1 = vec![0x04];
            sec1.extend(decode(&jwk.x)?);
            sec1.extend(decode(y)?);
            sec1
        }
    };
    Ok(STANDARD.encode(raw))
}

/// `POST /contexts` for a keyless-signed body (RFC-ACDP-0021 §6.3), in
/// place of RFC-ACDP-0003 §2.1 step 6's key resolution. Runs, in order:
///
/// 0. the SDK's schema, size and hash validation (kl-003 I is refused here);
/// 1. `attestation_missing`; 2. `attestation_foreign`;
/// 3. `attestation_not_found`, or `attestation_mismatch` (400) when the
///    stored bytes differ. Only indexed attestations count, so the
///    `ctx_id` of some other body cannot be probed;
/// 4. `attestation_mismatch` (403) when `agent_id`, `key_id` or
///    `algorithm` differ;
/// 5. `attestation_withdrawn`;
/// 6. `outside_attestation_window` against the handler's clock. This is
///    provisional: the store repeats it inside its transaction against the
///    `created_at` it assigns, which decides;
/// 7. `identity_not_authorized` against the controller's (cached) DID
///    document;
/// 8. the signature under `public_key_jwk` (`invalid_signature`).
///
/// Then the pinned-key proof, whose receipt and log-leaf fingerprint is the
/// attested `key_fingerprint`, the charge, and the commit. The store's own
/// refusals (withdrawn, §7 `keyless_identity_mismatch`, the window) keep
/// their reasons.
///
/// `charge` is handed back whatever happens, armed only once the proof
/// holds, as on the other proven branches.
pub(crate) async fn publish_keyless<S: ExtendedRegistryStore + 'static>(
    state: &Arc<AppState<S>>,
    kl: &KeylessState,
    req: &PublishRequest,
    idempotency_key: Option<String>,
    tenant: Option<String>,
    charge: PublishCharge,
) -> (
    PublishCharge,
    Result<(bool, PublishResponse), RegistryError>,
) {
    let attested = match check_keyless_publish(state, kl, req).await {
        Ok(a) => a,
        Err(e) => return (charge, Err(e)),
    };
    let pinned = match pinned_key_b64(&attested.public_key_jwk) {
        Ok(p) => p,
        Err(e) => return (charge, Err(e)),
    };
    let server = state.server.clone();
    let req = req.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let mut charge = charge;
        let outcome = (|| {
            let proven = server
                .prove_publish_identity_pinned(&req, &pinned, &attested.algorithm)
                .map_err(RegistryError::Acdp)?;
            if let Some(fp) = proven.key_fingerprint() {
                if fp != attested.key_fingerprint {
                    return Err(RegistryError::Internal(format!(
                        "the pinned fingerprint {fp} of the attested key is not the \
                         attestation's key_fingerprint {}",
                        attested.key_fingerprint
                    )));
                }
            }
            charge.arm();
            server
                .commit_proven(proven, idempotency_key.as_deref(), tenant.as_deref())
                .map(|o| (o.is_replay(), o.into_response()))
                .map_err(with_keyless_reason)
        })();
        (charge, outcome)
    })
    .await;
    match joined {
        Ok(r) => r,
        // The charge moved into the panicked task and fired on unwind.
        Err(e) => (
            PublishCharge::new(None, ""),
            Err(RegistryError::Internal(format!("join: {e}"))),
        ),
    }
}

/// §6.3 steps 0-8 (see [`publish_keyless`]); the attestation on success.
async fn check_keyless_publish<S: ExtendedRegistryStore + 'static>(
    state: &Arc<AppState<S>>,
    kl: &KeylessState,
    req: &PublishRequest,
) -> Result<KeyAttestation, RegistryError> {
    // Step 0: the SDK validator, as `prove_publish_identity_pinned` runs it
    // again before the commit (same raw length, so the same answer).
    let raw_len = serde_json::to_vec(req)
        .map_err(|e| RegistryError::Internal(format!("re-serializing the request: {e}")))?
        .len();
    let validated = PublishValidator::for_authority(
        state.server.capabilities(),
        &state.config.registry.authority,
    )
    .validate_post_schema(req, raw_len)
    .map_err(RegistryError::Acdp)?;

    // Step 1.
    let reference = req.key_attestation.as_ref().ok_or_else(|| {
        refuse(
            KeylessReason::AttestationMissing,
            format!(
                "signature.key_id '{}' is a keyless key id but the body has no key_attestation",
                req.signature.key_id
            ),
        )
    })?;

    // Step 2.
    if reference.ctx_id.authority() != state.config.registry.authority {
        return Err(refuse(
            KeylessReason::AttestationForeign,
            format!(
                "key_attestation '{}' is not on this registry; publish to the registry that \
                 attested the key",
                reference.ctx_id
            ),
        ));
    }

    // Step 3.
    let record = state
        .server
        .store()
        .key_attestation(reference.ctx_id.as_str())
        .await
        .map_err(RegistryError::Acdp)?
        .ok_or_else(|| {
            refuse(
                KeylessReason::AttestationNotFound,
                format!("no key attestation at '{}'", reference.ctx_id),
            )
        })?;
    if record.content_hash != reference.content_hash {
        return Err(attestation_bytes_differ(format!(
            "key_attestation.content_hash {} is not the stored attestation's {}",
            reference.content_hash, record.content_hash
        )));
    }
    let att = record.attestation;

    // Step 4.
    if !att.binds(&req.agent_id, &req.signature) {
        return Err(refuse(
            KeylessReason::AttestationMismatch,
            format!(
                "agent_id, signature.key_id or signature.algorithm differ from key attestation \
                 '{}'",
                reference.ctx_id
            ),
        ));
    }

    // Step 5.
    if record.status == Status::Retracted {
        return Err(refuse(
            KeylessReason::AttestationWithdrawn,
            format!("key attestation '{}' was retracted", reference.ctx_id),
        ));
    }

    // Step 6, provisional (the store decides on the assigned created_at).
    let now_ms = (kl.clock)();
    let now = chrono::DateTime::from_timestamp_millis(now_ms)
        .ok_or_else(|| RegistryError::Internal(format!("keyless clock out of range: {now_ms}")))?;
    if !att.window_contains(now) {
        return Err(refuse(
            KeylessReason::OutsideAttestationWindow,
            format!(
                "the registry's time {} is outside the window of key attestation '{}'",
                now.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                reference.ctx_id
            ),
        ));
    }

    // Step 7 (SHOULD), with the resolver's normal caching.
    let doc = state
        .auth
        .resolver
        .resolve(&att.controller)
        .await
        .map_err(RegistryError::Acdp)?;
    if ListedIdentities::of(&doc).find(&att.identity).is_none() {
        return Err(refuse(
            KeylessReason::IdentityNotAuthorized,
            format!(
                "{} no longer lists the workload identity of key attestation '{}'",
                att.controller, reference.ctx_id
            ),
        ));
    }

    // Step 8: the signature over the recomputed content_hash, under the
    // attested key. No reason: `invalid_signature` alone (kl-003 H).
    att.public_key_jwk
        .verify(
            &req.signature.algorithm,
            &req.signature.value,
            validated.recomputed_hash.as_str(),
        )
        .map_err(|e| match e {
            AcdpError::InvalidSignature(_) => RegistryError::Acdp(e),
            other => RegistryError::Acdp(AcdpError::InvalidSignature(format!(
                "the signature does not verify under the attested key: {other}"
            ))),
        })?;
    Ok(att)
}
