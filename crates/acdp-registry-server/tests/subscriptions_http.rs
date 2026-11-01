//! RFC-ACDP-0018 §8.1: `POST/GET/DELETE /subscriptions` over the real router
//! and a real SQLite store.
//!
//! Each test names the `rec-007` scenario it makes concrete (A, B, F); the
//! delivery scenarios (C, G, H) belong to the hint worker. The store-level
//! CRUD and the per-owner limit under concurrency are pinned by the parity
//! suite in every backend.

// `common` is SQLite-backed; see `caps_visibility.rs` for why every
// harness-using test binary carries this gate.
#![cfg(feature = "storage-sqlite")]

mod common;

use std::net::IpAddr;
use std::sync::Arc;

use acdp::crypto::SigningKey;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::change::{Subscription, SubscriptionStatus};
use acdp_registry_core::callback::StaticResolver;
use acdp_registry_core::changes_key::ChangesKey;
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryConfig;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use common::{forged_bearer, SeededHarness};
use serde_json::{json, Value};
use tower::ServiceExt;

const AUTHORITY: &str = "registry.test";
const CHANGES_SEED: [u8; 32] = [9u8; 32];
/// A callback host the test resolver answers with a public address.
const HOOK_HOST: &str = "hooks.example.com";
const PUBLIC_IP: &str = "93.184.216.34";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn config(events: bool, max: u64) -> RegistryConfig {
    let mut cfg = RegistryConfig::defaults();
    cfg.registry.authority = AUTHORITY.into();
    cfg.auth.enabled = true;
    cfg.auth.did_methods = vec!["did:web".into(), "did:key".into()];
    cfg.receipt.signing_key_seed_b64 = b64(&[99u8; 32]);
    cfg.lifecycle.enabled = true;
    cfg.changes.enabled = true;
    cfg.changes.key_b64 = b64(&CHANGES_SEED);
    cfg.events.enabled = events;
    cfg.events.max_subscriptions_per_requester = max;
    cfg
}

fn caps() -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: "0.5.0".into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        supported_signature_algorithms: vec!["ed25519".into()],
        supported_did_methods: vec!["did:web".into(), "did:key".into()],
        profiles: vec!["acdp-registry-core".into()],
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
            change_retention_seconds: None,
            max_change_filters: None,
            max_changes_page: None,
            max_subscriptions_per_requester: None,
            max_reads_per_minute: None,
            max_anonymous_reads_per_minute: None,
            extensions: Default::default(),
        },
        read_authentication_methods: vec![],
        anonymous_public_reads: true,
        supports_idempotency_key: true,
        extensions: Default::default(),
    }
}

fn resolver() -> StaticResolver {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    StaticResolver::new()
        .with(HOOK_HOST, &[ip(PUBLIC_IP)])
        .with("internal.example.com", &[ip("10.0.0.5")])
        .with("mixed.example.com", &[ip(PUBLIC_IP), ip("192.168.1.9")])
        .with("metadata.example.com", &[ip("169.254.169.254")])
        .with("v6local.example.com", &[ip("fd00::1")])
}

async fn harness_with(events: bool, max: u64) -> SeededHarness {
    let cfg = config(events, max);
    let (cfg, caps) = common::with_anonymous_public_reads(cfg, caps(), true);
    let mut h = SeededHarness::new(cfg.clone(), caps, AUTHORITY).await;
    h.rebuild_state_with(cfg, |s| s.with_callback_resolver(Arc::new(resolver())));
    h
}

async fn harness() -> SeededHarness {
    harness_with(true, 10).await
}

fn did_of(seed: u8) -> String {
    acdp::did::key::did_key_from_ed25519(&SigningKey::from_bytes(&[seed; 32]).verifying_key_bytes())
}

fn bearer_for(seed: u8) -> String {
    forged_bearer(&did_of(seed), &uuid::Uuid::new_v4().to_string(), 600)
}

fn key() -> ChangesKey {
    ChangesKey::from_master(&CHANGES_SEED, 1).unwrap()
}

