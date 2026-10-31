//! RFC-ACDP-0021 §5.2 steps 2–3: workload-token verification against an
//! in-process HTTPS issuer (Phase 4; nothing here is reachable over HTTP).
//!
//! The issuer presents the `didweb` fixture's `agents.test` leaf; the
//! resolver maps `agents.test` to loopback, which the test SSRF policy
//! allows (and nothing else private). The cache clock is injected, so TTL,
//! stale-if-error and the unknown-kid refetch window are exercised without
//! waiting. Every refusal is checked against its exact wire code, status
//! and `details.reason` from `registries/error-codes.md`.

// Only the fixture TLS server and its CA are used here.
#[allow(dead_code)]
mod didweb;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicI64, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use acdp::crypto::SigningKey;
use acdp::safe_http::SsrfPolicy;
use acdp::types::keyless::{token_hash, IdentityDescriptor, Jwk};
use acdp_registry_core::callback::StaticResolver;
use acdp_registry_core::keyless::{
    verify, JwksCache, KeySource, KeylessRefusal, Outbound, VerifiedToken,
};
use acdp_registry_types::config::{KeylessConfig, OidcIssuer, SpiffeDomain};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};

const REGISTRY_DID: &str = "did:web:registry.test";
const SUBJECT: &str = "repo:acme/app:ref:refs/heads/main";
const TRUST_DOMAIN: &str = "prod.example.test";
const SPIFFE_SUB: &str = "spiffe://prod.example.test/ns/a/sa/b";
/// The injected clock's start, Unix ms.
const T0_MS: i64 = 1_800_000_000_000;
const TTL_S: u64 = 300;
const STALE_S: u64 = 3600;

// ── the issuer ─────────────────────────────────────────────────────────────

/// An HTTPS issuer: serves `/jwks` and `/bundle` (the same key set),
/// `/.well-known/openid-configuration`, a 70 KiB `/big`, a 302
/// `/redirect` to `/jwks`, and a `/slow` that answers after 3 s. Counts
/// hits per path.
struct Issuer {
    jwks: Mutex<Value>,
    discovery: Mutex<Value>,
    status: AtomicU16,
    cache_control: Mutex<Option<String>>,
    hits: Mutex<HashMap<&'static str, usize>>,
}

impl Issuer {
    fn hit(&self, path: &'static str) {
        *self.hits.lock().unwrap().entry(path).or_default() += 1;
    }

    fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    fn keys_response(&self) -> Response {
        let status = StatusCode::from_u16(self.status.load(Ordering::SeqCst)).unwrap();
        if status != StatusCode::OK {
            return status.into_response();
        }
        let mut resp = axum::Json(self.jwks.lock().unwrap().clone()).into_response();
        if let Some(cc) = self.cache_control.lock().unwrap().clone() {
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, cc.parse().unwrap());
        }
        resp
    }

    fn serve(&self, keys: Vec<Value>) {
        *self.jwks.lock().unwrap() = json!({ "keys": keys });
    }
}

async fn spawn_issuer() -> (Arc<Issuer>, String) {
    let iss = Arc::new(Issuer {
        jwks: Mutex::new(json!({ "keys": [] })),
        discovery: Mutex::new(json!({})),
        status: AtomicU16::new(200),
        cache_control: Mutex::new(None),
        hits: Mutex::new(HashMap::new()),
    });
    let (a, b, c, d, e, f) = (
        iss.clone(),
        iss.clone(),
        iss.clone(),
        iss.clone(),
        iss.clone(),
        iss.clone(),
    );
    let app = axum::Router::new()
        .route(
            "/jwks",
            get(move || async move {
                a.hit("jwks");
                a.keys_response()
            }),
        )
        .route(
            "/bundle",
            get(move || async move {
                b.hit("bundle");
                b.keys_response()
            }),
        )
        .route(
            "/.well-known/openid-configuration",
            get(move || async move {
                c.hit("discovery");
                axum::Json(c.discovery.lock().unwrap().clone())
            }),
        )
        .route(
            "/big",
            get(move || async move {
                d.hit("big");
                let pad = "x".repeat(70 * 1024);
                axum::Json(json!({ "keys": [], "pad": pad }))
            }),
        )
        .route(
            "/redirect",
            get(move || async move {
                e.hit("redirect");
                (StatusCode::FOUND, [(header::LOCATION, "/jwks")]).into_response()
            }),
        )
        .route(
            "/slow",
            get(move || async move {
                f.hit("slow");
                tokio::time::sleep(Duration::from_secs(3)).await;
                f.keys_response()
            }),
        );
    let addr = didweb::spawn_tls_server(app).await;
    let base = format!("https://{}:{}", didweb::DIDWEB_AUTHORITY, addr.port());
    (iss, base)
}

