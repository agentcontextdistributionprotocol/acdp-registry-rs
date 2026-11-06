//! RFC-ACDP-0018 §7: the SDK consumer (`acdp::client::recall`) against this
//! registry, end to end over real TLS.
//!
//! The registry's router (real SQLite store, receipts, lifecycle, change
//! feed) is served at `https://agents.test` by the `didweb` fixture TLS
//! server, so `origin_registry`, the client's authority and the
//! registry's `did:web` (whose document the same router serves) all agree,
//! as they do in a deployment. Every entry kind the feed carries is read
//! with `RegistryClient::changes` / `ChangeFeed` and decided by
//! `recall::verify_change`, the procedure a real consumer runs; forged
//! entries must be rejected.

// `common` is SQLite-backed; see `caps_visibility.rs` for why every
// harness-using test binary carries this gate.
#![cfg(feature = "storage-sqlite")]

mod common;
// Only the fixture TLS server and its CA are used here.
#[allow(dead_code)]
mod didweb;

use acdp::client::{
    verify_change, ChangeEffect, ChangeFeed, ChangeVerdict, RegistryClient, SeenChanges, Since,
    VerificationPolicy,
};
use acdp::crypto::SigningKey;
use acdp::did::WebResolver;
use acdp::error::AcdpError;
use acdp::producer::Producer;
use acdp::types::body::Body as AcdpBody;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::change::{Change, ChangeHint, ChangeKind, WatchFilter};
use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
use acdp_registry_types::RegistryConfig;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use common::{body_to_json, forged_bearer_for, pct_encode_path_segment, publish, SeededHarness};
use serde_json::{json, Value};
use tower::ServiceExt;

const AUTHORITY: &str = didweb::DIDWEB_AUTHORITY;
const PRODUCER: u8 = 0x51;
/// A second agent that revokes its own key; kept apart from `PRODUCER`
/// so the revocation cannot taint the producer's other entries.
const REVOKER: u8 = 0x54;
const REVOKED: u8 = 0x55;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn config() -> RegistryConfig {
    let mut cfg = RegistryConfig::defaults();
    cfg.registry.authority = AUTHORITY.into();
    cfg.auth.enabled = true;
    cfg.auth.did_methods = vec!["did:web".into(), "did:key".into()];
    cfg.receipt.signing_key_seed_b64 = b64(&[0x52; 32]);
    cfg.lifecycle.enabled = true;
    cfg.changes.enabled = true;
    cfg.changes.key_b64 = b64(&[0x53; 32]);
    cfg
}

fn caps() -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: "0.5.0".into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        supported_signature_algorithms: vec!["ed25519".into()],
        supported_did_methods: vec!["did:web".into(), "did:key".into()],
        profiles: vec![
            "acdp-registry-core".into(),
            "acdp-registry-lifecycle".into(),
            "acdp-registry-changes".into(),
        ],
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
            change_retention_seconds: Some(2_592_000),
            max_change_filters: Some(100),
            max_changes_page: Some(100),
            max_change_bucket_length: None,
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

fn producer() -> Producer {
    agent(PRODUCER)
}

fn agent(seed: u8) -> Producer {
    Producer::new_did_key(SigningKey::from_bytes(&[seed; 32]))
}

fn did_of(seed: u8) -> String {
    acdp::did::key::did_key_from_ed25519(&SigningKey::from_bytes(&[seed; 32]).verifying_key_bytes())
}

/// The registry, served over TLS as `agents.test`, plus a consumer's
/// client and resolver pinned to it.
struct World {
    h: SeededHarness,
    client: RegistryClient,
    resolver: WebResolver,
}

async fn world() -> World {
    let (cfg, caps) = common::with_anonymous_public_reads(config(), caps(), true);
    let h = SeededHarness::new(cfg, caps, AUTHORITY).await;
    let addr = didweb::spawn_tls_server(h.router.clone()).await;
    let ca = didweb::ca_pem().as_bytes();
    let client =
        RegistryClient::with_test_endpoint(&format!("https://{AUTHORITY}"), addr, ca).unwrap();
    let resolver = WebResolver::with_test_endpoint(ca, AUTHORITY, addr).unwrap();
    World {
        h,
        client,
        resolver,
    }
}

async fn publish_public(h: &SeededHarness, title: &str) -> String {
    let req = producer()
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "publish {title} = {v}");
    v["ctx_id"].as_str().unwrap().to_string()
}

async fn supersede(h: &SeededHarness, old: &str) -> String {
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/contexts/{}/body", pct_encode_path_segment(old)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let prev: AcdpBody = serde_json::from_value(body_to_json(resp).await).unwrap();
    let req = producer()
        .supersede_body(&prev)
        .title("next version")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::CREATED, "supersede {old} = {v}");
    v["ctx_id"].as_str().unwrap().to_string()
}