struct Resp {
    status: StatusCode,
    cache_control: Option<String>,
    content_type: Option<String>,
    body: Value,
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Vec<u8>>,
) -> Resp {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(bytes) => b
            .header("content-type", "application/acdp+json")
            .body(Body::from(bytes))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let cache_control = header("cache-control");
    let content_type = header("content-type");
    let status = resp.status();
    Resp {
        status,
        cache_control,
        content_type,
        body: common::body_to_json_lenient(resp).await,
    }
}

fn request(filter: Value, callback_url: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"filter": filter, "callback_url": callback_url})).unwrap()
}

fn agent_filter(seed: u8) -> Value {
    json!({"agent_id": [did_of(seed)], "kind": ["lifecycle"]})
}

async fn create(app: &axum::Router, bearer: Option<&str>, body: Vec<u8>) -> Resp {
    call(app, "POST", "/subscriptions", bearer, Some(body)).await
}

/// Create a subscription for `seed`, asserting 201, and return it.
async fn create_ok(h: &SeededHarness, seed: u8) -> Subscription {
    let r = create(
        &h.router,
        Some(&bearer_for(seed)),
        request(
            agent_filter(seed),
            &format!("https://{HOOK_HOST}/acdp/hints"),
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    Subscription::from_value(&r.body).unwrap_or_else(|e| panic!("{e}: {}", r.body))
}

fn assert_field_error(r: &Resp, field: &str, what: &str) {
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}: {}", r.body);
    assert_eq!(
        r.body["error"]["code"], "schema_violation",
        "{what}: {}",
        r.body
    );
    assert_eq!(
        r.body["error"]["details"]["field"], field,
        "{what}: {}",
        r.body
    );
}

// ─── rec-007 A: create, read, delete ────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec007_a_create_get_delete() {
    let h = harness().await;
    let owner = bearer_for(1);
    let url = format!("https://{HOOK_HOST}/acdp/hints");
    let r = create(&h.router, Some(&owner), request(agent_filter(1), &url)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    // The one response that carries the secret is never stored.
    assert_eq!(r.cache_control.as_deref(), Some("no-store"));
    assert_eq!(r.content_type.as_deref(), Some("application/acdp+json"));
    let created = Subscription::from_value(&r.body).unwrap_or_else(|e| panic!("{e}: {}", r.body));
    assert_eq!(created.status, SubscriptionStatus::Active);
    assert_eq!(created.callback_url, url);
    assert_eq!(
        serde_json::to_value(&created.filter).unwrap(),
        agent_filter(1)
    );
    let secret = created.secret.clone().expect("201 carries the secret");
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&secret)
        .expect("secret is base64");
    assert!(raw.len() >= 32, "secret is {} bytes", raw.len());

    let path = format!("/subscriptions/{}", created.subscription_id);
    let r = call(&h.router, "GET", &path, Some(&owner), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(
        r.body.get("secret").is_none(),
        "GET leaks the secret: {}",
        r.body
    );
    assert_eq!(r.cache_control.as_deref(), Some("private"));
    let got = Subscription::from_value(&r.body).unwrap();
    assert_eq!(got, created.without_secret());

    let r = call(&h.router, "DELETE", &path, Some(&owner), None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.body);
    for method in ["GET", "DELETE"] {
        let r = call(&h.router, method, &path, Some(&owner), None).await;
        assert_eq!(
            r.status,
            StatusCode::NOT_FOUND,
            "{method} after delete: {}",
            r.body
        );
        assert_eq!(r.body["error"]["code"], "not_found");
    }
}

/// Two creates never share an id or a secret.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_subscription_gets_its_own_id_and_secret() {
    let h = harness().await;
    let a = create_ok(&h, 1).await;
    let b = create_ok(&h, 1).await;
    assert_ne!(a.subscription_id, b.subscription_id);
    assert_ne!(a.secret, b.secret);
}

