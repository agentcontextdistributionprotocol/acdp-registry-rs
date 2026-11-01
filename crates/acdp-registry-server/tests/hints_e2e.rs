//! RFC-ACDP-0018 §8.2–§8.3: change hints delivered over real TLS to an
//! in-process receiver, from subscriptions created through the real router
//! on a real SQLite store.
//!
//! Each pass is driven with [`run_hint_pass`] and an injected clock, so
//! backoff and the 72-hour disable are exercised without waiting. One test
//! runs the spawned worker instead, to prove a commit wakes it.
//!
//! The receiver presents the `didweb` fixture's `agents.test` leaf; the
//! callback resolver maps `agents.test` to loopback, which the test SSRF
//! policy allows (and nothing else private).

// `common` is SQLite-backed; see `caps_visibility.rs` for why every
// harness-using test binary carries this gate.
#![cfg(feature = "storage-sqlite")]

mod common;
// Only the fixture TLS server and its CA are used here.
#[allow(dead_code)]
mod didweb;

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acdp::crypto::SigningKey;
use acdp::error::AcdpError;
use acdp::producer::Producer;
use acdp::safe_http::SsrfPolicy;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::change::{verify_hint_signature, ChangeHint, Subscription, SubscriptionStatus};
use acdp::types::primitives::{ContextType, Visibility};
use acdp_registry_core::callback::{CallbackResolver, StaticResolver};
use acdp_registry_core::changes_key::ChangesKey;
use acdp_registry_core::hints::{
    retry_delay_ms, run_hint_pass, spawn_hint_worker, HintTimeouts, PassStats, DISABLE_AFTER_MS,
    HINT_MIN_INTERVAL_MS, REQUEST_TIMEOUT, SIGNATURE_HEADER,
};
use acdp_registry_core::AppStateInner;
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryConfig;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::post;
use base64::Engine as _;
use common::{forged_bearer, pct_encode_path_segment, publish, SeededHarness};
use serde_json::{json, Value};
use tower::ServiceExt;

const AUTHORITY: &str = "registry.test";
const CHANGES_SEED: [u8; 32] = [9u8; 32];
/// The producer whose changes the subscriptions watch.
const PRODUCER: u8 = 1;
/// The subscription owner: not the producer, so a private context of the
/// producer's is hidden from it.
const OWNER: u8 = 2;

