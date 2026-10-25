//! RFC-ACDP-0017 §10 event receipts: the store-side pieces every backend
//! shares — the minter seam, the §10.1 clock clamp, the fill-on-append
//! planner, and the §10.2 read rule.
//!
//! Receipts are minted inside the backend's own lifecycle transaction
//! (under the same lock that serializes appends), so an accepted event and
//! its receipt commit or roll back together. The SDK's
//! `RegistryStore::commit_lifecycle_event` has no signer, which is why the
//! minter is attached to the store at construction rather than passed per
//! call.

use acdp::error::AcdpError;
use acdp::types::body::FullContext;
use acdp::types::lifecycle::LifecycleEvent;
use acdp::types::primitives::CtxId;
use acdp::types::{EventReceipt, ReceiptSigner};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;

/// Metric names (the `acdp_registry_*` family in
/// `acdp-registry-core::metrics`). Defined here because the store emits
/// them and core depends on the store, not the reverse.
pub const EVENT_RECEIPTS_MINTED_TOTAL: &str = "acdp_registry_event_receipts_minted_total";
pub const EVENT_RECEIPTS_INCOMPLETE_TOTAL: &str = "acdp_registry_event_receipts_incomplete_total";
pub const EVENT_RECEIPTS_SWEEP_SKIPPED_TOTAL: &str =
    "acdp_registry_event_receipts_sweep_skipped_total";

/// Signs `acdp-event-receipt/1` receipts (RFC-ACDP-0017 §10.1). The store
/// owns the clock rules and passes a final `accepted_at`; the minter only
/// signs.
pub trait EventReceiptMinter: Send + Sync {
    fn mint(
        &self,
        ctx_id: &CtxId,
        raw_event: &Value,
        index: u64,
        accepted_at: DateTime<Utc>,
    ) -> Result<EventReceipt, AcdpError>;
}

impl EventReceiptMinter for ReceiptSigner {
    fn mint(
        &self,
        ctx_id: &CtxId,
        raw_event: &Value,
        index: u64,
        accepted_at: DateTime<Utc>,
    ) -> Result<EventReceipt, AcdpError> {
        self.mint_event_receipt(ctx_id, raw_event, index, accepted_at)
    }
}

/// One persisted receipt slot, as decoded from storage.
#[derive(Debug, Clone, PartialEq)]
pub enum StoredReceipt {
    /// No receipt was minted (an event accepted before minting was on).
    Null,
    /// A well-formed receipt bound to this slot's event and position.
    /// Boxed: the receipt dwarfs the other variants.
    Valid(Box<EventReceipt>),
    /// Present but undecodable, or bound to a different event/position.
    Corrupt,
}

impl StoredReceipt {
    /// Decode a stored receipt for the event at `index`. A value that
    /// fails the closed schema, or names another event or position, is
    /// `Corrupt` (logged) — never an error that would fail the read.
    pub fn decode(value: Option<&Value>, event: &LifecycleEvent, index: u64) -> Self {
        let Some(value) = value else {
            return Self::Null;
        };
        match EventReceipt::from_value(value) {
            Ok(r)
                if r.event_id == event.event_id && r.index == index && r.ctx_id == event.ctx_id =>
            {
                Self::Valid(Box::new(r))
            }
            Ok(_) => {
                tracing::error!(
                    ctx_id = %event.ctx_id,
                    event_id = %event.event_id,
                    index,
                    "stored event receipt is bound to a different event or position"
                );
                Self::Corrupt
            }
            Err(e) => {
                tracing::error!(
                    ctx_id = %event.ctx_id,
                    event_id = %event.event_id,
                    index,
                    error = %e,
                    "stored event receipt does not decode"
                );
                Self::Corrupt
            }
        }
    }

    pub fn into_option(self) -> Option<EventReceipt> {
        match self {
            Self::Valid(r) => Some(*r),
            Self::Null | Self::Corrupt => None,
        }
    }
}

