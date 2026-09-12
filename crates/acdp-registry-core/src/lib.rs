//! axum router + handlers for `acdp-registry-rs`.
//!
//! Storage is injected via the type parameter `S: ExtendedRegistryStore`.
//! `acdp-registry-core` itself has no compile-time dependency on a
//! specific storage crate — the binary picks one via Cargo features.

pub mod handlers;
pub mod log;
pub mod metrics;
pub mod playground;
pub mod rate_limit;
pub mod receipt;
pub(crate) mod secure_compare;
pub mod state;
pub mod witness;

pub use state::{AppState, AppStateInner};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryError;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, map_response, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

/// Build the registry HTTP router.
///
/// The returned router carries no global timeout for streaming endpoints —
/// upstream operators terminating TLS can layer in their own caps. The
/// `/auth/*` endpoints are mounted only when `cfg.auth.enabled` so a
/// registry running without auth doesn't advertise a token-mint endpoint
/// it can't enforce.
pub fn build_router<S: ExtendedRegistryStore + 'static>(state: AppState<S>) -> Router {
    // FEAT-06/FEAT-10: `from_fn_with_state` middleware (the `/auth/*` limiter)
    // and the `/metrics` route both need the shared state up front, so build
    // the `Arc` here rather than only at `.with_state` time.
    let state = Arc::new(state);
    let admin = admin_router::<S>();
    let auth_enabled = state.config.auth.enabled;
    let metrics_enabled = state.metrics.is_some();
    let body_limit = state.config.limits.max_payload_bytes;
    let cors = build_cors_layer(&state.config.registry.cors.allowed_origins);

    // ACDP data + capabilities + auth endpoints. RFC-ACDP-0007 §4 requires
    // `application/acdp+json` on EVERY response from these endpoints (success
    // bodies and error envelopes alike), so they are grouped under a
    // response-header layer that sets the media type. JWKS, health, and the
    // operational admin routes keep their conventional media types and are
    // mounted separately below.
    // #205: the requester-relative data plane, grouped so the cache posture
    // below lands on exactly these routes and nothing else. Membership of this
    // group IS the posture -- a route added to `acdp` instead of `data` gets no
    // cache directive, and nothing about the code will look wrong.
    //
    // `every_route_in_the_core_router_is_classified` in `http_integration.rs`
    // is what notices: it scans THIS FILE's `.route(...)` literals and fails on
    // any path it cannot classify, so a new route must be placed in a posture
    // group (or explicitly exempted) before the suite goes green. A round-trip
    // assertion in the same test pins this block's membership against
    // `DATA_PLANE_ROUTES` in both directions.
    let data = Router::new()
        // Contexts
        .route("/contexts", post(handlers::publish::<S>))
        .route("/contexts/search", get(handlers::search::<S>))
        .route("/contexts/{ctx_id}", get(handlers::retrieve::<S>))
        .route("/contexts/{ctx_id}/body", get(handlers::retrieve_body::<S>))
        // Lifecycle events & retraction (RFC-ACDP-0013 §6). Always
        // mounted: a registry not advertising `acdp-registry-lifecycle`
        // answers 501 not_implemented from the handler, per §6.
        .route("/contexts/{ctx_id}/retract", post(handlers::retract::<S>))
        .route(
            "/contexts/{ctx_id}/republish",
            post(handlers::republish::<S>),
        )
        // Lineages
        .route("/lineages/{lineage_id}", get(handlers::lineage::<S>))
        .route(
            "/lineages/{lineage_id}/current",
            get(handlers::current::<S>),
        )
        // Transparency log (RFC-ACDP-0012 §8). Always mounted: a
        // registry not advertising `acdp-registry-transparency-log`
        // answers 501 not_implemented from the handler (the lifecycle
        // posture); there is never a `log_unavailable` (§7.1).
        .route("/log/checkpoint", get(handlers::log_checkpoint::<S>))
        .route("/log/proof", get(handlers::log_proof::<S>))
        .route("/log/entries", get(handlers::log_entries::<S>))
        // #205 (wire half of #190). These responses depend on WHO is asking --
        // §4.5 visibility, tenant scoping, and the per-requester leaf echo on
        // `/log/*` -- so a shared cache must never reuse one requester's copy
        // for another.
        //
        // `private` rather than `no-store`: the threat is shared caches, which
        // `private` excludes exactly. `no-store` would only add protection
        // against caches that ignore directives (they ignore `no-store` too)
        // while permanently forbidding legitimate same-requester caching.
        //
        // `if_not_present` so a handler can still override per route -- that is
        // also what leaves `/log/checkpoint` (hash-only, requester-invariant, and
        // currently inheriting `private` it never needed) an explicit public TTL
        // later without touching this layer.
        //
        // `route_layer`, not `layer`: the router fallback must stay bare. Note
        // this DOES cover the 405 arm and handler-produced 4xx on matched routes
        // -- deliberate: a 404 whose existence depends on the caller's
        // visibility is precisely what `private` is for.
        .route_layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("private"),
        ))
        // Both axes, not just `authorization`: `tenant_for_request` honours
        // `x-tenant-id`, and with `auth.enabled = false` it is the ONLY tenant
        // signal (`handlers/context.rs`). `Vary: Authorization` alone would be
        // semantically wrong here. `appending` so a future handler-set `Vary`
        // survives -- CorsLayer is the outer layer and appends on its own, so it
        // is not at risk either way.
        //
        // Vary is the SECONDARY line. `private` carries the guarantee; shared
        // caches with overridden or buggy Vary handling are common.
        .route_layer(SetResponseHeaderLayer::appending(
            axum::http::header::VARY,
            HeaderValue::from_static("authorization, x-tenant-id"),
        ));

    // Capabilities is deliberately OUTSIDE `data`: it is requester-invariant and
    // sets its own `Cache-Control: public, max-age=300` (`handlers/meta.rs`).
    // `if_not_present` would have spared the header anyway, but `appending` on
    // Vary has no such guard and would fragment CDN caching of the discovery
    // document by `Authorization`.
    let mut acdp = Router::new()
        .route("/.well-known/acdp.json", get(handlers::capabilities::<S>))
        .merge(data);

    if auth_enabled {
        // FEAT-06: the `/auth/*` endpoints are the most attacker-controllable
        // surface (token issuance / refresh / revoke: unauthenticated writes,
        // RNG, DID-document fetches, Ed25519 verifies). Group them in their
        // own subrouter carrying the per-IP + process-global limiter as a
        // `route_layer` — it fires only on these matched routes, not on 404s
        // or any other endpoint. The per-agent `[limits]` budgets still apply
        // inside the handlers on top of this.
        let auth = Router::new()
            .route("/auth/challenge", post(handlers::issue_challenge::<S>))
            .route("/auth/token", post(handlers::issue_token::<S>))
            .route("/auth/token/revoke", post(handlers::revoke_token::<S>))
            .route_layer(from_fn_with_state(state.clone(), auth_rate_limit::<S>))
            // #205: credential responses must never be stored. RFC 6749 §5.1.
            //
            // Chained AFTER the limiter deliberately -- each successive
            // `route_layer` wraps the current endpoint, so the LATER call is the
            // OUTER one. Outside the limiter, this also covers
            // `auth_rate_limit`'s early-return 429; inside it, the 429 would
            // bypass the header entirely.
            //
            // These routes are NOT members of the `data` group above, so they
            // receive no data-plane posture at all -- `no-store` is their only
            // cache directive. There is no outer `if_not_present` to fall back
            // on; do not assume one exists.
            .route_layer(SetResponseHeaderLayer::overriding(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            ));
        acdp = acdp.merge(auth);
    }

    let acdp = acdp.layer(SetResponseHeaderLayer::overriding(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/acdp+json"),
    ));

    // Non-ACDP endpoints: JWKS sets `application/jwk-set+json` itself, health
    // and admin/status are operational JSON — none get the acdp+json override.
    let aux = Router::new()
        .route("/.well-known/jwks.json", get(handlers::jwks::<S>))
        // The registry's own did:web document (receipt verification keys,
        // RFC-ACDP-0010). Conventional application/json — DID resolvers
        // don't expect the acdp+json media type here.
        .route(
            "/.well-known/did.json",
            get(handlers::registry_did_document::<S>),
        )
        .route("/healthz", get(handlers::health::<S>))
        // Storage READINESS is `/healthz` (503 when the store is down).
        // Process LIVENESS is `/livez` (always 200). Wiring a k8s
        // livenessProbe at `/healthz` restart-loops healthy pods through a DB
        // outage and discards the in-memory webhook queue each cycle.
        .route("/livez", get(handlers::livez));

    // #205: operational internals behind an admin token -- never cacheable.
    // Grouped rather than layered onto `aux` wholesale, because `aux` also
    // carries `/.well-known/jwks.json` and `/.well-known/did.json` (which set
    // their own `public, max-age=300`) and `/metrics` (out of scope for #205).
    let admin_ops = Router::new()
        // Admin status (auth-gated by auth.admin_tokens; ships in every build)
        .route("/admin/status", get(handlers::admin_status::<S>))
        // Full lineage walk as an on-demand integrity audit (D3) — the
        // publish path anchors on the immediate predecessor; this is where
        // the complete chain is still re-checked.
        .route(
            "/admin/lineages/{lineage_id}/audit",
            get(handlers::lineage_audit::<S>),
        )
        // Registry-attested lifecycle (RFC-ACDP-0013 §6 registry-initiated
        // events): admin-gated retract/republish attributed to the
        // registry's own DID — the policy/legal takedown lever for when a
        // producer is unavailable. Auth-gated by `auth.admin_tokens`, same as
        // `/admin/status` and `/admin/lineages/{id}/audit` above — and the
        // same as `GET /admin/contexts` (in `admin_router` below): every
        // `/admin/*` route checks `auth.admin_tokens`. Requires
        // `[lifecycle] enabled` (501 otherwise). Ships in every build.
        .route(
            "/admin/contexts/{ctx_id}/retract",
            post(handlers::admin_retract::<S>),
        )
        .route(
            "/admin/contexts/{ctx_id}/republish",
            post(handlers::admin_republish::<S>),
        )
        // `overriding`, not `if_not_present`: `docs/HTTP-API.md` states flatly
        // that every `/admin/*` response is `no-store`, and `if_not_present`
        // would make that "no-store unless some future handler set its own" --
        // an overstated guarantee is the #190 defect. Same reasoning, same
        // mode, as `/auth/*` above. `if_not_present` is right only where a
        // handler's own directive is the INTENDED answer, which is the
        // `/.well-known/*` case on the `data` group.
        .route_layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ));

    let mut aux = aux.merge(admin_ops);

    // FEAT-10: mount `GET /metrics` only when a recorder is installed
    // ([metrics] enabled). Deliberately in the un-authed, un-rate-limited
    // `aux` group so a Prometheus scraper reaches it unimpeded; the handler
    // applies its own optional `metrics.bearer_token` gate.
    if metrics_enabled {
        // #218: `no-store` on THIS route only. Not on `aux` wholesale -- `aux`
        // also carries `/.well-known/jwks.json` and `/.well-known/did.json`,
        // which set their own `public, max-age=300` on the 200 arm.
        //
        // `overriding`, not `if_not_present`, for the same reason as `/admin/*`
        // above: the guarantee written into `docs/HTTP-API.md` is unconditional,
        // so the layer must be too.
        //
        // A `route_layer` covers the 401 arm (emitted inside the handler when
        // `metrics.bearer_token` rejects) and the 405 arm as well -- which is
        // the point, since a cached 401 handed to an authorized scraper is the
        // worse half of this bug. `/metrics` content is authorization-relative:
        // 200-vs-401 depends on the caller.
        aux = aux.merge(
            Router::new()
                .route("/metrics", get(metrics::metrics_endpoint::<S>))
                .route_layer(SetResponseHeaderLayer::overriding(
                    axum::http::header::CACHE_CONTROL,
                    HeaderValue::from_static("no-store"),
                )),
        );
    }

    let mut app = acdp.merge(aux).merge(admin).with_state(state);

    // FEAT-10: request-level metrics, recording count/latency/status by matched
    // route. This layer is applied FIRST, so it is the INNERMOST of the outer
    // stack -- observed latency therefore covers the router and the per-route
    // layers, and EXCLUDES the trace/timeout/body-limit/CORS/request-id layers
    // above it. (The comment here used to claim the opposite, from back when
    // the stack was read in `ServiceBuilder` order.) Added only when
    // metrics are enabled — the `counter!`/`histogram!` macros would no-op
    // without a recorder, but skipping the layer avoids the per-request
    // `MatchedPath` clone entirely on the common (metrics-off) path.
    if metrics_enabled {
        app = app.layer(from_fn(metrics::track_metrics));
    }

    // FEAT-10 / observability: the span carries the request id so a JSON log
    // line can be joined to a client-reported `x-request-id`. Both the span AND
    // the request/response events must be raised to INFO: the binary's default
    // filter is `info,acdp=info,acdp_registry=info` (`main.rs`), and
    // `TraceLayer::new_for_http()` defaults every one of them to DEBUG — so
    // raising only the events would leave them without span context, and
    // raising only the span would emit no events. Either half alone still
    // leaves the logs unjoinable.
    app.layer(
        TraceLayer::new_for_http()
            .make_span_with(make_request_span)
            .on_request(DefaultOnRequest::new().level(Level::INFO))
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
    .layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(30),
    ))
    // SEC-06: cap every request body uniformly. The publish handler
    // used to perform this check inline; the layer applies it to
    // `/auth/challenge` and `/auth/token` as well so an unauthenticated
    // caller can't push arbitrarily-large JSON at those routes.
    .layer(RequestBodyLimitLayer::new(
        usize::try_from(body_limit).unwrap_or(usize::MAX),
    ))
    .layer(cors)
    // RFC-ACDP-0007 §5: give the 413 that `RequestBodyLimitLayer`
    // synthesizes a real error envelope. Placed OUTSIDE that layer (so it
    // sees the `Content-Length` short-circuit, which never calls inner) and
    // INSIDE the request-id pair (so the rewritten response still gets its
    // `x-request-id`).
    .layer(map_response(envelope_payload_too_large))
    // The request-id pair sits here -- OUTSIDE cors, the body limit and the
    // timeout -- so that a response those layers synthesize themselves (a
    // 413, a 408, a CORS preflight) still carries `x-request-id`. It used to
    // sit innermost, which meant an operator could never correlate exactly
    // the failures they most need to correlate.
    //
    // The ORDER WITHIN THE PAIR is load-bearing and is INVERTED from
    // tower-http's own `ServiceBuilder` doc example, because `Router::layer`
    // makes the LATER call the OUTER one while `ServiceBuilder` makes the
    // FIRST call outermost. `PropagateRequestId` reads the id from the
    // REQUEST headers before calling inner, so it must run AFTER
    // `SetRequestId` has stamped them -- i.e. be applied EARLIER here.
    // Applied in the other order (as it was), Propagate looked for a header
    // Set had not written yet and the generated id reached NO response at
    // all, not merely the middleware-generated ones. Only a client-supplied
    // id echoed back, which is what hid the defect.
    .layer(PropagateRequestIdLayer::x_request_id())
    .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
    // RFC-ACDP-0007 §4: failures generated by the outer middleware itself
    // (the 413 from RequestBodyLimitLayer, a 408 from TimeoutLayer) bypass
    // both the per-route acdp+json layer and RegistryError::into_response,
    // so they would otherwise carry no ACDP media type. Set it here, as the
    // outermost layer, only when the response carries no Content-Type — so
    // JWKS (`application/jwk-set+json`), health, and every handler/error
    // response that already set their own media type are left untouched.
    .layer(SetResponseHeaderLayer::if_not_present(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/acdp+json"),
    ))
}

