//! Playground-mode pinned-key enforcement.
//!
//! In playground mode the publish handler short-circuits the full
//! DID-web resolution pipeline (so demos don't need DID documents
//! served over HTTPS). That makes any agent_id+signature pair pass —
//! anyone can claim any DID. `PlaygroundConfig.pinned_keys` (see
//! `acdp-registry-types::config`) opts the operator back into
//! integrity: publishes from listed agents must verify against the
//! pinned public key.
//!
//! This module is the bridge between the pure-data
//! [`PlaygroundConfig`] in the types crate and the signature
//! verifiers in `acdp::crypto::verify`. Splitting it out keeps the
//! publish handler thin and makes the policy testable in isolation.
//!
//! Strict vs lax modes are documented on [`PlaygroundConfig`]. The
//! decision tree this module implements:
//!
//! ```text
//!   pinned_keys empty?         ──► Ok(Skipped)             (no policy)
//!   agent in pinned list?
//!     ├── alg mismatch?        ──► Err(downgrade defense)
//!     ├── ed25519              ──► verify_ed25519(...)
//!     └── ecdsa-p256           ──► verify_ecdsa_p256(...)
//!   not in list, lax?          ──► Ok(Unpinned)            (allowed)
//!   not in list, strict?       ──► Err(NotAuthorized)      (rejected)
//! ```
//!
//! ## Algorithm-downgrade defense
//!
//! The request's `signature.algorithm` MUST match the pinned entry's
//! `algorithm`. Without this check, an attacker who steals an Ed25519
//! key could claim `algorithm = "ecdsa-p256"` against an ECDSA-pinned
//! agent (or vice versa) and force-verify under the wrong code path.

use acdp::error::AcdpError;
use acdp::types::publish::PublishRequest;
use acdp_registry_types::{
    config::{PinnedAgentKey, PlaygroundConfig},
    RegistryError,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// Supported algorithms for pinned-key entries.
///
/// Kept narrow on purpose: even though the ACDP signature-algorithms
/// registry lists more, only the ones a real agent in the wild
/// publishes today are accepted here. Add new variants by extending
/// this enum + the dispatch in [`enforce_pinned_signature`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinnedAlgorithm {
    Ed25519,
    EcdsaP256,
}

impl PinnedAlgorithm {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "ed25519" => Some(Self::Ed25519),
            "ecdsa-p256" => Some(Self::EcdsaP256),
            _ => None,
        }
    }

    fn wire_name(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::EcdsaP256 => "ecdsa-p256",
        }
    }
}

/// Outcome of [`enforce_pinned_signature`].
///
/// `Verified` and `Unpinned` are both "publish may proceed"; they're
/// kept distinct so the handler can log the path taken (useful when
/// triaging "why did this publish go through?" tickets).
#[derive(Debug, PartialEq, Eq)]
pub enum PinOutcome {
    /// `pinned_keys` was empty — no policy active.
    Skipped,
    /// Agent was pinned and the signature verified against the pinned key.
    /// Carries the matched entry's key material so the caller can mint a
    /// receipt fingerprint without re-looking up the config.
    Verified {
        public_key_b64: String,
        algorithm: String,
    },
    /// Agent was not pinned and `pinned_only = false`.
    Unpinned,
}

