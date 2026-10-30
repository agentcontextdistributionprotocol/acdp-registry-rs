//! An in-process `acdp-registry-keyless` registry over any store, for the
//! `POST/GET /key-attestations` tests (RFC-ACDP-0021 Phase 5).
//!
//! One loopback HTTPS server, presenting the `didweb` fixture's
//! `agents.test` leaf, plays both outside parties:
//!
//! - the **issuer**: `/jwks` (OIDC, EdDSA) and `/bundle` (SPIFFE, ES256,
//!   `use: "jwt-svid"`), reached through the Phase 4 test outbound (the
//!   fixture CA, `agents.test` resolved to loopback, loopback allowed);
//! - the **controllers**: `did:web:agents.test:<name>` documents with an
//!   `AcdpKeylessSigning` service, reached through
//!   `WebResolver::with_test_endpoint`.
//!
//! It counts hits per path, so a test can prove a fetch did or did not
//! happen. The keyless clock is injectable; it starts at the wall clock.
//!
//! Included as a module by `tests/keyless_http.rs` (sqlite),
//! `tests/pg_integration.rs` (pg) and `src/memory_ext.rs` (memory), so the
//! three stores run the same scenarios.

#![allow(dead_code)]

#[path = "../didweb/mod.rs"]
pub mod didweb;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicI64, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acdp::crypto::{AcdpSigningKey, SigningKey};
use acdp::did::WebResolver;
use acdp::producer::keyless::EphemeralKey;
use acdp::registry::RegistryServer;
use acdp::safe_http::SsrfPolicy;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::keyless::{IdentityDescriptor, KeyAttestationRequest, WorkloadIdentity};
use acdp_registry_auth::{
    AuthService, ChallengeStore, InMemoryChallengeStore, JwtSecret, JwtSigner,
};
use acdp_registry_core::callback::StaticResolver;
use acdp_registry_core::keyless::{JwksCache, Outbound};
use acdp_registry_core::{build_router_shared, AppStateInner};
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::config::{OidcIssuer, SpiffeDomain};
use acdp_registry_types::{AuthConfig, RegistryConfig};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD};
use base64::Engine as _;
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use tower::ServiceExt;

pub const AUTHORITY: &str = "registry.test";
pub const REGISTRY_DID: &str = "did:web:registry.test";
pub const SUBJECT: &str = "repo:acme/app:ref:refs/heads/main";
pub const TRUST_DOMAIN: &str = "prod.example.test";
pub const SPIFFE_SUB: &str = "spiffe://prod.example.test/ns/a/sa/b";
/// The rcpt-001 registry test seed, as kl-001 uses it.
pub const RECEIPT_SEED: [u8; 32] = [0x11; 32];

// ── the outside parties ────────────────────────────────────────────────────

/// The issuer and the controllers' DID documents.
pub struct Outside {
    jwks: Mutex<Value>,
    bundle: Mutex<Value>,
    docs: Mutex<HashMap<String, Value>>,
    /// Non-200 answers every DID document with this status.
    did_status: AtomicU16,
    /// Non-200 answers `/jwks` and `/bundle` with this status.
    keys_status: AtomicU16,
    hits: Mutex<HashMap<String, usize>>,
}

impl Outside {
    fn hit(&self, path: &str) {
        *self
            .hits
            .lock()
            .unwrap()
            .entry(path.to_string())
            .or_default() += 1;
    }

    /// Requests served for `path` (`jwks`, `bundle`, or a DID's `<name>`).
    pub fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    /// Serve `doc` as `did:web:agents.test:<name>`.
    pub fn set_doc(&self, name: &str, doc: Value) {
        self.docs.lock().unwrap().insert(name.to_string(), doc);
    }

    /// Answer `/jwks` and `/bundle` with `status` (200 restores them).
    pub fn set_keys_status(&self, status: u16) {
        self.keys_status.store(status, Ordering::SeqCst);
    }