/// RFC-ACDP-0007 §5: ensure a 413 carries the ACDP error envelope.
///
/// Two distinct paths produce a 413 and BOTH were non-conformant:
///
/// 1. **`Content-Length` short-circuit.** `RequestBodyLimit::call` reads the
///    header and, when it exceeds the cap, returns its own response without
///    ever calling inner — hard-setting `Content-Type: text/plain`. The
///    outermost `if_not_present` media-type layer cannot correct that, because
///    a Content-Type is already present.
/// 2. **Streamed (no `Content-Length`).** The body is wrapped and the 413 comes
///    from inside the router, so the per-route layer stamps `acdp+json` onto
///    it — but the BODY is still tower-http's plain-text prose. The media type
///    was right and the payload was not.
///
/// So the rewrite cannot key on the media type: path 2 already advertises
/// `acdp+json` while carrying non-JSON. It keys on whether the body actually
/// parses as a §5 envelope, which is the property callers depend on. A
/// handler-produced 413 (`AcdpError::EmbeddedTooLarge`) already satisfies that
/// and is passed through untouched.
///
/// **408 is deliberately NOT handled here.** RFC-ACDP-0007 §5 has no wire code
/// for a timeout — `acdp_wire_code` would fall through to `internal_error`,
/// which is worse than silence because it misattributes a client-side timeout
/// to a server fault. Minting a `request_timeout` code is a change to the
/// shared §5 registry in `acdp-registry-types`, outside this lane's claim.
/// Tracked rather than guessed at.
async fn envelope_payload_too_large(resp: Response) -> Response {
    if resp.status() != StatusCode::PAYLOAD_TOO_LARGE {
        return resp;
    }
    let (mut parts, body) = resp.into_parts();
    // Error bodies are small; the cap is a bound, not a tuning knob.
    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            // We could not read the body, so we have NO evidence about what it
            // said -- and synthesizing `payload_too_large` here would assert a
            // cause we never observed. A handler-produced 413 carrying
            // `embedded_too_large` would be silently relabelled. Keep the
            // headers, drop the unreadable body, claim nothing.
            tracing::warn!("could not buffer a 413 body; leaving it un-enveloped");
            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
            return Response::from_parts(parts, axum::body::Body::empty());
        }
    };
    let already_enveloped = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_str())
                .map(str::to_string)
        })
        .is_some();
    if already_enveloped {
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    // Transplant the envelope onto the ORIGINAL parts rather than returning a
    // fresh response. `CorsLayer` is applied INSIDE this layer, so a fresh
    // response would discard `Access-Control-Allow-Origin` and `Vary` --
    // handing a browser client a correct §5 envelope it is not allowed to
    // read. That would have made the 413 worse for the exact consumer this
    // envelope exists to serve.
    let envelope = RegistryError::Acdp(acdp::error::AcdpError::PayloadTooLarge(
        "request body exceeds the configured limit".into(),
    ))
    .into_response();
    let (env_parts, env_body) = envelope.into_parts();
    parts.status = env_parts.status;
    if let Some(ct) = env_parts.headers.get(axum::http::header::CONTENT_TYPE) {
        parts
            .headers
            .insert(axum::http::header::CONTENT_TYPE, ct.clone());
    }
    // The body changed, so any inherited length is now a lie.
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, env_body)
}

