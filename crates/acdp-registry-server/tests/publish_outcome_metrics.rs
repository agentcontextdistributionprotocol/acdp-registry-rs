//! publish-201-location Phase 1a (`plans/publish-201-location.md`): the
//! publish counters tell a replay from an insert.
//!
//! `acdp_registry_publish_total{outcome}` is `inserted` for a new context and
//! `idempotent_replay` for a replay, on every publish branch (before this, only
//! the playground-unpinned branch distinguished them; the other three counted a
//! replay as `inserted`). A replay mints no receipt and appends no log leaf, so
//! `acdp_registry_receipts_minted_total` and `acdp_registry_log_leaves_total`
//! do not move on one.
//!
//! **Why a binary of its own.** The `metrics` recorder is process-global.
//! `metrics_integration.rs` pins `publish_total{outcome="inserted"} == 2` from a
//! test that runs in parallel with that binary's others, so a test there that
//! published would break it, and `http_integration.rs` runs hundreds of
//! publishing tests in parallel, so no count there is exact. Here a single test
//! owns the process, so every count below is an absolute, exact value.

#![cfg(feature = "storage-sqlite")]

mod common;
// Only the fixture server is used here; the module's other helpers serve
// `http_integration.rs`.
#[allow(dead_code)]
mod didweb;

use std::sync::Arc;

use acdp::crypto::SigningKey;
use acdp::did::WebResolver;
use acdp::producer::Producer;
use acdp::registry::RegistryServer;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::primitives::{AgentDid, ContextType, Visibility};
use acdp::types::publish::PublishRequest;
use acdp_registry_auth::{
    AuthService, ChallengeStore, InMemoryChallengeStore, JwtSecret, JwtSigner,
};
use acdp_registry_core::metrics::{LOG_LEAVES_TOTAL, PUBLISH_TOTAL, RECEIPTS_MINTED_TOTAL};
use acdp_registry_core::{build_router, AppStateInner};
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::{
    config::PinnedAgentKey, AuthConfig, LimitsConfig, MetricsConfig, PlaygroundConfig,
    RegistryConfig, RegistrySection, StorageBackend, StorageConfig, WebhookConfig,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use common::{publish, Harness};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const AUTHORITY: &str = "registry.test";
const RECEIPT_SEED: [u8; 32] = [77u8; 32];

fn caps(acdp_version: &str) -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: acdp_version.into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        supported_signature_algorithms: vec!["ed25519".into(), "ecdsa-p256".into()],
        supported_did_methods: vec!["did:web".into(), "did:key".into()],
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

/// Receipts + transparency log, mirroring `http_integration.rs`'s `log_caps()`.
fn log_caps() -> CapabilitiesDocument {
    let mut c = caps("0.3.0");
    c.profiles.push("acdp-registry-transparency-log".into());
    c
}

/// Metrics ON (the recorder installs on the first harness built), scrape
/// unauthenticated, no rate limiting in the way.
fn config(playground: bool) -> RegistryConfig {
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
        auth: AuthConfig {
            anonymous_public_reads: true,
            did_methods: vec!["did:web".into(), "did:key".into()],
            ..AuthConfig::default()
        },
        webhook: WebhookConfig::default(),
        limits: LimitsConfig::default(),
        rate_limit: Default::default(),
        metrics: MetricsConfig {
            enabled: true,
            bearer_token: String::new(),
            ..MetricsConfig::default()
        },
        playground: PlaygroundConfig {
            enabled: playground,
            ..Default::default()
        },
        receipt: Default::default(),
        lifecycle: Default::default(),
        log: Default::default(),
        witnesses: Vec::new(),
    }
}

async fn harness(cfg: RegistryConfig, caps: CapabilitiesDocument) -> Harness {
    common::build_harness_with_webhook(cfg, caps, AUTHORITY, common::StoreMode::Memory, None, None)
        .await
}