/// Verify a publish request against the pinned-key list.
///
/// Returns `Ok(_)` when the publish should proceed; `Err(_)` when it
/// must be rejected. The returned [`PinOutcome`] tells the caller
/// which branch fired (mainly for logging).
///
/// Algorithm dispatch covers `ed25519` (32-byte raw key) and
/// `ecdsa-p256` (65-byte SEC1-uncompressed key). The request's
/// declared `signature.algorithm` must match the pinned entry's
/// `algorithm` — see the downgrade-defense paragraph in this
/// module's docs.
pub fn enforce_pinned_signature(
    req: &PublishRequest,
    config: &PlaygroundConfig,
) -> Result<PinOutcome, RegistryError> {
    if config.pinned_keys.is_empty() {
        return Ok(PinOutcome::Skipped);
    }

    let agent_did = req.agent_id.as_str();
    let Some(pinned) = config.pinned_for(agent_did) else {
        if config.pinned_only {
            return Err(RegistryError::Acdp(AcdpError::KeyNotAuthorized(format!(
                "agent_did '{agent_did}' is not in playground.pinned_keys \
                 and playground.pinned_only is true"
            ))));
        }
        return Ok(PinOutcome::Unpinned);
    };

    let alg = PinnedAlgorithm::parse(&pinned.algorithm).ok_or_else(|| {
        RegistryError::Config(format!(
            "pinned algorithm '{}' for {agent_did} is not supported (expected one of: ed25519, ecdsa-p256)",
            pinned.algorithm
        ))
    })?;

    // Downgrade defense: the request's signature algorithm MUST match
    // the pinned algorithm. Without this, a stolen key for one curve
    // could be claimed against the other curve's verifier.
    if req.signature.algorithm != alg.wire_name() {
        return Err(RegistryError::Acdp(AcdpError::KeyNotAuthorized(format!(
            "signature.algorithm '{}' does not match pinned algorithm '{}' for {agent_did}",
            req.signature.algorithm,
            alg.wire_name(),
        ))));
    }

    match alg {
        PinnedAlgorithm::Ed25519 => verify_ed25519_pinned(agent_did, pinned, req)?,
        PinnedAlgorithm::EcdsaP256 => verify_ecdsa_p256_pinned(agent_did, pinned, req)?,
    }

    Ok(PinOutcome::Verified {
        public_key_b64: pinned.public_key_b64.clone(),
        algorithm: alg.wire_name().to_string(),
    })
}

fn verify_ed25519_pinned(
    agent_did: &str,
    pinned: &PinnedAgentKey,
    req: &PublishRequest,
) -> Result<(), RegistryError> {
    let pub_bytes = decode_ed25519_pinned(agent_did, pinned)?;

    acdp::crypto::verify::verify_ed25519(
        &pub_bytes,
        &req.signature.value,
        req.content_hash.as_str(),
    )
    .map_err(RegistryError::Acdp)
}

fn verify_ecdsa_p256_pinned(
    agent_did: &str,
    pinned: &PinnedAgentKey,
    req: &PublishRequest,
) -> Result<(), RegistryError> {
    let pub_bytes = decode_ecdsa_p256_pinned(agent_did, pinned)?;

    acdp::crypto::verify::verify_ecdsa_p256(
        &pub_bytes,
        &req.signature.value,
        req.content_hash.as_str(),
    )
    .map_err(RegistryError::Acdp)
}

/// Decode a pinned ed25519 key, enforcing exactly the rules the verify path
/// enforces. Extracted (W3-U1, #193) so the startup validator and the request
/// path cannot drift: these length/encoding rules are the definition of a
/// usable pinned key, and a second copy would be the defect this closes.
///
/// Error text is byte-identical to what the request path emitted before the
/// extraction — deliberately, so request-path behaviour is unchanged.
fn decode_ed25519_pinned(
    agent_did: &str,
    pinned: &PinnedAgentKey,
) -> Result<[u8; 32], RegistryError> {
    let pub_bytes_vec = STANDARD.decode(&pinned.public_key_b64).map_err(|e| {
        RegistryError::Config(format!(
            "pinned public_key_b64 for {agent_did} is not valid base64: {e}"
        ))
    })?;
    pub_bytes_vec.as_slice().try_into().map_err(|_| {
        RegistryError::Config(format!(
            "pinned ed25519 public_key_b64 for {agent_did} decoded to {} bytes (expected 32)",
            pub_bytes_vec.len()
        ))
    })
}