// ── keys and tokens ────────────────────────────────────────────────────────

struct Signer {
    alg: Algorithm,
    kid: String,
    enc: EncodingKey,
    /// The public JWK the issuer publishes (with `kid`).
    jwk: Value,
}

impl Signer {
    fn with_use(&self, u: &str) -> Value {
        let mut j = self.jwk.clone();
        j["use"] = json!(u);
        j
    }
}

/// Ed25519 (EdDSA) from a seed: PKCS#8 v1 DER is a fixed prefix + seed.
fn ed_signer(seed: u8, kid: &str) -> Signer {
    let mut der = hex_bytes("302e020100300506032b657004220420");
    der.extend_from_slice(&[seed; 32]);
    let public = SigningKey::from_bytes(&[seed; 32]).verifying_key_bytes();
    Signer {
        alg: Algorithm::EdDSA,
        kid: kid.into(),
        enc: EncodingKey::from_ed_der(&der),
        jwk: json!({"kty": "OKP", "crv": "Ed25519", "x": URL_SAFE_NO_PAD.encode(public), "kid": kid}),
    }
}

/// A fresh P-256 key (ES256).
fn es_signer(kid: &str) -> Signer {
    let kp = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let raw = kp.public_key_raw();
    assert_eq!(raw.len(), 65, "uncompressed SEC1 point");
    Signer {
        alg: Algorithm::ES256,
        kid: kid.into(),
        enc: EncodingKey::from_ec_der(&kp.serialize_der()),
        jwk: json!({
            "kty": "EC", "crv": "P-256", "kid": kid,
            "x": URL_SAFE_NO_PAD.encode(&raw[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&raw[33..65]),
        }),
    }
}

/// A 2048-bit RSA key (RS256), generated once per test binary.
fn rs_signer(kid: &str) -> Signer {
    use rsa::pkcs1::EncodeRsaPrivateKey as _;
    use rsa::traits::PublicKeyParts as _;
    static KEY: OnceLock<rsa::RsaPrivateKey> = OnceLock::new();
    let key = KEY.get_or_init(|| {
        rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).expect("RSA keygen")
    });
    Signer {
        alg: Algorithm::RS256,
        kid: kid.into(),
        enc: EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes()),
        jwk: json!({
            "kty": "RSA", "kid": kid,
            "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
        }),
    }
}

fn hex_bytes(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

fn sign(s: &Signer, claims: &Value) -> String {
    let mut h = Header::new(s.alg);
    h.kid = Some(s.kid.clone());
    jsonwebtoken::encode(&h, claims, &s.enc).unwrap()
}

/// A token with a hand-built header and a junk signature.
fn raw_token(header: &Value, claims: &Value) -> String {
    let b = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    format!("{}.{}.c2lnbmF0dXJl", b(header), b(claims))
}

/// The request's ephemeral `public_key_jwk`.
fn ephemeral() -> Jwk {
    Jwk::from_ed25519(&SigningKey::from_bytes(&[42; 32]).verifying_key_bytes())
}

// ── the harness ────────────────────────────────────────────────────────────

struct Fx {
    iss: Arc<Issuer>,
    base: String,
    clock: Arc<AtomicI64>,
    cfg: KeylessConfig,
    resolver: StaticResolver,
}

impl Fx {
    async fn new() -> Self {
        let (iss, base) = spawn_issuer().await;
        let cfg = KeylessConfig {
            enabled: true,
            oidc_issuers: vec![OidcIssuer {
                issuer: base.clone(),
                jwks_uri: Some(format!("{base}/jwks")),
            }],
            spiffe_trust_domains: vec![SpiffeDomain {
                trust_domain: TRUST_DOMAIN.into(),
                bundle_endpoint: format!("{base}/bundle"),
                jwt_issuer: None,
            }],
            jwks_cache_ttl_seconds: TTL_S,
            jwks_max_stale_seconds: STALE_S,
            fetch_timeout_ms: 2000,
            ..KeylessConfig::default()
        };
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        Self {
            iss,
            base,
            clock: Arc::new(AtomicI64::new(T0_MS)),
            cfg,
            resolver: StaticResolver::new().with(didweb::DIDWEB_AUTHORITY, &[loopback]),
        }
    }

    fn cache(&self) -> JwksCache {
        let clock = self.clock.clone();
        let outbound = Outbound {
            policy: SsrfPolicy {
                allow_loopback_resolved: true,
                ..SsrfPolicy::default()
            },
            resolver: Arc::new(self.resolver.clone()),
            extra_roots: Vec::new(),
        }
        .with_root_pem(didweb::ca_pem().as_bytes())
        .unwrap();
        JwksCache::new(
            &self.cfg,
            outbound,
            Arc::new(move || clock.load(Ordering::SeqCst)),
        )
    }

    fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst) / 1000
    }

    fn advance(&self, secs: i64) {
        self.clock.fetch_add(secs * 1000, Ordering::SeqCst);
    }

    /// OIDC claims every check accepts.
    fn claims(&self) -> Value {
        let now = self.now();
        json!({
            "iss": self.base, "sub": SUBJECT, "aud": REGISTRY_DID,
            "exp": now + 300, "iat": now, "nbf": now,
        })
    }

    fn oidc(&self, token: String) -> IdentityDescriptor {
        IdentityDescriptor::oidc(&self.base, SUBJECT, token)
    }

    async fn check(
        &self,
        cache: &JwksCache,
        id: &IdentityDescriptor,
    ) -> Result<VerifiedToken, KeylessRefusal> {
        verify(&self.cfg, cache, id, REGISTRY_DID, &ephemeral(), self.now()).await
    }
}

