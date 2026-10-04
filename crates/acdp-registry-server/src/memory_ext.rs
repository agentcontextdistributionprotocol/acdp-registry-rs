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

use acdp::error::AcdpError;
use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
use acdp::registry::{IdempotencyRecord, InMemoryStore};
use acdp::types::body::{Body, FullContext};
use acdp::types::primitives::{AgentDid, ContentHash, CtxId, LineageId};
use acdp::types::publish::PublishResponse;
use acdp::types::search::{SearchParams, SearchResponse};
use acdp_registry_store::{ExtendedRegistryStore, Page};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Adapter that delegates the protocol store to `acdp::registry::InMemoryStore`
/// while implementing the registry's extension trait.
#[derive(Default)]
pub struct MemoryStore {
    inner: InMemoryStore,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RegistryStore for MemoryStore {
    fn put(&self, body: Body) -> Result<(), AcdpError> {
        self.inner.put(body)
    }
    fn get(&self, ctx_id: &CtxId) -> Result<Option<FullContext>, AcdpError> {
        self.inner.get(ctx_id)
    }
    fn lineage(&self, lineage_id: &LineageId) -> Result<Vec<FullContext>, AcdpError> {
        self.inner.lineage(lineage_id)
    }
    fn current(&self, lineage_id: &LineageId) -> Result<Option<FullContext>, AcdpError> {
        self.inner.current(lineage_id)
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
        self.inner.commit_publish(commit)
    }
    fn commit_lifecycle_event(
        &self,
        event: &acdp::types::lifecycle::LifecycleEvent,
    ) -> Result<acdp::registry::LifecycleCommitOutcome, AcdpError> {
        // Delegate explicitly: without this the trait DEFAULT (a loud
        // `not_implemented`) would shadow the inner store's real
        // implementation (RFC-ACDP-0013).
        self.inner.commit_lifecycle_event(event)
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
        let public_retracted = publish("pub-retracted", Visibility::Public, vec![], vec![]);
        store
            .commit_lifecycle_event(&retract_event(&CtxId(public_retracted.clone()), did(11)))
            .unwrap();
        let ids = [
            private_aud.as_str(),
            private_contrib.as_str(),
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
            as_aud.contains(&public_retracted),
            "retracted stays retrievable"
        );
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
        assert!(!as_outsider.contains("not-a-ctx-id-at-all"));
        let anon_off = store
            .visible_ctx_ids(&ids, None, None, false)
            .await
            .unwrap();
        assert!(anon_off.is_empty(), "anonymous with reads off sees nothing");
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
    /// `InMemoryStore`), so it is asserted here rather than patched; if
    /// upstream changes, this test goes red and the parity call should
    /// replace it.
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
}
