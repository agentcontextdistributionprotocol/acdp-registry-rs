//! FEAT-10: Prometheus `/metrics` integration tests.
//!
//! These run in their own test binary (separate process) so the
//! process-global `metrics` recorder is isolated from other integration
//! tests. The counter-accumulation assertions live in a single `#[tokio::test]`
//! so accumulated values are deterministic within the process; the tests that
//! assert only on response headers or mounting build their own harness and are
//! independent of that ordering.

#![cfg(feature = "storage-sqlite")]

use std::sync::Arc;

use acdp::crypto::SigningKey;
use acdp::did::WebResolver;
use acdp::producer::Producer;
use acdp::registry::RegistryServer;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::primitives::{AgentDid, ContextType, Visibility};
use acdp_registry_auth::{
    AuthService, ChallengeStore, InMemoryChallengeStore, JwtSecret, JwtSigner,
};
use acdp_registry_core::metrics::RateLimitScope;
use acdp_registry_core::{build_router, AppStateInner};
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::{
    AuthConfig, LimitsConfig, MetricsConfig, PlaygroundConfig, RateLimitConfig, RegistryConfig,
    RegistrySection, StorageBackend, StorageConfig, WebhookConfig,
};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use std::net::SocketAddr;
use tower::ServiceExt;

const AUTHORITY: &str = "registry.test";

fn caps() -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: "0.1.0".into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        // Mirror the binary: both algorithms the registry actually verifies.
        supported_signature_algorithms: vec!["ed25519".into(), "ecdsa-p256".into()],
        supported_did_methods: vec!["did:web".into()],
        profiles: vec!["acdp-registry-core".into()],
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
        },
        read_authentication_methods: vec![],
        anonymous_public_reads: true,
        supports_idempotency_key: true,
        extensions: Default::default(),
    }
}

fn metrics_config() -> RegistryConfig {
    let auth = AuthConfig {
        enabled: true,
        anonymous_public_reads: true,
        ..AuthConfig::default()
    };
    RegistryConfig {
        registry: RegistrySection {
            authority: AUTHORITY.into(),
            port: 8443,
            bind: "0.0.0.0".into(),
            allow_public_bind: false,
            profiles: vec!["acdp-registry-core".into()],
            tls: Default::default(),
            cross_registry_resolution: false,
            cors: Default::default(),
            base_url: String::new(),
        },
        storage: StorageConfig {
            backend: StorageBackend::Sqlite,
            postgres_url: None,
            sqlite_path: None,
            max_connections: 1,
        },
        auth,
        webhook: WebhookConfig::default(),
        limits: LimitsConfig {
            // Disable the per-agent challenge limiter; the per-IP limiter is
            // what we drive to prove rate-limit rejections are counted.
            challenge_rate_per_minute: 0,
            ..LimitsConfig::default()
        },
        rate_limit: RateLimitConfig {
            enabled: true,
            per_ip_per_minute: 1,
            global_per_minute: 0,
            trusted_proxies: vec![],
        },
        metrics: MetricsConfig {
            enabled: true,
            bearer_token: String::new(),
            ..MetricsConfig::default()
        },
        playground: PlaygroundConfig {
            enabled: true,
            ..Default::default()
        },
        receipt: Default::default(),
        lifecycle: Default::default(),
        log: Default::default(),
        witnesses: Vec::new(),
    }
}

struct Harness {
    router: axum::Router,
    _db: tempfile::NamedTempFile,
}

async fn harness(cfg: RegistryConfig) -> Harness {
    let db = tempfile::Builder::new()
        .prefix("acdp-metrics-")
        .suffix(".sqlite")
        .tempfile()
        .unwrap();
    let store = SqliteStore::connect(db.path(), 1).await.unwrap();
    store.migrate().await.unwrap();
    let server = Arc::new(RegistryServer::try_new(store, caps(), AUTHORITY).unwrap());
    let challenges: Arc<dyn ChallengeStore> = Arc::new(InMemoryChallengeStore::new());
    let secret = JwtSecret::from_bytes(&[42u8; 32]);
    let signer = JwtSigner::new(secret, format!("did:web:{AUTHORITY}"), AUTHORITY.into(), 30);
    let resolver = Arc::new(WebResolver::new());
    let auth = Arc::new(AuthService::new(
        AuthConfig::default(),
        challenges,
        signer,
        resolver,
        AUTHORITY.into(),
    ));
    let state = AppStateInner::new(server, auth, None, cfg, None);
    Harness {
        router: build_router(state),
        _db: db,
    }
}

