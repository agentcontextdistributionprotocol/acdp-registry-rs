//! RFC-ACDP-0018 §5–§6: `GET /changes` over the real router and a real
//! SQLite store, with lifecycle, receipts and event receipts wired the way
//! the binary's `serve_with_store` wires them.
//!
//! Each test names the `rec-*` fixture scenario it makes concrete. The feed
//! rows themselves (one per change, same transaction) are pinned at the
//! store layer; this file pins what a consumer sees over HTTP: paging,
//! cursors, filters, visibility and retention.

// `common` is SQLite-backed; see `caps_visibility.rs` for why every
// harness-using test binary carries this gate.
#![cfg(feature = "storage-sqlite")]

mod common;

use acdp::crypto::SigningKey;
use acdp::producer::Producer;
use acdp::types::body::Body as AcdpBody;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::primitives::{AgentDid, ContextType, Visibility};
use acdp_registry_core::changes_key::ChangesKey;
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryConfig;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use common::{body_to_json, forged_bearer, pct_encode_path_segment, publish, SeededHarness};
use serde_json::{json, Value};
use tower::ServiceExt;

const AUTHORITY: &str = "registry.test";
const CHANGES_SEED: [u8; 32] = [9u8; 32];

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn config(changes: bool) -> RegistryConfig {
    let mut cfg = RegistryConfig::defaults();
    cfg.registry.authority = AUTHORITY.into();
    // Per-requester visibility needs bearers to be read at all.
    cfg.auth.enabled = true;
    cfg.auth.did_methods = vec!["did:web".into(), "did:key".into()];
    cfg.receipt.signing_key_seed_b64 = b64(&[99u8; 32]);
    cfg.lifecycle.enabled = true;
    cfg.changes.enabled = changes;
    cfg.changes.key_b64 = b64(&CHANGES_SEED);
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

async fn harness(changes: bool) -> SeededHarness {
    let (cfg, caps) = common::with_anonymous_public_reads(config(changes), caps(), true);
    SeededHarness::new(cfg, caps, AUTHORITY).await
}

fn key() -> ChangesKey {
    ChangesKey::from_master(&CHANGES_SEED, 1).unwrap()
}

fn did_key_producer(seed: u8) -> Producer {
    Producer::new_did_key(SigningKey::from_bytes(&[seed; 32]))
}

fn did_of(seed: u8) -> String {
    acdp::did::key::did_key_from_ed25519(&SigningKey::from_bytes(&[seed; 32]).verifying_key_bytes())
}

async fn get(app: &axum::Router, uri: &str, bearer: Option<&str>) -> (StatusCode, Value) {
    let mut b = Request::builder().uri(uri);
    if let Some(t) = bearer {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let resp = app
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    (status, common::body_to_json_lenient(resp).await)
}

async fn changes(app: &axum::Router, query: &str, bearer: Option<&str>) -> (StatusCode, Value) {
    let uri = if query.is_empty() {
        "/changes".to_string()
    } else {
        format!("/changes?{query}")
    };
    get(app, &uri, bearer).await
}

/// `/changes` 200, with the page parsed (and validated) by the SDK type a
/// consumer would use.
async fn page(
    app: &axum::Router,
    query: &str,
    bearer: Option<&str>,
) -> acdp::types::change::ChangesResponse {
    let (status, v) = changes(app, query, bearer).await;
    assert_eq!(status, StatusCode::OK, "/changes?{query} = {v}");
    acdp::types::change::ChangesResponse::from_value(&v)
        .unwrap_or_else(|e| panic!("/changes?{query} does not validate: {e}: {v}"))
}

fn enc(s: &str) -> String {
    pct_encode_path_segment(s)
}

async fn publish_ctx(
    h: &SeededHarness,
    seed: u8,
    title: &str,
    vis: Visibility,
) -> (String, String) {
    publish_for(h, seed, title, vis, &[]).await
}

/// `publish_ctx` with an `audience` of the given seeds' DIDs (for
/// `restricted`).
async fn publish_for(
    h: &SeededHarness,
    seed: u8,
    title: &str,
    vis: Visibility,
    audience: &[u8],
) -> (String, String) {
    let producer = did_key_producer(seed);
    let mut b = producer
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(vis);
    if !audience.is_empty() {
        b = b.audience(audience.iter().map(|s| AgentDid::new(did_of(*s))).collect());
    }
    let req = b.build().unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "publish {title} = {v}");
    (
        v["ctx_id"].as_str().unwrap().to_string(),
        v["lineage_id"].as_str().unwrap().to_string(),
    )
}

async fn body_of(h: &SeededHarness, ctx_id: &str, bearer: Option<&str>) -> AcdpBody {
    let (status, v) = get(
        &h.router,
        &format!("/contexts/{}/body", pct_encode_path_segment(ctx_id)),
        bearer,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body of {ctx_id} = {v}");
    serde_json::from_value(v).unwrap()
}

async fn supersede(
    h: &SeededHarness,
    seed: u8,
    old: &str,
    vis: Visibility,
    bearer: Option<&str>,
) -> String {
    supersede_for(h, seed, old, vis, bearer, &[]).await
}

/// `supersede` with an `audience` of the given seeds' DIDs.
async fn supersede_for(
    h: &SeededHarness,
    seed: u8,
    old: &str,
    vis: Visibility,
    bearer: Option<&str>,
    audience: &[u8],
) -> String {
    let prev = body_of(h, old, bearer).await;
    let producer = did_key_producer(seed);
    let mut b = producer
        .supersede_body(&prev)
        .title("next version")
        .context_type(ContextType::DataSnapshot)
        .visibility(vis);
    if !audience.is_empty() {
        b = b.audience(audience.iter().map(|s| AgentDid::new(did_of(*s))).collect());
    }
    let req = b.build().unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "supersede {old} = {v}");
    v["ctx_id"].as_str().unwrap().to_string()
}

fn signed_event(seed: u8, ctx_id: &str, event_type: &str) -> Value {
    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    let key = SigningKey::from_bytes(&[seed; 32]);
    let did = did_of(seed);
    let key_id = acdp::did::key::did_key_url(&did).unwrap();
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        acdp::types::primitives::CtxId(ctx_id.to_string()),
        LifecycleEventType::parse(event_type).unwrap(),
        chrono::Utc::now(),
        AgentDid::new(did),
        None,
    )
    .unwrap()
    .sign_with(key, key_id)
    .unwrap();
    serde_json::to_value(&event).unwrap()
}

