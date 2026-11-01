//! RFC-ACDP-0018 change feed on Postgres: every statement against the
//! `changes` / `changes_meta` tables (migration 017).
//!
//! Appends run inside the caller's transaction as its LAST statements and
//! take [`FEED_APPEND_LOCK_KEY`] first. The identity value is drawn while
//! that lock is held and the lock is only released after the commit is
//! visible, so `seq` order is commit order (§5.3): a reader can never see
//! seq n+1 committed while n is still in flight. Being last keeps the lock
//! order rows → log lock → feed lock in every writer, and keeps the hold
//! time to the final insert plus commit.
//!
//! Reads take one REPEATABLE READ, READ ONLY transaction, so the rows and
//! both markers come from one snapshot.

use acdp::error::AcdpError;
use acdp::types::body::Body;
use acdp::types::change::ChangeKind;
use acdp::types::lifecycle::LifecycleEvent;
use acdp::types::primitives::CtxId;
use acdp::types::EventReceipt;
use acdp_registry_store::changes::{
    feed_appends_for_publish, feed_fingerprint, lifecycle_append, FeedAppend, FeedFingerprint,
    FeedHead, FeedPage, FeedQuery, FeedRow, Predecessor,
};
use acdp_registry_store::event_receipts::receipt_value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};

/// `pg_advisory_xact_lock` key serializing feed appends; see the module
/// docs. Distinct from the transparency log's key. Public so tests can hold
/// it to prove appends wait for it.
pub const FEED_APPEND_LOCK_KEY: i64 = 0x00AC_D900_0018;

/// The DB clock in epoch milliseconds (`clock_timestamp`, not the
/// transaction-start `now()`, so a row appended after a lock wait is not
/// backdated).
const NOW_MS: &str = "(EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT";

fn db_err(what: &'static str) -> impl Fn(sqlx::Error) -> AcdpError {
    move |e| AcdpError::RegistryInternal(format!("changes {what}: {e}"))
}

/// Append the `lifecycle` row for `event`, just written to `body`'s
/// context with `receipt` (none without a minter, and then nothing is
/// appended). Call as the transaction's last statement before commit.
pub(crate) async fn append_lifecycle(
    tx: &mut Transaction<'_, Postgres>,
    event: &LifecycleEvent,
    body: &Body,
    receipt: Option<&EventReceipt>,
) -> Result<(), AcdpError> {
    let receipt = receipt.map(receipt_value).transpose()?;
    let appends: Vec<FeedAppend> = lifecycle_append(event, body, receipt.as_ref())?
        .into_iter()
        .collect();
    insert_all(tx, &appends).await
}

/// Append the rows for an accepted publish of `body` (`superseded` for
/// `supersedes`, `key_revocation` for a revocation). Call on the Inserted
/// path only, after the idempotency gate, as the transaction's last
/// statement before commit.
pub(crate) async fn append_publish(
    tx: &mut Transaction<'_, Postgres>,
    body: &Body,
    receipt: Option<&serde_json::Value>,
    supersedes: Option<&CtxId>,
) -> Result<(), AcdpError> {
    if receipt.is_none() {
        return Ok(());
    }
    let predecessor = match supersedes {
        Some(prev) => {
            let (agent_id,): (String,) =
                sqlx::query_as("SELECT agent_id FROM contexts WHERE ctx_id = $1")
                    .bind(prev.as_str())
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(db_err("predecessor"))?;
            Some(Predecessor {
                ctx_id: prev.as_str().to_string(),
                agent_id,
            })
        }
        None => None,
    };
    let appends = feed_appends_for_publish(body, receipt, predecessor.as_ref())?;
    insert_all(tx, &appends).await
}