/// `(code, status, details.reason)` on the wire.
fn wire(r: KeylessRefusal) -> (String, u16, Option<String>) {
    let e = r.into_error();
    let v = serde_json::to_value(acdp_registry_types::error::WireError::from(&e)).unwrap();
    (
        e.wire_code().to_owned(),
        e.http_status(),
        v["error"]["details"]["reason"].as_str().map(str::to_owned),
    )
}

fn refused<T: std::fmt::Debug>(r: Result<T, KeylessRefusal>) -> (String, u16, Option<String>) {
    wire(r.expect_err("must be refused"))
}

fn is(code: &str, status: u16, reason: &str) -> (String, u16, Option<String>) {
    (code.into(), status, Some(reason.into()))
}

fn token_invalid() -> (String, u16, Option<String>) {
    is("key_not_authorized", 403, "token_invalid")
}

fn unreachable() -> (String, u16, Option<String>) {
    is("key_resolution_unreachable", 502, "issuer_unreachable")
}

// ── success paths ──────────────────────────────────────────────────────────

#[tokio::test]
async fn eddsa_es256_and_rs256_oidc_tokens_verify() {
    let fx = Fx::new().await;
    let (ed, es, rs) = (ed_signer(1, "ed"), es_signer("es"), rs_signer("rs"));
    fx.iss
        .serve(vec![ed.jwk.clone(), es.jwk.clone(), rs.with_use("sig")]);
    let cache = fx.cache();
    for s in [&ed, &es, &rs] {
        let token = sign(s, &fx.claims());
        let got = fx.check(&cache, &fx.oidc(token.clone())).await.unwrap();
        assert_eq!(got.exp, fx.now() + 300, "{:?}", s.alg);
        assert_eq!(got.token_hash, token_hash(&token));
    }
    // One fetch served all three.
    assert_eq!(fx.iss.hits("jwks"), 1);
    assert_eq!(cache.stats().hits, 2);
}

#[tokio::test]
async fn oidc_discovery_finds_the_jwks() {
    let mut fx = Fx::new().await;
    fx.cfg.oidc_issuers[0].jwks_uri = None;
    *fx.iss.discovery.lock().unwrap() =
        json!({"issuer": fx.base, "jwks_uri": format!("{}/jwks", fx.base)});
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
        .await
        .unwrap();
    assert_eq!((fx.iss.hits("discovery"), fx.iss.hits("jwks")), (1, 1));
}

/// A `kid`-less token uses the only key; with two keys it cannot choose.
#[tokio::test]
async fn a_token_without_kid_needs_exactly_one_key() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    let mut h = Header::new(Algorithm::EdDSA);
    h.kid = None;
    let token = jsonwebtoken::encode(&h, &fx.claims(), &ed.enc).unwrap();
    fx.iss.serve(vec![ed.jwk.clone()]);
    fx.check(&fx.cache(), &fx.oidc(token.clone()))
        .await
        .unwrap();
    fx.iss
        .serve(vec![ed.jwk.clone(), ed_signer(2, "other").jwk]);
    assert_eq!(
        refused(fx.check(&fx.cache(), &fx.oidc(token)).await),
        token_invalid()
    );
}

