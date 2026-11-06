//! Thin `ExtendedRegistryStore` wrapper around [`acdp::registry::InMemoryStore`].
//!
//! Enabled by the `storage-memory` feature. Intended for ephemeral
//! demos and tests where Postgres / SQLite would be overkill — every
//! restart loses state.
//!
//! `list_contexts` is unimplemented because `acdp::registry::InMemoryStore`
//! deliberately does not expose its internal map. The `/admin/contexts`
//! endpoint (compile-gated by `playground`) returns a 500 against this
//! backend; choose `storage-sqlite` if you need it.
//!
//! RFC-ACDP-0017 §10 event receipts live in a wrapper-level side map, since
//! the inner store has nowhere to put them. One mutex serializes every
//! lifecycle commit and every read that overlays receipts, so a read never
//! sees an event without the receipt committed with it. Lock order is
//! wrapper, then inner, everywhere.
//!
//! The RFC-ACDP-0018 change feed lives under the same mutex: every publish
//! and lifecycle commit holds it across the inner commit and the feed push,
//! so a feed read never sees a change without its row or a row without its
//! change (rec-008), and `seq` order is commit order. The feed is lost on
//! restart like everything else here, so this backend cannot advertise the
//! changes profile; it exists for tests and demos.
//!
//! The RFC-ACDP-0021 key-attestation index lives under the same mutex too.
//! `InMemoryStore::commit_publish` holds its own lock from its first check
//! through the receipt mint, so nothing this wrapper hands it may call back
//! into the inner store or this wrapper's lock: that would deadlock. Every
//! value the keyless checks need (the successor's attestation and whether it
//! is retracted, the predecessor's attestation row, whether a mint's token
//! was used) is read under the wrapper lock before the inner commit and
//! captured by value. The wrapped `predecessor_admission` and
//! `receipt_minter` closures only compare those values with what the inner
//! store hands them, at the point it calls them, so this backend refuses in
//! the same order as the SQL ones (`acdp_registry_store::key_attestations`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use acdp::error::AcdpError;
use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
use acdp::registry::{IdempotencyRecord, InMemoryStore, LifecycleCommitOutcome};
use acdp::types::body::{Body, FullContext};
use acdp::types::lifecycle::LifecycleEvent;
use acdp::types::primitives::{AgentDid, ContentHash, CtxId, LineageId};
use acdp::types::publish::PublishResponse;
use acdp::types::search::{SearchParams, SearchResponse};
use acdp::types::EventReceipt;
use acdp_registry_store::changes::{
    feed_appends_for_publish, lifecycle_append, FeedAppend, Predecessor,
};
use acdp_registry_store::event_receipts::{
    attach_event_receipts, plan_event_receipts, receipt_value,
};
use acdp_registry_store::key_attestations::{
    self, admit_successor, index_row_for, index_row_for_request, keyless_successor, list_item,
    predecessor_attestation, predecessor_signer, token_replayed, KeyAttestationFilter,
    KeyAttestationIndexRow, KeyAttestationRecord, SuccessorAttestation, MAX_LIST_LIMIT,
};
use acdp_registry_store::{
    CreateSubscriptionOutcome, EventReceiptMinter, ExtendedRegistryStore, FeedFingerprint,
    FeedHead, FeedPage, FeedQuery, FeedRow, NewSubscription, Page, StoredReceipt,
    SubscriptionCounts, SubscriptionProgress, SubscriptionRecord,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// `ctx_id -> event_id -> receipt`. Keyed by event, not position, so a slot
/// can never drift onto a different event; the position is checked against
/// the receipt's own `index` on every read.
type ReceiptMap = HashMap<String, HashMap<String, EventReceipt>>;

/// Everything the wrapper adds to the inner store, under one mutex.
#[derive(Default)]
struct State {
    receipts: ReceiptMap,
    feed: Feed,
    /// RFC-ACDP-0018 §8 subscriptions, by id.
    subscriptions: HashMap<String, SubscriptionRecord>,
    /// RFC-ACDP-0021 key-attestation index rows, by attestation `ctx_id`.
    key_attestations: HashMap<String, KeyAttestationIndexRow>,
    /// Every `token_hash` that minted an attestation (the UNIQUE index).
    attestation_tokens: HashSet<String>,
}

/// The RFC-ACDP-0018 feed: rows in seq order, never reordered.
#[derive(Default)]
struct Feed {
    rows: Vec<FeedRow>,
    /// The last seq assigned; positions are never reused.
    last_seq: u64,
    pruned_through: u64,
    pruned_through_fp: Option<FeedFingerprint>,
}

impl Feed {
    fn push(&mut self, appends: impl IntoIterator<Item = FeedAppend>) {
        let now_ms = Utc::now().timestamp_millis();
        for a in appends {
            self.last_seq += 1;
            self.rows.push(FeedRow {
                seq: self.last_seq,
                kind: a.kind,
                ctx_id: a.ctx_id,
                lineage_id: a.lineage_id,
                agent_id: a.agent_id,
                superseded_by: a.superseded_by,
                event: a.event,
                receipt: a.receipt,
                appended_at_ms: now_ms,
                lineage_hash: a.lineage_hash,
                agent_hash: a.agent_hash,
            });
        }
    }

    fn head(&self) -> FeedHead {
        FeedHead::from_snapshot(
            self.rows.last().map(|r| (r.seq, r.fingerprint())),
            self.pruned_through,
            self.pruned_through_fp,
        )
    }
}

/// Adapter that delegates the protocol store to `acdp::registry::InMemoryStore`
/// while implementing the registry's extension trait.
///
/// Clones share state: a clone with a minter attached and one without see
/// the same contexts, events and receipts.
#[derive(Default, Clone)]
pub struct MemoryStore {
    inner: Arc<InMemoryStore>,
    state: Arc<Mutex<State>>,
    event_receipt_minter: Option<Arc<dyn EventReceiptMinter>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint an RFC-ACDP-0017 §10 event receipt for every lifecycle event
    /// this store accepts, and serve `registry_state.event_receipts`.
    pub fn with_event_receipt_minter(mut self, minter: Arc<dyn EventReceiptMinter>) -> Self {
        self.event_receipt_minter = Some(minter);
        self
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, AcdpError> {
        self.state
            .lock()
            .map_err(|_| AcdpError::RegistryInternal("event receipt map lock poisoned".into()))
    }

    /// The context's events, each paired with its receipt slot.
    fn slots(map: &ReceiptMap, ctx: &FullContext) -> Vec<(LifecycleEvent, StoredReceipt)> {
        let stored = map.get(ctx.body.ctx_id.as_str());
        ctx.registry_state
            .lifecycle_events
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let slot = match stored.and_then(|m| m.get(&e.event_id)) {
                    None => StoredReceipt::Null,
                    Some(r) if r.index == i as u64 => StoredReceipt::Valid(Box::new(r.clone())),
                    Some(_) => StoredReceipt::Corrupt,
                };
                (e.clone(), slot)
            })
            .collect()
    }

    fn overlay(&self, map: &ReceiptMap, mut ctx: FullContext) -> FullContext {
        let receipts = Self::slots(map, &ctx)
            .into_iter()
            .map(|(_, s)| s.into_option())
            .collect();
        attach_event_receipts(&mut ctx, receipts, self.event_receipt_minter.is_some());
        ctx
    }

    /// Mint (when a minter is attached) and commit under the wrapper lock.
    ///
    /// The receipts are minted *before* the inner commit: the inner append
    /// cannot be rolled back, so a signing failure must abort first. A
    /// receipt minted for an event the inner store then refuses is dropped.
    fn commit_with_receipts(
        &self,
        state: &mut State,
        minter: &dyn EventReceiptMinter,
        event: &LifecycleEvent,
    ) -> Result<LifecycleCommitOutcome, AcdpError> {
        let map = &mut state.receipts;
        // Missing context, replay and event_id reuse all go straight to the
        // inner store, which owns those answers; nothing is minted for them.
        let Some(ctx) = self.inner.get(&event.ctx_id)? else {
            return self.inner.commit_lifecycle_event(event);
        };
        let slots = Self::slots(map, &ctx);
        if slots.iter().any(|(e, _)| e.event_id == event.event_id) {
            return self
                .inner
                .commit_lifecycle_event(event)
                .map(|o| self.overlay_outcome(map, o));
        }
        let plan = plan_event_receipts(
            minter,
            &event.ctx_id,
            ctx.body.created_at,
            &slots,
            event,
            Utc::now(),
        )?;
        // Derived before the inner commit too: that append cannot be rolled
        // back, so nothing that can fail may run after it.
        let row = lifecycle_append(event, &ctx.body, Some(&receipt_value(&plan.new)?))?;
        let outcome = self.inner.commit_lifecycle_event(event)?;
        if matches!(outcome, LifecycleCommitOutcome::Applied(_)) {
            let entry = map.entry(event.ctx_id.as_str().to_string()).or_default();
            plan.record_committed();
            for (_, r) in plan.fills {
                entry.insert(r.event_id.clone(), r);
            }
            entry.insert(plan.new.event_id.clone(), plan.new);
            state.feed.push(row);
            return Ok(self.overlay_outcome(&state.receipts, outcome));
        }
        Ok(self.overlay_outcome(map, outcome))
    }

    /// The index row at `att` and whether its context is retracted, read
    /// under the wrapper lock and outside the inner store's critical section.
    fn successor_snapshot(
        &self,
        state: &State,
        att: &CtxId,
    ) -> Result<Option<SuccessorAttestation>, AcdpError> {
        let Some(row) = state.key_attestations.get(att.as_str()).cloned() else {
            return Ok(None);
        };
        let withdrawn = self.inner.get(att)?.is_some_and(|c| {
            matches!(
                c.registry_state.status,
                acdp::types::primitives::Status::Retracted
            )
        });
        Ok(Some(SuccessorAttestation { row, withdrawn }))
    }

    fn overlay_outcome(
        &self,
        map: &ReceiptMap,
        outcome: LifecycleCommitOutcome,
    ) -> LifecycleCommitOutcome {
        match outcome {
            LifecycleCommitOutcome::Applied(c) => {
                LifecycleCommitOutcome::Applied(self.overlay(map, c))
            }
            LifecycleCommitOutcome::IdempotentReplay(c) => {
                LifecycleCommitOutcome::IdempotentReplay(self.overlay(map, c))
            }
        }
    }
}