/// Decode a pinned ecdsa-p256 key. Separate from the ed25519 decoder on
/// purpose: that one yields `[u8; 32]`, this one a `Vec<u8>` plus a tag check.
/// A single shared decoder would have forced the ed25519 caller into an
/// `.expect()` — a new panic path in a request handler, where there is none.
///
/// The length check MUST stay before the `pub_bytes[0]` index, or an empty
/// decode panics.
fn decode_ecdsa_p256_pinned(
    agent_did: &str,
    pinned: &PinnedAgentKey,
) -> Result<Vec<u8>, RegistryError> {
    let pub_bytes = STANDARD.decode(&pinned.public_key_b64).map_err(|e| {
        RegistryError::Config(format!(
            "pinned public_key_b64 for {agent_did} is not valid base64: {e}"
        ))
    })?;
    // SEC1 uncompressed P-256: 0x04 || X(32) || Y(32) = 65 bytes.
    if pub_bytes.len() != 65 {
        return Err(RegistryError::Config(format!(
            "pinned ecdsa-p256 public_key_b64 for {agent_did} decoded to {} bytes (expected 65 SEC1-uncompressed)",
            pub_bytes.len()
        )));
    }
    if pub_bytes[0] != 0x04 {
        return Err(RegistryError::Config(format!(
            "pinned ecdsa-p256 public_key_b64 for {agent_did} must start with 0x04 (SEC1 uncompressed); got 0x{:02x}",
            pub_bytes[0]
        )));
    }
    Ok(pub_bytes)
}