type State = Arc<AppStateInner<SqliteStore>>;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn config() -> RegistryConfig {
    let mut cfg = RegistryConfig::defaults();
    cfg.registry.authority = AUTHORITY.into();
    cfg.auth.enabled = true;
    cfg.auth.did_methods = vec!["did:web".into(), "did:key".into()];
    cfg.receipt.signing_key_seed_b64 = b64(&[99u8; 32]);
    cfg.lifecycle.enabled = true;
    cfg.changes.enabled = true;
    cfg.changes.key_b64 = b64(&CHANGES_SEED);
    cfg.events.enabled = true;
    cfg.events.max_subscriptions_per_requester = 10;
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

fn key() -> ChangesKey {
    ChangesKey::from_master(&CHANGES_SEED, 1).unwrap()
}

fn did_of(seed: u8) -> String {
    acdp::did::key::did_key_from_ed25519(&SigningKey::from_bytes(&[seed; 32]).verifying_key_bytes())
}

fn bearer_for(seed: u8) -> String {
    forged_bearer(&did_of(seed), &uuid::Uuid::new_v4().to_string(), 600)
}

/// Loopback is allowed so the in-process receiver is reachable; every
/// other private range still is not.
fn loopback_policy() -> SsrfPolicy {
    SsrfPolicy {
        allow_loopback_resolved: true,
        ..SsrfPolicy::default()
    }
}

fn loopback() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

/// One request the receiver saw.
#[derive(Debug, Clone)]
struct Seen {
    signature: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

/// An HTTPS hint receiver: records each POST to `/hook` and answers with
/// `status` (a 302 points at `/elsewhere`, which counts its own hits).
/// `/slow` answers only after the registry's request bound.
#[derive(Clone)]
struct Receiver {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    status: Arc<AtomicU16>,
    redirected: Arc<AtomicUsize>,
}

impl Receiver {
    async fn spawn() -> Self {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(AtomicU16::new(200));
        let redirected = Arc::new(AtomicUsize::new(0));
        let (s, st, r) = (seen.clone(), status.clone(), redirected.clone());
        let app = axum::Router::new()
            .route(
                "/hook",
                post(move |headers: HeaderMap, body: Bytes| {
                    let (s, st) = (s.clone(), st.clone());
                    async move {
                        let h = |n: &str| {
                            headers
                                .get(n)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned)
                        };
                        s.lock().unwrap().push(Seen {
                            signature: h(SIGNATURE_HEADER),
                            content_type: h("content-type"),
                            body: body.to_vec(),
                        });
                        let code = StatusCode::from_u16(st.load(Ordering::SeqCst)).unwrap();
                        axum::response::Response::builder()
                            .status(code)
                            .header("location", "/elsewhere")
                            .body(Body::from("receiver body, never read"))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/slow",
                post(|| async {
                    tokio::time::sleep(REQUEST_TIMEOUT + Duration::from_secs(5)).await;
                    StatusCode::OK
                }),
            )
            .route(
                "/elsewhere",
                post(move || {
                    let r = r.clone();
                    async move {
                        r.fetch_add(1, Ordering::SeqCst);
                        StatusCode::OK
                    }
                }),
            );
        let addr = didweb::spawn_tls_server(app).await;
        Self {
            addr,
            seen,
            status,
            redirected,
        }
    }

    fn url(&self) -> String {
        format!(
            "https://{}:{}/hook",
            didweb::DIDWEB_AUTHORITY,
            self.addr.port()
        )
    }

    fn slow_url(&self) -> String {
        self.url().replace("/hook", "/slow")
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn answer(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }
}

/// Delivery bounds for the harness: short, so a timeout path costs seconds
/// rather than the production 5 s lookup and 10 s request (a mutation run
/// pays it once per mutant), yet ample for a loopback receiver.
const TEST_TIMEOUTS: HintTimeouts = HintTimeouts {
    lookup: Duration::from_secs(2),
    connect: Duration::from_secs(2),
    request: Duration::from_secs(2),
};

/// A harness whose state trusts the fixture CA and resolves the receiver's
/// host to loopback (or as `resolver` says).
async fn harness_with(resolver: StaticResolver) -> (SeededHarness, State) {
    let (cfg, caps) = common::with_anonymous_public_reads(config(), caps(), true);
    let mut h = SeededHarness::new(cfg.clone(), caps, AUTHORITY).await;
    let state = h.rebuild_shared_with(cfg, |s| {
        s.with_callback_resolver(Arc::new(resolver))
            .with_callback_policy(loopback_policy())
            .with_hint_timeouts(TEST_TIMEOUTS)
            .with_hint_root_pem(didweb::ca_pem().as_bytes())
            .expect("fixture CA")
    });
    (h, state)
}

fn receiver_resolver() -> StaticResolver {
    StaticResolver::new().with(didweb::DIDWEB_AUTHORITY, &[loopback()])
}

async fn harness() -> (SeededHarness, State) {
    harness_with(receiver_resolver()).await
}

/// Another state over the same server and store, as a second replica (or
/// the same one after its resolver's answers changed).
fn replica(h: &mut SeededHarness, resolver: impl CallbackResolver + 'static) -> State {
    h.rebuild_shared_with(config(), |s| {
        s.with_callback_resolver(Arc::new(resolver))
            .with_callback_policy(loopback_policy())
            .with_hint_timeouts(TEST_TIMEOUTS)
            .with_hint_root_pem(didweb::ca_pem().as_bytes())
            .expect("fixture CA")
    })
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    let req = match body {
        Some(v) => b
            .header("content-type", "application/acdp+json")
            .body(Body::from(serde_json::to_vec(&v).unwrap()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    (status, common::body_to_json_lenient(resp).await)
}

/// Subscribe `OWNER` to `PRODUCER`'s lifecycle changes, delivered to `url`.
async fn subscribe(h: &SeededHarness, url: &str) -> Subscription {
    let (status, v) = call(
        &h.router,
        "POST",
        "/subscriptions",
        &bearer_for(OWNER),
        Some(json!({
            "filter": {"agent_id": [did_of(PRODUCER)], "kind": ["lifecycle"]},
            "callback_url": url,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    Subscription::from_value(&v).unwrap_or_else(|e| panic!("{e}: {v}"))
}

async fn subscription_status(h: &SeededHarness, id: &str) -> SubscriptionStatus {
    let (status, v) = call(
        &h.router,
        "GET",
        &format!("/subscriptions/{id}"),
        &bearer_for(OWNER),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    Subscription::from_value(&v).unwrap().status
}

/// Publish a context as `PRODUCER` and retract it: one lifecycle entry.
async fn retracted(h: &SeededHarness, title: &str, vis: Visibility) {
    let producer = Producer::new_did_key(SigningKey::from_bytes(&[PRODUCER; 32]));
    let req = producer
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(vis)
        .build()
        .unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "publish {title} = {v}");
    let ctx_id = v["ctx_id"].as_str().unwrap().to_string();

    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    let did = did_of(PRODUCER);
    let key_id = acdp::did::key::did_key_url(&did).unwrap();
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        acdp::types::primitives::CtxId(ctx_id.clone()),
        LifecycleEventType::parse("retracted").unwrap(),
        chrono::Utc::now(),
        acdp::types::primitives::AgentDid::new(did),
        None,
    )
    .unwrap()
    .sign_with(SigningKey::from_bytes(&[PRODUCER; 32]), key_id)
    .unwrap();
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/contexts/{}/retract",
                    pct_encode_path_segment(&ctx_id)
                ))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", bearer_for(PRODUCER)))
                .body(Body::from(
                    serde_json::to_vec(&json!({ "event": event })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "retract {title}");
}

async fn head(state: &State) -> u64 {
    state.server.store().changes_head().await.unwrap().head
}

async fn pass(state: &State, now_ms: i64) -> PassStats {
    run_hint_pass(state, now_ms).await.expect("hint pass")
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A hint is a signed, content-free `acdp-change-hint/1` (rec-007 C).
fn assert_valid_hint(seen: &Seen, sub: &Subscription, at_ms: i64, head: u64) {
    assert_eq!(seen.content_type.as_deref(), Some("application/json"));
    let header = seen.signature.as_deref().expect("ACDP-Signature header");
    let secret = sub.secret.as_deref().expect("creation carried the secret");
    let at = chrono::DateTime::from_timestamp_millis(at_ms).unwrap();
    verify_hint_signature(secret, header, &seen.body, at)
        .unwrap_or_else(|e| panic!("signature must verify: {e}"));
    assert!(
        verify_hint_signature("not the secret", header, &seen.body, at).is_err(),
        "another secret must not verify"
    );
    let v: Value = serde_json::from_slice(&seen.body).unwrap();
    let mut members: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    members.sort_unstable();
    assert_eq!(
        members,
        [
            "head_cursor",
            "hint_version",
            "registry_authority",
            "subscription_id"
        ],
        "a hint carries no change content: {v}"
    );
    let hint: ChangeHint = serde_json::from_value(v).unwrap();
    assert_eq!(hint.hint_version, "acdp-change-hint/1");
    assert_eq!(hint.subscription_id, sub.subscription_id);
    assert_eq!(hint.registry_authority, AUTHORITY);
    assert_eq!(key().decode_cursor(&hint.head_cursor).unwrap(), head);
}

// ─── rec-007 C: a signed hint, coalesced ────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec007_c_one_signed_hint_covers_every_change_since_the_last() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;

    // Nothing yet: the subscription starts at the head.
    let t0 = now_ms();
    assert_eq!(pass(&state, t0).await.claimed, 0);

    for n in 0..5 {
        retracted(&h, &format!("five-{n}"), Visibility::Public).await;
    }
    let stats = pass(&state, t0).await;
    assert_eq!((stats.claimed, stats.delivered), (1, 1), "{stats:?}");
    let seen = rx.seen();
    assert_eq!(seen.len(), 1, "five changes, one hint");
    assert_valid_hint(&seen[0], &sub, t0, head(&state).await);

    // Caught up: nothing more is sent.
    assert_eq!(pass(&state, t0 + 60_000).await.claimed, 0);
    assert_eq!(rx.seen().len(), 1);

    // The next change is held for the minimum interval, then hinted.
    retracted(&h, "after", Visibility::Public).await;
    assert_eq!(pass(&state, t0 + HINT_MIN_INTERVAL_MS - 1).await.claimed, 0);
    let t1 = t0 + HINT_MIN_INTERVAL_MS;
    assert_eq!(pass(&state, t1).await.delivered, 1);
    let seen = rx.seen();
    assert_eq!(seen.len(), 2);
    assert_valid_hint(&seen[1], &sub, t1, head(&state).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec007_c_changes_during_an_outage_coalesce_into_one_hint() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    rx.answer(503);
    let t0 = now_ms();

    retracted(&h, "outage-0", Visibility::Public).await;
    let stats = pass(&state, t0).await;
    assert_eq!(stats.failed, 1, "{stats:?}");
    for n in 1..5 {
        retracted(&h, &format!("outage-{n}"), Visibility::Public).await;
    }
    // Not due again before the backoff.
    assert_eq!(pass(&state, t0 + retry_delay_ms(1) - 1).await.claimed, 0);
    assert_eq!(rx.seen().len(), 1);

    rx.answer(204);
    let t1 = t0 + retry_delay_ms(1);
    assert_eq!(pass(&state, t1).await.delivered, 1);
    let seen = rx.seen();
    assert_eq!(seen.len(), 2, "the retry is the one hint for all five");
    assert_valid_hint(&seen[1], &sub, t1, head(&state).await);
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Active
    );
}

// ─── rec-007 H: only changes the owner may see ──────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec007_h_a_hidden_change_sends_no_hint() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    let t0 = now_ms();

    // The producer's private context: matches the filter, hidden from the
    // owner.
    retracted(&h, "hidden", Visibility::Private).await;
    let stats = pass(&state, t0).await;
    assert_eq!((stats.claimed, stats.nothing_visible), (1, 1), "{stats:?}");
    assert!(
        rx.seen().is_empty(),
        "no hint for a change the owner cannot see"
    );
    // It advanced past the hidden entry: not claimed again.
    assert_eq!(pass(&state, t0).await.claimed, 0);

    retracted(&h, "visible", Visibility::Public).await;
    assert_eq!(pass(&state, t0).await.delivered, 1);
    let seen = rx.seen();
    assert_eq!(seen.len(), 1);
    assert_valid_hint(&seen[0], &sub, t0, head(&state).await);
}

// ─── rec-007 G: failure, backoff, disable ───────────────────────────────────

/// A redirect is a failed delivery and is never followed (§8.3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_is_a_failure_and_is_not_followed() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    subscribe(&h, &rx.url()).await;
    rx.answer(302);
    retracted(&h, "redirected", Visibility::Public).await;
    let stats = pass(&state, now_ms()).await;
    assert_eq!(stats.failed, 1, "{stats:?}");
    assert_eq!(rx.seen().len(), 1);
    assert_eq!(
        rx.redirected.load(Ordering::SeqCst),
        0,
        "the redirect was followed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_deliveries_back_off_then_disable_after_72_hours() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    rx.answer(500);
    retracted(&h, "failing", Visibility::Public).await;

    let t0 = now_ms();
    let mut t = t0;
    let mut attempts = 0u32;
    // Retry while the backoff schedule says so, short of 72 h.
    loop {
        let stats = pass(&state, t).await;
        assert_eq!(
            stats.failed,
            1,
            "attempt {attempts} at +{}ms: {stats:?}",
            t - t0
        );
        attempts += 1;
        let next = t + retry_delay_ms(attempts);
        // Not due a millisecond early.
        assert_eq!(pass(&state, next - 1).await.claimed, 0);
        t = next;
        if t - t0 >= DISABLE_AFTER_MS {
            break;
        }
    }
    assert_eq!(rx.seen().len(), attempts as usize);
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Active
    );

    // The first failure at or past 72 h (the next scheduled retry) disables
    // it.
    let stats = pass(&state, t).await;
    assert_eq!(stats.disabled, 1, "{stats:?}");
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Disabled
    );
    // A disabled subscription is never tried again, even once the receiver
    // recovers.
    rx.answer(200);
    retracted(&h, "after-disable", Visibility::Public).await;
    assert_eq!(pass(&state, t + 10 * RETRY_CAP).await.claimed, 0);
    assert_eq!(rx.seen().len(), attempts as usize + 1);
}

const RETRY_CAP: i64 = 3_600_000;

/// A recovered delivery clears the failure run: a later failure starts its
/// 72 hours afresh.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delivery_resets_the_failure_run() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    let t0 = now_ms();
    rx.answer(500);
    retracted(&h, "a", Visibility::Public).await;
    assert_eq!(pass(&state, t0).await.failed, 1);
    rx.answer(200);
    let t1 = t0 + DISABLE_AFTER_MS - 1;
    assert_eq!(pass(&state, t1).await.delivered, 1);

    rx.answer(500);
    retracted(&h, "b", Visibility::Public).await;
    // Due again after the minimum interval, past t0 + 72 h.
    let t2 = t1 + HINT_MIN_INTERVAL_MS;
    let stats = pass(&state, t2).await;
    assert_eq!(stats.failed, 1, "a fresh run, not 72 h old: {stats:?}");
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Active
    );
}

// ─── §8.3 SSRF at every delivery ────────────────────────────────────────────

/// The callback host passed at creation, then resolved to a private
/// address: the delivery is refused before any connection, and retried.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_that_rebinds_to_a_private_address_is_refused() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    retracted(&h, "rebound", Visibility::Public).await;

    let rebound = replica(
        &mut h,
        StaticResolver::new().with(
            didweb::DIDWEB_AUTHORITY,
            &[loopback(), "10.0.0.5".parse().unwrap()],
        ),
    );
    let t0 = now_ms();
    let stats = pass(&rebound, t0).await;
    assert_eq!(stats.ssrf_refused, 1, "{stats:?}");
    assert!(rx.seen().is_empty(), "nothing was sent");

    // Refusals back off like failures; once the host resolves cleanly the
    // hint goes out.
    let healed = replica(&mut h, receiver_resolver());
    assert_eq!(pass(&healed, t0 + retry_delay_ms(1) - 1).await.claimed, 0);
    assert_eq!(pass(&healed, t0 + retry_delay_ms(1)).await.delivered, 1);
    assert_eq!(rx.seen().len(), 1);
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Active
    );
}

/// A host that no longer resolves is refused too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_that_stops_resolving_is_refused() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    subscribe(&h, &rx.url()).await;
    retracted(&h, "gone", Visibility::Public).await;
    let gone = replica(&mut h, StaticResolver::new());
    let stats = pass(&gone, now_ms()).await;
    assert_eq!(stats.ssrf_refused, 1, "{stats:?}");
    assert!(rx.seen().is_empty());
}

/// Answers loopback on the first lookup and `then` afterwards: the
/// registry's pre-check passes, the connection's own lookup does not.
struct FlipResolver {
    calls: AtomicUsize,
    then: IpAddr,
}

#[async_trait::async_trait]
impl CallbackResolver for FlipResolver {
    async fn lookup(&self, _host: &str, port: u16) -> Result<Vec<SocketAddr>, AcdpError> {
        let ip = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            loopback()
        } else {
            self.then
        };
        Ok(vec![SocketAddr::new(ip, port)])
    }
}

/// A rebinding between the pre-check and the connection is refused by
/// the connection's own resolver: nothing reaches the receiver.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rebinding_after_the_pre_check_is_refused_at_connect() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    subscribe(&h, &rx.url()).await;
    retracted(&h, "flip", Visibility::Public).await;
    let flip = replica(
        &mut h,
        FlipResolver {
            calls: AtomicUsize::new(0),
            then: "10.0.0.5".parse().unwrap(),
        },
    );
    let stats = pass(&flip, now_ms()).await;
    assert_eq!(stats.failed, 1, "{stats:?}");
    assert!(rx.seen().is_empty(), "nothing was sent");
}

/// Never answers.
struct HangingResolver;

#[async_trait::async_trait]
impl CallbackResolver for HangingResolver {
    async fn lookup(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>, AcdpError> {
        std::future::pending().await
    }
}

/// A callback host whose nameserver never answers costs the lookup bound,
/// not a delivery slot forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lookup_that_never_answers_is_bounded() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    subscribe(&h, &rx.url()).await;
    retracted(&h, "hang", Visibility::Public).await;
    let hanging = replica(&mut h, HangingResolver);
    let started = std::time::Instant::now();
    let stats = pass(&hanging, now_ms()).await;
    assert_eq!(stats.ssrf_refused, 1, "{stats:?}");
    assert!(
        started.elapsed() < TEST_TIMEOUTS.lookup + Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert!(rx.seen().is_empty());
}

/// A receiver slower than the request bound is a failure, after the bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_receiver_times_out_as_a_failure() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.slow_url()).await;
    retracted(&h, "slow", Visibility::Public).await;
    let started = std::time::Instant::now();
    let stats = pass(&state, now_ms()).await;
    assert_eq!(stats.failed, 1, "{stats:?}");
    let took = started.elapsed();
    assert!(
        took >= TEST_TIMEOUTS.request && took < TEST_TIMEOUTS.request + Duration::from_secs(4),
        "{took:?}"
    );
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Active
    );
}