fn producer(seed: u8) -> Producer {
    Producer::new(
        SigningKey::from_bytes(&[seed; 32]),
        AgentDid::new(format!("did:web:agents.test:m-{seed}")),
        format!("did:web:agents.test:m-{seed}#key-1"),
    )
}

async fn publish(app: &axum::Router, req: &acdp::types::publish::PublishRequest) -> StatusCode {
    let body = serde_json::to_vec(req).unwrap();
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/contexts")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn scrape(app: &axum::Router) -> (StatusCode, String, String) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8(bytes.to_vec()).unwrap())
}

/// Sum the values of every exposition line whose series contains all of
/// `needles` (metric name + label fragments).
fn metric_sum(text: &str, needles: &[&str]) -> f64 {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter(|l| needles.iter().all(|n| l.contains(n)))
        .filter_map(|l| l.rsplit(' ').next())
        .filter_map(|v| v.parse::<f64>().ok())
        .sum()
}

#[tokio::test]
async fn metrics_endpoint_exposes_request_and_domain_series() {
    let h = harness(metrics_config()).await;

    // Two accepted publishes.
    for seed in [10u8, 11u8] {
        let req = producer(seed)
            .publish_request()
            .title("m")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        assert_eq!(publish(&h.router, &req).await, StatusCode::OK);
    }

    // A malformed publish → schema_violation outcome.
    let bad = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/contexts")
                .body(Body::from("{ not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    // Drive the per-IP limiter to a rejection: per_ip_per_minute = 1, so the
    // second challenge from one IP is 429.
    let chal = |xff_ip: &str| {
        let addr: SocketAddr = format!("{xff_ip}:5000").parse().unwrap();
        let mut req = Request::builder()
            .method("POST")
            .uri("/auth/challenge")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"agent_id": "did:web:agents.test:flooder"}).to_string(),
            ))
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
        req
    };
    assert_eq!(
        h.router
            .clone()
            .oneshot(chal("203.0.113.7"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        h.router
            .clone()
            .oneshot(chal("203.0.113.7"))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    // A9: the two CHALLENGE scopes must be DISTINGUISHABLE. Until H-A/P4 both
    // arms recorded `challenge_per_agent`, so a global-ceiling breach -- the
    // `agent_id`-rotating flood the ceiling exists to catch -- was
    // indistinguishable from one noisy agent. Those two mean opposite things
    // operationally, so collapsing them hid the attack inside the normal case.
    //
    // A SEPARATE HARNESS PER ARM, deliberately: `check_global` increments on
    // every request it admits (`rate_limit.rs:80`), so driving the per-agent arm
    // first would leave the global bucket partly consumed and make the global
    // arm's threshold depend on test order.
    let challenge_cfg = || {
        let mut c = metrics_config();
        // Global ceiling is 64x the per-agent budget (`state.rs:97`), so 1 here
        // means per-agent 1 and global 64.
        c.limits.challenge_rate_per_minute = 1;
        // Otherwise the per-IP middleware 429s first and never reaches the
        // per-agent limiter at all.
        c.rate_limit.enabled = false;
        c
    };
    let challenge_for = |agent: &str| {
        Request::builder()
            .method("POST")
            .uri("/auth/challenge")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "agent_id": agent }).to_string()))
            .unwrap()
    };

    // Per-agent arm: one agent, twice. The global bucket is at 2 of 64, so the
    // ONLY bound that can reject the second request is the per-agent budget.
    let h_agent = harness(challenge_cfg()).await;
    let agent = "did:web:agents.test:repeat";
    for (i, expected) in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS]
        .into_iter()
        .enumerate()
    {
        let got = h_agent
            .router
            .clone()
            .oneshot(challenge_for(agent))
            .await
            .unwrap()
            .status();
        assert_eq!(got, expected, "per-agent arm, request {}", i + 1);
    }

    // Global arm: 64 DISTINCT agents fill the global bucket to exactly 64, each
    // comfortably inside its own per-agent budget of 1. The 65th agent is also
    // distinct and so is ALSO inside its own per-agent budget -- which is the
    // whole point of using a fresh id rather than repeating one. It leaves the
    // global ceiling as the only bound that can possibly reject it, so a
    // mislabelled rejection cannot pass this test by coincidence.
    let h_global = harness(challenge_cfg()).await;
    for i in 0..64 {
        let got = h_global
            .router
            .clone()
            .oneshot(challenge_for(&format!("did:web:agents.test:rot-{i}")))
            .await
            .unwrap()
            .status();
        assert_eq!(got, StatusCode::OK, "global arm, filling request {i}");
    }
    let got = h_global
        .router
        .clone()
        .oneshot(challenge_for("did:web:agents.test:rot-64"))
        .await
        .unwrap()
        .status();
    assert_eq!(
        got,
        StatusCode::TOO_MANY_REQUESTS,
        "the 65th distinct agent must be refused by the GLOBAL ceiling",
    );

    // A request against a parameterized route, so the scrape below can pin
    // the axum 0.8 MatchedPath label form. The ctx_id need not resolve to a
    // real (or even well-formed) context — MatchedPath is set from the
    // matched route pattern before the handler runs, so this 400s on
    // `CtxId::parse` and the route label is still recorded.
    let param_resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/contexts/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(param_resp.status(), StatusCode::BAD_REQUEST);

    // Scrape.
    let (status, ct, text) = scrape(&h.router).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ct.starts_with("text/plain") && ct.contains("version=0.0.4"),
        "unexpected content-type: {ct}"
    );

    // Request-level metrics exist and carry the MATCHED route pattern
    // (`/contexts`), never a resolved ctx_id.
    assert!(
        text.contains("acdp_registry_request_total"),
        "missing request counter:\n{text}"
    );
    assert!(
        text.contains("acdp_registry_request_duration_seconds"),
        "missing request histogram"
    );
    assert!(
        metric_sum(
            &text,
            &["acdp_registry_request_total", "route=\"/contexts\""]
        ) >= 3.0,
        "expected >=3 /contexts requests counted"
    );

    // axum 0.8 changed the MatchedPath syntax from `/contexts/:ctx_id` to
    // `/contexts/{ctx_id}` — this is a real behavioral change to the
    // Prometheus `route=` label on every parameterized route (dashboards and
    // alerts keyed on the old `:ctx_id` form go silently blank). Pin the new
    // form explicitly so a future axum/tower-http bump can't silently
    // regress it back, or drift it further, without a test noticing.
    assert_eq!(
        metric_sum(
            &text,
            &[
                "acdp_registry_request_total",
                "route=\"/contexts/{ctx_id}\""
            ]
        ),
        1.0,
        "expected the parameterized route labeled with the new {{ctx_id}} \
         form, not the old axum 0.7 `:ctx_id` form:\n{text}"
    );

    // Domain counters.
    assert_eq!(
        metric_sum(
            &text,
            &["acdp_registry_publish_total", "outcome=\"inserted\""]
        ),
        2.0,
        "two accepted publishes\n{text}"
    );
    assert_eq!(
        metric_sum(
            &text,
            &[
                "acdp_registry_publish_total",
                "outcome=\"schema_violation\""
            ]
        ),
        1.0,
        "one malformed publish"
    );
    assert_eq!(
        metric_sum(
            &text,
            &[
                "acdp_registry_rate_limit_rejections_total",
                "scope=\"auth_per_ip\""
            ]
        ),
        1.0,
        "one per-IP rejection counted"
    );

    // The two challenge arms are counted SEPARATELY and each exactly once.
    // Asserting both, with exact values, is what catches a label swap: if the
    // two arms were transposed each assertion would still find a 1.0 under
    // *some* scope, so only pinning them to the right scope distinguishes a
    // correct split from a renamed collapse.
    assert_eq!(
        metric_sum(
            &text,
            &[
                "acdp_registry_rate_limit_rejections_total",
                "scope=\"challenge_per_agent\""
            ]
        ),
        1.0,
        "exactly one per-agent challenge rejection\n{text}"
    );
    assert_eq!(
        metric_sum(
            &text,
            &[
                "acdp_registry_rate_limit_rejections_total",
                "scope=\"challenge_global\""
            ]
        ),
        1.0,
        "exactly one GLOBAL challenge rejection -- if this is 0 and \
         challenge_per_agent is 2, the global arm is being mislabelled, which \
         is the exact defect A9 reported\n{text}"
    );
}

