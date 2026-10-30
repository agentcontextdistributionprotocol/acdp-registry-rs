//! RFC-ACDP-0021 key attestations on SQLite (migration 019).
//!
//! The three `*_gate`/`index_*` functions run inside `commit_publish`'s
//! `BEGIN IMMEDIATE` transaction, one call line each, at the points the
//! shared refusal order in `acdp_registry_store::key_attestations` names.
//! The single-writer lock makes each read there the committed state, so a
//! check and the insert that follows it cannot be separated by another
//! writer.

use acdp::error::AcdpError;
use acdp::types::body::Body;
use acdp::types::keyless::KeyAttestationListItem;
use acdp::types::publish::PublishRequest;
use acdp_registry_store::key_attestations::{
    self, admit_successor, identity_from_columns, index_row_for, keyless_successor, list_item,
    predecessor_attestation, predecessor_signer, stored_status, token_replayed,
    KeyAttestationFilter, KeyAttestationIndexRow, KeyAttestationRecord, SuccessorAttestation,
    MAX_LIST_LIMIT,
};
use acdp_registry_store::{decode_cursor, encode_cursor, Page};
use chrono::{DateTime, Utc};
use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};

fn db_err(what: &'static str) -> impl Fn(sqlx::Error) -> AcdpError {
    move |e| AcdpError::RegistryInternal(format!("key_attestations {what}: {e}"))
}

const ROW_COLUMNS: &str = "k.ctx_id, k.controller, k.identity_type, k.issuer, k.subject, \
     k.org_wide, k.key_id, k.algorithm, k.key_fingerprint, k.not_before_ms, k.not_after_ms, \
     k.token_hash, k.created_at_ms";

fn index_row(r: &SqliteRow) -> Result<KeyAttestationIndexRow, AcdpError> {
    let get = db_err("decode");
    let identity_type: String = r.try_get("identity_type").map_err(&get)?;
    let issuer: Option<String> = r.try_get("issuer").map_err(&get)?;
    let subject: String = r.try_get("subject").map_err(&get)?;
    let org_wide: i64 = r.try_get("org_wide").map_err(&get)?;
    Ok(KeyAttestationIndexRow {
        ctx_id: r.try_get("ctx_id").map_err(&get)?,
        controller: r.try_get("controller").map_err(&get)?,
        identity: identity_from_columns(&identity_type, issuer, subject)?,
        org_wide: org_wide != 0,
        key_id: r.try_get("key_id").map_err(&get)?,
        algorithm: r.try_get("algorithm").map_err(&get)?,
        key_fingerprint: r.try_get("key_fingerprint").map_err(&get)?,
        not_before_ms: r.try_get("not_before_ms").map_err(&get)?,
        not_after_ms: r.try_get("not_after_ms").map_err(&get)?,
        token_hash: r.try_get("token_hash").map_err(&get)?,
        created_at_ms: r.try_get("created_at_ms").map_err(&get)?,
    })
}

fn decode_body(json: &str) -> Result<Body, AcdpError> {
    serde_json::from_str(json).map_err(|e| AcdpError::RegistryInternal(format!("decode body: {e}")))
}

/// The index row at `ctx_id` and whether its context is retracted.
async fn successor(
    tx: &mut Transaction<'_, Sqlite>,
    ctx_id: &str,
) -> Result<Option<SuccessorAttestation>, AcdpError> {
    let row = sqlx::query(&format!(
        "SELECT {ROW_COLUMNS}, c.retracted FROM key_attestations k \
         JOIN contexts c ON c.ctx_id = k.ctx_id WHERE k.ctx_id = ?"
    ))
    .bind(ctx_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err("read"))?;
    row.map(|r| {
        let retracted: i64 = r.try_get("retracted").map_err(db_err("decode"))?;
        Ok(SuccessorAttestation {
            row: index_row(&r)?,
            withdrawn: retracted != 0,
        })
    })
    .transpose()
}

/// Steps 2 and 3 of the shared order, for a keyless successor with
/// `supersedes`. Call after AlreadySuperseded and before the RFC-ACDP-0014
/// admission; `predecessor` is the locked predecessor row, with `body_json`.
pub(crate) async fn supersession_gate(
    tx: &mut Transaction<'_, Sqlite>,
    req: &PublishRequest,
    predecessor: &SqliteRow,
) -> Result<(), AcdpError> {
    let Some(r) = keyless_successor(req) else {
        return Ok(());
    };
    let found = successor(tx, r.ctx_id.as_str()).await?;
    let succ = admit_successor(req, &r.ctx_id, found.as_ref())?;
    let body_json: String = predecessor.try_get("body_json").map_err(db_err("decode"))?;
    let att = predecessor_attestation(&decode_body(&body_json)?)?;
    let row = match &att {
        Some(c) => successor(tx, c.as_str()).await?.map(|s| s.row),
        None => None,
    };
    let signer = predecessor_signer(att.as_ref(), row.as_ref())?;
    key_attestations::check_supersession(&signer, succ)
}

/// Steps 2 and 5 of the shared order, for a keyless successor. Call with
/// the assigned `created_at`, before the receipt is minted.
pub(crate) async fn window_gate(
    tx: &mut Transaction<'_, Sqlite>,
    req: &PublishRequest,
    created_at: DateTime<Utc>,
) -> Result<(), AcdpError> {
    let Some(r) = keyless_successor(req) else {
        return Ok(());
    };
    let found = successor(tx, r.ctx_id.as_str()).await?;
    key_attestations::check_window(admit_successor(req, &r.ctx_id, found.as_ref())?, created_at)
}

