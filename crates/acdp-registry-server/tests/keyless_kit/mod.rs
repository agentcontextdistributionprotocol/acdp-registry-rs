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

use acdp::crypto::{AcdpSigningKey, P256SigningKey, SigningKey};
use acdp::did::WebResolver;
use acdp::producer::keyless::EphemeralKey;
use acdp::producer::Producer;
use acdp::registry::RegistryServer;
use acdp::safe_http::SsrfPolicy;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::keyless::{
    keyless_key_id, IdentityDescriptor, KeyAttestation, KeyAttestationRef, KeyAttestationRequest,
    WorkloadIdentity,
};
use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
use acdp::types::primitives::{AgentDid, ContentHash, ContextType, CtxId, Visibility};
use acdp::types::publish::PublishRequest;
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

    /// Serve `jwks` at `/jwks`.
    pub fn set_jwks(&self, jwks: Value) {
        *self.jwks.lock().unwrap() = jwks;
    }
}

/// Serve the outside parties over HTTPS as `agents.test` (the `didweb`
/// fixture leaf) on a loopback port.
pub async fn spawn_outside() -> (Arc<Outside>, SocketAddr) {
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

/// `did`'s DID document: Ed25519 verification methods `<did>#<fragment>`
/// for `(fragment, seed)` in `keys` (each also in `assertionMethod`), and
/// an `AcdpKeylessSigning` service listing `listed` (each
/// `(identity, org_wide)`).
pub fn controller_document(
    did: &str,
    listed: &[(WorkloadIdentity, bool)],
    keys: &[(&str, u8)],
) -> Value {
    let methods: Vec<Value> = keys
        .iter()
        .map(|(fragment, seed)| {
            let key = SigningKey::from_bytes(&[*seed; 32]);
            let multibase = acdp::did::key::did_key_from_ed25519(&key.verifying_key_bytes())
                .strip_prefix("did:key:")
                .unwrap()
                .to_string();
            json!({
                "id": format!("{did}#{fragment}"),
                "type": "Ed25519VerificationKey2020",
                "controller": did,
                "publicKeyMultibase": multibase,
            })
        })
        .collect();
    let assertion: Vec<Value> = keys
        .iter()
        .map(|(fragment, _)| json!(format!("{did}#{fragment}")))
        .collect();
    let identities: Vec<Value> = listed
        .iter()
        .map(|(id, org_wide)| {
            let mut v = serde_json::to_value(id).unwrap();
            v["org_wide"] = json!(org_wide);
            v
        })
        .collect();
    json!({
        "@context": [
            "https://www.w3.org/ns/did/v1",
            "https://w3id.org/security/suites/ed25519-2020/v1"
        ],
        "id": did,
        "verificationMethod": methods,
        "assertionMethod": assertion,
        "service": [{
            "id": format!("{did}#keyless"),
            "type": "AcdpKeylessSigning",
            "serviceEndpoint": { "identities": identities },
        }],
    })
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
    /// The advertised `acdp_version` (kl-007 C uses 0.4.0).
    pub acdp_version: &'static str,
    /// `[playground] enabled`, unpinned (§12: never accepts a keyless body).
    pub playground: bool,
    /// `[metrics] enabled` (open, no bearer): mounts `GET /metrics` and
    /// installs the process-global recorder.
    pub metrics: bool,
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
            acdp_version: "0.5.0",
            playground: false,
            metrics: false,
        }
    }
}

/// The bearer `/admin/*` accepts (the kl-003 G registry withdrawal).
pub const ADMIN_TOKEN: &str = "keyless-kit-admin-token";

pub struct Kit {
    pub app: Router,
    pub outside: Arc<Outside>,
    /// `https://agents.test:<port>`, the OIDC issuer.
    pub issuer: String,
    pub oidc: Signer,
    pub spiffe: Signer,
    /// The keyless clock, Unix ms.
    pub clock: Arc<AtomicI64>,
    /// The registry's DID resolver (tests invalidate its cache).
    pub resolver: Arc<WebResolver>,
}

