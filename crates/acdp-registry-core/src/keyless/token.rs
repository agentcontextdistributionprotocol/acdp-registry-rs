//! RFC-ACDP-0021 §5.2 steps 2 and 3: is the workload token one this
//! registry accepts, for this identity, this registry and this key?
//!
//! The order is §5.2's and stops at the first failure:
//!
//! 1. The descriptor's schema rules again (an X509-SVID is a
//!    `schema_violation`), so the function is safe on its own.
//! 2. **Issuer acceptance**, before any fetch: an OIDC `issuer` listed in
//!    config byte for byte, or the SPIFFE subject's trust domain listed.
//!    `issuer_not_accepted`.
//! 3. **Token** (`token_invalid`): a JOSE header with an allow-listed `alg`
//!    (checked before any fetch; `none`, `HS*` and any `crit` are refused),
//!    a key selected by `kid` from the source's published keys whose type
//!    fits the `alg` (and whose own `alg`, when it has one, is the header's),
//!    the signature, then `exp` strictly in the future with no leeway,
//!    `iat` and `nbf` (when present) at most 60 s ahead, `iss` and `sub`
//!    byte for byte. Keys that cannot be fetched: `issuer_unreachable`.
//!    - **Audience**: `aud`, a string or an array, contains `registry_did`
//!      byte for byte. `audience_mismatch`.
//!    - **Confirmation**: a `cnf.jkt`, when present, is the RFC 7638
//!      thumbprint of `public_key_jwk`. `confirmation_mismatch`.
//!
//! Single use (`token_replayed`) is the caller's, against the store.
//!
//! The per-kind rules beyond RFC-ACDP-0021's restatement are
//! RFC-AITP-0002's: for a SPIFFE JWT-SVID (§6.3) the header holds only
//! `alg`, `kid` and `typ`; `kid` is required; `typ`, when present, is `JWT`
//! or `JOSE`; `alg` is an RSA or ECDSA algorithm (EdDSA is refused); and
//! only bundle keys with `use: "jwt-svid"` are candidates (§6.2). Where
//! AITP and RFC-ACDP-0021 disagree (`aud` cardinality, `iat` bounds,
//! required `nonce` and `cnf.jkt`), RFC-ACDP-0021 is followed: AITP's
//! rules are written for a peer handshake, which this request is not.
//!
//! jsonwebtoken checks the signature only; every claim rule above is
//! checked here, so each failure carries its own reason.

use acdp::types::keyless::{spiffe_trust_domain, token_hash, IdentityDescriptor, Jwk};
use acdp_registry_types::config::KeylessConfig;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::{Map, Value};

use super::jwks::{IssuerJwk, JwksCache, KeySet, KeySource};
use super::KeylessRefusal;

/// `iat` and `nbf` may be this far ahead of the registry's clock (§5.2).
pub const CLOCK_SKEW_SECONDS: i64 = 60;

/// RFC 7518 §3.3: an RSA key for RS*/PS* is at least 2048 bits.
const MIN_RSA_MODULUS_BYTES: usize = 256;

/// `alg` values an OIDC ID token may use (plan D9).
const OIDC_ALGS: &[&str] = &["RS256", "PS256", "ES256", "ES384", "EdDSA"];

/// `alg` values a JWT-SVID may use: RFC-AITP-0002 §6.3 (JWT-SVID §2.1),
/// less `ES512`, which the verifier has no implementation of and so
/// refuses like any other unlisted value.
const SPIFFE_ALGS: &[&str] = &[
    "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384",
];

/// A token that passed §5.2 step 3 (all but single use).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// The token's `exp`, Unix seconds: the attestation window may not
    /// outlast it (§5.2 step 7).
    pub exp: i64,
    /// `sha256:<hex>` of the token, the only trace kept of it.
    pub token_hash: String,
}

/// What kind of token, with its per-kind rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Oidc,
    Spiffe,
}

