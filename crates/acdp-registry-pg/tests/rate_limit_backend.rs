//! `PgRateLimitBackend` against a real Postgres
//! (`plans/rate-limit-shared-backend.md`, Phase 3).
//!
//! Gated like the rest of this crate's suite: unset `ACDP_REGISTRY_TEST_PG_URL`
//! skips, and `ACDP_REQUIRE_PG` turns the skip into a hard failure. No
//! `#[serial]`: every test uses its own UUID-suffixed `bucket_key`, which is what
//! makes them safe in parallel against the one shared database.

use std::sync::Arc;
use std::time::{Duration, Instant};

use acdp_registry_pg::{PgRateLimitBackend, PgStore};
use acdp_registry_store::{
    ExtendedRegistryStore, LimitDecision, SharedLimitScope, SharedRateLimitBackend,
};
use sqlx::{PgPool, Row};

const TIMEOUT: Duration = Duration::from_secs(5);

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
            eprintln!("ACDP_REGISTRY_TEST_PG_URL unset; skipping pg rate-limit test");
            None
        }
    }
}

async fn pool(url: &str) -> PgPool {
    let store = PgStore::connect(url, 24).await.expect("pg connect");
    store.migrate().await.expect("pg migrate");
    let pool = store.pool().clone();
    avoid_window_boundary(&pool).await;
    pool
}

/// The database clock cannot be injected, so a 60s window boundary landing
/// inside a test's check sequence would flip its assertions. Start each test
/// early enough in a window that its few hundred milliseconds stay inside it.
async fn avoid_window_boundary(pool: &PgPool) {
    let secs_in_window = db_now(pool).await % 60;
    if secs_in_window >= 56 {
        tokio::time::sleep(Duration::from_secs((60 - secs_in_window) as u64 + 1)).await;
    }
}

fn backend(pool: &PgPool, per_ip: u32, global: u32) -> PgRateLimitBackend {
    PgRateLimitBackend::new(pool.clone(), per_ip, global, TIMEOUT)
}

/// Serialises work on the shared, key-independent `auth_global` row across test
/// binaries: `acdp-registry-server`'s `pg_integration` multi-replica proofs reset
/// and drain the same row, so a hit from here in the middle of one would corrupt
/// its count. Session-level advisory lock; released when the returned connection
/// is dropped. The constant must match `GLOBAL_ROW_LOCK` in `pg_integration.rs`.
const GLOBAL_ROW_LOCK: i64 = 0x0AC0_9F06_0003;

async fn lock_global_row(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("acquire lock connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(GLOBAL_ROW_LOCK)
        .execute(&mut *conn)
        .await
        .expect("advisory lock");
    conn
}

fn key() -> String {
    format!("test-{}", uuid::Uuid::new_v4())
}

async fn row(pool: &PgPool, scope: &str, key: &str) -> (i32, i64) {
    let r = sqlx::query(
        "SELECT hits, window_start FROM rate_limit_windows WHERE scope = $1 AND bucket_key = $2",
    )
    .bind(scope)
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("row exists");
    (r.get("hits"), r.get("window_start"))
}

async fn db_now(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT floor(EXTRACT(EPOCH FROM now()))::bigint")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn admits_exactly_the_limit_then_denies() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 3, 1000);
    let k = key();
    for i in 1..=3 {
        assert_eq!(
            b.check(SharedLimitScope::PerIp, &k).await,
            LimitDecision::Allow,
            "request {i}"
        );
    }
    assert!(matches!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Deny { .. }
    ));
    assert_eq!(b.backend_name(), "postgres");
}

#[tokio::test]
async fn deny_carries_retry_after_between_one_and_sixty() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 1, 1000);
    let k = key();
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Allow
    );
    for _ in 0..5 {
        match b.check(SharedLimitScope::PerIp, &k).await {
            LimitDecision::Deny {
                retry_after_seconds,
            } => {
                assert!(
                    (1..=60).contains(&retry_after_seconds),
                    "{retry_after_seconds}"
                )
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn window_rolls_and_restores_full_budget() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 2, 1000);
    let k = key();
    for _ in 0..2 {
        assert_eq!(
            b.check(SharedLimitScope::PerIp, &k).await,
            LimitDecision::Allow
        );
    }
    assert!(matches!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Deny { .. }
    ));
    // The clock is the database's, so the only way to move it is the row.
    sqlx::query("UPDATE rate_limit_windows SET window_start = window_start - 120 WHERE scope = 'auth_per_ip' AND bucket_key = $1")
        .bind(&k)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Allow
    );
    // `hits` came back as 1 — not limit + 2 — so the budget is genuinely whole.
    assert_eq!(row(&pool, "auth_per_ip", &k).await.0, 1);
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Allow
    );
    assert!(matches!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Deny { .. }
    ));
}

/// The headline assertion: a check-then-increment race would over-admit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_checks_admit_exactly_the_limit() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = Arc::new(backend(&pool, 25, 1000));
    let k = Arc::new(key());
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (b, k) = (b.clone(), k.clone());
        tasks.push(tokio::spawn(async move {
            let mut allowed = 0u32;
            let mut denied = 0u32;
            for _ in 0..10 {
                match b.check(SharedLimitScope::PerIp, &k).await {
                    LimitDecision::Allow => allowed += 1,
                    LimitDecision::Deny { .. } => denied += 1,
                    LimitDecision::Unavailable => panic!("database unavailable"),
                }
            }
            (allowed, denied)
        }));
    }
    let (mut allowed, mut denied) = (0, 0);
    for t in tasks {
        let (a, d) = t.await.unwrap();
        allowed += a;
        denied += d;
    }
    assert_eq!((allowed, denied), (25, 135));
}

