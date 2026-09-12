//! Capabilities + health.

use std::sync::Arc;

use acdp_registry_store::ExtendedRegistryStore;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

/// The running build's identifier, served as `version` on `GET /healthz`
/// and inside the `build` group on `GET /admin/status` (#117).
///
/// Composed rather than read from a single source, because the workspace
/// crates still carry a placeholder `CARGO_PKG_VERSION` (`0.1.0`): the
/// package version alone would not distinguish two builds. CI injects the
/// commit through the `ACDP_BUILD_SHA` build ARG (see `docker/Dockerfile`),
/// giving `0.1.0+g<sha>`; a plain `cargo build` leaves it unset and yields
/// the bare package version, which does NOT uniquely identify a build.
/// `+g<sha>` is SemVer build metadata, so the result stays valid SemVer
/// either way.
///
/// This composes with — rather than being replaced by — a future fix to the
/// release pipeline: once the package version bumps, the same expression
/// emits `<new-version>+g<sha>` with no code change.
///
/// `option_env!` is resolved at compile time, so a cached build layer keeps
/// whatever SHA it was built with; the Dockerfile places the ARG after the
/// dependency-cook layer so the final layer rebuilds each commit.
///
/// Consumers MUST treat this as opaque — display or equality at most, never
/// parsing. See `docs/HTTP-API.md`.
pub(crate) fn build_version() -> String {
    match build_commit() {
        Some(sha) => format!("{}+g{sha}", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// The injected commit, or `None` when this build was not given one.
///
/// **The emptiness check is load-bearing — do not simplify it to a bare
/// `option_env!`.** `docker/Dockerfile` declares `ARG ACDP_BUILD_SHA` and
/// then `ENV ACDP_BUILD_SHA=${ACDP_BUILD_SHA}`. A `docker build` that passes
/// no `--build-arg` sets that variable to the **empty string** rather than
/// leaving it unset, and `option_env!` on a set-but-empty variable returns
/// `Some("")`, not `None`. Without this filter such a build would serve
/// `0.1.0+g` — a version falsely advertising an injected commit while
/// carrying none — and `/admin/status` would report `"commit": ""` instead
/// of omitting the field, destroying the "absence means this build is not
/// uniquely identified" signal the docs tell operators to rely on.
///
/// Not a hypothetical path: `docker/docker-compose.yml` passes only
/// `STORAGE_FEATURE`, and `README.md` documents `docker compose up --build`
/// as the local production path.
pub(crate) fn build_commit() -> Option<&'static str> {
    option_env!("ACDP_BUILD_SHA").filter(|sha| !sha.is_empty())
}

/// `GET /.well-known/acdp.json` — the registry capabilities document.
///
/// RFC-ACDP-0006 §4.2.1: registries SHOULD emit `Cache-Control: max-age=300`
/// or higher. The document changes rarely, so a 5-minute TTL lets clients and
/// CDNs hold it without re-fetching on every discovery probe.
pub async fn capabilities<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
) -> impl IntoResponse {
    let body =
        Json(serde_json::to_value(state.server.capabilities()).unwrap_or_else(|_| json!({})));
    (
        [(axum::http::header::CACHE_CONTROL, "public, max-age=300")],
        body,
    )
}

/// `GET /.well-known/did.json` — the registry's own `did:web` DID document
/// (RFC-ACDP-0010 workstream A1).
///
/// `did:web:<authority>` resolves to exactly this URL, so serving it here
/// makes the registry's receipt verification key discoverable without any
/// out-of-band hosting. The document is precomputed at startup from
/// `[receipt]`: the active signing key sits in both `verificationMethod`
/// and `assertionMethod`; rotated-out keys (`[[receipt.retired_keys]]`)
/// stay in `verificationMethod` forever — removing one bricks every
/// receipt it signed. 404 when no receipt key is configured.
pub async fn registry_did_document<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
) -> impl IntoResponse {
    match &state.registry_did_document {
        Some(doc) => (
            StatusCode::OK,
            [(axum::http::header::CACHE_CONTROL, "public, max-age=300")],
            Json(doc.clone()),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            // The 404 means "no receipt signing key is configured" -- it flips
            // to 200 on an operator action (add a key, restart). A shared cache
            // heuristically caching a directive-less 404 would mask the newly
            // available document from every resolver that saw the miss, and the
            // operator has no way to observe or flush it.
            //
            // `no-store` rather than a short `max-age`: the 404 carries no ETag
            // to revalidate against, so `max-age=0, must-revalidate` would be
            // `no-store` with extra ceremony plus a shared-cache-storable copy;
            // and nothing polls a missing DID document at a volume a TTL would
            // relieve. Same reasoning `/healthz` already applies below --
            // staleness here is the failure this endpoint exists to rule out.
            //
            // Set in the handler, not by a layer: the `aux` group also carries
            // the two well-known documents that set their own
            // `public, max-age=300`.
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(json!({
                "error": "not_found",
                "message":
                    "this registry serves no DID document (no receipt signing key configured)",
            })),
        )
            .into_response(),
    }
}