async fn retract(h: &SeededHarness, ctx_id: &str) {
    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    let did = did_of(PRODUCER);
    let key_id = acdp::did::key::did_key_url(&did).unwrap();
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        CtxId(ctx_id.to_string()),
        LifecycleEventType::parse("retracted").unwrap(),
        chrono::Utc::now(),
        AgentDid::new(did.clone()),
        None,
    )
    .unwrap()
    .sign_with(SigningKey::from_bytes(&[PRODUCER; 32]), key_id)
    .unwrap();
    let bearer = forged_bearer_for(AUTHORITY, &did, &uuid::Uuid::new_v4().to_string(), 600);
    let resp = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/contexts/{}/retract",
                    pct_encode_path_segment(ctx_id)
                ))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::from(
                    serde_json::to_vec(&json!({ "event": event })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    assert_eq!(
        status,
        StatusCode::OK,
        "retract {ctx_id} = {}",
        body_to_json(resp).await
    );
}

/// `REVOKER` publishes a public key-revocation context for another key
/// (a key may not attest its own compromise, RFC-ACDP-0014 §5).
async fn revoke_a_key(h: &SeededHarness) -> String {
    let fp = acdp::crypto::fingerprint::fingerprint_ed25519(
        &SigningKey::from_bytes(&[REVOKED; 32]).verifying_key_bytes(),
    );
    let req = agent(REVOKER)
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

/// Both agents (OR across `agent_id`, §6).
fn agents_filter() -> WatchFilter {
    WatchFilter::new()
        .agent_id(AgentDid::new(did_of(PRODUCER)))
        .agent_id(AgentDid::new(did_of(REVOKER)))
}

fn producer_filter() -> WatchFilter {
    WatchFilter::new().agent_id(AgentDid::new(did_of(PRODUCER)))
}

async fn decide(w: &World, change: &Change) -> ChangeVerdict {
    verify_change(
        &w.client,
        &w.resolver,
        change,
        None,
        &VerificationPolicy::default(),
    )
    .await
    .unwrap_or_else(|e| panic!("entry {} undecidable: {e}", change.seq()))
}

/// The three kinds of change, read with `ChangeFeed` two at a
/// time and each verified by the §7 procedure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_entry_kind_verifies_through_the_sdk() {
    let w = world().await;
    let retracted = publish_public(&w.h, "to retract").await;
    let old = publish_public(&w.h, "to supersede").await;
    let new = supersede(&w.h, &old).await;
    retract(&w.h, &retracted).await;
    let revocation = revoke_a_key(&w.h).await;

    let mut feed = ChangeFeed::new(&w.client, agents_filter()).limit(2);
    let mut seen = SeenChanges::new();
    let authority = w.client.authority().unwrap();
    assert_eq!(authority, AUTHORITY);
    let mut entries = Vec::new();
    let mut pages = 0;
    loop {
        let page = feed.fetch().await.expect("page");
        pages += 1;
        assert!(page.changes.len() <= 2);
        for change in &page.changes {
            assert!(seen.first_time(&authority, change.seq()), "seq repeats");
            entries.push(change.clone());
        }
        feed.advance(&page);
        if !page.has_more {
            break;
        }
    }
    assert!(pages >= 2, "a limit of 2 pages the three entries");
    let kinds: Vec<ChangeKind> = entries.iter().map(Change::kind).collect();
    assert_eq!(
        kinds,
        [
            ChangeKind::Superseded,
            ChangeKind::Lifecycle,
            ChangeKind::KeyRevocation
        ],
        "{entries:?}"
    );

    for change in &entries {
        match (change.kind(), decide(&w, change).await) {
            (
                ChangeKind::Superseded,
                ChangeVerdict::Verified(ChangeEffect::Superseded { new_version }),
            ) => {
                assert_eq!(change.ctx_id().as_str(), old);
                assert_eq!(new_version.body().ctx_id.as_str(), new);
            }
            (
                ChangeKind::Lifecycle,
                ChangeVerdict::Verified(ChangeEffect::Lifecycle { retracted: r, .. }),
            ) => {
                assert_eq!(change.ctx_id().as_str(), retracted);
                assert!(r, "the context is now retracted");
            }
            (
                ChangeKind::KeyRevocation,
                ChangeVerdict::Verified(ChangeEffect::KeyRevocation { .. }),
            ) => {
                assert_eq!(change.ctx_id().as_str(), revocation);
            }
            (kind, verdict) => panic!("{kind:?} entry {}: {verdict:?}", change.seq()),
        }
    }

    // Caught up: the next page from the saved position is empty.
    let page = feed.fetch().await.unwrap();
    assert!(page.changes.is_empty() && !page.has_more);
}

/// A tampered entry is a final rejection, never an effect.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_entries_are_rejected() {
    let w = world().await;
    let retracted = publish_public(&w.h, "to retract").await;
    let old = publish_public(&w.h, "to supersede").await;
    supersede(&w.h, &old).await;
    retract(&w.h, &retracted).await;
    revoke_a_key(&w.h).await;
    let page = w
        .client
        .changes(&Since::Start, &agents_filter(), None)
        .await
        .unwrap();
    assert_eq!(page.changes.len(), 3);

    for change in &page.changes {
        let mut raw = serde_json::to_value(change).unwrap();
        // Flip one byte of the receipt signature the entry carries.
        let sig = match change.kind() {
            ChangeKind::Lifecycle => &mut raw["event_receipt"]["signature"]["value"],
            _ => &mut raw["registry_receipt"]["signature"]["value"],
        };
        let s = sig.as_str().unwrap().to_owned();
        let flipped = if s.starts_with('A') { "B" } else { "A" };
        *sig = Value::String(format!("{flipped}{}", &s[1..]));
        let forged = Change::from_value(&raw).unwrap();
        let verdict = decide(&w, &forged).await;
        assert!(
            verdict.rejection().is_some(),
            "{:?} with a forged receipt verified: {verdict:?}",
            change.kind()
        );
    }

    // An entry about one context that claims another's evidence.
    let mut raw = serde_json::to_value(&page.changes[1]).unwrap();
    raw["ctx_id"] = Value::String(old.clone());
    let verdict = decide(&w, &Change::from_value(&raw).unwrap()).await;
    assert!(verdict.rejection().is_some(), "{verdict:?}");
}

