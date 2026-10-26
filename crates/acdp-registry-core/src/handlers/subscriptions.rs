//! `POST/GET/DELETE /subscriptions[/{id}]` — RFC-ACDP-0018 §8.1 change-hint
//! subscriptions.
//!
//! Every request is authenticated and owner-scoped: a subscription belongs
//! to the requester DID that created it, and another requester's id is
//! `not_found`, never `not_authorized`, so ids disclose nothing. A malformed
//! id is `not_found` for the same reason.
//!
//! The secret is generated here, returned once in the `201`, and stored
//! only sealed under the changes key with the subscription id as AAD.

use std::sync::Arc;

use acdp::error::AcdpError;
use acdp::types::change::{Subscription, SubscriptionRequest, SubscriptionStatus, WatchFilter};
use acdp_registry_store::subscriptions::{CreateSubscriptionOutcome, NewSubscription};
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryError;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use ring::rand::SecureRandom as _;
use serde_json::Value;
use zeroize::Zeroize as _;

use crate::extract::AcdpBytes;
use crate::handlers::context::{caller_from_headers, tenant_for_request};
use crate::state::AppState;
use crate::tenant_trust::PeerIp;

/// Random bytes in a subscription secret (§8.1: at least 32).
const SECRET_BYTES: usize = 32;

/// The `details.field` named when the owner is at
/// `limits.max_subscriptions_per_requester`.
const LIMIT_FIELD: &str = "limits.max_subscriptions_per_requester";

fn not_implemented() -> RegistryError {
    RegistryError::Acdp(AcdpError::NotImplemented(
        "this registry does not advertise acdp-registry-events \
         (RFC-ACDP-0018 §8: /subscriptions is not implemented)"
            .into(),
    ))
}

fn not_found() -> RegistryError {
    RegistryError::Acdp(AcdpError::NotFound("subscription not found".into()))
}

fn field(field: &'static str, message: impl Into<String>) -> RegistryError {
    RegistryError::FieldViolation {
        field,
        message: message.into(),
    }
}

/// The authenticated owner, or `not_authorized` (403) for an anonymous
/// request (§8.1).
fn owner<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    headers: &HeaderMap,
) -> Result<String, RegistryError> {
    caller_from_headers(state, headers)?
        .map(|did| did.as_str().to_owned())
        .ok_or_else(|| {
            RegistryError::Acdp(AcdpError::NotAuthorized(
                "subscriptions require an authenticated requester (RFC-ACDP-0018 §8.1)".into(),
            ))
        })
}

/// Parse and validate a `POST /subscriptions` body, naming the member that
/// failed in `details.field` (§8.1). The §8.3 network checks come after.
pub(crate) fn parse_subscription_request(
    body: &[u8],
    max_change_filters: u64,
) -> Result<SubscriptionRequest, RegistryError> {
    let value: Value = serde_json::from_slice(body).map_err(|e| {
        RegistryError::Acdp(AcdpError::SchemaViolation(format!(
            "subscription request is not JSON: {e}"
        )))
    })?;
    let obj = value.as_object().ok_or_else(|| {
        RegistryError::Acdp(AcdpError::SchemaViolation(
            "subscription request is not a JSON object".into(),
        ))
    })?;
    let filter = obj
        .get("filter")
        .ok_or_else(|| field("filter", "filter is required"))?;
    if let Some(members) = filter.as_object() {
        for (k, v) in members {
            if v.as_array().is_some_and(Vec::is_empty) {
                return Err(field("filter", format!("filter.{k} is empty (minItems 1)")));
            }
        }
    }
    let parsed: WatchFilter = serde_json::from_value(filter.clone())
        .map_err(|e| field("filter", format!("filter does not parse: {e}")))?;
    parsed
        .validate_for_subscription(Some(max_change_filters))
        .map_err(|e| field("filter", e.to_string()))?;
    let url = obj
        .get("callback_url")
        .ok_or_else(|| field("callback_url", "callback_url is required"))?;
    if !url.is_string() {
        return Err(field("callback_url", "callback_url is not a string"));
    }
    // Whatever is left (an unknown member, a callback_url outside the
    // schema) is the closed schema's own refusal.
    let req = SubscriptionRequest::from_value(&value).map_err(|e| match e {
        AcdpError::SchemaViolation(m) if m.contains("callback_url") => field("callback_url", m),
        other => RegistryError::Acdp(other),
    })?;
    req.validate(Some(max_change_filters))
        .map_err(|e| field("callback_url", e.to_string()))?;
    Ok(req)
}