    fn keys(&self, path: &str, set: &Mutex<Value>) -> axum::response::Response {
        self.hit(path);
        let status = self.keys_status.load(Ordering::SeqCst);
        if status != 200 {
            return StatusCode::from_u16(status).unwrap().into_response();
        }
        axum::Json(set.lock().unwrap().clone()).into_response()
    }

    /// Answer every DID document with `status` (200 restores them).
    pub fn set_did_status(&self, status: u16) {
        self.did_status.store(status, Ordering::SeqCst);
    }
}

async fn spawn_outside() -> (Arc<Outside>, SocketAddr) {
    let o = Arc::new(Outside {
        jwks: Mutex::new(json!({ "keys": [] })),
        bundle: Mutex::new(json!({ "keys": [] })),
        docs: Mutex::new(HashMap::new()),
        did_status: AtomicU16::new(200),
        keys_status: AtomicU16::new(200),
        hits: Mutex::new(HashMap::new()),
    });
    let (a, b, c) = (o.clone(), o.clone(), o.clone());
    let app = Router::new()
        .route("/jwks", get(move || async move { a.keys("jwks", &a.jwks) }))
        .route(
            "/bundle",
            get(move || async move { b.keys("bundle", &b.bundle) }),
        )
        .route(
            "/{name}/did.json",
            get(
                move |axum::extract::Path(name): axum::extract::Path<String>| async move {
                    c.hit(&name);
                    let status = c.did_status.load(Ordering::SeqCst);
                    if status != 200 {
                        return StatusCode::from_u16(status).unwrap().into_response();
                    }
                    match c.docs.lock().unwrap().get(&name).cloned() {
                        Some(doc) => axum::Json(doc).into_response(),
                        None => StatusCode::NOT_FOUND.into_response(),
                    }
                },
            ),
        );
    let addr = didweb::spawn_tls_server(app).await;
    (o, addr)
}

// ── token signers ──────────────────────────────────────────────────────────

pub struct Signer {
    pub alg: Algorithm,
    pub kid: String,
    enc: EncodingKey,
    pub jwk: Value,
}

/// Ed25519 (EdDSA) from a seed: PKCS#8 v1 DER is a fixed prefix + seed.
pub fn ed_signer(seed: u8, kid: &str) -> Signer {
    let mut der = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    der.extend_from_slice(&[seed; 32]);
    let public = SigningKey::from_bytes(&[seed; 32]).verifying_key_bytes();
    Signer {
        alg: Algorithm::EdDSA,
        kid: kid.into(),
        enc: EncodingKey::from_ed_der(&der),
        jwk: json!({"kty": "OKP", "crv": "Ed25519", "x": URL_SAFE_NO_PAD.encode(public), "kid": kid}),
    }
}

/// A fresh P-256 key (ES256), published with `use: "jwt-svid"`.
pub fn es_signer(kid: &str) -> Signer {
    let kp = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let raw = kp.public_key_raw();
    Signer {
        alg: Algorithm::ES256,
        kid: kid.into(),
        enc: EncodingKey::from_ec_der(&kp.serialize_der()),
        jwk: json!({
            "kty": "EC", "crv": "P-256", "kid": kid, "use": "jwt-svid",
            "x": URL_SAFE_NO_PAD.encode(&raw[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&raw[33..65]),
        }),
    }
}

impl Signer {
    pub fn sign(&self, claims: &Value) -> String {
        let mut h = Header::new(self.alg);
        h.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&h, claims, &self.enc).unwrap()
    }
}

static UNIQUE: AtomicU32 = AtomicU32::new(0);

/// A name no other call in this process shares.
pub fn unique(label: &str) -> String {
    format!(
        "{label}-{}-{}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    )
}

// ── the registry ───────────────────────────────────────────────────────────

/// Harness knobs.
#[derive(Debug, Clone)]
pub struct Opts {
    /// The store appends log leaves (the caller built it so).
    pub log: bool,
    pub rate_per_identity: u32,
    pub rate_per_ip: u32,
    pub max_ttl: u32,
    /// The DID resolver's forced-refresh interval.
    pub forced_refresh: Duration,
    /// `keyless.enabled`; false gives the feature-off 501s.
    pub enabled: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            log: false,
            rate_per_identity: 1000,
            rate_per_ip: 1000,
            max_ttl: 3600,
            forced_refresh: Duration::from_secs(3600),
            enabled: true,
        }
    }
}

