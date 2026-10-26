//! RFC-ACDP-0018 §8 change-hint subscriptions on Postgres (migration 018).
//!
//! A create takes a transaction-scoped advisory lock on the owner before
//! counting their rows, so concurrent creates by one owner serialize and
//! `max_subscriptions_per_requester` is never exceeded. Creates by
//! different owners do not contend (barring a 32-bit hash collision, which
//! only serializes them).

use acdp::error::AcdpError;
use acdp::types::change::SubscriptionStatus;
use acdp_registry_store::subscriptions::{
    filter_json, parse_filter, parse_status, status_str, CreateSubscriptionOutcome,
    NewSubscription, SubscriptionCounts, SubscriptionProgress, SubscriptionRecord,
};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

/// First half of the two-key `pg_advisory_xact_lock(int4, int4)` taken per
/// owner on create; the second half is `hashtext(owner_did)`. The two-key
/// space is disjoint from the single-bigint keys used elsewhere.
const OWNER_LOCK_NAMESPACE: i32 = 0x0018_0005;

fn db_err(what: &'static str) -> impl Fn(sqlx::Error) -> AcdpError {
    move |e| AcdpError::RegistryInternal(format!("subscriptions {what}: {e}"))
}

const COLUMNS: &str = "subscription_id, owner_did, tenant_id, filter::text AS filter, \
     callback_url, secret_ct, secret_key_id, status, created_at_ms, last_hinted_seq, \
     next_attempt_at_ms, failing_since_ms, attempts, lease_until_ms";

pub(crate) async fn create(
    pool: &PgPool,
    new: &NewSubscription,
    max_per_owner: u64,
) -> Result<CreateSubscriptionOutcome, AcdpError> {
    let filter = filter_json(&new.filter)?;
    let mut tx = pool.begin().await.map_err(db_err("begin"))?;
    sqlx::query("SELECT pg_advisory_xact_lock($1, hashtext($2))")
        .bind(OWNER_LOCK_NAMESPACE)
        .bind(&new.owner_did)
        .execute(&mut *tx)
        .await
        .map_err(db_err("lock"))?;
    let (held,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM subscriptions WHERE owner_did = $1")
        .bind(&new.owner_did)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err("count"))?;
    if u64::try_from(held).unwrap_or(0) >= max_per_owner {
        return Ok(CreateSubscriptionOutcome::LimitReached);
    }
    sqlx::query(
        "INSERT INTO subscriptions (subscription_id, owner_did, tenant_id, filter, callback_url, \
         secret_ct, secret_key_id, status, created_at_ms, last_hinted_seq, next_attempt_at_ms, \
         failing_since_ms, attempts, lease_until_ms) \
         VALUES ($1,$2,$3,$4::jsonb,$5,$6,$7,$8,$9,$10,$9,NULL,0,NULL)",
    )
    .bind(&new.subscription_id)
    .bind(&new.owner_did)
    .bind(&new.tenant_id)
    .bind(&filter)
    .bind(&new.callback_url)
    .bind(&new.secret_ct)
    .bind(i16::from(new.secret_key_id))
    .bind(status_str(SubscriptionStatus::Active))
    .bind(new.created_at_ms)
    .bind(i64::try_from(new.last_hinted_seq).unwrap_or(i64::MAX))
    .execute(&mut *tx)
    .await
    .map_err(db_err("insert"))?;
    tx.commit().await.map_err(db_err("commit"))?;
    Ok(CreateSubscriptionOutcome::Created)
}

pub(crate) async fn get(
    pool: &PgPool,
    owner_did: &str,
    subscription_id: &str,
) -> Result<Option<SubscriptionRecord>, AcdpError> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM subscriptions WHERE subscription_id = $1 AND owner_did = $2"
    ))
    .bind(subscription_id)
    .bind(owner_did)
    .fetch_optional(pool)
    .await
    .map_err(db_err("get"))?;
    row.as_ref().map(record).transpose()
}

pub(crate) async fn delete(
    pool: &PgPool,
    owner_did: &str,
    subscription_id: &str,
) -> Result<bool, AcdpError> {
    let done =
        sqlx::query("DELETE FROM subscriptions WHERE subscription_id = $1 AND owner_did = $2")
            .bind(subscription_id)
            .bind(owner_did)
            .execute(pool)
            .await
            .map_err(db_err("delete"))?;
    Ok(done.rows_affected() > 0)
}

