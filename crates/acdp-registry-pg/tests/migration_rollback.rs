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
use acdp_registry_store::ExtendedRegistryStore;
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
