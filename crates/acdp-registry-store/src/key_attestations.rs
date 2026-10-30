//! RFC-ACDP-0021 key attestations: the storage-side shapes and the pure
//! checks every backend runs inside its publish transaction.
//!
//! An attestation is an ordinary context body under the registry DID
//! (§5.3). Beside it each backend keeps a derived index row (migration
//! 019, `key_attestations`), written in the same transaction as the body
//! and never anywhere else. The row carries what listing and the
//! publish-time checks need. The attestation's `content_hash`, status and
//! metadata are not copied into it: they come from the `contexts` row it
//! joins to, so the body stays the single source of truth.
//!
//! The checks themselves live here once, and each backend only supplies
//! the reads. Every backend refuses in the same order:
//!
//! 1. the backend's own pre-checks: idempotency replay, then the
//!    `supersedes` existence, ownership, coherence and AlreadySuperseded
//!    checks;
//! 2. for a keyless successor (§6.3): its attestation must exist
//!    (`attestation_not_found`) and must not be retracted
//!    (`attestation_withdrawn`);
//! 3. for a keyless successor with `supersedes` (§7): the supersession rule
//!    (`keyless_identity_mismatch`);
//! 4. the RFC-ACDP-0014 §4 predecessor admission, when the caller passes one;
//! 5. for a keyless successor: the registry-assigned `created_at` lies in the
//!    attestation window (`outside_attestation_window`);
//! 6. for an attestation mint: the token was never used before
//!    (`token_replayed`).
//!
//! Steps 2 and 3 run only after step 1, so they never become an existence
//! oracle for a predecessor the caller does not own.

use acdp::error::AcdpError;
use acdp::types::body::Body;
use acdp::types::keyless::{
    is_keyless_key_id, may_supersede, KeyAttestation, KeyAttestationListItem, KeyAttestationRef,
    KeylessReason, KeylessSigner, WorkloadIdentity, KEY_ATTESTATION_TYPE,
};
use acdp::types::primitives::{ContentHash, ContextType, CtxId, LineageId, Status};
use acdp::types::publish::PublishRequest;
use chrono::{DateTime, Utc};

/// The most items one `list_key_attestations` page returns.
pub const MAX_LIST_LIMIT: u32 = 200;

/// One `key_attestations` index row (migration 019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAttestationIndexRow {
    /// The attestation body's `ctx_id`, which is also the row's key.
    pub ctx_id: String,
    pub controller: String,
    pub identity: WorkloadIdentity,
    pub org_wide: bool,
    pub key_id: String,
    pub algorithm: String,
    pub key_fingerprint: String,
    /// Start of the window, inclusive.
    pub not_before_ms: i64,
    /// End of the window, exclusive.
    pub not_after_ms: i64,
    /// UNIQUE: a token mints at most one attestation, ever (D4).
    pub token_hash: String,
    /// The attestation body's `created_at`.
    pub created_at_ms: i64,
}

impl KeyAttestationIndexRow {
    /// The row for `att`, the metadata of the stored `body`.
    pub fn new(body: &Body, att: &KeyAttestation) -> Self {
        Self {
            ctx_id: body.ctx_id.as_str().to_string(),
            controller: att.controller.clone(),
            identity: att.identity.clone(),
            org_wide: att.org_wide,
            key_id: att.key_id.clone(),
            algorithm: att.algorithm.clone(),
            key_fingerprint: att.key_fingerprint.clone(),
            not_before_ms: att.not_before.timestamp_millis(),
            not_after_ms: att.not_after.timestamp_millis(),
            token_hash: att.token_hash.clone(),
            created_at_ms: body.created_at.timestamp_millis(),
        }
    }

