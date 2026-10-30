//! `POST/GET /key-attestations` — RFC-ACDP-0021 keyless signing.
//!
//! This release does not implement `acdp-registry-keyless` and refuses to
//! advertise it (`validate_config`). A registry not advertising the profile
//! MUST answer both endpoints with `not_implemented` (501), RFC-ACDP-0021
//! §5.4 and the profile registry's note, so they are mounted to say so
//! rather than fall through to a 404.

use acdp::error::AcdpError;
use acdp_registry_types::RegistryError;

/// Both methods of `/key-attestations`: always 501 in this release.
pub async fn key_attestations_not_implemented() -> RegistryError {
    RegistryError::Acdp(AcdpError::NotImplemented(
        "this registry does not advertise acdp-registry-keyless \
         (RFC-ACDP-0021: /key-attestations is not implemented)"
            .into(),
    ))
}
