//! RFC-ACDP-0018 §8 change-hint subscriptions on SQLite (migration 018).
//!
//! A create counts the owner's rows and inserts under one `BEGIN
//! IMMEDIATE` transaction, so the single-writer lock makes the
//! `max_subscriptions_per_requester` check and the insert atomic.

use acdp::error::AcdpError;
use acdp::types::change::SubscriptionStatus;
use acdp_registry_store::subscriptions::{
    filter_json, parse_filter, parse_status, status_str, CreateSubscriptionOutcome,
    NewSubscription, SubscriptionRecord,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

fn db_err(what: &'static str) -> impl Fn(sqlx::Error) -> AcdpError {
    move |e| AcdpError::RegistryInternal(format!("subscriptions {what}: {e}"))
}

const COLUMNS: &str = "subscription_id, owner_did, tenant_id, filter, callback_url, secret_ct, \
     secret_key_id, status, created_at_ms, last_hinted_seq, next_attempt_at_ms, \
     failing_since_ms, attempts, lease_until_ms";

pub(crate) async fn create(
    pool: &SqlitePool,
    new: &NewSubscription,
    max_per_owner: u64,
) -> Result<CreateSubscriptionOutcome, AcdpError> {
    let filter = filter_json(&new.filter)?;
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(db_err("begin"))?;
    let (held,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM subscriptions WHERE owner_did = ?")
        .bind(&new.owner_did)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err("count"))?;
    if u64::try_from(held).unwrap_or(0) >= max_per_owner {
        return Ok(CreateSubscriptionOutcome::LimitReached);
    }
    sqlx::query(&format!(
        "INSERT INTO subscriptions ({COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?,NULL,0,NULL)"
    ))
    .bind(&new.subscription_id)
    .bind(&new.owner_did)
    .bind(&new.tenant_id)
    .bind(&filter)
    .bind(&new.callback_url)
    .bind(&new.secret_ct)
    .bind(i64::from(new.secret_key_id))
    .bind(status_str(SubscriptionStatus::Active))
    .bind(new.created_at_ms)
    .bind(i64::try_from(new.last_hinted_seq).unwrap_or(i64::MAX))
    .bind(new.created_at_ms)
    .execute(&mut *tx)
    .await
    .map_err(db_err("insert"))?;
    tx.commit().await.map_err(db_err("commit"))?;
    Ok(CreateSubscriptionOutcome::Created)
}

pub(crate) async fn get(
    pool: &SqlitePool,
    owner_did: &str,
    subscription_id: &str,
) -> Result<Option<SubscriptionRecord>, AcdpError> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM subscriptions WHERE subscription_id = ? AND owner_did = ?"
    ))
    .bind(subscription_id)
    .bind(owner_did)
    .fetch_optional(pool)
    .await
    .map_err(db_err("get"))?;
    row.as_ref().map(record).transpose()
}

pub(crate) async fn delete(
    pool: &SqlitePool,
    owner_did: &str,
    subscription_id: &str,
) -> Result<bool, AcdpError> {
    let done = sqlx::query("DELETE FROM subscriptions WHERE subscription_id = ? AND owner_did = ?")
        .bind(subscription_id)
        .bind(owner_did)
        .execute(pool)
        .await
        .map_err(db_err("delete"))?;
    Ok(done.rows_affected() > 0)
}

fn record(r: &SqliteRow) -> Result<SubscriptionRecord, AcdpError> {
    let get = db_err("decode");
    let key_id: i64 = r.try_get("secret_key_id").map_err(&get)?;
    let seq: i64 = r.try_get("last_hinted_seq").map_err(&get)?;
    let attempts: i64 = r.try_get("attempts").map_err(&get)?;
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