/// Verify `identity`'s token per §5.2 steps 1 (descriptor rules) to 3
/// (less single use) at `now` (Unix seconds), against `cfg`'s accepted
/// issuers and the keys `cache` holds for them. `registry_did` is this
/// registry's own DID; `jwk` is the request's `public_key_jwk`.
pub async fn verify(
    cfg: &KeylessConfig,
    cache: &JwksCache,
    identity: &IdentityDescriptor,
    registry_did: &str,
    jwk: &Jwk,
    now: i64,
) -> Result<VerifiedToken, KeylessRefusal> {
    identity
        .validate()
        .map_err(|e| KeylessRefusal::SchemaViolation(e.to_string()))?;

    // Step 2: issuer acceptance. No fetch has happened yet.
    let (kind, source, expected_iss, subject) = match identity {
        IdentityDescriptor::Oidc {
            issuer, subject, ..
        } => {
            let o = cfg
                .oidc_issuers
                .iter()
                .find(|o| o.issuer == *issuer)
                .ok_or(KeylessRefusal::IssuerNotAccepted)?;
            (
                Kind::Oidc,
                KeySource::Oidc(o.issuer.clone()),
                Some(o.issuer.as_str()),
                subject.as_str(),
            )
        }
        IdentityDescriptor::Spiffe { subject, .. } => {
            let domain = spiffe_trust_domain(subject).ok_or_else(|| {
                KeylessRefusal::SchemaViolation("identity.subject is not a SPIFFE ID".into())
            })?;
            let d = cfg
                .spiffe_trust_domains
                .iter()
                .find(|d| d.trust_domain == domain)
                .ok_or(KeylessRefusal::IssuerNotAccepted)?;
            (
                Kind::Spiffe,
                KeySource::Spiffe(d.trust_domain.clone()),
                d.jwt_issuer.as_deref(),
                subject.as_str(),
            )
        }
    };
    let token = identity.token();

    // Step 3, header: refused before any key is fetched.
    let header = Header::parse(token, kind)?;

    // Step 3, key: by kid, with one forced refetch for an unknown kid.
    let keys = cache
        .keys(&source)
        .await
        .map_err(|_| KeylessRefusal::IssuerUnreachable)?;
    let key = match select_key(&keys, &header, kind) {
        Selected::One(k) => k,
        Selected::Ambiguous => {
            return Err(KeylessRefusal::TokenInvalid(
                "kid matches more than one key",
            ))
        }
        Selected::None if header.kid.is_some() => {
            let refreshed = cache
                .refresh_for_unknown_kid(&source)
                .await
                .map_err(|_| KeylessRefusal::IssuerUnreachable)?
                .ok_or(KeylessRefusal::TokenInvalid("unknown kid"))?;
            match select_key(&refreshed, &header, kind) {
                Selected::One(k) => k,
                Selected::Ambiguous => {
                    return Err(KeylessRefusal::TokenInvalid(
                        "kid matches more than one key",
                    ))
                }
                Selected::None => return Err(KeylessRefusal::TokenInvalid("unknown kid")),
            }
        }
        Selected::None => {
            return Err(KeylessRefusal::TokenInvalid(
                "no kid and not exactly one usable key",
            ))
        }
    };
    let decoding = decoding_key(&key, &header)?;

    // Step 3, signature. Every claim is checked by hand below.
    let mut validation = Validation::new(header.alg);
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.leeway = 0;
    validation.required_spec_claims.clear();
    let claims = jsonwebtoken::decode::<Map<String, Value>>(token, &decoding, &validation)
        .map_err(|_| KeylessRefusal::TokenInvalid("signature does not verify"))?
        .claims;

    // Step 3, claims.
    let exp = numeric(claims.get("exp")).ok_or(KeylessRefusal::TokenInvalid("exp missing"))?;
    if exp <= now {
        return Err(KeylessRefusal::TokenInvalid("expired"));
    }
    for (name, why) in [("iat", "iat in the future"), ("nbf", "nbf in the future")] {
        match claims.get(name) {
            None => {}
            Some(v) => match numeric(Some(v)) {
                Some(t) if t <= now.saturating_add(CLOCK_SKEW_SECONDS) => {}
                Some(_) => return Err(KeylessRefusal::TokenInvalid(why)),
                None => {
                    return Err(KeylessRefusal::TokenInvalid(
                        "iat or nbf is not a NumericDate",
                    ))
                }
            },
        }
    }
    if let Some(iss) = expected_iss {
        if claims.get("iss").and_then(Value::as_str) != Some(iss) {
            return Err(KeylessRefusal::TokenInvalid("iss mismatch"));
        }
    }
    if claims.get("sub").and_then(Value::as_str) != Some(subject) {
        return Err(KeylessRefusal::TokenInvalid("sub mismatch"));
    }

    // Step 3, audience.
    let aud_ok = match claims.get("aud") {
        Some(Value::String(a)) => a == registry_did,
        Some(Value::Array(a)) => a.iter().any(|v| v.as_str() == Some(registry_did)),
        _ => false,
    };
    if !aud_ok {
        return Err(KeylessRefusal::AudienceMismatch);
    }

    // Step 3, confirmation: only when the token carries one.
    match claims.get("cnf") {
        None => {}
        Some(Value::Object(cnf)) => match cnf.get("jkt") {
            None => {}
            Some(Value::String(jkt)) => {
                let thumb = jwk
                    .thumbprint()
                    .map_err(|e| KeylessRefusal::SchemaViolation(e.to_string()))?;
                if *jkt != thumb {
                    return Err(KeylessRefusal::ConfirmationMismatch);
                }
            }
            Some(_) => return Err(KeylessRefusal::ConfirmationMismatch),
        },
        Some(_) => return Err(KeylessRefusal::ConfirmationMismatch),
    }

    Ok(VerifiedToken {
        exp,
        token_hash: token_hash(token),
    })
}