// ─── rec-007 B: owner scoping ───────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec007_b_owner_scoping() {
    let h = harness().await;
    let sub = create_ok(&h, 1).await;
    let path = format!("/subscriptions/{}", sub.subscription_id);
    let other = bearer_for(2);
    for method in ["GET", "DELETE"] {
        let r = call(&h.router, method, &path, Some(&other), None).await;
        assert_eq!(
            r.status,
            StatusCode::NOT_FOUND,
            "{method} by another: {}",
            r.body
        );
        assert_eq!(r.body["error"]["code"], "not_found");
    }
    // Anonymous requests are refused outright, on every verb.
    let url = format!("https://{HOOK_HOST}/h");
    let r = create(&h.router, None, request(agent_filter(1), &url)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["error"]["code"], "not_authorized");
    for method in ["GET", "DELETE"] {
        let r = call(&h.router, method, &path, None, None).await;
        assert_eq!(
            r.status,
            StatusCode::FORBIDDEN,
            "anonymous {method}: {}",
            r.body
        );
        assert_eq!(r.body["error"]["code"], "not_authorized");
    }
    // The other requester's DELETE removed nothing.
    let r = call(&h.router, "GET", &path, Some(&bearer_for(1)), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

/// An id that is not a v4 UUID is just an id nobody owns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_ids_are_not_found() {
    let h = harness().await;
    for id in ["nope", "00000000-0000-0000-0000-000000000000", "%20"] {
        for method in ["GET", "DELETE"] {
            let r = call(
                &h.router,
                method,
                &format!("/subscriptions/{id}"),
                Some(&bearer_for(1)),
                None,
            )
            .await;
            assert_eq!(r.status, StatusCode::NOT_FOUND, "{method} {id}: {}", r.body);
            assert_eq!(r.body["error"]["code"], "not_found");
        }
    }
}

// ─── rec-007 F: SSRF at creation ────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec007_f_ssrf_at_creation() {
    let h = harness().await;
    let owner = bearer_for(1);
    for url in [
        format!("http://{HOOK_HOST}/h"),
        "https://127.0.0.1/h".into(),
        "https://[::1]/h".into(),
        "https://169.254.169.254/latest/meta-data".into(),
        format!("https://{PUBLIC_IP}/h"),
        // Hosts whose answers fall outside public unicast, whole or in part.
        "https://internal.example.com/h".into(),
        "https://mixed.example.com/h".into(),
        "https://metadata.example.com/h".into(),
        "https://v6local.example.com/h".into(),
        // A host that does not resolve.
        "https://unknown.example.com/h".into(),
        "not a url".into(),
    ] {
        let r = create(&h.router, Some(&owner), request(agent_filter(1), &url)).await;
        assert_field_error(&r, "callback_url", &url);
    }
    // A resolution refusal reads the same whatever the answer was, so the
    // registry's resolver is no oracle for internal names.
    let mut messages = std::collections::BTreeSet::new();
    for host in ["internal", "mixed", "metadata", "v6local", "unknown"] {
        let url = format!("https://{host}.example.com/h");
        let r = create(&h.router, Some(&owner), request(agent_filter(1), &url)).await;
        let msg = r.body["error"]["message"]
            .as_str()
            .expect("error.message")
            .to_owned();
        for leak in ["10.0.0.5", "192.168.1.9", "169.254", "fd00", "resolve to a"] {
            assert!(!msg.contains(leak), "{url}: {msg}");
        }
        messages.insert(msg);
    }
    assert_eq!(messages.len(), 1, "{messages:?}");
}

