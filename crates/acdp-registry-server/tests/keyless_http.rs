//! RFC-ACDP-0021 Phase 5: `POST/GET /key-attestations` over HTTP, on the
//! SQLite store (the memory and pg stores run the A/H/E core from the same
//! kit: `src/memory_ext.rs`, `tests/pg_integration.rs`).
//!
//! The registry is built in-process with `keyless.enabled`, against the
//! kit's loopback HTTPS issuer and DID host (the spawned binary is
//! `tests/keyless_e2e.rs`). Every
//! §5.2 refusal is checked for its exact `(status, code, details.reason)`,
//! and the order is checked by requests that fail two steps at once.

#![cfg(feature = "storage-sqlite")]

mod keyless_kit;

use std::net::SocketAddr;
use std::time::Duration;

use acdp::crypto::SigningKey;
use acdp::types::body::FullContext;
use acdp::types::keyless::{token_hash, KeyAttestation, WorkloadIdentity};
use acdp::types::receipt::RegistryReceipt;
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use axum::http::StatusCode;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use keyless_kit::{
    ed_key, ed_key_from, is, p256_key, unique, urlencode, wire, Kit, Opts, RECEIPT_SEED,
    REGISTRY_DID, SPIFFE_SUB, SUBJECT,
};
use serde_json::{json, Value};

async fn kit_with(opts: Opts) -> (Kit, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::connect(&dir.path().join("r.sqlite"), 1)
        .await
        .unwrap();
    let store = if opts.log {
        store.with_transparency_log()
    } else {
        store
    };
    store.migrate().await.unwrap();
    (Kit::new(store, opts).await, dir)
}

async fn kit() -> (Kit, tempfile::TempDir) {
    kit_with(Opts::default()).await
}

/// A controller listing the kit's OIDC [`SUBJECT`].
fn listed(kit: &Kit) -> String {
    kit.controller("c", &[(kit.oidc_identity(SUBJECT), false)])
}

/// The JSON of a valid request, for hand edits.
fn body(kit: &Kit, controller: &str) -> Value {
    serde_json::to_value(Kit::request(&ed_key(), controller, kit.oidc(SUBJECT), None)).unwrap()
}

/// A valid request whose `pop` is another key's.
fn bad_pop(controller: &str, descriptor: acdp::types::keyless::IdentityDescriptor) -> Value {
    let mut v = serde_json::to_value(Kit::request(
        &ed_key(),
        controller,
        descriptor.clone(),
        None,
    ))
    .unwrap();
    let other = Kit::request(&ed_key(), controller, descriptor, None);
    v["pop"] = json!(other.pop);
    v
}

/// A 64-byte P1363 `r || s` signature re-encoded as ASN.1 DER.
fn p1363_to_der_b64(p1363: &str) -> String {
    let raw = B64.decode(p1363).unwrap();
    assert_eq!(raw.len(), 64);
    let int = |b: &[u8]| {
        let mut v: Vec<u8> = b.iter().copied().skip_while(|x| *x == 0).collect();
        if v.first().is_none_or(|x| x & 0x80 != 0) {
            v.insert(0, 0);
        }
        let mut out = vec![0x02, v.len() as u8];
        out.extend(v);
        out
    };
    let (r, s) = (int(&raw[..32]), int(&raw[32..]));
    let mut der = vec![0x30, (r.len() + s.len()) as u8];
    der.extend(r);
    der.extend(s);
    B64.encode(der)
}

// ── kl-002 A/H/E and the 201 ───────────────────────────────────────────────

#[tokio::test]
async fn kl002_a_h_e_core_with_the_log() {
    let (kit, _d) = kit_with(Opts {
        log: true,
        ..Opts::default()
    })
    .await;
    keyless_kit::assert_mint_core(&kit, "sqlite").await;
}