async fn insert_all(
    tx: &mut Transaction<'_, Postgres>,
    appends: &[FeedAppend],
) -> Result<(), AcdpError> {
    if appends.is_empty() {
        return Ok(());
    }
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(FEED_APPEND_LOCK_KEY)
        .execute(&mut **tx)
        .await
        .map_err(db_err("lock"))?;
    for a in appends {
        sqlx::query(&format!(
            "INSERT INTO changes \
             (kind, ctx_id, lineage_id, agent_id, superseded_by, event, receipt, appended_at_ms) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,{NOW_MS})"
        ))
        .bind(a.kind.as_str())
        .bind(&a.ctx_id)
        .bind(&a.lineage_id)
        .bind(&a.agent_id)
        .bind(&a.superseded_by)
        .bind(&a.event)
        .bind(&a.receipt)
        .execute(&mut **tx)
        .await
        .map_err(db_err("append"))?;
    }
    Ok(())
}

async fn read_head(tx: &mut Transaction<'_, Postgres>) -> Result<FeedHead, AcdpError> {
    let (pruned, pruned_fp): (i64, Option<Vec<u8>>) =
        sqlx::query_as("SELECT pruned_through, pruned_through_fp FROM changes_meta WHERE id = 1")
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err("head"))?;
    let top: Option<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT seq, appended_at_ms, kind, ctx_id FROM changes ORDER BY seq DESC LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err("head"))?;
    let top = top
        .map(|(seq, at, kind, ctx_id)| fingerprint_columns(seq, at, &kind, &ctx_id))
        .transpose()?;
    Ok(FeedHead::from_snapshot(
        top,
        pruned.max(0) as u64,
        // A stored value of the wrong width is unusable, like a missing one.
        pruned_fp.and_then(|fp| FeedFingerprint::try_from(fp).ok()),
    ))
}

/// `(seq, fingerprint)` from a row's fingerprinted columns.
fn fingerprint_columns(
    seq: i64,
    appended_at_ms: i64,
    kind: &str,
    ctx_id: &str,
) -> Result<(u64, FeedFingerprint), AcdpError> {
    let kind = ChangeKind::parse(kind).map_err(|e| {
        AcdpError::RegistryInternal(format!("stored change {seq} has a bad kind: {e}"))
    })?;
    let seq = u64::try_from(seq)
        .map_err(|_| AcdpError::RegistryInternal(format!("stored change seq {seq} is negative")))?;
    Ok((seq, feed_fingerprint(seq, appended_at_ms, kind, ctx_id)))
}

async fn snapshot(pool: &PgPool) -> Result<Transaction<'static, Postgres>, AcdpError> {
    pool.begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .map_err(db_err("begin"))
}

pub(crate) async fn head(pool: &PgPool) -> Result<FeedHead, AcdpError> {
    let mut tx = snapshot(pool).await?;
    let head = read_head(&mut tx).await?;
    tx.commit().await.map_err(db_err("commit"))?;
    Ok(head)
}

pub(crate) async fn page(pool: &PgPool, q: &FeedQuery) -> Result<FeedPage, AcdpError> {
    let mut tx = snapshot(pool).await?;
    let head = read_head(&mut tx).await?;
    let mut sql = QueryBuilder::<Postgres>::new(
        "SELECT seq, kind, ctx_id, lineage_id, agent_id, superseded_by, event, receipt, \
         appended_at_ms FROM changes WHERE seq > ",
    );
    sql.push_bind(i64::try_from(q.after).unwrap_or(i64::MAX));
    let arms: [(&str, &Vec<String>); 3] = [
        ("ctx_id", &q.ctx_ids),
        ("lineage_id", &q.lineage_ids),
        ("agent_id", &q.agent_ids),
    ];
    let mut ors: Vec<(&str, &Vec<String>, bool)> = arms
        .into_iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(c, ids)| (c, ids, false))
        .collect();
    if !q.revocation_agents.is_empty() {
        ors.push(("agent_id", &q.revocation_agents, true));
    }
    if !ors.is_empty() {
        sql.push(" AND (");
        for (i, (col, ids, revocation_only)) in ors.into_iter().enumerate() {
            if i > 0 {
                sql.push(" OR ");
            }
            if revocation_only {
                sql.push("(kind = 'key_revocation' AND ");
            }
            sql.push(col).push(" = ANY(");
            sql.push_bind(ids.clone());
            sql.push(")");
            if revocation_only {
                sql.push(")");
            }
        }
        sql.push(")");
    }
    if !q.kinds.is_empty() {
        let kinds: Vec<String> = q.kinds.iter().map(|k| k.as_str().to_string()).collect();
        sql.push(" AND kind = ANY(");
        sql.push_bind(kinds);
        sql.push(")");
    }
    sql.push(" ORDER BY seq ASC LIMIT ");
    sql.push_bind(i64::from(q.limit()));
    let rows = sql
        .build()
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err("page"))?;
    tx.commit().await.map_err(db_err("commit"))?;
    let rows = rows
        .iter()
        .map(row_to_feed)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FeedPage::from_scan(q, rows, head))
}