/// The JOSE header members this verifier reads, after its rules.
struct Header {
    alg: Algorithm,
    alg_name: &'static str,
    kid: Option<String>,
}

impl Header {
    fn parse(token: &str, kind: Kind) -> Result<Self, KeylessRefusal> {
        let bad = KeylessRefusal::TokenInvalid("malformed JOSE header");
        let b64 = token.split('.').next().ok_or(bad.clone())?;
        let raw = URL_SAFE_NO_PAD.decode(b64).map_err(|_| bad.clone())?;
        let h: Map<String, Value> = serde_json::from_slice(&raw).map_err(|_| bad.clone())?;
        if h.contains_key("crit") {
            return Err(KeylessRefusal::TokenInvalid("crit header not understood"));
        }
        let alg = h.get("alg").and_then(Value::as_str).ok_or(bad.clone())?;
        let allowed = match kind {
            Kind::Oidc => OIDC_ALGS,
            Kind::Spiffe => SPIFFE_ALGS,
        };
        let alg_name = allowed
            .iter()
            .copied()
            .find(|a| *a == alg)
            .ok_or(KeylessRefusal::TokenInvalid("alg not accepted"))?;
        let kid = match h.get("kid") {
            None => None,
            Some(Value::String(k)) => Some(k.clone()),
            Some(_) => return Err(bad),
        };
        if kind == Kind::Spiffe {
            // RFC-AITP-0002 §6.3 (JWT-SVID §2).
            if h.keys()
                .any(|k| !matches!(k.as_str(), "alg" | "kid" | "typ"))
            {
                return Err(KeylessRefusal::TokenInvalid(
                    "JWT-SVID header holds more than alg, kid and typ",
                ));
            }
            if kid.is_none() {
                return Err(KeylessRefusal::TokenInvalid("JWT-SVID has no kid"));
            }
            match h.get("typ") {
                None => {}
                Some(Value::String(t)) if t == "JWT" || t == "JOSE" => {}
                Some(_) => {
                    return Err(KeylessRefusal::TokenInvalid("JWT-SVID typ not JWT or JOSE"))
                }
            }
        }
        Ok(Self {
            alg: algorithm(alg_name),
            alg_name,
            kid,
        })
    }
}

