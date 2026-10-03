//! Postgres-backed [`SharedRateLimitBackend`] for the `/auth/*` ceilings
//! (FEAT-06 item 3).
//!
//! One atomic `INSERT … ON CONFLICT DO UPDATE` per check counts a fixed 60s
//! window in `rate_limit_windows`, so every replica sharing the database counts
//! once across the fleet. The database is the clock: `now()` is fixed at
//! statement start, so every replica floors to the same window boundary
//! regardless of its own clock.
//!
//! # Hot row
//!
//! `auth_global` is a single row updated by every `/auth/*` request fleet-wide,
//! a genuine serialization point. Each update holds the row lock for roughly
//! 0.5–2 ms (less on the UNLOGGED table), so one row sustains on the order of
//! 500–2000 updates/s — an estimate, not a measurement. The shipped
//! `global_per_minute = 6000` is ~100 updates/s. Sharding the global counter into
//! N summed rows is deliberately not built: it would break the single-statement
//! exactness and is not warranted until `global_per_minute` reaches the tens of
//! thousands across several replicas.
//!
//! # Differences from the in-memory limiter
//!
//! Both admit exactly `limit` requests per window and show clients the same
//! status and `Retry-After`. This backend also *counts* the rejecting request
//! (saturated at `limit + 1`) — the price of a single-statement design, and
//! invisible to clients.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use acdp_registry_store::{LimitDecision, SharedLimitScope, SharedRateLimitBackend};
use async_trait::async_trait;
use sqlx::{PgPool, Row};

/// The fixed window, in seconds. Matches the in-memory limiter's `WINDOW`.
const WINDOW_SECS: i64 = 60;

/// Minimum seconds between `warn!` lines about an unavailable database. A
/// database outage would otherwise log once per request.
const WARN_EVERY_SECS: i64 = 60;

/// `hits` is `INTEGER` and the saturation clamp is `limit + 1`, so the limit
/// must leave room for the `+ 1`.
const MAX_LIMIT: u32 = (i32::MAX - 1) as u32;

/// Atomic count-and-decide. `$1` scope, `$2` key, `$3` window seconds (`i64`),
/// `$4` the saturation clamp (`i32`, `limit + 1`).
///
/// * `floor(…)` rather than a bare `::bigint` cast, which rounds half away from
///   zero and would put the window boundary at X.5s.
/// * The window roll is monotonic (`GREATEST` + the `>` test): a statement whose
///   `now()` was fixed before it queued on the row lock must not roll the window
///   *backwards* and hand the next request a fresh budget. A stale straddler
///   instead counts into the newer window — the conservative direction.
/// * `LEAST(…, $4)` bounds `hits`, because a rejected request still writes.
/// * Must run autocommit: inside a transaction `now()` would go stale.
const CHECK_SQL: &str = "\
INSERT INTO rate_limit_windows AS b (scope, bucket_key, window_start, hits)
VALUES ($1, $2, (floor(EXTRACT(EPOCH FROM now()))::bigint / $3) * $3, 1)
ON CONFLICT (scope, bucket_key) DO UPDATE
   SET hits = CASE WHEN EXCLUDED.window_start > b.window_start
                   THEN 1
                   ELSE LEAST(b.hits + 1, $4) END,
       window_start = GREATEST(b.window_start, EXCLUDED.window_start)
RETURNING hits, window_start, floor(EXTRACT(EPOCH FROM now()))::bigint AS now_epoch";

/// A [`SharedRateLimitBackend`] backed by the registry's own Postgres pool.
pub struct PgRateLimitBackend {
    pool: PgPool,
    per_ip_limit: u32,
    global_limit: u32,
    timeout: Duration,
    /// Epoch seconds of the last `warn!` about an unavailable database.
    last_warn_epoch: AtomicI64,
}

impl PgRateLimitBackend {
    /// Borrow an existing pool (`PgStore::pool().clone()`): the limiter adds no
    /// connection pool of its own. `timeout` bounds every check; a timeout or
    /// any database error is [`LimitDecision::Unavailable`].
    pub fn new(pool: PgPool, per_ip_limit: u32, global_limit: u32, timeout: Duration) -> Self {
        Self {
            pool,
            per_ip_limit,
            global_limit,
            timeout,
            last_warn_epoch: AtomicI64::new(i64::MIN),
        }
    }