/// A secret sealed under a `key_id` this replica does not hold (a
/// rotation, a rollback, a replica on the wrong key) is retried, not
/// disabled: nothing is sent, the key coming back resumes delivery, and
/// only 72 h of it disables the subscription.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_key_retries_then_disables() {
    let rx = Receiver::spawn().await;
    let (mut h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    retracted(&h, "rotated", Visibility::Public).await;
    let mut cfg = config();
    cfg.changes.key_b64 = b64(&[10u8; 32]);
    cfg.changes.key_id = cfg.changes.key_id.wrapping_add(1);
    let rotated = h.rebuild_shared_with(cfg, |s| {
        s.with_callback_resolver(Arc::new(receiver_resolver()))
            .with_callback_policy(loopback_policy())
            .with_hint_root_pem(didweb::ca_pem().as_bytes())
            .expect("fixture CA")
    });
    let t0 = now_ms();
    let stats = pass(&rotated, t0).await;
    assert_eq!(stats.unknown_key, 1, "{stats:?}");
    assert!(rx.seen().is_empty());
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Active
    );
    // The original key back (a rollback): the retry delivers.
    let stats = pass(&state, t0 + 3_600_000).await;
    assert_eq!(stats.delivered, 1, "{stats:?}");
    assert_eq!(rx.seen().len(), 1);

    // Under the unknown key for 72 h: disabled.
    retracted(&h, "rotated again", Visibility::Public).await;
    let t1 = t0 + 3_600_000 + HINT_MIN_INTERVAL_MS;
    assert_eq!(pass(&rotated, t1).await.unknown_key, 1);
    let stats = pass(&rotated, t1 + DISABLE_AFTER_MS).await;
    assert_eq!(stats.disabled, 1, "{stats:?}");
    assert_eq!(rx.seen().len(), 1);
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Disabled
    );
}

