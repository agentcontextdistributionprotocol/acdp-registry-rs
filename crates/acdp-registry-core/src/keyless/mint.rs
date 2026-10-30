//! RFC-ACDP-0021 §5.2 steps 4, 5 and 7 and the §5.3 attestation body: the
//! pure part of `POST /key-attestations`, with no I/O.
//!
//! - [`verify_pop`] and [`check_algorithm`]: steps 4 and 5. The proof is
//!   verified under the JWK's own algorithm, so a request whose
//!   `algorithm` disagrees with its key, but whose proof is good, reaches
//!   step 5 (`unsupported_algorithm`) instead of collapsing into step 4
//!   (`pop_invalid`). Both use the SDK's `pop_preimage` and `Jwk::verify`,
//!   which accepts a P-256 proof only in its 64-byte P1363 `r||s` form
//!   (R8: DER is `pop_invalid`).
//! - [`window`]: step 7.
//! - [`attestation`] and [`build`]: the §5.3 metadata and the publish
//!   request the registry signs as a producer under its own DID. The title
//!   and summary follow kl-001 vector 1, so the canonical form is
//!   byte-for-byte the golden one.

use acdp::crypto::SigningKey;
use acdp::error::AcdpError;
use acdp::producer::Producer;
use acdp::types::keyless::{
    keyless_key_id, KeyAttestation, KeyAttestationRequest, KeylessReason, ListedIdentity,
    ATTESTATION_VERSION, DEFAULT_TTL_SECONDS, KEY_ATTESTATION_SCHEMA_URI, KEY_ATTESTATION_TYPE,
    MAX_WINDOW_SECONDS,
};
use acdp::types::primitives::{AgentDid, ContextType, Visibility};
use acdp::types::publish::PublishRequest;
use acdp_registry_types::RegistryError;
use chrono::{DateTime, Utc};

use super::KeylessRefusal;

/// The attestation body's `title` (kl-001 vector 1).
pub const TITLE: &str = "Key attestation";

/// The attestation body's `acdp_version` (§5.3: at least 0.5.0).
pub const ACDP_VERSION: &str = "0.5.0";

/// The `pop_invalid` reason token.
pub const POP_INVALID: &str = "pop_invalid";

/// The attestation body's `summary` (kl-001 vector 1).
pub fn summary(authority: &str, algorithm: &str, controller: &str) -> String {
    format!("{authority} attests an ephemeral {algorithm} key for {controller} (RFC-ACDP-0021).")
}

/// §5.2 step 4: `pop` verifies under `public_key_jwk`, for this registry.
/// Returns the JWK's algorithm, which step 5 compares with the request's.
pub fn verify_pop(
    req: &KeyAttestationRequest,
    registry_did: &str,
) -> Result<&'static str, RegistryError> {
    let schema = |e: AcdpError| RegistryError::Acdp(e);
    let alg = req.public_key_jwk.algorithm().map_err(schema)?;
    let preimage = req.pop_preimage(registry_did).map_err(schema)?;
    req.public_key_jwk
        .verify(alg, &req.pop, &preimage)
        .map_err(|e| {
            RegistryError::with_reason(
                KeylessReason::PopInvalid
                    .error(format!("pop does not verify under public_key_jwk: {e}")),
                POP_INVALID,
            )
        })?;
    Ok(alg)
}

/// §5.2 step 5: `algorithm` is one this registry supports and the JWK's.
pub fn check_algorithm(
    req: &KeyAttestationRequest,
    jwk_algorithm: &str,
    supported: &[String],
) -> Result<(), RegistryError> {
    if req.algorithm != jwk_algorithm {
        return Err(RegistryError::Acdp(AcdpError::UnsupportedAlgorithm(
            format!(
                "algorithm '{}' does not match the public_key_jwk key type ({jwk_algorithm})",
                req.algorithm
            ),
        )));
    }
    if !supported.iter().any(|a| a == jwk_algorithm) {
        return Err(RegistryError::Acdp(AcdpError::UnsupportedAlgorithm(
            format!("algorithm '{jwk_algorithm}' is not supported by this registry"),
        )));
    }
    Ok(())
}