#[tokio::test]
async fn metrics_no_store_is_scoped_to_the_route_not_the_aux_group() {
    // The `no-store` for `/metrics` must be attached with a `route_layer` on a
    // one-route sub-router, NOT with `.layer()` on `aux` wholesale -- `aux` also
    // carries `/.well-known/jwks.json` and `/.well-known/did.json`, which are
    // requester-INVARIANT and keep `public, max-age=300`. A group-wide override
    // would silently make those uncacheable.
    //
    // THIS TEST HAS TO LIVE HERE, and the reason is worth recording. The obvious
    // place is `every_well_known_document_keeps_public_caching` in
    // `http_integration.rs`, and the plan prescribed exactly that. It cannot
    // work: that harness runs with `metrics.enabled = false`, so the whole
    // `if metrics_enabled { .. }` block is never executed there and a mutation
    // inside it has nothing to observe. The prescribed falsification was
    // verified to pass against the defect -- a probe aimed at code its harness
    // never runs.
    let mut cfg = metrics_config();
    cfg.metrics.bearer_token = String::new(); // gate off; we only care about headers
    let h = harness(cfg).await;

    for (path, want) in [
        ("/.well-known/jwks.json", "public, max-age=300"),
        ("/.well-known/acdp.json", "public, max-age=300"),
    ] {
        let resp = h
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some(want),
            "{path} is requester-invariant and must keep `{want}` -- the /metrics \
             no-store must be scoped to its own route, not applied to the aux group",
        );
    }

    // And the scrape endpoint itself is still no-store, in the same build.
    let (_, _, _) = scrape(&h.router).await;
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "/metrics must be no-store in the same build that keeps the well-known \
         documents cacheable",
    );
}