pub struct Kit {
    pub app: Router,
    pub outside: Arc<Outside>,
    /// `https://agents.test:<port>`, the OIDC issuer.
    pub issuer: String,
    pub oidc: Signer,
    pub spiffe: Signer,
    /// The keyless clock, Unix ms.
    pub clock: Arc<AtomicI64>,
}

fn caps(log: bool) -> CapabilitiesDocument {
    let mut profiles = vec!["acdp-registry-core".to_string()];
    if log {
        profiles.push("acdp-registry-transparency-log".into());
    }
    CapabilitiesDocument {
        acdp_version: "0.5.0".into(),
        registry_did: REGISTRY_DID.into(),
        supported_signature_algorithms: vec!["ed25519".into(), "ecdsa-p256".into()],
        supported_did_methods: vec!["did:web".into(), "did:key".into()],
        profiles,
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
            change_retention_seconds: None,
            max_change_filters: None,
            max_changes_page: None,
            max_subscriptions_per_requester: None,
        },
        read_authentication_methods: vec![],
        anonymous_public_reads: true,
        supports_idempotency_key: true,
        extensions: Default::default(),
    }
}

impl Kit {
    /// A keyless registry over `store` (migrated, and built with the
    /// transparency log when `opts.log`).
    pub async fn new<S: ExtendedRegistryStore + 'static>(store: S, opts: Opts) -> Self {
        let (outside, addr) = spawn_outside().await;
        let issuer = format!("https://{}:{}", didweb::DIDWEB_AUTHORITY, addr.port());
        let (oidc, spiffe) = (ed_signer(7, "oidc-1"), es_signer("svid-1"));
        *outside.jwks.lock().unwrap() = json!({ "keys": [oidc.jwk.clone()] });
        *outside.bundle.lock().unwrap() = json!({ "keys": [spiffe.jwk.clone()] });

        let mut cfg = RegistryConfig::defaults();
        cfg.registry.authority = AUTHORITY.into();
        cfg.auth.anonymous_public_reads = true;
        cfg.auth.did_methods = vec!["did:web".into(), "did:key".into()];
        cfg.receipt.signing_key_seed_b64 = B64.encode(RECEIPT_SEED);
        cfg.log.enabled = opts.log;
        cfg.keyless.enabled = opts.enabled;
        cfg.keyless.max_attestation_ttl_seconds = opts.max_ttl;
        cfg.keyless.oidc_issuers = vec![OidcIssuer {
            issuer: issuer.clone(),
            jwks_uri: Some(format!("{issuer}/jwks")),
        }];
        cfg.keyless.spiffe_trust_domains = vec![SpiffeDomain {
            trust_domain: TRUST_DOMAIN.into(),
            bundle_endpoint: format!("{issuer}/bundle"),
            jwt_issuer: None,
        }];
        cfg.keyless.rate_per_identity_per_minute = opts.rate_per_identity;
        cfg.keyless.rate_per_ip_per_minute = opts.rate_per_ip;
        cfg.keyless.fetch_timeout_ms = 3000;

        let signer =
            acdp_registry_core::receipt::build_signer(&cfg.receipt, &cfg.registry.authority)
                .expect("receipt signer");
        let server = RegistryServer::try_new(store, caps(opts.log), AUTHORITY)
            .unwrap()
            .with_receipt_signer(signer)
            .expect("receipt signer");
        let server = Arc::new(server);