/// A secret that fails to open under the right `key_id` (tampered, or a
/// new key installed without a new `key_id`) can never be signed for: it
/// is disabled at once, nothing sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_secret_disables_the_subscription() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    retracted(&h, "rotated", Visibility::Public).await;
    let mut cfg = config();
    cfg.changes.key_b64 = b64(&[10u8; 32]);
    let rotated = h.rebuild_shared_with(cfg, |s| {
        s.with_callback_resolver(Arc::new(receiver_resolver()))
            .with_callback_policy(loopback_policy())
            .with_hint_root_pem(didweb::ca_pem().as_bytes())
            .expect("fixture CA")
    });
    let stats = pass(&rotated, now_ms()).await;
    assert_eq!(stats.disabled, 1, "{stats:?}");
    assert!(rx.seen().is_empty());
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Disabled
    );
    assert_eq!(pass(&rotated, now_ms() + DISABLE_AFTER_MS).await.claimed, 0);
}

/// A position pruned out of retention (the worker was down that long)
/// is re-scanned from the oldest retained entry, so the hint still goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pruned_position_rescans_from_the_oldest_entry() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    retracted(&h, "pruned", Visibility::Public).await;
    retracted(&h, "kept", Visibility::Public).await;
    // Drop only the oldest entry: the subscription's position is now
    // before the retained range.
    assert_eq!(
        state
            .server
            .store()
            .prune_changes(i64::MAX, 1)
            .await
            .unwrap(),
        1
    );
    let stats = pass(&state, now_ms()).await;
    assert_eq!(stats.delivered, 1, "{stats:?}");
    let seen = rx.seen();
    assert_eq!(seen.len(), 1);
    let hint: ChangeHint = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(hint.subscription_id, sub.subscription_id);
    // The position caught up to the head.
    assert_eq!(
        pass(&state, now_ms() + HINT_MIN_INTERVAL_MS).await.claimed,
        0
    );
}