/// POST a lifecycle event; returns the served `registry_state`.
async fn lifecycle(
    h: &SeededHarness,
    seed: u8,
    ctx_id: &str,
    endpoint: &str,
    event_type: &str,
) -> Value {
    let envelope = json!({ "event": signed_event(seed, ctx_id, event_type) });
    // The producer's own bearer: a private context is only addressable by
    // its owner.
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/contexts/{}/{endpoint}",
                    pct_encode_path_segment(ctx_id)
                ))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", bearer_for(seed)))
                .body(Body::from(serde_json::to_vec(&envelope).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let v = body_to_json(resp).await;
    assert_eq!(status, StatusCode::OK, "{endpoint} {ctx_id} = {v}");
    v["registry_state"].clone()
}

async fn retract(h: &SeededHarness, seed: u8, ctx_id: &str) -> Value {
    lifecycle(h, seed, ctx_id, "retract", "retracted").await
}

fn bearer_for(seed: u8) -> String {
    forged_bearer(&did_of(seed), &uuid::Uuid::new_v4().to_string(), 600)
}

fn seqs(p: &acdp::types::change::ChangesResponse) -> Vec<u64> {
    p.changes.iter().map(|c| c.seq()).collect()
}

/// Every change visible from the start, following `next_cursor` `limit`
/// at a time; asserts the paging invariants on the way.
async fn drain(app: &axum::Router, filters: &str, limit: u64, bearer: Option<&str>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut since: Option<String> = None;
    for _ in 0..100 {
        let mut q = format!("limit={limit}");
        if let Some(c) = &since {
            q.push_str(&format!("&since={}", enc(c)));
        }
        if !filters.is_empty() {
            q.push('&');
            q.push_str(filters);
        }
        let (status, v) = changes(app, &q, bearer).await;
        assert_eq!(status, StatusCode::OK, "{q}: {v}");
        let entries = v["changes"].as_array().unwrap().clone();
        assert!(entries.len() as u64 <= limit, "{q}: page over limit: {v}");
        let more = v["has_more"].as_bool().unwrap();
        since = Some(v["next_cursor"].as_str().unwrap().to_string());
        out.extend(entries);
        if !more {
            return out;
        }
    }
    panic!("feed never reached has_more=false");
}

// ─── rec-001: feed mechanics ────────────────────────────────────────────────

/// rec-001 A–F: an empty feed still hands out a cursor; entries come in
/// strictly increasing `seq`; `limit` bounds a page and `has_more` says
/// whether to keep reading; `next_cursor` resumes exactly after the last
/// entry; `since=head` starts at the head and sees only what comes after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec001_feed_mechanics() {
    let h = harness(true).await;

    let empty = page(&h.router, "", None).await;
    assert!(empty.changes.is_empty());
    assert!(!empty.has_more);
    assert_eq!(key().decode_cursor(&empty.next_cursor), Ok(0));

    let (a, _) = publish_ctx(&h, 41, "rec001 a", Visibility::Public).await;
    let (b, _) = publish_ctx(&h, 41, "rec001 b", Visibility::Public).await;
    retract(&h, 41, &a).await;
    lifecycle(&h, 41, &a, "republish", "republished").await;
    retract(&h, 41, &b).await;

    let full = page(&h.router, "", None).await;
    assert_eq!(seqs(&full), vec![1, 2, 3]);
    assert!(!full.has_more);

    let first = page(&h.router, "limit=2", None).await;
    assert_eq!(seqs(&first), vec![1, 2]);
    assert!(first.has_more);
    let rest = page(
        &h.router,
        &format!("limit=2&since={}", enc(&first.next_cursor)),
        None,
    )
    .await;
    assert_eq!(seqs(&rest), vec![3]);
    assert!(!rest.has_more);

    // Re-reading from the same cursor is repeatable (durable cursors).
    let again = page(
        &h.router,
        &format!("limit=2&since={}", enc(&first.next_cursor)),
        None,
    )
    .await;
    assert_eq!(seqs(&again), vec![3]);

    // At the head: nothing, and the cursor stays put.
    let tail = page(
        &h.router,
        &format!("since={}", enc(&rest.next_cursor)),
        None,
    )
    .await;
    assert!(tail.changes.is_empty() && !tail.has_more);
    assert_eq!(key().decode_cursor(&tail.next_cursor), Ok(3));

    // since=head skips history and sees only the next change.
    let head = page(&h.router, "since=head", None).await;
    assert!(head.changes.is_empty() && !head.has_more);
    assert_eq!(key().decode_cursor(&head.next_cursor), Ok(3));
    lifecycle(&h, 41, &b, "republish", "republished").await;
    let after = page(
        &h.router,
        &format!("since={}", enc(&head.next_cursor)),
        None,
    )
    .await;
    assert_eq!(seqs(&after), vec![4]);
    assert_eq!(after.changes[0].ctx_id().as_str(), b);
}

