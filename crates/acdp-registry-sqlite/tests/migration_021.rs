//! Migration 021 on SQLite: the RFC-ACDP-0018 §6.1 bucket hash columns,
//! their NULL-refusing triggers, and the Rust backfill `migrate()` runs.
//!
//! A "database at 020" (or at 021 without the backfill) is built with a
//! `Migrator` over a tempdir copy of the first migrations only, seeded
//! through raw 020-shaped `INSERT`s, and then handed to `store.migrate()`.
//! The copies carry the same names and bytes, so the full migrator accepts
//! them as applied and runs only the rest, exactly as an upgrade would.

use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::{bucket_hash, ExtendedRegistryStore, FeedQuery};
use sqlx::migrate::Migrator;

/// rec-010's derivation vectors (spec 3f50697): `(value, sha256_hex)`.
const VECTORS: [(&str, &str); 5] = [
    (
        "lin:sha256:a3cb6a87c1601b9f6dddd8d54709b01df3e05f1095b43e6d58e3a862bf9906fa",
        "1f54677c32bda0fce357ac477603794f48739d9e1590b5ad5fa7ee48801432de",
    ),
    (
        "lin:sha256:9eb5e0ee5d9a110edc81f7e1fdafca3e3f521650fb58b8bb09c53f29bec58592",
        "fdd0e7e87d1c2056989122d13b8031a5be52be71358f0f4e99539a24c07a65fc",
    ),
    (
        "did:key:z6MkiTBz1ymuepAQ4HEHYSF1H8quG5GLVVQR3djdX3mDooWp",
        "ad90ec18fd5e0735b1e5eeab571e8dc9f733dcd65276387844c8236fefb87977",
    ),
    (
        "did:web:agents.example.com:test-producer",
        "7a3e4116a94f4d654b8953a260b2203e2504014c4a70234198f69304dbd1d79e",
    ),
    (
        "did:web:registry.example.com%3A8443",
        "cf5a338210f6c09d7715ee5a9271c03304b35a995e74db80da7db719595c5bf0",
    ),
];

/// Rows seeded at 020: more than two backfill batches of 1000.
const SEEDED: usize = 2_345;

/// A store on a fresh file database, NOT migrated.
async fn bare_store() -> (SqliteStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::connect(&dir.path().join("registry.sqlite"), 4)
        .await
        .expect("connect");
    (store, dir)
}

/// Apply migrations `1..=through` only, from a tempdir copy.
async fn migrate_through(store: &SqliteStore, through: i64) {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let dst = tempfile::tempdir().expect("tempdir");
    for entry in std::fs::read_dir(&src).expect("read migrations") {
        let path = entry.expect("entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let version: i64 = name
            .split('_')
            .next()
            .unwrap()
            .parse()
            .expect("NNN_ prefix");
        if version <= through {
            std::fs::copy(&path, dst.path().join(&name)).expect("copy migration");
        }
    }
    Migrator::new(dst.path())
        .await
        .expect("partial migrator")
        .run(store.pool())
        .await
        .expect("partial migrate");
}

/// The lineage and agent of seeded row `i`: the vectors first, then
/// distinct generated ids.
fn ids(i: usize) -> (String, String) {
    match i {
        0 => (VECTORS[0].0.into(), VECTORS[2].0.into()),
        1 => (VECTORS[1].0.into(), VECTORS[3].0.into()),
        2 => (VECTORS[0].0.into(), VECTORS[4].0.into()),
        _ => (
            format!("lin:sha256:{:064x}", i),
            format!("did:web:agents.test:seed-{i}"),
        ),
    }
}

/// Seed `n` rows through the 020-shaped insert (no hash columns).
async fn seed_020(store: &SqliteStore, n: usize) {
    let mut tx = store.pool().begin().await.expect("begin");
    for i in 0..n {
        let (lineage, agent) = ids(i);
        sqlx::query(
            "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, appended_at_ms) \
             VALUES ('key_revocation', ?, ?, ?, '{}', 0)",
        )
        .bind(format!("acdp://reg.test/{i}"))
        .bind(lineage)
        .bind(agent)
        .execute(&mut *tx)
        .await
        .expect("seed a 020 row");
    }
    tx.commit().await.expect("commit");
}

async fn null_hashes(store: &SqliteStore) -> i64 {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM changes WHERE lineage_hash IS NULL OR agent_hash IS NULL",
    )
    .fetch_one(store.pool())
    .await
    .expect("count NULL hashes")
    .0
}

/// Every stored row's hashes are over its ids, read back through
/// `changes_page` (which decodes them as non-NULL), and the vector rows
/// carry the spec's digests.
async fn assert_hashes_are_over_the_ids(store: &SqliteStore, n: usize) {
    let mut after = 0;
    let mut seen = 0;
    loop {
        let page = store
            .changes_page(&FeedQuery {
                after,
                scan_limit: 1000,
                ..FeedQuery::default()
            })
            .await
            .expect("changes_page");
        for row in &page.rows {
            assert_eq!(
                row.lineage_hash,
                bucket_hash(&row.lineage_id),
                "{}",
                row.seq
            );
            assert_eq!(row.agent_hash, bucket_hash(&row.agent_id), "{}", row.seq);
            for (value, sha) in VECTORS {
                if row.lineage_id == value {
                    assert_eq!(row.lineage_hash, sha);
                }
                if row.agent_id == value {
                    assert_eq!(row.agent_hash, sha);
                }
            }
        }
        seen += page.rows.len();
        if page.reached_head {
            break;
        }
        after = page.last_scanned;
    }
    assert_eq!(seen, n, "every seeded row is served");
}