    /// The `identity_type` column: `oidc` or `spiffe`.
    pub fn identity_type(&self) -> &'static str {
        match self.identity {
            WorkloadIdentity::Oidc { .. } => "oidc",
            WorkloadIdentity::Spiffe { .. } => "spiffe",
        }
    }

    /// The `issuer` column; `None` for a SPIFFE identity.
    pub fn issuer(&self) -> Option<&str> {
        match &self.identity {
            WorkloadIdentity::Oidc { issuer, .. } => Some(issuer),
            WorkloadIdentity::Spiffe { .. } => None,
        }
    }

    /// The `subject` column.
    pub fn subject(&self) -> &str {
        match &self.identity {
            WorkloadIdentity::Oidc { subject, .. } | WorkloadIdentity::Spiffe { subject } => {
                subject
            }
        }
    }

    /// The signer this attestation makes of a body (§7).
    pub fn signer(&self) -> KeylessSigner {
        KeylessSigner::Keyless {
            identity: self.identity.clone(),
            org_wide: self.org_wide,
        }
    }

    /// True when `t` lies in `[not_before, not_after)`.
    pub fn window_contains(&self, t: DateTime<Utc>) -> bool {
        let t = t.timestamp_millis();
        self.not_before_ms <= t && t < self.not_after_ms
    }
}

/// Rebuild the identity from its three index columns.
pub fn identity_from_columns(
    identity_type: &str,
    issuer: Option<String>,
    subject: String,
) -> Result<WorkloadIdentity, AcdpError> {
    match (identity_type, issuer) {
        ("oidc", Some(issuer)) => Ok(WorkloadIdentity::Oidc { issuer, subject }),
        ("spiffe", None) => Ok(WorkloadIdentity::Spiffe { subject }),
        (t, _) => Err(AcdpError::RegistryInternal(format!(
            "key_attestations row has an inconsistent identity (type '{t}')"
        ))),
    }
}

/// True when `body` is an attestation this registry minted: typed
/// `acdp:key-attestation` and published under `did:web:<authority>`.
fn is_minted_attestation(context_type: &ContextType, agent_id: &str, authority: &str) -> bool {
    matches!(context_type, ContextType::Custom(t) if t == KEY_ATTESTATION_TYPE)
        && agent_id == acdp::did::authority_to_did_web(authority)
}

/// Step 6's input: the index row a just-assigned `body` derives, or `None`
/// when it is not an attestation this registry minted.
///
/// A body that is one but breaks §5.3 is a code bug in the mint path. It
/// aborts the publish with `RegistryInternal`, so an attestation is never
/// committed without its index row.
pub fn index_row_for(
    body: &Body,
    authority: &str,
) -> Result<Option<KeyAttestationIndexRow>, AcdpError> {
    if !is_minted_attestation(&body.context_type, body.agent_id.as_str(), authority) {
        return Ok(None);
    }
    let att = KeyAttestation::from_body(body).map_err(|e| {
        AcdpError::RegistryInternal(format!(
            "refusing to commit key attestation '{}': {e}",
            body.ctx_id
        ))
    })?;
    Ok(Some(KeyAttestationIndexRow::new(body, &att)))
}

/// [`index_row_for`] on the body `req` will become, for a backend that
/// must decide before it commits. The placeholder `ctx_id` and
/// `created_at` play no part in the §5.3 checks, so the answer (and the
/// token hash) is the one the committed body gives.
pub fn index_row_for_request(
    req: &PublishRequest,
    authority: &str,
) -> Result<Option<KeyAttestationIndexRow>, AcdpError> {
    if !is_minted_attestation(&req.context_type, req.agent_id.as_str(), authority) {
        return Ok(None);
    }
    let placeholder = format!("acdp://{authority}/00000000-0000-4000-8000-000000000000");
    let body = Body::from_publish_request(
        req,
        CtxId(placeholder.clone()),
        LineageId(placeholder),
        authority,
        Utc::now(),
    );
    index_row_for(&body, authority)
}

/// The refusal for a token that already minted an attestation (§5.2).
pub fn token_replayed(token_hash: &str) -> AcdpError {
    KeylessReason::TokenReplayed.error(format!(
        "token {token_hash} was already used for an attestation"
    ))
}

/// The attestation a publish rests on, when its `signature.key_id` is a
/// keyless key id and it names one (§6.2). A keyless key id with no member
/// is the handler's `attestation_missing`; the store never sees it.
pub fn keyless_successor(req: &PublishRequest) -> Option<&KeyAttestationRef> {
    if is_keyless_key_id(&req.signature.key_id) {
        req.key_attestation.as_ref()
    } else {
        None
    }
}

/// What a backend read about a keyless successor's attestation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorAttestation {
    pub row: KeyAttestationIndexRow,
    /// The attestation context is retracted (§9.2).
    pub withdrawn: bool,
}