/// rec-001: an unparseable or forged cursor is `invalid_cursor`; a cursor
/// this registry cannot place (another key, or past the head) is
/// `cursor_expired`, so a consumer restarts rather than silently skipping.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec001_cursor_errors() {
    let h = harness(true).await;
    let (a, _) = publish_ctx(&h, 42, "rec001 cursors", Visibility::Public).await;
    retract(&h, 42, &a).await;

    let mut tampered = key().encode_cursor(1).into_bytes();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    for bad in [
        "not-a-cursor".to_string(),
        String::from_utf8(tampered).unwrap(),
        "A".repeat(500),
    ] {
        let (status, v) = changes(&h.router, &format!("since={}", enc(&bad)), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
        assert_eq!(v["error"]["code"], "invalid_cursor", "{bad}: {v}");
    }
    let (status, v) = changes(&h.router, "since=", None).await;
    assert_eq!(
        (status, v["error"]["code"].clone()),
        (StatusCode::BAD_REQUEST, json!("invalid_cursor"))
    );

    let other_key = ChangesKey::from_master(&[8u8; 32], 2)
        .unwrap()
        .encode_cursor(1);
    let beyond_head = key().encode_cursor(99);
    for expired in [other_key, beyond_head] {
        let (status, v) = changes(&h.router, &format!("since={}", enc(&expired)), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["error"]["code"], "cursor_expired", "{v}");
    }
}

/// §5.1 request validation: `limit` must be a positive integer (named in
/// `details.field`), one above max_changes_page is clamped; repeated scalars and malformed filters are
/// `schema_violation`; more than `max_change_filters` filters are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changes_request_validation() {
    let h = harness(true).await;
    for q in ["limit=0", "limit=x", "limit=-1"] {
        let (status, v) = changes(&h.router, q, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q}: {v}");
        assert_eq!(v["error"]["code"], "schema_violation", "{q}: {v}");
        assert_eq!(v["error"]["details"]["field"], "limit", "{q}: {v}");
    }
    let (status, v) = changes(&h.router, "limit=101", None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "limit above the max is clamped: {v}"
    );
    assert!(v["next_cursor"].is_string(), "{v}");
    for q in [
        "limit=1&limit=2",
        "since=head&since=head",
        "kind=deleted",
        "ctx_id=not-a-ctx",
        "agent_id=nope",
    ] {
        let (status, v) = changes(&h.router, q, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q}: {v}");
        assert_eq!(v["error"]["code"], "schema_violation", "{q}: {v}");
    }
    let many: Vec<String> = (0..101)
        .map(|i| format!("agent_id={}", enc(&format!("did:web:agents.test:a{i}"))))
        .collect();
    let (status, v) = changes(&h.router, &many.join("&"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "schema_violation", "{v}");
    // Unknown parameters are ignored; the max itself is accepted.
    let ok = page(&h.router, "limit=100&future_param=1", None).await;
    assert!(ok.changes.is_empty());
}

/// A registry that does not serve the profile answers 501, not 404.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changes_disabled_is_501() {
    let h = harness(false).await;
    let (status, v) = changes(&h.router, "", None).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{v}");
    assert_eq!(v["error"]["code"], "not_implemented", "{v}");
}

