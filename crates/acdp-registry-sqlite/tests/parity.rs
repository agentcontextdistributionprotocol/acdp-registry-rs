//! SQLite's half of the cross-backend parity suite.
//!
//! Every assertion lives in `acdp_registry_store::parity` so this backend and
//! Postgres run the *same* code. Keep this file thin: a store, a call. Adding
//! a backend-specific expectation here would defeat the point — the whole
//! value is that a divergence fails both suites, and it cannot do that if the
//! expectations are written per backend.

use std::sync::Arc;

use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::{parity, ExtendedRegistryStore};

async fn store() -> (Arc<SqliteStore>, tempfile::NamedTempFile) {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let store = SqliteStore::connect(tmp.path(), 4).await.expect("connect");
    store.migrate().await.expect("migrate");
    (Arc::new(store), tmp)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn data_period_filters_match_the_cross_backend_contract() {
    let (store, _tmp) = store().await;
    parity::assert_data_period_filter_parity(&store, "sqlite").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fulltext_matches_the_cross_backend_contract() {
    let (store, _tmp) = store().await;
    parity::assert_fulltext_parity(&store, "sqlite").await;
}

/// H-H: a tenant-scoped search must not leak a foreign tenant's rows — not in
/// the page, not in `total_estimate`, and not through the cursor anchor.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tenant_scoped_search_matches_the_cross_backend_contract() {
    let (store, _tmp) = store().await;
    parity::assert_tenant_scoped_search_parity(&store, "sqlite").await;
}

/// B3: desynchronize the denormalized `retracted` column from the event log —
/// the exact state a torn read between the row query and the event query would
/// observe — and assert `get()` still serves a coherent pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_desynced_retraction_is_not_served_as_active() {
    let (store, _tmp) = store().await;
    let ctx_id = parity::publish_then_retract(&store, 220, "torn read fixture").await;

    // Clear only the denormalized flag; the lifecycle event stays.
    let affected = sqlx::query("UPDATE contexts SET retracted = 0 WHERE ctx_id = ?")
        .bind(&ctx_id)
        .execute(store.pool())
        .await
        .expect("desync the flag")
        .rows_affected();
    assert_eq!(affected, 1, "the UPDATE must have hit the context row");

    parity::assert_desynced_retraction_is_not_served_active(&store, "sqlite", &ctx_id).await;
}

/// B6: a corrupt `contributors` column must FAIL a superseding publish, not
/// silently decode to an empty list.
///
/// This is deliberately **not** a cross-backend parity test. Postgres stores
/// `contributors` as `TEXT[]`, so invalid JSON cannot be written into it at all
/// — the divergence existed precisely because SQLite's TEXT column can hold
/// something undecodable and the old code chose to ignore that. The guard
/// therefore belongs where the hazard is.
///
/// Why it matters beyond tidiness: `contributors` feeds the RFC-ACDP-0014 §4
/// predecessor-admission check, so swallowing the error meant a legitimate
/// contributor was refused with `SupersededTarget::NotFound` while the check
/// reported success. Silent wrong answers are worse than loud failures here.
///
/// Note this is a *different* field from the one
/// `admission_is_not_skipped_when_the_predecessor_body_is_undecodable` covers
/// (that one corrupts `body_json`), which is why B6 needs its own test rather
/// than inheriting that one's coverage.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_corrupt_contributors_column_fails_the_publish() {
    use acdp::crypto::SigningKey;
    use acdp::producer::Producer;
    use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
    use acdp::types::primitives::{AgentDid, ContextType, Visibility};

    let (store, _tmp) = store().await;
    let p = Producer::new(
        SigningKey::from_bytes(&[230; 32]),
        AgentDid::new("did:web:agents.test:parity-230".to_string()),
        "did:web:agents.test:parity-230#key-1".to_string(),
    );

    // v1, so there is a predecessor to corrupt.
    let v1 = p
        .publish_request()
        .title("contributors fixture v1")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid v1");
    let s = std::sync::Arc::clone(&store);
    let outcome = tokio::task::spawn_blocking(move || {
        s.commit_publish(PublishCommit {
            req: &v1,
            authority: "reg.test",
            idempotency: None,
            tenant: None,
            receipt_minter: None,
            predecessor_admission: None,
        })
    })
    .await
    .expect("task")
    .expect("v1 publishes");
    let v1_ctx = match outcome {
        PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
            r.ctx_id.clone()
        }
    };
    // The response carries no body, so read it back — the same way this repo's
    // other supersede tests build their predecessor.
    let s = std::sync::Arc::clone(&store);
    let id = v1_ctx.clone();
    let v1_body = tokio::task::spawn_blocking(move || s.get(&id))
        .await
        .expect("task")
        .expect("retrieve ok")
        .expect("v1 present")
        .body;

    // Corrupt ONLY `contributors`, leaving `body_json` intact so the failure
    // cannot be the body decode this test would otherwise be confused with.
    let affected = sqlx::query("UPDATE contexts SET contributors = ? WHERE ctx_id = ?")
        .bind("{not-json")
        .bind(v1_ctx.as_str())
        .execute(store.pool())
        .await
        .expect("corrupting UPDATE")
        .rows_affected();
    assert_eq!(affected, 1, "the UPDATE must have hit the predecessor row");

    let v2 = p
        .supersede_body(&v1_body)
        .title("contributors fixture v2")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid v2");
    let s = std::sync::Arc::clone(&store);
    let result = tokio::task::spawn_blocking(move || {
        s.commit_publish(PublishCommit {
            req: &v2,
            authority: "reg.test",
            idempotency: None,
            tenant: None,
            receipt_minter: None,
            predecessor_admission: None,
        })
    })
    .await
    .expect("task");

    // Pin the CONTRIBUTORS decode specifically. `is_err()` alone would also be
    // satisfied by the ownership / lineage / version gates that run nearby, so
    // it would not prove which failure happened.
    match &result {
        Err(acdp::error::AcdpError::RegistryInternal(msg)) => assert!(
            msg.contains("decode contributors"),
            "the failure must be the contributors decode, got RegistryInternal({msg})"
        ),
        other => panic!(
            "an undecodable `contributors` column must fail the publish rather than \
             silently becoming an empty list and skipping the RFC-ACDP-0014 §4 \
             admission check, got {other:?}"
        ),
    }
}

