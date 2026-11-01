//! `bearer_static` read tokens (RFC-ACDP-0008 §6.2).
//!
//! A token is `acdp_rt_` followed by base64url (no padding) of 32 bytes from
//! the OS CSPRNG: 256 bits, past the §6.2 floor of 128. The registry keeps
//! only the SHA-256 of the full token string. A plain hash is enough here: the
//! input is 256 random bits, so there is nothing for a slow hash to protect.
//!
//! The prefix lets the read gate tell a static token from a JWT without
//! trying to parse one as the other, and lets secret scanners find leaked
//! tokens.
//!
//! Nothing in this module logs, formats or returns a token: errors carry a
//! reason only, and [`StaticReadPrincipal`] carries the binding, not the
//! secret.

use acdp_registry_types::config::AuthConfig;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;
use rand::Rng;
use sha2::{Digest, Sha256};

/// Every `bearer_static` token starts with this.
pub const READ_TOKEN_PREFIX: &str = "acdp_rt_";

/// Random bytes behind a token.
const TOKEN_BYTES: usize = 32;

/// base64url without padding of [`TOKEN_BYTES`].
const TOKEN_BODY_LEN: usize = 43;

/// Whether `token` claims to be a `bearer_static` token. Says nothing about
/// validity; a prefixed token that fails [`ReadTokens::validate`] is a 403,
/// never a JWT.
pub fn is_read_token(token: &str) -> bool {
    token.starts_with(READ_TOKEN_PREFIX)
}

/// A fresh token and its lowercase hex SHA-256, for `[[auth.read_tokens]]`.
/// The token is for the consumer's secret store; only the hash goes into
/// registry config.
pub fn generate_read_token() -> (String, String) {
    let mut bytes = [0u8; TOKEN_BYTES];
    // Straight from the OS: a failing system RNG panics rather than
    // yielding a weak token.
    UnwrapErr(SysRng).fill_bytes(&mut bytes);
    let token = format!("{READ_TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    let hash = read_token_sha256_hex(&token);
    (token, hash)
}

/// Lowercase hex SHA-256 of the full token string, prefix included.
pub fn read_token_sha256_hex(token: &str) -> String {
    hex_lower(&Sha256::digest(token.as_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Parse a configured `sha256` value: exactly 64 lowercase hex characters.
pub fn parse_sha256_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out[i] = u8::try_from(hi * 16 + lo).ok()?;
    }
    Some(out)
}

/// Why a presented token did not validate. Every variant is a 403
/// `not_authorized` (§6.2); the distinction is for metrics and tests, and
/// none of them carries the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadTokenError {
    #[error("malformed read token")]
    Malformed,
    #[error("unknown read token")]
    Unknown,
    #[error("expired read token")]
    Expired,
}

/// The binding a valid token authenticates as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticReadPrincipal {
    /// `effective_requester_did` for the request.
    pub did: String,
    /// The entry's tenant, else the DID's `[[auth.tenant_agents]]` binding.
    pub tenant: Option<String>,
    /// The entry's operator label, for audit logs.
    pub token_id: String,
}

#[derive(Debug, Clone)]
struct Entry {
    hash: [u8; 32],
    expires_at: Option<DateTime<Utc>>,
    principal: StaticReadPrincipal,
}

/// The configured tokens, by hash. Built once from [`AuthConfig`]; an entry
/// whose hash does not parse is left out, so it can never validate. Startup
/// validation refuses such a config before this is built, so dropping one
/// only matters for an embedder that skips it, and it fails closed.
#[derive(Debug, Clone, Default)]
pub struct ReadTokens {
    entries: Vec<Entry>,
}