// ─── rec-002: one entry per change, evidence as served ──────────────────────

/// rec-002: a plain publish writes nothing; each lifecycle event and each
/// superseding publish writes exactly one entry, whose evidence is
/// byte-identical to what `registry_state` serves for the same change.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec002_one_entry_per_change_with_served_evidence() {
    let h = harness(true).await;
    let (v1, lineage) = publish_ctx(&h, 43, "rec002", Visibility::Public).await;
    assert!(
        page(&h.router, "", None).await.changes.is_empty(),
        "plain publish writes nothing"
    );

    let state = retract(&h, 43, &v1).await;
    let v2 = supersede(&h, 43, &v1, Visibility::Public, None).await;

    let (_, raw) = changes(&h.router, "", None).await;
    let entries = raw["changes"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{raw}");

    let lc = &entries[0];
    assert_eq!(lc["kind"], "lifecycle");
    assert_eq!(lc["ctx_id"], v1.as_str());
    assert_eq!(lc["lineage_id"], lineage.as_str());
    assert_eq!(lc["agent_id"], did_of(43).as_str());
    assert_eq!(lc["event"], state["lifecycle_events"][0], "event as served");
    assert_eq!(
        lc["event_receipt"], state["event_receipts"][0],
        "receipt as served"
    );

    let sup = &entries[1];
    assert_eq!(sup["kind"], "superseded");
    assert_eq!(sup["ctx_id"], v1.as_str());
    assert_eq!(sup["superseded_by"], v2.as_str());
    let (_, served_v2) = get(
        &h.router,
        &format!("/contexts/{}", pct_encode_path_segment(&v2)),
        None,
    )
    .await;
    assert_eq!(
        sup["registry_receipt"], served_v2["registry_receipt"],
        "the new version's receipt, as served"
    );
    assert!(sup.get("event").is_none() && lc.get("superseded_by").is_none());
}

// ─── rec-003: watch filters ─────────────────────────────────────────────────

/// rec-003 A–D: ids OR together, `kind` ANDs with them, and an empty
/// filtered page still advances the cursor past what it scanned.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec003_watch_filters() {
    let h = harness(true).await;
    let (a, lineage_a) = publish_ctx(&h, 44, "rec003 a", Visibility::Public).await;
    let (b, _) = publish_ctx(&h, 45, "rec003 b", Visibility::Public).await;
    let (c, _) = publish_ctx(&h, 46, "rec003 c", Visibility::Public).await;
    retract(&h, 44, &a).await; // 1: a
    retract(&h, 45, &b).await; // 2: b
    retract(&h, 46, &c).await; // 3: c
    let a2 = supersede(&h, 44, &a, Visibility::Public, None).await; // 4: a superseded

    let ids = |entries: &[Value]| -> Vec<u64> {
        entries.iter().map(|e| e["seq"].as_u64().unwrap()).collect()
    };
    // A: ctx_id.
    assert_eq!(
        ids(&drain(&h.router, &format!("ctx_id={}", enc(&a)), 1, None).await),
        vec![1, 4]
    );
    // B: OR across ctx_id and agent_id.
    let q = format!("ctx_id={}&agent_id={}", enc(&b), enc(&did_of(46)));
    assert_eq!(ids(&drain(&h.router, &q, 2, None).await), vec![2, 3]);
    // C: lineage_id, ANDed with kind.
    let q = format!("lineage_id={}&kind=superseded", enc(&lineage_a));
    let sup = drain(&h.router, &q, 5, None).await;
    assert_eq!(ids(&sup), vec![4]);
    assert_eq!(sup[0]["superseded_by"], a2.as_str());
    let q = format!(
        "lineage_id={}&kind=lifecycle&kind=superseded",
        enc(&lineage_a)
    );
    assert_eq!(ids(&drain(&h.router, &q, 5, None).await), vec![1, 4]);
    // D: kind alone.
    assert_eq!(
        ids(&drain(&h.router, "kind=lifecycle", 2, None).await),
        vec![1, 2, 3]
    );

    // An unmatched filter returns no entries but a cursor at the head, so
    // a poller never rescans what it has already been told is empty.
    let none = page(&h.router, "agent_id=did%3Aweb%3Aagents.test%3Anobody", None).await;
    assert!(none.changes.is_empty() && !none.has_more);
    assert_eq!(key().decode_cursor(&none.next_cursor), Ok(4));
}

// ─── rec-004: visibility ────────────────────────────────────────────────────

