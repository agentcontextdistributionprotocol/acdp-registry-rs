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
