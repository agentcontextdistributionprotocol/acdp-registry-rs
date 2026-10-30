//! RFC-ACDP-0021 keyless signing (`acdp-registry-keyless`), registry side.
//!
//! - [`token`]: §5.2 steps 2 and 3, verifying the workload token (an OIDC
//!   ID token per RFC-AITP-0002 §2.3, or a SPIFFE JWT-SVID per §6.3/§6.5)
//!   against the issuer keys [`jwks`] fetches and caches.
//! - [`jwks`]: the in-process cache of each configured issuer's published
//!   keys, fetched through [`crate::outbound`].
//! - [`mint`]: §5.2 steps 4, 5 and 7 and the §5.3 body, pure.
//! - [`KeylessState`]: what `POST /key-attestations` holds beyond the
//!   cache: the attestation signing key, the clock and the two limiters.
//!   Present only when `keyless.enabled`; without it both
//!   `/key-attestations` methods answer 501. Startup still refuses
//!   `enabled = true`, so only in-process harnesses build one.

pub mod jwks;
pub mod mint;
pub mod token;

pub use jwks::{JwksCache, KeySource, Outbound};
pub use token::{verify, VerifiedToken};

use acdp::error::AcdpError;
use acdp_registry_types::RegistryError;

/// A clock in Unix milliseconds. Injected so the cache's TTL, stale window
/// and refetch interval are tested without waiting.
pub type Clock = std::sync::Arc<dyn Fn() -> i64 + Send + Sync>;

/// The wall clock.
pub fn system_clock() -> Clock {
    std::sync::Arc::new(|| chrono::Utc::now().timestamp_millis())
}

/// `POST /key-attestations` state (RFC-ACDP-0021 §5), built from config
/// when `keyless.enabled`.
pub struct KeylessState {
    /// The key attestations are signed with: the receipt key, which
    /// `build_did_document` lists in `assertionMethod` (§5.3).
    signing_key: acdp::crypto::SigningKey,
    /// `did:web:<authority>#<receipt.key_id_fragment>`.
    pub key_id: String,
    /// Standard base64 of the signing key's public half, the pinned key
    /// the mint is proven against.
    pub public_key_b64: String,
    /// `did:web:<authority>`, the `aud` tokens must name.
    pub registry_did: String,
    /// Sets `not_before` (§5.2 step 7) and the token checks' `now`. Store
    /// `created_at` stays on the real clock.
    pub clock: Clock,
    /// Per client IP, charged on arrival. `None` when the budget is 0.
    pub ip_limiter: Option<std::sync::Arc<crate::rate_limit::AgentRateLimiter>>,
    /// Per workload identity, charged only once a token verified for it
    /// (§5.1, plan D17). `None` when the budget is 0.
    pub identity_limiter: Option<std::sync::Arc<crate::rate_limit::AgentRateLimiter>>,
}

impl KeylessState {
    /// The state for `cfg`, or `None` while `keyless.enabled` is false.
    /// A receipt key that does not load is an error: the binary's startup
    /// validation refuses keyless without one.
    pub fn from_config(cfg: &acdp_registry_types::RegistryConfig) -> Result<Option<Self>, String> {
        if !cfg.keyless.enabled {
            return Ok(None);
        }
        let signer = crate::receipt::build_signer(&cfg.receipt, &cfg.registry.authority)?;
        let signing_key = crate::receipt::load_signing_key(&cfg.receipt)?;
        use base64::Engine as _;
        let limiter = |n: u32| {
            (n > 0).then(|| std::sync::Arc::new(crate::rate_limit::AgentRateLimiter::new(n)))
        };
        Ok(Some(Self {
            public_key_b64: base64::engine::general_purpose::STANDARD
                .encode(signing_key.verifying_key_bytes()),
            signing_key,
            key_id: signer.key_id().to_string(),
            registry_did: acdp::did::authority_to_did_web(&cfg.registry.authority),
            clock: system_clock(),
            ip_limiter: limiter(cfg.keyless.rate_per_ip_per_minute),
            identity_limiter: limiter(cfg.keyless.rate_per_identity_per_minute),
        }))
    }

    /// The attestation signing key.
    pub fn signing_key(&self) -> &acdp::crypto::SigningKey {
        &self.signing_key
    }

    /// Replace the clock (tests).
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }
}

/// The `details.reason` tokens this module emits (`registries/error-codes.md`,
/// "Keyless-signing reason tokens").
pub mod reason {
    /// §5.2 step 2.
    pub const ISSUER_NOT_ACCEPTED: &str = "issuer_not_accepted";
    /// §5.2 step 3: signature, expiry, `iat`/`nbf`, `iss` or `sub`.
    pub const TOKEN_INVALID: &str = "token_invalid";
    /// §5.2 step 3: the issuer's keys cannot be fetched (transient).
    pub const ISSUER_UNREACHABLE: &str = "issuer_unreachable";
    /// §5.2 step 3, audience.
    pub const AUDIENCE_MISMATCH: &str = "audience_mismatch";
    /// §5.2 step 3, confirmation.
    pub const CONFIRMATION_MISMATCH: &str = "confirmation_mismatch";
    /// §5.2 step 3, single use.
    pub const TOKEN_REPLAYED: &str = "token_replayed";
    /// §5.2 step 6.
    pub const IDENTITY_NOT_AUTHORIZED: &str = "identity_not_authorized";
    /// §6.3 "Lifecycle events" (R13): a keyless key id signed an
    /// RFC-ACDP-0013 event. The SDK has no `KeylessReason` for it.
    pub const KEYLESS_EVENT_UNSUPPORTED: &str = "keyless_event_unsupported";
}