/// 020 → 021: `migrate()` backfills every existing row, in batches, and a
/// second `migrate()` is a no-op.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_021_backfills_a_020_database() {
    let (store, _dir) = bare_store().await;
    migrate_through(&store, 20).await;
    seed_020(&store, SEEDED).await;
    store.migrate().await.expect("migrate 020 → 021");
    assert_eq!(null_hashes(&store).await, 0);
    assert_hashes_are_over_the_ids(&store, SEEDED).await;
    store
        .migrate()
        .await
        .expect("re-running migrate is a no-op");
    assert_hashes_are_over_the_ids(&store, SEEDED).await;
}

/// The failure path the RAISE triggers exist for: any insert path other
/// than `changes::insert` that forgets a hash fails loudly, and a hash
/// cannot be cleared afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_021_triggers_refuse_null_bucket_hashes() {
    let (store, _dir) = bare_store().await;
    store.migrate().await.expect("migrate");
    let insert = |lineage_hash: Option<String>, agent_hash: Option<String>| {
        sqlx::query(
            "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, appended_at_ms, \
             lineage_hash, agent_hash) VALUES ('key_revocation', 'c', 'l', 'a', '{}', 0, ?, ?)",
        )
        .bind(lineage_hash)
        .bind(agent_hash)
        .execute(store.pool())
    };
    for (what, res) in [
        (
            "a 020-shaped insert",
            sqlx::query(
                "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, appended_at_ms) \
                 VALUES ('key_revocation', 'c', 'l', 'a', '{}', 0)",
            )
            .execute(store.pool())
            .await,
        ),
        ("no lineage_hash", insert(None, Some(bucket_hash("a"))).await),
        ("no agent_hash", insert(Some(bucket_hash("l")), None).await),
    ] {
        let err = res.expect_err(what).to_string();
        assert!(
            err.contains("changes row without bucket hashes (migration 021)"),
            "{what}: {err}"
        );
    }
    insert(Some(bucket_hash("l")), Some(bucket_hash("a")))
        .await
        .expect("a row with both hashes is accepted (positive control)");
    for col in ["lineage_hash", "agent_hash"] {
        let err = sqlx::query(&format!("UPDATE changes SET {col} = NULL"))
            .execute(store.pool())
            .await
            .expect_err(col)
            .to_string();
        assert!(
            err.contains("changes bucket hashes cannot be cleared (migration 021)"),
            "{col}: {err}"
        );
    }
    assert_eq!(null_hashes(&store).await, 0);
}

/// Interrupted backfill: 021's SQL ran (the partial migrator applies it
/// without the Rust half) and left NULL hashes, some rows already filled.
/// `migrate()` resumes, fills the rest, and leaves the filled rows alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_backfill_resumes_on_the_next_migrate() {
    let (store, _dir) = bare_store().await;
    migrate_through(&store, 20).await;
    seed_020(&store, SEEDED).await;
    migrate_through(&store, 21).await;
    assert_eq!(
        null_hashes(&store).await,
        SEEDED as i64,
        "021's SQL alone fills nothing"
    );
    // A first batch that committed before the crash.
    sqlx::query("UPDATE changes SET lineage_hash = ?, agent_hash = ? WHERE seq <= 10")
        .bind("0".repeat(64))
        .bind("0".repeat(64))
        .execute(store.pool())
        .await
        .expect("pre-fill (both columns at once passes the update trigger)");
    store.migrate().await.expect("migrate resumes the backfill");
    assert_eq!(null_hashes(&store).await, 0);
    let (kept,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM changes WHERE seq <= 10 AND lineage_hash = ?")
            .bind("0".repeat(64))
            .fetch_one(store.pool())
            .await
            .expect("count");
    assert_eq!(kept, 10, "a filled row is never recomputed");
}

/// A backfill that cannot write fails `migrate()`, so the registry refuses
/// to start rather than serve rows no bucket can find; once the cause is
/// gone, `migrate()` succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_backfill_fails_migrate() {
    let (store, _dir) = bare_store().await;
    migrate_through(&store, 20).await;
    seed_020(&store, 3).await;
    migrate_through(&store, 21).await;
    sqlx::query(
        "CREATE TRIGGER block_backfill BEFORE UPDATE ON changes \
         BEGIN SELECT RAISE(ABORT, 'injected backfill failure'); END",
    )
    .execute(store.pool())
    .await
    .expect("inject the failure");
    let err = store
        .migrate()
        .await
        .expect_err("a failed backfill must fail migrate")
        .to_string();
    assert!(
        err.contains("changes backfill update") && err.contains("injected backfill failure"),
        "{err}"
    );
    assert_eq!(null_hashes(&store).await, 3, "the failed batch rolled back");
    sqlx::query("DROP TRIGGER block_backfill")
        .execute(store.pool())
        .await
        .expect("remove the failure");
    store
        .migrate()
        .await
        .expect("migrate succeeds once it can write");
    assert_hashes_are_over_the_ids(&store, 3).await;
}