/// rec-004 A–C: entries are post-filtered per requester. Hidden entries
/// never end a page early — a page fills from later visible entries — and
/// the owner sees what an anonymous reader does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec004_visibility_post_filters_without_short_pages() {
    let h = harness(true).await;
    let owner = bearer_for(47);
    let mut private = Vec::new();
    for i in 0..5 {
        let (id, _) =
            publish_ctx(&h, 47, &format!("rec004 private {i}"), Visibility::Private).await;
        private.push(id);
    }
    let (public, _) = publish_ctx(&h, 48, "rec004 public", Visibility::Public).await;
    for id in &private {
        retract(&h, 47, id).await; // 1..=5, private
    }
    retract(&h, 48, &public).await; // 6, public

    // A: anonymous, limit=1 — the page skips the five hidden entries and
    // still delivers the visible one.
    let anon = page(&h.router, "limit=1", None).await;
    assert_eq!(seqs(&anon), vec![6]);
    assert!(!anon.has_more);
    // Watching a hidden ctx_id anonymously yields nothing, not an error.
    let hidden = page(&h.router, &format!("ctx_id={}", enc(&private[0])), None).await;
    assert!(hidden.changes.is_empty());

    // B: the owner sees all six, in order, across pages.
    let all = drain(&h.router, "", 4, Some(&owner)).await;
    let s: Vec<u64> = all.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(s, vec![1, 2, 3, 4, 5, 6]);

    // C: another authenticated agent is no better than anonymous.
    let stranger = bearer_for(49);
    assert_eq!(seqs(&page(&h.router, "", Some(&stranger)).await), vec![6]);
}

/// rec-004 D–E: a `superseded` entry is visible only when the new version
/// is retrievable by the requester — superseding a public context with a
/// private one must not disclose the successor's existence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec004_superseded_visible_only_with_its_successor() {
    let h = harness(true).await;
    let owner = bearer_for(50);
    let (v1, _) = publish_ctx(&h, 50, "rec004 sup", Visibility::Public).await;
    let v2 = supersede(&h, 50, &v1, Visibility::Private, None).await;

    let anon = page(&h.router, &format!("ctx_id={}", enc(&v1)), None).await;
    assert!(
        anon.changes.is_empty(),
        "successor is private: {:?}",
        seqs(&anon)
    );
    let mine = page(&h.router, &format!("ctx_id={}", enc(&v1)), Some(&owner)).await;
    assert_eq!(mine.changes.len(), 1);
    let entry = serde_json::to_value(&mine.changes[0]).unwrap();
    assert_eq!(entry["superseded_by"], v2.as_str());
}

/// rec-004 D: more hidden entries than one request's scan budget
/// (`REFILL_MAX_SCANS` store pages). The anonymous reader gets an empty page
/// with `has_more: true` — not the end — and the next page reaches the
/// public retraction behind them. The store page is lowered to `limit` (2)
/// so the budget is 12 rows, not the production 600: the refill loop is the
/// same, and this test runs once per mutant in the mutation ratchet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec004_empty_page_is_not_the_end() {
    use acdp_registry_core::handlers::REFILL_MAX_SCANS;
    const LIMIT: u64 = 2;
    let base = config(true);
    let (cfg, caps) = common::with_anonymous_public_reads(base, caps(), true);
    let mut h = SeededHarness::new(cfg.clone(), caps, AUTHORITY).await;
    use acdp_registry_core::handlers::MIN_SCAN_ROWS;
    let raised = h.rebuild_shared_with(cfg.clone(), |s| s.with_changes_min_scan_rows(u32::MAX));
    assert_eq!(
        raised.changes_min_scan_rows, MIN_SCAN_ROWS,
        "a test can only lower it"
    );
    let lowered = h.rebuild_shared_with(cfg, |s| s.with_changes_min_scan_rows(0));
    assert_eq!(lowered.changes_min_scan_rows, 1);
    let (hidden, _) = publish_ctx(&h, 56, "rec004 hidden churn", Visibility::Private).await;
    let budget = REFILL_MAX_SCANS as u64 * LIMIT;
    for _ in 0..=budget / 2 {
        retract(&h, 56, &hidden).await;
        lifecycle(&h, 56, &hidden, "republish", "republished").await;
    }
    let (public, _) = publish_ctx(&h, 57, "rec004 public tail", Visibility::Public).await;
    retract(&h, 57, &public).await;
    let tail = budget + 3;

    let first = page(&h.router, &format!("limit={LIMIT}"), None).await;
    assert!(first.changes.is_empty(), "{:?}", seqs(&first));
    assert!(
        first.has_more,
        "an empty page with entries left is not the end"
    );
    assert_eq!(key().decode_cursor(&first.next_cursor), Ok(budget));
    let next = page(
        &h.router,
        &format!("limit={LIMIT}&since={}", enc(&first.next_cursor)),
        None,
    )
    .await;
    assert_eq!((seqs(&next), next.has_more), (vec![tail], false));
}