/// A stored owner DID that no longer parses can never be evaluated as its
/// owner: the subscription is disabled, not retried forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_owner_disables_the_subscription() {
    use acdp_registry_store::{CreateSubscriptionOutcome, NewSubscription};
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let store = state.server.store();
    let id = uuid::Uuid::new_v4().to_string();
    let created = store
        .create_subscription(
            &NewSubscription {
                subscription_id: id.clone(),
                owner_did: "not a did".into(),
                tenant_id: None,
                filter: serde_json::from_value(json!({"kind": ["lifecycle"]})).unwrap(),
                callback_url: rx.url(),
                secret_ct: vec![0u8; 48],
                secret_key_id: 1,
                created_at_ms: now_ms(),
                last_hinted_seq: 0,
            },
            10,
        )
        .await
        .unwrap();
    assert_eq!(created, CreateSubscriptionOutcome::Created);
    retracted(&h, "orphan", Visibility::Public).await;
    let stats = pass(&state, now_ms()).await;
    assert_eq!(stats.disabled, 1, "{stats:?}");
    assert!(rx.seen().is_empty());
    let row = store
        .get_subscription("not a did", &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, SubscriptionStatus::Disabled);
}

/// A subscription created without a tenant, once `auth.require_tenant` is
/// set: its owner's own `GET /changes` would now be refused, so it is
/// disabled rather than evaluated across every tenant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tenantless_subscription_disables_under_require_tenant() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    retracted(&h, "tenantless", Visibility::Public).await;
    let mut cfg = config();
    cfg.auth.require_tenant = true;
    let strict = h.rebuild_shared_with(cfg, |s| {
        s.with_callback_resolver(Arc::new(receiver_resolver()))
            .with_callback_policy(loopback_policy())
            .with_hint_root_pem(didweb::ca_pem().as_bytes())
            .expect("fixture CA")
    });
    let stats = pass(&strict, now_ms()).await;
    assert_eq!(stats.disabled, 1, "{stats:?}");
    assert!(rx.seen().is_empty());
    assert_eq!(
        subscription_status(&h, &sub.subscription_id).await,
        SubscriptionStatus::Disabled
    );
}

