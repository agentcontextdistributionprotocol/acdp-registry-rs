//! RFC-ACDP-0018 change feed: the storage-side shapes every backend shares.
//!
//! The feed is an append-only table with a registry-wide, strictly
//! increasing `seq` (§5.3). Rows are written ONLY inside the transactions
//! that make the change they record — `RegistryStore::commit_publish` and
//! `RegistryStore::commit_lifecycle_event` — so an entry is visible exactly
//! when its change is (rec-008). There is deliberately no standalone append
//! API on [`crate::ExtendedRegistryStore`].
//!
//! A row stores copies of its evidence (the event, and the receipt minted in
//! the same transaction), so entries are immutable by construction and the
//! read path is one indexed range scan. [`FeedRow::to_change`] rebuilds the
//! wire entry through the 0.17.0 constructors, which derive `recorded_at` and
//! re-check the evidence shape.
//!
//! What gets appended is decided here, not in the backends:
//! [`lifecycle_append`] and [`feed_appends_for_publish`] are the §4 rules,
//! and every backend only persists their output.

use acdp::error::AcdpError;
use acdp::types::body::Body;
use acdp::types::change::{Change, ChangeKind};
use acdp::types::lifecycle::LifecycleEvent;
use acdp::types::primitives::{AgentDid, CtxId, LineageId};
use acdp::types::{EventReceipt, RegistryReceipt};
use serde_json::Value;

/// One row to append, as derived by the §4 rules.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedAppend {
    pub kind: ChangeKind,
    /// The affected context.
    pub ctx_id: String,
    pub lineage_id: String,
    /// The affected context's producer (lifecycle, superseded) or the
    /// revoked key's controller (key_revocation).
    pub agent_id: String,
    /// `superseded` only: the new version.
    pub superseded_by: Option<String>,
    /// `lifecycle` only: the stored event, as `registry_state` serves it.
    pub event: Option<Value>,
    /// The event receipt (`lifecycle`) or the new version's registry receipt
    /// (`superseded`, `key_revocation`).
    pub receipt: Value,
}

/// One stored feed row.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedRow {
    pub seq: u64,
    pub kind: ChangeKind,
    pub ctx_id: String,
    pub lineage_id: String,
    pub agent_id: String,
    pub superseded_by: Option<String>,
    pub event: Option<Value>,
    pub receipt: Value,
    /// When the row was appended, by the backend's clock. Drives retention
    /// only; the served `recorded_at` comes from the receipt.
    pub appended_at_ms: i64,
}

impl FeedRow {
    /// A row from its stored columns. `kind` is the stored wire form.
    #[allow(clippy::too_many_arguments)]
    pub fn from_columns(
        seq: i64,
        kind: &str,
        ctx_id: String,
        lineage_id: String,
        agent_id: String,
        superseded_by: Option<String>,
        event: Option<Value>,
        receipt: Value,
        appended_at_ms: i64,
    ) -> Result<Self, AcdpError> {
        let seq = u64::try_from(seq).map_err(|_| {
            AcdpError::RegistryInternal(format!("stored change seq {seq} is negative"))
        })?;
        let kind = ChangeKind::parse(kind).map_err(|e| {
            AcdpError::RegistryInternal(format!("stored change {seq} has a bad kind: {e}"))
        })?;
        Ok(Self {
            seq,
            kind,
            ctx_id,
            lineage_id,
            agent_id,
            superseded_by,
            event,
            receipt,
            appended_at_ms,
        })
    }