fn row_to_feed(r: &PgRow) -> Result<FeedRow, AcdpError> {
    let get = db_err("decode");
    let kind: String = r.try_get("kind").map_err(&get)?;
    FeedRow::from_columns(
        r.try_get("seq").map_err(&get)?,
        &kind,
        r.try_get("ctx_id").map_err(&get)?,
        r.try_get("lineage_id").map_err(&get)?,
        r.try_get("agent_id").map_err(&get)?,
        r.try_get("superseded_by").map_err(&get)?,
        r.try_get("event").map_err(&get)?,
        r.try_get("receipt").map_err(&get)?,
        r.try_get("appended_at_ms").map_err(&get)?,
    )
}

pub(crate) async fn prune(pool: &PgPool, older_than_ms: i64, batch: u32) -> Result<u64, AcdpError> {
    let mut tx = pool.begin().await.map_err(db_err("begin"))?;
    // Serialize prunes (two replicas' retention loops) on the marker row.
    sqlx::query("SELECT pruned_through FROM changes_meta WHERE id = 1 FOR UPDATE")
        .execute(&mut *tx)
        .await
        .map_err(db_err("prune lock"))?;
    let oldest: Vec<(i64, i64)> =
        sqlx::query_as("SELECT seq, appended_at_ms FROM changes ORDER BY seq ASC LIMIT $1")
            .bind(i64::from(batch.max(1)))
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err("prune scan"))?;
    // Only the seq prefix older than the cutoff: stop at the first young
    // row so a row is never deleted while an older-seq one survives.
    let Some(last) = oldest
        .iter()
        .take_while(|(_, at)| *at < older_than_ms)
        .map(|(seq, _)| *seq)
        .last()
    else {
        return Ok(0);
    };
    // Fingerprint the new boundary row before it goes.
    let boundary = boundary_fp(&mut tx, last).await?;
    let deleted = sqlx::query("DELETE FROM changes WHERE seq <= $1")
        .bind(last)
        .execute(&mut *tx)
        .await
        .map_err(db_err("prune delete"))?
        .rows_affected();
    // Both columns read the old `pruned_through`: the fingerprint moves only
    // with the marker.
    sqlx::query(
        "UPDATE changes_meta SET \
         pruned_through_fp = CASE WHEN $1 >= pruned_through THEN $2 ELSE pruned_through_fp END, \
         pruned_through = GREATEST(pruned_through, $1) WHERE id = 1",
    )
    .bind(last)
    .bind(boundary.to_vec())
    .execute(&mut *tx)
    .await
    .map_err(db_err("prune marker"))?;
    tx.commit().await.map_err(db_err("commit"))?;
    Ok(deleted)
}

/// [`FeedRow::fingerprint`] of the row at `seq`, which the caller is about to
/// prune and holds in its scan.
async fn boundary_fp(
    tx: &mut Transaction<'_, Postgres>,
    seq: i64,
) -> Result<FeedFingerprint, AcdpError> {
    let (appended_at_ms, kind, ctx_id): (i64, String, String) =
        sqlx::query_as("SELECT appended_at_ms, kind, ctx_id FROM changes WHERE seq = $1")
            .bind(seq)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err("prune boundary"))?;
    Ok(fingerprint_columns(seq, appended_at_ms, &kind, &ctx_id)?.1)
}