impl RegistryStore for MemoryStore {
    fn put(&self, body: Body) -> Result<(), AcdpError> {
        self.inner.put(body)
    }
    fn get(&self, ctx_id: &CtxId) -> Result<Option<FullContext>, AcdpError> {
        let state = self.lock()?;
        Ok(self
            .inner
            .get(ctx_id)?
            .map(|c| self.overlay(&state.receipts, c)))
    }
    fn lineage(&self, lineage_id: &LineageId) -> Result<Vec<FullContext>, AcdpError> {
        let state = self.lock()?;
        Ok(self
            .inner
            .lineage(lineage_id)?
            .into_iter()
            .map(|c| self.overlay(&state.receipts, c))
            .collect())
    }
    fn current(&self, lineage_id: &LineageId) -> Result<Option<FullContext>, AcdpError> {
        let state = self.lock()?;
        Ok(self
            .inner
            .current(lineage_id)?
            .map(|c| self.overlay(&state.receipts, c)))
    }
    fn mark_superseded(&self, ctx_id: &CtxId) -> Result<(), AcdpError> {
        self.inner.mark_superseded(ctx_id)
    }
    fn first_version_ctx_id(&self, lineage_id: &LineageId) -> Result<Option<CtxId>, AcdpError> {
        self.inner.first_version_ctx_id(lineage_id)
    }
    fn idempotency_lookup(
        &self,
        agent_id: &AgentDid,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, AcdpError> {
        self.inner.idempotency_lookup(agent_id, key)
    }
    fn idempotency_record(
        &self,
        agent_id: &AgentDid,
        key: &str,
        hash: &ContentHash,
        response: &PublishResponse,
        expires_at: DateTime<Utc>,
    ) -> Result<(), AcdpError> {
        self.inner
            .idempotency_record(agent_id, key, hash, response, expires_at)
    }
    fn idempotency_evict_expired(&self, now: DateTime<Utc>) -> Result<(), AcdpError> {
        self.inner.idempotency_evict_expired(now)
    }
    fn commit_publish(&self, commit: PublishCommit<'_>) -> Result<PublishCommitOutcome, AcdpError> {
        // Under the wrapper lock so the feed rows land in commit order and
        // no reader sees the publish without them.
        let mut state = self.lock()?;
        let PublishCommit {
            req,
            authority,
            idempotency,
            tenant,
            receipt_minter,
            predecessor_admission,
        } = commit;
        let supersedes = req.supersedes.clone();

        // RFC-ACDP-0021 snapshots, taken now and captured by value (see the
        // module docs). Nothing is refused here: every refusal happens where
        // the SQL backends make it, inside the closures below.
        let mint = index_row_for_request(req, authority);
        let is_mint = !matches!(mint, Ok(None));
        let used_token = match &mint {
            Ok(Some(row)) if state.attestation_tokens.contains(&row.token_hash) => {
                Some(row.token_hash.clone())
            }
            _ => None,
        };
        let successor = keyless_successor(req).map(|r| r.ctx_id.clone());
        let successor_found = match &successor {
            Some(att) => self.successor_snapshot(&state, att)?,
            None => None,
        };
        let predecessor_row = match (&successor, &supersedes) {
            (Some(_), Some(prev)) => self
                .inner
                .get(prev)?
                .and_then(|p| predecessor_attestation(&p.body).ok().flatten())
                .and_then(|att| state.key_attestations.get(att.as_str()).cloned()),
            _ => None,
        };

        // Steps 2, 3 and then the caller's own admission, run by the inner
        // store after its ownership and AlreadySuperseded checks.
        let keyless_admission = |pred: &Body| -> Result<(), AcdpError> {
            if let Some(att) = &successor {
                let found = admit_successor(req, att, successor_found.as_ref())?;
                let pred_att = predecessor_attestation(pred)?;
                let row = predecessor_row
                    .as_ref()
                    .filter(|r| pred_att.as_ref().is_some_and(|c| c.as_str() == r.ctx_id));
                let signer = predecessor_signer(pred_att.as_ref(), row)?;
                key_attestations::check_supersession(&signer, found)?;
            }
            predecessor_admission.map_or(Ok(()), |admit| admit(pred))
        };
        // Steps 2 and 5 against the assigned `created_at`, then step 6, then
        // the real mint.
        let keyless_minter = receipt_minter.map(|mint| {
            let (successor, found, used, authority) =
                (&successor, &successor_found, &used_token, authority);
            move |body: &Body| {
                if let Some(att) = successor {
                    key_attestations::check_window(
                        admit_successor(req, att, found.as_ref())?,
                        body.created_at,
                    )?;
                }
                if let (Some(_), Some(token)) = (index_row_for(body, authority)?, used) {
                    return Err(token_replayed(token));
                }
                mint(body)
            }
        });
        if receipt_minter.is_none() {
            // Without a minter the inner store shows this wrapper no
            // assigned body before it commits, and its commit cannot be
            // undone, so decide now. A keyless deployment always mints
            // receipts (its startup prerequisites).
            if successor.is_some() {
                return Err(AcdpError::RegistryInternal(
                    "a keyless publish needs a receipt minter on the memory backend".into(),
                ));
            }
            mint?;
            if let Some(token) = &used_token {
                return Err(token_replayed(token));
            }
        }
        let wrap_admission = successor.is_some() && supersedes.is_some();
        let wrap_minter = successor.is_some() || is_mint;
        let outcome = self.inner.commit_publish(PublishCommit {
            req,
            authority,
            idempotency,
            tenant,
            receipt_minter: match &keyless_minter {
                Some(m) if wrap_minter => Some(m),
                _ => receipt_minter,
            },
            predecessor_admission: if wrap_admission {
                Some(&keyless_admission)
            } else {
                predecessor_admission
            },
        })?;
        // The new body only exists once the inner store has built it, so the
        // derivation runs after the commit. It cannot fail on a receipt this
        // registry just minted; if it ever did, the publish stands without
        // its rows and the error says so.
        if let PublishCommitOutcome::Inserted(resp) = &outcome {
            if resp.registry_receipt.is_some() || is_mint {
                let body = self.inner.get(&resp.ctx_id)?.ok_or_else(|| {
                    AcdpError::RegistryInternal("published context vanished".into())
                })?;
                // Checked before the commit (the minter above, or the
                // receipt-less branch), so this insert cannot collide.
                if let Some(row) = index_row_for(&body.body, authority)? {
                    state.attestation_tokens.insert(row.token_hash.clone());
                    state.key_attestations.insert(row.ctx_id.clone(), row);
                }
                if resp.registry_receipt.is_some() {
                    let predecessor = match &supersedes {
                        Some(prev) => self.inner.get(prev)?.map(|c| Predecessor {
                            ctx_id: prev.as_str().to_string(),
                            agent_id: c.body.agent_id.as_str().to_string(),
                        }),
                        None => None,
                    };
                    let rows = feed_appends_for_publish(
                        &body.body,
                        resp.registry_receipt.as_ref(),
                        predecessor.as_ref(),
                    )?;
                    state.feed.push(rows);
                }
            }
        }
        Ok(outcome)
    }
    fn commit_lifecycle_event(
        &self,
        event: &LifecycleEvent,
    ) -> Result<LifecycleCommitOutcome, AcdpError> {
        // Delegate explicitly: without this the trait DEFAULT (a loud
        // `not_implemented`) would shadow the inner store's real
        // implementation (RFC-ACDP-0013). The lock is taken even without a
        // minter, so a non-minting clone cannot append between a minting
        // clone's plan and its commit.
        let mut state = self.lock()?;
        match self.event_receipt_minter.clone() {
            Some(minter) => self.commit_with_receipts(&mut state, minter.as_ref(), event),
            None => self.inner.commit_lifecycle_event(event),
        }
    }
    fn search(
        &self,
        params: &SearchParams,
        requester: Option<&AgentDid>,
        public_arm_open: bool,
    ) -> Result<SearchResponse, AcdpError> {
        self.inner.search(params, requester, public_arm_open)
    }
}

#[async_trait]
impl ExtendedRegistryStore for MemoryStore {
    async fn migrate(&self) -> Result<(), AcdpError> {
        Ok(())
    }
    async fn health(&self) -> Result<(), AcdpError> {
        Ok(())
    }
    async fn list_contexts(
        &self,
        _limit: u32,
        _cursor: Option<&str>,
        _requester: Option<&AgentDid>,
        _tenant: Option<&str>,
        _public_arm_open: bool,
    ) -> Result<Page<FullContext>, AcdpError> {
        // The protocol-library InMemoryStore deliberately doesn't expose
        // its internal map; admin listing isn't supported on this backend.
        Err(AcdpError::RegistryInternal(
            "list_contexts is not supported by the memory backend; use SQLite or Postgres".into(),
        ))
    }