/// rec-004 E: a cursor carries a position, not a scope. One issued to the
/// owner and replayed anonymously yields only anonymous-visible entries,
/// and every cursor has the same length whatever lies behind it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec004_cursor_rescoping() {
    let h = harness(true).await;
    let owner = bearer_for(58);
    let (private, _) = publish_ctx(&h, 58, "rec004 e private", Visibility::Private).await;
    let (public, _) = publish_ctx(&h, 59, "rec004 e public", Visibility::Public).await;
    retract(&h, 58, &private).await; // 1
    retract(&h, 59, &public).await; // 2
    lifecycle(&h, 58, &private, "republish", "republished").await; // 3

    let mine = page(&h.router, "limit=1", Some(&owner)).await;
    assert_eq!(seqs(&mine), vec![1]);
    let replay = page(
        &h.router,
        &format!("since={}", enc(&mine.next_cursor)),
        None,
    )
    .await;
    assert_eq!(seqs(&replay), vec![2], "the owner's cursor grants nothing");

    let anon = page(&h.router, "", None).await;
    let cursors = [
        &mine.next_cursor,
        &replay.next_cursor,
        &anon.next_cursor,
        &page(&h.router, "since=head", None).await.next_cursor,
    ];
    assert!(
        cursors.iter().all(|c| c.len() == mine.next_cursor.len()),
        "cursor lengths differ: {cursors:?}"
    );
}

// ─── rec-005: retention ─────────────────────────────────────────────────────

/// rec-005 A–D: pruning removes a seq prefix; a cursor before the oldest
/// retained entry is `cursor_expired`; a cursor at the prune point and an
/// absent `since` both resume cleanly at the oldest retained entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec005_retention_and_expired_cursors() {
    let h = harness(true).await;
    let (a, _) = publish_ctx(&h, 51, "rec005", Visibility::Public).await;
    retract(&h, 51, &a).await; // 1
    lifecycle(&h, 51, &a, "republish", "republished").await; // 2
    let at_one = page(&h.router, "limit=1", None).await.next_cursor;
    let at_zero = key().encode_cursor(0);

    // A: the pruner, running at t0 + 2 h and at t0 + 29 d with the
    // advertised 30-day retention, deletes nothing; C1 still resumes.
    let now = chrono::Utc::now().timestamp_millis();
    let retention_ms = 2_592_000_000;
    for later in [2 * 3_600_000, 29 * 86_400_000] {
        let cutoff = now + later - retention_ms;
        assert_eq!(h.store().prune_changes(cutoff, 100).await.unwrap(), 0);
        let resumed = page(&h.router, &format!("since={}", enc(&at_one)), None).await;
        assert_eq!(seqs(&resumed), vec![2]);
    }

    // B: past retention, the oldest entry goes and C0 expires.
    let future = now + 3_600_000;
    assert_eq!(h.store().prune_changes(future, 1).await.unwrap(), 1);
    let head = h.store().changes_head().await.unwrap();
    assert_eq!((head.head, head.pruned_through), (2, 1));

    let (status, v) = changes(&h.router, &format!("since={}", enc(&at_zero)), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "cursor_expired");

    assert_eq!(
        seqs(&page(&h.router, &format!("since={}", enc(&at_one)), None).await),
        vec![2]
    );
    assert_eq!(seqs(&page(&h.router, "", None).await), vec![2]);

    // Pruning everything keeps the head: the feed is empty, not reset.
    assert_eq!(h.store().prune_changes(future, 100).await.unwrap(), 1);
    let drained = page(&h.router, "", None).await;
    assert!(drained.changes.is_empty() && !drained.has_more);
    assert_eq!(key().decode_cursor(&drained.next_cursor), Ok(2));
    retract(&h, 51, &a).await;
    assert_eq!(
        seqs(
            &page(
                &h.router,
                &format!("since={}", enc(&drained.next_cursor)),
                None
            )
            .await
        ),
        vec![3]
    );
}

// ─── capabilities ───────────────────────────────────────────────────────────

/// The feed's responses carry `Cache-Control: private` like every other
/// per-requester data-plane read, so a shared cache never replays one
/// requester's view to another.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changes_responses_are_private_to_the_requester() {
    let h = harness(true).await;
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/changes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cc = resp
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cc.contains("private"), "{cc}");
}

async fn publish_revocation(h: &SeededHarness, seed: u8, revoked_seed: u8) -> String {
    let fp = acdp::crypto::fingerprint::fingerprint_ed25519(
        &SigningKey::from_bytes(&[revoked_seed; 32]).verifying_key_bytes(),
    );
    let req = did_key_producer(seed)
        .publish_request()
        .title("key revocation")
        .context_type(ContextType::KeyRevocation)
        .visibility(Visibility::Public)
        .metadata(json!({
            "revoked_key_fingerprint": fp,
            "compromised_since": "2026-10-01T00:00:00.000Z",
        }))
        .build()
        .unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "revocation = {v}");
    v["ctx_id"].as_str().unwrap().to_string()
}