/// Round up to the next whole millisecond (identity when already
/// ms-aligned).
pub fn ceil_ms(dt: DateTime<Utc>) -> DateTime<Utc> {
    let t = acdp::time::trunc_ms(dt);
    if t == dt {
        t
    } else {
        t + TimeDelta::milliseconds(1)
    }
}

/// RFC-ACDP-0017 §10.1 `accepted_at`: the registry clock in canonical
/// milliseconds, never backdated, lifted to at least the context's
/// `created_at` (step 9) and the previous receipt's `accepted_at`
/// (step 8). `created_at` is rounded *up*: it may carry sub-ms precision
/// and the mint truncates, so truncating it would land below itself.
pub fn clamp_accepted_at(
    now: DateTime<Utc>,
    created_at: DateTime<Utc>,
    prev: Option<DateTime<Utc>>,
) -> DateTime<Utc> {
    let mut at = acdp::time::trunc_ms(now).max(ceil_ms(created_at));
    if let Some(prev) = prev {
        at = at.max(prev);
    }
    at
}

/// What a lifecycle commit must write: receipts for prior NULL slots
/// (fill-on-append, RFC-ACDP-0017 §10.2) and the new event's receipt.
#[derive(Debug)]
pub struct ReceiptPlan {
    /// `(position, receipt)` for each prior NULL slot to fill, in order.
    pub fills: Vec<(usize, EventReceipt)>,
    /// The receipt for the event being appended (position `prior.len()`).
    pub new: EventReceipt,
    /// The prior history has a hole (a NULL before a receipted slot, or a
    /// corrupt slot), so nothing was filled and the list stays incomplete.
    pub hole: bool,
}

/// True when the slots cannot be completed without violating §10.3 step 8:
/// a corrupt slot, or a NULL preceding a receipted one.
pub fn has_hole(prior: &[StoredReceipt]) -> bool {
    let mut seen_null = false;
    for slot in prior {
        match slot {
            StoredReceipt::Corrupt => return true,
            StoredReceipt::Null => seen_null = true,
            StoredReceipt::Valid(_) if seen_null => return true,
            StoredReceipt::Valid(_) => {}
        }
    }
    false
}

/// Plan the receipts for appending `new_event` after `prior` (the
/// context's events in acceptance order, each with its stored slot).
///
/// Call under the backend's append lock, with `now` read *after* the lock
/// was taken (§10.1: never backdated by the lock wait).
///
/// Without a hole, every trailing NULL is filled in order and then the new
/// event is receipted, each clamp chained from the last. With a hole,
/// nothing is filled (filling would need an `accepted_at` above a later
/// receipt's, a permanent step-8 failure): the error is logged and counted,
/// and only the new event is receipted, above every existing receipt.
pub fn plan_event_receipts(
    minter: &dyn EventReceiptMinter,
    ctx_id: &CtxId,
    created_at: DateTime<Utc>,
    prior: &[(LifecycleEvent, StoredReceipt)],
    new_event: &LifecycleEvent,
    now: DateTime<Utc>,
) -> Result<ReceiptPlan, AcdpError> {
    let hole = slots_have_hole(prior);
    let mut prev = latest_accepted_at(prior);
    let fills = if hole {
        tracing::error!(
            %ctx_id,
            "lifecycle event receipts have a hole (a missing or corrupt receipt before a \
             receipted event); not filling it — event_receipts stays omitted until repaired"
        );
        Vec::new()
    } else {
        fill_nulls(minter, ctx_id, created_at, prior, now, &mut prev)?
    };
    let at = clamp_accepted_at(now, created_at, prev);
    let new = minter.mint(ctx_id, &event_value(new_event)?, prior.len() as u64, at)?;
    Ok(ReceiptPlan { fills, new, hole })
}

fn slots_have_hole(prior: &[(LifecycleEvent, StoredReceipt)]) -> bool {
    let slots: Vec<StoredReceipt> = prior.iter().map(|(_, s)| s.clone()).collect();
    has_hole(&slots)
}