    /// Delete windows that started before `before_epoch` (epoch seconds).
    /// Table-wide: the return value counts every stale row, whoever wrote it.
    pub async fn prune(&self, before_epoch: i64) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM rate_limit_windows WHERE window_start < $1")
            .bind(before_epoch)
            .execute(&self.pool)
            .await
            .map(|r| r.rows_affected())
    }

    async fn run(&self, scope: SharedLimitScope, key: &str) -> Result<LimitDecision, sqlx::Error> {
        let (limit, key) = match scope {
            SharedLimitScope::PerIp => (self.per_ip_limit, key),
            // The global scope has one row; the caller's key is irrelevant.
            SharedLimitScope::Global => (self.global_limit, ""),
        };
        let limit = limit.min(MAX_LIMIT);
        // Autocommit, one statement: no `begin()`, no preceding SELECT.
        let row = sqlx::query(CHECK_SQL)
            .bind(scope.label())
            .bind(key)
            .bind(WINDOW_SECS)
            .bind(limit as i32 + 1)
            .fetch_one(&self.pool)
            .await?;
        let hits: i32 = row.try_get("hits")?;
        let window_start: i64 = row.try_get("window_start")?;
        let now_epoch: i64 = row.try_get("now_epoch")?;
        Ok(decide(hits, limit, window_start, now_epoch))
    }

    /// `warn!` at most once per [`WARN_EVERY_SECS`]; the rest at `debug!`.
    fn note_unavailable(&self, scope: SharedLimitScope, cause: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let last = self.last_warn_epoch.load(Ordering::Relaxed);
        let due = last == i64::MIN || now.saturating_sub(last) >= WARN_EVERY_SECS;
        if due
            && self
                .last_warn_epoch
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!(
                scope = scope.label(),
                cause,
                "shared rate-limit database unavailable"
            );
        } else {
            tracing::debug!(
                scope = scope.label(),
                cause,
                "shared rate-limit database unavailable"
            );
        }
    }
}

/// `allow ⟺ hits <= limit`; a denial's `Retry-After` is the remainder of the
/// window by the database's clock, clamped to `[1, 60]`. The lower bound means a
/// client never sees `Retry-After: 0`; the upper bound is reachable because a
/// straddler counted into a newer window computes more than a window.
fn decide(hits: i32, limit: u32, window_start: i64, now_epoch: i64) -> LimitDecision {
    if i64::from(hits) <= i64::from(limit) {
        return LimitDecision::Allow;
    }
    let remaining = window_start + WINDOW_SECS - now_epoch;
    LimitDecision::Deny {
        retry_after_seconds: remaining.clamp(1, WINDOW_SECS) as u64,
    }
}

#[async_trait]
impl SharedRateLimitBackend for PgRateLimitBackend {
    async fn check(&self, scope: SharedLimitScope, key: &str) -> LimitDecision {
        match tokio::time::timeout(self.timeout, self.run(scope, key)).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(e)) => {
                self.note_unavailable(scope, &e.to_string());
                LimitDecision::Unavailable
            }
            Err(_) => {
                self.note_unavailable(scope, "timeout");
                LimitDecision::Unavailable
            }
        }
    }

    fn backend_name(&self) -> &'static str {
        "postgres"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_allows_up_to_the_limit_and_denies_past_it() {
        assert_eq!(decide(3, 3, 0, 0), LimitDecision::Allow);
        assert!(matches!(decide(4, 3, 0, 0), LimitDecision::Deny { .. }));
    }

    #[test]
    fn decide_retry_after_is_clamped_to_one_through_sixty() {
        // Mid-window: the exact remainder.
        assert_eq!(
            decide(9, 3, 100, 130),
            LimitDecision::Deny {
                retry_after_seconds: 30
            }
        );
        // Window already over by the DB clock: floor at 1, never 0.
        assert_eq!(
            decide(9, 3, 100, 160),
            LimitDecision::Deny {
                retry_after_seconds: 1
            }
        );
        assert_eq!(
            decide(9, 3, 100, 500),
            LimitDecision::Deny {
                retry_after_seconds: 1
            }
        );
        // A straddler counted into a newer window computes > 60: cap at 60.
        assert_eq!(
            decide(9, 3, 160, 100),
            LimitDecision::Deny {
                retry_after_seconds: 60
            }
        );
    }

    #[test]
    fn max_limit_leaves_room_for_the_saturation_clamp() {
        assert_eq!(i64::from(MAX_LIMIT) + 1, i64::from(i32::MAX));
    }
}
