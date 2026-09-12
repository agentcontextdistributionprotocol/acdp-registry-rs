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

/// H-H: a tenant-scoped search must not leak a foreign tenant's rows — not in
/// the page, not in `total_estimate`, and not through the cursor anchor. Same
/// assertion body SQLite runs, so a divergence fails both suites.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tenant_scoped_search_matches_the_cross_backend_contract() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;
    parity::assert_tenant_scoped_search_parity(&store, "pg").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fulltext_matches_the_cross_backend_contract() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;
    parity::assert_fulltext_parity(&store, "pg").await;
}

/// The stopword list SQLite filters with is a hand-copied table, and the
/// characteristic failure of such tables is that nobody notices when they go
/// stale. So check it against the only authority that matters: Postgres.
///
/// For every entry, `to_tsvector('english', w)` must come back empty — that is
/// exactly what makes `plainto_tsquery` yield an empty query for it, which is
/// the behaviour SQLite is imitating. If Postgres ever stops treating one of
/// these as a stopword, this reddens instead of search quietly skewing.
///
/// The converse — Postgres *gaining* a stopword this list lacks — cannot be
/// checked from SQL, because their list is a file the server does not expose.
/// That residual gap is documented on the constant rather than papered over.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_stopword_list_still_matches_postgres() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;

    let mut disagreements: Vec<String> = Vec::new();
    for word in acdp_registry_store::fulltext::PG_ENGLISH_STOPWORDS {
        let (is_empty,): (bool,) =
            sqlx::query_as("SELECT to_tsvector('english', $1) = ''::tsvector")
                .bind(*word)
                .fetch_one(store.pool())
                .await
                .expect("ask postgres whether the word is a stopword");
        if !is_empty {
            disagreements.push((*word).to_string());
        }
    }
    assert!(
        disagreements.is_empty(),
        "PG_ENGLISH_STOPWORDS has drifted from PostgreSQL's own english.stop —          these entries are NOT stopwords according to this server, so SQLite is          dropping terms Postgres keeps: {disagreements:?}"
    );

    // The negative direction, so the test cannot pass by the list being empty
    // or the query being vacuous.
    for word in ["running", "report", "quarterly", "figures"] {
        let (is_empty,): (bool,) =
            sqlx::query_as("SELECT to_tsvector('english', $1) = ''::tsvector")
                .bind(word)
                .fetch_one(store.pool())
                .await
                .expect("ask postgres");
        assert!(
            !is_empty,
            "{word:?} must NOT be a stopword; if it is, this test's oracle is              not measuring what it claims to"
        );
        assert!(
            !acdp_registry_store::fulltext::is_pg_english_stopword(word),
            "{word:?} must not be in PG_ENGLISH_STOPWORDS"
        );
    }
}

/// B3: same construction as the SQLite side — see that test. `retracted` is a
/// boolean here rather than an integer, which is the only difference.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_desynced_retraction_is_not_served_as_active() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;
    let ctx_id = parity::publish_then_retract(&store, 221, "torn read fixture").await;

    let affected = sqlx::query("UPDATE contexts SET retracted = false WHERE ctx_id = $1")
        .bind(&ctx_id)
        .execute(store.pool())
        .await
        .expect("desync the flag")
        .rows_affected();
    assert_eq!(affected, 1, "the UPDATE must have hit the context row");

    parity::assert_desynced_retraction_is_not_served_active(&store, "pg", &ctx_id).await;
}

/// B7: pin that `contexts.version` is `bigint`.
///
/// SQLite bound `version` `as i64` (lossless for a `u32`); Postgres bound it
/// `as i32`, which wraps above 2^31-1, and the column was `INTEGER` to match.
/// Migration 012 widens it and the casts are now `i64::from`.
///
/// **Honest scope.** This is defence in depth and a parity fix, NOT a live
/// exploit closed. The original finding called the narrowing unreachable
/// "because `put()` has no production callers", which was wrong — the casts
/// were in `commit_publish` and the row INSERT, both on the live publish path.
/// But it *is* unreachable, for a different reason, measured rather than
/// assumed: the SDK request builder requires `version == 1` for a first publish
/// and `prev + 1` for a supersession, so reaching 2^31 needs ~2 billion
/// sequential supersessions. A publish carrying version 3_000_000_000 is
/// refused before the store sees it, on both backends.
///
/// So what is asserted here is the column width itself, which is what the
/// migration changed and what makes the two backends agree by construction
/// rather than by both being narrow.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_version_column_is_wide_enough_for_any_u32() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;

    let (data_type,): (String,) = sqlx::query_as(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_name = 'contexts' AND column_name = 'version'",
    )
    .fetch_one(store.pool())
    .await
    .expect("read the column type");
    assert_eq!(
        data_type, "bigint",
        "contexts.version must be bigint so every u32 fits; `integer` is the \
         narrowing that made Postgres disagree with SQLite"
    );

    // And prove the width is real rather than just declared: a value above
    // i32::MAX must survive a round trip through the column.
    let (echoed,): (i64,) = sqlx::query_as("SELECT $1::bigint")
        .bind(3_000_000_000_i64)
        .fetch_one(store.pool())
        .await
        .expect("round-trip a value above i32::MAX");
    assert_eq!(echoed, 3_000_000_000_i64);
}

/// B8: pin that the two search-filter indexes exist. Mirror of the SQLite
/// assertion — see that one for why a migration needs its result asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_search_filter_indexes_exist() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT indexname FROM pg_indexes WHERE tablename = 'contexts' \
         AND indexname IN ('idx_ctx_domain', 'idx_ctx_expires') ORDER BY indexname",
    )
    .fetch_all(store.pool())
    .await
    .expect("read pg_indexes");
    let found: Vec<&str> = rows.iter().map(|(n,)| n.as_str()).collect();
    assert_eq!(
        found,
        vec!["idx_ctx_domain", "idx_ctx_expires"],
        "both search-filter indexes must exist after migration 013"
    );
}

/// H-I-s: a batched visibility check must answer exactly what N individual
/// retrieve checks answer — same assertion SQLite runs, so a divergence between
/// the two backends fails both suites.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn batched_visibility_matches_the_cross_backend_contract() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = store(&url).await;
    parity::assert_batched_visibility_parity(&store, "pg").await;
}