    async fn changes_page(&self, query: &FeedQuery) -> Result<FeedPage, AcdpError> {
        let state = self.lock()?;
        let feed = &state.feed;
        let start = feed.rows.partition_point(|r| r.seq <= query.after);
        let rows = feed.rows[start..]
            .iter()
            .filter(|r| query.matches(r))
            .take(query.limit() as usize)
            .cloned()
            .collect();
        Ok(FeedPage::from_scan(query, rows, feed.head()))
    }

    async fn changes_head(&self) -> Result<FeedHead, AcdpError> {
        Ok(self.lock()?.feed.head())
    }

    async fn prune_changes(&self, older_than_ms: i64, batch: u32) -> Result<u64, AcdpError> {
        let mut state = self.lock()?;
        let feed = &mut state.feed;
        let n = feed
            .rows
            .iter()
            .take(batch.max(1) as usize)
            .take_while(|r| r.appended_at_ms < older_than_ms)
            .count();
        if n > 0 && feed.rows[n - 1].seq >= feed.pruned_through {
            feed.pruned_through = feed.rows[n - 1].seq;
            feed.pruned_through_fp = Some(feed.rows[n - 1].fingerprint());
        }
        feed.rows.drain(..n);
        Ok(n as u64)
    }

    async fn key_attestation(
        &self,
        ctx_id: &str,
    ) -> Result<Option<KeyAttestationRecord>, AcdpError> {
        let state = self.lock()?;
        let Some(row) = state.key_attestations.get(ctx_id) else {
            return Ok(None);
        };
        let ctx = self.inner.get(&CtxId(ctx_id.to_string()))?.ok_or_else(|| {
            AcdpError::RegistryInternal(format!("indexed key attestation '{ctx_id}' vanished"))
        })?;
        KeyAttestationRecord::from_parts(
            ctx_id,
            ctx.body.content_hash.0.clone(),
            ctx.registry_state.status.clone(),
            row.created_at_ms,
            &ctx.body,
        )
        .map(Some)
    }

    async fn key_attestation_token_seen(&self, token_hash: &str) -> Result<bool, AcdpError> {
        Ok(self.lock()?.attestation_tokens.contains(token_hash))
    }

    async fn list_key_attestations(
        &self,
        controller: &str,
        filter: &KeyAttestationFilter,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Page<acdp::types::keyless::KeyAttestationListItem>, AcdpError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT) as usize;
        let anchor = cursor
            .map(acdp_registry_store::decode_cursor)
            .transpose()?
            .flatten()
            .map(|(at, ctx)| (at.timestamp_millis(), ctx));
        let state = self.lock()?;
        let mut rows: Vec<&KeyAttestationIndexRow> = state
            .key_attestations
            .values()
            .filter(|r| r.controller == controller && filter.matches(r))
            .filter(|r| {
                anchor.as_ref().is_none_or(|(at, ctx)| {
                    r.created_at_ms < *at || (r.created_at_ms == *at && r.ctx_id > *ctx)
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then_with(|| a.ctx_id.cmp(&b.ctx_id))
        });
        let more = rows.len() > limit;
        let mut items = Vec::with_capacity(limit);
        for row in rows.iter().take(limit) {
            let ctx = self.inner.get(&CtxId(row.ctx_id.clone()))?.ok_or_else(|| {
                AcdpError::RegistryInternal(format!(
                    "indexed key attestation '{}' vanished",
                    row.ctx_id
                ))
            })?;
            items.push(list_item(
                &row.ctx_id,
                ctx.body.content_hash.0.clone(),
                &ctx.body,
            )?);
        }
        let next_cursor = more
            .then(|| rows.get(limit - 1))
            .flatten()
            .map(|r| acdp_registry_store::encode_cursor(r.created_at_ms, &r.ctx_id));
        Ok(Page { items, next_cursor })
    }

    async fn create_subscription(
        &self,
        new: &NewSubscription,
        max_per_owner: u64,
    ) -> Result<CreateSubscriptionOutcome, AcdpError> {
        let mut state = self.lock()?;
        let held = state
            .subscriptions
            .values()
            .filter(|s| s.owner_did == new.owner_did)
            .count();
        if held as u64 >= max_per_owner {
            return Ok(CreateSubscriptionOutcome::LimitReached);
        }
        state.subscriptions.insert(
            new.subscription_id.clone(),
            SubscriptionRecord {
                subscription_id: new.subscription_id.clone(),
                owner_did: new.owner_did.clone(),
                tenant_id: new.tenant_id.clone(),
                filter: new.filter.clone(),
                callback_url: new.callback_url.clone(),
                secret_ct: new.secret_ct.clone(),
                secret_key_id: new.secret_key_id,
                status: acdp::types::change::SubscriptionStatus::Active,
                created_at_ms: new.created_at_ms,
                last_hinted_seq: new.last_hinted_seq,
                next_attempt_at_ms: new.created_at_ms,
                failing_since_ms: None,
                attempts: 0,
                lease_until_ms: None,
            },
        );
        Ok(CreateSubscriptionOutcome::Created)
    }

    async fn get_subscription(
        &self,
        owner_did: &str,
        subscription_id: &str,
    ) -> Result<Option<SubscriptionRecord>, AcdpError> {
        Ok(self
            .lock()?
            .subscriptions
            .get(subscription_id)
            .filter(|s| s.owner_did == owner_did)
            .cloned())
    }

    async fn delete_subscription(
        &self,
        owner_did: &str,
        subscription_id: &str,
    ) -> Result<bool, AcdpError> {
        let mut state = self.lock()?;
        let owned = state
            .subscriptions
            .get(subscription_id)
            .is_some_and(|s| s.owner_did == owner_did);
        if owned {
            state.subscriptions.remove(subscription_id);
        }
        Ok(owned)
    }

    async fn claim_due_subscriptions(
        &self,
        now_ms: i64,
        head: u64,
        lease_until_ms: i64,
        limit: u32,
    ) -> Result<Vec<SubscriptionRecord>, AcdpError> {
        let mut state = self.lock()?;
        let mut due: Vec<&mut SubscriptionRecord> = state
            .subscriptions
            .values_mut()
            .filter(|s| {
                s.status == acdp::types::change::SubscriptionStatus::Active
                    && s.next_attempt_at_ms <= now_ms
                    && s.lease_until_ms.is_none_or(|l| l < now_ms)
                    && s.last_hinted_seq < head
            })
            .collect();
        due.sort_by_key(|s| s.next_attempt_at_ms);
        Ok(due
            .into_iter()
            .take(limit as usize)
            .map(|s| {
                s.lease_until_ms = Some(lease_until_ms);
                s.clone()
            })
            .collect())
    }

    async fn finish_subscription_attempt(
        &self,
        subscription_id: &str,
        lease_until_ms: i64,
        progress: &SubscriptionProgress,
    ) -> Result<bool, AcdpError> {
        let mut state = self.lock()?;
        let Some(s) = state
            .subscriptions
            .get_mut(subscription_id)
            .filter(|s| s.lease_until_ms == Some(lease_until_ms))
        else {
            return Ok(false);
        };
        s.last_hinted_seq = progress.last_hinted_seq;
        s.status = progress.status;
        s.next_attempt_at_ms = progress.next_attempt_at_ms;
        s.failing_since_ms = progress.failing_since_ms;
        s.attempts = progress.attempts;
        s.lease_until_ms = None;
        Ok(true)
    }

    async fn subscription_counts(&self, head: u64) -> Result<SubscriptionCounts, AcdpError> {
        let state = self.lock()?;
        let mut out = SubscriptionCounts::default();
        for s in state.subscriptions.values() {
            match s.status {
                acdp::types::change::SubscriptionStatus::Active => {
                    out.active += 1;
                    if s.last_hinted_seq < head {
                        out.oldest_due_at_ms = Some(
                            out.oldest_due_at_ms
                                .map_or(s.next_attempt_at_ms, |o| o.min(s.next_attempt_at_ms)),
                        );
                    }
                }
                acdp::types::change::SubscriptionStatus::Disabled => out.disabled += 1,
            }
        }
        Ok(out)
    }
}

/// Tests for the memory backend (hardening C7).
///
/// These live here, not in `tests/`, because the server is bin-only: an
/// integration test links against a lib target and cannot name `MemoryStore`.
///
/// Three groups:
///
/// 1. **The shared parity suite** (`acdp_registry_store::parity`), the same
///    code the SQLite and Postgres suites run. Every check was RUN against
///    this backend before deciding whether it applies; the ones that do not
///    are named below with the measured reason, never skipped silently.
/// 2. **Trait round-trips** through `MemoryStore` itself, so a wrapper method
///    that stops delegating (e.g. `get` returning `Ok(None)`) goes red.
/// 3. **Pinned gaps and divergences** — behaviour that is deliberately (or,
///    for full-text search, measurably) different from the durable backends.
///    Pinned so a change in either direction is a visible decision.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use acdp::crypto::SigningKey;
    use acdp::error::SupersessionReason;
    use acdp::producer::Producer;
    use acdp::registry::store::PendingIdempotencyCommit;
    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    use acdp::types::primitives::{ContextType, Status, Visibility};
    use acdp::types::publish::PublishRequest;
    use acdp_registry_store::parity;