fn latest_accepted_at(prior: &[(LifecycleEvent, StoredReceipt)]) -> Option<DateTime<Utc>> {
    prior
        .iter()
        .filter_map(|(_, s)| match s {
            StoredReceipt::Valid(r) => Some(r.accepted_at),
            _ => None,
        })
        .max()
}

/// Receipt every NULL slot of a hole-free history, in order, each clamp
/// chained from the last (`prev` is advanced past every minted receipt).
fn fill_nulls(
    minter: &dyn EventReceiptMinter,
    ctx_id: &CtxId,
    created_at: DateTime<Utc>,
    prior: &[(LifecycleEvent, StoredReceipt)],
    now: DateTime<Utc>,
    prev: &mut Option<DateTime<Utc>>,
) -> Result<Vec<(usize, EventReceipt)>, AcdpError> {
    let mut fills = Vec::new();
    for (i, (event, slot)) in prior.iter().enumerate() {
        if matches!(slot, StoredReceipt::Null) {
            let at = clamp_accepted_at(now, created_at, *prev);
            let r = minter.mint(ctx_id, &event_value(event)?, i as u64, at)?;
            *prev = Some(r.accepted_at);
            fills.push((i, r));
        }
    }
    Ok(fills)
}

/// What the startup sweep must do for one context's history.
#[derive(Debug)]
pub enum BackfillPlan {
    /// Every slot already holds a valid receipt.
    Complete,
    /// `(position, receipt)` for each trailing NULL slot, in order.
    Fill(Vec<(usize, EventReceipt)>),
    /// A NULL precedes a receipted slot, or a slot is corrupt: filling
    /// would break §10.3 step 8, so nothing is written (logged).
    Hole,
}

/// Plan the startup sweep for one context (RFC-ACDP-0017 §10.2): the same
/// rules as fill-on-append, without a new event. Call under the backend's
/// append lock, with `now` read after it was taken.
pub fn plan_backfill(
    minter: &dyn EventReceiptMinter,
    ctx_id: &CtxId,
    created_at: DateTime<Utc>,
    prior: &[(LifecycleEvent, StoredReceipt)],
    now: DateTime<Utc>,
) -> Result<BackfillPlan, AcdpError> {
    if slots_have_hole(prior) {
        tracing::error!(
            %ctx_id,
            "event receipt sweep skipped a context whose receipts have a hole (a missing or \
             corrupt receipt before a receipted event); event_receipts stays omitted"
        );
        return Ok(BackfillPlan::Hole);
    }
    let mut prev = latest_accepted_at(prior);
    let fills = fill_nulls(minter, ctx_id, created_at, prior, now, &mut prev)?;
    Ok(if fills.is_empty() {
        BackfillPlan::Complete
    } else {
        BackfillPlan::Fill(fills)
    })
}

/// Contexts the sweep examines per page (keyset-paginated by `ctx_id`, so
/// a skipped context is never revisited in the same run).
pub const SWEEP_BATCH: i64 = 200;

/// What [`crate::ExtendedRegistryStore::backfill_event_receipts`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Contexts whose trailing NULL receipts were filled.
    pub contexts_filled: u64,
    /// Receipts minted across them.
    pub receipts_filled: u64,
    /// Contexts left incomplete: a hole, or (defensively) no context row.
    pub contexts_skipped: u64,
}

impl SweepReport {
    /// Count one context's committed fill. Call after its commit.
    pub fn record_filled(&mut self, receipts: usize) {
        self.contexts_filled += 1;
        self.receipts_filled += receipts as u64;
        metrics::counter!(EVENT_RECEIPTS_MINTED_TOTAL).increment(receipts as u64);
    }

    /// Count one context the sweep left incomplete.
    pub fn record_skipped(&mut self) {
        self.contexts_skipped += 1;
        metrics::counter!(EVENT_RECEIPTS_SWEEP_SKIPPED_TOTAL).increment(1);
    }
}

impl ReceiptPlan {
    /// Count the plan's receipts. Call only after the transaction that
    /// persisted them committed, so a rolled-back append counts nothing.
    pub fn record_committed(&self) {
        metrics::counter!(EVENT_RECEIPTS_MINTED_TOTAL).increment(self.fills.len() as u64 + 1);
        if self.hole {
            metrics::counter!(EVENT_RECEIPTS_SWEEP_SKIPPED_TOTAL).increment(1);
        }
    }
}