fn caps(log: bool, acdp_version: &str) -> CapabilitiesDocument {
    let mut profiles = vec!["acdp-registry-core".to_string()];
    if log {
        profiles.push("acdp-registry-transparency-log".into());
    }
    CapabilitiesDocument {
        acdp_version: acdp_version.into(),
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
        cfg.lifecycle.enabled = true;
        cfg.auth.admin_tokens = vec![ADMIN_TOKEN.into()];
        cfg.playground.enabled = opts.playground;
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
        cfg.metrics.enabled = opts.metrics;

        let signer =
            acdp_registry_core::receipt::build_signer(&cfg.receipt, &cfg.registry.authority)
                .expect("receipt signer");
        let server = RegistryServer::try_new(store, caps(opts.log, opts.acdp_version), AUTHORITY)
            .unwrap()
            .with_receipt_signer(signer)
            .expect("receipt signer")
            .with_lifecycle()
            .expect("lifecycle");
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
            resolver.clone(),
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
            resolver,
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
        self.list_with_keys(did, listed, &[]);
    }

    /// [`Self::list`], with Ed25519 verification methods
    /// `<did>#<fragment>` for `(fragment, seed)` in `keys`, each also in
    /// `assertionMethod`.
    pub fn list_with_keys(
        &self,
        did: &str,
        listed: &[(WorkloadIdentity, bool)],
        keys: &[(&str, u8)],
    ) {
        let name = did.rsplit(':').next().unwrap();
        self.outside
            .set_doc(name, controller_document(did, listed, keys));
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

// ── keyless publish, §7 and R13 (Phase 6) ──────────────────────────────────

/// The Ed25519 key for `seed`.
pub fn signing_key(seed: u8) -> AcdpSigningKey {
    AcdpSigningKey::Ed25519(SigningKey::from_bytes(&[seed; 32]))
}

/// A minted attestation and what a producer needs to sign under it.
pub struct Minted {
    pub att: KeyAttestation,
    pub reference: KeyAttestationRef,
    /// The ephemeral key's seed.
    pub seed: u8,
}

impl Minted {
    /// `seed`'s key under an attestation no registry minted, naming a
    /// `ctx_id` that holds nothing: enough for the SDK to build a
    /// keyless-signed body (the profile-off and pre-0.5.0 scenarios).
    pub fn unminted(seed: u8, controller: &str) -> Self {
        Self {
            att: unminted_attestation(seed, controller),
            reference: dangling(AUTHORITY),
            seed,
        }
    }

    /// The SDK's keyless producer for this attestation.
    pub fn producer(&self) -> Producer {
        self.producer_with(&self.att, self.reference.clone())
    }

    /// A keyless producer signing with this key, claiming `att` and
    /// naming `reference` (for the mismatch scenarios).
    pub fn producer_with(&self, att: &KeyAttestation, reference: KeyAttestationRef) -> Producer {
        Producer::keyless(signing_key(self.seed), att, reference).expect("keyless producer")
    }
}

/// A plain public request from `p`.
pub fn plain(p: &Producer, title: &str) -> PublishRequest {
    p.publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("request")
}

/// A successor of `prev` from `p`.
pub fn successor(p: &Producer, prev: &acdp::types::body::Body, title: &str) -> PublishRequest {
    p.supersede_body(prev)
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("successor")
}

/// The controller's DID-document key `<controller>#key-1` for `seed`.
pub fn controller_producer(controller: &str, seed: u8) -> Producer {
    Producer::new(
        SigningKey::from_bytes(&[seed; 32]),
        AgentDid::new(controller.to_string()),
        format!("{controller}#key-1"),
    )
}

/// An attestation shape for `seed`'s key under `controller` that no
/// registry minted: it only lets the SDK build a keyless-signed body.
pub fn unminted_attestation(seed: u8, controller: &str) -> KeyAttestation {
    let key = EphemeralKey::from_key(signing_key(seed));
    let fingerprint = key.fingerprint().unwrap();
    let now = chrono::Utc::now();
    KeyAttestation {
        attestation_version: acdp::types::keyless::ATTESTATION_VERSION.into(),
        controller: controller.into(),
        identity: WorkloadIdentity::Oidc {
            issuer: "https://issuer.example".into(),
            subject: SUBJECT.into(),
        },
        org_wide: false,
        key_id: keyless_key_id(controller, &fingerprint).unwrap(),
        algorithm: key.algorithm().into(),
        public_key_jwk: key.jwk().unwrap(),
        key_fingerprint: fingerprint,
        not_before: now,
        not_after: now + chrono::Duration::seconds(600),
        token_hash: format!("sha256:{}", "a".repeat(64)),
    }
}

/// A reference to a ctx_id on `authority` that holds nothing.
pub fn dangling(authority: &str) -> KeyAttestationRef {
    KeyAttestationRef {
        ctx_id: CtxId(format!("acdp://{authority}/{}", uuid::Uuid::new_v4())),
        content_hash: ContentHash(format!("sha256:{}", "c".repeat(64))),
    }
}

/// A retract envelope for `ctx_id` by `actor`, signed under `#key-1` and
/// then relabelled `key_id` on the wire: the SDK will not sign under a
/// keyless key id, and the refusal comes before the signature is read.
pub fn event_under(ctx_id: &str, actor: &str, key_id: &str, event_type: &str) -> Value {
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        CtxId(ctx_id.to_string()),
        LifecycleEventType::parse(event_type).unwrap(),
        chrono::Utc::now(),
        AgentDid::new(actor.to_string()),
        None,
    )
    .unwrap()
    .sign_with(SigningKey::from_bytes(&[3; 32]), format!("{actor}#key-1"))
    .unwrap();
    let mut v = json!({ "event": event });
    v["event"]["signature"]["key_id"] = json!(key_id);
    v
}

impl Kit {
    /// A controller listing `listed` whose DID document also has the
    /// Ed25519 `#<fragment>` keys for `(fragment, seed)`.
    pub fn controller_with_keys(
        &self,
        label: &str,
        listed: &[(WorkloadIdentity, bool)],
        keys: &[(&str, u8)],
    ) -> String {
        let did = self.controller(label, listed);
        self.list_with_keys(&did, listed, keys);
        did
    }

    /// Mint an attestation for `seed`'s Ed25519 key.
    pub async fn mint(
        &self,
        seed: u8,
        controller: &str,
        descriptor: IdentityDescriptor,
        ttl: Option<u32>,
    ) -> Minted {
        let key = EphemeralKey::from_key(signing_key(seed));
        let req = Self::request(&key, controller, descriptor, ttl);
        let (status, v) = self.post(&req).await;
        assert_eq!(status, StatusCode::CREATED, "mint: {v}");
        let body: acdp::types::body::Body = serde_json::from_value(v["body"].clone()).unwrap();
        let att = KeyAttestation::from_body(&body).unwrap();
        Minted {
            reference: KeyAttestationRef {
                ctx_id: body.ctx_id.clone(),
                content_hash: body.content_hash.clone(),
            },
            att,
            seed,
        }
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// `POST /contexts`, with an `Idempotency-Key` when given.
    pub async fn publish_idem(
        &self,
        req: &impl serde::Serialize,
        idempotency_key: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut b = Request::builder()
            .method("POST")
            .uri("/contexts")
            .header("content-type", "application/json");
        if let Some(k) = idempotency_key {
            b = b.header("idempotency-key", k);
        }
        self.send(
            b.body(Body::from(serde_json::to_vec(req).unwrap()))
                .unwrap(),
        )
        .await
    }

    /// `POST /contexts`.
    pub async fn publish(&self, req: &impl serde::Serialize) -> (StatusCode, Value) {
        self.publish_idem(req, None).await
    }

    /// The stored body at `ctx_id`.
    pub async fn body_of(&self, ctx_id: &str) -> acdp::types::body::Body {
        let (s, v) = self.get(&format!("/contexts/{}", urlencode(ctx_id))).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        serde_json::from_value(v["body"].clone()).unwrap()
    }

    /// `POST /contexts/{ctx_id}/<endpoint>` with `envelope`.
    pub async fn post_event(
        &self,
        ctx_id: &str,
        endpoint: &str,
        envelope: &Value,
    ) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .method("POST")
                .uri(format!("/contexts/{}/{endpoint}", urlencode(ctx_id)))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(envelope).unwrap()))
                .unwrap(),
        )
        .await
    }

    /// The registry withdraws an attestation (§9.2): an admin retract.
    pub async fn admin_retract(&self, ctx_id: &str) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/contexts/{}/retract", urlencode(ctx_id)))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// The log leaf at `index`.
    pub async fn leaf(&self, index: u64) -> Value {
        let (s, v) = self
            .get(&format!("/log/entries?start={index}&end={}", index + 1))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["entries"][0]["leaf"].clone()
    }
}