impl ReadTokens {
    pub fn from_config(cfg: &AuthConfig) -> Self {
        let entries = cfg
            .read_tokens
            .iter()
            .filter_map(|e| {
                Some(Entry {
                    hash: parse_sha256_hex(&e.sha256)?,
                    expires_at: e.expires_at,
                    principal: StaticReadPrincipal {
                        did: e.did.clone(),
                        tenant: e.tenant.clone().or_else(|| cfg.tenant_for_agent(&e.did)),
                        token_id: e.id.clone(),
                    },
                })
            })
            .collect();
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Validate a presented token at `now`.
    ///
    /// The digest is compared against every entry with a fold that never
    /// short-circuits, so the time taken does not depend on which entry (if
    /// any) matched or on how many leading bytes agreed.
    pub fn validate(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<StaticReadPrincipal, ReadTokenError> {
        let body = token
            .strip_prefix(READ_TOKEN_PREFIX)
            .ok_or(ReadTokenError::Malformed)?;
        if body.len() != TOKEN_BODY_LEN
            || URL_SAFE_NO_PAD
                .decode(body)
                .map_or(true, |b| b.len() != TOKEN_BYTES)
        {
            return Err(ReadTokenError::Malformed);
        }
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut found: Option<&Entry> = None;
        for entry in &self.entries {
            if ct_eq_32(&entry.hash, &digest) {
                found = Some(entry);
            }
        }
        let entry = found.ok_or(ReadTokenError::Unknown)?;
        if entry.expires_at.is_some_and(|t| now >= t) {
            return Err(ReadTokenError::Expired);
        }
        Ok(entry.principal.clone())
    }
}

/// Constant-time equality of two digests: an XOR fold over every byte.
fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp_registry_types::config::{ReadTokenEntry, TenantAgentBinding};

    fn entry(id: &str, did: &str, token: &str) -> ReadTokenEntry {
        ReadTokenEntry {
            id: id.into(),
            did: did.into(),
            sha256: read_token_sha256_hex(token),
            expires_at: None,
            tenant: None,
            operator_administered: true,
        }
    }

    #[test]
    fn generated_tokens_are_prefixed_256_bit_and_distinct() {
        let (a, ha) = generate_read_token();
        let (b, _) = generate_read_token();
        assert_ne!(a, b);
        assert!(is_read_token(&a));
        let body = a.strip_prefix(READ_TOKEN_PREFIX).unwrap();
        assert_eq!(body.len(), TOKEN_BODY_LEN);
        assert_eq!(URL_SAFE_NO_PAD.decode(body).unwrap().len(), 32);
        assert_eq!(ha, read_token_sha256_hex(&a));
        assert_eq!(ha.len(), 64);
        assert!(parse_sha256_hex(&ha).is_some());
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        // SHA-256("abc"), FIPS 180-2 appendix B.1.
        assert_eq!(
            read_token_sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parse_sha256_hex_wants_exactly_64_lowercase_hex() {
        let ok = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(parse_sha256_hex(ok).unwrap()[0], 0xba);
        assert!(parse_sha256_hex(&ok.to_uppercase()).is_none());
        assert!(parse_sha256_hex(&ok[..62]).is_none());
        assert!(parse_sha256_hex(&format!("{ok}00")).is_none());
        assert!(parse_sha256_hex(&format!("sha256:{}", &ok[7..])).is_none());
        assert!(parse_sha256_hex(&"g".repeat(64)).is_none());
    }

    #[test]
    fn validate_binds_did_and_tenant_and_rejects_the_rest() {
        let (t1, _) = generate_read_token();
        let (t2, _) = generate_read_token();
        let (stranger, _) = generate_read_token();
        let cfg = AuthConfig {
            read_tokens: vec![
                entry("one", "did:web:a.example", &t1),
                ReadTokenEntry {
                    tenant: Some("t-explicit".into()),
                    ..entry("two", "did:web:a.example", &t2)
                },
            ],
            tenant_agents: vec![TenantAgentBinding {
                agent_did: "did:web:a.example".into(),
                tenant_id: "t-bound".into(),
            }],
            ..AuthConfig::default()
        };
        let set = ReadTokens::from_config(&cfg);
        let now = Utc::now();
        // Two valid tokens for one DID at once (rotation, §6.2).
        let p1 = set.validate(&t1, now).unwrap();
        assert_eq!(p1.did, "did:web:a.example");
        assert_eq!(p1.tenant.as_deref(), Some("t-bound"), "binding fallback");
        assert_eq!(p1.token_id, "one");
        let p2 = set.validate(&t2, now).unwrap();
        assert_eq!(p2.tenant.as_deref(), Some("t-explicit"), "entry wins");
        assert_eq!(set.validate(&stranger, now), Err(ReadTokenError::Unknown));
        for bad in [
            "acdp_rt_",
            "acdp_rt_short",
            &format!("{t1}x"),
            &t1[..t1.len() - 1],
            "acdp_rt_!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!",
            "eyJhbGciOiJIUzI1NiJ9.e30.x",
        ] {
            assert_eq!(
                set.validate(bad, now),
                Err(ReadTokenError::Malformed),
                "{bad}"
            );
        }
    }

    #[test]
    fn expiry_is_inclusive_of_the_instant() {
        let (t, _) = generate_read_token();
        let at = Utc::now();
        let cfg = AuthConfig {
            read_tokens: vec![ReadTokenEntry {
                expires_at: Some(at),
                ..entry("e", "did:web:a.example", &t)
            }],
            ..AuthConfig::default()
        };
        let set = ReadTokens::from_config(&cfg);
        assert!(set.validate(&t, at - chrono::Duration::seconds(1)).is_ok());
        assert_eq!(set.validate(&t, at), Err(ReadTokenError::Expired));
    }

    #[test]
    fn an_unparseable_configured_hash_never_validates() {
        let (t, h) = generate_read_token();
        let cfg = AuthConfig {
            read_tokens: vec![ReadTokenEntry {
                sha256: h.to_uppercase(),
                ..entry("u", "did:web:a.example", &t)
            }],
            ..AuthConfig::default()
        };
        let set = ReadTokens::from_config(&cfg);
        assert!(set.is_empty());
        assert_eq!(set.validate(&t, Utc::now()), Err(ReadTokenError::Unknown));
    }

    #[test]
    fn errors_never_carry_the_token() {
        let (t, _) = generate_read_token();
        let set = ReadTokens::default();
        for e in [
            set.validate(&t, Utc::now()).unwrap_err(),
            set.validate(&format!("{t}x"), Utc::now()).unwrap_err(),
        ] {
            let shown = format!("{e} {e:?}");
            assert!(!shown.contains(&t[READ_TOKEN_PREFIX.len()..]), "{shown}");
        }
    }

    #[test]
    fn ct_eq_32_is_equality() {
        let a = [7u8; 32];
        let mut b = a;
        assert!(ct_eq_32(&a, &b));
        b[31] ^= 1;
        assert!(!ct_eq_32(&a, &b));
        b = a;
        b[0] ^= 0x80;
        assert!(!ct_eq_32(&a, &b));
    }
}
