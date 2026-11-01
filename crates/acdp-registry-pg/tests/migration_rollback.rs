//! N-1 rollback safety for `PgStore::migrate`
//! (`plans/rate-limit-shared-backend.md`, Phase 2).
//!
//! `sqlx::migrate!` defaults to `ignore_missing = false`, which makes the
//! migrator error when `_sqlx_migrations` holds a version the binary's
//! embedded set doesn't contain — i.e. a binary one release behind the
//! database crash-loops instead of serving. `PgStore::migrate` now sets
//! `ignore_missing(true)`, which tolerates that "database is ahead of me"
//! case while still checksum-verifying every migration the binary DOES
//! know about, so a corrupted or edited migration still fails startup.
//!
//! Both tests mutate `_sqlx_migrations`, which is global to the database.
//! `cargo test -p acdp-registry-pg` runs this whole crate in parallel
//! against one `ACDP_REGISTRY_TEST_PG_URL` database shared with
//! `parity.rs` and `store_contract.rs`, both of which call
//! `store.migrate()` — a synthetic higher version or a corrupted checksum
//! left in that shared database would fail every concurrent `migrate()`
//! call, not just this file's own assertions. So each test creates its
//! own throwaway database instead of touching the shared one.
//!
//! Gated the same way as the rest of this crate's tests: unset
//! `ACDP_REGISTRY_TEST_PG_URL` skips (with a printed line), and
//! `ACDP_REQUIRE_PG` turns that skip into a hard failure.

use std::str::FromStr;

use acdp_registry_pg::PgStore;
use acdp_registry_store::{bucket_hash, ExtendedRegistryStore};
use sqlx::migrate::Migrator;
use sqlx::postgres::PgConnectOptions;
use sqlx::{PgPool, Row};

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
            eprintln!("ACDP_REGISTRY_TEST_PG_URL unset; skipping pg migration rollback test");
            None
        }
    }
}

/// Swaps the path segment of a `postgres://...` URL for `db_name`, keeping
/// any query string. Deliberately not a general-purpose URL parser — this
/// only needs to handle the connection strings this repo actually uses
/// (`docker/docker-compose.yml`, CI's Postgres service).
fn with_database(url: &str, db_name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let authority_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let path_start = base[authority_end..]
        .find('/')
        .map(|i| authority_end + i)
        .unwrap_or(base.len());
    let mut out = format!("{}/{}", &base[..path_start], db_name);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    out
}

/// A maintenance connection against whatever database `url` names, used to
/// create/drop throwaway databases. `CREATE DATABASE`/`DROP DATABASE`
/// cannot run inside a transaction; a plain autocommit `execute` on a
/// single-connection pool is exactly that.
async fn maintenance_pool(url: &str) -> PgPool {
    // Validate the URL shape early with a clear panic rather than a cryptic
    // connection error, since `with_database` is hand-rolled.
    PgConnectOptions::from_str(url).expect("parse ACDP_REGISTRY_TEST_PG_URL");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .expect("connect maintenance pool")
}

async fn create_throwaway_database(base_url: &str, db_name: &str) {
    let pool = maintenance_pool(base_url).await;
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&pool)
        .await
        .expect("create throwaway database");
}

async fn drop_throwaway_database(base_url: &str, db_name: &str) {
    let pool = maintenance_pool(base_url).await;
    // Best-effort: terminate any lingering backends so DROP DATABASE
    // doesn't fail with "database is being accessed by other users". The
    // test's own PgStore pool should already be out of scope by the time
    // this runs, but sqlx pools close asynchronously in the background.
    let _ = sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = $1 AND pid <> pg_backend_pid()",
    )
    .bind(db_name)
    .execute(&pool)
    .await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS \"{db_name}\""))
        .execute(&pool)
        .await
        .expect("drop throwaway database");
}

/// Runs `body` against a fresh, uniquely-named throwaway database that is
/// dropped afterwards regardless of whether `body` panics. Uses
/// `tokio::spawn` + `JoinError::into_panic` to observe a panic without
/// `catch_unwind`, so no `unsafe` is needed (this workspace forbids it —
/// `Cargo.toml`'s `[workspace.lints.rust] unsafe_code = "forbid"`).
async fn with_throwaway_database<F, Fut>(base_url: &str, label: &str, body: F)
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let db_name = format!("acdp_rl_migrate_{label}_{}", uuid::Uuid::new_v4().simple());
    create_throwaway_database(base_url, &db_name).await;
    let db_url = with_database(base_url, &db_name);

    let result = tokio::spawn(body(db_url)).await;

    drop_throwaway_database(base_url, &db_name).await;

    if let Err(join_err) = result {
        std::panic::resume_unwind(join_err.into_panic());
    }
}

