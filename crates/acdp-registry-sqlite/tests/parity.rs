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