// ── step 2: issuer acceptance, before any fetch ────────────────────────────

#[tokio::test]
async fn an_unlisted_issuer_or_trust_domain_is_refused_with_no_fetch() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    let token = sign(&ed, &fx.claims());
    for id in [
        IdentityDescriptor::oidc(format!("{}/", fx.base), SUBJECT, token.clone()),
        IdentityDescriptor::oidc("https://issuer.elsewhere.test", SUBJECT, token.clone()),
        IdentityDescriptor::spiffe_jwt("spiffe://other.example.test/ns/a/sa/b", token.clone()),
    ] {
        assert_eq!(
            refused(fx.check(&cache, &id).await),
            is("key_not_authorized", 403, "issuer_not_accepted"),
            "{id:?}"
        );
    }
    assert_eq!(fx.iss.hits("jwks") + fx.iss.hits("bundle"), 0);
    assert_eq!(cache.stats().fetches, 0);
}

// ── step 3: token_invalid ──────────────────────────────────────────────────

#[tokio::test]
async fn header_refusals_make_no_fetch() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    let c = fx.claims();
    for h in [
        json!({"alg": "none", "kid": "ed"}),
        json!({"alg": "HS256", "kid": "ed"}),
        json!({"alg": "ES512", "kid": "ed"}),
        json!({"alg": "EdDSA", "kid": "ed", "crit": ["exp"]}),
    ] {
        let id = fx.oidc(raw_token(&h, &c));
        assert_eq!(refused(fx.check(&cache, &id).await), token_invalid(), "{h}");
    }
    // An unsigned `none` token with an empty signature, as attackers send it.
    let b = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    let unsigned = format!("{}.{}.", b(&json!({"alg": "none"})), b(&c));
    assert_eq!(
        refused(fx.check(&cache, &fx.oidc(unsigned)).await),
        ("schema_violation".to_owned(), 400, None),
        "an empty signature segment is not a compact JWT (schema)"
    );
    assert_eq!(fx.iss.hits("jwks"), 0);
}

#[tokio::test]
async fn signature_time_issuer_and_subject_failures_are_token_invalid() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    let now = fx.now();
    let with = |patch: Value| {
        let mut c = fx.claims();
        for (k, v) in patch.as_object().unwrap() {
            if v.is_null() {
                c.as_object_mut().unwrap().remove(k);
            } else {
                c[k] = v.clone();
            }
        }
        c
    };
    // Bad signature: signed by another key under the published kid.
    let forged = sign(&ed_signer(9, "ed"), &fx.claims());
    let cases: Vec<(&str, String)> = vec![
        ("bad signature", forged),
        ("expired: exp == now", sign(&ed, &with(json!({"exp": now})))),
        (
            "expired: exp < now",
            sign(&ed, &with(json!({"exp": now - 1}))),
        ),
        ("exp missing", sign(&ed, &with(json!({"exp": null})))),
        ("exp not a number", sign(&ed, &with(json!({"exp": "soon"})))),
        ("nbf now+61", sign(&ed, &with(json!({"nbf": now + 61})))),
        ("iat now+61", sign(&ed, &with(json!({"iat": now + 61})))),
        (
            "iss differs",
            sign(&ed, &with(json!({"iss": format!("{}/", fx.base)}))),
        ),
        ("iss missing", sign(&ed, &with(json!({"iss": null})))),
        (
            "sub differs",
            sign(&ed, &with(json!({"sub": "repo:acme/other"}))),
        ),
        (
            "sub case differs",
            sign(&ed, &with(json!({"sub": SUBJECT.to_uppercase()}))),
        ),
    ];
    for (what, token) in cases {
        assert_eq!(
            refused(fx.check(&cache, &fx.oidc(token)).await),
            token_invalid(),
            "{what}"
        );
    }
    // The 60 s skew is inclusive; iat/nbf may be absent; iat in the past is fine.
    for ok in [
        with(json!({"nbf": now + 60, "iat": now + 60})),
        with(json!({"nbf": null, "iat": null})),
        with(json!({"iat": now - 86_400})),
    ] {
        fx.check(&cache, &fx.oidc(sign(&ed, &ok))).await.unwrap();
    }
}