/// §5.2 step 7: `[not_before, not_after)` from the registry's clock
/// `now_ms`, the requested length (default 600 s), the advertised
/// `max_ttl_seconds`, the 3600 s ceiling and the token's `exp` (Unix
/// seconds). A token that expires before the window can open is
/// `token_invalid`.
pub fn window(
    now_ms: i64,
    requested_ttl_seconds: Option<u32>,
    max_ttl_seconds: u32,
    token_exp: i64,
) -> Result<(DateTime<Utc>, DateTime<Utc>), KeylessRefusal> {
    let ttl = i64::from(
        requested_ttl_seconds
            .unwrap_or(DEFAULT_TTL_SECONDS)
            .min(max_ttl_seconds),
    )
    .min(MAX_WINDOW_SECONDS);
    let not_after_ms = now_ms
        .saturating_add(ttl.saturating_mul(1000))
        .min(token_exp.saturating_mul(1000));
    if not_after_ms <= now_ms {
        return Err(KeylessRefusal::TokenInvalid("expired"));
    }
    let at = |ms| {
        DateTime::<Utc>::from_timestamp_millis(ms)
            .ok_or(KeylessRefusal::TokenInvalid("timestamp out of range"))
    };
    Ok((at(now_ms)?, at(not_after_ms)?))
}

/// The §5.3 metadata for a request that passed steps 1 to 6. `algorithm`
/// is the JWK's, `entry` the controller's listing of the identity, and
/// `token_hash` the token's.
pub fn attestation(
    req: &KeyAttestationRequest,
    algorithm: &str,
    entry: &ListedIdentity,
    window: (DateTime<Utc>, DateTime<Utc>),
    token_hash: String,
) -> Result<KeyAttestation, AcdpError> {
    let key_fingerprint = req.public_key_jwk.fingerprint()?;
    Ok(KeyAttestation {
        attestation_version: ATTESTATION_VERSION.into(),
        controller: req.controller.clone(),
        identity: entry.identity.clone(),
        org_wide: entry.org_wide,
        key_id: keyless_key_id(&req.controller, &key_fingerprint)?,
        algorithm: algorithm.into(),
        public_key_jwk: req.public_key_jwk.clone(),
        key_fingerprint,
        not_before: window.0,
        not_after: window.1,
        token_hash,
    })
}