/// The kl-003 and kl-004 registry scenarios, plus kl-009 B and kl-007 B,
/// end to end over HTTP. Every store must answer them the same; with the
/// log, the leaf half of kl-003 A is checked too.
pub async fn assert_keyless_publish_core(kit: &Kit, label: &str) {
    let has_log = kit_has_log(kit).await;
    let same = kit.oidc_identity(SUBJECT);
    let other_subject = "repo:acme/other:ref:refs/heads/main";
    let org_subject = "repo:acme/org:ref:refs/heads/main";
    let controller = kit.controller_with_keys(
        "pub",
        &[
            (same.clone(), false),
            (kit.oidc_identity(other_subject), false),
            (kit.oidc_identity(org_subject), true),
        ],
        &[("key-1", 40)],
    );

    // kl-003 A: accepted; the receipt and the leaf carry the attested
    // fingerprint. An idempotent replay answers 200 from the stored reply.
    let m = kit
        .mint(61, &controller, kit.oidc(SUBJECT), Some(600))
        .await;
    let p = m.producer();
    let req = plain(&p, "kl-003 A");
    let before = if has_log {
        Some(kit.log_size().await)
    } else {
        None
    };
    let idem = unique("kl-a");
    let a = kit.publish_idem(&req, Some(&idem)).await;
    assert_eq!(a.0, StatusCode::CREATED, "{label} A: {}", a.1);
    assert_eq!(
        a.1["registry_receipt"]["key_fingerprint"],
        json!(m.att.key_fingerprint),
        "{label} A receipt"
    );
    if let Some(n) = before {
        assert_eq!(kit.log_size().await, n + 1, "{label} A: one leaf");
        assert_eq!(
            kit.leaf(n).await["key_fingerprint"],
            json!(m.att.key_fingerprint),
            "{label} A leaf"
        );
    }
    let replay = kit.publish_idem(&req, Some(&idem)).await;
    assert_eq!(replay.0, StatusCode::OK, "{label} A replay: {}", replay.1);
    assert_eq!(replay.1["ctx_id"], a.1["ctx_id"], "{label} A replay");
    // The same replay once the window has closed is refused, not answered
    // from the stored reply: the §6.3 checks run before the commit's
    // idempotency gate, fail-closed as on the did:web path.
    let t0 = kit.clock.load(Ordering::SeqCst);
    kit.clock
        .store(m.att.not_after.timestamp_millis() + 1_000, Ordering::SeqCst);
    assert_eq!(
        wire(&kit.publish_idem(&req, Some(&idem)).await),
        is(
            403,
            "key_not_authorized",
            Some("outside_attestation_window")
        ),
        "{label} A replay after the window"
    );
    kit.clock.store(t0, Ordering::SeqCst);
    let ctx_a = a.1["ctx_id"].as_str().unwrap().to_string();

    // kl-003 C: a keyless key id and no member (signed under the same key;
    // the signature is outside the content_hash preimage).
    let mut c = plain(&controller_producer(&controller, 61), "kl-003 C");
    c.signature.key_id = m.att.key_id.clone();
    assert_eq!(
        wire(&kit.publish(&c).await),
        is(400, "key_resolution_failed", Some("attestation_missing")),
        "{label} C"
    );

    // kl-003 D: an attestation on another registry.
    let foreign = KeyAttestationRef {
        ctx_id: CtxId(format!(
            "acdp://other.test/{}",
            m.reference.ctx_id.as_str().rsplit('/').next().unwrap()
        )),
        content_hash: m.reference.content_hash.clone(),
    };
    let d = plain(&m.producer_with(&m.att, foreign), "kl-003 D");
    assert_eq!(
        wire(&kit.publish(&d).await),
        is(400, "key_resolution_failed", Some("attestation_foreign")),
        "{label} D"
    );

    // §6.3 step 3: nothing there, and an ordinary body is no attestation.
    let missing = plain(&m.producer_with(&m.att, dangling(AUTHORITY)), "not found");
    assert_eq!(
        wire(&kit.publish(&missing).await),
        is(400, "key_resolution_failed", Some("attestation_not_found")),
        "{label} not found"
    );
    let probe_ref = KeyAttestationRef {
        ctx_id: CtxId(ctx_a.clone()),
        content_hash: kit.body_of(&ctx_a).await.content_hash,
    };
    let probe = plain(&m.producer_with(&m.att, probe_ref), "probe");
    assert_eq!(
        wire(&kit.publish(&probe).await),
        is(400, "key_resolution_failed", Some("attestation_not_found")),
        "{label} an ordinary body is not an attestation"
    );

    // kl-003 E: the pinned bytes differ.
    let e_ref = KeyAttestationRef {
        content_hash: ContentHash(format!("sha256:{}", "0".repeat(64))),
        ..m.reference.clone()
    };
    let e = plain(&m.producer_with(&m.att, e_ref), "kl-003 E");
    assert_eq!(
        wire(&kit.publish(&e).await),
        is(400, "key_resolution_failed", Some("attestation_mismatch")),
        "{label} E"
    );

    // kl-003 F: another controller claims the attestation.
    let controller2 = kit.controller("pub2", &[(same.clone(), false)]);
    let mut att2 = m.att.clone();
    att2.controller = controller2.clone();
    att2.key_id = keyless_key_id(&controller2, &m.att.key_fingerprint).unwrap();
    let f = plain(&m.producer_with(&att2, m.reference.clone()), "kl-003 F");
    assert_eq!(f.agent_id.as_str(), controller2);
    assert_eq!(
        wire(&kit.publish(&f).await),
        is(403, "key_not_authorized", Some("attestation_mismatch")),
        "{label} F controller"
    );
    // kl-003 F: another key under the attestation.
    let k62 = EphemeralKey::from_key(signing_key(62));
    let mut att3 = m.att.clone();
    att3.key_fingerprint = k62.fingerprint().unwrap();
    att3.public_key_jwk = k62.jwk().unwrap();
    att3.key_id = k62.key_id(&controller).unwrap();
    let other_key = Minted {
        att: att3.clone(),
        reference: m.reference.clone(),
        seed: 62,
    };
    let f2 = plain(&other_key.producer(), "kl-003 F key");
    assert_eq!(
        wire(&kit.publish(&f2).await),
        is(403, "key_not_authorized", Some("attestation_mismatch")),
        "{label} F key"
    );

    // kl-003 H: the signature is another body's.
    let mut h = plain(&p, "kl-003 H");
    h.signature.value = plain(&p, "kl-003 H other").signature.value;
    let rh = kit.publish(&h).await;
    assert_eq!(wire(&rh), is(400, "invalid_signature", None), "{label} H");
    assert!(
        rh.1["error"].get("details").is_none(),
        "{label} H: {}",
        rh.1
    );

    // kl-003 G: withdrawn by the registry.
    let mg = kit
        .mint(66, &controller, kit.oidc(SUBJECT), Some(600))
        .await;
    let w = kit.admin_retract(mg.reference.ctx_id.as_str()).await;
    assert_eq!(w.0, StatusCode::OK, "{label} G retract: {}", w.1);
    assert_eq!(
        wire(&kit.publish(&plain(&mg.producer(), "kl-003 G")).await),
        is(403, "key_not_authorized", Some("attestation_withdrawn")),
        "{label} G"
    );

    // kl-003 B: the window. Mint two hours ago (window [-2h, -2h+600 s)).
    let real = kit.clock.load(Ordering::SeqCst);
    let past = real - 2 * 3600 * 1000;
    kit.clock.store(past, Ordering::SeqCst);
    let mb = kit
        .mint(67, &controller, kit.oidc(SUBJECT), Some(600))
        .await;
    let mb2 = kit
        .mint(68, &controller, kit.oidc(SUBJECT), Some(600))
        .await;
    // The handler's clock still inside the window: only the store's
    // created_at (real time) refuses — the authoritative check.
    let rb2 = kit.publish(&plain(&mb2.producer(), "kl-003 B store")).await;
    assert_eq!(
        wire(&rb2),
        is(
            403,
            "key_not_authorized",
            Some("outside_attestation_window")
        ),
        "{label} B store"
    );
    assert!(
        rb2.1["error"]["message"]
            .as_str()
            .unwrap()
            .contains("created_at"),
        "{label} B: the store refused: {}",
        rb2.1
    );
    // Back to real time: the provisional check refuses first, and before
    // the signature (order: step 6 before step 8).
    kit.clock.store(real, Ordering::SeqCst);
    let rb = kit.publish(&plain(&mb.producer(), "kl-003 B")).await;
    assert_eq!(
        wire(&rb),
        is(
            403,
            "key_not_authorized",
            Some("outside_attestation_window")
        ),
        "{label} B"
    );
    assert!(
        rb.1["error"]["message"]
            .as_str()
            .unwrap()
            .contains("registry's time"),
        "{label} B: the handler refused: {}",
        rb.1
    );
    let mut bad = plain(&mb.producer(), "kl-003 B bad sig");
    bad.signature.value = plain(&mb.producer(), "x").signature.value;
    assert_eq!(
        wire(&kit.publish(&bad).await),
        is(
            403,
            "key_not_authorized",
            Some("outside_attestation_window")
        ),
        "{label} B before H"
    );

    // kl-009 B: the controller stops listing the identity while the
    // attestation is live; once the cached document expires the publish
    // is refused.
    let revoked = kit.controller("rev", &[(same.clone(), false)]);
    let mr = kit.mint(69, &revoked, kit.oidc(SUBJECT), Some(600)).await;
    kit.list(&revoked, &[]);
    kit.resolver.invalidate(&revoked);
    assert_eq!(
        wire(&kit.publish(&plain(&mr.producer(), "kl-009 B")).await),
        is(403, "key_not_authorized", Some("identity_not_authorized")),
        "{label} kl-009 B"
    );

    // kl-004, from the kl-003 A body (keyless, SUBJECT, not org-wide).
    let body_a = kit.body_of(&ctx_a).await;
    let m_other = kit
        .mint(70, &controller, kit.oidc(other_subject), Some(600))
        .await;
    assert_eq!(
        wire(
            &kit.publish(&successor(&m_other.producer(), &body_a, "kl-004 A"))
                .await
        ),
        is(403, "not_authorized", Some("keyless_identity_mismatch")),
        "{label} kl-004 A"
    );
    let m_same = kit
        .mint(71, &controller, kit.oidc(SUBJECT), Some(600))
        .await;
    let b = kit
        .publish(&successor(&m_same.producer(), &body_a, "kl-004 B"))
        .await;
    assert_eq!(b.0, StatusCode::CREATED, "{label} kl-004 B: {}", b.1);
    let body_b = kit.body_of(b.1["ctx_id"].as_str().unwrap()).await;
    let m_org = kit
        .mint(72, &controller, kit.oidc(org_subject), Some(600))
        .await;
    let cc = kit
        .publish(&successor(&m_org.producer(), &body_b, "kl-004 C"))
        .await;
    assert_eq!(cc.0, StatusCode::CREATED, "{label} kl-004 C: {}", cc.1);
    let body_c = kit.body_of(cc.1["ctx_id"].as_str().unwrap()).await;
    let dd = kit
        .publish(&successor(
            &controller_producer(&controller, 40),
            &body_c,
            "kl-004 D",
        ))
        .await;
    assert_eq!(dd.0, StatusCode::CREATED, "{label} kl-004 D: {}", dd.1);
    let body_d = kit.body_of(dd.1["ctx_id"].as_str().unwrap()).await;
    assert_eq!(
        wire(
            &kit.publish(&successor(&m_same.producer(), &body_d, "kl-004 E"))
                .await
        ),
        is(403, "not_authorized", Some("keyless_identity_mismatch")),
        "{label} kl-004 E"
    );

    // kl-004 F (R13): a keyless-signed event, whatever its target and
    // identity, org-wide included, on both endpoints.
    let target_b = body_b.ctx_id.as_str().to_string();
    let nowhere = dangling(AUTHORITY).ctx_id.as_str().to_string();
    for (target, key_id, endpoint, event_type) in [
        (&target_b, &m_same.att.key_id, "retract", "retracted"),
        (&target_b, &m_org.att.key_id, "retract", "retracted"),
        (&nowhere, &m_same.att.key_id, "retract", "retracted"),
        (&target_b, &m_org.att.key_id, "republish", "republished"),
    ] {
        let envelope = event_under(target, &controller, key_id, event_type);
        assert_eq!(
            wire(&kit.post_event(target, endpoint, &envelope).await),
            is(
                400,
                "key_resolution_failed",
                Some("keyless_event_unsupported")
            ),
            "{label} kl-004 F {target} {endpoint}"
        );
    }

    // kl-007 B: a DID document carrying an `acdp-kl-` verification method
    // does not make the key resolvable.
    let k73 = EphemeralKey::from_key(signing_key(73));
    let fragment = k73.key_id(&controller).unwrap();
    let fragment = fragment.rsplit('#').next().unwrap().to_string();
    let squatter = kit.controller_with_keys("squat", &[], &[(&fragment, 73), ("key-1", 74)]);
    let mut sq = plain(&controller_producer(&squatter, 73), "kl-007 B");
    sq.signature.key_id = format!("{squatter}#{fragment}");
    let r = kit.publish(&sq).await;
    assert_eq!(
        (r.0.as_u16(), r.1["error"]["code"].as_str()),
        (400, Some("key_resolution_failed")),
        "{label} kl-007 B: {}",
        r.1
    );
}