/// Criterion 3: a database carrying an unknown higher version no longer
/// fails `migrate()`.
#[tokio::test]
async fn migrate_tolerates_a_newer_unknown_version() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "newer", |db_url| async move {
        let store = PgStore::connect(&db_url, 2).await.expect("pg connect");
        store.migrate().await.expect("first migrate");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .expect("connect for synthetic version insert");
        sqlx::query(
            "INSERT INTO _sqlx_migrations \
             (version, description, installed_on, success, checksum, execution_time) \
             VALUES (999, 'synthetic future migration', now(), true, '\\x00'::bytea, 0)",
        )
        .execute(&pool)
        .await
        .expect("insert synthetic future migration row");

        store
            .migrate()
            .await
            .expect("migrate must tolerate an unknown higher version (N-1 rollback safety)");
    })
    .await;
}

/// Criterion 2: a checksum mismatch on an applied migration still fails.
/// Without this, criterion 1 could be satisfied by disabling verification
/// wholesale instead of by `ignore_missing`.
#[tokio::test]
async fn migrate_still_rejects_a_changed_checksum() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "checksum", |db_url| async move {
        let store = PgStore::connect(&db_url, 2).await.expect("pg connect");
        store.migrate().await.expect("first migrate");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .expect("connect for checksum corruption");
        let applied: i64 = sqlx::query("SELECT MIN(version) AS v FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .expect("read an applied migration version")
            .get("v");
        sqlx::query("UPDATE _sqlx_migrations SET checksum = '\\x00'::bytea WHERE version = $1")
            .bind(applied)
            .execute(&pool)
            .await
            .expect("corrupt the applied migration's checksum");

        let result = store.migrate().await;
        assert!(
            result.is_err(),
            "migrate() must still reject a checksum mismatch on an applied migration \
             (version {applied}); ignore_missing only tolerates a database AHEAD of the \
             binary, not a modified migration the binary already knows about"
        );
    })
    .await;
}

/// Migration 017: the feed tables, the seeded marker row and the shape
/// CHECKs exist on a fresh database, and re-running `migrate()` is a no-op.
#[tokio::test]
async fn migration_017_creates_the_change_feed() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "feed", |db_url| async move {
        let store = PgStore::connect(&db_url, 2).await.expect("pg connect");
        store.migrate().await.expect("first migrate");
        store
            .migrate()
            .await
            .expect("re-running migrate is a no-op");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .expect("connect");
        let marker: Vec<(i32, i64)> = sqlx::query_as("SELECT id, pruned_through FROM changes_meta")
            .fetch_all(&pool)
            .await
            .expect("read changes_meta");
        assert_eq!(marker, vec![(1, 0)], "one marker row, starting at 0");
        let indexes: Vec<(String,)> = sqlx::query_as(
            "SELECT indexname::text FROM pg_indexes \
             WHERE tablename = 'changes' AND indexname LIKE 'idx_changes_%' ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .expect("read pg_indexes");
        let found: Vec<&str> = indexes.iter().map(|(n,)| n.as_str()).collect();
        assert_eq!(
            found,
            vec![
                "idx_changes_agent",
                "idx_changes_agent_hash",
                "idx_changes_ctx",
                "idx_changes_lineage",
                "idx_changes_lineage_hash",
            ]
        );
        for (kind, event, superseded_by) in [
            ("lifecycle", None::<&str>, None::<&str>),
            ("superseded", None, None),
            ("key_revocation", Some("{}"), None),
            ("other", None, None),
        ] {
            let res = sqlx::query(
                "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, superseded_by, \
                 event, receipt, appended_at_ms) \
                 VALUES ($1, 'c', 'l', 'a', $2, $3::jsonb, '{}'::jsonb, 0)",
            )
            .bind(kind)
            .bind(superseded_by)
            .bind(event)
            .execute(&pool)
            .await;
            // Migration 021's trigger fills the hashes first (BEFORE
            // triggers run before CHECKs), so the refusal is the CHECK.
            let err = res.expect_err(kind).to_string();
            assert!(
                err.contains("violates check constraint"),
                "a malformed {kind} row must be refused by its CHECK: {err}"
            );
        }
        let res = sqlx::query("INSERT INTO changes_meta (id, pruned_through) VALUES (2, 0)")
            .execute(&pool)
            .await;
        assert!(res.is_err(), "changes_meta holds exactly one row");
    })
    .await;
}