/// Step 2: the attestation `att_ctx` exists and is not retracted.
pub fn admit_successor<'a>(
    att_ctx: &CtxId,
    found: Option<&'a SuccessorAttestation>,
) -> Result<&'a SuccessorAttestation, AcdpError> {
    let found = found.ok_or_else(|| {
        KeylessReason::AttestationNotFound.error(format!("no key attestation at '{att_ctx}'"))
    })?;
    if found.withdrawn {
        return Err(KeylessReason::AttestationWithdrawn
            .error(format!("key attestation '{att_ctx}' was retracted")));
    }
    Ok(found)
}

/// Step 5: `created_at`, the time the registry assigns, lies in the window.
pub fn check_window(
    found: &SuccessorAttestation,
    created_at: DateTime<Utc>,
) -> Result<(), AcdpError> {
    if found.row.window_contains(created_at) {
        return Ok(());
    }
    Err(KeylessReason::OutsideAttestationWindow.error(format!(
        "created_at {} is outside the window of key attestation '{}'",
        acdp::time::trunc_ms(created_at).format("%Y-%m-%dT%H:%M:%S%.3fZ"),
        found.row.ctx_id
    )))
}

/// The attestation a stored predecessor was signed under: `None` when its
/// key id is not keyless (a controller key). A stored keyless body always
/// carries a valid `key_attestation`, so a missing or malformed one is a
/// `RegistryInternal` fault.
pub fn predecessor_attestation(body: &Body) -> Result<Option<CtxId>, AcdpError> {
    if !is_keyless_key_id(&body.signature.key_id) {
        return Ok(None);
    }
    match body.key_attestation() {
        Ok(Some(r)) => Ok(Some(r.ctx_id)),
        Ok(None) => Err(AcdpError::RegistryInternal(format!(
            "stored keyless body '{}' has no key_attestation",
            body.ctx_id
        ))),
        Err(e) => Err(AcdpError::RegistryInternal(format!(
            "stored keyless body '{}' has a malformed key_attestation: {e}",
            body.ctx_id
        ))),
    }
}

/// The predecessor's §7 signer, from its attestation `att` (see
/// [`predecessor_attestation`]) and that attestation's index row. A
/// keyless predecessor whose row is missing fails closed.
pub fn predecessor_signer(
    att: Option<&CtxId>,
    row: Option<&KeyAttestationIndexRow>,
) -> Result<KeylessSigner, AcdpError> {
    match (att, row) {
        (None, _) => Ok(KeylessSigner::Controller),
        (Some(_), Some(row)) => Ok(row.signer()),
        (Some(att), None) => Err(AcdpError::RegistryInternal(format!(
            "the predecessor's key attestation '{att}' has no index row"
        ))),
    }
}

/// Step 3: §7, may `successor` supersede a body signed by `predecessor`?
pub fn check_supersession(
    predecessor: &KeylessSigner,
    successor: &SuccessorAttestation,
) -> Result<(), AcdpError> {
    may_supersede(predecessor, &successor.row.signer()).map_err(|reason| {
        reason.error(format!(
            "the identity of key attestation '{}' may not supersede this predecessor",
            successor.row.ctx_id
        ))
    })
}

/// One attestation read back with its `contexts` row (D19).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyAttestationRecord {
    pub ctx_id: CtxId,
    /// `contexts.content_hash`, for §6.3 step 3.
    pub content_hash: ContentHash,
    /// The context's current status, for §6.3 step 5.
    pub status: Status,
    pub created_at_ms: i64,
    /// The parsed metadata, with `public_key_jwk` for §6.3 step 8.
    pub attestation: KeyAttestation,
    /// The metadata exactly as stored, for the list items.
    pub metadata: serde_json::Value,
}

impl KeyAttestationRecord {
    /// Assemble a record from the joined row's parts.
    pub fn from_parts(
        ctx_id: &str,
        content_hash: String,
        status: Status,
        created_at_ms: i64,
        body: &Body,
    ) -> Result<Self, AcdpError> {
        let metadata = body.metadata.clone().ok_or_else(|| {
            AcdpError::RegistryInternal(format!("key attestation '{ctx_id}' has no metadata"))
        })?;
        let attestation = KeyAttestation::from_metadata(&metadata).map_err(|e| {
            AcdpError::RegistryInternal(format!("stored key attestation '{ctx_id}': {e}"))
        })?;
        Ok(Self {
            ctx_id: CtxId(ctx_id.to_string()),
            content_hash: ContentHash(content_hash),
            status,
            created_at_ms,
            attestation,
            metadata,
        })
    }
}