// ── keyless publish under a P-256 ephemeral key ────────────────────────────

/// A minted attestation for a P-256 ephemeral key. The key is not `Clone`
/// and zeroizes on drop, so the scalar is held and the key rebuilt per use.
pub struct MintedP256 {
    pub att: KeyAttestation,
    pub reference: KeyAttestationRef,
    pub scalar: [u8; 32],
}

/// The P-256 signing key for `scalar`.
pub fn p256_signing_key(scalar: &[u8; 32]) -> AcdpSigningKey {
    AcdpSigningKey::P256(P256SigningKey::from_bytes(scalar).expect("a P-256 scalar"))
}

/// A fresh random P-256 scalar.
pub fn p256_scalar() -> [u8; 32] {
    P256SigningKey::generate().seed_bytes()
}

impl MintedP256 {
    /// The SDK's keyless producer for this attestation.
    pub fn producer(&self) -> Producer {
        Producer::keyless(
            p256_signing_key(&self.scalar),
            &self.att,
            self.reference.clone(),
        )
        .expect("keyless producer")
    }
}

impl Kit {
    /// Mint an attestation for the P-256 key `scalar`. The SDK's request
    /// carries the R8 proof of possession: a 64-byte IEEE 1363 `r||s`.
    pub async fn mint_p256(
        &self,
        scalar: [u8; 32],
        controller: &str,
        descriptor: IdentityDescriptor,
        ttl: Option<u32>,
    ) -> MintedP256 {
        let key = EphemeralKey::from_key(p256_signing_key(&scalar));
        let req = Self::request(&key, controller, descriptor, ttl);
        assert_eq!(req.algorithm, "ecdsa-p256");
        assert_eq!(
            B64.decode(&req.pop).expect("base64 pop").len(),
            64,
            "R8: the P-256 pop is P1363 r||s"
        );
        let (status, v) = self.post(&req).await;
        assert_eq!(status, StatusCode::CREATED, "mint p256: {v}");
        let body: acdp::types::body::Body = serde_json::from_value(v["body"].clone()).unwrap();
        let att = KeyAttestation::from_body(&body).unwrap();
        MintedP256 {
            reference: KeyAttestationRef {
                ctx_id: body.ctx_id.clone(),
                content_hash: body.content_hash.clone(),
            },
            att,
            scalar,
        }
    }
}