/// §5.2 step 8: 201, `Location`, the full retrieval representation; the
/// body is the registry's, signed with its receipt key over its
/// `content_hash`; the receipt verifies against the same key and binds the
/// body; the attestation is retrievable and its key can sign (SDK).
#[tokio::test]
async fn the_201_is_a_verifiable_registry_signed_attestation() {
    let (kit, _d) = kit_with(Opts {
        log: true,
        ..Opts::default()
    })
    .await;
    let controller = listed(&kit);
    let key = ed_key();
    let mut c = kit.claims(SUBJECT);
    c["exp"] = json!(kit.now() + 900);
    let req = Kit::request(&key, &controller, kit.oidc_with(SUBJECT, &c), Some(600));
    let (status, headers, v) = kit
        .post_full(
            serde_json::to_vec(&req).unwrap(),
            "application/json",
            None,
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let full: FullContext = serde_json::from_value(v.clone()).expect("a FullContext");
    let ctx_id = full.body.ctx_id.as_str().to_string();
    let location = headers
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(location, format!("/contexts/{}", urlencode(&ctx_id)));

    // The body: hash binding and the registry's signature.
    acdp::crypto::verify::verify_context_hash(&full).expect("content_hash binds the body");
    let registry_pub = SigningKey::from_bytes(&RECEIPT_SEED).verifying_key_bytes();
    assert_eq!(full.body.signature.algorithm, "ed25519");
    assert!(full.body.signature.key_id.starts_with(REGISTRY_DID));
    acdp::crypto::verify_ed25519(
        &registry_pub,
        &full.body.signature.value,
        full.body.content_hash.as_str(),
    )
    .expect("signed by the receipt key");

    // The receipt.
    let receipt: RegistryReceipt =
        serde_json::from_value(full.registry_receipt.clone().expect("a receipt")).unwrap();
    receipt
        .verify_signature_with_key(Some(&registry_pub), None)
        .expect("receipt verifies");
    assert_eq!(receipt.ctx_id.as_str(), ctx_id);
    assert_eq!(receipt.content_hash, full.body.content_hash);

    // The attestation.
    let att = KeyAttestation::from_body(&full.body).expect("§5.3");
    assert_eq!(att.controller, controller);
    assert_eq!(att.key_fingerprint, key.fingerprint().unwrap());
    assert_eq!(att.token_hash, token_hash(req.identity.token()));
    assert_eq!((att.not_after - att.not_before).num_seconds(), 600);
    assert!(!att.org_wide);

    // Retrievable as an ordinary context, and usable by the SDK producer.
    let (s, got) = kit.get(&location).await;
    assert_eq!(s, StatusCode::OK, "{got}");
    assert_eq!(got["body"]["content_hash"], v["body"]["content_hash"]);
    key.into_producer(&full.body)
        .expect("the SDK accepts the attestation");
}

// ── §5.2 refusals ──────────────────────────────────────────────────────────

#[tokio::test]
async fn step_1_schema_refusals() {
    let (kit, _d) = kit().await;
    let controller = listed(&kit);
    let schema = is(400, "schema_violation", None);

    // K: a did:key controller.
    let mut v = body(&kit, &controller);
    v["controller"] = json!("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK");
    assert_eq!(wire(&kit.post(&v).await), schema, "K");
    // N: an X509-SVID identity.
    let mut v = body(&kit, &controller);
    v["identity"] =
        json!({"type": "spiffe", "subject": SPIFFE_SUB, "svid_format": "x509", "svid": "MIIB"});
    assert_eq!(wire(&kit.post(&v).await), schema, "N");
    // ttl 0 and 3601.
    for ttl in [0, 3601] {
        let mut v = body(&kit, &controller);
        v["requested_ttl_seconds"] = json!(ttl);
        assert_eq!(wire(&kit.post(&v).await), schema, "ttl {ttl}");
    }
    // An unknown member, a missing one, a wrong version.
    let mut v = body(&kit, &controller);
    v["extra"] = json!(1);
    assert_eq!(wire(&kit.post(&v).await), schema, "unknown member");
    let mut v = body(&kit, &controller);
    v.as_object_mut().unwrap().remove("pop");
    assert_eq!(wire(&kit.post(&v).await), schema, "missing pop");
    let mut v = body(&kit, &controller);
    v["request_version"] = json!("acdp-key-attestation-request/2");
    assert_eq!(wire(&kit.post(&v).await), schema, "version");
    // An algorithm string outside the closed set.
    let mut v = body(&kit, &controller);
    v["algorithm"] = json!("rsa");
    assert_eq!(wire(&kit.post(&v).await).1, "schema_violation", "algorithm");

    // Malformed JSON; the message never echoes the body (it may carry the
    // token).
    let token = kit.oidc(SUBJECT).token().to_string();
    let junk = format!("{{\"controller\": \"{controller}\", \"x\": {token}}}");
    let r = kit
        .post_raw(junk.into_bytes(), "application/json", None, &[])
        .await;
    assert_eq!(wire(&r), schema, "malformed");
    assert!(
        !r.1.to_string().contains(&token[..20]),
        "token echoed: {}",
        r.1
    );

    // The media type and the size limit.
    let r = kit.post_raw(b"{}".to_vec(), "text/plain", None, &[]).await;
    assert_eq!(r.0, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}", r.1);
    let r = kit
        .post_raw(vec![b' '; 2 * 1024 * 1024], "application/json", None, &[])
        .await;
    assert_eq!(r.0, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.1);
}

#[tokio::test]
async fn step_2_and_3_token_refusals() {
    let (kit, _d) = kit().await;
    let controller = listed(&kit);
    let post = |d| {
        let req = Kit::request(&ed_key(), &controller, d, None);
        let kit = &kit;
        async move { wire(&kit.post(&req).await) }
    };

    // D: an issuer not configured.
    let d = acdp::types::keyless::IdentityDescriptor::oidc(
        "https://evil.example",
        SUBJECT,
        kit.oidc.sign(&kit.claims(SUBJECT)),
    );
    assert_eq!(
        post(d).await,
        is(403, "key_not_authorized", Some("issuer_not_accepted")),
        "D"
    );

    // J: a bad signature; an expired token.
    let mut t = kit.oidc.sign(&kit.claims(SUBJECT));
    t.push_str("AAAA");
    let d = acdp::types::keyless::IdentityDescriptor::oidc(&kit.issuer, SUBJECT, t);
    let token_invalid = is(403, "key_not_authorized", Some("token_invalid"));
    assert_eq!(post(d).await, token_invalid, "J");
    let mut c = kit.claims(SUBJECT);
    c["exp"] = json!(kit.now() - 120);
    assert_eq!(
        post(kit.oidc_with(SUBJECT, &c)).await,
        token_invalid,
        "expired"
    );
    // A token whose sub is not the descriptor's subject.
    let c = kit.claims("repo:acme/other:ref:refs/heads/main");
    assert_eq!(post(kit.oidc_with(SUBJECT, &c)).await, token_invalid, "sub");

    // B/C: the audience.
    let audience = is(403, "key_not_authorized", Some("audience_mismatch"));
    let mut c = kit.claims(SUBJECT);
    c["aud"] = json!("did:web:other.test");
    assert_eq!(post(kit.oidc_with(SUBJECT, &c)).await, audience, "B");
    let mut c = kit.claims(SUBJECT);
    c["aud"] = json!(["did:web:other.test", "https://registry.test"]);
    assert_eq!(post(kit.oidc_with(SUBJECT, &c)).await, audience, "C");

    // F: cnf.jkt names another key; G: it names the presented one.
    let key = ed_key();
    let mut c = kit.claims(SUBJECT);
    c["cnf"] = json!({ "jkt": ed_key().jwk().unwrap().thumbprint().unwrap() });
    let req = Kit::request(&key, &controller, kit.oidc_with(SUBJECT, &c), None);
    assert_eq!(
        wire(&kit.post(&req).await),
        is(403, "key_not_authorized", Some("confirmation_mismatch")),
        "F"
    );
    let mut c = kit.claims(SUBJECT);
    c["cnf"] = json!({ "jkt": key.jwk().unwrap().thumbprint().unwrap() });
    let req = Kit::request(&key, &controller, kit.oidc_with(SUBJECT, &c), None);
    assert_eq!(kit.post(&req).await.0, StatusCode::CREATED, "G");

    // H: single use.
    let d = kit.oidc(SUBJECT);
    assert_eq!(
        kit.post(&Kit::request(&ed_key(), &controller, d.clone(), None))
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        post(d).await,
        is(403, "key_not_authorized", Some("token_replayed")),
        "H"
    );
}

#[tokio::test]
async fn step_3_an_unreachable_issuer_is_502() {
    let (kit, _d) = kit().await;
    let controller = listed(&kit);
    // Nothing cached yet, and the bundle endpoint fails.
    kit.outside.set_keys_status(503);
    let req = Kit::request(&ed_key(), &controller, kit.svid(), None);
    assert_eq!(
        wire(&kit.post(&req).await),
        is(
            502,
            "key_resolution_unreachable",
            Some("issuer_unreachable")
        )
    );
}

#[tokio::test]
async fn step_4_and_5_pop_and_algorithm() {
    let (kit, _d) = kit().await;
    let controller = listed(&kit);

    // I: the pop is another key's.
    let v = bad_pop(&controller, kit.oidc(SUBJECT));
    assert_eq!(
        wire(&kit.post(&v).await),
        is(400, "invalid_signature", Some("pop_invalid")),
        "I"
    );

    // R8: a P-256 pop in DER is refused, P1363 accepted.
    let key = p256_key();
    let req = Kit::request(&key, &controller, kit.oidc(SUBJECT), None);
    let mut v = serde_json::to_value(&req).unwrap();
    v["pop"] = json!(p1363_to_der_b64(&req.pop));
    assert_eq!(
        wire(&kit.post(&v).await),
        is(400, "invalid_signature", Some("pop_invalid")),
        "DER"
    );
    let req = Kit::request(&key, &controller, kit.oidc(SUBJECT), None);
    assert_eq!(kit.post(&req).await.0, StatusCode::CREATED, "P1363");

    // Step 5: `algorithm` does not name the key's algorithm.
    let mut v = body(&kit, &controller);
    v["algorithm"] = json!("ecdsa-p256");
    assert_eq!(
        wire(&kit.post(&v).await),
        is(400, "unsupported_algorithm", None),
        "algorithm"
    );
}

#[tokio::test]
async fn step_6_did_authorization() {
    let (kit, _d) = kit().await;
    let other = "repo:acme/other:ref:refs/heads/main";

    // E on a cold cache: one fetch, fresh, final.
    let controller = listed(&kit);
    let req = Kit::request(&ed_key(), &controller, kit.oidc(other), None);
    assert_eq!(
        wire(&kit.post(&req).await),
        is(403, "key_not_authorized", Some("identity_not_authorized")),
        "E cold"
    );
    assert_eq!(kit.did_hits(&controller), 1, "a cold cache fetches once");

    // A newly listed identity is honoured by the re-fetch (cached doc
    // does not list it): one more fetch, then 201.
    let controller = listed(&kit);
    let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
    assert_eq!(kit.post(&req).await.0, StatusCode::CREATED);
    kit.list(
        &controller,
        &[
            (kit.oidc_identity(SUBJECT), false),
            (kit.oidc_identity(other), true),
        ],
    );
    let req = Kit::request(&ed_key(), &controller, kit.oidc(other), None);
    let r = kit.post(&req).await;
    assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
    assert_eq!(kit.did_hits(&controller), 2);
    let att = KeyAttestation::from_metadata(&r.1["body"]["metadata"]).unwrap();
    assert!(att.org_wide, "org_wide comes from the listing");

    // A listed identity never forces a fetch.
    let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
    assert_eq!(kit.post(&req).await.0, StatusCode::CREATED);
    assert_eq!(kit.did_hits(&controller), 2);

    // An unlisted identity on a warm cache: one forced re-fetch, then the
    // final 403. Asked again at once, the re-fetch is rate-limited and the
    // cached document is not fresh, so the answer is 502, not a final 403.
    let controller = listed(&kit);
    let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
    assert_eq!(kit.post(&req).await.0, StatusCode::CREATED);
    assert_eq!(kit.did_hits(&controller), 1);
    let req = Kit::request(&ed_key(), &controller, kit.oidc(other), None);
    assert_eq!(
        wire(&kit.post(&req).await),
        is(403, "key_not_authorized", Some("identity_not_authorized")),
        "E warm"
    );
    assert_eq!(kit.did_hits(&controller), 2, "exactly one re-fetch");
    let req = Kit::request(&ed_key(), &controller, kit.oidc(other), None);
    assert_eq!(
        wire(&kit.post(&req).await),
        is(502, "key_resolution_unreachable", None),
        "stale and rate-limited"
    );
    assert_eq!(kit.did_hits(&controller), 2, "no fetch past the limit");

    // L: the controller's DID host fails.
    let controller = listed(&kit);
    kit.outside.set_did_status(500);
    let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
    let r = wire(&kit.post(&req).await);
    kit.outside.set_did_status(200);
    assert_eq!(
        (r.0, r.1.as_str()),
        (502, "key_resolution_unreachable"),
        "L"
    );
    // A DID with no document at all.
    let req = Kit::request(
        &ed_key(),
        "did:web:agents.test:nobody-here",
        kit.oidc(SUBJECT),
        None,
    );
    let r = wire(&kit.post(&req).await);
    // A non-2xx DID host, 404 included, is unreachable: RFC-ACDP-0001
    // §5.11 step 3, RFC-ACDP-0021 §5.2 step 6, kl-002 L.
    assert_eq!(
        (r.0, r.1.as_str()),
        (502, "key_resolution_unreachable"),
        "no document"
    );
}

#[tokio::test]
async fn step_7_the_window_is_bounded() {
    let (kit, _d) = kit_with(Opts {
        max_ttl: 300,
        ..Opts::default()
    })
    .await;
    let controller = listed(&kit);
    // Requested 3600, capped by max_attestation_ttl_seconds (the token
    // outlives both).
    let mut c = kit.claims(SUBJECT);
    c["exp"] = json!(kit.now() + 900);
    let req = Kit::request(
        &ed_key(),
        &controller,
        kit.oidc_with(SUBJECT, &c),
        Some(3600),
    );
    let r = kit.post(&req).await;
    assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
    let att = KeyAttestation::from_metadata(&r.1["body"]["metadata"]).unwrap();
    assert_eq!((att.not_after - att.not_before).num_seconds(), 300);
    // Capped by the token's exp.
    let mut c = kit.claims(SUBJECT);
    c["exp"] = json!(kit.now() + 90);
    let req = Kit::request(
        &ed_key(),
        &controller,
        kit.oidc_with(SUBJECT, &c),
        Some(3600),
    );
    let r = kit.post(&req).await;
    assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
    let att = KeyAttestation::from_metadata(&r.1["body"]["metadata"]).unwrap();
    assert_eq!(att.not_after.timestamp(), kit.now() + 90);
}

/// §5.2's order, by requests that fail two steps at once: the earlier one
/// answers, and nothing the later one needs is fetched.
#[tokio::test]
async fn the_checks_run_in_order() {
    let (kit, _d) = kit().await;
    let controller = listed(&kit);
    let unlisted = kit.controller("u", &[]);

    // schema before the token: a ttl of 0 with a forged token.
    let mut v = bad_pop(&controller, kit.oidc(SUBJECT));
    v["requested_ttl_seconds"] = json!(0);
    v["identity"]["token"] = json!("x.y.z");
    assert_eq!(wire(&kit.post(&v).await), is(400, "schema_violation", None));

    // issuer before pop.
    let d =
        acdp::types::keyless::IdentityDescriptor::oidc("https://evil.example", SUBJECT, "a.b.c");
    let v = bad_pop(&controller, d);
    assert_eq!(
        wire(&kit.post(&v).await).2.as_deref(),
        Some("issuer_not_accepted")
    );

    // token before pop.
    let mut c = kit.claims(SUBJECT);
    c["aud"] = json!("did:web:other.test");
    let v = bad_pop(&controller, kit.oidc_with(SUBJECT, &c));
    assert_eq!(
        wire(&kit.post(&v).await).2.as_deref(),
        Some("audience_mismatch")
    );

    // replay before pop.
    let d = kit.oidc(SUBJECT);
    assert_eq!(
        kit.post(&Kit::request(&ed_key(), &controller, d.clone(), None))
            .await
            .0,
        StatusCode::CREATED
    );
    let v = bad_pop(&controller, d);
    assert_eq!(
        wire(&kit.post(&v).await).2.as_deref(),
        Some("token_replayed")
    );

    // pop before the DID: the unlisted controller is never fetched.
    let v = bad_pop(&unlisted, kit.oidc(SUBJECT));
    assert_eq!(wire(&kit.post(&v).await).2.as_deref(), Some("pop_invalid"));
    // algorithm before the DID.
    let mut v = body(&kit, &unlisted);
    v["algorithm"] = json!("ecdsa-p256");
    assert_eq!(wire(&kit.post(&v).await).1, "unsupported_algorithm");
    assert_eq!(kit.did_hits(&unlisted), 0, "the DID is step 6");

    // the DID before the window: an unlisted identity with a token that
    // expires inside the clock skew is identity_not_authorized.
    let mut c = kit.claims(SUBJECT);
    c["exp"] = json!(kit.now() + 1);
    let req = Kit::request(&ed_key(), &unlisted, kit.oidc_with(SUBJECT, &c), None);
    assert_eq!(
        wire(&kit.post(&req).await).2.as_deref(),
        Some("identity_not_authorized")
    );
}

// ── SPIFFE, tenancy, rate limits ───────────────────────────────────────────

#[tokio::test]
async fn spiffe_jwt_svid_with_a_p256_key_end_to_end() {
    let (kit, _d) = kit().await;
    let spiffe = WorkloadIdentity::Spiffe {
        subject: SPIFFE_SUB.into(),
    };
    let controller = kit.controller("s", &[(spiffe.clone(), false)]);
    let key = p256_key();
    let req = Kit::request(&key, &controller, kit.svid(), None);
    let r = kit.post(&req).await;
    assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
    let att = KeyAttestation::from_metadata(&r.1["body"]["metadata"]).unwrap();
    assert_eq!(att.identity, spiffe);
    assert_eq!(att.algorithm, "ecdsa-p256");
    assert_eq!(kit.outside.hits("bundle"), 1);
}

/// Attestations are untenanted (plan D18): a tenant header neither scopes
/// the mint nor hides it.
#[tokio::test]
async fn a_tenant_header_does_not_scope_the_attestation() {
    let (kit, _d) = kit().await;
    let controller = listed(&kit);
    let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
    let r = kit
        .post_raw(
            serde_json::to_vec(&req).unwrap(),
            "application/json",
            None,
            &[("x-tenant-id", "tenant-a")],
        )
        .await;
    assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
    let ctx_id = r.1["body"]["ctx_id"].as_str().unwrap();
    assert_eq!(
        kit.get(&format!("/contexts/{}", urlencode(ctx_id))).await.0,
        StatusCode::OK
    );
    let (_, list) = kit
        .get(&format!(
            "/key-attestations?controller={}",
            urlencode(&controller)
        ))
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_identity_budget_is_charged_only_after_the_token_verifies() {
    let (kit, _d) = kit_with(Opts {
        rate_per_identity: 2,
        ..Opts::default()
    })
    .await;
    let other = "repo:acme/other:ref:refs/heads/main";
    let controller = kit.controller(
        "r",
        &[
            (kit.oidc_identity(SUBJECT), false),
            (kit.oidc_identity(other), false),
        ],
    );
    // Forged tokens naming the identity spend nothing.
    for _ in 0..3 {
        let d = acdp::types::keyless::IdentityDescriptor::oidc(&kit.issuer, SUBJECT, "a.b.c");
        let req = Kit::request(&ed_key(), &controller, d, None);
        assert_eq!(
            wire(&kit.post(&req).await).2.as_deref(),
            Some("token_invalid")
        );
    }
    for _ in 0..2 {
        let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
        assert_eq!(kit.post(&req).await.0, StatusCode::CREATED);
    }
    let req = Kit::request(&ed_key(), &controller, kit.oidc(SUBJECT), None);
    let r = kit.post(&req).await;
    assert_eq!(r.0, StatusCode::TOO_MANY_REQUESTS, "{}", r.1);
    // Another identity has its own budget.
    let req = Kit::request(&ed_key(), &controller, kit.oidc(other), None);
    assert_eq!(kit.post(&req).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn the_ip_budget_is_charged_on_arrival() {
    let (kit, _d) = kit_with(Opts {
        rate_per_ip: 2,
        ..Opts::default()
    })
    .await;
    let a: SocketAddr = "192.0.2.1:4000".parse().unwrap();
    let b: SocketAddr = "192.0.2.2:4000".parse().unwrap();
    // Even unparseable requests spend it.
    for _ in 0..2 {
        let r = kit
            .post_raw(b"{".to_vec(), "application/json", Some(a), &[])
            .await;
        assert_eq!(wire(&r).1, "schema_violation");
    }
    let r = kit
        .post_raw(b"{".to_vec(), "application/json", Some(a), &[])
        .await;
    assert_eq!(r.0, StatusCode::TOO_MANY_REQUESTS, "{}", r.1);
    let r = kit
        .post_raw(b"{".to_vec(), "application/json", Some(b), &[])
        .await;
    assert_eq!(
        wire(&r).1,
        "schema_violation",
        "another IP is not throttled"
    );
}

// ── GET /key-attestations ──────────────────────────────────────────────────

#[tokio::test]
async fn list_filters_orders_and_pages() {
    let (kit, _d) = kit().await;
    let other = "repo:acme/other:ref:refs/heads/main";
    let spiffe = WorkloadIdentity::Spiffe {
        subject: SPIFFE_SUB.into(),
    };
    let controller = kit.controller(
        "l",
        &[
            (kit.oidc_identity(SUBJECT), false),
            (kit.oidc_identity(other), false),
            (spiffe, false),
        ],
    );
    let bystander = listed(&kit);
    let mut minted = Vec::new();
    for d in [
        kit.oidc(SUBJECT),
        kit.oidc(other),
        kit.svid(),
        kit.oidc(SUBJECT),
    ] {
        let r = kit
            .post(&Kit::request(&ed_key(), &controller, d, None))
            .await;
        assert_eq!(r.0, StatusCode::CREATED, "{}", r.1);
        minted.push(r.1["body"]["ctx_id"].as_str().unwrap().to_string());
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let r = kit
        .post(&Kit::request(
            &ed_key(),
            &bystander,
            kit.oidc(SUBJECT),
            None,
        ))
        .await;
    assert_eq!(r.0, StatusCode::CREATED);

    let q = |extra: &str| {
        format!(
            "/key-attestations?controller={}{extra}",
            urlencode(&controller)
        )
    };
    let ids = |v: &Value| -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["ctx_id"].as_str().unwrap().to_string())
            .collect()
    };
    let newest_first: Vec<String> = minted.iter().rev().cloned().collect();

    let (s, all) = kit.get(&q("")).await;
    assert_eq!(s, StatusCode::OK, "{all}");
    assert_eq!(
        ids(&all),
        newest_first,
        "newest first, this controller only"
    );
    assert!(all["next_cursor"].is_null());
    for item in all["items"].as_array().unwrap() {
        let att = KeyAttestation::from_metadata(&item["metadata"]).unwrap();
        assert_eq!(att.controller, controller);
    }

    // Filters.
    let (_, by_sub) = kit
        .get(&q(&format!("&subject={}", urlencode(SUBJECT))))
        .await;
    assert_eq!(ids(&by_sub), vec![minted[3].clone(), minted[0].clone()]);
    let (_, by_iss) = kit
        .get(&q(&format!("&issuer={}", urlencode(&kit.issuer))))
        .await;
    assert_eq!(
        ids(&by_iss),
        vec![minted[3].clone(), minted[1].clone(), minted[0].clone()]
    );
    let (_, spiffe_only) = kit
        .get(&q(&format!("&subject={}", urlencode(SPIFFE_SUB))))
        .await;
    assert_eq!(ids(&spiffe_only), vec![minted[2].clone()]);

    // Paging.
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let extra = match &cursor {
            Some(c) => format!("&limit=1&cursor={}", urlencode(c)),
            None => "&limit=1".into(),
        };
        let (s, page) = kit.get(&q(&extra)).await;
        assert_eq!(s, StatusCode::OK, "{page}");
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        seen.extend(ids(&page));
        match page["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
        assert!(seen.len() <= minted.len(), "paging does not terminate");
    }
    assert_eq!(seen, newest_first);

    // Refusals and the empty page.
    let (s, v) = kit.get(&q("&cursor=not-a-cursor")).await;
    assert_eq!(
        (s.as_u16(), v["error"]["code"].as_str()),
        (400, Some("invalid_cursor")),
        "{v}"
    );
    let (s, v) = kit.get("/key-attestations").await;
    assert_eq!(
        (s.as_u16(), v["error"]["code"].as_str()),
        (400, Some("schema_violation")),
        "{v}"
    );
    let (s, v) = kit.get("/key-attestations?controller=did:key:z6Mk").await;
    assert_eq!(
        (s.as_u16(), v["error"]["code"].as_str()),
        (400, Some("schema_violation")),
        "{v}"
    );
    let (s, v) = kit.get(&q("&limit=x")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, v) = kit
        .get(&format!(
            "/key-attestations?controller={}",
            urlencode(&format!("did:web:agents.test:{}", unique("none")))
        ))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["items"], json!([]));
}

// ── keyless off ────────────────────────────────────────────────────────────

/// Without `keyless.enabled` both methods are 501 before anything about the
/// request is read: no body or query refusal, no fetch.
#[tokio::test]
async fn disabled_answers_501_before_reading_the_request() {
    let (kit, _d) = kit_with(Opts {
        enabled: false,
        ..Opts::default()
    })
    .await;
    let controller = listed(&kit);
    let req = Kit::request(&ed_key_from(9), &controller, kit.oidc(SUBJECT), None);
    for r in [
        kit.post(&req).await,
        kit.post_raw(b"{".to_vec(), "text/plain", None, &[]).await,
        kit.get("/key-attestations").await,
        kit.get("/key-attestations?limit=x&cursor=y").await,
    ] {
        assert_eq!(wire(&r), is(501, "not_implemented", None), "{}", r.1);
    }
    assert_eq!(kit.outside.hits("jwks"), 0);
    assert_eq!(kit.did_hits(&controller), 0);
}

// ── keyless publish, §7 and R13 (Phase 6) ──────────────────────────────────

/// kl-003 A-H, kl-004 A-F, kl-009 B and kl-007 B over HTTP on SQLite, with
/// the transparency log (the leaf half of kl-003 A).
#[tokio::test(flavor = "multi_thread")]
async fn keyless_publish_scenarios_on_sqlite() {
    let (kit, _d) = kit_with(Opts {
        log: true,
        ..Opts::default()
    })
    .await;
    keyless_kit::assert_keyless_publish_core(&kit, "sqlite").await;
}

/// §12 / D13: with the profile on, a keyless body never reaches the
/// unpinned playground branch, which verifies nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_unpinned_playground_never_accepts_a_keyless_body() {
    let (kit, _d) = kit_with(Opts {
        playground: true,
        ..Opts::default()
    })
    .await;
    let controller = listed(&kit);
    let m = kit.mint(80, &controller, kit.oidc(SUBJECT), None).await;
    let bad = keyless_kit::plain(
        &m.producer_with(&m.att, keyless_kit::dangling(keyless_kit::AUTHORITY)),
        "playground",
    );
    assert_eq!(
        wire(&kit.publish(&bad).await),
        is(400, "key_resolution_failed", Some("attestation_not_found"))
    );
    let mut forged = keyless_kit::plain(&m.producer(), "playground forged");
    forged.signature.value = keyless_kit::plain(&m.producer(), "x").signature.value;
    assert_eq!(
        wire(&kit.publish(&forged).await),
        is(400, "invalid_signature", None)
    );
    let ok = kit
        .publish(&keyless_kit::plain(&m.producer(), "playground ok"))
        .await;
    assert_eq!(ok.0, StatusCode::CREATED, "{}", ok.1);
    assert_eq!(
        ok.1["registry_receipt"]["key_fingerprint"],
        json!(m.att.key_fingerprint)
    );
}

/// kl-003 J, kl-004 F and kl-007 B with the profile off: the SDK refuses at
/// key resolution, with no `details`, exactly as before this profile.
#[tokio::test(flavor = "multi_thread")]
async fn profile_off_refuses_keyless_bodies_and_events_without_a_reason() {
    let (kit, _d) = kit_with(Opts {
        enabled: false,
        ..Opts::default()
    })
    .await;
    let controller = kit.controller_with_keys(
        "off",
        &[(kit.oidc_identity(SUBJECT), false)],
        &[("key-1", 81)],
    );
    let m = keyless_kit::Minted::unminted(82, &controller);
    let no_details = |r: &(StatusCode, Value), what: &str| {
        assert_eq!(
            wire(r),
            is(400, "key_resolution_failed", None),
            "{what}: {}",
            r.1
        );
        assert!(r.1["error"].get("details").is_none(), "{what}: {}", r.1);
    };
    // kl-003 J.
    no_details(
        &kit.publish(&keyless_kit::plain(&m.producer(), "off J"))
            .await,
        "kl-003 J",
    );
    // kl-004 F: a keyless-signed event on an existing context.
    let ok = kit
        .publish(&keyless_kit::plain(
            &keyless_kit::controller_producer(&controller, 81),
            "off target",
        ))
        .await;
    assert_eq!(ok.0, StatusCode::CREATED, "{}", ok.1);
    let target = ok.1["ctx_id"].as_str().unwrap();
    no_details(
        &kit.post_event(
            target,
            "retract",
            &keyless_kit::event_under(target, &controller, &m.att.key_id, "retracted"),
        )
        .await,
        "kl-004 F",
    );
    // kl-007 B.
    let k = ed_key_from(83);
    let fragment = k
        .key_id(&controller)
        .unwrap()
        .rsplit('#')
        .next()
        .unwrap()
        .to_string();
    let squatter = kit.controller_with_keys("squat-off", &[], &[(&fragment, 83)]);
    let mut sq = keyless_kit::plain(&keyless_kit::controller_producer(&squatter, 83), "off B");
    sq.signature.key_id = format!("{squatter}#{fragment}");
    let r = kit.publish(&sq).await;
    assert_eq!(
        (r.0.as_u16(), r.1["error"]["code"].as_str()),
        (400, Some("key_resolution_failed")),
        "kl-007 B: {}",
        r.1
    );
}

/// kl-007 C: a registry advertising `acdp_version` < 0.5.0 refuses
/// `key_attestation` with `schema_violation` naming §6.2, profile on or
/// off, on the did:web and the did:key branches. At 0.5.0 with the profile
/// off the same body is still kl-003 J's `key_resolution_failed`.
#[tokio::test(flavor = "multi_thread")]
async fn a_pre_050_registry_refuses_key_attestation_as_a_schema_violation() {
    for enabled in [false, true] {
        let (kit, _d) = kit_with(Opts {
            enabled,
            acdp_version: "0.4.0",
            ..Opts::default()
        })
        .await;
        let controller = listed(&kit);
        let m = keyless_kit::Minted::unminted(84, &controller);
        let did_web = keyless_kit::plain(&m.producer(), "kl-007 C");
        let mut did_key = keyless_kit::plain(
            &acdp::producer::Producer::new_did_key(SigningKey::from_bytes(&[85; 32])),
            "kl-007 C did:key",
        );
        did_key.key_attestation = Some(m.reference.clone());
        for (what, req) in [("did:web", &did_web), ("did:key", &did_key)] {
            let r = kit.publish(req).await;
            assert_eq!(
                wire(&r),
                is(400, "schema_violation", None),
                "{what} enabled={enabled}: {}",
                r.1
            );
            assert!(
                r.1["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("RFC-ACDP-0021 §6.2"),
                "{}",
                r.1
            );
        }
        if !enabled {
            let (kit5, _d5) = kit_with(Opts {
                enabled: false,
                ..Opts::default()
            })
            .await;
            let r = kit5.publish(&did_web).await;
            assert_eq!(wire(&r), is(400, "key_resolution_failed", None), "{}", r.1);
        }
    }
}