    /// The `acdp-change/1` entry this row serves (RFC-ACDP-0018 §4).
    ///
    /// The evidence is re-serialized through its typed form, the same path
    /// `registry_state` takes, so an entry's event and receipt are the bytes
    /// a consumer reads from the context itself. A row that no longer parses
    /// is a `RegistryInternal` error, never a silently dropped entry.
    pub fn to_change(&self) -> Result<Change, AcdpError> {
        let corrupt = |e: AcdpError| {
            AcdpError::RegistryInternal(format!("stored change {} is malformed: {e}", self.seq))
        };
        let agent = AgentDid::parse(&self.agent_id).map_err(corrupt)?;
        match self.kind {
            ChangeKind::Lifecycle => {
                let raw = self.event.as_ref().ok_or_else(|| {
                    corrupt(AcdpError::RegistryInternal(
                        "lifecycle entry without an event".into(),
                    ))
                })?;
                let event = LifecycleEvent::from_value(raw).map_err(corrupt)?;
                let event = serde_json::to_value(&event)
                    .map_err(|e| corrupt(AcdpError::RegistryInternal(e.to_string())))?;
                let receipt = EventReceipt::from_value(&self.receipt).map_err(corrupt)?;
                let lineage = LineageId::parse(&self.lineage_id).map_err(corrupt)?;
                Change::lifecycle(self.seq, lineage, agent, event, &receipt).map_err(corrupt)
            }
            ChangeKind::Superseded => {
                let ctx = CtxId::parse(&self.ctx_id).map_err(corrupt)?;
                let receipt = RegistryReceipt::from_value(&self.receipt).map_err(corrupt)?;
                Change::superseded(self.seq, ctx, agent, &receipt).map_err(corrupt)
            }
            ChangeKind::KeyRevocation => {
                let receipt = RegistryReceipt::from_value(&self.receipt).map_err(corrupt)?;
                Change::key_revocation(self.seq, agent, &receipt).map_err(corrupt)
            }
        }
    }
}

/// A feed scan, with the §6 filter pushed down.
///
/// A row matches when it passes the id clause and the kind clause:
///
/// - id clause: `ctx_id ∈ ctx_ids` OR `lineage_id ∈ lineage_ids` OR
///   `agent_id ∈ agent_ids` OR (`kind = key_revocation` AND
///   `agent_id ∈ revocation_agents`). Empty lists drop out of the OR; when
///   all four are empty the clause matches every row.
/// - kind clause: `kind ∈ kinds`, or every kind when `kinds` is empty.
///
/// `revocation_agents` is how a `ctx_id` watch also matches key revocations
/// of that context's producer (§6). The caller resolves producers only for
/// contexts the requester may see.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedQuery {
    /// Return rows with `seq > after`.
    pub after: u64,
    pub ctx_ids: Vec<String>,
    pub lineage_ids: Vec<String>,
    pub agent_ids: Vec<String>,
    pub revocation_agents: Vec<String>,
    pub kinds: Vec<ChangeKind>,
    /// At most this many matching rows. Zero is treated as one.
    pub scan_limit: u32,
}

impl FeedQuery {
    /// The effective row cap.
    pub fn limit(&self) -> u32 {
        self.scan_limit.max(1)
    }

    fn has_id_clause(&self) -> bool {
        !(self.ctx_ids.is_empty()
            && self.lineage_ids.is_empty()
            && self.agent_ids.is_empty()
            && self.revocation_agents.is_empty())
    }

    /// The filter part of the query (not `after`), for backends that
    /// evaluate it in memory. The SQL backends build the same predicate.
    pub fn matches(&self, row: &FeedRow) -> bool {
        let has = |list: &[String], v: &str| list.iter().any(|x| x == v);
        let id_ok = !self.has_id_clause()
            || has(&self.ctx_ids, &row.ctx_id)
            || has(&self.lineage_ids, &row.lineage_id)
            || has(&self.agent_ids, &row.agent_id)
            || (row.kind == ChangeKind::KeyRevocation
                && has(&self.revocation_agents, &row.agent_id));
        let kind_ok = self.kinds.is_empty() || self.kinds.contains(&row.kind);
        id_ok && kind_ok
    }
}

/// The feed's position markers, read in one snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeedHead {
    /// The highest position ever assigned: `max(MAX(seq), pruned_through)`.
    /// Zero for a feed that has never had a row.
    pub head: u64,
    /// Every row with `seq <= pruned_through` has been pruned (§5.4).
    pub pruned_through: u64,
}

