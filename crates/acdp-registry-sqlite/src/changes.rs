//! RFC-ACDP-0018 change feed on SQLite: every statement against the
//! `changes` / `changes_meta` tables (migration 017).
//!
//! Appends run inside the caller's `BEGIN IMMEDIATE` transaction, so they
//! commit or roll back with the change they record, and the single-writer
//! lock makes `seq` order commit order. Reads take one deferred read
//! transaction, which in WAL mode is a consistent snapshot of the rows and
//! both markers.

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
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};

fn db_err(what: &'static str) -> impl Fn(sqlx::Error) -> AcdpError {
    move |e| AcdpError::RegistryInternal(format!("changes {what}: {e}"))
}

/// Append the `lifecycle` row for `event`, just written to `body`'s
/// context with `receipt` (none without a minter, and then nothing is
/// appended). Call as the transaction's last statement before commit.
pub(crate) async fn append_lifecycle(
    tx: &mut Transaction<'_, Sqlite>,
    event: &LifecycleEvent,
    body: &Body,
    receipt: Option<&EventReceipt>,
    now_ms: i64,
) -> Result<(), AcdpError> {
    let receipt = receipt.map(receipt_value).transpose()?;
    if let Some(append) = lifecycle_append(event, body, receipt.as_ref())? {
        insert(tx, &append, now_ms).await?;
    }
    Ok(())
}