/// §6.3 end to end under a P-256 ephemeral key: mint, publish (201, then
/// an idempotent 200), the receipt's and the leaf's `key_fingerprint` are
/// the attested key's (the SDK's fingerprint over the compressed SEC1
/// point), and a body signed by another P-256 key under the same keyless
/// key id is `invalid_signature` with no reason (kl-003 H).
pub async fn assert_keyless_p256_publish(kit: &Kit, label: &str) {
    let has_log = kit_has_log(kit).await;
    let controller = kit.controller("p256", &[(kit.oidc_identity(SUBJECT), false)]);

    let scalar = p256_scalar();
    let sdk = EphemeralKey::from_key(p256_signing_key(&scalar));
    let fingerprint = sdk.fingerprint().unwrap();
    let m = kit
        .mint_p256(scalar, &controller, kit.oidc(SUBJECT), Some(600))
        .await;
    assert_eq!(m.att.algorithm, "ecdsa-p256", "{label}");
    assert_eq!(m.att.key_fingerprint, fingerprint, "{label} attested");
    assert_eq!(m.att.public_key_jwk, sdk.jwk().unwrap(), "{label} jwk");
    assert_eq!(m.att.key_id, sdk.key_id(&controller).unwrap(), "{label}");
    assert!(
        m.att.key_id.starts_with(&format!("{controller}#acdp-kl-")),
        "{label}: {}",
        m.att.key_id
    );

    let req = plain(&m.producer(), "p256 keyless");
    assert_eq!(req.signature.algorithm, "ecdsa-p256", "{label}");
    assert_eq!(req.signature.key_id, m.att.key_id, "{label}");
    assert_eq!(
        B64.decode(&req.signature.value).unwrap().len(),
        64,
        "{label}: P1363 body signature"
    );
    let before = if has_log {
        Some(kit.log_size().await)
    } else {
        None
    };
    let idem = unique("p256");
    let a = kit.publish_idem(&req, Some(&idem)).await;
    assert_eq!(a.0, StatusCode::CREATED, "{label} p256 publish: {}", a.1);
    assert_eq!(
        a.1["registry_receipt"]["key_fingerprint"],
        json!(fingerprint),
        "{label} p256 receipt"
    );
    if let Some(n) = before {
        assert_eq!(kit.log_size().await, n + 1, "{label} p256: one leaf");
        assert_eq!(
            kit.leaf(n).await["key_fingerprint"],
            json!(fingerprint),
            "{label} p256 leaf"
        );
    }
    let replay = kit.publish_idem(&req, Some(&idem)).await;
    assert_eq!(
        replay.0,
        StatusCode::OK,
        "{label} p256 replay: {}",
        replay.1
    );
    assert_eq!(replay.1["ctx_id"], a.1["ctx_id"], "{label} p256 replay");
    assert_eq!(
        kit.body_of(a.1["ctx_id"].as_str().unwrap())
            .await
            .signature
            .key_id,
        m.att.key_id,
        "{label} stored key id"
    );

    // Another P-256 key signs the same content hash under the keyless key
    // id: the attested key does not verify it.
    let mut forged = plain(&m.producer(), "p256 forged");
    let other = p256_signing_key(&p256_scalar());
    let (alg, value) = other.sign_content_hash(&forged.content_hash);
    assert_eq!(alg, "ecdsa-p256");
    forged.signature.value = value;
    let before = if has_log {
        Some(kit.log_size().await)
    } else {
        None
    };
    let r = kit.publish(&forged).await;
    assert_eq!(
        wire(&r),
        is(400, "invalid_signature", None),
        "{label} p256 forged: {}",
        r.1
    );
    assert!(
        r.1["error"].get("details").is_none(),
        "{label} p256 forged: {}",
        r.1
    );
    if let Some(n) = before {
        assert_eq!(kit.log_size().await, n, "{label} p256 forged: no leaf");
    }
}