/// Production path with a resolver that reaches the in-process `did:web`
/// fixture server, assembled from the same public pieces as
/// `http_integration.rs`'s `didweb_lifecycle_harness` (the shared harness
/// hard-codes a real-DNS `WebResolver`).
async fn didweb_harness() -> Harness {
    let addr = didweb::spawn_didweb_server().await;
    let resolver = Arc::new(
        WebResolver::with_test_endpoint(
            didweb::ca_pem().as_bytes(),
            didweb::DIDWEB_AUTHORITY,
            addr,
        )
        .expect("test-endpoint resolver"),
    );
    let db = tempfile::Builder::new()
        .prefix("acdp-publish-metrics-")
        .tempdir()
        .unwrap();
    let store = SqliteStore::connect(&db.path().join(common::DB_FILE_NAME), 1)
        .await
        .unwrap();
    store.migrate().await.unwrap();
    let server = Arc::new(RegistryServer::try_new(store, caps("0.3.0"), AUTHORITY).unwrap());
    let challenges: Arc<dyn ChallengeStore> = Arc::new(InMemoryChallengeStore::new());
    let secret = JwtSecret::from_bytes(&[42u8; 32]);
    let signer = JwtSigner::new(secret, format!("did:web:{AUTHORITY}"), AUTHORITY.into(), 30);
    let auth = Arc::new(AuthService::new(
        AuthConfig::default(),
        challenges,
        signer,
        resolver,
        AUTHORITY.into(),
    ));
    let state = AppStateInner::new(server, auth, None, config(false), None);
    Harness::with_db_dir(build_router(state), db)
}

fn request(p: &Producer, title: &str) -> PublishRequest {
    p.publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap()
}