/// A joined `contexts` row's status: the retraction flag dominates the
/// stored supersession status, as every read path projects it
/// (RFC-ACDP-0013 §7.2).
pub fn stored_status(status: &str, retracted: bool) -> Status {
    if retracted {
        return Status::Retracted;
    }
    match status {
        "active" => Status::Active,
        "superseded" => Status::Superseded,
        "expired" => Status::Expired,
        "retracted" => Status::Retracted,
        other => Status::Other(other.to_string()),
    }
}

/// One `GET /key-attestations` item from a joined row.
pub fn list_item(
    ctx_id: &str,
    content_hash: String,
    body: &Body,
) -> Result<KeyAttestationListItem, AcdpError> {
    let metadata = body.metadata.clone().ok_or_else(|| {
        AcdpError::RegistryInternal(format!("key attestation '{ctx_id}' has no metadata"))
    })?;
    Ok(KeyAttestationListItem {
        ctx_id: CtxId(ctx_id.to_string()),
        content_hash: ContentHash(content_hash),
        metadata,
    })
}

/// The optional §5.4 narrowing: each member, when set, must match the
/// identity byte for byte. A SPIFFE identity has no issuer, so an `issuer`
/// filter never matches one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyAttestationFilter {
    pub issuer: Option<String>,
    pub subject: Option<String>,
}