/// alg confusion: an `HS256` header over an asymmetric JWK, a JWK whose
/// type does not fit, and a JWK `alg` that disagrees are all refused.
#[tokio::test]
async fn a_key_that_does_not_fit_the_alg_is_token_invalid() {
    let fx = Fx::new().await;
    let es = es_signer("k");
    let mut jwk_alg = es.jwk.clone();
    jwk_alg["alg"] = json!("ES384");
    fx.iss.serve(vec![jwk_alg]);
    let cache = fx.cache();
    assert_eq!(
        refused(fx.check(&cache, &fx.oidc(sign(&es, &fx.claims()))).await),
        token_invalid()
    );
    // EdDSA header against an EC key under the same kid.
    let ed = ed_signer(1, "k");
    fx.iss.serve(vec![es.jwk.clone()]);
    let cache = fx.cache();
    assert_eq!(
        refused(fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims()))).await),
        token_invalid()
    );
}

// ── step 3: audience and confirmation ──────────────────────────────────────

#[tokio::test]
async fn aud_must_contain_the_registry_did_byte_for_byte() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    for aud in [
        json!("did:web:other-registry.test"),
        json!("registry.test"),
        json!("https://registry.test"),
        json!("DID:WEB:REGISTRY.TEST"),
        json!([]),
        json!(["did:web:other-registry.test"]),
        json!(7),
        Value::Null,
    ] {
        let mut c = fx.claims();
        if aud.is_null() {
            c.as_object_mut().unwrap().remove("aud");
        } else {
            c["aud"] = aud.clone();
        }
        assert_eq!(
            refused(fx.check(&cache, &fx.oidc(sign(&ed, &c))).await),
            is("key_not_authorized", 403, "audience_mismatch"),
            "aud {aud}"
        );
    }
    let mut c = fx.claims();
    c["aud"] = json!(["https://other", REGISTRY_DID]);
    fx.check(&cache, &fx.oidc(sign(&ed, &c))).await.unwrap();
}

/// A token failing on both iss and aud reports the earlier (§5.2 order).
#[tokio::test]
async fn token_invalid_precedes_audience_mismatch() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let mut c = fx.claims();
    c["sub"] = json!("someone-else");
    c["aud"] = json!("did:web:other.test");
    assert_eq!(
        refused(fx.check(&fx.cache(), &fx.oidc(sign(&ed, &c))).await),
        token_invalid()
    );
}

#[tokio::test]
async fn cnf_jkt_when_present_must_be_the_presented_keys_thumbprint() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    let thumb = ephemeral().thumbprint().unwrap();
    let other = Jwk::from_ed25519(&[3; 32]).thumbprint().unwrap();
    for cnf in [json!({"jkt": other}), json!({"jkt": 5}), json!("jkt")] {
        let mut c = fx.claims();
        c["cnf"] = cnf.clone();
        assert_eq!(
            refused(fx.check(&cache, &fx.oidc(sign(&ed, &c))).await),
            is("key_not_authorized", 403, "confirmation_mismatch"),
            "cnf {cnf}"
        );
    }
    // Present and right, or absent (kl-002 G): accepted.
    let mut c = fx.claims();
    c["cnf"] = json!({ "jkt": thumb });
    fx.check(&cache, &fx.oidc(sign(&ed, &c))).await.unwrap();
    fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
        .await
        .unwrap();
}

// ── SPIFFE JWT-SVIDs ───────────────────────────────────────────────────────

fn svid_claims(fx: &Fx) -> Value {
    let now = fx.now();
    json!({"sub": SPIFFE_SUB, "aud": [REGISTRY_DID], "exp": now + 300, "iat": now})
}

#[tokio::test]
async fn a_jwt_svid_from_the_bundle_endpoint_verifies() {
    let mut fx = Fx::new().await;
    let es = es_signer("svid-1");
    fx.iss.serve(vec![es.with_use("jwt-svid")]);
    let id = IdentityDescriptor::spiffe_jwt(SPIFFE_SUB, sign(&es, &svid_claims(&fx)));

    // No jwt_issuer configured: iss is not checked.
    fx.check(&fx.cache(), &id).await.unwrap();
    assert_eq!(fx.iss.hits("bundle"), 1);

    // jwt_issuer configured: a mismatching iss is token_invalid, a matching one verifies.
    fx.cfg.spiffe_trust_domains[0].jwt_issuer = Some("https://spire.prod.example.test".into());
    let cache = fx.cache();
    assert_eq!(refused(fx.check(&cache, &id).await), token_invalid());
    let mut c = svid_claims(&fx);
    c["iss"] = json!("https://spire.prod.example.test");
    let ok = IdentityDescriptor::spiffe_jwt(SPIFFE_SUB, sign(&es, &c));
    fx.check(&cache, &ok).await.unwrap();
}