/// The publish request for `att`: a public `acdp:key-attestation` under
/// `did:web:<authority>`, signed by `key` as `key_id` (a key in the
/// registry DID document's `assertionMethod`, §5.3).
pub fn build(
    key: &SigningKey,
    key_id: &str,
    authority: &str,
    att: &KeyAttestation,
) -> Result<PublishRequest, AcdpError> {
    let metadata = serde_json::to_value(att)?;
    Producer::new(
        SigningKey::from_bytes(&key.seed_bytes()),
        AgentDid::new(acdp::did::authority_to_did_web(authority)),
        key_id,
    )
    .publish_request()
    .title(TITLE)
    .summary(summary(authority, &att.algorithm, &att.controller))
    .context_type(ContextType::Custom(KEY_ATTESTATION_TYPE.into()))
    .visibility(Visibility::Public)
    .metadata(metadata)
    .schema_uri(KEY_ATTESTATION_SCHEMA_URI)
    .acdp_version(ACDP_VERSION)
    .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp::types::keyless::{IdentityDescriptor, Jwk, WorkloadIdentity, REQUEST_VERSION};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    // kl-001 (spec 546b8c8), embedded; Phase 7 asserts these against the
    // fixture file itself.
    const REGISTRY: &str = "did:web:registry.example.com";
    const CONTROLLER: &str = "did:web:agents.example.com:ci";
    const ISSUER: &str = "https://token.actions.githubusercontent.com";
    const SUBJECT: &str = "repo:example/pipeline:ref:refs/heads/main";
    const V3_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.TEST-ONLY-NOT-A-REAL-TOKEN.kl-001";
    const V3_TOKEN_HASH: &str =
        "sha256:a375998360472ac02ef1181e21466c19ef2db143cb204c0bc120acc17d991009";
    const V3_POP_CANONICAL: &str = "{\"controller\":\"did:web:agents.example.com:ci\",\"key_fingerprint\":\"sha256:f7b7676c94df7e8fd9998e38f9fff04d8588b00d1801764e3f1877a3feae4477\",\"pop_version\":\"acdp-key-attestation-pop/1\",\"registry_did\":\"did:web:registry.example.com\",\"token_hash\":\"sha256:a375998360472ac02ef1181e21466c19ef2db143cb204c0bc120acc17d991009\"}";
    const V3_POP: &str =
        "wMkYJkQDDFPR4aWUtKkLNebbLaQxt+Qo0EYCYip81vBZVxCNcnoT2EVsOO2VaR9P9Wc4IJr4RG0dL9ImVJF8Ag==";
    const V1_CANONICAL: &str = "{\"acdp_version\":\"0.5.0\",\"agent_id\":\"did:web:registry.example.com\",\"contributors\":[],\"data_refs\":[],\"derived_from\":[],\"metadata\":{\"algorithm\":\"ed25519\",\"attestation_version\":\"acdp-key-attestation/1\",\"controller\":\"did:web:agents.example.com:ci\",\"identity\":{\"issuer\":\"https://token.actions.githubusercontent.com\",\"subject\":\"repo:example/pipeline:ref:refs/heads/main\",\"type\":\"oidc\"},\"key_fingerprint\":\"sha256:f7b7676c94df7e8fd9998e38f9fff04d8588b00d1801764e3f1877a3feae4477\",\"key_id\":\"did:web:agents.example.com:ci#acdp-kl-f7b7676c94df7e8fd9998e38f9fff04d8588b00d1801764e3f1877a3feae4477\",\"not_after\":\"2026-10-26T09:10:00.000Z\",\"not_before\":\"2026-10-26T09:00:00.000Z\",\"org_wide\":false,\"public_key_jwk\":{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\"NLTZBDFWy23PC-sKKUm3VZyUDSvLbb6MU6mzAnjjp0Y\"},\"token_hash\":\"sha256:a375998360472ac02ef1181e21466c19ef2db143cb204c0bc120acc17d991009\"},\"schema_uri\":\"https://schemas.acdp.io/v0.1.0/acdp-key-attestation.schema.json\",\"summary\":\"registry.example.com attests an ephemeral ed25519 key for did:web:agents.example.com:ci (RFC-ACDP-0021).\",\"supersedes\":null,\"title\":\"Key attestation\",\"type\":\"acdp:key-attestation\",\"version\":1,\"visibility\":\"public\"}";
    const V1_CONTENT_HASH: &str =
        "sha256:fbed72af712224f7d9d6e05a896d3a364c6c709247548a3de059198473399672";
    const V1_SIGNATURE: &str =
        "Ax85cwl90F9sULBDm3guc8Fuwqlg4B6eJmvdaNvx3HM089UwnNmx9eDo6Ic+b3xjUf8rfYfVbjKToRWeMTSDAg==";
    const V6_POP_P1363: &str =
        "GcnB3rNSbrTGEzqgolY2+p5CrrdDACDwZ+zY78nI/J5i3upzXXtjJyhsDI/lm2RXuak2ZfVEAnkSj8p7GLBvVQ==";
    const V6_POP_DER: &str = "MEQCIBnJwd6zUm60xhM6oKJWNvqeQq63QwAg8Gfs2O/JyPyeAiBi3upzXXtjJyhsDI/lm2RXuak2ZfVEAnkSj8p7GLBvVQ==";
    const V6_POP_CANONICAL: &str = "{\"controller\":\"did:web:agents.example.com:ci\",\"key_fingerprint\":\"sha256:97e9d1d2a642f9d771b3a301aaedc8c8b534dfcb7bff6813309b04d088525031\",\"pop_version\":\"acdp-key-attestation-pop/1\",\"registry_did\":\"did:web:registry.example.com\",\"token_hash\":\"sha256:a375998360472ac02ef1181e21466c19ef2db143cb204c0bc120acc17d991009\"}";

    fn ed_jwk() -> Jwk {
        Jwk::from_ed25519(&SigningKey::from_bytes(&[0x66; 32]).verifying_key_bytes())
    }

    fn p256_jwk() -> Jwk {
        Jwk {
            crv: "P-256".into(),
            kty: "EC".into(),
            x: "fPJ7GI0DT36KUjgDBLUaw8CJaeJ38hs1pgtI_EdmmXg".into(),
            y: Some("B3dVENuO0EApPZrGn3Qw27p9reY86YIpngS3nSJ4c9E".into()),
        }
    }

    fn request(algorithm: &str, jwk: Jwk, pop: &str) -> KeyAttestationRequest {
        KeyAttestationRequest {
            request_version: REQUEST_VERSION.into(),
            controller: CONTROLLER.into(),
            identity: IdentityDescriptor::oidc(ISSUER, SUBJECT, V3_TOKEN),
            algorithm: algorithm.into(),
            public_key_jwk: jwk,
            pop: pop.into(),
            requested_ttl_seconds: None,
        }
    }

    fn reason(e: &RegistryError) -> (&'static str, u16, Option<String>) {
        let wire = serde_json::to_value(acdp_registry_types::error::WireError::from(e)).unwrap();
        (
            e.wire_code(),
            e.http_status(),
            wire["error"]["details"]["reason"]
                .as_str()
                .map(str::to_owned),
        )
    }

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// kl-001 vector 1: under the rcpt-001 seed and the vector's window,
    /// the minted request is the golden body: canonical form, content hash
    /// and registry signature.
    #[test]
    fn build_reproduces_kl_001_vector_1() {
        let entry = ListedIdentity {
            identity: WorkloadIdentity::Oidc {
                issuer: ISSUER.into(),
                subject: SUBJECT.into(),
            },
            org_wide: false,
        };
        let req = request("ed25519", ed_jwk(), "AA==");
        let att = attestation(
            &req,
            "ed25519",
            &entry,
            (
                ts("2026-10-26T09:00:00.000Z"),
                ts("2026-10-26T09:10:00.000Z"),
            ),
            V3_TOKEN_HASH.into(),
        )
        .unwrap();
        let key = SigningKey::from_bytes(&[0x11; 32]);
        let publish = build(
            &key,
            "did:web:registry.example.com#receipt-key-1",
            "registry.example.com",
            &att,
        )
        .unwrap();
        assert_eq!(
            // `token_hash` is `sha256:` + hex SHA-256 of the bytes, the
            // content-hash format too.
            acdp::types::keyless::token_hash(V1_CANONICAL),
            V1_CONTENT_HASH,
            "the embedded canonical form is the vector's"
        );
        assert_eq!(publish.content_hash.as_str(), V1_CONTENT_HASH);
        assert_eq!(publish.signature.value, V1_SIGNATURE);
        assert_eq!(publish.signature.algorithm, "ed25519");
        assert_eq!(
            publish.signature.key_id,
            "did:web:registry.example.com#receipt-key-1"
        );
        assert_eq!(STANDARD.decode(V1_SIGNATURE).unwrap().len(), 64);
    }

    /// kl-001 vector 3: the token hash, the JCS preimage and the
    /// (deterministic) Ed25519 proof, which verifies.
    #[test]
    fn kl_001_vector_3_preimage_and_ed25519_pop() {
        let req = request("ed25519", ed_jwk(), V3_POP);
        assert_eq!(req.token_hash(), V3_TOKEN_HASH);
        assert_eq!(req.pop_preimage(REGISTRY).unwrap(), V3_POP_CANONICAL);
        let ours = SigningKey::from_bytes(&[0x66; 32]).sign_string(V3_POP_CANONICAL);
        assert_eq!(ours, V3_POP, "Ed25519 is deterministic");
        assert_eq!(verify_pop(&req, REGISTRY).unwrap(), "ed25519");
    }

    /// kl-001 vector 6 (R8): the P1363 `r||s` proof is accepted; the same
    /// signature DER-encoded is `invalid_signature`, `pop_invalid`.
    #[test]
    fn kl_001_vector_6_p1363_accepted_der_rejected() {
        let ok = request("ecdsa-p256", p256_jwk(), V6_POP_P1363);
        assert_eq!(ok.pop_preimage(REGISTRY).unwrap(), V6_POP_CANONICAL);
        assert_eq!(verify_pop(&ok, REGISTRY).unwrap(), "ecdsa-p256");

        let der = request("ecdsa-p256", p256_jwk(), V6_POP_DER);
        let e = verify_pop(&der, REGISTRY).unwrap_err();
        assert_eq!(
            reason(&e),
            ("invalid_signature", 400, Some("pop_invalid".into()))
        );
    }

    /// A good proof for another registry is `pop_invalid` here.
    #[test]
    fn a_pop_for_another_registry_is_pop_invalid() {
        let req = request("ed25519", ed_jwk(), V3_POP);
        let e = verify_pop(&req, "did:web:other.example.com").unwrap_err();
        assert_eq!(
            reason(&e),
            ("invalid_signature", 400, Some("pop_invalid".into()))
        );
    }

    /// Step 5 after step 4: a good proof under a P-256 key with
    /// `algorithm: ed25519` is `unsupported_algorithm`, with no reason.
    #[test]
    fn algorithm_mismatch_is_unsupported_algorithm_after_a_good_pop() {
        let req = request("ed25519", p256_jwk(), V6_POP_P1363);
        let alg = verify_pop(&req, REGISTRY).unwrap();
        let both = ["ed25519".to_string(), "ecdsa-p256".to_string()];
        let e = check_algorithm(&req, alg, &both).unwrap_err();
        assert_eq!(reason(&e), ("unsupported_algorithm", 400, None));
        let ok = request("ecdsa-p256", p256_jwk(), V6_POP_P1363);
        check_algorithm(&ok, alg, &both).unwrap();
        let e = check_algorithm(&ok, alg, &["ed25519".to_string()]).unwrap_err();
        assert_eq!(reason(&e), ("unsupported_algorithm", 400, None));
    }

    /// Step 7: the lesser of requested, advertised maximum, 3600 and the
    /// token's remaining life; an exhausted token is `token_invalid`.
    #[test]
    fn the_window_is_bounded_by_every_limit() {
        let now = 1_800_000_000_000_i64;
        let exp = now / 1000 + 10_000;
        let len = |w: (DateTime<Utc>, DateTime<Utc>)| {
            assert_eq!(w.0.timestamp_millis(), now);
            (w.1 - w.0).num_seconds()
        };
        assert_eq!(len(window(now, None, 3600, exp).unwrap()), 600);
        assert_eq!(len(window(now, Some(120), 3600, exp).unwrap()), 120);
        assert_eq!(len(window(now, Some(3600), 900, exp).unwrap()), 900);
        assert_eq!(len(window(now, Some(4000), 9000, exp).unwrap()), 3600);
        assert_eq!(len(window(now, None, 3600, now / 1000 + 30).unwrap()), 30);
        assert_eq!(
            window(now, None, 3600, now / 1000),
            Err(KeylessRefusal::TokenInvalid("expired"))
        );
    }
}