async fn scrape(app: &axum::Router) -> String {
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
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Sum the values of every exposition line whose series contains all of
/// `needles` (metric name + label fragments). Same as `metrics_integration.rs`.
fn metric_sum(text: &str, needles: &[&str]) -> f64 {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter(|l| needles.iter().all(|n| l.contains(n)))
        .filter_map(|l| l.rsplit(' ').next())
        .filter_map(|v| v.parse::<f64>().ok())
        .sum()
}

/// `(inserted, idempotent_replay, receipts minted, log leaves)` as scraped.
#[derive(Debug, PartialEq)]
struct Counts {
    inserted: f64,
    replayed: f64,
    receipts: f64,
    leaves: f64,
}

async fn counts(app: &axum::Router) -> Counts {
    let text = scrape(app).await;
    Counts {
        inserted: metric_sum(&text, &[PUBLISH_TOTAL, "outcome=\"inserted\""]),
        replayed: metric_sum(&text, &[PUBLISH_TOTAL, "outcome=\"idempotent_replay\""]),
        receipts: metric_sum(&text, &[RECEIPTS_MINTED_TOTAL]),
        leaves: metric_sum(&text, &[LOG_LEAVES_TOTAL]),
    }
}

/// Fresh publish then a same-key, same-body replay on `app`, asserting the
/// counters after each step against `after_fresh` / `after_replay`.
async fn fresh_then_replay(
    branch: &str,
    app: &axum::Router,
    req: &PublishRequest,
    key: &str,
    after_fresh: Counts,
    after_replay: Counts,
) -> Value {
    let (s1, v1) = publish(app, req, Some(key)).await;
    assert_eq!(s1, StatusCode::CREATED, "{branch}: fresh publish: {v1}");
    assert_eq!(
        counts(app).await,
        after_fresh,
        "{branch}: after the fresh publish"
    );
    let (s2, v2) = publish(app, req, Some(key)).await;
    // RFC-ACDP-0003 §6.2: a fresh publish is `201 Created`, a genuine replay
    // `200 OK` (idem-002 forbids `201` there).
    assert_eq!(s2, StatusCode::OK, "{branch}: replay: {v2}");
    assert_eq!(v2, v1, "{branch}: control: this must really be a replay");
    assert_eq!(
        counts(app).await,
        after_replay,
        "{branch}: a replay counts as `idempotent_replay`, not `inserted`, and \
         mints no receipt and appends no log leaf"
    );
    v1
}

/// One test owns the process, so the counts are absolute and exact. The
/// branches run in sequence and each continues the running totals.
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_is_counted_as_a_replay_on_every_publish_branch() {
    // did:key on a registry that mints receipts AND keeps the log: the only
    // registry here where those two counters can move at all, so the fresh
    // publish moving them is what makes their NOT moving on the replay mean
    // something.
    let mut cfg = config(false);
    cfg.receipt.signing_key_seed_b64 = B64.encode(RECEIPT_SEED);
    cfg.log.enabled = true;
    let h = harness(cfg, log_caps()).await;
    assert_eq!(
        counts(&h.router).await,
        Counts {
            inserted: 0.0,
            replayed: 0.0,
            receipts: 0.0,
            leaves: 0.0
        },
        "control: nothing counted before the first publish"
    );
    let v = fresh_then_replay(
        "did:key (receipts + log)",
        &h.router,
        &request(
            &Producer::new_did_key(SigningKey::from_bytes(&[31u8; 32])),
            "m-didkey",
        ),
        "metrics-didkey-1",
        Counts {
            inserted: 1.0,
            replayed: 0.0,
            receipts: 1.0,
            leaves: 1.0,
        },
        Counts {
            inserted: 1.0,
            replayed: 1.0,
            receipts: 1.0,
            leaves: 1.0,
        },
    )
    .await;
    assert!(
        v["registry_receipt"].is_object(),
        "control: the fresh publish carried a receipt: {v}"
    );

    // Playground, pinned key verified against the pin.
    let did = "did:web:agents.test:metrics-pinned";
    let mut cfg = config(true);
    cfg.playground.pinned_keys = vec![PinnedAgentKey {
        agent_did: did.into(),
        public_key_b64: B64.encode(SigningKey::from_bytes(&[32u8; 32]).verifying_key_bytes()),
        algorithm: "ed25519".into(),
        valid_from: None,
        valid_until: None,
    }];
    let h = harness(cfg, caps("0.1.0")).await;
    let pinned = Producer::new(
        SigningKey::from_bytes(&[32u8; 32]),
        AgentDid::new(did),
        format!("{did}#key-1"),
    );
    fresh_then_replay(
        "playground pinned",
        &h.router,
        &request(&pinned, "m-pinned"),
        "metrics-pinned-1",
        Counts {
            inserted: 2.0,
            replayed: 1.0,
            receipts: 1.0,
            leaves: 1.0,
        },
        Counts {
            inserted: 2.0,
            replayed: 2.0,
            receipts: 1.0,
            leaves: 1.0,
        },
    )
    .await;

    // Playground, unpinned (the handler's own idempotency lookup).
    let h = harness(config(true), caps("0.1.0")).await;
    fresh_then_replay(
        "playground unpinned",
        &h.router,
        &request(&common::producer("metrics", 33), "m-unpinned"),
        "metrics-unpinned-1",
        Counts {
            inserted: 3.0,
            replayed: 2.0,
            receipts: 1.0,
            leaves: 1.0,
        },
        Counts {
            inserted: 3.0,
            replayed: 3.0,
            receipts: 1.0,
            leaves: 1.0,
        },
    )
    .await;

    // Production did:web, resolved through the fixture server.
    let h = didweb_harness().await;
    fresh_then_replay(
        "production did:web",
        &h.router,
        &request(&common::producer("smoke", 34), "m-didweb"),
        "metrics-didweb-1",
        Counts {
            inserted: 4.0,
            replayed: 3.0,
            receipts: 1.0,
            leaves: 1.0,
        },
        Counts {
            inserted: 4.0,
            replayed: 4.0,
            receipts: 1.0,
            leaves: 1.0,
        },
    )
    .await;
}
