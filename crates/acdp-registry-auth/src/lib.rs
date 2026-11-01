//! DID challenge-response auth + JWT bearer tokens for `acdp-registry-rs`.
//!
//! See module docs for details.

pub mod challenge_store;
pub mod jwt;
pub mod read_token;
pub mod revocation_poller;
pub mod revocation_store;
pub mod service;

pub use challenge_store::{
    ChallengeRecord, ChallengeStore, InMemoryChallengeStore, PgChallengeStore, SqliteChallengeStore,
};
pub use jwt::{JwtSecret, JwtSigner};
pub use read_token::{
    generate_read_token, is_read_token, read_token_sha256_hex, ReadTokenError, ReadTokens,
    StaticReadPrincipal, READ_TOKEN_PREFIX,
};
pub use revocation_store::{
    tombstone_cutoff, InMemoryRevocationStore, PgRevocationStore, RevocationRecord,
    RevocationStore, SqliteRevocationStore,
};
pub use service::{extract_bearer, AuthService};

use acdp_registry_types::RegistryError;
use thiserror::Error;

/// Auth-layer errors. Bubble up via [`RegistryError`] when surfaced to a handler.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("config error: {0}")]
    Config(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("challenge unknown or already consumed: {0}")]
    ChallengeUnknown(String),

    #[error("challenge expired")]
    ChallengeExpired,

    #[error("challenge replay: {0}")]
    ChallengeReplay(String),

    #[error("DID resolution failed: {0}")]
    Resolution(String),

    /// Transient DID resolution failure (RFC-ACDP-0001 §5.11 step 3): the
    /// DID host was unreachable (non-2xx, timeout, TLS failure), or the key
    /// was absent from a stale cached copy whose forced re-fetch the
    /// per-DID rate limit refused. Surfaces as `key_resolution_unreachable`
    /// (502) so clients retry instead of treating their credentials as bad.
    #[error("DID resolution unreachable: {0}")]
    ResolutionUnreachable(String),

    #[error("key_id malformed: {0}")]
    KeyIdMalformed(String),

    #[error("key_id DID portion does not match agent_id")]
    KeyIdMismatch,

    #[error("key is not an assertionMethod")]
    KeyNotAssertion,

    #[error("unsupported DID method: {0}")]
    UnsupportedDidMethod(String),

    #[error("unsupported algorithm: {0}")]
    AlgorithmNotSupported(String),

    #[error("signature invalid: {0}")]
    SignatureInvalid(String),

    #[error("token invalid: {0}")]
    TokenInvalid(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl From<AuthError> for RegistryError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::Config(m) => RegistryError::Config(m),
            AuthError::Storage(m) => RegistryError::Storage(m),
            AuthError::ChallengeUnknown(m) => RegistryError::AuthChallenge(m),
            AuthError::ChallengeReplay(m) => RegistryError::AuthChallenge(m),
            AuthError::ChallengeExpired => RegistryError::AuthChallenge("expired".into()),
            AuthError::Resolution(m) => RegistryError::AuthChallenge(m),
            AuthError::ResolutionUnreachable(m) => {
                RegistryError::Acdp(acdp::error::AcdpError::KeyResolutionUnreachable(m))
            }
            AuthError::KeyIdMalformed(m) => RegistryError::AuthChallenge(format!("key_id: {m}")),
            AuthError::KeyIdMismatch => {
                RegistryError::AuthChallenge("key_id does not bind to agent_id".into())
            }
            AuthError::KeyNotAssertion => {
                RegistryError::AuthChallenge("key is not an assertionMethod".into())
            }
            AuthError::UnsupportedDidMethod(m) => {
                RegistryError::AuthChallenge(format!("unsupported DID method: {m}"))
            }
            AuthError::AlgorithmNotSupported(m) => {
                RegistryError::AuthChallenge(format!("unsupported algorithm: {m}"))
            }
            AuthError::SignatureInvalid(m) => {
                RegistryError::AuthChallenge(format!("signature: {m}"))
            }
            AuthError::TokenInvalid(m) => RegistryError::AuthToken(m),
            AuthError::Internal(m) => RegistryError::Internal(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_errors_split_transient_from_permanent() {
        let transient = RegistryError::from(AuthError::ResolutionUnreachable("x".into()));
        assert_eq!(transient.wire_code(), "key_resolution_unreachable");
        assert_eq!(transient.http_status(), 502);
        let permanent = RegistryError::from(AuthError::Resolution("x".into()));
        assert_eq!(permanent.wire_code(), "not_authorized");
        assert_eq!(permanent.http_status(), 403);
    }
}