/// The §8.3 creation check: `https`, no IP literal, and every address the
/// host resolves to is public unicast. Any failure is `schema_violation`
/// naming `callback_url`.
///
/// A URL-shape refusal says why. A resolution refusal does not: one
/// message covers "does not resolve", "lookup failed" and "resolves to a
/// forbidden address", so the registry's resolver cannot be used to map
/// internal names to addresses. The detail goes to the log.
pub(crate) async fn check_callback<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    url: &str,
) -> Result<(), RegistryError> {
    let refuse = |why: String| field("callback_url", format!("callback_url refused: {why}"));
    let policy = &state.callback_policy;
    policy.check_url(url).map_err(|e| refuse(e.to_string()))?;
    let parsed = url::Url::parse(url).map_err(|e| refuse(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| refuse("no host".into()))?
        .to_owned();
    let port = parsed.port_or_known_default().unwrap_or(443);
    crate::callback::resolve_allowed(policy, state.callback_resolver.as_ref(), &host, port)
        .await
        .map(drop)
        .map_err(|detail| {
            tracing::info!(%host, %detail, "subscription callback_url refused at resolution");
            refuse(UNRESOLVABLE.into())
        })
}

/// The one message for every resolution-time refusal (see [`check_callback`]).
pub(crate) const UNRESOLVABLE: &str =
    "its host must resolve, and resolve only to public unicast addresses";

fn new_secret() -> Result<String, RegistryError> {
    let mut raw = [0u8; SECRET_BYTES];
    ring::rand::SystemRandom::new()
        .fill(&mut raw)
        .map_err(|_| RegistryError::Internal("subscription secret: RNG failure".into()))?;
    let secret = base64::engine::general_purpose::STANDARD.encode(raw);
    raw.zeroize();
    Ok(secret)
}

/// Count one request by its outcome: `ok` or the wire code it failed with.
fn recorded<T>(op: &'static str, result: Result<T, RegistryError>) -> Result<T, RegistryError> {
    let outcome = match &result {
        Ok(_) => "ok",
        Err(e) => e.wire_code(),
    };
    crate::metrics::record_subscription_request(op, outcome);
    result
}

/// `POST /subscriptions` (§8.1).
pub async fn create_subscription<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    headers: HeaderMap,
    PeerIp(peer): PeerIp,
    AcdpBytes(body): AcdpBytes,
) -> Result<Response, RegistryError> {
    recorded("create", create(&state, &headers, peer, &body).await)
}

async fn create<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    headers: &HeaderMap,
    peer: Option<std::net::IpAddr>,
    body: &[u8],
) -> Result<Response, RegistryError> {
    let key = state.events_key().ok_or_else(not_implemented)?;
    let owner = owner(state, headers)?;
    let tenant = tenant_for_request(state, headers, peer)?;
    let req = parse_subscription_request(body, state.config.changes.max_change_filters)?;
    check_callback(state, &req.callback_url).await?;

    let store = state.server.store();
    let subscription_id = uuid::Uuid::new_v4().to_string();
    let mut secret = new_secret()?;
    let sealed = key
        .seal_secret(&subscription_id, &secret)
        .map_err(|e| RegistryError::Internal(format!("subscription secret: {e}")));
    let sealed = match sealed {
        Ok(s) => s,
        Err(e) => {
            secret.zeroize();
            return Err(e);
        }
    };
    let now = chrono::Utc::now();
    let created_at_ms = now.timestamp_millis();
    let head = store.changes_head().await?.head;
    let new = NewSubscription {
        subscription_id: subscription_id.clone(),
        owner_did: owner,
        tenant_id: tenant,
        filter: req.filter.clone(),
        callback_url: req.callback_url.clone(),
        secret_ct: sealed,
        secret_key_id: key.key_id(),
        created_at_ms,
        last_hinted_seq: head,
    };
    let max = state.config.events.max_subscriptions_per_requester;
    match store.create_subscription(&new, max).await {
        Ok(CreateSubscriptionOutcome::Created) => {}
        Ok(CreateSubscriptionOutcome::LimitReached) => {
            secret.zeroize();
            return Err(field(
                LIMIT_FIELD,
                format!(
                    "the requester already holds limits.max_subscriptions_per_requester \
                     ({max}) subscriptions (RFC-ACDP-0018 §8.1)"
                ),
            ));
        }
        Err(e) => {
            secret.zeroize();
            return Err(e.into());
        }
    }
    let created_at = chrono::DateTime::from_timestamp_millis(created_at_ms).unwrap_or(now);
    let sub = Subscription {
        subscription_id,
        filter: req.filter,
        callback_url: req.callback_url,
        created_at,
        status: SubscriptionStatus::Active,
        secret: Some(std::mem::take(&mut secret)),
    };
    // Serialize, then wipe the struct's copy before it drops. The response
    // body necessarily carries the secret once (§8.1).
    let body = serde_json::to_vec(&sub);
    let mut sub = sub;
    if let Some(s) = sub.secret.as_mut() {
        s.zeroize();
    }
    let body = body.map_err(|e| RegistryError::Internal(format!("subscription: {e}")))?;
    Ok((
        StatusCode::CREATED,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        body,
    )
        .into_response())
}

/// `GET /subscriptions/{id}` (§8.1): the owner's subscription without its
/// secret; anything else is `not_found`.
pub async fn get_subscription<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Subscription>, RegistryError> {
    let result = async {
        state.events_key().ok_or_else(not_implemented)?;
        let owner = owner(&state, &headers)?;
        let record = state
            .server
            .store()
            .get_subscription(&owner, &id)
            .await?
            .ok_or_else(not_found)?;
        Ok(Json(record.to_subscription()?))
    }
    .await;
    recorded("get", result)
}

/// `DELETE /subscriptions/{id}` (§8.1): `204` when the owner's subscription
/// was removed, else `not_found`.
pub async fn delete_subscription<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, RegistryError> {
    let result = async {
        state.events_key().ok_or_else(not_implemented)?;
        let owner = owner(&state, &headers)?;
        if state
            .server
            .store()
            .delete_subscription(&owner, &id)
            .await?
        {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(not_found())
        }
    }
    .await;
    recorded("delete", result)
}