#[tokio::test]
async fn metrics_is_never_cacheable() {
    // #218. `/metrics` is authorization-relative -- 200 vs 401 depends on the
    // caller's bearer token -- so a shared cache must never store it.
    //
    // This test lives here rather than in `http_integration.rs` because that
    // file's harness has `metrics: Default::default()` (enabled = false), so
    // `/metrics` is not mounted there at all. It *could* build its own
    // metrics-enabled config; it simply should not, since this file already
    // owns the metrics surface.
    //
    // Both arms are asserted, and the 401 is the MORE important one: a cached
    // 401 is what a shared cache would hand to an authorized scraper.
    let mut cfg = metrics_config();
    cfg.metrics.bearer_token = "scrape-secret".into();
    let h = harness(cfg).await;

    // 401 arm -- no token.
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "/metrics 401 arm must be no-store -- a cached 401 is what a shared cache \
         would hand to an authorized scraper",
    );

    // 200 arm -- correct token.
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .header("authorization", "Bearer scrape-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "/metrics 200 arm must be no-store -- its content is authorization-relative",
    );

    // 405 arm -- the arm that JUSTIFIES the design. `ASSUMPTIONS.md` records
    // that a handler-set header was rejected precisely because it misses this
    // arm: a 405 is produced by the router before any handler runs, so only a
    // `route_layer` can reach it. Without this assertion, a future refactor to
    // a handler-set header leaves every other test in this file green while
    // silently dropping the property the rationale is built on -- a guard
    // weaker than the reasoning it is supposed to protect.
    for method in ["POST", "DELETE"] {
        let resp = h
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/metrics")
                    .header("authorization", "Bearer scrape-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} /metrics should be 405",
        );
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "/metrics 405 arm ({method}) must be no-store -- it is emitted by the \
             router before any handler runs, so a handler-set header would miss it",
        );
    }
}

#[tokio::test]
async fn metrics_absent_when_disabled() {
    let mut cfg = metrics_config();
    cfg.metrics.enabled = false;
    let h = harness(cfg).await;
    let (status, _, _) = scrape(&h.router).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "/metrics must 404 when disabled"
    );
}