/// The tracing span for one HTTP request, carrying `request_id` so JSON logs
/// can be joined to a client-reported `x-request-id`.
///
/// Reads the HEADER rather than the `RequestId` extension: `SetRequestIdLayer`
/// writes the header on the generated path, and a client-supplied id is already
/// in the headers on the other path, so the header is populated in BOTH cases
/// by the time this runs. That is only true because this layer sits INSIDE the
/// request-id pair -- see the ordering comment in `build_router`.
/// The `x-request-id` carried by a request, or `""` when absent or non-ASCII.
///
/// Split out from `make_request_span` so the extraction can be tested directly:
/// a `Span` exposes no way to read its fields back, so testing through the span
/// alone requires a live subscriber. Both are tested below.
fn request_id_of(req: &Request) -> &str {
    req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

fn make_request_span(req: &Request) -> tracing::Span {
    let request_id = request_id_of(req);
    tracing::info_span!(
        "http_request",
        method = %req.method(),
        uri = %req.uri(),
        request_id = %request_id,
    )
}

/// FEAT-06: per-IP + process-global rate limiting middleware for `/auth/*`.
///
/// Runs before the auth handlers (which apply their own per-agent budgets).
/// The client IP is resolved from `ConnectInfo<SocketAddr>` (the TCP socket
/// peer) plus the trusted-proxy `X-Forwarded-For` policy — see
/// [`rate_limit::client_ip`] for the security rationale. When no
/// `ConnectInfo` is present (e.g. an in-process `oneshot` test that did not
/// inject one) the peer defaults to `0.0.0.0` so every such request shares a
/// single bucket rather than panicking.
async fn auth_rate_limit<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    req: Request,
    next: Next,
) -> Response {
    let Some(limiter) = &state.auth_ip_limiter else {
        return next.run(req).await;
    };
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| rate_limit::canonical_ip(ci.0.ip()))
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let xff = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok());
    let ip = rate_limit::client_ip(peer, xff, &state.trusted_proxies);
    let rl = &state.config.rate_limit;

    // Process-global ceiling first (bounds a source-IP-rotating flood), then
    // the per-IP budget. Mirrors the challenge handler's global-then-key
    // ordering.
    if rl.global_per_minute > 0 {
        if let Err(retry_after_seconds) = limiter.check_global() {
            metrics::record_rate_limit_rejection(metrics::RateLimitScope::AuthGlobal);
            return RegistryError::RateLimited {
                retry_after_seconds,
            }
            .into_response();
        }
    }
    if rl.per_ip_per_minute > 0 {
        if let Err(retry_after_seconds) = limiter.check(&ip.to_string()) {
            metrics::record_rate_limit_rejection(metrics::RateLimitScope::AuthPerIp);
            return RegistryError::RateLimited {
                retry_after_seconds,
            }
            .into_response();
        }
    }
    next.run(req).await
}