/// The result of one [`FeedQuery`] scan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedPage {
    /// Matching rows with `seq > after`, ascending, at most `scan_limit`.
    pub rows: Vec<FeedRow>,
    /// The position the scan covered through: the last row's seq when the
    /// cap was hit, else the head.
    pub last_scanned: u64,
    /// No matching row exists past `last_scanned` (as of the snapshot).
    pub reached_head: bool,
    pub head: u64,
    pub pruned_through: u64,
}

impl FeedPage {
    /// Assemble a page from a capped scan. `rows` must be the matching rows
    /// past `query.after`, ascending, at most `query.limit()` of them.
    pub fn from_scan(query: &FeedQuery, rows: Vec<FeedRow>, head: FeedHead) -> Self {
        let reached_head = rows.len() < query.limit() as usize;
        let last_scanned = match rows.last() {
            Some(last) if !reached_head => last.seq,
            _ => head.head.max(query.after),
        };
        Self {
            rows,
            last_scanned,
            reached_head,
            head: head.head,
            pruned_through: head.pruned_through,
        }
    }
}

/// The superseded predecessor of a publish.
#[derive(Debug, Clone, PartialEq)]
pub struct Predecessor {
    pub ctx_id: String,
    /// The predecessor body's `agent_id`.
    pub agent_id: String,
}

/// The `lifecycle` row for an event just appended to `body`'s context
/// (RFC-ACDP-0018 §4), or `None` when the event has no receipt.
///
/// Without a receipt there is no evidence to serve, and the store must not
/// invent one; a registry that mints no event receipts cannot advertise the
/// changes profile.
pub fn lifecycle_append(
    event: &LifecycleEvent,
    body: &Body,
    receipt: Option<&Value>,
) -> Result<Option<FeedAppend>, AcdpError> {
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    let event = serde_json::to_value(event)
        .map_err(|e| AcdpError::RegistryInternal(format!("encode lifecycle event: {e}")))?;
    let append = FeedAppend {
        kind: ChangeKind::Lifecycle,
        ctx_id: body.ctx_id.as_str().to_string(),
        lineage_id: body.lineage_id.as_str().to_string(),
        agent_id: body.agent_id.as_str().to_string(),
        superseded_by: None,
        event: Some(event),
        receipt: receipt.clone(),
    };
    check_servable(&append)?;
    Ok(Some(append))
}

/// The rows for an accepted publish of `body` (RFC-ACDP-0018 §4), in append
/// order: `superseded` for the predecessor (if any), then `key_revocation`
/// (if `body` is a key revocation). A plain first publish appends nothing,
/// and so does any publish without a registry receipt.
pub fn feed_appends_for_publish(
    body: &Body,
    receipt: Option<&Value>,
    predecessor: Option<&Predecessor>,
) -> Result<Vec<FeedAppend>, AcdpError> {
    let Some(receipt) = receipt else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    if let Some(prev) = predecessor {
        out.push(FeedAppend {
            kind: ChangeKind::Superseded,
            ctx_id: prev.ctx_id.clone(),
            lineage_id: body.lineage_id.as_str().to_string(),
            agent_id: prev.agent_id.clone(),
            superseded_by: Some(body.ctx_id.as_str().to_string()),
            event: None,
            receipt: receipt.clone(),
        });
    }
    if body.context_type.is_key_revocation() {
        out.push(FeedAppend {
            kind: ChangeKind::KeyRevocation,
            ctx_id: body.ctx_id.as_str().to_string(),
            lineage_id: body.lineage_id.as_str().to_string(),
            agent_id: revoked_key_controller(body)?,
            superseded_by: None,
            event: None,
            receipt: receipt.clone(),
        });
    }
    for a in &out {
        check_servable(a)?;
    }
    Ok(out)
}

/// `metadata.revoked_key_controller` when present, else the body's
/// `agent_id` (RFC-ACDP-0014 §4, RFC-ACDP-0018 §4).
fn revoked_key_controller(body: &Body) -> Result<String, AcdpError> {
    let declared = body
        .metadata
        .as_ref()
        .and_then(|m| m.get("revoked_key_controller"));
    match declared {
        None | Some(Value::Null) => Ok(body.agent_id.as_str().to_string()),
        Some(Value::String(s)) => Ok(AgentDid::parse(s)?.as_str().to_string()),
        Some(other) => Err(AcdpError::SchemaViolation(format!(
            "metadata.revoked_key_controller must be a DID string, got {other}"
        ))),
    }
}