// ─── rec-002 D / rec-003 A: key revocations ─────────────────────────────────

/// rec-002 D: an accepted key-revocation context appends one
/// `key_revocation` entry carrying its own receipt. rec-003 A: watching a
/// context by `ctx_id` also matches revocations by that context's
/// producer — but only when the watched context is itself visible, so a
/// watch on a hidden ctx_id never confirms who produced it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec003_ctx_id_watch_matches_its_producers_key_revocations() {
    let h = harness(true).await;
    let (x, _) = publish_ctx(&h, 60, "rec003 x", Visibility::Public).await;
    let (hidden, _) = publish_ctx(&h, 62, "rec003 hidden", Visibility::Private).await;
    let (other, _) = publish_ctx(&h, 63, "rec003 other", Visibility::Public).await;
    retract(&h, 60, &x).await; // 1
    retract(&h, 63, &other).await; // 2
    let r = publish_revocation(&h, 60, 61).await; // 3
    let r2 = publish_revocation(&h, 62, 64).await; // 4

    let (_, raw) = changes(&h.router, "kind=key_revocation", None).await;
    let revs = raw["changes"].as_array().unwrap();
    assert_eq!(revs.len(), 2, "{raw}");
    assert_eq!(revs[0]["ctx_id"], r.as_str());
    assert_eq!(revs[0]["agent_id"], did_of(60).as_str());
    let (_, served) = get(
        &h.router,
        &format!("/contexts/{}", pct_encode_path_segment(&r)),
        None,
    )
    .await;
    assert_eq!(revs[0]["registry_receipt"], served["registry_receipt"]);
    assert!(revs[0].get("event").is_none() && revs[0].get("superseded_by").is_none());

    let ids = |p: acdp::types::change::ChangesResponse| seqs(&p);
    assert_eq!(
        ids(page(&h.router, &format!("ctx_id={}", enc(&x)), None).await),
        vec![1, 3]
    );
    let q = format!("ctx_id={}&kind=key_revocation", enc(&x));
    assert_eq!(ids(page(&h.router, &q, None).await), vec![3]);

    // The hidden context's producer is not disclosed to an anonymous
    // watcher, though the revocation itself is public in the full feed.
    let q = format!("ctx_id={}", enc(&hidden));
    assert!(page(&h.router, &q, None).await.changes.is_empty());
    let mine = page(&h.router, &q, Some(&bearer_for(62))).await;
    assert_eq!(seqs(&mine), vec![4]);
    assert_eq!(mine.changes[0].ctx_id().as_str(), r2);
}

// ─── fixture scenarios as the fixtures state them ───────────────────────────

/// rec-003 A–F with the fixture's own setup: P publishes X (lineage Lx)
/// and W (lineage Lw); X is retracted, W superseded by W2, and P's key
/// revoked by R; an unrelated Q's context V is retracted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec003_fixture_scenarios() {
    const P: u8 = 70;
    const Q: u8 = 71;
    let h = harness(true).await;
    let (x, _) = publish_ctx(&h, P, "rec003 X", Visibility::Public).await;
    let (w, lw) = publish_ctx(&h, P, "rec003 W", Visibility::Public).await;
    let (v, _) = publish_ctx(&h, Q, "rec003 V", Visibility::Public).await;
    retract(&h, P, &x).await; // 1
    let w2 = supersede(&h, P, &w, Visibility::Public, None).await; // 2
    let r = publish_revocation(&h, P, 72).await; // 3
    retract(&h, Q, &v).await; // 4

    let app = &h.router;
    let s = |q: String| async move { seqs(&page(app, &q, None).await) };
    // A: X's retraction and the revocation of X's producer.
    assert_eq!(s(format!("ctx_id={}", enc(&x))).await, vec![1, 3]);
    let a = page(&h.router, &format!("ctx_id={}", enc(&x)), None).await;
    assert_eq!(a.changes[1].ctx_id().as_str(), r);
    // B: W's superseded entry only.
    let b = page(&h.router, &format!("lineage_id={}", enc(&lw)), None).await;
    assert_eq!(seqs(&b), vec![2]);
    let entry = serde_json::to_value(&b.changes[0]).unwrap();
    assert_eq!(entry["ctx_id"], w.as_str());
    assert_eq!(entry["superseded_by"], w2.as_str());
    // C: OR across ctx_id and lineage_id.
    assert_eq!(
        s(format!("ctx_id={}&lineage_id={}", enc(&v), enc(&lw))).await,
        vec![2, 4]
    );
    // D: agent_id ANDed with kind.
    assert_eq!(
        s(format!("agent_id={}&kind=key_revocation", enc(&did_of(P)))).await,
        vec![3]
    );
    // E: kind alone.
    assert_eq!(s("kind=lifecycle".into()).await, vec![1, 4]);
    // F: 101 filter values exceed max_change_filters (100).
    let many: Vec<String> = (0..101)
        .map(|i| format!("agent_id={}", enc(&format!("did:web:agents.test:a{i}"))))
        .collect();
    let (status, body) = changes(&h.router, &many.join("&"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "schema_violation", "{body}");
}