/// Validate a `[playground]` section before it is allowed to take effect
/// (W3-U1, closing #192 and #193).
///
/// Called from **two** places that must agree: the server binary's
/// `validate_config` at startup, and `POST /admin/pinned-keys/reload` at
/// runtime. Before this, only the former validated anything and only
/// shallowly, so an operator could reload their way past every startup guard.
///
/// `receipt_configured` is passed in because the RFC-ACDP-0010 §7 rule couples
/// `[playground]` to `[receipt]`, and the live cell this guards only ever holds
/// `playground` — the reload path supplies the *running* receipt posture, which
/// is the right input since receipts are restart-only.
///
/// `now` is injected rather than read from the clock so callers can test
/// validity windows deterministically; the types crate's clock helper is
/// private to it.
///
/// Returns non-fatal warnings on success. Warnings are *returned* rather than
/// logged so the caller owns presentation and tests can assert on them without
/// a tracing subscriber.
pub fn validate_playground_config(
    cfg: &PlaygroundConfig,
    receipt_configured: bool,
    now: i64,
) -> Result<Vec<String>, String> {
    // Structural defects first: deterministic, time-independent, and silent
    // today (they surface only as per-request 500s).
    for (i, pin) in cfg.pinned_keys.iter().enumerate() {
        let did = pin.agent_did.as_str();
        let alg = PinnedAlgorithm::parse(&pin.algorithm).ok_or_else(|| {
            format!(
                "playground.pinned_keys[{i}] ({did}): algorithm '{}' is not supported (expected \
                 one of: ed25519, ecdsa-p256). A publish from this agent that selects this entry \
                 is rejected at request time (HTTP 500, internal_error).",
                pin.algorithm
            )
        })?;
        match alg {
            PinnedAlgorithm::Ed25519 => {
                decode_ed25519_pinned(did, pin)
                    .map(|_| ())
                    .map_err(|e| format!("playground.pinned_keys[{i}]: {e}"))?;
            }
            PinnedAlgorithm::EcdsaP256 => {
                decode_ecdsa_p256_pinned(did, pin)
                    .map(|_| ())
                    .map_err(|e| format!("playground.pinned_keys[{i}]: {e}"))?;
            }
        }
        if let (Some(from), Some(until)) = (pin.valid_from, pin.valid_until) {
            if from >= until {
                return Err(format!(
                    "playground.pinned_keys[{i}] ({did}): valid_from ({from}) is not before \
                     valid_until ({until}), so this entry can never be in its validity window"
                ));
            }
        }
    }

    // RFC-ACDP-0010 §7: a receipts-advertising registry has no unverified
    // publish path. Enforced here rather than only at startup so a reload
    // cannot reintroduce the state (#192).
    if receipt_configured && cfg.enabled && !cfg.pinned_only {
        return Err(
            "playground.enabled with pinned_only=false is incompatible with [receipt]: a \
             receipts-advertising registry has no unverified publish path (RFC-ACDP-0010 \
             §7: no degraded mode). Set playground.pinned_only=true (every publish then \
             verifies against a playground.pinned_keys entry) or disable playground.enabled."
                .to_string(),
        );
    }

    // W2-U1 (#185), moved here from the binary so both doors share one copy.
    if cfg.enabled && cfg.pinned_only && cfg.pinned_keys.is_empty() {
        return Err(
            "playground.pinned_only=true has no effect while playground.pinned_keys is \
             empty: this config does NOT restrict publishing — every non-did:key agent \
             falls through to the fully unverified playground path and is accepted \
             without a signature check. Add at least one [[playground.pinned_keys]] entry, \
             or set playground.enabled=false."
                .to_string(),
        );
    }

    // Non-fatal: no entry currently in window. NOT a boot failure — expiry is
    // time-dependent, so refusing would make bootability a function of the wall
    // clock, and in lax mode this state is behaviourally identical to having no
    // pins at all, which is supported. But the two branches do OPPOSITE things,
    // so the text branches too.
    let mut warnings = Vec::new();
    if cfg.enabled && !cfg.pinned_keys.is_empty() {
        let any_live = cfg.pinned_keys.iter().any(|p| p.is_valid_at(now));
        if !any_live {
            let n = cfg.pinned_keys.len();
            let latest = cfg
                .pinned_keys
                .iter()
                .filter_map(|p| p.valid_until)
                .max()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "none".to_string());
            warnings.push(if cfg.pinned_only {
                format!(
                    "playground.pinned_keys: no entry is currently within its validity window \
                     ({n} entries, most recent valid_until {latest}). With pinned_only=true every \
                     did:web publish is now rejected until a key is rotated in; did:key publishes \
                     and all reads are unaffected."
                )
            } else {
                format!(
                    "playground.pinned_keys: no entry is currently within its validity window \
                     ({n} entries, most recent valid_until {latest}). With pinned_only=false \
                     publishes from these agents are now ACCEPTED WITH NO SIGNATURE CHECK — \
                     pinning is inert until a key is rotated in."
                )
            });
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp::crypto::sign::P256SigningKey;
    use acdp::crypto::SigningKey;
    use acdp::producer::Producer;
    use acdp::types::primitives::{AgentDid, ContextType, Visibility};
    use acdp_registry_types::config::PinnedAgentKey;

    /// Build a real, signed Ed25519 PublishRequest from a freshly
    /// generated SigningKey. Mirrors what the Python SDK builds.
    fn build_signed_request(key: SigningKey, did: &str) -> (PublishRequest, String) {
        let agent_did = AgentDid::new(did);
        let key_id = format!("{did}#key-1");
        let pub_b64 = base64::engine::general_purpose::STANDARD.encode(key.verifying_key_bytes());
        let producer = Producer::new(key, agent_did, &key_id);
        let req = producer
            .publish_request()
            .title("test")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .summary("body")
            .build()
            .expect("build request");
        (req, pub_b64)
    }

    /// Build a P-256-signed publish request by hand. We don't have a
    /// `Producer::new_p256` helper, so we build the Ed25519 request
    /// first (to get the canonical content_hash + everything else) and
    /// then re-sign + relabel the `signature` field with P-256.
    fn build_p256_signed_request(did: &str) -> (PublishRequest, String) {
        // First build a request with a throwaway Ed25519 key — gives
        // us the right shape + a real content_hash to sign.
        let throwaway = SigningKey::generate();
        let (mut req, _) = build_signed_request(throwaway, did);

        // Now sign the canonical content_hash with a fresh P-256 key
        // and swap that into the signature object.
        let p256_key = P256SigningKey::generate();
        let sig_b64 = p256_key.sign_content_hash(&req.content_hash);
        req.signature.algorithm = "ecdsa-p256".into();
        req.signature.value = sig_b64;

        let pub_b64 =
            base64::engine::general_purpose::STANDARD.encode(p256_key.verifying_key_sec1());
        (req, pub_b64)
    }

    fn cfg(pinned: Vec<PinnedAgentKey>, strict: bool) -> PlaygroundConfig {
        PlaygroundConfig {
            enabled: true,
            pinned_keys: pinned,
            pinned_only: strict,
        }
    }

    #[test]
    fn empty_pinned_list_is_skipped() {
        let key = SigningKey::generate();
        let (req, _) = build_signed_request(key, "did:web:x:agents:alice");
        let outcome = enforce_pinned_signature(&req, &cfg(vec![], false)).unwrap();
        assert_eq!(outcome, PinOutcome::Skipped);
    }

    #[test]
    fn pinned_agent_with_matching_ed25519_key_verifies() {
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (req, pub_b64) = build_signed_request(key, did);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: pub_b64.clone(),
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let outcome = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap();
        assert_eq!(
            outcome,
            PinOutcome::Verified {
                public_key_b64: pub_b64,
                algorithm: "ed25519".into(),
            }
        );
    }

    #[test]
    fn pinned_agent_with_wrong_ed25519_key_is_rejected() {
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (req, _real_pub) = build_signed_request(key, did);

        let wrong = SigningKey::generate();
        let wrong_pub =
            base64::engine::general_purpose::STANDARD.encode(wrong.verifying_key_bytes());
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: wrong_pub,
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("InvalidSignature") || msg.contains("signature"),
            "expected signature failure, got {msg}"
        );
    }

    #[test]
    fn unpinned_agent_in_lax_mode_passes() {
        let key = SigningKey::generate();
        let (req, _) = build_signed_request(key, "did:web:x:agents:bob");
        let pinned_for_other = PinnedAgentKey {
            agent_did: "did:web:x:agents:alice".into(),
            public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let outcome = enforce_pinned_signature(&req, &cfg(vec![pinned_for_other], false)).unwrap();
        assert_eq!(outcome, PinOutcome::Unpinned);
    }

    #[test]
    fn unpinned_agent_in_strict_mode_is_rejected() {
        let key = SigningKey::generate();
        let (req, _) = build_signed_request(key, "did:web:x:agents:bob");
        let pinned_for_other = PinnedAgentKey {
            agent_did: "did:web:x:agents:alice".into(),
            public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned_for_other], true)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not in playground.pinned_keys"), "got {msg}");
    }

    // ── ECDSA-P256 cases ────────────────────────────────────────────

    #[test]
    fn pinned_agent_with_matching_p256_key_verifies() {
        let did = "did:web:x:agents:alice";
        let (req, pub_b64) = build_p256_signed_request(did);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: pub_b64.clone(),
            algorithm: "ecdsa-p256".into(),
            valid_from: None,
            valid_until: None,
        };
        let outcome = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap();
        assert_eq!(
            outcome,
            PinOutcome::Verified {
                public_key_b64: pub_b64,
                algorithm: "ecdsa-p256".into(),
            }
        );
    }

    #[test]
    fn pinned_p256_with_wrong_key_is_rejected() {
        let did = "did:web:x:agents:alice";
        let (req, _real_pub) = build_p256_signed_request(did);
        let wrong = P256SigningKey::generate();
        let wrong_pub =
            base64::engine::general_purpose::STANDARD.encode(wrong.verifying_key_sec1());
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: wrong_pub,
            algorithm: "ecdsa-p256".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("InvalidSignature") || msg.contains("signature"),
            "expected signature failure, got {msg}"
        );
    }

    #[test]
    fn p256_pin_with_wrong_length_key_is_a_config_error() {
        let did = "did:web:x:agents:alice";
        let (req, _) = build_p256_signed_request(did);
        // Truncate to 32 bytes — wrong length for SEC1 uncompressed P-256.
        let too_short = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: too_short,
            algorithm: "ecdsa-p256".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("65"), "got {msg}");
    }

    #[test]
    fn p256_pin_with_wrong_sec1_tag_is_a_config_error() {
        let did = "did:web:x:agents:alice";
        let (req, pub_b64) = build_p256_signed_request(did);
        // Replace the first byte (0x04 SEC1 tag) with 0x02 (compressed
        // form) — verifier wouldn't accept this; we surface it as a
        // config error so the operator notices on first publish.
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&pub_b64)
            .unwrap();
        bytes[0] = 0x02;
        let bad_tag = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: bad_tag,
            algorithm: "ecdsa-p256".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("0x04") || msg.contains("SEC1"), "got {msg}");
    }

    // ── algorithm-downgrade defense ─────────────────────────────────

    #[test]
    fn ed25519_sig_against_p256_pin_is_rejected() {
        // Build an ed25519 publish request, then pin the agent with
        // a P-256 key. Request's signature.algorithm = "ed25519",
        // pinned.algorithm = "ecdsa-p256" → mismatch.
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (req, _) = build_signed_request(key, did);
        let p256_key = P256SigningKey::generate();
        let pub_b64 =
            base64::engine::general_purpose::STANDARD.encode(p256_key.verifying_key_sec1());
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: pub_b64,
            algorithm: "ecdsa-p256".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("does not match pinned algorithm"),
            "expected downgrade-defense rejection, got {msg}"
        );
    }

    #[test]
    fn p256_sig_against_ed25519_pin_is_rejected() {
        let did = "did:web:x:agents:alice";
        let (req, _) = build_p256_signed_request(did);
        // Pin with an Ed25519 key — request's algorithm = "ecdsa-p256"
        // does not match → downgrade defense fires before any
        // verification path runs.
        let ed_key = SigningKey::generate();
        let ed_pub = base64::engine::general_purpose::STANDARD.encode(ed_key.verifying_key_bytes());
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: ed_pub,
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("does not match pinned algorithm"),
            "expected downgrade-defense rejection, got {msg}"
        );
    }

    // ── config errors ───────────────────────────────────────────────

    #[test]
    fn unsupported_algorithm_surfaces_config_error() {
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (req, pub_b64) = build_signed_request(key, did);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: pub_b64,
            algorithm: "rsa-sha256".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("rsa-sha256"), "got {msg}");
        assert!(msg.contains("ed25519, ecdsa-p256"), "got {msg}");
    }

    #[test]
    fn invalid_base64_in_config_surfaces_config_error() {
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (req, _) = build_signed_request(key, did);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: "not-base64!!!".into(),
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("base64"), "got {msg}");
    }

    #[test]
    fn wrong_length_pinned_ed25519_key_surfaces_config_error() {
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (req, _) = build_signed_request(key, did);
        let too_short = base64::engine::general_purpose::STANDARD.encode([0u8; 16]);
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: too_short,
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("32"), "got {msg}");
    }

    #[test]
    fn signature_algorithm_label_is_case_sensitive() {
        // Downgrade defense compares the request's signature.algorithm against
        // the pinned algorithm's exact wire name. An uppercased "ED25519" must
        // NOT be silently accepted as "ed25519" — a refactor toward a
        // case-insensitive match would weaken the downgrade guard.
        let key = SigningKey::generate();
        let did = "did:web:x:agents:alice";
        let (mut req, pub_b64) = build_signed_request(key, did);
        req.signature.algorithm = "ED25519".into();
        let pinned = PinnedAgentKey {
            agent_did: did.into(),
            public_key_b64: pub_b64,
            algorithm: "ed25519".into(),
            valid_from: None,
            valid_until: None,
        };
        let err = enforce_pinned_signature(&req, &cfg(vec![pinned], false)).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("does not match pinned algorithm"),
            "expected downgrade-defense rejection, got {msg}"
        );
    }
}