/// RFC-AITP-0002 §6.2/§6.3: only `use: jwt-svid` keys, `kid` required,
/// RSA/ECDSA only.
#[tokio::test]
async fn jwt_svid_rules_from_rfc_aitp_0002() {
    let fx = Fx::new().await;
    let es = es_signer("svid-1");
    let ed = ed_signer(1, "svid-ed");
    // A bundle key with no `use` is ignored, so the kid is unknown.
    fx.iss.serve(vec![es.jwk.clone(), ed.with_use("jwt-svid")]);
    let cache = fx.cache();
    let id = |t| IdentityDescriptor::spiffe_jwt(SPIFFE_SUB, t);
    assert_eq!(
        refused(fx.check(&cache, &id(sign(&es, &svid_claims(&fx)))).await),
        token_invalid()
    );
    // EdDSA is not a JWT-SVID algorithm, even with a jwt-svid key.
    assert_eq!(
        refused(fx.check(&cache, &id(sign(&ed, &svid_claims(&fx)))).await),
        token_invalid()
    );
    // No kid: refused before any fetch of a fresh cache.
    let cold = fx.cache();
    let no_kid = raw_token(&json!({"alg": "ES256", "typ": "JWT"}), &svid_claims(&fx));
    let before = fx.iss.hits("bundle");
    assert_eq!(refused(fx.check(&cold, &id(no_kid)).await), token_invalid());
    assert_eq!(fx.iss.hits("bundle"), before);
}

#[tokio::test]
async fn an_x509_svid_is_a_schema_violation() {
    let fx = Fx::new().await;
    let es = es_signer("svid-1");
    fx.iss.serve(vec![es.with_use("jwt-svid")]);
    let id = IdentityDescriptor::Spiffe {
        subject: SPIFFE_SUB.into(),
        svid_type: "x509-svid".into(),
        proof: sign(&es, &svid_claims(&fx)),
    };
    let cache = fx.cache();
    assert_eq!(
        refused(fx.check(&cache, &id).await),
        ("schema_violation".into(), 400, None)
    );
    assert_eq!(cache.stats().fetches, 0);
}

// ── the cache ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_fresh_copy_is_reused_and_an_expired_one_refetched() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    let run = || async {
        fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
            .await
            .unwrap()
    };
    run().await;
    fx.advance(TTL_S as i64 - 1);
    run().await;
    assert_eq!(
        (fx.iss.hits("jwks"), cache.stats().hits),
        (1, 1),
        "hit inside the TTL"
    );
    fx.advance(1);
    run().await;
    assert_eq!(fx.iss.hits("jwks"), 2, "refetched at the TTL");

    // max-age shortens the TTL (60 here) and is floored at 60 s.
    *fx.iss.cache_control.lock().unwrap() = Some("public, max-age=60".into());
    fx.advance(TTL_S as i64);
    run().await;
    assert_eq!(fx.iss.hits("jwks"), 3);
    fx.advance(60);
    run().await;
    assert_eq!(fx.iss.hits("jwks"), 4, "max-age=60 expired the copy");
    *fx.iss.cache_control.lock().unwrap() = Some("max-age=5".into());
    fx.advance(60);
    run().await;
    assert_eq!(fx.iss.hits("jwks"), 5);
    fx.advance(30);
    run().await;
    assert_eq!(fx.iss.hits("jwks"), 5, "max-age=5 is floored at 60 s");
}