/// The event exactly as it is persisted and served (RFC-ACDP-0017 §10.1:
/// `event_hash` covers the stored event including its `signature`).
pub fn event_value(event: &LifecycleEvent) -> Result<Value, AcdpError> {
    serde_json::to_value(event)
        .map_err(|e| AcdpError::RegistryInternal(format!("encode lifecycle event: {e}")))
}

/// Serialize a receipt for storage.
pub fn receipt_value(receipt: &EventReceipt) -> Result<Value, AcdpError> {
    serde_json::to_value(receipt)
        .map_err(|e| AcdpError::RegistryInternal(format!("encode event receipt: {e}")))
}

/// RFC-ACDP-0017 §10.2 read rule. With `minting` off, never touch the
/// context (byte-identical to a registry without event receipts). With it
/// on, serve `event_receipts` only when every served lifecycle event has
/// one — the array is parallel and closed, with no placeholder — and
/// otherwise omit it, warn and count.
pub fn attach_event_receipts(
    ctx: &mut FullContext,
    receipts: Vec<Option<EventReceipt>>,
    minting: bool,
) {
    if !minting {
        return;
    }
    let events = ctx
        .registry_state
        .lifecycle_events
        .as_ref()
        .map_or(0, Vec::len);
    let complete = receipts.len() == events && receipts.iter().all(Option::is_some);
    if events == 0 || !complete {
        if events != 0 {
            tracing::warn!(
                ctx_id = %ctx.body.ctx_id,
                events,
                receipts = receipts.iter().filter(|r| r.is_some()).count(),
                "event_receipts incomplete; omitting the member (RFC-ACDP-0017 §10.2)"
            );
            metrics::counter!(EVENT_RECEIPTS_INCOMPLETE_TOTAL).increment(1);
        }
        // `set_event_receipts(vec![])` removes the member.
        let _ = ctx.registry_state.set_event_receipts(Vec::new());
        return;
    }
    let receipts: Vec<EventReceipt> = receipts.into_iter().flatten().collect();
    if let Err(e) = ctx.registry_state.set_event_receipts(receipts) {
        tracing::error!(ctx_id = %ctx.body.ctx_id, error = %e, "encode event_receipts");
        let _ = ctx.registry_state.set_event_receipts(Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(ms: i64) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(ms).unwrap()
    }

    #[test]
    fn clamp_uses_now_when_latest() {
        assert_eq!(
            clamp_accepted_at(t(5_000), t(1_000), Some(t(2_000))),
            t(5_000)
        );
    }

    #[test]
    fn clamp_lifts_to_created_at_when_now_is_earlier() {
        assert_eq!(clamp_accepted_at(t(1_000), t(3_000), None), t(3_000));
    }

    #[test]
    fn clamp_lifts_to_prev_when_clock_went_backwards() {
        assert_eq!(
            clamp_accepted_at(t(1_000), t(500), Some(t(4_000))),
            t(4_000)
        );
    }

    #[test]
    fn clamp_truncates_sub_ms_now() {
        let now = t(1_000) + TimeDelta::microseconds(999);
        assert_eq!(clamp_accepted_at(now, t(0), None), t(1_000));
    }

    #[test]
    fn clamp_rounds_sub_ms_created_at_up() {
        let created = t(2_000) + TimeDelta::microseconds(1);
        let at = clamp_accepted_at(t(2_000), created, None);
        assert_eq!(at, t(2_001));
        assert!(
            at >= created,
            "§10.3 step 9 holds after the mint's trunc_ms"
        );
        assert_eq!(acdp::time::trunc_ms(at), at);
    }

    #[test]
    fn ceil_ms_is_identity_on_aligned_values() {
        assert_eq!(ceil_ms(t(7)), t(7));
    }

    fn r() -> StoredReceipt {
        // Content is irrelevant to `has_hole`; only the variant matters.
        StoredReceipt::Valid(Box::new(EventReceipt {
            receipt_type: "acdp-event-receipt/1".into(),
            registry_did: "did:web:r.example".into(),
            ctx_id: CtxId::parse("acdp://r.example/00000000-0000-4000-8000-000000000000").unwrap(),
            event_id: "e".into(),
            event_hash: "sha256:00".into(),
            index: 0,
            accepted_at: t(0),
            signature: acdp::types::Signature {
                algorithm: "ed25519".into(),
                key_id: "k".into(),
                value: "v".into(),
            },
        }))
    }

    #[test]
    fn hole_rule() {
        use StoredReceipt::{Corrupt, Null};
        assert!(!has_hole(&[]));
        assert!(!has_hole(&[Null, Null]));
        assert!(!has_hole(&[r(), r(), Null]), "a NULL tail is fillable");
        assert!(has_hole(&[Null, r()]), "a NULL before a receipt is a hole");
        assert!(has_hole(&[r(), Null, r()]));
        assert!(
            has_hole(&[r(), Corrupt]),
            "a corrupt slot is never repaired"
        );
    }

    fn ctx_with_events(n: usize) -> FullContext {
        let id = "acdp://r.example/00000000-0000-4000-8000-000000000000";
        let events: Vec<LifecycleEvent> = (0..n)
            .map(|i| {
                serde_json::from_value(serde_json::json!({
                    "event_id": format!("00000000-0000-4000-8000-00000000000{i}"),
                    "ctx_id": id,
                    "event_type": if i % 2 == 0 { "retracted" } else { "republished" },
                    "occurred_at": "2026-01-01T00:00:00.000Z",
                    "actor": "did:web:agents.test:actor",
                }))
                .expect("valid lifecycle event")
            })
            .collect();
        let p = acdp::producer::Producer::new(
            acdp::crypto::SigningKey::from_bytes(&[11u8; 32]),
            acdp::types::primitives::AgentDid::new("did:web:agents.test:p".to_string()),
            "did:web:agents.test:p#key-1".to_string(),
        );
        let req = p
            .publish_request()
            .title("attach fixture")
            .context_type(acdp::types::primitives::ContextType::DataSnapshot)
            .build()
            .expect("valid publish request");
        let ctx_id = CtxId(id.to_string());
        let lineage_id = acdp::crypto::derive_lineage_id(&ctx_id);
        let body = acdp::types::body::Body::from_publish_request(
            &req,
            ctx_id,
            lineage_id,
            "r.example".to_string(),
            t(0),
        );
        FullContext {
            body,
            registry_state: acdp::types::RegistryState {
                status: acdp::types::primitives::Status::Active,
                lifecycle_events: (n > 0).then_some(events),
                extensions: serde_json::Map::new(),
            },
            registry_receipt: None,
            lineage_head_receipt: None,
            log_inclusion: None,
            extensions: serde_json::Map::new(),
            raw_body: None,
        }
    }

    /// A real, schema-valid receipt for event `i` of `c`.
    fn receipt_for(c: &FullContext, i: usize) -> EventReceipt {
        let signer = ReceiptSigner::new(
            acdp::crypto::SigningKey::from_bytes(&[5u8; 32]),
            "did:web:r.example",
            "did:web:r.example#receipt-key-1",
        )
        .expect("signer");
        let e = &c.registry_state.lifecycle_events.as_ref().expect("events")[i];
        signer
            .mint_event_receipt(&c.body.ctx_id, &event_value(e).unwrap(), i as u64, t(1_000))
            .expect("mint")
    }

    /// The store-side clamp, not just `clamp_accepted_at`: a prior receipt
    /// dated after `now` (clock skew between replicas) lifts both the fills
    /// and the new event, and a `created_at` after `now` lifts a fill.
    #[test]
    fn plans_chain_the_clamp_from_a_future_prior_receipt_and_created_at() {
        let c = ctx_with_events(3);
        let signer = ReceiptSigner::new(
            acdp::crypto::SigningKey::from_bytes(&[5u8; 32]),
            "did:web:r.example",
            "did:web:r.example#receipt-key-1",
        )
        .expect("signer");
        let events = c.registry_state.lifecycle_events.clone().expect("events");
        let id = &c.body.ctx_id;
        let ahead = signer
            .mint_event_receipt(id, &event_value(&events[0]).unwrap(), 0, t(5_000))
            .expect("mint");
        let prior = vec![
            (events[0].clone(), StoredReceipt::Valid(Box::new(ahead))),
            (events[1].clone(), StoredReceipt::Null),
        ];

        let plan =
            plan_event_receipts(&signer, id, t(0), &prior, &events[2], t(1_000)).expect("plan");
        assert!(!plan.hole);
        assert_eq!(plan.fills.len(), 1);
        assert_eq!(plan.fills[0].0, 1);
        assert_eq!(plan.fills[0].1.index, 1);
        assert_eq!(plan.fills[0].1.accepted_at, t(5_000), "fill lifted to prev");
        assert_eq!(plan.new.index, 2);
        assert_eq!(plan.new.accepted_at, t(5_000), "new event lifted to prev");

        match plan_backfill(&signer, id, t(0), &prior, t(1_000)).expect("backfill") {
            BackfillPlan::Fill(f) => {
                assert_eq!(f.len(), 1);
                assert_eq!(f[0].1.accepted_at, t(5_000), "sweep lifted to prev");
            }
            other => panic!("expected a fill, got {other:?}"),
        }

        let nulls = vec![(events[0].clone(), StoredReceipt::Null)];
        let plan =
            plan_event_receipts(&signer, id, t(9_000), &nulls, &events[1], t(1_000)).expect("plan");
        assert_eq!(
            plan.fills[0].1.accepted_at,
            t(9_000),
            "fill lifted to created_at"
        );
        assert_eq!(
            plan.new.accepted_at,
            t(9_000),
            "new event lifted to created_at"
        );
        match plan_backfill(&signer, id, t(9_000), &nulls, t(1_000)).expect("backfill") {
            BackfillPlan::Fill(f) => assert_eq!(f[0].1.accepted_at, t(9_000)),
            other => panic!("expected a fill, got {other:?}"),
        }
    }

    #[test]
    fn attach_serves_a_complete_parallel_array() {
        let mut c = ctx_with_events(2);
        let rs = vec![Some(receipt_for(&c, 0)), Some(receipt_for(&c, 1))];
        attach_event_receipts(&mut c, rs, true);
        assert_eq!(c.registry_state.event_receipts().unwrap().unwrap().len(), 2);
    }

    #[test]
    fn attach_omits_a_partial_or_mismatched_array() {
        let fixture = ctx_with_events(2);
        let (r0, r1) = (receipt_for(&fixture, 0), receipt_for(&fixture, 1));
        for receipts in [
            vec![Some(r0.clone()), None],
            vec![Some(r0.clone())],
            vec![Some(r0.clone()), Some(r1.clone()), Some(r1.clone())],
        ] {
            let mut c = fixture.clone();
            // A stale member from an earlier overlay must be removed too.
            c.registry_state
                .set_event_receipts(vec![r0.clone(), r1.clone()])
                .unwrap();
            attach_event_receipts(&mut c, receipts, true);
            assert!(c.registry_state.event_receipts().is_none());
        }
    }

    #[test]
    fn attach_omits_the_member_when_there_are_no_events() {
        let mut c = ctx_with_events(0);
        attach_event_receipts(&mut c, vec![], true);
        assert!(c.registry_state.event_receipts().is_none());
        assert!(!serde_json::to_string(&c)
            .unwrap()
            .contains("event_receipts"));
    }

    #[test]
    fn attach_without_minting_leaves_the_context_untouched() {
        let mut c = ctx_with_events(1);
        let r = receipt_for(&c, 0);
        let before = serde_json::to_value(&c).unwrap();
        attach_event_receipts(&mut c, vec![Some(r)], false);
        assert_eq!(serde_json::to_value(&c).unwrap(), before);
    }
}