/// Migration 019: the key-attestation index, its two indexes, the UNIQUE
/// token and the shape CHECKs exist on a fresh database, and re-running
/// `migrate()` is a no-op.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_019_creates_the_key_attestation_index() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "keyless", |db_url| async move {
        use acdp_registry_store::parity::keyless_fixtures::{
            commit, controller, controller_request, inserted,
        };
        let store = std::sync::Arc::new(PgStore::connect(&db_url, 2).await.expect("pg connect"));
        store.migrate().await.expect("first migrate");
        store
            .migrate()
            .await
            .expect("re-running migrate is a no-op");
        let pool = store.pool().clone();
        let indexes: Vec<(String,)> = sqlx::query_as(
            "SELECT indexname::text FROM pg_indexes \
             WHERE tablename = 'key_attestations' AND indexname LIKE 'idx_key_attestations_%' \
             ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .expect("read pg_indexes");
        let found: Vec<&str> = indexes.iter().map(|(n,)| n.as_str()).collect();
        assert_eq!(
            found,
            vec![
                "idx_key_attestations_controller",
                "idx_key_attestations_identity"
            ]
        );

        // Two real contexts, so each refusal is the CHECK or UNIQUE it
        // names and not the foreign key.
        let c = controller("pg-019");
        let a = inserted(
            commit(&store, controller_request(&c, None), None).await,
            "a",
        );
        let b = inserted(
            commit(&store, controller_request(&c, None), None).await,
            "b",
        );
        let (a, b) = (a.as_str().to_string(), b.as_str().to_string());
        let insert = |ctx: String,
                      ty: &'static str,
                      issuer: Option<&'static str>,
                      window: (i64, i64),
                      token: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO key_attestations (ctx_id, controller, identity_type, issuer, \
                     subject, org_wide, key_id, algorithm, key_fingerprint, not_before_ms, \
                     not_after_ms, token_hash, created_at_ms) \
                     VALUES ($1, 'c', $2, $3, 's', false, 'k', 'ed25519', 'f', $4, $5, $6, 0)",
                )
                .bind(ctx)
                .bind(ty)
                .bind(issuer)
                .bind(window.0)
                .bind(window.1)
                .bind(token)
                .execute(&pool)
                .await
            }
        };
        for (what, res) in [
            (
                "an unknown identity type",
                insert(a.clone(), "x509", None, (0, 1), "t1").await,
            ),
            (
                "an oidc row without an issuer",
                insert(a.clone(), "oidc", None, (0, 1), "t1").await,
            ),
            (
                "a spiffe row with an issuer",
                insert(a.clone(), "spiffe", Some("i"), (0, 1), "t1").await,
            ),
            (
                "an empty window",
                insert(a.clone(), "spiffe", None, (1, 1), "t1").await,
            ),
            (
                "a row with no context",
                insert("acdp://reg.test/none".into(), "spiffe", None, (0, 1), "t1").await,
            ),
        ] {
            assert!(res.is_err(), "{what} must be refused");
        }
        insert(a, "oidc", Some("i"), (0, 1), "t1")
            .await
            .expect("a well-formed row is accepted (positive control)");
        assert!(
            insert(b, "spiffe", None, (0, 1), "t1").await.is_err(),
            "token_hash is UNIQUE"
        );
        drop(pool);
        store.pool().close().await;
    })
    .await;
}

// ── Migration 021: §6.1 bucket hashes ────────────────────────────────────────

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

/// Rows seeded at 020 for the backfill test, enough for a rate that means
/// something (UPGRADING.md quotes it to size the production lock hold).
const BACKFILL_ROWS: i64 = 100_000;

async fn connect(db_url: &str) -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(db_url)
        .await
        .expect("connect")
}

/// Apply migrations `1..=through` only, from a tempdir copy. The copies
/// carry the same names and bytes, so `PgStore::migrate` later accepts them
/// as applied and runs only the rest, exactly as an upgrade would.
async fn migrate_through(pool: &PgPool, through: i64) {
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
        .run(pool)
        .await
        .expect("partial migrate");
}