/// Step 6: index a just-inserted attestation body. A token that already
/// minted one aborts the publish with `token_replayed`; the caller's
/// transaction then rolls back the body, so neither it nor a log leaf
/// survives.
pub(crate) async fn index_attestation(
    tx: &mut Transaction<'_, Sqlite>,
    body: &Body,
    authority: &str,
) -> Result<(), AcdpError> {
    let Some(row) = index_row_for(body, authority)? else {
        return Ok(());
    };
    let inserted = sqlx::query(
        "INSERT INTO key_attestations (ctx_id, controller, identity_type, issuer, subject, \
         org_wide, key_id, algorithm, key_fingerprint, not_before_ms, not_after_ms, token_hash, \
         created_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?) \
         ON CONFLICT (token_hash) DO NOTHING",
    )
    .bind(&row.ctx_id)
    .bind(&row.controller)
    .bind(row.identity_type())
    .bind(row.issuer())
    .bind(row.subject())
    .bind(i64::from(row.org_wide))
    .bind(&row.key_id)
    .bind(&row.algorithm)
    .bind(&row.key_fingerprint)
    .bind(row.not_before_ms)
    .bind(row.not_after_ms)
    .bind(&row.token_hash)
    .bind(row.created_at_ms)
    .execute(&mut **tx)
    .await
    .map_err(db_err("insert"))?
    .rows_affected();
    if inserted == 0 {
        return Err(token_replayed(&row.token_hash));
    }
    Ok(())
}

pub(crate) async fn record(
    pool: &SqlitePool,
    ctx_id: &str,
) -> Result<Option<KeyAttestationRecord>, AcdpError> {
    let row = sqlx::query(
        "SELECT k.ctx_id, k.created_at_ms, c.content_hash, c.status, c.retracted, c.body_json \
         FROM key_attestations k JOIN contexts c ON c.ctx_id = k.ctx_id WHERE k.ctx_id = ?",
    )
    .bind(ctx_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err("read"))?;
    let Some(r) = row else {
        return Ok(None);
    };
    let get = db_err("decode");
    let status: String = r.try_get("status").map_err(&get)?;
    let retracted: i64 = r.try_get("retracted").map_err(&get)?;
    let body_json: String = r.try_get("body_json").map_err(&get)?;
    KeyAttestationRecord::from_parts(
        ctx_id,
        r.try_get("content_hash").map_err(&get)?,
        stored_status(&status, retracted != 0),
        r.try_get("created_at_ms").map_err(&get)?,
        &decode_body(&body_json)?,
    )
    .map(Some)
}

pub(crate) async fn token_seen(pool: &SqlitePool, token_hash: &str) -> Result<bool, AcdpError> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM key_attestations WHERE token_hash = ?")
        .bind(token_hash)
        .fetch_one(pool)
        .await
        .map_err(db_err("read"))?;
    Ok(n > 0)
}

pub(crate) async fn list(
    pool: &SqlitePool,
    controller: &str,
    filter: &KeyAttestationFilter,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Page<KeyAttestationListItem>, AcdpError> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT) as usize;
    let anchor = cursor.map(decode_cursor).transpose()?.flatten();
    let mut q: QueryBuilder<'_, Sqlite> = QueryBuilder::new(
        "SELECT k.ctx_id, k.created_at_ms, c.content_hash, c.body_json FROM key_attestations k \
         JOIN contexts c ON c.ctx_id = k.ctx_id WHERE k.controller = ",
    );
    q.push_bind(controller);
    if let Some(issuer) = &filter.issuer {
        q.push(" AND k.issuer = ").push_bind(issuer.clone());
    }
    if let Some(subject) = &filter.subject {
        q.push(" AND k.subject = ").push_bind(subject.clone());
    }
    if let Some((at, ctx)) = &anchor {
        let at = at.timestamp_millis();
        q.push(" AND (k.created_at_ms < ")
            .push_bind(at)
            .push(" OR (k.created_at_ms = ")
            .push_bind(at)
            .push(" AND k.ctx_id > ")
            .push_bind(ctx.clone())
            .push("))");
    }
    q.push(" ORDER BY k.created_at_ms DESC, k.ctx_id ASC LIMIT ")
        .push_bind(limit as i64 + 1);
    let rows = q.build().fetch_all(pool).await.map_err(db_err("list"))?;
    let more = rows.len() > limit;
    let get = db_err("decode");
    let mut items = Vec::with_capacity(limit);
    let mut last = None;
    for r in rows.into_iter().take(limit) {
        let ctx_id: String = r.try_get("ctx_id").map_err(&get)?;
        let created_at_ms: i64 = r.try_get("created_at_ms").map_err(&get)?;
        let body_json: String = r.try_get("body_json").map_err(&get)?;
        items.push(list_item(
            &ctx_id,
            r.try_get("content_hash").map_err(&get)?,
            &decode_body(&body_json)?,
        )?);
        last = Some((created_at_ms, ctx_id));
    }
    let next_cursor = more
        .then_some(last)
        .flatten()
        .map(|(at, ctx)| encode_cursor(at, &ctx));
    Ok(Page { items, next_cursor })
}