// ─── Request validation ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_validation_names_the_field() {
    let h = harness().await;
    let owner = bearer_for(1);
    let url = format!("https://{HOOK_HOST}/h");
    let cases: Vec<(&str, Value, &str)> = vec![
        ("missing filter", json!({"callback_url": url}), "filter"),
        (
            "empty filter",
            json!({"filter": {}, "callback_url": url}),
            "filter",
        ),
        (
            "empty member",
            json!({"filter": {"agent_id": []}, "callback_url": url}),
            "filter",
        ),
        (
            "unknown kind",
            json!({"filter": {"agent_id": [did_of(1)], "kind": ["deleted"]}, "callback_url": url}),
            "filter",
        ),
        (
            "duplicate value",
            json!({"filter": {"agent_id": [did_of(1), did_of(1)]}, "callback_url": url}),
            "filter",
        ),
        (
            "bad ctx id",
            json!({"filter": {"ctx_id": ["not-a-ctx"]}, "callback_url": url}),
            "filter",
        ),
        (
            "unknown filter member",
            json!({"filter": {"agent_id": [did_of(1)], "tenant": ["x"]}, "callback_url": url}),
            "filter",
        ),
        (
            "missing callback_url",
            json!({"filter": agent_filter(1)}),
            "callback_url",
        ),
        (
            "non-string callback_url",
            json!({"filter": agent_filter(1), "callback_url": 7}),
            "callback_url",
        ),
    ];
    for (what, body, field) in cases {
        let r = create(&h.router, Some(&owner), serde_json::to_vec(&body).unwrap()).await;
        assert_field_error(&r, field, what);
    }
    // `kind` alone is a filter (minProperties 1): every visible change of
    // that kind.
    let r = create(
        &h.router,
        Some(&owner),
        request(json!({"kind": ["superseded"]}), &url),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "kind-only: {}", r.body);
    // Over max_change_filters values.
    let many: Vec<String> = (0..101)
        .map(|i| format!("did:web:agents.test:a{i}"))
        .collect();
    let r = create(
        &h.router,
        Some(&owner),
        request(json!({"agent_id": many}), &url),
    )
    .await;
    assert_field_error(&r, "filter", "101 values");
    // Not a request at all.
    for (what, body) in [
        ("not json", b"{".to_vec()),
        ("not an object", b"[]".to_vec()),
        (
            "unknown member",
            serde_json::to_vec(&json!({"filter": agent_filter(1), "callback_url": url, "x": 1}))
                .unwrap(),
        ),
    ] {
        let r = create(&h.router, Some(&owner), body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}: {}", r.body);
        assert_eq!(
            r.body["error"]["code"], "schema_violation",
            "{what}: {}",
            r.body
        );
    }
}

// ─── Limit ──────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_per_requester_limit_is_a_field_error_and_frees_on_delete() {
    let h = harness_with(true, 2).await;
    let a = create_ok(&h, 1).await;
    create_ok(&h, 1).await;
    let url = format!("https://{HOOK_HOST}/h");
    let r = create(
        &h.router,
        Some(&bearer_for(1)),
        request(agent_filter(1), &url),
    )
    .await;
    assert_field_error(&r, "limits.max_subscriptions_per_requester", "third create");
    // Another requester has its own allowance.
    create_ok(&h, 2).await;
    // Deleting one frees its slot.
    let r = call(
        &h.router,
        "DELETE",
        &format!("/subscriptions/{}", a.subscription_id),
        Some(&bearer_for(1)),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.body);
    create_ok(&h, 1).await;
}

// ─── Disabled ───────────────────────────────────────────────────────────────

/// Without `acdp-registry-events` every verb answers 501, before auth.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscriptions_disabled_is_501() {
    let h = harness_with(false, 10).await;
    let url = format!("https://{HOOK_HOST}/h");
    let id = "/subscriptions/00000000-0000-4000-8000-000000000000";
    for bearer in [None, Some(bearer_for(1))] {
        let r = create(&h.router, bearer.as_deref(), request(agent_filter(1), &url)).await;
        assert_eq!(r.status, StatusCode::NOT_IMPLEMENTED, "{}", r.body);
        assert_eq!(r.body["error"]["code"], "not_implemented");
        for method in ["GET", "DELETE"] {
            let r = call(&h.router, method, id, bearer.as_deref(), None).await;
            assert_eq!(
                r.status,
                StatusCode::NOT_IMPLEMENTED,
                "{method}: {}",
                r.body
            );
        }
    }
}

// ─── At rest ────────────────────────────────────────────────────────────────