impl KeyAttestationFilter {
    /// True when `row` passes the filter.
    pub fn matches(&self, row: &KeyAttestationIndexRow) -> bool {
        self.issuer
            .as_deref()
            .is_none_or(|i| row.issuer() == Some(i))
            && self.subject.as_deref().is_none_or(|s| row.subject() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp::types::body::Signature;
    use chrono::TimeZone;

    fn row(identity: WorkloadIdentity, org_wide: bool) -> KeyAttestationIndexRow {
        KeyAttestationIndexRow {
            ctx_id: "acdp://reg.test/00000000-0000-4000-8000-000000000001".into(),
            controller: "did:web:agents.test".into(),
            identity,
            org_wide,
            key_id: "did:web:agents.test#acdp-kl-00".into(),
            algorithm: "ed25519".into(),
            key_fingerprint: "sha256:00".into(),
            not_before_ms: 1_000,
            not_after_ms: 2_000,
            token_hash: "sha256:11".into(),
            created_at_ms: 1_000,
        }
    }

    fn oidc(subject: &str) -> WorkloadIdentity {
        WorkloadIdentity::Oidc {
            issuer: "https://issuer.example".into(),
            subject: subject.into(),
        }
    }

    fn at(ms: i64) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(ms).single().expect("valid ms")
    }

    #[test]
    fn the_window_is_half_open() {
        let found = SuccessorAttestation {
            row: row(oidc("a"), false),
            withdrawn: false,
        };
        assert!(
            check_window(&found, at(1_000)).is_ok(),
            "not_before is inside"
        );
        assert!(check_window(&found, at(1_999)).is_ok());
        for outside in [999, 2_000, 2_001] {
            let err = check_window(&found, at(outside)).expect_err("outside");
            assert_eq!(
                KeylessReason::of(&err),
                Some(KeylessReason::OutsideAttestationWindow),
                "{outside}"
            );
            assert!(matches!(err, AcdpError::KeyNotAuthorized(_)), "{err:?}");
        }
    }

    #[test]
    fn a_missing_attestation_is_not_found_and_a_retracted_one_withdrawn() {
        let att = CtxId("acdp://reg.test/x".into());
        let err = admit_successor(&att, None).expect_err("missing");
        assert_eq!(
            KeylessReason::of(&err),
            Some(KeylessReason::AttestationNotFound)
        );
        assert!(matches!(err, AcdpError::KeyResolution(_)), "{err:?}");
        let withdrawn = SuccessorAttestation {
            row: row(oidc("a"), false),
            withdrawn: true,
        };
        let err = admit_successor(&att, Some(&withdrawn)).expect_err("withdrawn");
        assert_eq!(
            KeylessReason::of(&err),
            Some(KeylessReason::AttestationWithdrawn)
        );
        assert!(matches!(err, AcdpError::KeyNotAuthorized(_)), "{err:?}");
    }

    #[test]
    fn a_mismatch_is_not_authorized_with_its_reason() {
        let succ = SuccessorAttestation {
            row: row(oidc("b"), false),
            withdrawn: false,
        };
        let pred = row(oidc("a"), true).signer();
        let err = check_supersession(&pred, &succ).expect_err("S8");
        assert!(matches!(err, AcdpError::NotAuthorized(_)), "{err:?}");
        assert_eq!(
            KeylessReason::of(&err),
            Some(KeylessReason::KeylessIdentityMismatch)
        );
        assert!(check_supersession(&row(oidc("b"), false).signer(), &succ).is_ok());
    }

    #[test]
    fn a_keyless_predecessor_without_a_row_fails_closed() {
        let att = CtxId("acdp://reg.test/x".into());
        assert_eq!(
            predecessor_signer(None, None).expect("controller"),
            KeylessSigner::Controller
        );
        let err = predecessor_signer(Some(&att), None).expect_err("no row");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
    }

    #[test]
    fn identity_columns_round_trip_and_reject_inconsistent_rows() {
        for id in [
            oidc("s"),
            WorkloadIdentity::Spiffe {
                subject: "spiffe://td/x".into(),
            },
        ] {
            let r = row(id.clone(), false);
            let back = identity_from_columns(
                r.identity_type(),
                r.issuer().map(str::to_string),
                r.subject().to_string(),
            )
            .expect("round trip");
            assert_eq!(back, id);
        }
        assert!(identity_from_columns("oidc", None, "s".into()).is_err());
        assert!(identity_from_columns("spiffe", Some("i".into()), "s".into()).is_err());
        assert!(identity_from_columns("x509", None, "s".into()).is_err());
    }

    #[test]
    fn the_filter_matches_byte_for_byte() {
        let r = row(oidc("repo:a"), false);
        let f = |issuer: Option<&str>, subject: Option<&str>| KeyAttestationFilter {
            issuer: issuer.map(str::to_string),
            subject: subject.map(str::to_string),
        };
        assert!(f(None, None).matches(&r));
        assert!(f(Some("https://issuer.example"), Some("repo:a")).matches(&r));
        assert!(!f(Some("https://issuer.example/"), None).matches(&r));
        assert!(!f(None, Some("repo:A")).matches(&r));
        let spiffe = row(
            WorkloadIdentity::Spiffe {
                subject: "spiffe://td/x".into(),
            },
            false,
        );
        assert!(!f(Some("https://issuer.example"), None).matches(&spiffe));
        assert!(f(None, Some("spiffe://td/x")).matches(&spiffe));
    }

    #[test]
    fn only_a_keyless_key_id_with_a_member_is_a_keyless_successor() {
        let p = acdp::producer::Producer::new(
            acdp::crypto::SigningKey::from_bytes(&[7; 32]),
            acdp::types::primitives::AgentDid::new("did:web:agents.test".to_string()),
            "did:web:agents.test#key-1",
        );
        let mut req = p
            .publish_request()
            .title("t")
            .context_type(ContextType::DataSnapshot)
            .visibility(acdp::types::primitives::Visibility::Public)
            .build()
            .expect("request");
        assert!(keyless_successor(&req).is_none());
        let r = KeyAttestationRef {
            ctx_id: CtxId("acdp://reg.test/00000000-0000-4000-8000-000000000001".into()),
            content_hash: ContentHash(format!("sha256:{}", "a".repeat(64))),
        };
        req.key_attestation = Some(r.clone());
        assert!(keyless_successor(&req).is_none(), "member, controller key");
        req.signature = Signature {
            key_id: format!("did:web:agents.test#acdp-kl-{}", "b".repeat(64)),
            ..req.signature.clone()
        };
        assert_eq!(keyless_successor(&req), Some(&r));
        req.key_attestation = None;
        assert!(
            keyless_successor(&req).is_none(),
            "keyless key id, no member"
        );
    }
}