// ─── replicas and the worker ────────────────────────────────────────────────

/// Two replicas passing at once deliver one hint between them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_replicas_deliver_once() {
    let rx = Receiver::spawn().await;
    let (mut h, a) = harness().await;
    subscribe(&h, &rx.url()).await;
    let b = replica(&mut h, receiver_resolver());
    retracted(&h, "raced", Visibility::Public).await;
    let t0 = now_ms();
    let (sa, sb) = tokio::join!(pass(&a, t0), pass(&b, t0));
    assert_eq!(sa.delivered + sb.delivered, 1, "{sa:?} {sb:?}");
    assert_eq!(rx.seen().len(), 1);
}

/// A deleted subscription is never hinted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_subscription_gets_no_hint() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    let (status, _) = call(
        &h.router,
        "DELETE",
        &format!("/subscriptions/{}", sub.subscription_id),
        &bearer_for(OWNER),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    retracted(&h, "after-delete", Visibility::Public).await;
    assert_eq!(pass(&state, now_ms()).await.claimed, 0);
    assert!(rx.seen().is_empty());
}

/// The spawned worker is woken by the commit that grows the feed, not by
/// its timer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_commit_wakes_the_worker() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let sub = subscribe(&h, &rx.url()).await;
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let worker = spawn_hint_worker(Arc::clone(&state), Duration::from_secs(3600), stop_rx);
    retracted(&h, "wake", Visibility::Public).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while rx.seen().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no hint within 10 s of the commit"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Shutdown is cooperative: the worker ends on the signal by itself.
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the worker stops on the shutdown signal")
        .unwrap();
    let seen = rx.seen();
    assert_eq!(seen.len(), 1);
    let v: ChangeHint = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(v.subscription_id, sub.subscription_id);
}