#[tokio::test]
async fn an_outage_serves_the_stale_copy_within_its_window_then_502s() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
        .await
        .unwrap();

    fx.iss.status.store(503, Ordering::SeqCst);
    fx.advance(TTL_S as i64);
    fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
        .await
        .unwrap();
    let s = cache.stats();
    assert_eq!(
        (s.stale_served, s.fetch_failures, fx.iss.hits("jwks")),
        (1, 1, 2)
    );

    // Inside the failure backoff: the stale copy answers with no fetch.
    fx.advance(10);
    fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
        .await
        .unwrap();
    assert_eq!((cache.stats().stale_served, fx.iss.hits("jwks")), (2, 2));

    // Past TTL + max_stale: nothing usable is held.
    fx.advance(STALE_S as i64);
    assert_eq!(
        refused(fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims()))).await),
        unreachable()
    );

    assert_eq!(fx.iss.hits("jwks"), 3);

    // The issuer recovers, but that failed refetch opened a new backoff
    // window: inside it the source answers unreachable with no fetch.
    fx.iss.status.store(200, Ordering::SeqCst);
    assert_eq!(
        refused(fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims()))).await),
        unreachable()
    );
    assert_eq!(fx.iss.hits("jwks"), 3);

    // After the window, the next lookup fetches again.
    fx.advance(30);
    fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims())))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_outage_with_no_copy_is_issuer_unreachable() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    for status in [500u16, 503, 404] {
        fx.iss.status.store(status, Ordering::SeqCst);
        assert_eq!(
            refused(
                fx.check(&fx.cache(), &fx.oidc(sign(&ed, &fx.claims())))
                    .await
            ),
            unreachable(),
            "{status}"
        );
    }
}

/// A cold source that just failed is not refetched for 30 s: requests
/// inside the window answer 502 at once instead of each waiting a timeout.
#[tokio::test]
async fn a_cold_failed_source_answers_unreachable_without_refetching_for_30_seconds() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    fx.iss.status.store(503, Ordering::SeqCst);
    let cache = fx.cache();
    let run = || async { fx.check(&cache, &fx.oidc(sign(&ed, &fx.claims()))).await };
    assert_eq!(refused(run().await), unreachable());
    fx.advance(29);
    assert_eq!(refused(run().await), unreachable());
    assert_eq!((cache.stats().fetches, fx.iss.hits("jwks")), (1, 1));
    // The issuer recovers; past the window the next lookup refetches.
    fx.iss.status.store(200, Ordering::SeqCst);
    fx.advance(1);
    run().await.unwrap();
    assert_eq!((cache.stats().fetches, fx.iss.hits("jwks")), (2, 2));
}

#[tokio::test]
async fn an_unknown_kid_refetches_at_most_once_per_30_seconds() {
    let fx = Fx::new().await;
    let (a, b) = (ed_signer(1, "a"), ed_signer(2, "b"));
    fx.iss.serve(vec![a.jwk.clone()]);
    let cache = fx.cache();
    let tb = || fx.oidc(sign(&b, &fx.claims()));
    // Fetch, then one forced refetch: still unknown.
    assert_eq!(refused(fx.check(&cache, &tb()).await), token_invalid());
    assert_eq!(fx.iss.hits("jwks"), 2);
    // Within 30 s: no further fetch, even though the issuer now has b.
    fx.iss.serve(vec![a.jwk.clone(), b.jwk.clone()]);
    fx.advance(29);
    assert_eq!(refused(fx.check(&cache, &tb()).await), token_invalid());
    assert_eq!(fx.iss.hits("jwks"), 2);
    // After 30 s: one refetch finds the rotated key.
    fx.advance(1);
    fx.check(&cache, &tb()).await.unwrap();
    assert_eq!(fx.iss.hits("jwks"), 3);
}

#[tokio::test]
async fn concurrent_lookups_on_a_cold_cache_make_one_fetch() {
    let fx = Fx::new().await;
    let ed = ed_signer(1, "ed");
    fx.iss.serve(vec![ed.jwk.clone()]);
    let cache = fx.cache();
    let ids: Vec<_> = (0..8).map(|_| fx.oidc(sign(&ed, &fx.claims()))).collect();
    let runs = ids.iter().map(|id| fx.check(&cache, id));
    for r in futures_join_all(runs).await {
        r.unwrap();
    }
    assert_eq!(fx.iss.hits("jwks"), 1);
}