#[tokio::test]
async fn global_and_per_ip_scopes_do_not_share_a_bucket() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    // The Global scope always lands on key ""; that row is shared with every
    // other test, so assert on per-IP independence with a per-IP-only key and
    // check the Global scope ignores the caller's key.
    let b = backend(&pool, 1, 1_000_000);
    let k = key();
    let _global_row = lock_global_row(&pool).await;
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Allow
    );
    assert!(matches!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Deny { .. }
    ));
    // Same key under the other scope: untouched budget (the global limit is huge).
    assert_eq!(
        b.check(SharedLimitScope::Global, &k).await,
        LimitDecision::Allow
    );
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rate_limit_windows WHERE scope = 'auth_global' AND bucket_key = $1",
    )
    .bind(&k)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        n, 0,
        "Global must be keyed by '' regardless of the caller's key"
    );
    assert_eq!(row(&pool, "auth_per_ip", &k).await.0, 2);
}

#[tokio::test]
async fn hits_saturate_at_limit_plus_one() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 3, 1000);
    let k = key();
    for _ in 0..200 {
        b.check(SharedLimitScope::PerIp, &k).await;
    }
    assert_eq!(row(&pool, "auth_per_ip", &k).await.0, 4);
}

/// The `GREATEST` guard as an executable assertion: under a naive
/// `window_start = EXCLUDED.window_start` roll this goes red.
#[tokio::test]
async fn a_stale_window_never_rolls_the_counter_backwards() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 3, 1000);
    let k = key();
    for _ in 0..3 {
        assert_eq!(
            b.check(SharedLimitScope::PerIp, &k).await,
            LimitDecision::Allow
        );
    }
    let (_, ws) = row(&pool, "auth_per_ip", &k).await;
    // Make the row look like a *newer* window than this statement's now().
    sqlx::query("UPDATE rate_limit_windows SET window_start = window_start + 60 WHERE scope = 'auth_per_ip' AND bucket_key = $1")
        .bind(&k)
        .execute(&pool)
        .await
        .unwrap();
    let decision = b.check(SharedLimitScope::PerIp, &k).await;
    assert!(
        matches!(decision, LimitDecision::Deny { .. }),
        "{decision:?}"
    );
    let (hits, ws_after) = row(&pool, "auth_per_ip", &k).await;
    assert_eq!(ws_after, ws + 60, "window_start must not move backwards");
    assert_eq!(hits, 4, "the stale request counts into the newer window");
}

#[tokio::test]
async fn retry_after_is_never_zero_and_never_above_sixty() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 1, 1000);
    let k = key();
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Allow
    );
    // Straddler: the row is a full window in the future, raw remainder > 60.
    sqlx::query("UPDATE rate_limit_windows SET window_start = window_start + 60 WHERE scope = 'auth_per_ip' AND bucket_key = $1")
        .bind(&k)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Deny {
            retry_after_seconds: 60
        }
    );
    // Window long over by the database clock but still the stored one: floor 1.
    sqlx::query("UPDATE rate_limit_windows SET window_start = window_start - 180, hits = 5 WHERE scope = 'auth_per_ip' AND bucket_key = $1")
        .bind(&k)
        .execute(&pool)
        .await
        .unwrap();
    // The check rolls the window (stale row), so it is allowed with a fresh
    // budget — never a Deny carrying 0.
    assert_eq!(
        b.check(SharedLimitScope::PerIp, &k).await,
        LimitDecision::Allow
    );
}

#[tokio::test]
async fn unreachable_database_reports_unavailable() {
    // No database needed: a pool pointed at a closed port.
    let dead = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(300))
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .expect("lazy pool");
    let b = PgRateLimitBackend::new(dead, 10, 10, Duration::from_millis(500));
    let start = Instant::now();
    assert_eq!(
        b.check(SharedLimitScope::PerIp, "k").await,
        LimitDecision::Unavailable
    );
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "must not hang: {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn prune_removes_only_stale_windows() {
    let Some(url) = pg_url_or_skip() else { return };
    let pool = pool(&url).await;
    let b = backend(&pool, 10, 1000);
    let (fresh, stale) = (key(), key());
    let now = db_now(&pool).await;
    for (k, ws) in [(&fresh, now), (&stale, now - 3600)] {
        sqlx::query("INSERT INTO rate_limit_windows (scope, bucket_key, window_start, hits) VALUES ('auth_per_ip', $1, $2, 1)")
            .bind(k)
            .bind(ws)
            .execute(&pool)
            .await
            .unwrap();
    }
    b.prune_older_than(300).await.expect("prune");
    // Assert on this test's own rows, never on `prune`'s table-wide count.
    let exists = |k: String| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM rate_limit_windows WHERE bucket_key = $1",
            )
            .bind(k)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(exists(fresh).await, 1);
    assert_eq!(exists(stale).await, 0);
}

#[tokio::test]
async fn migration_is_idempotent_and_table_is_unlogged() {
    let Some(url) = pg_url_or_skip() else { return };
    let store = PgStore::connect(&url, 4).await.expect("connect");
    store.migrate().await.expect("first migrate");
    store.migrate().await.expect("second migrate");
    let persistence: String = sqlx::query_scalar(
        "SELECT relpersistence::text FROM pg_class WHERE relname = 'rate_limit_windows'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persistence, "u", "rate_limit_windows must be UNLOGGED");
    // No index beyond the primary key (a window_start index defeats HOT updates).
    let idx: Vec<String> = sqlx::query_scalar(
        "SELECT indexname FROM pg_indexes WHERE tablename = 'rate_limit_windows'",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(idx, vec!["rate_limit_windows_pkey".to_string()]);
}