/// Insert one row the way a 020 binary does: no hash columns.
async fn insert_020_row(pool: &PgPool, lineage_id: &str, agent_id: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, appended_at_ms) \
         VALUES ('key_revocation', 'acdp://reg.test/x', $1, $2, '{}'::jsonb, 0) RETURNING seq",
    )
    .bind(lineage_id)
    .bind(agent_id)
    .fetch_one(pool)
    .await
    .expect("020-shaped insert")
}

async fn hashes_of(pool: &PgPool, seq: i64) -> (String, String) {
    sqlx::query_as("SELECT lineage_hash, agent_hash FROM changes WHERE seq = $1")
        .bind(seq)
        .fetch_one(pool)
        .await
        .expect("read hashes")
}

/// 020 → 021 on a seeded feed: the in-migration backfill leaves no NULL and
/// matches the Rust derivation on every row, including the rec-010 vector
/// rows. The elapsed time gives the backfill rate UPGRADING.md quotes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_021_backfills_a_020_database() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "backfill", |db_url| async move {
        let pool = connect(&db_url).await;
        migrate_through(&pool, 20).await;
        for (i, (lineage, _)) in VECTORS.iter().enumerate() {
            insert_020_row(&pool, lineage, VECTORS[(i + 2) % 5].0).await;
        }
        // A receipt-sized JSONB per row, so the UPDATE rewrites tuples of
        // realistic width.
        sqlx::query(
            "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, appended_at_ms) \
             SELECT 'key_revocation', 'acdp://reg.test/' || g, \
                    'lin:sha256:' || encode(sha256(convert_to(g::text, 'UTF8')), 'hex'), \
                    'did:web:agents.test:seed-' || (g % 500), \
                    jsonb_build_object('receipt_version', 'acdp-receipt/1', \
                        'signature', repeat('x', 700), 'n', g), \
                    0 \
             FROM generate_series(1, $1) AS g",
        )
        .bind(BACKFILL_ROWS)
        .execute(&pool)
        .await
        .expect("seed 020 rows");
        let rows = BACKFILL_ROWS + VECTORS.len() as i64;

        let store = PgStore::connect(&db_url, 2).await.expect("pg connect");
        let started = std::time::Instant::now();
        store.migrate().await.expect("migrate 020 → 021");
        let elapsed = started.elapsed();
        eprintln!(
            "pg migration 021: {rows} rows backfilled and indexed in {:.3}s ({:.0} rows/s)",
            elapsed.as_secs_f64(),
            rows as f64 / elapsed.as_secs_f64()
        );

        let (nulls,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM changes WHERE lineage_hash IS NULL OR agent_hash IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count NULLs");
        assert_eq!(nulls, 0);
        let all: Vec<(String, String, String, String)> =
            sqlx::query_as("SELECT lineage_id, agent_id, lineage_hash, agent_hash FROM changes")
                .fetch_all(&pool)
                .await
                .expect("read rows");
        assert_eq!(all.len() as i64, rows);
        for (lineage, agent, lh, ah) in &all {
            assert_eq!(lh, &bucket_hash(lineage), "{lineage}");
            assert_eq!(ah, &bucket_hash(agent), "{agent}");
        }
        for (value, sha) in VECTORS {
            assert!(
                all.iter().any(|(l, _, lh, _)| l == value && lh == sha)
                    || all.iter().any(|(_, a, _, ah)| a == value && ah == sha),
                "{value}"
            );
        }

        // Diagnostic only: the planner's choice is data-dependent.
        let plan: Vec<(String,)> = sqlx::query_as(
            "EXPLAIN SELECT seq FROM changes WHERE seq > 0 \
             AND ((lineage_hash >= $1 AND lineage_hash < $2)) ORDER BY seq ASC LIMIT 100",
        )
        .bind(&VECTORS[0].1[..4])
        .bind("1f55")
        .fetch_all(&pool)
        .await
        .expect("explain");
        eprintln!(
            "pg 4-hex bucket plan: {}",
            plan.iter()
                .map(|(l,)| l.trim())
                .collect::<Vec<_>>()
                .join(" | ")
        );

        store
            .migrate()
            .await
            .expect("re-running migrate is a no-op");
        store.pool().close().await;
        pool.close().await;
    })
    .await;
}