async fn futures_join_all<F: std::future::Future>(fs: impl Iterator<Item = F>) -> Vec<F::Output> {
    let mut set = Vec::new();
    for f in fs {
        set.push(Box::pin(f));
    }
    // Poll all together: each lookup waits on the slot lock the first holds.
    let mut out = Vec::new();
    let joined = async {
        let mut pending: Vec<_> = set.into_iter().map(Some).collect();
        std::future::poll_fn(|cx| {
            let mut done = true;
            for p in pending.iter_mut() {
                if let Some(f) = p {
                    match f.as_mut().poll(cx) {
                        std::task::Poll::Ready(v) => {
                            out.push(v);
                            *p = None;
                        }
                        std::task::Poll::Pending => done = false,
                    }
                }
            }
            if done {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
    };
    joined.await;
    out
}

// ── fetch policy ───────────────────────────────────────────────────────────

/// Each of these fetches fails, so with no copy held the answer is 502
/// `issuer_unreachable`.
async fn unreachable_via(fx: &Fx, jwks_uri: Option<String>) {
    let mut cfg = fx.cfg.clone();
    cfg.oidc_issuers[0].jwks_uri = jwks_uri;
    let ed = ed_signer(1, "ed");
    let fx2 = Fx {
        iss: fx.iss.clone(),
        base: fx.base.clone(),
        clock: fx.clock.clone(),
        cfg,
        resolver: fx.resolver.clone(),
    };
    assert_eq!(
        refused(
            fx2.check(&fx2.cache(), &fx2.oidc(sign(&ed, &fx2.claims())))
                .await
        ),
        unreachable()
    );
}

#[tokio::test]
async fn an_oversize_jwks_is_a_fetch_failure() {
    let fx = Fx::new().await;
    unreachable_via(&fx, Some(format!("{}/big", fx.base))).await;
    assert_eq!(fx.iss.hits("big"), 1);
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let fx = Fx::new().await;
    fx.iss.serve(vec![ed_signer(1, "ed").jwk]);
    unreachable_via(&fx, Some(format!("{}/redirect", fx.base))).await;
    assert_eq!((fx.iss.hits("redirect"), fx.iss.hits("jwks")), (1, 0));
}

#[tokio::test]
async fn a_slow_issuer_times_out() {
    let mut fx = Fx::new().await;
    fx.cfg.fetch_timeout_ms = 300;
    fx.iss.serve(vec![ed_signer(1, "ed").jwk]);
    unreachable_via(&fx, Some(format!("{}/slow", fx.base))).await;
}

#[tokio::test]
async fn an_unreachable_host_is_issuer_unreachable() {
    let fx = Fx::new().await;
    unreachable_via(
        &fx,
        Some(format!("https://{}:1/jwks", didweb::DIDWEB_AUTHORITY)),
    )
    .await;
}

#[tokio::test]
async fn a_dns_answer_mixing_a_private_address_is_refused_at_connect() {
    let mut fx = Fx::new().await;
    fx.iss.serve(vec![ed_signer(1, "ed").jwk]);
    let mixed: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap(), "10.0.0.7".parse().unwrap()];
    fx.resolver = StaticResolver::new().with(didweb::DIDWEB_AUTHORITY, &mixed);
    unreachable_via(&fx, Some(format!("{}/jwks", fx.base))).await;
    assert_eq!(fx.iss.hits("jwks"), 0, "no connection was made");
}

#[tokio::test]
async fn a_discovery_document_pointing_somewhere_unsafe_is_refused() {
    let mut fx = Fx::new().await;
    fx.iss.serve(vec![ed_signer(1, "ed").jwk]);
    fx.resolver = fx
        .resolver
        .clone()
        .with("internal.test", &["10.0.0.9".parse().unwrap()]);
    let port = fx.base.rsplit(':').next().unwrap().to_owned();
    for discovery in [
        json!({"issuer": fx.base, "jwks_uri": "https://10.0.0.1/jwks"}),
        json!({"issuer": fx.base, "jwks_uri": format!("https://internal.test:{port}/jwks")}),
        json!({"issuer": fx.base, "jwks_uri": format!("http://{}:{port}/jwks", didweb::DIDWEB_AUTHORITY)}),
        json!({"issuer": format!("{}/", fx.base), "jwks_uri": format!("{}/jwks", fx.base)}),
        json!({"jwks_uri": format!("{}/jwks", fx.base)}),
    ] {
        *fx.iss.discovery.lock().unwrap() = discovery.clone();
        unreachable_via(&fx, None).await;
    }
    assert_eq!(fx.iss.hits("jwks"), 0);
    assert_eq!(fx.iss.hits("discovery"), 5);
}

/// The cache answers for configured sources only.
#[tokio::test]
async fn sources_outside_config_are_unreachable_without_a_fetch() {
    let fx = Fx::new().await;
    let cache = fx.cache();
    assert!(cache
        .keys(&KeySource::Oidc("https://elsewhere.test".into()))
        .await
        .is_err());
    assert_eq!(cache.stats().fetches, 0);
}