/// Append the rows for an accepted publish of `body` (`superseded` for
/// `supersedes`, `key_revocation` for a revocation). Call on the Inserted
/// path only, as the transaction's last statement before commit.
pub(crate) async fn append_publish(
    tx: &mut Transaction<'_, Sqlite>,
    body: &Body,
    receipt: Option<&serde_json::Value>,
    supersedes: Option<&CtxId>,
    now_ms: i64,
) -> Result<(), AcdpError> {
    if receipt.is_none() {
        return Ok(());
    }
    let predecessor = match supersedes {
        Some(prev) => {
            let (agent_id,): (String,) =
                sqlx::query_as("SELECT agent_id FROM contexts WHERE ctx_id = ?")
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
    for append in feed_appends_for_publish(body, receipt, predecessor.as_ref())? {
        insert(tx, &append, now_ms).await?;
    }
    Ok(())
}

async fn insert(
    tx: &mut Transaction<'_, Sqlite>,
    a: &FeedAppend,
    now_ms: i64,
) -> Result<(), AcdpError> {
    sqlx::query(
        "INSERT INTO changes \
         (kind, ctx_id, lineage_id, agent_id, superseded_by, event, receipt, appended_at_ms) \
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(a.kind.as_str())
    .bind(&a.ctx_id)
    .bind(&a.lineage_id)
    .bind(&a.agent_id)
    .bind(&a.superseded_by)
    .bind(a.event.as_ref().map(|v| v.to_string()))
    .bind(a.receipt.to_string())
    .bind(now_ms)
    .execute(&mut **tx)
    .await
    .map_err(db_err("append"))?;
    Ok(())
}

async fn read_head(tx: &mut Transaction<'_, Sqlite>) -> Result<FeedHead, AcdpError> {
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

pub(crate) async fn head(pool: &SqlitePool) -> Result<FeedHead, AcdpError> {
    let mut tx = pool.begin().await.map_err(db_err("begin"))?;
    let head = read_head(&mut tx).await?;
    tx.commit().await.map_err(db_err("commit"))?;
    Ok(head)
}

pub(crate) async fn page(pool: &SqlitePool, q: &FeedQuery) -> Result<FeedPage, AcdpError> {
    let mut tx = pool.begin().await.map_err(db_err("begin"))?;
    let head = read_head(&mut tx).await?;
    let mut sql = QueryBuilder::<Sqlite>::new(
        "SELECT seq, kind, ctx_id, lineage_id, agent_id, superseded_by, event, receipt, \
         appended_at_ms FROM changes WHERE seq > ",
    );
    sql.push_bind(i64::try_from(q.after).unwrap_or(i64::MAX));
    let arms: [(&str, &[String]); 3] = [
        ("ctx_id", &q.ctx_ids),
        ("lineage_id", &q.lineage_ids),
        ("agent_id", &q.agent_ids),
    ];
    let id_clause = arms.iter().any(|(_, ids)| !ids.is_empty()) || !q.revocation_agents.is_empty();
    if id_clause {
        sql.push(" AND (");
        let mut first = true;
        for (col, ids) in arms {
            if ids.is_empty() {
                continue;
            }
            if !first {
                sql.push(" OR ");
            }
            first = false;
            sql.push(col).push(" IN (");
            let mut list = sql.separated(", ");
            for id in ids {
                list.push_bind(id.as_str());
            }
            list.push_unseparated(")");
        }
        if !q.revocation_agents.is_empty() {
            if !first {
                sql.push(" OR ");
            }
            sql.push("(kind = 'key_revocation' AND agent_id IN (");
            let mut list = sql.separated(", ");
            for id in &q.revocation_agents {
                list.push_bind(id.as_str());
            }
            list.push_unseparated("))");
        }
        sql.push(")");
    }
    if !q.kinds.is_empty() {
        sql.push(" AND kind IN (");
        let mut list = sql.separated(", ");
        for k in &q.kinds {
            list.push_bind(k.as_str());
        }
        list.push_unseparated(")");
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

fn row_to_feed(r: &sqlx::sqlite::SqliteRow) -> Result<FeedRow, AcdpError> {
    let get = db_err("decode");
    let json = |col: &str, raw: String| {
        serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
            AcdpError::RegistryInternal(format!("changes decode: column {col} is not JSON: {e}"))
        })
    };
    let event: Option<String> = r.try_get("event").map_err(&get)?;
    let receipt: String = r.try_get("receipt").map_err(&get)?;
    let kind: String = r.try_get("kind").map_err(&get)?;
    FeedRow::from_columns(
        r.try_get("seq").map_err(&get)?,
        &kind,
        r.try_get("ctx_id").map_err(&get)?,
        r.try_get("lineage_id").map_err(&get)?,
        r.try_get("agent_id").map_err(&get)?,
        r.try_get("superseded_by").map_err(&get)?,
        event.map(|e| json("event", e)).transpose()?,
        json("receipt", receipt)?,
        r.try_get("appended_at_ms").map_err(&get)?,
    )
}

pub(crate) async fn prune(
    pool: &SqlitePool,
    older_than_ms: i64,
    batch: u32,
) -> Result<u64, AcdpError> {
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(db_err("begin"))?;
    let oldest: Vec<(i64, i64)> =
        sqlx::query_as("SELECT seq, appended_at_ms FROM changes ORDER BY seq ASC LIMIT ?")
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
    let deleted = sqlx::query("DELETE FROM changes WHERE seq <= ?")
        .bind(last)
        .execute(&mut *tx)
        .await
        .map_err(db_err("prune delete"))?
        .rows_affected();
    // Both columns read the old `pruned_through`: the fingerprint moves only
    // with the marker.
    sqlx::query(
        "UPDATE changes_meta SET \
         pruned_through_fp = CASE WHEN ?1 >= pruned_through THEN ?2 ELSE pruned_through_fp END, \
         pruned_through = MAX(pruned_through, ?1) WHERE id = 1",
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
    tx: &mut Transaction<'_, Sqlite>,
    seq: i64,
) -> Result<FeedFingerprint, AcdpError> {
    let (appended_at_ms, kind, ctx_id): (i64, String, String) =
        sqlx::query_as("SELECT appended_at_ms, kind, ctx_id FROM changes WHERE seq = ?")
            .bind(seq)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err("prune boundary"))?;
    Ok(fingerprint_columns(seq, appended_at_ms, &kind, &ctx_id)?.1)
}