    const AUTHORITY: &str = "reg.test";

    fn did(seed: u8) -> AgentDid {
        AgentDid::new(format!("did:web:agents.test:memory-{seed}"))
    }

    fn producer(seed: u8) -> Producer {
        Producer::new(
            SigningKey::from_bytes(&[seed; 32]),
            did(seed),
            format!("did:web:agents.test:memory-{seed}#key-1"),
        )
    }

    fn request(p: &Producer, title: &str) -> PublishRequest {
        p.publish_request()
            .title(title)
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid publish request")
    }

    fn commit_with(
        store: &MemoryStore,
        req: &PublishRequest,
        idem_key: Option<&str>,
        tenant: Option<&str>,
    ) -> Result<PublishCommitOutcome, AcdpError> {
        store.commit_publish(PublishCommit {
            req,
            authority: AUTHORITY,
            idempotency: idem_key.map(|key| PendingIdempotencyCommit {
                key,
                ttl: chrono::Duration::hours(1),
            }),
            tenant,
            receipt_minter: None,
            predecessor_admission: None,
        })
    }

    fn commit(store: &MemoryStore, req: &PublishRequest) -> PublishResponse {
        commit_with(store, req, None, None)
            .expect("publish succeeds")
            .into_response()
    }

    fn retract_event(ctx_id: &CtxId, actor: AgentDid) -> LifecycleEvent {
        LifecycleEvent::new(
            uuid::Uuid::new_v4().to_string(),
            ctx_id.clone(),
            LifecycleEventType::Retracted,
            Utc::now(),
            actor,
            Some("memory_ext tests".to_string()),
        )
        .expect("valid lifecycle event")
    }

    // ── 1. The shared parity suite ──────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn data_period_filters_match_the_cross_backend_contract() {
        parity::assert_data_period_filter_parity(&Arc::new(MemoryStore::new()), "memory").await;
    }

