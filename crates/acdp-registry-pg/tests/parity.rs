//! Postgres's half of the cross-backend parity suite.
//!
//! Every assertion lives in `acdp_registry_store::parity` so this backend and
//! SQLite run the *same* code. Keep this file thin: a store, a call.
//!
//! Gated on `ACDP_REGISTRY_TEST_PG_URL` like the rest of this crate's suite,
//! and honouring `ACDP_REQUIRE_PG` the same way — without that, an absent
//! database would turn this parity test into a vacuous pass, which is exactly
//! the failure the gate was added for.

use std::sync::Arc;

use acdp_registry_pg::PgStore;
use acdp_registry_store::{parity, ExtendedRegistryStore};

/// True when `ACDP_REQUIRE_PG` is set to any value, including the empty
/// string — matches `ACDP_REQUIRE_CONFORMANCE`. Do not "improve" this to a
/// truthiness check.
fn require_pg() -> bool {
    std::env::var("ACDP_REQUIRE_PG").is_ok()
}

fn pg_url_or_skip() -> Option<String> {
    match std::env::var("ACDP_REGISTRY_TEST_PG_URL") {
        Ok(u) => Some(u),
        Err(_) => {
            assert!(
                !require_pg(),
                "ACDP_REQUIRE_PG is set but ACDP_REGISTRY_TEST_PG_URL is not"
            );
            eprintln!("ACDP_REGISTRY_TEST_PG_URL unset; skipping pg parity test");
            None
        }
    }
}

async fn store(url: &str) -> Arc<PgStore> {
    let store = PgStore::connect(url, 8).await.expect("pg connect");
    store.migrate().await.expect("pg migrate");
    Arc::new(store)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn data_period_filters_match_the_cross_backend_contract() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;
    parity::assert_data_period_filter_parity(&store, "pg").await;
}