        let resolver = Arc::new(
            WebResolver::with_test_endpoint(
                didweb::ca_pem().as_bytes(),
                didweb::DIDWEB_AUTHORITY,
                addr,
            )
            .unwrap()
            .with_forced_refresh_interval(opts.forced_refresh),
        );
        let challenges: Arc<dyn ChallengeStore> = Arc::new(InMemoryChallengeStore::new());
        let jwt = JwtSigner::new(
            JwtSecret::from_bytes(&[42u8; 32]),
            REGISTRY_DID.to_string(),
            AUTHORITY.into(),
            30,
        );
        let auth = Arc::new(AuthService::new(
            AuthConfig::default(),
            challenges,
            jwt,
            resolver,
            AUTHORITY.into(),
        ));

        let clock = Arc::new(AtomicI64::new(chrono::Utc::now().timestamp_millis()));
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        let outbound = Outbound {
            policy: SsrfPolicy {
                allow_loopback_resolved: true,
                ..SsrfPolicy::default()
            },
            resolver: Arc::new(StaticResolver::new().with(didweb::DIDWEB_AUTHORITY, &[loopback])),
            extra_roots: Vec::new(),
        }
        .with_root_pem(didweb::ca_pem().as_bytes())
        .unwrap();
        let (c1, c2) = (clock.clone(), clock.clone());
        let cache = JwksCache::new(
            &cfg.keyless,
            outbound,
            Arc::new(move || c1.load(Ordering::SeqCst)),
        );
        let state = AppStateInner::new(server, auth, None, cfg, None)
            .with_keyless_jwks(cache)
            .with_keyless_clock(Arc::new(move || c2.load(Ordering::SeqCst)));
        assert_eq!(state.keyless.is_some(), opts.enabled);
        Self {
            app: build_router_shared(Arc::new(state)),
            outside,
            issuer,
            oidc,
            spiffe,
            clock,
        }
    }

    /// The keyless clock, Unix seconds.
    pub fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst) / 1000
    }

    /// A fresh controller `did:web:agents.test:<name>` listing `listed`
    /// (each `(identity, org_wide)`).
    pub fn controller(&self, label: &str, listed: &[(WorkloadIdentity, bool)]) -> String {
        let name = unique(label);
        let did = format!("did:web:{}:{name}", didweb::DIDWEB_AUTHORITY);
        self.list(&did, listed);
        did
    }

    /// Replace `did`'s listed identities.
    pub fn list(&self, did: &str, listed: &[(WorkloadIdentity, bool)]) {
        let name = did.rsplit(':').next().unwrap();
        let identities: Vec<Value> = listed
            .iter()
            .map(|(id, org_wide)| {
                let mut v = serde_json::to_value(id).unwrap();
                v["org_wide"] = json!(org_wide);
                v
            })
            .collect();
        self.outside.set_doc(
            name,
            json!({
                "@context": ["https://www.w3.org/ns/did/v1"],
                "id": did,
                "service": [{
                    "id": format!("{did}#keyless"),
                    "type": "AcdpKeylessSigning",
                    "serviceEndpoint": { "identities": identities },
                }],
            }),
        );
    }

    /// DID-document fetches served for `did`.
    pub fn did_hits(&self, did: &str) -> usize {
        self.outside.hits(did.rsplit(':').next().unwrap())
    }

    /// The OIDC identity `(issuer, subject)`.
    pub fn oidc_identity(&self, subject: &str) -> WorkloadIdentity {
        WorkloadIdentity::Oidc {
            issuer: self.issuer.clone(),
            subject: subject.into(),
        }
    }

    /// OIDC claims every check accepts, unique per call (`jti`).
    pub fn claims(&self, subject: &str) -> Value {
        let now = self.now();
        json!({
            "iss": self.issuer, "sub": subject, "aud": REGISTRY_DID,
            "exp": now + 300, "iat": now, "jti": unique("jti"),
        })
    }

    /// A signed OIDC descriptor for `subject` with `claims`.
    pub fn oidc_with(&self, subject: &str, claims: &Value) -> IdentityDescriptor {
        IdentityDescriptor::oidc(&self.issuer, subject, self.oidc.sign(claims))
    }

    /// A valid OIDC descriptor for `subject`.
    pub fn oidc(&self, subject: &str) -> IdentityDescriptor {
        self.oidc_with(subject, &self.claims(subject))
    }

    /// A valid JWT-SVID descriptor for [`SPIFFE_SUB`].
    pub fn svid(&self) -> IdentityDescriptor {
        let now = self.now();
        let token = self.spiffe.sign(&json!({
            "sub": SPIFFE_SUB, "aud": [REGISTRY_DID], "exp": now + 300, "iat": now,
            "jti": unique("jti"),
        }));
        IdentityDescriptor::spiffe_jwt(SPIFFE_SUB, token)
    }

    /// The SDK-built request body for `controller`.
    pub fn request(
        key: &EphemeralKey,
        controller: &str,
        identity: IdentityDescriptor,
        ttl: Option<u32>,
    ) -> KeyAttestationRequest {
        key.attestation_request(controller, REGISTRY_DID, identity, ttl)
            .expect("valid request")
    }

    /// `POST /key-attestations` with `body` from `peer` (default: none).
    pub async fn post_raw(
        &self,
        body: Vec<u8>,
        content_type: &str,
        peer: Option<SocketAddr>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let (s, _, v) = self.post_full(body, content_type, peer, headers).await;
        (s, v)
    }

    /// [`Self::post_raw`], with the response headers.
    pub async fn post_full(
        &self,
        body: Vec<u8>,
        content_type: &str,
        peer: Option<SocketAddr>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut b = Request::builder()
            .method("POST")
            .uri("/key-attestations")
            .header("content-type", content_type);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let mut req = b.body(Body::from(body)).unwrap();
        if let Some(p) = peer {
            req.extensions_mut().insert(ConnectInfo(p));
        }
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// `POST /key-attestations` with a JSON body.
    pub async fn post(&self, body: &impl serde::Serialize) -> (StatusCode, Value) {
        self.post_raw(
            serde_json::to_vec(body).unwrap(),
            "application/json",
            None,
            &[],
        )
        .await
    }

    pub async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The transparency log's size.
    pub async fn log_size(&self) -> u64 {
        let (s, v) = self.get("/log/checkpoint").await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["tree_size"]
            .as_u64()
            .or_else(|| v["checkpoint"]["tree_size"].as_u64())
            .unwrap_or_else(|| panic!("no tree_size in {v}"))
    }
}