/// SEC-02: build a CORS layer driven by `[registry.cors] allowed_origins`.
///
/// Default (empty list) sends no CORS headers — third-party origins
/// cannot make cross-origin authenticated requests using a visitor's
/// stored bearer token. `CorsLayer::permissive()` (the prior default)
/// unconditionally set `Access-Control-Allow-Origin: *`, which was
/// inappropriate for a registry that serves restricted/private contexts.
fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    if allowed_origins.is_empty() {
        return CorsLayer::new();
    }
    let parsed: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(parsed))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            HeaderName::from_static("authorization"),
            HeaderName::from_static("content-type"),
            HeaderName::from_static("idempotency-key"),
            HeaderName::from_static("x-run-id"),
        ])
}

#[cfg(feature = "playground")]
fn admin_router<S: ExtendedRegistryStore + 'static>() -> Router<Arc<AppState<S>>> {
    Router::new()
        .route("/admin/contexts", get(handlers::admin_list::<S>))
        .route(
            "/admin/pinned-keys/reload",
            post(handlers::reload_pinned_keys::<S>),
        )
        // #205: admin internals are never cacheable. This layer lives INSIDE
        // the playground variant on purpose -- the `not(playground)` arm below
        // returns an empty router, and axum's `route_layer` PANICS on a router
        // with no routes ("Adding a route_layer before any routes is a no-op").
        // `default = []` in this crate's Cargo.toml, so non-playground is the
        // DEFAULT build: hoisting this out would panic at router construction
        // in the common configuration.
        //
        // `overriding` for the same reason as `admin_ops` above: the documented
        // guarantee for `/admin/*` is unconditional, so the layer must be too.
        .route_layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
}