#[tokio::test]
async fn metrics_bearer_gate_enforced() {
    let mut cfg = metrics_config();
    cfg.metrics.bearer_token = "scrape-secret".into();
    let h = harness(cfg).await;

    // No token → 401.
    let (status, _, _) = scrape(&h.router).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Correct token → 200.
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .header("authorization", "Bearer scrape-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// #166 — pins the `/metrics` row of the bearer-parser table in
/// `docs/AUTHENTICATION.md`. Before this test, deleting `.map(str::trim)` from
/// `metrics.rs:128` left the whole workspace suite green (measured), so the
/// documented trimming behaviour rested on nothing.
///
/// This matters beyond the docs: #162's startup guard is deliberately
/// NARROWER than the `auth.admin_tokens` guard, on the stated grounds that
/// this path trims BOTH the configured value and the presented one, so a
/// padded token is not protocol-dependent here. That rationale is asserted in
/// four places (`main.rs`, `CHANGELOG.md`, `docs/CONFIGURATION.md`,
/// `docs/AUTHENTICATION.md`). If the trim is ever dropped, this test is what
/// fails instead of every padded-token deployment 401-ing its scrapes.
#[tokio::test]
async fn metrics_bearer_parser_shape_is_pinned() {
    let mut cfg = metrics_config();
    cfg.metrics.bearer_token = "scrape-secret".into();
    let h = harness(cfg).await;

    async fn get(app: &axum::Router, hdr: Option<&str>) -> (StatusCode, Option<String>) {
        let mut b = Request::builder().method("GET").uri("/metrics");
        if let Some(v) = hdr {
            b = b.header("authorization", v);
        }
        let resp = app
            .clone()
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let challenge = resp
            .headers()
            .get("www-authenticate")
            .map(|v| v.to_str().unwrap().to_string());
        (status, challenge)
    }

    // The parser TRIMS: interior extra space and trailing space both still
    // authenticate. This is the assertion #162's narrower guard depends on.
    for accepted in [
        "Bearer scrape-secret",
        "Bearer  scrape-secret",
        "Bearer scrape-secret ",
        "Bearer \tscrape-secret\t",
    ] {
        let (status, challenge) = get(&h.router, Some(accepted)).await;
        assert_eq!(status, StatusCode::OK, "{accepted:?} must authenticate");
        assert!(challenge.is_none(), "{accepted:?} must not be challenged");
    }

    // The parser is CASE-SENSITIVE on the scheme and accepts only `"Bearer "`
    // — unlike `extract_bearer`, which also takes `"bearer "`. It is also
    // strict about the single separating space (a TAB is not one).
    for refused in [
        "bearer scrape-secret",
        "BEARER scrape-secret",
        "BeArEr scrape-secret",
        "Bearer\tscrape-secret",
        "Basic scrape-secret",
        "scrape-secret",
        "",
    ] {
        let (status, challenge) = get(&h.router, Some(refused)).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{refused:?} must be refused"
        );
        assert_eq!(
            challenge.as_deref(),
            Some("Bearer realm=\"metrics\""),
            "{refused:?} must carry the challenge"
        );
    }

    // #166 — /metrics is the ONE place in this registry that answers 401 and
    // emits a challenge. `docs/AUTHENTICATION.md` scopes its "403, never 401"
    // sentence around exactly this exception, so pin the exception itself.
    let (status, challenge) = get(&h.router, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(challenge.as_deref(), Some("Bearer realm=\"metrics\""));
}

/// #168 — the gate's actual AUTHORIZATION decision, which nothing pinned.
///
/// Found by mutation while implementing #168: replacing the comparison with a
/// literal `true` left the entire workspace suite green. Every "refused" case
/// in `metrics_bearer_gate_enforced` and `metrics_bearer_parser_shape_is_pinned`
/// is refused at the `"Bearer "` PREFIX — none of them presents a well-formed
/// header carrying the WRONG token, so the comparison itself was untested and
/// `/metrics` could have been made to accept any token with CI staying green.
///
/// The near-miss cases matter for #168 specifically: a token sharing a long
/// prefix with the real one is exactly what a timing oracle would exploit, and
/// it is the case a short-circuiting `!=` treats differently from `ct_eq`.
#[tokio::test]
async fn metrics_wrong_bearer_token_is_refused() {
    let mut cfg = metrics_config();
    cfg.metrics.bearer_token = "scrape-secret".into();
    let h = harness(cfg).await;

    for wrong in [
        "Bearer wrong-token",
        // Correct prefix, one byte short — the timing-oracle shape.
        "Bearer scrape-secre",
        // Correct token plus a trailing byte (not whitespace, so not trimmed).
        "Bearer scrape-secretX",
        // Same bytes, different case: the token compare is case-SENSITIVE.
        "Bearer SCRAPE-SECRET",
        // Differs only in the final byte.
        "Bearer scrape-secreT",
        // Well-formed scheme, empty token.
        "Bearer ",
    ] {
        let resp = h
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .header("authorization", wrong)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{wrong:?} must not authorize"
        );
    }
}

/// #162 — an EMPTY `metrics.bearer_token` must keep meaning "gate disabled".
/// The startup guard refuses whitespace-only values; it must not have made the
/// documented open-by-default behaviour unreachable.
#[tokio::test]
async fn empty_metrics_bearer_token_leaves_the_endpoint_open() {
    let cfg = metrics_config();
    assert!(
        cfg.metrics.bearer_token.is_empty(),
        "this test only means anything while empty is the default"
    );
    let h = harness(cfg).await;

    let (status, _, _) = scrape(&h.router).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an empty token must leave /metrics open"
    );
}