    /// B3's coherence assertion. The SQL backends first desynchronise a
    /// denormalised `retracted` column with backend-specific SQL; this backend
    /// has no such column (the SDK derives `status` from the event log at read
    /// time), so there is nothing to desynchronise and the assertion runs on
    /// the state a retraction produces directly. It still proves the served
    /// pair is coherent AND that `commit_lifecycle_event` reaches the inner
    /// store rather than the trait's loud `not_implemented` default.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_retracted_context_is_not_served_as_active() {
        let store = Arc::new(MemoryStore::new());
        let ctx_id = parity::publish_then_retract(&store, 220, "memory retract fixture").await;
        parity::assert_desynced_retraction_is_not_served_active(&store, "memory", &ctx_id).await;
    }

    // NOT RUN HERE, with the measured reason (each was run against this
    // backend on 2026-10-03 and failed ONLY for the reason given):
    //
    // * `parity::assert_tenant_scoped_search_parity` — this backend does not
    //   model tenancy (`InMemoryStore::commit_publish` discards `tenant`), so
    //   `search_in_tenant(Some("tenant-a-…"))` takes the trait default's
    //   foreign-tenant arm: zero rows, `total_estimate = Some(0)`, no cursor.
    //   Guarantees (a) and (d) held; (b) "cursor absent" and (c)
    //   "total_estimate 0, must be 3" failed. The untenanted behaviour is
    //   pinned instead by `tenancy_is_not_modelled_every_row_is_default`.
    //
    // * `parity::assert_batched_visibility_parity` — same cause. Every
    //   violation it reported was on a tenant-SCOPED arm (the fixture
    //   publishes into `tenant-vis-…`, which `tenant_of_ctx`'s default reports
    //   as `default`, so a scoped call withholds everything). The unscoped
    //   §4.5 arms raised no violation; they are re-asserted absolutely by
    //   `batched_visibility_follows_retrieve_rules_when_unscoped`.
    //
    // * `parity::assert_fulltext_parity` — a REAL divergence, not a
    //   non-applicable check: see
    //   `fulltext_search_diverges_from_the_cross_backend_contract`.

    // ── 2. Trait round-trips ────────────────────────────────────────────────

    #[test]
    fn put_then_get_round_trips_and_a_duplicate_put_is_refused() {
        // Mint a real, fully-assigned Body via a publish into a scratch store,
        // then `put` it into the store under test.
        let scratch = MemoryStore::new();
        let resp = commit(&scratch, &request(&producer(1), "put round trip"));
        let body = scratch
            .get(&resp.ctx_id)
            .expect("get ok")
            .expect("scratch holds it")
            .body;

        let store = MemoryStore::new();
        store.put(body.clone()).expect("put succeeds");
        let got = store
            .get(&resp.ctx_id)
            .expect("get ok")
            .expect("get must return what put stored");
        // `Body` is not `PartialEq`; compare its wire form.
        assert_eq!(
            serde_json::to_value(&got.body).unwrap(),
            serde_json::to_value(&body).unwrap()
        );
        assert_eq!(got.registry_state.status, Status::Active);

        let err = store
            .put(body)
            .expect_err("a duplicate ctx_id must be refused");
        assert!(matches!(err, AcdpError::SchemaViolation(_)), "{err:?}");
    }

    #[test]
    fn get_of_a_published_context_returns_it_and_a_missing_id_is_none() {
        let store = MemoryStore::new();
        let resp = commit(&store, &request(&producer(2), "get round trip"));
        let got = store
            .get(&resp.ctx_id)
            .expect("get ok")
            .expect("a just-published context must be retrievable");
        assert_eq!(got.body.ctx_id, resp.ctx_id);
        assert_eq!(got.body.title, "get round trip");

        let missing = CtxId(format!(
            "acdp://{AUTHORITY}/00000000-0000-4000-8000-000000000000"
        ));
        assert!(store.get(&missing).expect("get ok").is_none());
        assert!(store
            .lineage(&LineageId(missing.0.clone()))
            .expect("lineage ok")
            .is_empty());
        assert!(store
            .current(&LineageId(missing.0.clone()))
            .expect("current ok")
            .is_none());
        assert!(store
            .first_version_ctx_id(&LineageId(missing.0))
            .expect("first_version ok")
            .is_none());
    }

    #[test]
    fn supersession_links_the_lineage_and_moves_current() {
        let store = MemoryStore::new();
        let p = producer(3);
        let v1 = commit(&store, &request(&p, "v1"));
        let v1_body = store.get(&v1.ctx_id).unwrap().unwrap().body;
        let v2_req = p
            .supersede_body(&v1_body)
            .title("v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid v2");
        let v2 = commit(&store, &v2_req);

        assert_eq!(v2.version, 2);
        assert_eq!(v2.lineage_id, v1.lineage_id);
        assert_eq!(
            store
                .get(&v1.ctx_id)
                .unwrap()
                .unwrap()
                .registry_state
                .status,
            Status::Superseded,
            "the predecessor must be marked superseded"
        );
        let lineage = store.lineage(&v1.lineage_id).expect("lineage ok");
        let ids: Vec<_> = lineage.iter().map(|c| c.body.ctx_id.clone()).collect();
        assert_eq!(ids, vec![v1.ctx_id.clone(), v2.ctx_id.clone()]);
        assert_eq!(
            store
                .current(&v1.lineage_id)
                .unwrap()
                .expect("a head exists")
                .body
                .ctx_id,
            v2.ctx_id
        );
        assert_eq!(
            store.first_version_ctx_id(&v1.lineage_id).unwrap(),
            Some(v1.ctx_id.clone())
        );

        // Superseding the already-superseded v1 again is refused.
        let again = p
            .supersede_body(&v1_body)
            .title("v2 again")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let err = commit_with(&store, &again, None, None).expect_err("v1 is already superseded");
        assert!(
            matches!(
                err,
                AcdpError::SupersededTarget {
                    reason: SupersessionReason::AlreadySuperseded,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn mark_superseded_changes_status_and_leaves_no_head() {
        let store = MemoryStore::new();
        let v1 = commit(&store, &request(&producer(4), "lone v1"));
        store.mark_superseded(&v1.ctx_id).expect("mark ok");
        assert_eq!(
            store
                .get(&v1.ctx_id)
                .unwrap()
                .unwrap()
                .registry_state
                .status,
            Status::Superseded
        );
        // RFC-ACDP-0004 §5: a lineage whose every version is superseded has
        // no head.
        assert!(store.current(&v1.lineage_id).unwrap().is_none());
        // Marking an unknown id is a silent no-op on this backend (pinned).
        store
            .mark_superseded(&CtxId(format!(
                "acdp://{AUTHORITY}/00000000-0000-4000-8000-000000000001"
            )))
            .expect("unknown id is Ok(())");
    }

    #[test]
    fn superseding_a_missing_or_foreign_predecessor_is_not_found() {
        let store = MemoryStore::new();
        let owner = producer(5);
        let v1 = commit(&store, &request(&owner, "owned v1"));
        let v1_body = store.get(&v1.ctx_id).unwrap().unwrap().body;

        // A stranger superseding someone else's context learns nothing.
        let stranger_req = producer(6)
            .supersede_body(&v1_body)
            .title("takeover")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let err = commit_with(&store, &stranger_req, None, None).expect_err("non-owner");
        assert!(
            matches!(
                err,
                AcdpError::SupersededTarget {
                    reason: SupersessionReason::NotFound,
                    ..
                }
            ),
            "{err:?}"
        );

        // A predecessor that does not exist at all.
        let ghost = owner
            .supersede(CtxId(format!(
                "acdp://{AUTHORITY}/00000000-0000-4000-8000-000000000002"
            )))
            .version(2)
            .title("ghost v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let err = commit_with(&store, &ghost, None, None).expect_err("missing predecessor");
        assert!(
            matches!(
                err,
                AcdpError::SupersededTarget {
                    reason: SupersessionReason::NotFound,
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(
            store
                .get(&v1.ctx_id)
                .unwrap()
                .unwrap()
                .registry_state
                .status,
            Status::Active,
            "a refused supersession must leave the predecessor untouched"
        );
    }

    /// FEAT-01: N distinct v2 publishes racing to supersede one v1 produce
    /// exactly one winner; every loser fails with `superseded_target`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_supersession_has_exactly_one_winner() {
        const THREADS: usize = 16;
        let store = Arc::new(MemoryStore::new());
        let p = producer(7);
        let v1 = commit(&store, &request(&p, "race v1"));
        let v1_body = store.get(&v1.ctx_id).unwrap().unwrap().body;

        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let req = p
                    .supersede_body(&v1_body)
                    .title(format!("v2-candidate-{i}"))
                    .context_type(ContextType::DataSnapshot)
                    .visibility(Visibility::Public)
                    .build()
                    .expect("valid v2");
                let s = Arc::clone(&store);
                tokio::task::spawn_blocking(move || commit_with(&s, &req, None, None))
            })
            .collect();
        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.expect("task"));
        }
        let (winners, losers): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| r.is_ok());
        assert_eq!(winners.len(), 1, "exactly one supersession wins");
        for l in &losers {
            let err = l.as_ref().unwrap_err();
            assert!(matches!(err, AcdpError::SupersededTarget { .. }), "{err:?}");
        }
        let winner = winners.into_iter().next().unwrap().unwrap().into_response();
        let lineage = store.lineage(&v1.lineage_id).unwrap();
        assert_eq!(lineage.len(), 2, "exactly v1 + the single winner");
        assert_eq!(
            store.current(&v1.lineage_id).unwrap().unwrap().body.ctx_id,
            winner.ctx_id
        );
    }

    #[test]
    fn idempotent_publish_replays_and_a_changed_body_is_a_duplicate() {
        let store = MemoryStore::new();
        let p = producer(8);
        let req = request(&p, "idem original");

        let first = commit_with(&store, &req, Some("k1"), None).expect("first publish");
        assert!(!first.is_replay());
        let replay = commit_with(&store, &req, Some("k1"), None).expect("replay");
        assert!(replay.is_replay(), "same key + same hash must replay");
        assert_eq!(replay.response().ctx_id, first.response().ctx_id);

        let rec = store
            .idempotency_lookup(&did(8), "k1")
            .expect("lookup ok")
            .expect("the publish must have recorded the key");
        assert_eq!(rec.content_hash, req.content_hash);
        assert_eq!(rec.response.ctx_id, first.response().ctx_id);

        let changed = request(&p, "idem DIFFERENT body");
        let err =
            commit_with(&store, &changed, Some("k1"), None).expect_err("same key + different hash");
        assert!(matches!(err, AcdpError::DuplicatePublish(_)), "{err:?}");
    }

    #[test]
    fn idempotency_record_lookup_and_eviction_round_trip() {
        let store = MemoryStore::new();
        let resp = commit(&store, &request(&producer(9), "idem source"));
        let hash = request(&producer(9), "idem source").content_hash;
        let agent = did(9);
        let now = Utc::now();

        store
            .idempotency_record(
                &agent,
                "live",
                &hash,
                &resp,
                now + chrono::Duration::hours(1),
            )
            .expect("record ok");
        let got = store
            .idempotency_lookup(&agent, "live")
            .expect("lookup ok")
            .expect("an unexpired record must be found");
        assert_eq!(got.content_hash, hash);
        assert_eq!(got.response.ctx_id, resp.ctx_id);
        assert!(store
            .idempotency_lookup(&agent, "never-recorded")
            .unwrap()
            .is_none());

        // Explicit eviction with a clock past the TTL removes it.
        store
            .idempotency_evict_expired(now + chrono::Duration::hours(2))
            .expect("evict ok");
        assert!(
            store.idempotency_lookup(&agent, "live").unwrap().is_none(),
            "evicting at a time past expires_at must remove the record"
        );

        // An already-expired record is never served (lazy eviction on lookup).
        store
            .idempotency_record(
                &agent,
                "stale",
                &hash,
                &resp,
                now - chrono::Duration::seconds(1),
            )
            .unwrap();
        assert!(store.idempotency_lookup(&agent, "stale").unwrap().is_none());
    }

    #[test]
    fn lifecycle_events_reach_the_inner_store_and_enforce_alternation() {
        let store = MemoryStore::new();
        let resp = commit(&store, &request(&producer(10), "retract me"));
        store
            .commit_lifecycle_event(&retract_event(&resp.ctx_id, did(10)))
            .expect("first retraction applies");
        assert_eq!(
            store
                .get(&resp.ctx_id)
                .unwrap()
                .unwrap()
                .registry_state
                .status,
            Status::Retracted
        );
        let err = store
            .commit_lifecycle_event(&retract_event(&resp.ctx_id, did(10)))
            .expect_err("double retract");
        assert!(
            matches!(err, AcdpError::InvalidLifecycleTransition(_)),
            "{err:?}"
        );

        let missing = CtxId(format!(
            "acdp://{AUTHORITY}/00000000-0000-4000-8000-000000000003"
        ));
        let err = store
            .commit_lifecycle_event(&retract_event(&missing, did(10)))
            .expect_err("unknown ctx_id");
        assert!(matches!(err, AcdpError::NotFound(_)), "{err:?}");
    }

    fn receipt_signer(seed: u8) -> (Arc<dyn EventReceiptMinter>, [u8; 32]) {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let pubkey = key.verifying_key_bytes();
        let signer = acdp::types::ReceiptSigner::new(
            key,
            "did:web:reg.test",
            "did:web:reg.test#receipt-key-1",
        )
        .expect("signer");
        (Arc::new(signer), pubkey)
    }

    /// RFC-ACDP-0017 §10: the same contract the durable backends run. The
    /// two handles share state, so the legacy prefix written by `plain` is
    /// filled on the first append through `minting`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn event_receipts_match_the_cross_backend_contract() {
        let plain = Arc::new(MemoryStore::new());
        let (signer, pubkey) = receipt_signer(98);
        let minting = Arc::new(plain.as_ref().clone().with_event_receipt_minter(signer));
        parity::assert_lifecycle_event_receipts(&plain, &minting, pubkey, "memory").await;
    }

    /// RFC-ACDP-0018 §4/§5.3: the feed rows every durable backend appends.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn change_feed_matches_the_cross_backend_contract() {
        let plain = Arc::new(MemoryStore::new());
        let (signer, _) = receipt_signer(90);
        let minting = Arc::new(plain.as_ref().clone().with_event_receipt_minter(signer));
        parity::assert_change_feed(&plain, &minting, "memory").await;
    }

    /// RFC-ACDP-0018 §6.1: bucket filters over the stored hashes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn change_feed_bucket_filter_matches_the_cross_backend_contract() {
        let plain = Arc::new(MemoryStore::new());
        parity::assert_change_feed_bucket_filter(&plain, "memory").await;
    }

    /// RFC-ACDP-0018 §4: one row per receipted event, matching registry_state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn change_feed_lifecycle_rows_match_registry_state() {
        let plain = Arc::new(MemoryStore::new());
        let (signer, _) = receipt_signer(87);
        let minting = Arc::new(plain.as_ref().clone().with_event_receipt_minter(signer));
        parity::assert_change_feed_lifecycle_sequence(&plain, &minting, "memory").await;
    }

    /// RFC-ACDP-0018 §5.3: racing keyed retries append one row, never fail.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn change_feed_concurrent_idempotent_supersede() {
        let plain = Arc::new(MemoryStore::new());
        parity::assert_change_feed_concurrent_idempotent_supersede(&plain, "memory").await;
    }

    /// RFC-ACDP-0018 §8.1: subscription CRUD is owner-scoped.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn subscription_crud_is_owner_scoped() {
        let plain = Arc::new(MemoryStore::new());
        parity::assert_subscription_crud(&plain, "memory").await;
    }

    /// RFC-ACDP-0018 §8.1: concurrent creates never exceed the per-owner limit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn subscription_limit_holds_under_concurrent_creates() {
        let plain = Arc::new(MemoryStore::new());
        parity::assert_subscription_limit(&plain, "memory").await;
    }

    /// RFC-ACDP-0018 §8.2: delivery leases hand each due row to one claimant.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn subscription_claims_have_one_holder() {
        let plain = Arc::new(MemoryStore::new());
        parity::assert_subscription_claims(&plain, "memory").await;
    }

    /// RFC-ACDP-0018 §5.4: prefix pruning and the marker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn change_feed_prune_matches_the_cross_backend_contract() {
        let plain = Arc::new(MemoryStore::new());
        let (signer, _) = receipt_signer(89);
        let minting = Arc::new(plain.as_ref().clone().with_event_receipt_minter(signer));
        parity::assert_change_feed_prune(&plain, &minting, "memory").await;
    }

    /// RFC-ACDP-0018 §5.2: seq order is commit order under concurrent writers.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn change_feed_seq_order_is_commit_order() {
        let plain = Arc::new(MemoryStore::new());
        let (signer, _) = receipt_signer(88);
        let minting = Arc::new(plain.as_ref().clone().with_event_receipt_minter(signer));
        parity::assert_change_feed_commit_order(&plain, &minting, "memory").await;
    }

    struct FailingMinter;

    impl EventReceiptMinter for FailingMinter {
        fn mint(
            &self,
            _ctx_id: &CtxId,
            _raw_event: &serde_json::Value,
            _index: u64,
            _accepted_at: DateTime<Utc>,
        ) -> Result<EventReceipt, AcdpError> {
            Err(AcdpError::RegistryInternal("minter unavailable".into()))
        }
    }

    /// The inner append cannot roll back, so a mint failure must stop the
    /// commit before it reaches the inner store.
    #[test]
    fn a_mint_failure_never_reaches_the_inner_store() {
        let plain = MemoryStore::new();
        let failing = plain
            .clone()
            .with_event_receipt_minter(Arc::new(FailingMinter));
        let resp = commit(&plain, &request(&producer(11), "mint failure"));
        let err = failing
            .commit_lifecycle_event(&retract_event(&resp.ctx_id, did(11)))
            .expect_err("a failing minter must fail the commit");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
        let ctx = plain.get(&resp.ctx_id).unwrap().unwrap();
        assert!(ctx.registry_state.lifecycle_events.is_none());
        assert_eq!(ctx.registry_state.status, Status::Active);
    }

    /// A replay and a refused transition persist no receipt (a refusal may
    /// call the minter, but its receipt is dropped); reads through a
    /// non-minting clone never carry receipts.
    #[test]
    fn replays_and_refusals_persist_no_receipt() {
        let plain = MemoryStore::new();
        let (signer, _) = receipt_signer(97);
        let minting = plain.clone().with_event_receipt_minter(signer);
        let resp = commit(&plain, &request(&producer(12), "replay"));
        let event = retract_event(&resp.ctx_id, did(12));
        let LifecycleCommitOutcome::Applied(first) =
            minting.commit_lifecycle_event(&event).unwrap()
        else {
            panic!("first retract must apply")
        };
        let first =
            serde_json::to_value(first.registry_state.event_receipts.clone().unwrap()).unwrap();
        let LifecycleCommitOutcome::IdempotentReplay(again) =
            minting.commit_lifecycle_event(&event).unwrap()
        else {
            panic!("identical retry must replay")
        };
        assert_eq!(
            serde_json::to_value(again.registry_state.event_receipts.clone().unwrap()).unwrap(),
            first
        );
        minting
            .commit_lifecycle_event(&retract_event(&resp.ctx_id, did(12)))
            .expect_err("double retract");
        let map = plain.state.lock().unwrap();
        assert_eq!(
            map.receipts[resp.ctx_id.as_str()].len(),
            1,
            "a refusal minted"
        );
        drop(map);
        assert!(plain
            .get(&resp.ctx_id)
            .unwrap()
            .unwrap()
            .registry_state
            .event_receipts
            .is_none());
    }

    #[test]
    fn a_minting_commit_to_an_unknown_context_is_not_found_and_stores_nothing() {
        let (signer, _) = receipt_signer(96);
        let minting = MemoryStore::new().with_event_receipt_minter(signer);
        let ghost = CtxId("acdp://r.example/00000000-0000-4000-8000-0000000000ff".into());
        let err = minting
            .commit_lifecycle_event(&retract_event(&ghost, did(13)))
            .expect_err("unknown ctx_id");
        assert!(matches!(err, AcdpError::NotFound(_)), "{err:?}");
        assert!(minting.state.lock().unwrap().receipts.is_empty());
    }

    /// A stored receipt whose index does not match the event's position is
    /// corrupt: the read omits the member rather than serve it.
    #[test]
    fn a_receipt_at_the_wrong_index_is_omitted() {
        let plain = MemoryStore::new();
        let (signer, _) = receipt_signer(95);
        let minting = plain.clone().with_event_receipt_minter(signer);
        let resp = commit(&plain, &request(&producer(14), "corrupt"));
        minting
            .commit_lifecycle_event(&retract_event(&resp.ctx_id, did(14)))
            .unwrap();
        let read = |s: &MemoryStore| s.get(&resp.ctx_id).unwrap().unwrap();
        assert!(read(&minting).registry_state.event_receipts.is_some());
        for r in plain
            .state
            .lock()
            .unwrap()
            .receipts
            .get_mut(resp.ctx_id.as_str())
            .unwrap()
            .values_mut()
        {
            r.index += 1;
        }
        assert!(read(&minting).registry_state.event_receipts.is_none());
    }

    /// A poisoned receipt map is an internal error on reads and commits,
    /// never a panic and never a read that silently drops receipts.
    #[test]
    fn a_poisoned_receipt_map_is_an_internal_error() {
        let plain = MemoryStore::new();
        let resp = commit(&plain, &request(&producer(15), "poison"));
        let clone = plain.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.state.lock().unwrap();
            panic!("poison the receipt map");
        })
        .join();
        let err = plain.get(&resp.ctx_id).expect_err("poisoned get");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
        let err = plain
            .commit_lifecycle_event(&retract_event(&resp.ctx_id, did(15)))
            .expect_err("poisoned commit");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
    }

    /// The unscoped half of `parity::assert_batched_visibility_parity`, stated
    /// absolutely (RFC-ACDP-0008 §4.5 retrieve semantics). See the note in
    /// section 1 for why the parity check itself is not run.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn batched_visibility_follows_retrieve_rules_when_unscoped() {
        let store = MemoryStore::new();
        let owner = producer(11);
        let (aud, contrib, outsider) = (did(12), did(13), did(14));
        let publish = |title: &str, vis: Visibility, audience: Vec<AgentDid>, c: Vec<AgentDid>| {
            let mut b = owner
                .publish_request()
                .title(title)
                .context_type(ContextType::DataSnapshot)
                .visibility(vis);
            if !audience.is_empty() {
                b = b.audience(audience);
            }
            if !c.is_empty() {
                b = b.contributors(c);
            }
            commit(&store, &b.build().unwrap()).ctx_id.0
        };
        let private_aud = publish("p-aud", Visibility::Private, vec![aud.clone()], vec![]);
        let private_contrib = publish(
            "p-contrib",
            Visibility::Private,
            vec![],
            vec![contrib.clone()],
        );
        let private_owner_only = publish("p-owner", Visibility::Private, vec![], vec![]);
        let restricted_aud = publish("r-aud", Visibility::Restricted, vec![aud.clone()], vec![]);
        let public_retracted = publish("pub-retracted", Visibility::Public, vec![], vec![]);
        store
            .commit_lifecycle_event(&retract_event(&CtxId(public_retracted.clone()), did(11)))
            .unwrap();
        let ids = [
            private_aud.as_str(),
            private_contrib.as_str(),
            private_owner_only.as_str(),
            restricted_aud.as_str(),
            public_retracted.as_str(),
            "not-a-ctx-id-at-all",
        ];

        let as_aud = store
            .visible_ctx_ids(&ids, Some(&aud), None, false)
            .await
            .unwrap();
        assert!(
            as_aud.contains(&private_aud),
            "audience may retrieve private"
        );
        assert!(
            as_aud.contains(&restricted_aud),
            "audience may retrieve restricted"
        );
        assert!(
            !as_aud.contains(&private_owner_only),
            "another context's audience is not this one's"
        );
        assert!(
            as_aud.contains(&public_retracted),
            "retracted stays retrievable"
        );
        let as_owner = store
            .visible_ctx_ids(&ids, Some(&did(11)), None, false)
            .await
            .unwrap();
        for id in [
            &private_aud,
            &private_contrib,
            &private_owner_only,
            &restricted_aud,
        ] {
            assert!(as_owner.contains(id), "the producer may retrieve {id}");
        }
        let as_contrib = store
            .visible_ctx_ids(&ids, Some(&contrib), None, false)
            .await
            .unwrap();
        assert!(
            !as_contrib.contains(&private_contrib),
            "contributors are never authorization"
        );
        let as_outsider = store
            .visible_ctx_ids(&ids, Some(&outsider), None, false)
            .await
            .unwrap();
        assert!(!as_outsider.contains(&private_aud));
        assert!(
            !as_outsider.contains(&private_owner_only),
            "an outsider is refused a private owner-only context"
        );
        assert!(
            !as_outsider.contains(&restricted_aud),
            "an outsider is refused a restricted context"
        );
        assert!(
            as_outsider.contains(&public_retracted),
            "an authenticated outsider may retrieve public"
        );
        assert!(!as_outsider.contains("not-a-ctx-id-at-all"));
        let anon_off = store
            .visible_ctx_ids(&ids, None, None, false)
            .await
            .unwrap();
        assert!(anon_off.is_empty(), "anonymous with reads off sees nothing");
    }

    /// RFC-ACDP-0008 §4.5 search disclosure through `MemoryStore::search`:
    /// a wrapper that dropped `requester` or forced `public_arm_open` on would
    /// leak a private context or open anonymous reads, and goes red here.
    #[test]
    fn search_applies_the_disclosure_rules_to_requester_and_public_arm() {
        let store = MemoryStore::new();
        let owner = producer(17);
        let publish = |title: &str, vis: Visibility| {
            let req = owner
                .publish_request()
                .title(title)
                .context_type(ContextType::DataSnapshot)
                .visibility(vis)
                .build()
                .expect("valid publish request");
            commit(&store, &req).ctx_id
        };
        let private = publish("search private", Visibility::Private);
        let public = publish("search public", Visibility::Public);
        let found = |requester: Option<&AgentDid>, public_arm_open: bool| {
            let params = SearchParams {
                agent_id: Some(did(17).as_str().to_string()),
                ..Default::default()
            };
            let mut ids: Vec<CtxId> = store
                .search(&params, requester, public_arm_open)
                .expect("search ok")
                .matches
                .into_iter()
                .map(|m| m.ctx_id)
                .collect();
            ids.sort_by(|a, b| a.0.cmp(&b.0));
            ids
        };
        let sorted = |mut v: Vec<CtxId>| {
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };

        assert_eq!(
            found(Some(&did(18)), false),
            vec![public.clone()],
            "an outsider sees the public context only, never the private one"
        );
        assert!(
            found(None, false).is_empty(),
            "anonymous with public reads off sees nothing"
        );
        assert_eq!(
            found(None, true),
            vec![public.clone()],
            "anonymous with public reads on sees only the public context"
        );
        assert_eq!(
            found(Some(&did(17)), false),
            sorted(vec![private, public]),
            "the producer sees both"
        );
    }

    // ── 3. Pinned gaps and divergences ──────────────────────────────────────

    /// **Divergence (measured 2026-10-03), not a non-applicable check.**
    ///
    /// The SDK `InMemoryStore::search` implements `q=` as ONE case-insensitive
    /// SUBSTRING match of the whole query over a concatenated haystack. The
    /// durable backends implement the `parity::assert_fulltext_parity`
    /// contract: stemmed, stopword-dropping, term-AND full-text search. Run
    /// against this backend, the parity check failed at its first case.
    ///
    /// Each line below states the contract's answer and pins this backend's
    /// opposite one. The fix belongs upstream (`acdp-server`'s
    /// `InMemoryStore`). A wrapper-side fix (re-filtering the inner store's
    /// matches here) was rejected: the inner store has already paginated and
    /// counted, so post-filtering would make `total_estimate` wrong and leave
    /// short or empty pages behind valid cursors. It is asserted here instead;
    /// if upstream changes, this test goes red and the parity call should
    /// replace it. (Accepted because this backend is demo-only; see
    /// ASSUMPTIONS.md.)
    #[test]
    fn fulltext_search_diverges_from_the_cross_backend_contract() {
        let store = MemoryStore::new();
        let p = producer(15);
        let run = commit(&store, &request(&p, "run report")).ctx_id;
        let the = commit(&store, &request(&p, "the quarterly figures")).ctx_id;
        let hits = |q: &str, id: &CtxId| {
            let params = acdp::types::search::SearchParams {
                q: Some(q.to_string()),
                agent_id: Some(did(15).as_str().to_string()),
                ..Default::default()
            };
            store
                .search(&params, None, true)
                .expect("search ok")
                .matches
                .iter()
                .any(|m| &m.ctx_id == id)
        };

        // Contract: stemmed match. Memory: no stemming.
        assert!(
            !hits("running", &run),
            "DIVERGENCE: q=running vs \"run report\""
        );
        // Contract: stopword-only query matches nothing. Memory: substring hit.
        assert!(
            hits("the", &the),
            "DIVERGENCE: q=the vs \"the quarterly figures\""
        );
        // Contract: terms are AND-ed, order-free. Memory: contiguous phrase.
        assert!(!hits("the figures", &the), "DIVERGENCE: q='the figures'");
        assert!(!hits("report run", &run), "DIVERGENCE: q='report run'");

        // Where the two agree (so the divergence is not "search is broken"):
        assert!(hits("RUN REPORT", &run), "case folding holds");
        assert!(hits("report", &run), "a plain contained term matches");
        assert!(!hits("elephant", &run), "an absent term excludes");
    }

    /// Deliberate gap: tenancy is not modelled. Every row is in the reserved
    /// `default` tenant whatever the publish asked for, a foreign tenant sees
    /// an empty page, and `default` sees everything. (Startup is what keeps a
    /// multi-tenant deployment off this backend; this pins the store side.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn tenancy_is_not_modelled_every_row_is_default() {
        let store = MemoryStore::new();
        let req = request(&producer(16), "tenanted publish");
        let resp = commit_with(&store, &req, None, Some("tenant-a"))
            .expect("publish ok")
            .into_response();
        let id = resp.ctx_id.as_str();

        assert_eq!(
            store.tenant_of_ctx(id).await.unwrap().as_deref(),
            Some("default"),
            "the requested tenant is discarded"
        );
        store.set_tenant_of_ctx(id, "tenant-a").await.unwrap();
        assert_eq!(
            store.tenant_of_ctx(id).await.unwrap().as_deref(),
            Some("default"),
            "set_tenant_of_ctx is a no-op"
        );
        assert_eq!(
            store
                .tenant_of_ctx("not-even-an-id")
                .await
                .unwrap()
                .as_deref(),
            Some("default"),
            "unknown ids are reported as default too"
        );

        let params = acdp::types::search::SearchParams {
            agent_id: Some(did(16).as_str().to_string()),
            ..Default::default()
        };
        let foreign = store
            .search_in_tenant(&params, None, true, Some("tenant-a"))
            .await
            .unwrap();
        assert!(foreign.matches.is_empty());
        assert_eq!(foreign.total_estimate, Some(0));
        assert!(foreign.next_cursor.is_none());
        let default = store
            .search_in_tenant(&params, None, true, Some("default"))
            .await
            .unwrap();
        assert_eq!(default.matches.len(), 1);
        assert_eq!(default.matches[0].ctx_id, resp.ctx_id);
    }

    /// Deliberate gaps: no admin listing, no transparency log, no witness
    /// storage. Each must fail LOUDLY (or, for the optional read, be empty),
    /// never return a plausible-looking empty success.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unsupported_surfaces_fail_loudly() {
        let store = MemoryStore::new();
        store.migrate().await.expect("migrate is a no-op");
        store.health().await.expect("always healthy");

        let err = store
            .list_contexts(10, None, None, None, true)
            .await
            .expect_err("list_contexts is unsupported");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");

        let ni = |r: Result<(), AcdpError>, what: &str| {
            let err = r.expect_err(what);
            assert!(
                matches!(err, AcdpError::NotImplemented(_)),
                "{what}: {err:?}"
            );
        };
        ni(store.log_tree_size().await.map(drop), "log_tree_size");
        ni(store.log_leaf_hashes(1).await.map(drop), "log_leaf_hashes");
        ni(
            store.log_leaf_by_ctx("x").await.map(drop),
            "log_leaf_by_ctx",
        );
        ni(
            store.log_leaf_by_index(0).await.map(drop),
            "log_leaf_by_index",
        );
        ni(store.log_entries(0, 1).await.map(drop), "log_entries");
        ni(
            store
                .upsert_witness_cosignature(
                    "log",
                    1,
                    "root",
                    "did:web:w",
                    "2026-01-01T00:00:00Z",
                    "{}",
                )
                .await,
            "upsert_witness_cosignature",
        );
        assert!(store
            .witness_cosignatures_for("log", 1, "root")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(store.count_idempotency_records().await.unwrap(), None);
    }

    // ── RFC-ACDP-0021 key attestations ──────────────────────────────────────

    /// D4: index derivation, token replay, the corrupt-attestation abort and
    /// the one-token race. This backend has no transparency log.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn key_attestation_index_and_replay_match_the_cross_backend_contract() {
        let store = Arc::new(MemoryStore::new());
        parity::assert_key_attestation_index_and_replay(&store, false, "memory").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn keyless_window_and_withdrawal_match_the_cross_backend_contract() {
        let store = Arc::new(MemoryStore::new());
        parity::assert_keyless_window_and_withdrawal(&store, "memory").await;
    }

    /// Round 2 (b): the wrapped closures refuse in the SQL backends' order.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn keyless_refusal_order_matches_the_cross_backend_contract() {
        let store = Arc::new(MemoryStore::new());
        parity::assert_keyless_refusal_order(&store, "memory").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn keyless_supersession_table_matches_kl_001_vector_5() {
        let store = Arc::new(MemoryStore::new());
        parity::assert_keyless_supersession_table(&store, parity::KL_001_VECTOR_5_CASES, "memory")
            .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_unindexed_keyless_predecessor_fails_closed() {
        let store = Arc::new(MemoryStore::new());
        let handle = Arc::clone(&store);
        parity::assert_unindexed_keyless_predecessor_fails_closed(
            &store,
            "memory",
            |ctx| async move {
                let removed = handle.lock().unwrap().key_attestations.remove(&ctx);
                assert!(removed.is_some(), "fixture removes one index row");
            },
        )
        .await;
    }

    /// Plants the keyless-signed body through the inner store, which runs
    /// none of this wrapper's keyless checks.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_malformed_keyless_predecessor_fails_closed() {
        let store = Arc::new(MemoryStore::new());
        let planter = Arc::clone(&store);
        parity::assert_malformed_keyless_predecessor_fails_closed(
            &store,
            "memory",
            |req| async move {
                planter
                    .inner
                    .commit_publish(PublishCommit {
                        req: &req,
                        authority: AUTHORITY,
                        idempotency: None,
                        tenant: None,
                        receipt_minter: None,
                        predecessor_admission: None,
                    })
                    .expect("plant")
                    .into_response()
                    .ctx_id
            },
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn key_attestation_listing_matches_the_cross_backend_contract() {
        let store = Arc::new(MemoryStore::new());
        parity::assert_key_attestation_listing(&store, "memory").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn key_attestation_record_join_matches_the_cross_backend_contract() {
        let store = Arc::new(MemoryStore::new());
        parity::assert_key_attestation_record_join(&store, "memory").await;
    }

    /// Round 2 (a): the re-entrancy regression. A VALID keyless successor
    /// superseding a keyless predecessor drives BOTH wrapped closures (the
    /// admission and the minter) inside `InMemoryStore`'s critical section.
    /// Either one touching the inner store or the wrapper lock would hang,
    /// so the commit runs under a timeout and each closure records it ran.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_valid_keyless_supersede_runs_both_closures_without_deadlock() {
        use parity::keyless_fixtures::*;
        use std::sync::atomic::{AtomicBool, Ordering};

        let store = Arc::new(MemoryStore::new());
        let c = controller("memory-deadlock");
        let id = oidc("repo:example/pipeline:ref:refs/heads/main");
        let p = mint(&store, &Spec::new(&c, id.clone())).await;
        let v1 = inserted(commit(&store, signed_under(&p, None), None).await, "v1");
        let s = mint(&store, &Spec::new(&c, id)).await;
        let req = signed_under(&s, Some((&v1, 2)));

        let worker = Arc::clone(&store);
        let task = tokio::task::spawn_blocking(move || {
            let (admitted, minted) = (AtomicBool::new(false), AtomicBool::new(false));
            let signer = receipt_signer();
            let admit = |_: &Body| {
                admitted.store(true, Ordering::SeqCst);
                Ok(())
            };
            let mint = |body: &Body| {
                minted.store(true, Ordering::SeqCst);
                let r = signer.mint(
                    &body.ctx_id,
                    &body.lineage_id,
                    &body.origin_registry,
                    body.created_at,
                    &body.content_hash,
                    &format!("sha256:{}", "c".repeat(64)),
                )?;
                serde_json::to_value(r).map_err(AcdpError::from)
            };
            let outcome = worker.commit_publish(PublishCommit {
                req: &req,
                authority: AUTHORITY,
                idempotency: None,
                tenant: None,
                receipt_minter: Some(&mint),
                predecessor_admission: Some(&admit),
            });
            (outcome, admitted.into_inner(), minted.into_inner())
        });
        let (outcome, admitted, minted) =
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .expect("a keyless supersede must not deadlock the memory store")
                .expect("task");
        assert!(
            matches!(outcome, Ok(PublishCommitOutcome::Inserted(_))),
            "{outcome:?}"
        );
        assert!(admitted, "the wrapped predecessor admission ran");
        assert!(minted, "the wrapped receipt minter ran");
        assert_eq!(
            store.get(&v1).unwrap().unwrap().registry_state.status,
            Status::Superseded
        );
    }

    /// Without a receipt minter the inner store never shows this wrapper the
    /// assigned `created_at`, so a keyless publish fails closed and a mint
    /// is checked before the commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_a_minter_keyless_fails_closed_and_mints_are_checked_first() {
        use acdp::types::keyless::KeylessReason;
        use parity::keyless_fixtures::*;

        let store = MemoryStore::new();
        let shared = Arc::new(store.clone());
        let c = controller("memory-no-minter");
        let spec = Spec::new(
            &c,
            spiffe("spiffe://prod.example.com/ns/agents/sa/publisher"),
        );
        let m = mint(&shared, &spec).await;
        let err = commit_with(&store, &signed_under(&m, None), None, None)
            .expect_err("keyless without a minter");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");

        let (replay, _) = Spec {
            window: past_window(),
            ..spec.clone()
        }
        .request(8);
        let err = commit_with(&store, &replay, None, None).expect_err("replayed token");
        assert_eq!(KeylessReason::of(&err), Some(KeylessReason::TokenReplayed));

        let (fresh, _) =
            Spec::new(&c, spiffe("spiffe://prod.example.com/ns/agents/sa/other")).request(9);
        let ctx = commit_with(&store, &fresh, None, None)
            .expect("a fresh mint commits")
            .into_response()
            .ctx_id;
        assert!(store.key_attestation(ctx.as_str()).await.unwrap().is_some());
    }
}