/// `GET /.well-known/jwks.json` — publish the public key(s) federated
/// peers should use to verify tokens issued by this registry.
///
/// Returns:
///   - EdDSA: `{ keys: [<OKP/Ed25519 JWK>] }`
///   - HS256: `{ keys: [] }` (symmetric secrets are never published)
///
/// `Cache-Control: public, max-age=300` matches the typical JWKS-client
/// cache TTL so peers can hold the response without hammering the registry.
pub async fn jwks<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
) -> impl IntoResponse {
    let body = Json(state.auth.signer.jwks());
    (
        [
            (axum::http::header::CACHE_CONTROL, "public, max-age=300"),
            (axum::http::header::CONTENT_TYPE, "application/jwk-set+json"),
        ],
        body,
    )
}

/// `GET /livez` — process liveness. Always 200, never touches storage.
///
/// The split from `/healthz` is the entire point. `/healthz` reports storage
/// READINESS and returns 503 during a DB outage (below), which is correct for a
/// load balancer and catastrophic for a Kubernetes `livenessProbe`: the kubelet
/// restarts a process that is perfectly alive and cannot fix the database by
/// restarting, and each restart discards the in-memory webhook queue (bounded at
/// `webhook.queue_capacity`, no outbox, no replay).
///
/// Nothing in this repo wires `/healthz` as a liveness probe today — there is no
/// `HEALTHCHECK` in the Dockerfile and no k8s manifests — so this is a latent
/// trap rather than a live bug. The push toward it is prose: `docker/RAILWAY.md`
/// tells operators to point Railway's healthcheck at `/healthz` and says nothing
/// about the 503 arm.
///
/// `no-store` from the handler, for the same reason `/healthz` is: a cached 200
/// from a dead process is precisely the failure a liveness probe exists to rule
/// out. Set here rather than by a layer because `aux` also carries the two
/// well-known documents that set their own `public, max-age=300`.
pub async fn livez() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(json!({ "status": "ok", "version": build_version() })),
    )
}

/// BUG-05: returns HTTP 503 when storage health fails so load balancers,
/// Kubernetes readiness probes, and Prometheus blackbox exporters take the
/// pod out of rotation. Returning 200 + `"status":"degraded"` (the prior
/// behaviour) left the registry serving requests it could not satisfy.
pub async fn health<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
) -> impl IntoResponse {
    let storage_ok = state.server.store().health().await.is_ok();
    // `version` rides the degraded/503 body too: build identity matters most
    // when the service is unhealthy, and `acdp-control-plane` sets the same
    // precedent (its health tests pin `version` on the DB-failure path).
    let body = Json(json!({
        "status": if storage_ok { "ok" } else { "degraded" },
        "storage": storage_ok,
        "version": build_version(),
    }));
    let status = if storage_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    // #205: never cacheable, on BOTH arms. A cached "degraded" masks a recovery
    // and a cached "ok" masks an outage; staleness here is the failure mode the
    // endpoint exists to rule out. Set in the handler rather than by a layer
    // because `/healthz` shares the `aux` group with the two well-known
    // documents (which set their own `public, max-age=300`) and `/metrics`.
    (
        status,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        body,
    )
}