#[cfg(not(feature = "playground"))]
fn admin_router<S: ExtendedRegistryStore + 'static>() -> Router<Arc<AppState<S>>> {
    // Intentionally empty -- and intentionally NOT carrying the #205 no-store
    // `route_layer`, which would panic here. See the playground arm above.
    Router::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};

    /// Captures `tracing` output so a test can assert on what was actually
    /// RECORDED, not on what a Debug impl happens to print.
    #[derive(Clone, Default)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn emit_with_span(req: Request) -> String {
        let cap = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(cap.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let span = make_request_span(&req);
            let _entered = span.enter();
            tracing::info!("probe");
        });
        let bytes = cap.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    /// The extraction itself, tested directly.
    #[test]
    fn request_id_of_reads_the_header() {
        let with = Request::builder()
            .uri("/x")
            .header("x-request-id", "abc-123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(request_id_of(&with), "abc-123");

        let without = Request::builder().uri("/x").body(Body::empty()).unwrap();
        assert_eq!(
            request_id_of(&without),
            "",
            "a missing header must yield an empty id, never a panic",
        );
    }

    /// The span must actually RECORD the id, which only a live subscriber can
    /// observe.
    ///
    /// An earlier version of this test asserted on `format!("{span:?}")`. That
    /// was a guard that did not guard: `Span`'s Debug emits only metadata (name,
    /// level, target, file, line) — no field names and no values — and with no
    /// subscriber installed the span is `disabled` and records nothing at all.
    /// Deleting the `request_id` field from `make_request_span` left that test
    /// green. It is the exact defect class this unit exists to remove, so it is
    /// called out here rather than quietly replaced.
    #[test]
    fn make_request_span_records_the_request_id() {
        // TWO distinct ids, and the assertion is FIELD-QUALIFIED. Both details
        // are load-bearing, and an audit proved it by mutation:
        //
        // * One fixed id cannot tell "reads the header" from "records a
        //   hardcoded constant" — a span recording a literal `"abc-123"` passes
        //   a single-id test. Two ids kill that.
        // * `contains("abc-123")` asserts the value appears SOMEWHERE, not that
        //   it appears under this field. Renaming the field to `request_id_str`
        //   breaks every log query joining on `request_id` — the whole point of
        //   the change — yet passed the unqualified assertion, and emitted no
        //   clippy warning either. `request_id={id}` kills that.
        for id in ["abc-123", "zz-987"] {
            let req = Request::builder()
                .uri("/anything")
                .header("x-request-id", id)
                .body(Body::empty())
                .unwrap();
            let out = emit_with_span(req);
            assert!(
                out.contains(&format!("request_id={id}")),
                "the span must record the request id UNDER THE FIELD NAME `request_id`, \
                 so a log line can be joined to a client-reported x-request-id; \
                 captured output was:\n{out}",
            );
            assert!(
                out.contains("http_request"),
                "expected the http_request span in the captured output:\n{out}",
            );
        }
    }

    /// The no-header path records an empty id rather than panicking or omitting
    /// the field.
    #[test]
    fn make_request_span_tolerates_a_missing_request_id() {
        let req = Request::builder()
            .uri("/anything")
            .body(Body::empty())
            .unwrap();
        let out = emit_with_span(req);
        // The trailing `=` is load-bearing: a bare `contains("request_id")`
        // matches a DECOY field such as `request_id_absent` or a renamed
        // `request_id_str`, so an implementation that drops the field on this
        // exact path — the one this test is named for — passed it.
        assert!(
            out.contains("request_id="),
            "the request_id field must be present (and empty) even with no header:\n{out}",
        );
    }

    /// The rewriter must pass through a 413 that ALREADY carries a §5 envelope,
    /// rather than relabelling it. `AcdpError::EmbeddedTooLarge` also maps to
    /// 413 but carries the distinct code `embedded_too_large`; clobbering it
    /// would report the wrong cause to the caller. No handler emits that today,
    /// which is exactly why the branch needs a direct test — it is unreachable
    /// from the integration suite.
    #[tokio::test]
    async fn envelope_rewrite_passes_through_an_existing_envelope() {
        let original = RegistryError::Acdp(acdp::error::AcdpError::EmbeddedTooLarge(
            "embedded payload too large".into(),
        ))
        .into_response();
        assert_eq!(original.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let out = envelope_payload_too_large(original).await;
        let bytes = axum::body::to_bytes(out.into_body(), 64 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["error"]["code"], "embedded_too_large",
            "a 413 that already carries a §5 envelope must pass through unchanged, \
             not be relabelled payload_too_large: {v}",
        );
    }

    /// Non-413 responses must be returned untouched and, critically, must not be
    /// buffered — the status check short-circuits before `into_parts`.
    #[tokio::test]
    async fn envelope_rewrite_ignores_non_413_responses() {
        let mut resp = Response::new(Body::from("not json"));
        *resp.status_mut() = StatusCode::OK;
        resp.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        resp.headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        let out = envelope_payload_too_large(resp).await;
        assert_eq!(out.status(), StatusCode::OK);
        assert_eq!(
            out.headers().get(CONTENT_TYPE).unwrap(),
            "text/plain",
            "a non-413 response must not be rewritten",
        );
        assert_eq!(out.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    }

    /// A rewritten 413 must not inherit the pre-rewrite `Content-Length`, which
    /// described a body that no longer exists.
    #[tokio::test]
    async fn envelope_rewrite_drops_the_stale_content_length() {
        let mut resp = Response::new(Body::from("plain text 413"));
        *resp.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
        resp.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        resp.headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from_static("14"));
        let out = envelope_payload_too_large(resp).await;
        assert!(
            out.headers().get(CONTENT_LENGTH).is_none_or(|v| v != "14"),
            "the stale Content-Length from the replaced body must not survive",
        );
    }
}