/// `(status, error.code, details.reason)` of a response.
pub fn wire(r: &(StatusCode, Value)) -> (u16, String, Option<String>) {
    (
        r.0.as_u16(),
        r.1["error"]["code"].as_str().unwrap_or("").to_string(),
        r.1["error"]["details"]["reason"]
            .as_str()
            .map(str::to_owned),
    )
}

/// The expected `(status, code, reason)`.
pub fn is(status: u16, code: &str, reason: Option<&str>) -> (u16, String, Option<String>) {
    (status, code.into(), reason.map(str::to_owned))
}

/// A fresh Ed25519 ephemeral key.
pub fn ed_key() -> EphemeralKey {
    EphemeralKey::generate()
}

/// A fresh P-256 ephemeral key.
pub fn p256_key() -> EphemeralKey {
    EphemeralKey::generate_p256()
}

/// The ephemeral key for a seed (deterministic).
pub fn ed_key_from(seed: u8) -> EphemeralKey {
    EphemeralKey::from_key(AcdpSigningKey::Ed25519(SigningKey::from_bytes(&[seed; 32])))
}

/// The A/H/E core every store must answer the same (plan Phase 5:
/// "a three-store run of the A/H/E core"):
///
/// - A: a valid request is 201 with the full retrieval representation,
///   its receipt, the index (GET lists it) and, with the log, one leaf;
/// - H: the same token again is `token_replayed`, and so is a concurrent
///   pair: exactly one of two simultaneous mints with one token wins;
/// - E: an identity the controller does not list is
///   `identity_not_authorized`, after exactly one uncached re-fetch.
pub async fn assert_mint_core(kit: &Kit, label: &str) {
    let id = kit.oidc_identity(SUBJECT);
    let controller = kit.controller("core", &[(id.clone(), false)]);

    // A.
    let before = if kit_has_log(kit).await {
        Some(kit.log_size().await)
    } else {
        None
    };
    let key = ed_key();
    let descriptor = kit.oidc(SUBJECT);
    let req = Kit::request(&key, &controller, descriptor.clone(), Some(120));
    let created = kit.post(&req).await;
    assert_eq!(created.0, StatusCode::CREATED, "{label} A: {}", created.1);
    let body = &created.1["body"];
    assert_eq!(body["type"], "acdp:key-attestation", "{label} A");
    assert_eq!(body["agent_id"], REGISTRY_DID, "{label} A");
    assert!(
        created.1["registry_receipt"].is_object(),
        "{label} A: receipt"
    );
    let att = acdp::types::keyless::KeyAttestation::from_metadata(&body["metadata"])
        .expect("a §5.3 attestation");
    assert_eq!(att.controller, controller);
    assert_eq!(att.identity, id);
    assert_eq!(att.key_fingerprint, key.fingerprint().unwrap());
    assert_eq!((att.not_after - att.not_before).num_seconds(), 120);
    if let Some(before) = before {
        assert_eq!(kit.log_size().await, before + 1, "{label} A: one log leaf");
    }
    let ctx_id = body["ctx_id"].as_str().unwrap().to_string();
    let listed = kit
        .get(&format!(
            "/key-attestations?controller={}",
            urlencode(&controller)
        ))
        .await;
    assert_eq!(listed.0, StatusCode::OK, "{label} A list: {}", listed.1);
    assert_eq!(listed.1["items"][0]["ctx_id"], ctx_id.as_str(), "{label}");
    assert_eq!(
        listed.1["items"][0]["content_hash"], body["content_hash"],
        "{label}"
    );

    // H: sequential replay.
    let again = Kit::request(&ed_key(), &controller, descriptor, None);
    assert_eq!(
        wire(&kit.post(&again).await),
        is(403, "key_not_authorized", Some("token_replayed")),
        "{label} H"
    );

    // H: concurrent pair with one fresh token.
    let shared = kit.oidc(SUBJECT);
    let (r1, r2) = (
        Kit::request(&ed_key(), &controller, shared.clone(), None),
        Kit::request(&ed_key(), &controller, shared, None),
    );
    let (a, b) = tokio::join!(kit.post(&r1), kit.post(&r2));
    let mut statuses = [a.0.as_u16(), b.0.as_u16()];
    statuses.sort();
    assert_eq!(
        statuses,
        [201, 403],
        "{label} concurrent: {} / {}",
        a.1,
        b.1
    );
    let loser = if a.0 == StatusCode::CREATED { &b } else { &a };
    assert_eq!(
        wire(loser),
        is(403, "key_not_authorized", Some("token_replayed")),
        "{label} concurrent loser"
    );

    // E: the cached document does not list this subject; exactly one
    // uncached re-fetch, then the refusal.
    let fetches = kit.did_hits(&controller);
    let other = "repo:acme/other:ref:refs/heads/main";
    let unlisted = Kit::request(&ed_key(), &controller, kit.oidc(other), None);
    assert_eq!(
        wire(&kit.post(&unlisted).await),
        is(403, "key_not_authorized", Some("identity_not_authorized")),
        "{label} E"
    );
    assert_eq!(
        kit.did_hits(&controller),
        fetches + 1,
        "{label} E: exactly one uncached re-fetch"
    );
}

async fn kit_has_log(kit: &Kit) -> bool {
    kit.get("/log/checkpoint").await.0 == StatusCode::OK
}

/// Percent-encode a query value (RFC 3986 unreserved kept).
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b))
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