/// An allow-listed `alg` name to its jsonwebtoken value. Only names in
/// [`OIDC_ALGS`] or [`SPIFFE_ALGS`] reach here.
fn algorithm(name: &str) -> Algorithm {
    match name {
        "RS256" => Algorithm::RS256,
        "RS384" => Algorithm::RS384,
        "RS512" => Algorithm::RS512,
        "PS256" => Algorithm::PS256,
        "PS384" => Algorithm::PS384,
        "PS512" => Algorithm::PS512,
        "ES256" => Algorithm::ES256,
        "ES384" => Algorithm::ES384,
        _ => Algorithm::EdDSA,
    }
}

enum Selected {
    One(Box<IssuerJwk>),
    None,
    Ambiguous,
}

/// The candidate keys for `kind` (OIDC: a signing key, `use` absent or
/// `sig`; SPIFFE: `use: "jwt-svid"`, RFC-AITP-0002 §6.2), then the one the
/// header's `kid` names, or the only one when there is no `kid`.
fn select_key(keys: &KeySet, header: &Header, kind: Kind) -> Selected {
    let candidates: Vec<&IssuerJwk> = keys
        .keys
        .iter()
        .filter(|k| k.kty != "oct")
        .filter(|k| match kind {
            Kind::Oidc => k.use_.as_deref().is_none_or(|u| u == "sig"),
            Kind::Spiffe => k.use_.as_deref() == Some("jwt-svid"),
        })
        .collect();
    let matching: Vec<&IssuerJwk> = match &header.kid {
        Some(kid) => candidates
            .into_iter()
            .filter(|k| k.kid.as_deref() == Some(kid.as_str()))
            .collect(),
        None => candidates,
    };
    match matching.as_slice() {
        [] => Selected::None,
        [k] => Selected::One(Box::new((*k).clone())),
        _ if header.kid.is_some() => Selected::Ambiguous,
        _ => Selected::None,
    }
}

/// The verification key, if `key` fits the header's `alg` (RFC 7518 §3):
/// `RSA` for RS*/PS*, `EC` P-256 for ES256, `EC` P-384 for ES384, `OKP`
/// Ed25519 for EdDSA; and a JWK `alg`, when present, equal to the header's.
fn decoding_key(key: &IssuerJwk, header: &Header) -> Result<DecodingKey, KeylessRefusal> {
    let misfit = KeylessRefusal::TokenInvalid("key does not fit alg");
    if key.alg.as_deref().is_some_and(|a| a != header.alg_name) {
        return Err(misfit);
    }
    let crv = key.crv.as_deref();
    let got = match (header.alg_name, key.kty.as_str()) {
        ("RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512", "RSA") => {
            let (n, e) = key
                .n
                .as_deref()
                .zip(key.e.as_deref())
                .ok_or(misfit.clone())?;
            let modulus = URL_SAFE_NO_PAD.decode(n).map_err(|_| misfit.clone())?;
            let significant = modulus.iter().skip_while(|b| **b == 0).count();
            if significant < MIN_RSA_MODULUS_BYTES {
                return Err(KeylessRefusal::TokenInvalid(
                    "RSA key shorter than 2048 bits",
                ));
            }
            DecodingKey::from_rsa_components(n, e)
        }
        ("ES256", "EC") if crv == Some("P-256") => ec_key(key)?,
        ("ES384", "EC") if crv == Some("P-384") => ec_key(key)?,
        ("EdDSA", "OKP") if crv == Some("Ed25519") => {
            DecodingKey::from_ed_components(key.x.as_deref().ok_or(misfit.clone())?)
        }
        _ => return Err(misfit),
    };
    got.map_err(|_| misfit)
}