/// rec-004 A–C with the fixture's own setup: public X, restricted Y
/// (audience [A]) and private Z are retracted; public W is superseded by
/// restricted W2 (audience [A]).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec004_fixture_scenarios() {
    const P: u8 = 73;
    const A: u8 = 74;
    let h = harness(true).await;
    let (x, _) = publish_ctx(&h, P, "rec004 X", Visibility::Public).await;
    let (y, _) = publish_for(&h, P, "rec004 Y", Visibility::Restricted, &[A]).await;
    let (z, _) = publish_ctx(&h, P, "rec004 Z", Visibility::Private).await;
    let (w, _) = publish_ctx(&h, P, "rec004 W", Visibility::Public).await;
    retract(&h, P, &x).await; // 1
    retract(&h, P, &y).await; // 2
    retract(&h, P, &z).await; // 3
    let w2 = supersede_for(&h, P, &w, Visibility::Restricted, None, &[A]).await; // 4

    // A: anonymous sees only X.
    assert_eq!(seqs(&page(&h.router, "", None).await), vec![1]);
    // B: the audience member sees X, Y and W→W2, never Z.
    let member = page(&h.router, "", Some(&bearer_for(A))).await;
    assert_eq!(seqs(&member), vec![1, 2, 4]);
    let sup = serde_json::to_value(&member.changes[2]).unwrap();
    assert_eq!(sup["superseded_by"], w2.as_str());
    // C: the producer sees all four.
    assert_eq!(
        seqs(&page(&h.router, "", Some(&bearer_for(P))).await),
        vec![1, 2, 3, 4]
    );
}

/// rec-002 B: republishing appends a lifecycle entry whose event is the
/// `republished` event the registry accepted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rec002_republish_appends_a_republished_entry() {
    let h = harness(true).await;
    let (x, _) = publish_ctx(&h, 75, "rec002 republish", Visibility::Public).await;
    retract(&h, 75, &x).await;
    let state = lifecycle(&h, 75, &x, "republish", "republished").await;
    let (_, raw) = changes(&h.router, "", None).await;
    let entries = raw["changes"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{raw}");
    assert_eq!(entries[1]["kind"], "lifecycle");
    assert_eq!(entries[1]["event"]["event_type"], "republished");
    let served = state["lifecycle_events"].as_array().unwrap();
    assert_eq!(&entries[1]["event"], served.last().unwrap());
}

/// Tenancy: a feed entry for a context stamped to one tenant is invisible
/// to a requester in another, exactly like the context itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changes_are_scoped_to_the_requesters_tenant() {
    let mut base = config(true);
    base.auth.tenant_header_trust = Some(acdp_registry_types::TenantHeaderTrust::AnyPeer);
    let (cfg, caps) = common::with_anonymous_public_reads(base, caps(), true);
    let h = SeededHarness::new(cfg, caps, AUTHORITY).await;
    let req = did_key_producer(76)
        .publish_request()
        .title("tenant a")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap();
    let send = |method: &str, uri: String, tenant: &str, body: Body| {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", bearer_for(76)))
            .header("X-Tenant-Id", tenant)
            .body(body)
            .unwrap();
        h.router.clone().oneshot(req)
    };
    let resp = send(
        "POST",
        "/contexts".into(),
        "tenant-a",
        Body::from(serde_json::to_vec(&req).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let ctx = body_to_json(resp).await["ctx_id"]
        .as_str()
        .unwrap()
        .to_string();
    let envelope = json!({ "event": signed_event(76, &ctx, "retracted") });
    let resp = send(
        "POST",
        format!("/contexts/{}/retract", pct_encode_path_segment(&ctx)),
        "tenant-a",
        Body::from(serde_json::to_vec(&envelope).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    for (tenant, want) in [("tenant-a", 1usize), ("tenant-b", 0)] {
        let resp = send("GET", "/changes".into(), tenant, Body::empty())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_to_json(resp).await;
        assert_eq!(
            v["changes"].as_array().unwrap().len(),
            want,
            "{tenant}: {v}"
        );
    }
}

/// With the public arm closed, `/changes` refuses an anonymous reader
/// (403 `not_authorized`), as search does, and serves an authenticated one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anonymous_changes_is_refused_when_public_reads_are_closed() {
    let (cfg, caps) = common::with_anonymous_public_reads(config(true), caps(), false);
    let h = SeededHarness::new(cfg, caps, AUTHORITY).await;
    let (status, body) = changes(&h.router, "", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "not_authorized", "{body}");
    let (status, body) = changes(&h.router, "", Some(&bearer_for(77))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