/// The SDK's view of the §5 cursor errors and the head.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursor_errors_map_to_sdk_errors() {
    let w = world().await;
    let head = w
        .client
        .changes(&Since::Head, &WatchFilter::new(), None)
        .await
        .unwrap();
    assert!(head.changes.is_empty() && !head.has_more);

    // `since=head` then a change: the change, from the head cursor.
    let ctx = publish_public(&w.h, "after head").await;
    retract(&w.h, &ctx).await;
    let page = w
        .client
        .changes(
            &Since::Cursor(head.next_cursor.clone()),
            &WatchFilter::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.changes[0].ctx_id().as_str(), ctx);

    let mut tampered = head.next_cursor.clone().into_bytes();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    let err = w
        .client
        .changes(
            &Since::Cursor(String::from_utf8(tampered).unwrap()),
            &WatchFilter::new(),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(&err, AcdpError::InvalidCursor(_)), "{err:?}");
}

/// A position older than the retained feed is `CursorExpired`; the
/// SDK's recovery (`reset_to_head`) then reads on from the head.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pruned_position_is_cursor_expired() {
    use acdp_registry_store::ExtendedRegistryStore as _;
    let w = world().await;
    let mut feed = ChangeFeed::new(&w.client, producer_filter()).since(Since::Head);
    let head = feed.fetch().await.unwrap();
    feed.advance(&head);
    for title in ["pruned", "kept"] {
        let ctx = publish_public(&w.h, title).await;
        retract(&w.h, &ctx).await;
    }
    // Retention removes the oldest entry, which the feed's position needs.
    assert_eq!(w.h.store().prune_changes(i64::MAX, 1).await.unwrap(), 1);
    let err = feed.fetch().await.unwrap_err();
    assert!(matches!(err, AcdpError::CursorExpired), "{err:?}");

    feed.reset_to_head();
    let at_head = feed.fetch().await.unwrap();
    assert!(at_head.changes.is_empty() && !at_head.has_more);
    feed.advance(&at_head);
    let ctx = publish_public(&w.h, "after reset").await;
    retract(&w.h, &ctx).await;
    let page = feed.fetch().await.unwrap();
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.changes[0].ctx_id().as_str(), ctx);
}

/// A hint names this registry: `on_hint` reads from the feed's own
/// position, not the hint's (rec-007 E: the hint's `head_cursor` is past
/// the change, so reading from it would miss it), and a hint from
/// another registry is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_hint_reads_from_the_feeds_own_position() {
    let w = world().await;
    let mut feed = ChangeFeed::new(&w.client, producer_filter()).since(Since::Head);
    let head = feed.fetch().await.unwrap();
    feed.advance(&head);
    let ctx = publish_public(&w.h, "hinted").await;
    retract(&w.h, &ctx).await;
    let after = w
        .client
        .changes(&Since::Head, &WatchFilter::new(), None)
        .await
        .unwrap();
    assert_ne!(after.next_cursor, head.next_cursor);
    let hint = ChangeHint::new(
        uuid::Uuid::new_v4().to_string(),
        AUTHORITY.to_string(),
        after.next_cursor,
    );
    let page = feed.on_hint(&hint).await.unwrap();
    assert_eq!(page.changes.len(), 1);
    assert_eq!(page.changes[0].ctx_id().as_str(), ctx);

    let elsewhere = ChangeHint::new(
        uuid::Uuid::new_v4().to_string(),
        "other.test".to_string(),
        head.next_cursor,
    );
    assert!(feed.on_hint(&elsewhere).await.is_err());
}