/// A9: the documented `scope` set and the emitted `scope` set must be the same
/// set -- checked in BOTH directions.
///
/// One direction alone is not enough, and the taxonomy has already been burned
/// by that: `lifecycle_per_agent` was emitted by the code and missing from the
/// documented list, while the `metrics.rs` docstring simultaneously advertised
/// a `challenge_global` that nothing emitted. Those are opposite failures and a
/// single-direction check catches only one of them.
///
/// Reads the file at RUNTIME rather than `include_str!`. `include_str!` would
/// bake a path reaching outside the crate root into the compiled test, which
/// breaks `cargo package`/`cargo publish` verification for this crate; reading
/// at runtime keeps the dependency to test execution, where it belongs.
#[test]
fn every_rate_limit_scope_is_documented() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/HTTP-API.md");
    let doc = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));

    // Scope to the row that documents THIS counter. Searching the whole file
    // would let an unrelated mention of, say, `auth_global` elsewhere satisfy
    // the assertion while the actual table row stayed stale.
    let row = doc
        .lines()
        .find(|l| l.contains("acdp_registry_rate_limit_rejections_total"))
        .expect("docs/HTTP-API.md must document acdp_registry_rate_limit_rejections_total");

    // Direction 1: every emitted scope is documented.
    for scope in RateLimitScope::ALL {
        assert!(
            row.contains(&format!("`{}`", scope.label())),
            "scope `{}` is emitted by the code but missing from the documented \
             set in docs/HTTP-API.md.\nrow: {row}",
            scope.label(),
        );
    }

    // Direction 2: every documented scope is actually emitted. Catches a label
    // left behind after a rename -- an operator would keep alerting on a series
    // that can never fire again.
    let known: Vec<&str> = RateLimitScope::ALL.iter().map(|s| s.label()).collect();
    // Backticked tokens in this row are exactly: the metric name, the label key
    // `scope`, and the scope values. Excluding the first two by name leaves the
    // scope values -- no shape heuristic needed.
    //
    // An earlier version filtered on `is_ascii_lowercase() || '_'` plus
    // `contains('_')`, which would have SILENTLY DROPPED a future scope whose
    // label has no underscore (say `global`): it would vanish from `documented`
    // and the count assertion below would blame the doc for a parser bug.
    //
    // That version also carried a `!documented.is_empty()` guard. It is gone
    // deliberately, not overlooked: direction 1 above has already proved every
    // known label is present in this row, so `documented` cannot be empty by
    // the time control reaches here. No mutation can redden that assertion
    // specifically, which by CHARTER Rule 51 makes it decoration rather than a
    // guard.
    let documented: Vec<&str> = row
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|t| !t.starts_with("acdp_registry_") && *t != "scope")
        .collect();
    for d in &documented {
        assert!(
            known.contains(d),
            "docs/HTTP-API.md documents scope `{d}`, but nothing emits it. \
             Known scopes: {known:?}",
        );
    }
    assert_eq!(
        documented.len(),
        known.len(),
        "documented scopes {documented:?} vs emitted {known:?}",
    );
}