/// `e` with its keyless `details.reason`, when its message carries one
/// (an SDK or store refusal built with `KeylessReason::error`); `e` as is
/// otherwise.
pub fn with_keyless_reason(e: AcdpError) -> RegistryError {
    match acdp::types::keyless::KeylessReason::of(&e) {
        Some(r) if r.is_registry_token() => RegistryError::with_reason(e, r.as_str()),
        _ => RegistryError::Acdp(e),
    }
}

/// Why a §5.2 step-1 to step-3 check refused, each with its wire answer
/// ([`Self::into_error`]). The `&'static str` details name the failed
/// rule for `error.message`; none carries any part of the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeylessRefusal {
    /// The descriptor breaks the request schema (an X509-SVID, a malformed
    /// subject or token): `schema_violation` (400), no reason.
    SchemaViolation(String),
    /// `key_not_authorized` (403), `issuer_not_accepted`.
    IssuerNotAccepted,
    /// `key_not_authorized` (403), `token_invalid`.
    TokenInvalid(&'static str),
    /// `key_resolution_unreachable` (502), `issuer_unreachable`.
    IssuerUnreachable,
    /// `key_not_authorized` (403), `audience_mismatch`.
    AudienceMismatch,
    /// `key_not_authorized` (403), `confirmation_mismatch`.
    ConfirmationMismatch,
}

impl KeylessRefusal {
    /// The `details.reason` token, if the refusal carries one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::SchemaViolation(_) => None,
            Self::IssuerNotAccepted => Some(reason::ISSUER_NOT_ACCEPTED),
            Self::TokenInvalid(_) => Some(reason::TOKEN_INVALID),
            Self::IssuerUnreachable => Some(reason::ISSUER_UNREACHABLE),
            Self::AudienceMismatch => Some(reason::AUDIENCE_MISMATCH),
            Self::ConfirmationMismatch => Some(reason::CONFIRMATION_MISMATCH),
        }
    }

    /// The wire answer (RFC-ACDP-0021 §5.2, `registries/error-codes.md`).
    pub fn into_error(self) -> RegistryError {
        let reason = self.reason();
        let inner = match self {
            Self::SchemaViolation(m) => return RegistryError::Acdp(AcdpError::SchemaViolation(m)),
            Self::IssuerNotAccepted => AcdpError::KeyNotAuthorized(
                "the workload token's issuer is not accepted by this registry".into(),
            ),
            Self::TokenInvalid(why) => {
                AcdpError::KeyNotAuthorized(format!("the workload token is invalid: {why}"))
            }
            Self::IssuerUnreachable => AcdpError::KeyResolutionUnreachable(
                "the issuer's published keys could not be fetched; retry later".into(),
            ),
            Self::AudienceMismatch => AcdpError::KeyNotAuthorized(
                "the workload token's aud does not contain this registry's DID".into(),
            ),
            Self::ConfirmationMismatch => AcdpError::KeyNotAuthorized(
                "the workload token's cnf.jkt is not the thumbprint of public_key_jwk".into(),
            ),
        };
        RegistryError::with_reason(
            inner,
            reason.expect("every non-schema refusal has a reason"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp::types::keyless::KeylessReason;

    /// Each refusal answers with the code, status and reason
    /// `registries/error-codes.md` registers for it.
    #[test]
    fn every_refusal_has_its_registered_wire_answer() {
        for (r, code, status, reason) in [
            (
                KeylessRefusal::IssuerNotAccepted,
                "key_not_authorized",
                403,
                Some("issuer_not_accepted"),
            ),
            (
                KeylessRefusal::TokenInvalid("x"),
                "key_not_authorized",
                403,
                Some("token_invalid"),
            ),
            (
                KeylessRefusal::IssuerUnreachable,
                "key_resolution_unreachable",
                502,
                Some("issuer_unreachable"),
            ),
            (
                KeylessRefusal::AudienceMismatch,
                "key_not_authorized",
                403,
                Some("audience_mismatch"),
            ),
            (
                KeylessRefusal::ConfirmationMismatch,
                "key_not_authorized",
                403,
                Some("confirmation_mismatch"),
            ),
            (
                KeylessRefusal::SchemaViolation("x509".into()),
                "schema_violation",
                400,
                None,
            ),
        ] {
            assert_eq!(r.reason(), reason);
            let e = r.into_error();
            assert_eq!(
                (e.wire_code(), e.http_status()),
                (code, status),
                "{reason:?}"
            );
            let wire =
                serde_json::to_value(acdp_registry_types::error::WireError::from(&e)).unwrap();
            assert_eq!(
                wire["error"].get("details"),
                reason.map(|r| serde_json::json!({ "reason": r })).as_ref()
            );
        }
    }

    /// The tokens the SDK also names are spelled as the SDK spells them.
    #[test]
    fn reason_tokens_match_the_sdk_spelling() {
        for (ours, sdk) in [
            (
                reason::ISSUER_NOT_ACCEPTED,
                KeylessReason::IssuerNotAccepted,
            ),
            (reason::TOKEN_INVALID, KeylessReason::TokenInvalid),
            (reason::AUDIENCE_MISMATCH, KeylessReason::AudienceMismatch),
            (
                reason::CONFIRMATION_MISMATCH,
                KeylessReason::ConfirmationMismatch,
            ),
        ] {
            assert_eq!(serde_json::to_value(sdk).unwrap(), serde_json::json!(ours));
        }
    }
}