/// B8: pin that the two search-filter indexes exist.
///
/// `GET /contexts/search` filters on `domain` and `expires_at`, and neither had
/// an index in either backend — both filters were full scans. A migration that
/// silently fails to run is indistinguishable from one that did unless
/// something asserts the result, which is what this is for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_search_filter_indexes_exist() {
    let (store, _tmp) = store().await;
    let names: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'index' \
         AND name IN ('idx_ctx_domain', 'idx_ctx_expires') ORDER BY name",
    )
    .fetch_all(store.pool())
    .await
    .expect("read sqlite_master");
    let found: Vec<&str> = names.iter().map(|(n,)| n.as_str()).collect();
    assert_eq!(
        found,
        vec!["idx_ctx_domain", "idx_ctx_expires"],
        "both search-filter indexes must exist after migration 014"
    );
}

/// H-I-s: a batched visibility check must answer exactly what N individual
/// retrieve checks answer — for a fixture mixing public / owned / audience /
/// contributor-only / retracted / foreign-tenant, across six requester
/// perspectives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn batched_visibility_matches_the_cross_backend_contract() {
    let (store, _tmp) = store().await;
    parity::assert_batched_visibility_parity(&store, "sqlite").await;
}

/// The point of the unit, measured rather than asserted.
///
/// This does not assert a speedup — a timing assertion is a flake waiting to
/// happen on a shared CI box. It measures both paths over a full 256-entry page
/// (the handler's `LOG_ENTRIES_PAGE_CAP`) and prints the numbers, so the claim
/// "one query instead of 256 round-trips" has an observation behind it instead
/// of an adjective. Run with `--nocapture` to read it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_batched_path_costs_one_round_trip_for_a_full_page() {
    use acdp::crypto::SigningKey;
    use acdp::producer::Producer;
    use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
    use acdp::types::primitives::{AgentDid, ContextType, Visibility};

    const PAGE: usize = 256; // LOG_ENTRIES_PAGE_CAP
    let (store, _tmp) = store().await;
    let requester = AgentDid::new("did:web:agents.test:parity-240".to_string());
    let p = Producer::new(
        SigningKey::from_bytes(&[240; 32]),
        requester.clone(),
        "did:web:agents.test:parity-240#key-1".to_string(),
    );

    let mut ids = Vec::with_capacity(PAGE);
    for i in 0..PAGE {
        let req = p
            .publish_request()
            .title(format!("page fixture {i}"))
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid request");
        let s = Arc::clone(&store);
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &req,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .expect("task")
        .expect("publishes");
        match outcome {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                ids.push(r.ctx_id.as_str().to_string())
            }
        }
    }
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();

    let t0 = std::time::Instant::now();
    let batched = store
        .visible_ctx_ids(&refs, Some(&requester), None, false)
        .await
        .expect("batched ok");
    let batched_ms = t0.elapsed();

    // The shape the handler uses today: one blocking `get` per entry.
    let t1 = std::time::Instant::now();
    let mut per_id = std::collections::HashSet::new();
    for id in &refs {
        let s = Arc::clone(&store);
        let parsed = acdp::types::primitives::CtxId((*id).to_string());
        if tokio::task::spawn_blocking(move || s.get(&parsed))
            .await
            .expect("task")
            .expect("get ok")
            .is_some()
        {
            per_id.insert((*id).to_string());
        }
    }
    let per_id_ms = t1.elapsed();

    // Correctness first: the measurement is worthless if the two disagree.
    assert_eq!(
        batched.len(),
        PAGE,
        "all {PAGE} public contexts are visible to their own producer"
    );
    assert_eq!(
        batched, per_id,
        "the batched path and the per-id path must agree on a full page"
    );

    println!(
        "[sqlite] {PAGE}-entry page: batched (1 query) {batched_ms:?} vs per-id ({PAGE} queries) \
         {per_id_ms:?}"
    );
}