/// Refuse to persist a row the read path could not serve: a malformed row
/// would fail every `/changes` page that reaches it, for its whole
/// retention. The publish or event it belongs to fails instead.
fn check_servable(append: &FeedAppend) -> Result<(), AcdpError> {
    FeedRow {
        seq: 1,
        kind: append.kind,
        ctx_id: append.ctx_id.clone(),
        lineage_id: append.lineage_id.clone(),
        agent_id: append.agent_id.clone(),
        superseded_by: append.superseded_by.clone(),
        event: append.event.clone(),
        receipt: append.receipt.clone(),
        appended_at_ms: 0,
    }
    .to_change()
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp::types::primitives::ContextType;
    use acdp::types::ReceiptSigner;
    use chrono::{TimeZone, Utc};

    const AGENT: &str = "did:web:agents.test:p";

    fn body(ct: ContextType, metadata: Option<Value>) -> Body {
        let p = acdp::producer::Producer::new(
            acdp::crypto::SigningKey::from_bytes(&[11u8; 32]),
            AgentDid::new(AGENT.to_string()),
            format!("{AGENT}#key-1"),
        );
        let mut req = p.publish_request().title("feed fixture").context_type(ct);
        if let Some(m) = metadata {
            req = req.metadata(m);
        }
        let req = req.build().expect("valid publish request");
        let ctx_id = CtxId("acdp://r.example/00000000-0000-4000-8000-000000000001".into());
        let lineage_id = acdp::crypto::derive_lineage_id(&ctx_id);
        Body::from_publish_request(
            &req,
            ctx_id,
            lineage_id,
            "r.example".to_string(),
            Utc.timestamp_millis_opt(1_767_225_600_000).unwrap(),
        )
    }

    fn signer() -> ReceiptSigner {
        ReceiptSigner::new(
            acdp::crypto::SigningKey::from_bytes(&[5u8; 32]),
            "did:web:r.example",
            "did:web:r.example#receipt-key-1",
        )
        .expect("signer")
    }

    fn registry_receipt(b: &Body) -> Value {
        let r = signer()
            .mint(
                &b.ctx_id,
                &b.lineage_id,
                &b.origin_registry,
                b.created_at,
                &b.content_hash,
                &format!("sha256:{}", "c".repeat(64)),
            )
            .expect("mint");
        serde_json::to_value(r).unwrap()
    }

    fn event(b: &Body) -> LifecycleEvent {
        LifecycleEvent::new(
            "00000000-0000-4000-8000-0000000000e1".to_string(),
            b.ctx_id.clone(),
            acdp::types::lifecycle::LifecycleEventType::Retracted,
            Utc.timestamp_millis_opt(1_767_225_601_000).unwrap(),
            AgentDid::new(AGENT.to_string()),
            None,
        )
        .expect("valid event")
    }

    fn row(seq: u64, kind: ChangeKind, ctx: &str, lineage: &str, agent: &str) -> FeedRow {
        FeedRow {
            seq,
            kind,
            ctx_id: ctx.into(),
            lineage_id: lineage.into(),
            agent_id: agent.into(),
            superseded_by: None,
            event: None,
            receipt: Value::Null,
            appended_at_ms: 0,
        }
    }

    #[test]
    fn no_receipt_means_no_rows() {
        let b = body(ContextType::KeyRevocation, None);
        assert_eq!(lifecycle_append(&event(&b), &b, None).unwrap(), None);
        let prev = Predecessor {
            ctx_id: "acdp://r.example/00000000-0000-4000-8000-000000000000".into(),
            agent_id: AGENT.into(),
        };
        assert!(feed_appends_for_publish(&b, None, Some(&prev))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_plain_first_publish_appends_nothing() {
        let b = body(ContextType::DataSnapshot, None);
        let r = registry_receipt(&b);
        assert!(feed_appends_for_publish(&b, Some(&r), None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_lifecycle_row_carries_the_event_and_receipt() {
        let b = body(ContextType::DataSnapshot, None);
        let e = event(&b);
        let raw = serde_json::to_value(&e).unwrap();
        let receipt = signer()
            .mint_event_receipt(
                &b.ctx_id,
                &raw,
                0,
                Utc.timestamp_millis_opt(1_767_225_602_000).unwrap(),
            )
            .unwrap();
        let receipt = serde_json::to_value(receipt).unwrap();
        let a = lifecycle_append(&e, &b, Some(&receipt)).unwrap().unwrap();
        assert_eq!(a.kind, ChangeKind::Lifecycle);
        assert_eq!(a.ctx_id, b.ctx_id.as_str());
        assert_eq!(a.lineage_id, b.lineage_id.as_str());
        assert_eq!(a.agent_id, AGENT);
        assert_eq!(a.event, Some(raw));
        assert_eq!(a.receipt, receipt);
    }

    #[test]
    fn a_receipt_that_is_not_an_event_receipt_is_refused_before_persisting() {
        let b = body(ContextType::DataSnapshot, None);
        let wrong = registry_receipt(&b);
        let err = lifecycle_append(&event(&b), &b, Some(&wrong)).expect_err("not servable");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
    }

    #[test]
    fn supersession_then_revocation_in_append_order() {
        let b = body(ContextType::KeyRevocation, None);
        let r = registry_receipt(&b);
        let prev = Predecessor {
            ctx_id: "acdp://r.example/00000000-0000-4000-8000-000000000000".into(),
            agent_id: "did:web:agents.test:older".into(),
        };
        let rows = feed_appends_for_publish(&b, Some(&r), Some(&prev)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, ChangeKind::Superseded);
        assert_eq!(rows[0].ctx_id, prev.ctx_id);
        assert_eq!(
            rows[0].agent_id, prev.agent_id,
            "the older version's producer"
        );
        assert_eq!(rows[0].superseded_by.as_deref(), Some(b.ctx_id.as_str()));
        assert_eq!(rows[0].lineage_id, b.lineage_id.as_str());
        assert_eq!(rows[1].kind, ChangeKind::KeyRevocation);
        assert_eq!(rows[1].ctx_id, b.ctx_id.as_str());
        assert_eq!(rows[1].agent_id, AGENT, "controller falls back to agent_id");
        assert!(rows.iter().all(|a| a.receipt == r));
    }

    #[test]
    fn the_declared_controller_wins_and_must_be_a_did() {
        let other = "did:web:agents.test:controller";
        for (meta, want) in [
            (
                serde_json::json!({ "revoked_key_controller": other }),
                Some(other),
            ),
            (
                serde_json::json!({ "revoked_key_controller": null }),
                Some(AGENT),
            ),
            (serde_json::json!({ "reason": "x" }), Some(AGENT)),
            (serde_json::json!({ "revoked_key_controller": 7 }), None),
            (
                serde_json::json!({ "revoked_key_controller": "not a did" }),
                None,
            ),
        ] {
            let b = body(ContextType::KeyRevocation, Some(meta.clone()));
            let r = registry_receipt(&b);
            let got = feed_appends_for_publish(&b, Some(&r), None);
            match want {
                Some(want) => assert_eq!(got.unwrap()[0].agent_id, want, "{meta}"),
                None => assert!(got.is_err(), "{meta} must be refused"),
            }
        }
    }

    #[test]
    fn rows_round_trip_to_valid_changes() {
        let b = body(ContextType::KeyRevocation, None);
        let r = registry_receipt(&b);
        let prev = Predecessor {
            ctx_id: "acdp://r.example/00000000-0000-4000-8000-000000000000".into(),
            agent_id: AGENT.into(),
        };
        for (i, a) in feed_appends_for_publish(&b, Some(&r), Some(&prev))
            .unwrap()
            .into_iter()
            .enumerate()
        {
            let row = FeedRow {
                seq: 10 + i as u64,
                kind: a.kind,
                ctx_id: a.ctx_id,
                lineage_id: a.lineage_id,
                agent_id: a.agent_id,
                superseded_by: a.superseded_by,
                event: a.event,
                receipt: a.receipt,
                appended_at_ms: 0,
            };
            let change = row.to_change().expect("servable");
            assert_eq!(change.seq(), row.seq);
        }
    }

    #[test]
    fn a_corrupt_stored_row_is_an_internal_error_naming_its_seq() {
        let mut bad = row(42, ChangeKind::Superseded, "acdp://x/y", "l", AGENT);
        bad.receipt = serde_json::json!({ "not": "a receipt" });
        let err = bad.to_change().expect_err("corrupt");
        assert!(
            matches!(&err, AcdpError::RegistryInternal(m) if m.contains("42")),
            "{err:?}"
        );
        let mut no_event = row(43, ChangeKind::Lifecycle, "acdp://x/y", "l", AGENT);
        no_event.receipt = Value::Null;
        assert!(no_event.to_change().is_err());
    }

    #[test]
    fn matches_is_or_across_ids_and_with_kind() {
        use ChangeKind::*;
        let lc = row(1, Lifecycle, "c1", "l1", "a1");
        let kr = row(2, KeyRevocation, "c2", "l2", "a2");
        let q = |f: fn(&mut FeedQuery)| {
            let mut q = FeedQuery::default();
            f(&mut q);
            q
        };
        let all = FeedQuery::default();
        assert!(
            all.matches(&lc) && all.matches(&kr),
            "no filter matches all"
        );
        let by_ctx = q(|q| q.ctx_ids = vec!["c1".into()]);
        assert!(by_ctx.matches(&lc) && !by_ctx.matches(&kr));
        let or = q(|q| {
            q.ctx_ids = vec!["c1".into()];
            q.agent_ids = vec!["a2".into()];
        });
        assert!(or.matches(&lc) && or.matches(&kr), "ids are OR'd");
        let and = q(|q| {
            q.ctx_ids = vec!["c1".into()];
            q.agent_ids = vec!["a2".into()];
            q.kinds = vec![KeyRevocation];
        });
        assert!(!and.matches(&lc) && and.matches(&kr), "kind is AND'd");
        let by_lineage = q(|q| q.lineage_ids = vec!["l2".into()]);
        assert!(!by_lineage.matches(&lc) && by_lineage.matches(&kr));
        let rev = q(|q| q.revocation_agents = vec!["a1".into(), "a2".into()]);
        assert!(
            !rev.matches(&lc) && rev.matches(&kr),
            "revocation agents match only key_revocation rows"
        );
        let kinds_only = q(|q| q.kinds = vec![Lifecycle]);
        assert!(kinds_only.matches(&lc) && !kinds_only.matches(&kr));
    }

    #[test]
    fn from_scan_reports_coverage() {
        let q = FeedQuery {
            after: 5,
            scan_limit: 2,
            ..Default::default()
        };
        let head = FeedHead {
            head: 20,
            pruned_through: 3,
        };
        let r = |s| row(s, ChangeKind::Lifecycle, "c", "l", "a");
        let full = FeedPage::from_scan(&q, vec![r(7), r(9)], head);
        assert!(!full.reached_head);
        assert_eq!(
            full.last_scanned, 9,
            "a full page covers through its last row"
        );
        let short = FeedPage::from_scan(&q, vec![r(7)], head);
        assert!(short.reached_head);
        assert_eq!(
            short.last_scanned, 20,
            "a short page covers through the head"
        );
        let empty = FeedPage::from_scan(&q, vec![], head);
        assert_eq!((empty.reached_head, empty.last_scanned), (true, 20));
        let past = FeedPage::from_scan(
            &FeedQuery {
                after: 30,
                ..q.clone()
            },
            vec![],
            head,
        );
        assert_eq!(past.last_scanned, 30, "never moves a position backwards");
        assert_eq!((past.head, past.pruned_through), (20, 3));
        assert_eq!(FeedQuery::default().limit(), 1, "zero is treated as one");
    }
}