/// The stored row holds the secret only sealed under the changes key, bound
/// to its own subscription id, and starts its hint cursor at the feed head.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_secret_is_stored_sealed_and_the_cursor_starts_at_head() {
    let h = harness().await;
    let head = h.store().changes_head().await.unwrap().head;
    let sub = create_ok(&h, 1).await;
    let secret = sub.secret.clone().unwrap();
    let row = h
        .store()
        .get_subscription(&did_of(1), &sub.subscription_id)
        .await
        .unwrap()
        .expect("stored");
    assert_eq!(row.owner_did, did_of(1));
    assert_eq!(row.last_hinted_seq, head);
    assert_eq!(row.attempts, 0);
    assert_eq!(row.failing_since_ms, None);
    let k = key();
    assert_eq!(row.secret_key_id, k.key_id());
    let ct = &row.secret_ct;
    assert!(
        !ct.windows(secret.len()).any(|w| w == secret.as_bytes()),
        "plaintext secret in the stored row"
    );
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&secret)
        .unwrap();
    assert!(!ct.windows(raw.len()).any(|w| w == raw.as_slice()));
    let opened = k
        .open_secret(&sub.subscription_id, row.secret_key_id, ct)
        .expect("opens under its own id");
    assert_eq!(opened.as_str(), secret);
    // Bound to its id: the same ciphertext does not open as another row's.
    assert!(k
        .open_secret(
            "00000000-0000-4000-8000-000000000000",
            row.secret_key_id,
            ct
        )
        .is_err());
    // The owner-scoped lookup does not find it for anyone else.
    assert!(h
        .store()
        .get_subscription(&did_of(2), &sub.subscription_id)
        .await
        .unwrap()
        .is_none());
}

/// RFC-ACDP-0008 §6.2 / R5: a `bearer_static` read token authenticates reads
/// only. Every `/subscriptions` route and `/auth/token/revoke` refuse it 403
/// `not_authorized`, even for the DID's own subscription, and the refusal
/// changes nothing.
#[tokio::test]
async fn bearer_static_token_is_refused_on_subscriptions_and_revoke() {
    let mut cfg = config(true, 10);
    let mut tokens = Vec::new();
    for id in ["a", "b"] {
        let (token, sha256) = acdp_registry_auth::generate_read_token();
        cfg.auth
            .read_tokens
            .push(acdp_registry_types::config::ReadTokenEntry {
                id: id.into(),
                did: did_of(71),
                sha256,
                expires_at: None,
                tenant: None,
                operator_administered: true,
            });
        tokens.push(token);
    }
    let (cfg, caps) = common::with_anonymous_public_reads(cfg, caps(), true);
    let mut h = SeededHarness::new(cfg.clone(), caps, AUTHORITY).await;
    h.rebuild_state_with(cfg, |s| s.with_callback_resolver(Arc::new(resolver())));
    let sub = create_ok(&h, 71).await;
    let one = format!("/subscriptions/{}", sub.subscription_id);
    let token = tokens[0].as_str();

    let body = request(agent_filter(71), &format!("https://{HOOK_HOST}/acdp/hints"));
    let revoke = serde_json::to_vec(&json!({"jti": "x"})).unwrap();
    for (method, uri, body) in [
        ("POST", "/subscriptions", Some(body)),
        ("GET", one.as_str(), None),
        ("DELETE", one.as_str(), None),
        ("POST", "/auth/token/revoke", Some(revoke)),
    ] {
        let r = call(&h.router, method, uri, Some(token), body).await;
        assert_eq!(
            r.status,
            StatusCode::FORBIDDEN,
            "{method} {uri}: {}",
            r.body
        );
        assert_eq!(
            r.body["error"]["code"], "not_authorized",
            "{method} {uri}: {}",
            r.body
        );
        assert!(
            r.body.to_string().contains("bearer_static"),
            "{method} {uri}: {}",
            r.body
        );
    }
    // The subscription survived the refused DELETE.
    let r = call(&h.router, "GET", &one, Some(&bearer_for(71)), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}