/// A superseding publish wakes the worker too: the publish path, not only
/// the lifecycle path, signals the commit that grew the feed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_superseding_publish_wakes_the_worker() {
    let rx = Receiver::spawn().await;
    let (h, state) = harness().await;
    let (status, v) = call(
        &h.router,
        "POST",
        "/subscriptions",
        &bearer_for(OWNER),
        Some(json!({
            "filter": {"agent_id": [did_of(PRODUCER)], "kind": ["superseded"]},
            "callback_url": rx.url(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let producer = Producer::new_did_key(SigningKey::from_bytes(&[PRODUCER; 32]));
    let req = producer
        .publish_request()
        .title("v1")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap();
    let (status, v1) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "{v1}");
    let v1 = v1["ctx_id"].as_str().unwrap().to_string();
    let (status, prev) = call(
        &h.router,
        "GET",
        &format!("/contexts/{}/body", pct_encode_path_segment(&v1)),
        &bearer_for(OWNER),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prev}");
    let prev: acdp::types::body::Body = serde_json::from_value(prev).unwrap();

    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let worker = spawn_hint_worker(Arc::clone(&state), Duration::from_secs(3600), stop_rx);
    // Let the worker's start-up pass (and the plain publish's wake-up)
    // finish, so only the superseding commit can wake it again.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(rx.seen().is_empty());
    let req = producer
        .supersede_body(&prev)
        .title("v2")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap();
    let (status, v2) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "{v2}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while rx.seen().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no hint within 10 s of the superseding publish"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the worker stops on the shutdown signal")
        .unwrap();
}

/// With the events profile off, a pass does nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_pass_without_the_events_profile() {
    let rx = Receiver::spawn().await;
    let (mut h, _state) = harness().await;
    subscribe(&h, &rx.url()).await;
    retracted(&h, "off", Visibility::Public).await;
    let mut cfg = config();
    cfg.events.enabled = false;
    let off = h.rebuild_shared_with(cfg, |s| {
        s.with_callback_resolver(Arc::new(receiver_resolver()))
            .with_callback_policy(loopback_policy())
    });
    assert_eq!(pass(&off, now_ms()).await, PassStats::default());
    assert!(rx.seen().is_empty());
}