/// The N-1 shim: after 021, an older binary's insert (no hash columns)
/// still gets the hashes the Rust derivation gives, on the five rec-010
/// vectors; a bound value is never overridden; and the column constraints
/// refuse a malformed or cleared hash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_021_trigger_fills_hashes_for_an_older_binary() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "shim", |db_url| async move {
        let store = PgStore::connect(&db_url, 2).await.expect("pg connect");
        store.migrate().await.expect("migrate");
        let pool = connect(&db_url).await;

        for (i, (value, sha)) in VECTORS.iter().enumerate() {
            let other = VECTORS[(i + 1) % 5];
            let seq = insert_020_row(&pool, value, other.0).await;
            let (lh, ah) = hashes_of(&pool, seq).await;
            assert_eq!((lh.as_str(), ah.as_str()), (*sha, other.1), "{value}");
            assert_eq!(lh, bucket_hash(value), "trigger and Rust agree: {value}");
        }

        // A value the binary bound wins over the trigger.
        let bound = "0".repeat(64);
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, appended_at_ms, \
             lineage_hash, agent_hash) \
             VALUES ('key_revocation', 'c', 'l', 'a', '{}'::jsonb, 0, $1, $1) RETURNING seq",
        )
        .bind(&bound)
        .fetch_one(&pool)
        .await
        .expect("insert with bound hashes");
        assert_eq!(hashes_of(&pool, seq).await, (bound.clone(), bound));

        for (what, sql, want) in [
            (
                "an upper-case hash",
                format!(
                    "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, \
                     appended_at_ms, lineage_hash) VALUES ('key_revocation', 'c', 'l', 'a', \
                     '{{}}'::jsonb, 0, '{}')",
                    "A".repeat(64)
                ),
                "changes_lineage_hash_hex",
            ),
            (
                "a short hash",
                "INSERT INTO changes (kind, ctx_id, lineage_id, agent_id, receipt, \
                 appended_at_ms, agent_hash) VALUES ('key_revocation', 'c', 'l', 'a', \
                 '{}'::jsonb, 0, '1f54')"
                    .to_string(),
                "changes_agent_hash_hex",
            ),
            (
                "a cleared hash",
                "UPDATE changes SET lineage_hash = NULL".to_string(),
                "not-null constraint",
            ),
        ] {
            let err = sqlx::query(&sql)
                .execute(&pool)
                .await
                .expect_err(what)
                .to_string();
            assert!(err.contains(want), "{what}: {err}");
        }
        store.pool().close().await;
        pool.close().await;
    })
    .await;
}

/// `SET LOCAL lock_timeout = '5s'`: with a reader holding a lock on
/// `changes`, 021 gives up within about 5 s (SQLSTATE 55P03) instead of
/// queueing every other reader and writer behind its ACCESS EXCLUSIVE
/// request, `migrate()` returns `Err`, and nothing is half-applied. Once
/// the reader is gone, `migrate()` succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_021_gives_up_on_a_held_lock() {
    let Some(base_url) = pg_url_or_skip() else {
        return;
    };
    with_throwaway_database(&base_url, "locktimeout", |db_url| async move {
        let pool = connect(&db_url).await;
        migrate_through(&pool, 20).await;
        let mut holder = pool.begin().await.expect("begin");
        sqlx::query("LOCK TABLE changes IN ACCESS SHARE MODE")
            .execute(&mut *holder)
            .await
            .expect("hold a reader's lock");

        let store = PgStore::connect(&db_url, 2).await.expect("pg connect");
        let started = std::time::Instant::now();
        let res = tokio::time::timeout(std::time::Duration::from_secs(60), store.migrate())
            .await
            .expect("migrate must give up, not hang");
        let elapsed = started.elapsed();
        let err = res.expect_err("021 cannot take its lock").to_string();
        assert!(
            err.contains("lock timeout"),
            "the 55P03 lock_timeout cancellation: {err}"
        );
        assert!(
            elapsed >= std::time::Duration::from_millis(4_500)
                && elapsed < std::time::Duration::from_secs(20),
            "gave up after {elapsed:?}, expected about 5 s"
        );
        let applied: Option<i64> =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success")
                .fetch_one(&pool)
                .await
                .expect("read _sqlx_migrations");
        assert_eq!(applied, Some(20), "021 rolled back whole");

        holder.rollback().await.expect("release the lock");
        store
            .migrate()
            .await
            .expect("migrate succeeds once the lock is free");
        let applied: Option<i64> =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success")
                .fetch_one(&pool)
                .await
                .expect("read _sqlx_migrations");
        assert_eq!(applied, Some(21));
        store.pool().close().await;
        pool.close().await;
    })
    .await;
}