/// RFC-ACDP-0021 Phase 5: the `POST/GET /key-attestations` A/H/E core on
/// this backend, from the same kit the SQLite and pg suites use. The
/// memory store keeps no transparency log, so the leaf half of A is the
/// SQLite and pg runs'.
#[cfg(test)]
#[path = "../tests/keyless_kit/mod.rs"]
mod keyless_kit;

#[cfg(test)]
mod keyless_http_tests {
    use super::keyless_kit::{assert_mint_core, Kit, Opts};
    use super::MemoryStore;
    use acdp_registry_store::ExtendedRegistryStore as _;

    #[tokio::test]
    async fn kl002_a_h_e_core_on_the_memory_store() {
        let store = MemoryStore::new();
        store.migrate().await.unwrap();
        let kit = Kit::new(store, Opts::default()).await;
        assert_mint_core(&kit, "memory").await;
    }

    /// RFC-ACDP-0021 Phase 6: kl-003, kl-004, kl-009 B and kl-007 B on
    /// this backend (no log, so no leaf half).
    #[tokio::test(flavor = "multi_thread")]
    async fn keyless_publish_scenarios_on_the_memory_store() {
        let store = MemoryStore::new();
        store.migrate().await.unwrap();
        let kit = Kit::new(store, Opts::default()).await;
        super::keyless_kit::assert_keyless_publish_core(&kit, "memory").await;
    }

    /// §6.3 under a P-256 ephemeral key on this backend (no log).
    #[tokio::test(flavor = "multi_thread")]
    async fn keyless_publish_under_a_p256_key_on_the_memory_store() {
        let store = MemoryStore::new();
        store.migrate().await.unwrap();
        let kit = Kit::new(store, Opts::default()).await;
        super::keyless_kit::assert_keyless_p256_publish(&kit, "memory").await;
    }
}