fn ec_key(key: &IssuerJwk) -> Result<jsonwebtoken::errors::Result<DecodingKey>, KeylessRefusal> {
    let (x, y) = key
        .x
        .as_deref()
        .zip(key.y.as_deref())
        .ok_or(KeylessRefusal::TokenInvalid("key does not fit alg"))?;
    Ok(DecodingKey::from_ec_components(x, y))
}

/// A NumericDate (RFC 7519 §2) as whole seconds, rounded down.
fn numeric(v: Option<&Value>) -> Option<i64> {
    let n = v?.as_number()?;
    n.as_i64().or_else(|| {
        n.as_f64()
            .filter(|f| f.is_finite())
            // Saturating: an absurd date is still a date, compared as such.
            .map(|f| f.floor() as i64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_with_header(h: Value) -> String {
        format!(
            "{}.e30.c2ln",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&h).unwrap())
        )
    }

    fn refused(h: Value, kind: Kind) -> Option<&'static str> {
        match Header::parse(&token_with_header(h), kind) {
            Ok(_) => None,
            Err(KeylessRefusal::TokenInvalid(why)) => Some(why),
            Err(other) => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn header_rules_refuse_none_hmac_crit_and_kind_mismatches() {
        for alg in ["none", "HS256", "HS512", "ES512", "rs256", ""] {
            assert_eq!(
                refused(serde_json::json!({"alg": alg}), Kind::Oidc),
                Some("alg not accepted"),
                "{alg}"
            );
        }
        assert_eq!(
            refused(
                serde_json::json!({"alg": "EdDSA", "kid": "k"}),
                Kind::Spiffe
            ),
            Some("alg not accepted")
        );
        assert_eq!(
            refused(serde_json::json!({"alg": "RS512"}), Kind::Oidc),
            Some("alg not accepted")
        );
        assert_eq!(
            refused(
                serde_json::json!({"alg": "ES256", "crit": ["b64"]}),
                Kind::Oidc
            ),
            Some("crit header not understood")
        );
        assert_eq!(
            refused(serde_json::json!({"alg": "ES256"}), Kind::Spiffe),
            Some("JWT-SVID has no kid")
        );
        assert_eq!(
            refused(
                serde_json::json!({"alg": "ES256", "kid": "k", "jku": "https://x"}),
                Kind::Spiffe
            ),
            Some("JWT-SVID header holds more than alg, kid and typ")
        );
        assert_eq!(
            refused(
                serde_json::json!({"alg": "ES256", "kid": "k", "typ": "at+jwt"}),
                Kind::Spiffe
            ),
            Some("JWT-SVID typ not JWT or JOSE")
        );
        assert_eq!(
            refused(serde_json::json!({"alg": "ES256", "kid": 7}), Kind::Oidc),
            Some("malformed JOSE header")
        );
        assert_eq!(
            refused(
                serde_json::json!({"alg": "ES256", "kid": "k", "typ": "JOSE"}),
                Kind::Spiffe
            ),
            None
        );
        assert_eq!(
            refused(
                serde_json::json!({"alg": "EdDSA", "typ": "JWT", "x5u": "y"}),
                Kind::Oidc
            ),
            None
        );
        assert!(matches!(
            Header::parse("!!.e30.c2ln", Kind::Oidc),
            Err(KeylessRefusal::TokenInvalid("malformed JOSE header"))
        ));
    }

    fn jwk(kty: &str, crv: Option<&str>, alg: Option<&str>) -> IssuerJwk {
        IssuerJwk {
            kty: kty.into(),
            kid: Some("k".into()),
            alg: alg.map(Into::into),
            use_: None,
            crv: crv.map(Into::into),
            n: Some(URL_SAFE_NO_PAD.encode([0xc5u8; 256])),
            e: Some("AQAB".into()),
            x: Some(URL_SAFE_NO_PAD.encode([1u8; 32])),
            y: Some(URL_SAFE_NO_PAD.encode([2u8; 32])),
        }
    }

    fn header(alg: &'static str) -> Header {
        Header {
            alg: algorithm(alg),
            alg_name: alg,
            kid: Some("k".into()),
        }
    }

    /// alg confusion: a header alg the key's type does not fit is refused
    /// before any verification, and so is a JWK `alg` that disagrees.
    #[test]
    fn a_key_must_fit_the_header_alg() {
        let misfit = Err(KeylessRefusal::TokenInvalid("key does not fit alg"));
        let check = |k: IssuerJwk, alg| decoding_key(&k, &header(alg)).map(|_| ());
        assert_eq!(check(jwk("RSA", None, None), "ES256"), misfit);
        assert_eq!(check(jwk("EC", Some("P-256"), None), "ES384"), misfit);
        assert_eq!(check(jwk("EC", Some("P-384"), None), "ES256"), misfit);
        assert_eq!(check(jwk("OKP", Some("Ed25519"), None), "ES256"), misfit);
        assert_eq!(check(jwk("EC", Some("P-256"), None), "EdDSA"), misfit);
        assert_eq!(check(jwk("RSA", None, Some("RS256")), "PS256"), misfit);
        assert_eq!(check(jwk("RSA", None, Some("RS256")), "RS256"), Ok(()));
        assert_eq!(
            check(jwk("OKP", Some("Ed25519"), Some("EdDSA")), "EdDSA"),
            Ok(())
        );
        let mut short = jwk("RSA", None, None);
        short.n = Some(URL_SAFE_NO_PAD.encode([0xc5u8; 128]));
        assert_eq!(
            check(short, "RS256"),
            Err(KeylessRefusal::TokenInvalid(
                "RSA key shorter than 2048 bits"
            ))
        );
    }

    #[test]
    fn key_selection_honours_kid_use_and_kind() {
        let mut sig = jwk("EC", Some("P-256"), None);
        sig.use_ = Some("sig".into());
        let mut svid = jwk("EC", Some("P-256"), None);
        svid.use_ = Some("jwt-svid".into());
        let mut oct = jwk("oct", None, None);
        oct.kid = Some("k".into());
        let set = |ks: Vec<IssuerJwk>| KeySet { keys: ks };
        let one = |s: Selected| matches!(s, Selected::One(_));
        let h = header("ES256");
        assert!(one(select_key(
            &set(vec![sig.clone(), oct.clone()]),
            &h,
            Kind::Oidc
        )));
        // SPIFFE takes only `use: jwt-svid` keys (AITP §6.2): `sig` and absent are ignored.
        assert!(matches!(
            select_key(&set(vec![sig.clone()]), &h, Kind::Spiffe),
            Selected::None
        ));
        assert!(matches!(
            select_key(&set(vec![jwk("EC", Some("P-256"), None)]), &h, Kind::Spiffe),
            Selected::None
        ));
        assert!(one(select_key(&set(vec![svid.clone()]), &h, Kind::Spiffe)));
        // OIDC ignores a jwt-svid key.
        assert!(matches!(
            select_key(&set(vec![svid]), &h, Kind::Oidc),
            Selected::None
        ));
        assert!(matches!(
            select_key(&set(vec![sig.clone(), sig.clone()]), &h, Kind::Oidc),
            Selected::Ambiguous
        ));
        let no_kid = Header {
            kid: None,
            ..header("ES256")
        };
        assert!(one(select_key(
            &set(vec![sig.clone()]),
            &no_kid,
            Kind::Oidc
        )));
        assert!(matches!(
            select_key(&set(vec![sig.clone(), sig]), &no_kid, Kind::Oidc),
            Selected::None
        ));
    }

    #[test]
    fn numeric_dates_round_down_and_refuse_non_numbers() {
        assert_eq!(numeric(Some(&serde_json::json!(100))), Some(100));
        assert_eq!(numeric(Some(&serde_json::json!(100.9))), Some(100));
        assert_eq!(numeric(Some(&serde_json::json!("100"))), None);
        assert_eq!(numeric(None), None);
    }
}