/// Lease up to `limit` due subscriptions in one statement. `FOR UPDATE SKIP LOCKED` in the subselect makes concurrent claimants on
/// other replicas skip rows already being leased instead of waiting on them.
pub(crate) async fn claim_due(
    pool: &PgPool,
    now_ms: i64,
    head: u64,
    lease_until_ms: i64,
    limit: u32,
) -> Result<Vec<SubscriptionRecord>, AcdpError> {
    let rows = sqlx::query(&format!(
        "UPDATE subscriptions SET lease_until_ms = $1 \
         WHERE subscription_id IN (\
           SELECT subscription_id FROM subscriptions \
           WHERE status = 'active' AND next_attempt_at_ms <= $2 \
             AND (lease_until_ms IS NULL OR lease_until_ms < $3) \
             AND last_hinted_seq < $4 \
           ORDER BY next_attempt_at_ms LIMIT $5 FOR UPDATE SKIP LOCKED) \
         RETURNING {COLUMNS}"
    ))
    .bind(lease_until_ms)
    .bind(now_ms)
    .bind(now_ms)
    .bind(i64::try_from(head).unwrap_or(i64::MAX))
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(db_err("claim"))?;
    rows.iter().map(record).collect()
}

/// Write an attempt's outcome and release the lease, if `lease_until_ms`
/// is still the row's lease.
pub(crate) async fn finish_attempt(
    pool: &PgPool,
    subscription_id: &str,
    lease_until_ms: i64,
    p: &SubscriptionProgress,
) -> Result<bool, AcdpError> {
    let done = sqlx::query(
        "UPDATE subscriptions SET last_hinted_seq = $1, status = $2, \
         next_attempt_at_ms = $3, failing_since_ms = $4, attempts = $5, \
         lease_until_ms = NULL \
         WHERE subscription_id = $6 AND lease_until_ms = $7",
    )
    .bind(i64::try_from(p.last_hinted_seq).unwrap_or(i64::MAX))
    .bind(status_str(p.status))
    .bind(p.next_attempt_at_ms)
    .bind(p.failing_since_ms)
    .bind(i32::try_from(p.attempts).unwrap_or(i32::MAX))
    .bind(subscription_id)
    .bind(lease_until_ms)
    .execute(pool)
    .await
    .map_err(db_err("finish"))?;
    Ok(done.rows_affected() > 0)
}

pub(crate) async fn counts(pool: &PgPool) -> Result<SubscriptionCounts, AcdpError> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM subscriptions GROUP BY status")
            .fetch_all(pool)
            .await
            .map_err(db_err("count"))?;
    let mut out = SubscriptionCounts::default();
    for (status, n) in rows {
        let n = u64::try_from(n).unwrap_or(0);
        match parse_status(&status)? {
            SubscriptionStatus::Active => out.active = n,
            SubscriptionStatus::Disabled => out.disabled = n,
        }
    }
    Ok(out)
}

fn record(r: &PgRow) -> Result<SubscriptionRecord, AcdpError> {
    let get = db_err("decode");
    let key_id: i16 = r.try_get("secret_key_id").map_err(&get)?;
    let seq: i64 = r.try_get("last_hinted_seq").map_err(&get)?;
    let attempts: i32 = r.try_get("attempts").map_err(&get)?;
    let status: String = r.try_get("status").map_err(&get)?;
    let filter: String = r.try_get("filter").map_err(&get)?;
    Ok(SubscriptionRecord {
        subscription_id: r.try_get("subscription_id").map_err(&get)?,
        owner_did: r.try_get("owner_did").map_err(&get)?,
        tenant_id: r.try_get("tenant_id").map_err(&get)?,
        filter: parse_filter(&filter)?,
        callback_url: r.try_get("callback_url").map_err(&get)?,
        secret_ct: r.try_get("secret_ct").map_err(&get)?,
        secret_key_id: u8::try_from(key_id).map_err(|_| {
            AcdpError::RegistryInternal(format!("stored secret_key_id {key_id} is out of range"))
        })?,
        status: parse_status(&status)?,
        created_at_ms: r.try_get("created_at_ms").map_err(&get)?,
        last_hinted_seq: u64::try_from(seq).unwrap_or(0),
        next_attempt_at_ms: r.try_get("next_attempt_at_ms").map_err(&get)?,
        failing_since_ms: r.try_get("failing_since_ms").map_err(&get)?,
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        lease_until_ms: r.try_get("lease_until_ms").map_err(&get)?,
    })
}
