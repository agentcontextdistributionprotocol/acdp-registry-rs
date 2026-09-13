//! SQLite implementation of `acdp::registry::RegistryStore` +
//! `acdp_registry_store::ExtendedRegistryStore`.

use std::path::Path;
use std::str::FromStr;

use acdp::error::AcdpError;
use acdp::pagination::try_paginate_rows;
use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
use acdp::registry::{IdempotencyRecord, LifecycleCommitOutcome, ValidatedPublish};
use acdp::types::body::{Body, FullContext, RegistryState};
use acdp::types::lifecycle::{retraction_state, LifecycleEvent, LifecycleEventType};
use acdp::types::primitives::{AgentDid, ContentHash, CtxId, LineageId, Status, Visibility};
use acdp::types::publish::PublishResponse;
use acdp::types::search::{SearchParams, SearchResponse, SearchResult};
use acdp_registry_store::lifecycle::reconcile_retraction;
use acdp_registry_store::{
    decode_cursor, encode_cursor, ExtendedRegistryStore, LogEntryRecord, Page,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

/// SQLite store. Held by `RegistryServer` and used by the HTTP layer.
#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
    /// RFC-ACDP-0012: when true, `commit_publish` appends a transparency-
    /// log leaf in the SAME transaction as the context row + receipt
    /// (§7.1 — no degraded mode). Enabled via
    /// [`Self::with_transparency_log`] when `[log]` is configured.
    log_enabled: bool,
}

/// How long a writer waits for SQLite's write lock before returning
/// `SQLITE_BUSY`.
///
/// Chosen to comfortably exceed a `BEGIN IMMEDIATE` held across the
/// receipt-minter callback plus an fsync on a slow disk — the window that
/// produced spurious 500s when the timeout was left implicit. Generous on
/// purpose: waiting is cheap and correct, failing the request is neither.
const SQLITE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl SqliteStore {
    /// Open or create a SQLite database at `path`.
    pub async fn connect(path: &Path, max_connections: u32) -> Result<Self, AcdpError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| AcdpError::RegistryInternal(format!("mkdir: {e}")))?;
            }
        }
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
            .map_err(|e| AcdpError::RegistryInternal(format!("sqlite uri: {e}")))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            // B5: set the busy timeout EXPLICITLY rather than inheriting
            // sqlx's implicit default.
            //
            // `commit_publish` holds `BEGIN IMMEDIATE` across the
            // receipt-minter callback, so a writer can legitimately hold the
            // write lock for as long as that callback plus an fsync takes. A
            // concurrent writer that waits less than that surfaces
            // `SQLITE_BUSY` as a 500 with no retry — a spurious failure under
            // ordinary contention rather than a real error.
            //
            // The value is a named constant here rather than a config field
            // because the storage config lives in `acdp-registry-types`, which
            // is outside this change's scope; making it tunable is tracked
            // separately. An explicit value that is written down beats an
            // implicit one that has to be looked up in a dependency.
            .busy_timeout(SQLITE_BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections.max(1))
            .connect_with(opts)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("sqlite connect: {e}")))?;
        Ok(Self {
            pool,
            log_enabled: false,
        })
    }

    /// In-memory store, primarily for tests.
    ///
    /// sqlx's pool defaults to `min_connections = 0` and a finite idle
    /// timeout — both of which let the single underlying `:memory:`
    /// connection get reaped between calls, after which the next query
    /// opens a brand-new empty database. Pin the pool to keep the same
    /// connection alive for the lifetime of the store.
    pub async fn connect_in_memory() -> Result<Self, AcdpError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect("sqlite::memory:")
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("sqlite mem: {e}")))?;
        Ok(Self {
            pool,
            log_enabled: false,
        })
    }

    /// Enable the RFC-ACDP-0012 transparency log: every subsequent
    /// `commit_publish` appends a §4 leaf atomically with the context
    /// row and its receipt (§7.1), and refuses to publish at all when no
    /// receipt is minted — the log profile's prerequisite is the
    /// receipts profile (§11), and there is no degraded mode.
    pub fn with_transparency_log(mut self) -> Self {
        self.log_enabled = true;
        self
    }

    fn block_on<F: std::future::Future<Output = T>, T>(&self, fut: F) -> T {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
    }

    /// Borrow the underlying connection pool. Used by the server binary to
    /// hand the same pool to `SqliteChallengeStore` / `SqliteRevocationStore`
    /// instead of standing up a parallel pool — and to drop the previous
    /// `InMemoryChallengeStore` wiring that left migration 003's table
    /// orphaned (BUG-06).
    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }
}

// ── ExtendedRegistryStore ────────────────────────────────────────────────────

#[async_trait]
impl ExtendedRegistryStore for SqliteStore {
    async fn migrate(&self) -> Result<(), AcdpError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("migrate: {e}")))?;
        Ok(())
    }

    async fn health(&self) -> Result<(), AcdpError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| AcdpError::RegistryInternal(format!("health: {e}")))
    }

    async fn count_idempotency_records(&self) -> Result<Option<u64>, AcdpError> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM idempotency_records")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("count_idempotency: {e}")))?;
        Ok(Some(n.max(0) as u64))
    }

    /// H-H: the tenant predicate rides the same query builder as
    /// `RegistryStore::search`, so the scan -- and therefore the keyset cursor
    /// anchored on its last raw row -- only ever sees `tenant`'s contexts.
    async fn search_in_tenant(
        &self,
        params: &SearchParams,
        requester: Option<&AgentDid>,
        public_arm_open: bool,
        tenant: Option<&str>,
    ) -> Result<SearchResponse, AcdpError> {
        self.search_inner(params, requester, public_arm_open, tenant)
            .await
    }

    async fn tenant_of_ctx(&self, ctx_id: &str) -> Result<Option<String>, AcdpError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT tenant_id FROM contexts WHERE ctx_id = ?1")
                .bind(ctx_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tenant_of_ctx: {e}")))?;
        Ok(row.map(|(t,)| t))
    }

    async fn set_tenant_of_ctx(&self, ctx_id: &str, tenant_id: &str) -> Result<(), AcdpError> {
        if tenant_id == "default" {
            return Ok(());
        }
        sqlx::query("UPDATE contexts SET tenant_id = ?1 WHERE ctx_id = ?2")
            .bind(tenant_id)
            .bind(ctx_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("set_tenant_of_ctx: {e}")))?;
        Ok(())
    }

    async fn tenants_of_ctxs(
        &self,
        ctx_ids: &[&str],
    ) -> Result<std::collections::HashMap<String, String>, AcdpError> {
        if ctx_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        // SQLite lacks array binding — build an `IN (?,?,?,…)` clause
        // with one placeholder per id, then bind each id. Placeholder
        // count is bounded by the caller's page size (default 200).
        let placeholders = std::iter::repeat_n("?", ctx_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("SELECT ctx_id, tenant_id FROM contexts WHERE ctx_id IN ({placeholders})");
        let mut q = sqlx::query_as::<_, (String, String)>(&sql);
        for id in ctx_ids {
            q = q.bind(*id);
        }
        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("tenants_of_ctxs: {e}")))?;
        Ok(rows.into_iter().collect())
    }

    /// One query for a whole page's worth of retrieval-visibility checks,
    /// replacing N blocking `retrieve` round-trips.
    ///
    /// Reuses `LIST_VISIBILITY_SQLITE` rather than restating it. That choice is
    /// the correctness of this method: the LIST predicate is the
    /// **retrieve**-shaped one, whose non-public arm covers `restricted` AND
    /// `private` together, so an audience member may retrieve a private context
    /// (RFC-ACDP-0008 §4.5, conformance `vis-004`). `SEARCH_VISIBILITY_SQLITE`
    /// is deliberately stricter — it requires ownership for `private` — and
    /// substituting it here would silently under-disclose. The two are not
    /// interchangeable and must not be harmonized.
    ///
    /// No status or `retracted` clause, deliberately: a retracted context is
    /// still retrievable, so filtering it here would hide log entries the
    /// caller is entitled to.
    async fn visible_ctx_ids(
        &self,
        ctx_ids: &[&str],
        requester: Option<&AgentDid>,
        tenant: Option<&str>,
        public_arm_open: bool,
    ) -> Result<std::collections::HashSet<String>, AcdpError> {
        let mut out = std::collections::HashSet::with_capacity(ctx_ids.len());
        if ctx_ids.is_empty() {
            return Ok(out);
        }
        let requester_s: Option<String> = requester.map(|r| r.as_str().to_string());
        let req = requester_s.as_deref();
        // SQLite lacks array binding, so ids become `IN (?,?,…)` — which makes
        // the host-parameter ceiling a real constraint. The caller's page cap
        // (256) fits in one chunk, but this method is public and takes an
        // arbitrary slice, so it chunks rather than failing on a large input.
        for chunk in ctx_ids.chunks(VISIBLE_CTX_IDS_CHUNK) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let mut sql = format!("SELECT ctx_id FROM contexts WHERE ctx_id IN ({placeholders})");
            sql.push_str(LIST_VISIBILITY_SQLITE);
            if tenant.is_some() {
                sql.push_str(" AND tenant_id = ?");
            }
            let mut q = sqlx::query_as::<_, (String,)>(&sql);
            // Bind order follows textual order: the id placeholders, then the
            // five disclosure binds, then the tenant.
            for id in chunk {
                q = q.bind(*id);
            }
            q = q
                .bind(req)
                .bind(public_arm_open)
                .bind(req)
                .bind(req)
                .bind(req);
            if let Some(t) = tenant {
                q = q.bind(t);
            }
            let rows = q
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("visible_ctx_ids: {e}")))?;
            out.extend(rows.into_iter().map(|(id,)| id));
        }
        Ok(out)
    }

    async fn list_contexts(
        &self,
        limit: u32,
        cursor: Option<&str>,
        requester: Option<&AgentDid>,
        tenant: Option<&str>,
        public_arm_open: bool,
    ) -> Result<Page<FullContext>, AcdpError> {
        let limit = limit.clamp(1, 200) as i64;
        let anchor = cursor.map(decode_cursor).transpose()?.flatten();
        let requester_s: Option<String> = requester.map(|r| r.as_str().to_string());
        let mut sql = String::from(
            "SELECT body_json, status, registry_receipt, retracted FROM contexts WHERE 1=1",
        );
        // DESIGN-01: push the RFC-ACDP-0008 §4.5 retrieval-style disclosure
        // predicate (`visible_to`) into SQL so restricted/private bodies the
        // requester may not see are never read or decoded, and the page fills
        // to `limit` instead of being trimmed by a post-query retain.
        // Placeholders (in order): `?req` (public), `?anon`, `?req` ×3
        // (restricted/private).
        sql.push_str(LIST_VISIBILITY_SQLITE);
        // Plan §7: push the tenant filter into SQL so a busy
        // mixed-tenant registry doesn't return short pages caused by a
        // post-query retain. The `idx_ctx_tenant_created` index lets
        // this stay selective.
        if tenant.is_some() {
            sql.push_str(" AND tenant_id = ?");
        }
        if anchor.is_some() {
            // RFC3339 strings compare lexicographically when the timezone is UTC,
            // so we can do the keyset compare directly on the stored TEXT column
            // without losing sub-second precision. The ctx_id tiebreaker keeps
            // pagination stable when two contexts share a created_at.
            sql.push_str(" AND (created_at < ? OR (created_at = ? AND ctx_id > ?))");
        }
        sql.push_str(" ORDER BY created_at DESC, ctx_id ASC LIMIT ?");

        let mut q = sqlx::query(&sql);
        // Disclosure binds first — same textual order as LIST_VISIBILITY_SQLITE.
        let req = requester_s.as_deref();
        q = q
            .bind(req)
            .bind(public_arm_open)
            .bind(req)
            .bind(req)
            .bind(req);
        if let Some(t) = tenant {
            q = q.bind(t);
        }
        if let Some((anchor_ts, anchor_ctx)) = anchor.as_ref() {
            let anchor_rfc = anchor_ts.to_rfc3339();
            q = q.bind(anchor_rfc.clone()).bind(anchor_rfc).bind(anchor_ctx);
        }
        q = q.bind(limit + 1);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("list: {e}")))?;

        // BUG-01 / BUG (#13): the "more rows?" signal comes from the raw
        // DB row count (the SQL `LIMIT limit+1` sentinel), and
        // `next_cursor` anchors on the last row *scanned*, not the last
        // item kept by the `visible_to` filter. Both invariants (and
        // their regression tests) are owned by
        // `acdp::pagination::try_paginate_rows`.
        let page = try_paginate_rows(
            rows,
            limit as usize,
            |r| row_to_context(&r),
            // DESIGN-01: §4.5 disclosure is enforced in SQL above, so the
            // raw scanned rows are already the exact visible set; no
            // post-query visibility retain is needed.
            |_ctx| true,
            |ctx| {
                encode_cursor(
                    ctx.body.created_at.timestamp_millis(),
                    ctx.body.ctx_id.as_str(),
                )
            },
        )?;
        Ok(Page {
            items: page.items,
            next_cursor: page.next_cursor,
        })
    }

    async fn lifecycle_events_of_ctx(
        &self,
        ctx_id: &str,
    ) -> Result<Vec<LifecycleEvent>, AcdpError> {
        events_for_ctx(&self.pool, ctx_id).await
    }

    // ── Transparency log reads (RFC-ACDP-0012) ─────────────────────────

    async fn log_tree_size(&self) -> Result<u64, AcdpError> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM log_leaves")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AcdpError::RegistryInternal(format!("log_tree_size: {e}")))?;
        Ok(n.max(0) as u64)
    }

    async fn log_leaf_hashes(&self, up_to: u64) -> Result<Vec<[u8; 32]>, AcdpError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT leaf_hash FROM log_leaves WHERE leaf_index < ? ORDER BY leaf_index ASC",
        )
        .bind(i64::try_from(up_to).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AcdpError::RegistryInternal(format!("log_leaf_hashes: {e}")))?;
        if rows.len() as u64 != up_to {
            return Err(AcdpError::RegistryInternal(format!(
                "transparency log is not dense: {} leaves stored below index {up_to} \
                 (RFC-ACDP-0012 §5.3)",
                rows.len()
            )));
        }
        rows.iter()
            .map(|(h,)| acdp::types::log::decode_sha256_hex(h))
            .collect()
    }

    async fn log_leaf_by_ctx(&self, ctx_id: &str) -> Result<Option<LogEntryRecord>, AcdpError> {
        let row = sqlx::query(
            "SELECT leaf_index, ctx_id, leaf_hash, leaf_json FROM log_leaves WHERE ctx_id = ?",
        )
        .bind(ctx_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AcdpError::RegistryInternal(format!("log_leaf_by_ctx: {e}")))?;
        row.map(|r| log_row_to_record(&r)).transpose()
    }

    async fn log_leaf_by_index(
        &self,
        leaf_index: u64,
    ) -> Result<Option<LogEntryRecord>, AcdpError> {
        let Ok(idx) = i64::try_from(leaf_index) else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT leaf_index, ctx_id, leaf_hash, leaf_json FROM log_leaves \
             WHERE leaf_index = ?",
        )
        .bind(idx)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AcdpError::RegistryInternal(format!("log_leaf_by_index: {e}")))?;
        row.map(|r| log_row_to_record(&r)).transpose()
    }

    async fn log_entries(&self, start: u64, end: u64) -> Result<Vec<LogEntryRecord>, AcdpError> {
        let rows = sqlx::query(
            "SELECT leaf_index, ctx_id, leaf_hash, leaf_json FROM log_leaves \
             WHERE leaf_index >= ? AND leaf_index < ? ORDER BY leaf_index ASC",
        )
        .bind(i64::try_from(start).unwrap_or(i64::MAX))
        .bind(i64::try_from(end).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AcdpError::RegistryInternal(format!("log_entries: {e}")))?;
        rows.iter().map(log_row_to_record).collect()
    }

    // ── Witness cosignature aggregation (RFC-ACDP-0015 §6.1) ───────────

    async fn upsert_witness_cosignature(
        &self,
        log_id: &str,
        tree_size: u64,
        root_hash: &str,
        witness_did: &str,
        witnessed_at: &str,
        cosignature_json: &str,
    ) -> Result<(), AcdpError> {
        sqlx::query(
            "INSERT INTO log_witness_cosignatures \
             (log_id, tree_size, root_hash, witness_did, witnessed_at, cosignature_json, stored_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(log_id, tree_size, root_hash, witness_did) DO UPDATE SET \
             witnessed_at = excluded.witnessed_at, \
             cosignature_json = excluded.cosignature_json, \
             stored_at = excluded.stored_at",
        )
        .bind(log_id)
        .bind(i64::try_from(tree_size).unwrap_or(i64::MAX))
        .bind(root_hash)
        .bind(witness_did)
        .bind(witnessed_at)
        .bind(cosignature_json)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| AcdpError::RegistryInternal(format!("upsert_witness_cosignature: {e}")))?;
        Ok(())
    }

    async fn witness_cosignatures_for(
        &self,
        log_id: &str,
        tree_size: u64,
        root_hash: &str,
    ) -> Result<Vec<serde_json::Value>, AcdpError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT cosignature_json FROM log_witness_cosignatures \
             WHERE log_id = ?1 AND tree_size = ?2 AND root_hash = ?3 \
             ORDER BY witness_did ASC",
        )
        .bind(log_id)
        .bind(i64::try_from(tree_size).unwrap_or(i64::MAX))
        .bind(root_hash)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AcdpError::RegistryInternal(format!("witness_cosignatures_for: {e}")))?;
        rows.iter()
            .map(|(j,)| {
                serde_json::from_str(j).map_err(|e| {
                    AcdpError::RegistryInternal(format!("stored cosignature is not JSON: {e}"))
                })
            })
            .collect()
    }
}

/// Decode one `log_leaves` row.
fn log_row_to_record(r: &sqlx::sqlite::SqliteRow) -> Result<LogEntryRecord, AcdpError> {
    let leaf_index: i64 = r.try_get("leaf_index").map_err(map_sqlx_err)?;
    Ok(LogEntryRecord {
        leaf_index: u64::try_from(leaf_index).map_err(|_| {
            AcdpError::RegistryInternal(format!("negative leaf_index {leaf_index}"))
        })?,
        ctx_id: r.try_get("ctx_id").map_err(map_sqlx_err)?,
        leaf_hash: r.try_get("leaf_hash").map_err(map_sqlx_err)?,
        leaf_json: r.try_get("leaf_json").map_err(map_sqlx_err)?,
    })
}

/// RFC-ACDP-0008 §4.5 retrieval-style disclosure used by the admin/debug
/// listing (the former in-Rust `visible_to`): `public` surfaces for any
/// authenticated caller or — when `public_arm_open` — anonymously,
/// mirroring `search`'s public arm; `restricted`/`private` require the
/// requester to be the producer (`agent_id`) or a named `audience` member.
/// Placeholders (in textual order): `?req` (public), `?anon`, then
/// `?req`,`?req`,`?req` (restricted/private), where `?req` is the requester
/// DID or SQL NULL for an anonymous caller and `?anon` is
/// `public_arm_open`. `json_each(body_json,'$.audience')` yields zero
/// rows when `audience` is absent, so the audience arm short-circuits.
/// Ids per `visible_ctx_ids` query. SQLite's default host-parameter ceiling is
/// 999; this leaves generous room for the five disclosure binds and the tenant
/// bind alongside the ids, and still answers the caller's 256-entry page cap in
/// a single round-trip.
const VISIBLE_CTX_IDS_CHUNK: usize = 900;

const LIST_VISIBILITY_SQLITE: &str = " AND (\
    (visibility = 'public' AND (? IS NOT NULL OR ?)) \
    OR (? IS NOT NULL AND (agent_id = ? \
        OR EXISTS (SELECT 1 FROM json_each(body_json, '$.audience') WHERE value = ?))))";

/// RFC-ACDP-0008 §4.5 search/discovery disclosure (the former in-Rust
/// `can_surface_in_search`), arm-for-arm with the visibility matrix:
/// `public` surfaces for any authenticated caller or — when
/// `public_arm_open` — anonymously; `restricted` surfaces to the
/// producer or a named `audience` member; `private` surfaces to the
/// producer ONLY (audience members can retrieve a `ctx_id` they know but
/// MUST NOT discover it via search). Placeholders (in textual order):
/// `?req` (public), `?anon`, `?req`,`?req`,`?req` (restricted),
/// `?req`,`?req` (private).
const SEARCH_VISIBILITY_SQLITE: &str = " AND (\
    (visibility = 'public' AND (? IS NOT NULL OR ?)) \
    OR (visibility = 'restricted' AND ? IS NOT NULL AND (agent_id = ? \
        OR EXISTS (SELECT 1 FROM json_each(body_json, '$.audience') WHERE value = ?))) \
    OR (visibility = 'private' AND ? IS NOT NULL AND agent_id = ?))";

// ── RegistryStore (sync, drives async sqlx via block_on) ─────────────────────

impl RegistryStore for SqliteStore {
    fn put(&self, body: Body) -> Result<(), AcdpError> {
        self.block_on(async {
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tx begin: {e}")))?;
            insert_body(&mut tx, &body, Status::Active, None, None).await?;
            tx.commit()
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tx commit: {e}")))?;
            Ok(())
        })
    }

    fn get(&self, ctx_id: &CtxId) -> Result<Option<FullContext>, AcdpError> {
        self.block_on(async {
            let row = sqlx::query(
                "SELECT body_json, status, registry_receipt, retracted FROM contexts \
                 WHERE ctx_id = ?",
            )
            .bind(ctx_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
            let Some(row) = row else {
                return Ok(None);
            };
            let mut ctx = row_to_context(&row)?;
            // RFC-ACDP-0013 §4.1: full retrieval serves the event array
            // inside registry_state (omitted, not [], when empty).
            let events = events_for_ctx(&self.pool, ctx_id.as_str()).await?;
            // B3: the row and the events are two separate reads with no
            // shared snapshot, so a retraction committing between them
            // would otherwise be served as `status: "active"` alongside a
            // `retracted` event — contradicting the §7.2 precedence this
            // projection is documented to guarantee. Reconciling here makes
            // the served pair self-consistent whichever read is fresher.
            ctx.registry_state.status =
                reconcile_retraction(ctx.registry_state.status.clone(), &events);
            if !events.is_empty() {
                ctx.registry_state.lifecycle_events = Some(events);
            }
            Ok(Some(project_context(ctx, Utc::now())))
        })
    }

    fn lineage(&self, lineage_id: &LineageId) -> Result<Vec<FullContext>, AcdpError> {
        self.block_on(async {
            let rows = sqlx::query(
                "SELECT body_json, status, registry_receipt, retracted FROM contexts \
                 WHERE lineage_id = ? \
                 ORDER BY version ASC, created_at ASC",
            )
            .bind(lineage_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
            // RFC-ACDP-0013 §4.1: the lineage array carries each version's
            // lifecycle_events. One batch query for the whole lineage.
            let mut events_by_ctx = events_for_lineage(&self.pool, lineage_id.as_str()).await?;
            let now = Utc::now();
            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                let mut ctx = row_to_context(&r)?;
                if let Some(events) = events_by_ctx.remove(ctx.body.ctx_id.as_str()) {
                    // B3: the row and the events are two separate reads with no
                    // shared snapshot, so a retraction committing between them
                    // would otherwise be served as `status: "active"` alongside a
                    // `retracted` event — contradicting the §7.2 precedence this
                    // projection is documented to guarantee. Reconciling here makes
                    // the served pair self-consistent whichever read is fresher.
                    ctx.registry_state.status =
                        reconcile_retraction(ctx.registry_state.status.clone(), &events);
                    if !events.is_empty() {
                        ctx.registry_state.lifecycle_events = Some(events);
                    }
                }
                out.push(project_context(ctx, now));
            }
            Ok(out)
        })
    }

    fn current(&self, lineage_id: &LineageId) -> Result<Option<FullContext>, AcdpError> {
        let all = self.lineage(lineage_id)?;
        for ctx in all.into_iter().rev() {
            // RFC-ACDP-0004 §5.2 as amended by RFC-ACDP-0013 §8.3: the head
            // is the newest version that is neither superseded nor retracted
            // (an expired head is still a valid head; a retracted one never
            // is — fixture lc-003). When every version is superseded or
            // retracted, there is no head.
            if !matches!(
                ctx.registry_state.status,
                Status::Superseded | Status::Retracted
            ) {
                return Ok(Some(ctx));
            }
        }
        Ok(None)
    }

    fn commit_lifecycle_event(
        &self,
        event: &LifecycleEvent,
    ) -> Result<LifecycleCommitOutcome, AcdpError> {
        let event = event.clone();
        self.block_on(async move {
            let now = Utc::now();
            // Same BEGIN IMMEDIATE rationale as commit_publish (REG-3.3):
            // take the write lock up front so two racing lifecycle writes
            // serialize and the loser observes the winner's committed
            // state, yielding the contract outcomes (idempotent replay /
            // invalid_lifecycle_transition) instead of SQLITE_BUSY 500s.
            let mut tx = self
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tx begin: {e}")))?;

            // 1. Resolve the context (visibility is the server's job,
            //    before this call — RFC-ACDP-0013 §6 step 1).
            let row = sqlx::query(
                "SELECT body_json, status, registry_receipt, retracted, tenant_id \
                 FROM contexts WHERE ctx_id = ?",
            )
            .bind(event.ctx_id.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            let Some(row) = row else {
                return Err(AcdpError::NotFound(format!(
                    "context '{}' not found in this registry",
                    event.ctx_id
                )));
            };
            let tenant_id: String = row.try_get("tenant_id").map_err(map_sqlx_err)?;
            let ctx = row_to_context(&row)?;

            // 2. Load the append-ordered event history under the same lock.
            let ev_rows = sqlx::query(
                "SELECT event_id, ctx_id, event_type, occurred_at, actor, reason, signature \
                 FROM lifecycle_events WHERE ctx_id = ? ORDER BY seq ASC",
            )
            .bind(event.ctx_id.as_str())
            .fetch_all(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            let mut events = Vec::with_capacity(ev_rows.len() + 1);
            for r in &ev_rows {
                events.push(event_from_row(r)?);
            }

            // 3. §6 retry idempotency / duplicate event_id (step 2): a
            //    byte-identical resubmission replays the current state; a
            //    divergent one is a schema_violation.
            if let Some(prior) = events.iter().find(|e| e.event_id == event.event_id) {
                if *prior == event {
                    tx.rollback().await.ok();
                    let ctx = attach_events(ctx, events);
                    return Ok(LifecycleCommitOutcome::IdempotentReplay(project_context(
                        ctx, now,
                    )));
                }
                return Err(AcdpError::SchemaViolation(format!(
                    "event_id '{}' was already appended with different content \
                     (RFC-ACDP-0013 §4: event_id MUST be unique within lifecycle_events)",
                    event.event_id
                )));
            }

            // 4. §6 step 4 — strict retracted/republished alternation
            //    against the §7.1 retraction state.
            let currently_retracted = retraction_state(&events);
            match &event.event_type {
                LifecycleEventType::Retracted if currently_retracted => {
                    return Err(AcdpError::InvalidLifecycleTransition(format!(
                        "context '{}' is already retracted — double retract violates the \
                         strict alternation rule (RFC-ACDP-0013 §6 step 4)",
                        event.ctx_id
                    )));
                }
                LifecycleEventType::Republished if !currently_retracted => {
                    return Err(AcdpError::InvalidLifecycleTransition(format!(
                        "context '{}' is not retracted — republish requires a prior \
                         retraction (RFC-ACDP-0013 §6 step 4)",
                        event.ctx_id
                    )));
                }
                LifecycleEventType::Other(other) => {
                    return Err(AcdpError::SchemaViolation(format!(
                        "event_type '{other}' is not registered for acceptance in 0.3.0 — \
                         only 'retracted' and 'republished' transition state \
                         (RFC-ACDP-0013 §7.3)"
                    )));
                }
                LifecycleEventType::Retracted | LifecycleEventType::Republished => {}
            }

            // 5. §6 step 5 — append the event AND apply its status effect
            //    (the denormalized `retracted` flag the read paths project
            //    from) in ONE transaction. Stored `status` keeps tracking
            //    supersession only.
            let signature_json = event
                .signature
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| AcdpError::RegistryInternal(format!("encode signature: {e}")))?;
            sqlx::query(
                "INSERT INTO lifecycle_events \
                 (ctx_id, event_id, event_type, occurred_at, actor, reason, signature, tenant_id) \
                 VALUES (?,?,?,?,?,?,?,?)",
            )
            .bind(event.ctx_id.as_str())
            .bind(event.event_id.as_str())
            .bind(event.event_type.as_str())
            .bind(canonical_ms(event.occurred_at))
            .bind(event.actor.as_str())
            .bind(event.reason.clone())
            .bind(signature_json)
            .bind(&tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            let retracted_now = matches!(event.event_type, LifecycleEventType::Retracted);
            sqlx::query("UPDATE contexts SET retracted = ? WHERE ctx_id = ?")
                .bind(retracted_now as i64)
                .bind(event.ctx_id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            tx.commit()
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tx commit: {e}")))?;

            // Post-transition projection: recompute the served status from
            // the NEW retraction state (row_to_context above applied the
            // pre-transition flag).
            events.push(event.clone());
            let mut ctx = attach_events(ctx, events);
            ctx.registry_state.status = if retracted_now {
                Status::Retracted
            } else {
                // Republished: re-derive from the stored (supersession-only)
                // status + expiry, as though never retracted (§7.2).
                let stored: String = row.try_get("status").map_err(map_sqlx_err)?;
                project_status_inline(&parse_status(&stored), ctx.body.expires_at, now)
            };
            Ok(LifecycleCommitOutcome::Applied(ctx))
        })
    }

    fn mark_superseded(&self, ctx_id: &CtxId) -> Result<(), AcdpError> {
        self.block_on(async {
            sqlx::query("UPDATE contexts SET status = 'superseded' WHERE ctx_id = ?")
                .bind(ctx_id.as_str())
                .execute(&self.pool)
                .await
                .map(|_| ())
                .map_err(map_sqlx_err)
        })
    }

    fn first_version_ctx_id(&self, lineage_id: &LineageId) -> Result<Option<CtxId>, AcdpError> {
        self.block_on(async {
            let row = sqlx::query(
                "SELECT ctx_id FROM contexts WHERE lineage_id = ? \
                 ORDER BY version ASC, created_at ASC LIMIT 1",
            )
            .bind(lineage_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
            Ok(row
                .and_then(|r| r.try_get::<String, _>("ctx_id").ok())
                .map(CtxId))
        })
    }

    fn idempotency_lookup(
        &self,
        agent_id: &AgentDid,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, AcdpError> {
        // Background task `evict_idempotency` keeps the table bounded so we
        // don't burn a DELETE on every read. Reads epoch-ms directly to
        // skip the per-call RFC 3339 parse.
        self.block_on(async {
            let row = sqlx::query(
                "SELECT content_hash, response_json, expires_at_ms \
                 FROM idempotency_records WHERE agent_id = ? AND key = ?",
            )
            .bind(agent_id.as_str())
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_err)?;
            let Some(row) = row else {
                return Ok(None);
            };
            let content_hash: String = row.try_get("content_hash").map_err(map_sqlx_err)?;
            let response_json: String = row.try_get("response_json").map_err(map_sqlx_err)?;
            let expires_ms: i64 = row.try_get("expires_at_ms").map_err(map_sqlx_err)?;
            let response: PublishResponse = serde_json::from_str(&response_json)
                .map_err(|e| AcdpError::RegistryInternal(format!("decode response: {e}")))?;
            let expires_at =
                DateTime::<Utc>::from_timestamp_millis(expires_ms).ok_or_else(|| {
                    AcdpError::RegistryInternal(format!("expires_at_ms out of range: {expires_ms}"))
                })?;
            Ok(Some(IdempotencyRecord {
                content_hash: ContentHash(content_hash),
                response,
                expires_at,
            }))
        })
    }

    fn idempotency_record(
        &self,
        agent_id: &AgentDid,
        key: &str,
        hash: &ContentHash,
        response: &PublishResponse,
        expires_at: DateTime<Utc>,
    ) -> Result<(), AcdpError> {
        let response_json = serde_json::to_string(response)
            .map_err(|e| AcdpError::RegistryInternal(format!("encode response: {e}")))?;
        let expires_ms = expires_at.timestamp_millis();
        // The TEXT `expires_at` column is kept populated for one release
        // (rollback safety); both columns carry the same moment.
        let expires_rfc = expires_at.to_rfc3339();
        self.block_on(async {
            sqlx::query(
                "INSERT INTO idempotency_records (agent_id, key, content_hash, response_json, expires_at, expires_at_ms) \
                 VALUES (?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(agent_id, key) DO UPDATE SET \
                   content_hash = excluded.content_hash, \
                   response_json = excluded.response_json, \
                   expires_at = excluded.expires_at, \
                   expires_at_ms = excluded.expires_at_ms",
            )
            .bind(agent_id.as_str())
            .bind(key)
            .bind(hash.0.as_str())
            .bind(response_json)
            .bind(expires_rfc)
            .bind(expires_ms)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(map_sqlx_err)
        })
    }

    fn idempotency_evict_expired(&self, now: DateTime<Utc>) -> Result<(), AcdpError> {
        self.block_on(self.idempotency_evict_inner(now))
    }

    fn commit_publish(&self, commit: PublishCommit<'_>) -> Result<PublishCommitOutcome, AcdpError> {
        let PublishCommit {
            req,
            authority,
            idempotency,
            tenant,
            receipt_minter,
            predecessor_admission,
        } = commit;
        let now = Utc::now();
        let req = req.clone();
        let authority = authority.to_string();
        let tenant = tenant.map(|t| t.to_string());
        let idem = idempotency.map(|i| (i.key.to_string(), i.ttl));

        self.block_on(async move {
            // REG-3.3: `BEGIN IMMEDIATE` takes the write lock up front, so
            // concurrent commits queue on SQLite's busy handler (5s default)
            // and each observes the previous winner's committed state. A
            // plain deferred BEGIN takes a read snapshot at the first SELECT
            // and then fails the mid-transaction write upgrade with
            // SQLITE_BUSY / SQLITE_BUSY_SNAPSHOT ("database is locked") —
            // which the busy handler does NOT retry — surfacing transient
            // 500s to racing publishers instead of the contract outcomes
            // (idempotent replay / `superseded_target`). This is the SQLite
            // analog of the Postgres backend's `SELECT ... FOR UPDATE`.
            let mut tx = self
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tx begin: {e}")))?;

            // 1. Idempotency replay / collision.
            if let Some((key, _ttl)) = &idem {
                // Evict an expired record for this key first so the claim in
                // step 7 (ON CONFLICT DO NOTHING) does not collide with a stale
                // row after its TTL has lapsed.
                sqlx::query(
                    "DELETE FROM idempotency_records \
                     WHERE agent_id = ? AND key = ? AND expires_at_ms <= ?",
                )
                .bind(req.agent_id.as_str())
                .bind(key.as_str())
                .bind(now.timestamp_millis())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                let row = sqlx::query(
                    "SELECT content_hash, response_json, expires_at_ms \
                     FROM idempotency_records WHERE agent_id = ? AND key = ?",
                )
                .bind(req.agent_id.as_str())
                .bind(key.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if let Some(row) = row {
                    let prior_hash: String = row.try_get("content_hash").map_err(map_sqlx_err)?;
                    let response_json: String =
                        row.try_get("response_json").map_err(map_sqlx_err)?;
                    let exp_ms: i64 = row.try_get("expires_at_ms").map_err(map_sqlx_err)?;
                    let expires_at = DateTime::<Utc>::from_timestamp_millis(exp_ms).ok_or_else(
                        || {
                            AcdpError::RegistryInternal(format!(
                                "expires_at_ms out of range: {exp_ms}"
                            ))
                        },
                    )?;
                    if expires_at > now {
                        if prior_hash == req.content_hash.0 {
                            let response: PublishResponse = serde_json::from_str(&response_json)
                                .map_err(|e| {
                                    AcdpError::RegistryInternal(format!("decode response: {e}"))
                                })?;
                            tx.rollback().await.ok();
                            return Ok(PublishCommitOutcome::IdempotentReplay(response));
                        } else {
                            return Err(AcdpError::DuplicatePublish(format!(
                                "Idempotency-Key '{}' was previously used by '{}' \
                                 with a different content_hash",
                                key, req.agent_id
                            )));
                        }
                    }
                }
            }

            // 2. Supersession coherence checks (mirrors InMemoryStore).
            let first_v1 = if let Some(prev) = &req.supersedes {
                let row = sqlx::query(
                    "SELECT lineage_id, version, status, agent_id, contributors, tenant_id, \
                     body_json \
                     FROM contexts WHERE ctx_id = ?",
                )
                .bind(prev.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                let Some(row) = row else {
                    // Identical message/shape to the not-owner and wrong-tenant
                    // rejections below, so a caller cannot distinguish "absent"
                    // from "exists but not yours / another tenant's" (no
                    // existence oracle). Matches the reference InMemoryStore.
                    return Err(AcdpError::SupersededTarget {
                        reason: acdp::error::SupersessionReason::NotFound,
                        message: format!("supersedes target '{prev}' not found in this registry"),
                    });
                };
                let prev_lineage: String = row.try_get("lineage_id").map_err(map_sqlx_err)?;
                let prev_version: i64 = row.try_get("version").map_err(map_sqlx_err)?;
                let prev_status: String = row.try_get("status").map_err(map_sqlx_err)?;
                let prev_agent: String = row.try_get("agent_id").map_err(map_sqlx_err)?;
                // contributors is stored as a JSON-encoded array of DIDs.
                // B6: a corrupt `contributors` column must FAIL the publish, not
                // silently become an empty list.
                //
                // This previously swallowed both errors with
                // `.ok().and_then(...).unwrap_or_default()`. The consequence was
                // not a cosmetic one: `contributors` feeds the RFC-ACDP-0014 §4
                // predecessor-admission check below, so an unreadable column
                // meant a legitimate contributor was told
                // `SupersededTarget::NotFound` while the check itself reported
                // success. Postgres already errored here (`TEXT[]`, decoded with
                // `?`), so this was also a backend divergence.
                let prev_contributors_raw: String =
                    row.try_get("contributors").map_err(map_sqlx_err)?;
                let prev_contributors: Vec<String> = serde_json::from_str(&prev_contributors_raw)
                    .map_err(|e| {
                        AcdpError::RegistryInternal(format!(
                            "decode contributors for predecessor '{prev}': {e}"
                        ))
                    })?;
                let prev_tenant: String = row.try_get("tenant_id").map_err(map_sqlx_err)?;
                // P0 (tenant continuity): a successor must live in the same
                // tenant as its predecessor. Even with the owner check below
                // (agent→tenant is normally 1:1), an agent whose binding changed
                // could otherwise stitch a v2 into a lineage owned by a different
                // tenant. Same NotFound shape — no cross-tenant existence oracle.
                // Only enforced when the publish carries an authoritative tenant
                // (production tenant-scoped path); when `None` the tenant is not
                // threaded here (untenanted, or the playground post-hoc stamp),
                // so there is nothing to compare against.
                if let Some(req_tenant) = tenant.as_deref() {
                    if prev_tenant != req_tenant {
                        return Err(AcdpError::SupersededTarget {
                            reason: acdp::error::SupersessionReason::NotFound,
                            message: format!(
                                "supersedes target '{prev}' not found in this registry"
                            ),
                        });
                    }
                }
                // P0 (producer-continuity): only the predecessor's producer or a
                // declared contributor may publish a successor in its lineage.
                // Signature verification only proves the *requester* signed their
                // own request — it does not bind `supersedes` to the predecessor's
                // owner. Without this, any signer could flip another producer's
                // context to `superseded` and re-point `current(lineage)` — a
                // lineage takeover (RFC-ACDP-0001 §5.9). This mirrors
                // `InMemoryStore::commit_publish`; the registry backends had
                // dropped the check. A non-owner gets the same NotFound shape as
                // a genuinely-absent target so it learns neither that the
                // predecessor exists nor its version/superseded status.
                let is_owner = prev_agent == req.agent_id.as_str()
                    || prev_contributors.iter().any(|c| c == req.agent_id.as_str());
                if !is_owner {
                    return Err(AcdpError::SupersededTarget {
                        reason: acdp::error::SupersessionReason::NotFound,
                        message: format!("supersedes target '{prev}' not found in this registry"),
                    });
                }

                if let Some(declared) = &req.lineage_id {
                    if declared.as_str() != prev_lineage.as_str() {
                        return Err(AcdpError::SupersededTarget {
                            reason: acdp::error::SupersessionReason::LineageMismatch,
                            message: format!(
                                "declared lineage_id '{declared}' ≠ predecessor's '{prev_lineage}'"
                            ),
                        });
                    }
                }
                if req.version as i64 != prev_version + 1 {
                    return Err(AcdpError::SupersededTarget {
                        reason: acdp::error::SupersessionReason::VersionMismatch,
                        message: format!(
                            "version {} ≠ predecessor.version + 1 ({})",
                            req.version,
                            prev_version + 1
                        ),
                    });
                }
                if prev_status == "superseded" {
                    return Err(AcdpError::SupersededTarget {
                        reason: acdp::error::SupersessionReason::AlreadySuperseded,
                        message: format!(
                            "supersedes target '{prev}' has already been superseded"
                        ),
                    });
                }

                // RFC-ACDP-0014 §4 `supersedes`-row admission. Runs AFTER
                // tenant scoping, producer-continuity, lineage/version
                // coherence and AlreadySuperseded have all passed — never
                // earlier — so a non-owner or cross-tenant probe has already
                // been turned away as `SupersededTarget::NotFound` above and
                // never reaches this line. Checking first would make publish a
                // cross-tenant, non-owner existence-and-`context_type` oracle
                // on the predecessor. Mirrors the reference
                // `InMemoryStore::commit_publish` ordering.
                //
                // Refusal aborts the whole publish: this sits before every
                // write (insert, log leaf, supersession UPDATE, idempotency
                // claim), so `?` here leaves no side effect (tx drop =
                // rollback).
                if let Some(admit) = predecessor_admission {
                    // Deserialized lazily: only a gated supersession pays for
                    // it. The `?` is load-bearing — swallowing a decode failure
                    // here would silently skip an RFC-ACDP-0014 §4 MUST.
                    let prev_body_json: String =
                        row.try_get("body_json").map_err(map_sqlx_err)?;
                    let prev_body: Body = serde_json::from_str(&prev_body_json)
                        .map_err(|e| AcdpError::RegistryInternal(format!("decode body: {e}")))?;
                    admit(&prev_body)?;
                }

                // First-version ctx_id derivation.
                let first_row = sqlx::query(
                    "SELECT ctx_id FROM contexts WHERE lineage_id = ? \
                     ORDER BY version ASC, created_at ASC LIMIT 1",
                )
                .bind(&prev_lineage)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                first_row.and_then(|r| r.try_get::<String, _>("ctx_id").ok()).map(CtxId)
            } else {
                None
            };

            // 3. Identifier assignment via the protocol library.
            let validated = ValidatedPublish {
                recomputed_hash: req.content_hash.clone(),
            };
            let (ctx_id, lineage_id) = acdp::registry::assign_identifiers(
                &authority,
                &req.supersedes,
                first_v1.as_ref(),
                &validated,
            )?;

            // 4. Build the stored Body via the SDK's single
            // materialization point (`from_publish_request` ms-truncates
            // `created_at` and starts with empty extensions, exactly as
            // the hand-rolled copy did).
            let created_at = acdp::time::trunc_ms(now);
            let body = Body::from_publish_request(
                &req,
                ctx_id.clone(),
                lineage_id.clone(),
                authority.clone(),
                created_at,
            );

            // 4.5. Mint the registry receipt (RFC-ACDP-0010 §7) inside the
            // transaction, against the fully-assigned body. The receipt is
            // written by the SAME INSERT as the context row below, so a
            // context can never be observed without its receipt — minting
            // failure aborts the whole publish (tx drop = rollback).
            let receipt: Option<serde_json::Value> = match receipt_minter {
                Some(mint) => Some(mint(&body)?),
                None => None,
            };

            // 5. Insert the new body.
            insert_body(
                &mut tx,
                &body,
                Status::Active,
                tenant.as_deref(),
                receipt.as_ref(),
            )
            .await?;

            // 5.5. Append the transparency-log leaf (RFC-ACDP-0012 §7.1)
            // in the SAME transaction as the context row and its receipt:
            // the three commit together, or none does. leaf_index is the
            // dense acceptance-order position (§5.3) — COUNT(*) is safe
            // here because BEGIN IMMEDIATE serializes writers.
            if self.log_enabled {
                let Some(receipt) = receipt.as_ref() else {
                    return Err(AcdpError::RegistryInternal(
                        "transparency log is enabled but no receipt was minted for this \
                         publish — the log profile's prerequisite is the receipts profile \
                         (RFC-ACDP-0012 §11) and there is no degraded mode (§7.1); \
                         aborting the publish"
                            .into(),
                    ));
                };
                let (leaf_json, leaf_hash) =
                    acdp_registry_store::build_leaf_record(&body, receipt)?;
                sqlx::query(
                    "INSERT INTO log_leaves (leaf_index, ctx_id, leaf_json, leaf_hash) \
                     VALUES ((SELECT COUNT(*) FROM log_leaves), ?, ?, ?)",
                )
                .bind(body.ctx_id.as_str())
                .bind(&leaf_json)
                .bind(&leaf_hash)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }

            // 6. Mark predecessor superseded.
            if let Some(prev) = &req.supersedes {
                sqlx::query("UPDATE contexts SET status = 'superseded' WHERE ctx_id = ?")
                    .bind(prev.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
            }

            let response = PublishResponse {
                ctx_id,
                lineage_id,
                version: req.version,
                created_at,
                status: Status::Active,
                registry_receipt: receipt,
            };

            // 7. Record idempotency — this INSERT is the concurrency gate
            // (P0 #5). `ctx_id` is random per request, so two concurrent
            // first-publishes of the same (agent_id, key) would otherwise each
            // INSERT a distinct context. The `ON CONFLICT DO NOTHING` makes the
            // racers serialize on the PK: the loser inserts 0 rows, so we roll
            // back its context insert and replay the winner instead of
            // persisting a second context.
            if let Some((key, ttl)) = &idem {
                let expires_at = now + *ttl;
                let response_json = serde_json::to_string(&response)
                    .map_err(|e| AcdpError::RegistryInternal(format!("encode response: {e}")))?;
                let inserted = sqlx::query(
                    "INSERT INTO idempotency_records (agent_id, key, content_hash, response_json, expires_at, expires_at_ms) \
                     VALUES (?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(agent_id, key) DO NOTHING",
                )
                .bind(req.agent_id.as_str())
                .bind(key.as_str())
                .bind(req.content_hash.0.as_str())
                .bind(response_json)
                .bind(expires_at.to_rfc3339())
                .bind(expires_at.timestamp_millis())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected();

                if inserted == 0 {
                    // A concurrent publish won the key. Discard our context
                    // insert and replay the winner's record (or reject as a
                    // duplicate when the content_hash differs).
                    tx.rollback().await.ok();
                    let row = sqlx::query(
                        "SELECT content_hash, response_json \
                         FROM idempotency_records WHERE agent_id = ? AND key = ?",
                    )
                    .bind(req.agent_id.as_str())
                    .bind(key.as_str())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(map_sqlx_err)?;
                    let Some(row) = row else {
                        return Err(AcdpError::RegistryInternal(
                            "idempotency key claimed by a concurrent publish but its record \
                             could not be read back"
                                .into(),
                        ));
                    };
                    let prior_hash: String = row.try_get("content_hash").map_err(map_sqlx_err)?;
                    if prior_hash != req.content_hash.0 {
                        return Err(AcdpError::DuplicatePublish(format!(
                            "Idempotency-Key '{}' was previously used by '{}' \
                             with a different content_hash",
                            key, req.agent_id
                        )));
                    }
                    let response_json: String =
                        row.try_get("response_json").map_err(map_sqlx_err)?;
                    let winner: PublishResponse = serde_json::from_str(&response_json)
                        .map_err(|e| AcdpError::RegistryInternal(format!("decode response: {e}")))?;
                    return Ok(PublishCommitOutcome::IdempotentReplay(winner));
                }
            }

            tx.commit()
                .await
                .map_err(|e| AcdpError::RegistryInternal(format!("tx commit: {e}")))?;
            Ok(PublishCommitOutcome::Inserted(response))
        })
    }

    fn search(
        &self,
        params: &SearchParams,
        requester: Option<&AgentDid>,
        public_arm_open: bool,
    ) -> Result<SearchResponse, AcdpError> {
        // Tenant-spanning: the protocol-level contract carries no tenancy.
        // `ExtendedRegistryStore::search_in_tenant` is the narrowed entry
        // point, and both run THIS body -- one query builder, so the
        // tenant and non-tenant paths cannot drift apart.
        self.block_on(self.search_inner(params, requester, public_arm_open, None))
    }
}

impl SqliteStore {
    /// The one search implementation. `tenant == None` spans tenants
    /// (`RegistryStore::search`); `Some(t)` restricts the scan to `t`
    /// (`ExtendedRegistryStore::search_in_tenant`).
    ///
    /// Duplicating this builder for the tenant case was rejected outright:
    /// two copies of a ~260-line filter chain drift, and the drift is
    /// invisible precisely when it matters -- which is the lesson
    /// `acdp_registry_store::parity`'s module docs were written to record.
    async fn search_inner(
        &self,
        params: &SearchParams,
        requester: Option<&AgentDid>,
        public_arm_open: bool,
        tenant: Option<&str>,
    ) -> Result<SearchResponse, AcdpError> {
        // Boundary parse of all RFC 3339 filters.
        let created_after = parse_opt_rfc3339(&params.created_after)?;
        let created_before = parse_opt_rfc3339(&params.created_before)?;
        let expires_after = parse_opt_rfc3339(&params.expires_after)?;
        let expires_before = parse_opt_rfc3339(&params.expires_before)?;
        let dp_start_after = parse_opt_rfc3339(&params.data_period_start_after)?;
        let dp_end_before = parse_opt_rfc3339(&params.data_period_end_before)?;

        // DESIGN-01: the §4.5 search disclosure predicate is pushed into
        // SQL (below) so restricted/private bodies the requester may not
        // see are never read or decoded, pages fill to `limit`, and
        // `COUNT(*) OVER ()` yields an honest, §4.5-correct pre-page total
        // that excludes contexts the requester is not entitled to count.
        let requester_s: Option<String> = requester.map(|r| r.as_str().to_string());
        let mut sql = String::from(
            "SELECT body_json, status, retracted, COUNT(*) OVER () AS total_rows \
             FROM contexts WHERE 1=1",
        );
        sql.push_str(SEARCH_VISIBILITY_SQLITE);
        let mut binds: Vec<String> = Vec::new();

        // H-H: push the tenant predicate into SQL, exactly as
        // `list_contexts` does (see its comment for the index rationale).
        // This is the whole point of the unit, so it is worth saying why it
        // has to be HERE and not after the query: `acdp::pagination` anchors
        // `next_cursor` on the last RAW scanned row (see the REG-P2-8 note
        // further down), deliberately, so that a page whose rows are all
        // dropped by post-SQL filters cannot halt pagination early. Filter
        // by tenant after the scan and that anchor can name another tenant's
        // row, which leaks its `(created_at, ctx_id)` to the caller. With the
        // predicate in the WHERE clause every raw row is already the
        // caller's, so the anchor is tenant-safe BY CONSTRUCTION rather than
        // by a second check -- and `COUNT(*) OVER ()` below becomes
        // tenant-correct on the same scan, with no extra query.
        //
        // Bound first in `binds`, matching this textual position. `None`
        // spans tenants, preserving `RegistryStore::search`'s behaviour
        // exactly.
        if let Some(t) = tenant {
            sql.push_str(" AND tenant_id = ?");
            binds.push(t.to_string());
        }

        if let Some(q) = &params.q {
            sql.push_str(
                " AND ctx_id IN (SELECT ctx_id FROM contexts_fts WHERE contexts_fts MATCH ?)",
            );
            binds.push(fts5_escape(q));
        }
        if let Some(d) = &params.domain {
            sql.push_str(" AND domain = ?");
            binds.push(d.clone());
        }
        if let Some(a) = &params.agent_id {
            sql.push_str(" AND agent_id = ?");
            binds.push(a.clone());
        }
        if let Some(t) = &params.context_type {
            sql.push_str(" AND context_type = ?");
            binds.push(t.clone());
        }
        if let Some(s) = &params.schema_uri {
            sql.push_str(" AND json_extract(body_json, '$.schema_uri') = ?");
            binds.push(s.clone());
        }
        if let Some(after) = created_after {
            sql.push_str(" AND created_at >= ?");
            binds.push(after.to_rfc3339());
        }
        if let Some(before) = created_before {
            sql.push_str(" AND created_at <= ?");
            binds.push(before.to_rfc3339());
        }
        if let Some(after) = expires_after {
            sql.push_str(" AND expires_at IS NOT NULL AND expires_at >= ?");
            binds.push(after.to_rfc3339());
        }
        if let Some(before) = expires_before {
            sql.push_str(" AND expires_at IS NOT NULL AND expires_at <= ?");
            binds.push(before.to_rfc3339());
        }
        // BUG-B1: compare data_period bounds NUMERICALLY, never as TEXT.
        //
        // These two predicates read out of `body_json`, whose timestamps
        // are chrono's serde output (`Z` suffix, 0/3/6/9 fractional
        // digits), while the bound is bound as `to_rfc3339()` (`+00:00`
        // suffix). Two different serializers for the same instant, so a
        // lexicographic compare is simply wrong: `'+'`(0x2B) `<
        // '.'`(0x2E) `<` digits `< 'Z'`(0x5A), which makes a stored
        // whole-second value sort AFTER a bound naming that very same
        // instant. Measured in sqlite3:
        //
        //     '2026-01-01T00:00:00Z' <= '2026-01-01T00:00:00+00:00'  -> 0
        //
        // so an inclusive bound excluded an equal instant, and the mirror
        // case wrongly included. The `created_at`/`expires_at` filters
        // above are NOT affected: both their stored and bound sides go
        // through `to_rfc3339()`, so their lexical order does hold — that
        // was measured too, rather than assumed by analogy.
        //
        // `unixepoch(..., 'subsec')` puts both sides through one parser
        // and yields a real number, which fixes every combination of
        // suffix and fractional width at once. Binding a `Z`-normalized
        // string instead would NOT be enough: '…00Z' still sorts after
        // '…00.500Z'. Requires SQLite >= 3.42; the bundled library is
        // 3.46.0.
        //
        // Deliberately query-side only. The alternative — normalizing the
        // stored form with a migration — would rewrite `body_json`, whose
        // exact bytes are the `content_hash` preimage.
        if let Some(after) = dp_start_after {
            sql.push_str(
                " AND unixepoch(json_extract(body_json, '$.data_period.start'), 'subsec') \
                 >= unixepoch(?, 'subsec')",
            );
            binds.push(after.to_rfc3339());
        }
        if let Some(before) = dp_end_before {
            sql.push_str(
                " AND unixepoch(json_extract(body_json, '$.data_period.end'), 'subsec') \
                 <= unixepoch(?, 'subsec')",
            );
            binds.push(before.to_rfc3339());
        }

        // BUG-02: bind the cursor and LIMIT as part of the SQL query so
        // pagination doesn't fetch the entire matching set into Rust
        // and discard it. The +1 sentinel lets us tell whether another
        // page exists. The visibility filter that runs in Rust below
        // can still drop a few rows, so the returned page size may be
        // slightly under `limit`; this is the same trade-off
        // `list_contexts` accepts.
        let cursor_anchor = params
            .cursor
            .as_deref()
            .map(decode_cursor)
            .transpose()?
            .flatten();
        if let Some((anchor_ts, anchor_id)) = cursor_anchor.as_ref() {
            sql.push_str(" AND (created_at < ? OR (created_at = ? AND ctx_id > ?))");
            let anchor_rfc = anchor_ts.to_rfc3339();
            binds.push(anchor_rfc.clone());
            binds.push(anchor_rfc);
            binds.push(anchor_id.clone());
        }
        let limit = params.limit.unwrap_or(50).min(100) as usize;
        sql.push_str(" ORDER BY created_at DESC, ctx_id ASC LIMIT ?");

        let mut q = sqlx::query(&sql);
        // Disclosure binds first — same textual order as SEARCH_VISIBILITY_SQLITE.
        let req = requester_s.as_deref();
        q = q
            .bind(req) // public:     ? IS NOT NULL
            .bind(public_arm_open) // public:     OR ?
            .bind(req) // restricted: ? IS NOT NULL
            .bind(req) // restricted: agent_id = ?
            .bind(req) // restricted: audience value = ?
            .bind(req) // private:    ? IS NOT NULL
            .bind(req); // private:    agent_id = ?
        for b in &binds {
            q = q.bind(b);
        }
        q = q.bind((limit as i64) + 1);
        let rows = q.fetch_all(&self.pool).await.map_err(map_sqlx_err)?;

        // DESIGN-01: `COUNT(*) OVER ()` rides the same scan, so the total
        // is the count of §4.5-visible rows matching the SQL filters
        // (before the LIMIT). It is an ESTIMATE: the post-SQL status /
        // tags / derived_from refinements below are not reflected, so it
        // is an upper bound on the returned matches. Crucially, the §4.5
        // visibility dimension IS in SQL, so the total never counts a
        // restricted/private context the requester may not see.
        let total_estimate = match rows.first() {
            Some(r) => Some(
                r.try_get::<i64, _>("total_rows")
                    .map_err(map_sqlx_err)?
                    .max(0) as u64,
            ),
            None => Some(0),
        };

        let now = Utc::now();
        let want_status = params.status.as_deref().unwrap_or("active");

        // REG-P2-8: the `limit + 1` sentinel and the "anchor the next
        // cursor on the last RAW scanned row, not the last visible
        // match" rule (a page whose rows are all dropped by the
        // disclosure / status / tag / derived_from filters must not
        // stop pagination early) are owned by `acdp::pagination`.
        let page = try_paginate_rows(
            rows,
            limit,
            |row| -> Result<FullContext, AcdpError> {
                let body_json: String = row.try_get("body_json").map_err(map_sqlx_err)?;
                let status: String = row.try_get("status").map_err(map_sqlx_err)?;
                // RFC-ACDP-0013 §8.2: project the retraction flag so a
                // retracted context falls out of the default (active)
                // filter — and out of status=superseded / status=expired
                // even where those facts also hold (§7.2 precedence).
                let retracted: i64 = row.try_get("retracted").map_err(map_sqlx_err)?;
                let body: Body = serde_json::from_str(&body_json)
                    .map_err(|e| AcdpError::RegistryInternal(format!("decode body: {e}")))?;
                // Receipts aren't projected into SearchResult rows, so the
                // search SELECT deliberately skips the column.
                let stored = if retracted != 0 {
                    Status::Retracted
                } else {
                    parse_status(&status)
                };
                let mut ctx = full_context(body, stored, None);
                ctx.registry_state.status =
                    project_status_inline(&ctx.registry_state.status, ctx.body.expires_at, now);
                Ok(ctx)
            },
            |ctx| {
                // DESIGN-01: §4.5 search disclosure is enforced in SQL
                // above; the raw scanned rows are already disclosure-
                // scoped. Remaining post-SQL refinements: status
                // projection, tags (stored as JSON), and derived_from.
                if ctx.registry_state.status.as_str() != want_status {
                    return false;
                }
                // tag filter — kept post-SQL because we store as JSON.
                if let Some(t) = &params.tags {
                    let want: Vec<&str> = t
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect();
                    let body_tags = ctx.body.tags.as_deref().unwrap_or(&[]);
                    if !want.iter().all(|w| body_tags.iter().any(|bt| bt == w)) {
                        return false;
                    }
                }
                // derived_from filter — also post-SQL.
                params
                    .derived_from
                    .as_ref()
                    .is_none_or(|df| ctx.body.derived_from.iter().any(|c| c.as_str() == df))
            },
            |ctx| {
                encode_cursor(
                    ctx.body.created_at.timestamp_millis(),
                    ctx.body.ctx_id.as_str(),
                )
            },
        )?;
        let (matches, next_cursor) = (page.items, page.next_cursor);

        let projected: Vec<SearchResult> = matches
            .iter()
            .map(|ctx| SearchResult {
                ctx_id: ctx.body.ctx_id.clone(),
                lineage_id: ctx.body.lineage_id.clone(),
                agent_id: ctx.body.agent_id.clone(),
                title: ctx.body.title.clone(),
                summary: ctx.body.summary.clone(),
                context_type: ctx.body.context_type.clone(),
                domain: ctx.body.domain.clone(),
                created_at: ctx.body.created_at,
                status: ctx.registry_state.status.clone(),
                visibility: Some(ctx.body.visibility.clone()),
            })
            .collect();

        // DESIGN-01: total_estimate is now the pre-page count of
        // §4.5-visible rows (from `COUNT(*) OVER ()`), not the page size.
        Ok(SearchResponse {
            matches: projected,
            total_estimate,
            next_cursor,
        })
    }
    async fn idempotency_evict_inner(&self, now: DateTime<Utc>) -> Result<(), AcdpError> {
        sqlx::query("DELETE FROM idempotency_records WHERE expires_at_ms <= ?")
            .bind(now.timestamp_millis())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(map_sqlx_err)
    }

    /// Public wrapper for the background eviction task spawned by the
    /// server binary. Inline RFC3339 -> ms cleanup is not on the critical
    /// path of any HTTP handler.
    pub async fn evict_idempotency(&self, now: DateTime<Utc>) -> Result<(), AcdpError> {
        self.idempotency_evict_inner(now).await
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn insert_body<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Sqlite>,
    body: &Body,
    status: Status,
    tenant: Option<&str>,
    receipt: Option<&serde_json::Value>,
) -> Result<(), AcdpError> {
    let body_json = serde_json::to_string(body)
        .map_err(|e| AcdpError::RegistryInternal(format!("encode body: {e}")))?;
    let contributors = serde_json::to_string(&body.contributors)
        .map_err(|e| AcdpError::RegistryInternal(format!("encode contribs: {e}")))?;
    let tags = serde_json::to_string(body.tags.as_deref().unwrap_or(&[]))
        .map_err(|e| AcdpError::RegistryInternal(format!("encode tags: {e}")))?;
    let visibility = match body.visibility {
        Visibility::Public => "public",
        Visibility::Restricted => "restricted",
        Visibility::Private => "private",
    };
    let context_type = context_type_str(&body.context_type);

    // P0 (#3): write tenant_id in the SAME INSERT as the context row so the
    // tenancy is atomic with the row. The previous design committed the row
    // with the column default ('default') and stamped the real tenant in a
    // separate, non-transactional UPDATE — a crash/error in between stranded
    // the context in the 'default' (untenanted) bucket permanently.
    let tenant_id = tenant.unwrap_or("default");
    // RFC-ACDP-0010 §7: the receipt rides the SAME INSERT as the context row,
    // so receipt-and-context atomicity is structural, not transactional
    // bookkeeping a refactor could break.
    let receipt_json = receipt
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AcdpError::RegistryInternal(format!("encode receipt: {e}")))?;
    sqlx::query(
        "INSERT INTO contexts (\
            ctx_id, lineage_id, agent_id, contributors, origin_registry, \
            created_at, status, visibility, context_type, version, supersedes, \
            title, description, summary, domain, tags, expires_at, content_hash, body_json, \
            tenant_id, registry_receipt\
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(body.ctx_id.as_str())
    .bind(body.lineage_id.as_str())
    .bind(body.agent_id.as_str())
    .bind(contributors)
    .bind(&body.origin_registry)
    .bind(body.created_at.to_rfc3339())
    .bind(status.as_str())
    .bind(visibility)
    .bind(context_type)
    .bind(body.version as i64)
    .bind(body.supersedes.as_ref().map(|c| c.as_str().to_string()))
    .bind(&body.title)
    .bind(body.description.clone())
    .bind(body.summary.clone())
    .bind(body.domain.clone())
    .bind(tags)
    .bind(body.expires_at.map(|t| t.to_rfc3339()))
    .bind(body.content_hash.0.as_str())
    .bind(body_json)
    .bind(tenant_id)
    .bind(receipt_json)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_err)?;

    // Lineage head bookkeeping.
    sqlx::query(
        "INSERT INTO lineages (lineage_id, first_version_ctx, latest_ctx) \
         VALUES (?, ?, ?) \
         ON CONFLICT(lineage_id) DO UPDATE SET latest_ctx = excluded.latest_ctx",
    )
    .bind(body.lineage_id.as_str())
    .bind(body.ctx_id.as_str())
    .bind(body.ctx_id.as_str())
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_err)?;

    Ok(())
}

fn full_context(body: Body, status: Status, receipt: Option<serde_json::Value>) -> FullContext {
    FullContext {
        body,
        registry_state: RegistryState {
            status,
            lifecycle_events: None,
            extensions: Default::default(),
        },
        registry_receipt: receipt,
        lineage_head_receipt: None,
        log_inclusion: None,
        extensions: Default::default(),
    }
}

/// Decode one `body_json, status, registry_receipt, retracted` row into a
/// [`FullContext`]. The receipt column is TEXT (JSON) and nullable —
/// `None` for contexts published before receipts were enabled.
///
/// STATUS PROJECTION (RFC-ACDP-0013 §7.2): the stored `status` column
/// tracks supersession ONLY; the denormalized `retracted` flag (kept in
/// lockstep with `lifecycle_events` by `commit_lifecycle_event`) dominates
/// it here, so the materialized context always carries the
/// `retracted > superseded > expired > active` precedence. Expiry is
/// projected afterwards by [`project_context`].
fn row_to_context(r: &sqlx::sqlite::SqliteRow) -> Result<FullContext, AcdpError> {
    let body_json: String = r.try_get("body_json").map_err(map_sqlx_err)?;
    let status: String = r.try_get("status").map_err(map_sqlx_err)?;
    let retracted: i64 = r.try_get("retracted").map_err(map_sqlx_err)?;
    let receipt_raw: Option<String> = r.try_get("registry_receipt").map_err(map_sqlx_err)?;
    let body: Body = serde_json::from_str(&body_json)
        .map_err(|e| AcdpError::RegistryInternal(format!("decode body: {e}")))?;
    let receipt = receipt_raw
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| AcdpError::RegistryInternal(format!("decode receipt: {e}")))?;
    let status = if retracted != 0 {
        Status::Retracted
    } else {
        parse_status(&status)
    };
    Ok(full_context(body, status, receipt))
}

/// Canonical millisecond-precision RFC 3339 UTC text (RFC-ACDP-0001 §5.3)
/// — the exact byte form the strict event serde emits; `occurred_at` is a
/// signed member and must be stored/re-served byte-identically.
fn canonical_ms(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Decode one `lifecycle_events` row back into a validated
/// [`LifecycleEvent`] through the closed RFC-ACDP-0013 §4 schema. Every
/// column was written from a strictly parsed event, so this round-trips
/// byte-identically (including the signed `occurred_at` form).
fn event_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<LifecycleEvent, AcdpError> {
    let event_id: String = r.try_get("event_id").map_err(map_sqlx_err)?;
    let ctx_id: String = r.try_get("ctx_id").map_err(map_sqlx_err)?;
    let event_type: String = r.try_get("event_type").map_err(map_sqlx_err)?;
    let occurred_at: String = r.try_get("occurred_at").map_err(map_sqlx_err)?;
    let actor: String = r.try_get("actor").map_err(map_sqlx_err)?;
    let reason: Option<String> = r.try_get("reason").map_err(map_sqlx_err)?;
    let signature: Option<String> = r.try_get("signature").map_err(map_sqlx_err)?;
    let mut value = serde_json::json!({
        "event_id": event_id,
        "ctx_id": ctx_id,
        "event_type": event_type,
        "occurred_at": occurred_at,
        "actor": actor,
    });
    if let Some(reason) = reason {
        value["reason"] = serde_json::Value::String(reason);
    }
    if let Some(sig) = signature {
        value["signature"] = serde_json::from_str(&sig)
            .map_err(|e| AcdpError::RegistryInternal(format!("decode event signature: {e}")))?;
    }
    LifecycleEvent::from_value(&value)
}

/// One context's lifecycle events in registry acceptance order (`seq`).
async fn events_for_ctx(pool: &SqlitePool, ctx_id: &str) -> Result<Vec<LifecycleEvent>, AcdpError> {
    let rows = sqlx::query(
        "SELECT event_id, ctx_id, event_type, occurred_at, actor, reason, signature \
         FROM lifecycle_events WHERE ctx_id = ? ORDER BY seq ASC",
    )
    .bind(ctx_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_err)?;
    rows.iter().map(event_from_row).collect()
}

/// All lifecycle events across a lineage, grouped by ctx_id, each group
/// in acceptance order — one round-trip for the whole lineage array.
async fn events_for_lineage(
    pool: &SqlitePool,
    lineage_id: &str,
) -> Result<std::collections::HashMap<String, Vec<LifecycleEvent>>, AcdpError> {
    let rows = sqlx::query(
        "SELECT e.event_id, e.ctx_id, e.event_type, e.occurred_at, e.actor, e.reason, e.signature \
         FROM lifecycle_events e \
         JOIN contexts c ON c.ctx_id = e.ctx_id \
         WHERE c.lineage_id = ? ORDER BY e.seq ASC",
    )
    .bind(lineage_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_err)?;
    let mut grouped: std::collections::HashMap<String, Vec<LifecycleEvent>> =
        std::collections::HashMap::new();
    for r in &rows {
        let event = event_from_row(r)?;
        grouped
            .entry(event.ctx_id.as_str().to_string())
            .or_default()
            .push(event);
    }
    Ok(grouped)
}

/// Attach a (possibly empty) event history to a context, honoring the
/// absent-vs-empty wire rule (RFC-ACDP-0013 §4.1: omit, never `[]`).
fn attach_events(mut ctx: FullContext, events: Vec<LifecycleEvent>) -> FullContext {
    ctx.registry_state.lifecycle_events = if events.is_empty() {
        None
    } else {
        Some(events)
    };
    ctx
}

/// DESIGN-04: typed accessor for the wire-form of `ContextType`. The prior
/// implementation went through `serde_json::to_value(...).as_str()`, which
/// silently produced an empty string for any future multi-field variant.
/// Matching directly on the enum also avoids an allocation per insert.
fn context_type_str(t: &acdp::types::primitives::ContextType) -> String {
    use acdp::types::primitives::ContextType;
    match t {
        ContextType::DataSnapshot => "data_snapshot".into(),
        ContextType::Analysis => "analysis".into(),
        ContextType::Prediction => "prediction".into(),
        ContextType::Alert => "alert".into(),
        ContextType::KeyRevocation => "key-revocation".into(),
        ContextType::Custom(s) => s.clone(),
    }
}

fn parse_status(s: &str) -> Status {
    match s {
        "active" => Status::Active,
        "superseded" => Status::Superseded,
        "expired" => Status::Expired,
        "retracted" => Status::Retracted,
        other => Status::Other(other.to_string()),
    }
}

fn project_status_inline(
    stored: &Status,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Status {
    match stored {
        Status::Active => match expires_at {
            Some(exp) if exp <= now => Status::Expired,
            _ => Status::Active,
        },
        other => other.clone(),
    }
}

fn project_context(mut ctx: FullContext, now: DateTime<Utc>) -> FullContext {
    ctx.registry_state.status =
        project_status_inline(&ctx.registry_state.status, ctx.body.expires_at, now);
    ctx
}

fn parse_opt_rfc3339(s: &Option<String>) -> Result<Option<DateTime<Utc>>, AcdpError> {
    let Some(raw) = s.as_deref() else {
        return Ok(None);
    };
    let dt = DateTime::parse_from_rfc3339(raw)
        .map_err(|e| AcdpError::SchemaViolation(format!("malformed datetime '{raw}': {e}")))?;
    Ok(Some(dt.with_timezone(&Utc)))
}

/// FTS5 input sanitization, and the query half of SQLite/Postgres `q=` parity.
///
/// Tokenizes on Unicode whitespace, drops stopwords, quotes each surviving
/// token as an FTS5 string literal, and joins with implicit AND (the default
/// FTS5 operator) — so `q=foo bar` matches documents containing BOTH terms,
/// which is what Postgres `plainto_tsquery` does with its terms too.
///
/// Per-token quoting neutralizes FTS5 operator syntax (`NOT`, `AND`, `OR`,
/// `NEAR`, column filters, `^`, `+`, `-`, `(`, `)`); embedded `"` characters
/// are doubled per FTS5 string-literal rules. Quoting is preserved
/// deliberately: it was measured that the `porter` stemmer still applies
/// inside a quoted phrase, so stemming and operator-neutralization are not in
/// tension and there is no reason to weaken the quoting.
///
/// # Parity with Postgres
///
/// **SQLite was brought to Postgres's semantics, not the other way round.**
/// Postgres is the production backend and stemming is better search
/// behaviour, so degrading it to reach agreement would have been a real
/// product regression; the cost instead lands on SQLite's FTS index, which is
/// derived data rebuilt from `contexts`.
///
/// Two mechanisms had to be matched, and they are matched in two different
/// places:
///
/// * **Stemming** — migration `013_fts5_porter.sql` switches `contexts_fts` to
///   `tokenize = 'porter unicode61'`. Handled at index+query time by FTS5.
/// * **Stopwords** — porter does NOT drop them (measured), so they are dropped
///   here, on the query side, using `acdp_registry_store::fulltext`.
///
/// # The limit, stated rather than implied
///
/// FTS5 `porter` and Postgres's snowball `english` are **different
/// implementations** and will not agree on every word in the language. The
/// parity suite therefore pins the *mechanisms* (a stemmed match, a stopword
/// query, case, punctuation) rather than claiming stemmer identity, and
/// the stopword list is verified against Postgres's own oracle instead of
/// being trusted as a hand-copied table. See
/// `acdp_registry_store::parity::assert_fulltext_parity`.
///
/// Per-token quoting neutralizes FTS5 operator syntax (`NOT`, `AND`,
/// `OR`, `NEAR`, column filters, `^`, `+`, `-`, `(`, `)`); embedded
/// `"` characters are doubled per FTS5 string-literal rules.
fn fts5_escape(q: &str) -> String {
    let tokens: Vec<String> = q
        .split_whitespace()
        .filter(|t| !t.is_empty())
        // Stopword removal, matching `plainto_tsquery('english', …)`. Compared
        // case-insensitively because Postgres lowercases before consulting the
        // list; `to_lowercase` (not `to_ascii_lowercase`) so a non-ASCII token
        // is not silently treated as a different word.
        .filter(|t| !acdp_registry_store::fulltext::is_pg_english_stopword(t))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        // FTS5 rejects a bare empty expression; synthesize a token that
        // can't appear in any indexed document so the query returns no
        // rows. The handler still applies non-FTS filters.
        "\"__acdp_empty_query__\"".into()
    } else {
        tokens.join(" ")
    }
}

fn map_sqlx_err(e: sqlx::Error) -> AcdpError {
    AcdpError::RegistryInternal(format!("sqlite: {e}"))
}

#[cfg(test)]
mod tests {
    use super::fts5_escape;

    #[test]
    fn fts5_escape_single_token() {
        assert_eq!(fts5_escape("hello"), "\"hello\"");
    }

    #[test]
    fn fts5_escape_handles_numeric_and_unicode_tokens() {
        // Numeric-only and multi-byte tokens must still be quoted (not dropped,
        // not treated as operators) so they participate in the AND query.
        assert_eq!(super::fts5_escape("2024 report"), "\"2024\" \"report\"");
        assert_eq!(super::fts5_escape("café"), "\"café\"");
        // A column-filter injection attempt (`title:`) is neutralized by quoting.
        assert_eq!(super::fts5_escape("title:secret"), "\"title:secret\"");
    }

    #[test]
    fn fts5_escape_tokens_anded() {
        // Implicit AND between quoted tokens is FTS5's default operator.
        assert_eq!(fts5_escape("hello world"), "\"hello\" \"world\"");
    }

    #[test]
    fn fts5_escape_doubles_embedded_quotes() {
        assert_eq!(fts5_escape("foo\"bar"), "\"foo\"\"bar\"");
    }

    #[test]
    fn fts5_escape_quotes_operator_keywords() {
        // `NOT`, `AND`, `OR`, `NEAR` are FTS5 operators, and a caller must not
        // be able to inject operator syntax through `q=`.
        //
        // Three of those four keywords are also PostgreSQL `english`
        // stopwords, so they are now DROPPED before quoting rather than
        // quoted — which is exactly what Postgres does, verified against the
        // server:
        //
        //     plainto_tsquery('english', 'NOT hack')  ->  'hack'
        //
        // This test previously asserted `"NOT" "hack"`, which was the
        // divergent behaviour. Dropping is strictly safer than quoting here:
        // the caller cannot obtain operator semantics either way, and with
        // `OR` removed they cannot even widen the query to a disjunction.
        assert_eq!(fts5_escape("NOT hack"), "\"hack\"");
        assert_eq!(fts5_escape("foo OR bar"), "\"foo\" \"bar\"");
        assert_eq!(fts5_escape("foo AND bar"), "\"foo\" \"bar\"");

        // `NEAR` is the one FTS5 operator that is NOT a stopword — Postgres
        // keeps it (`plainto_tsquery('english', 'NEAR hack')` -> `'near' &
        // 'hack'`), so it survives tokenization and quoting is what stops it
        // being read as an operator. This assertion carries the
        // operator-neutralization property now that the other three are
        // dropped before they ever reach the quoter.
        assert_eq!(fts5_escape("NEAR hack"), "\"NEAR\" \"hack\"");

        // Non-keyword operator characters are not stopwords, so they are still
        // quoted rather than dropped.
        assert_eq!(fts5_escape("^foo"), "\"^foo\"");
        assert_eq!(fts5_escape("(bar)"), "\"(bar)\"");
    }

    #[test]
    fn fts5_escape_all_stopword_query_yields_the_empty_sentinel() {
        // Postgres produces an empty tsquery for a stopword-only query
        // (`plainto_tsquery('english', 'AND OR NOT')` -> `''`), which matches
        // nothing. SQLite must reach the same result, and an empty FTS5
        // expression is a syntax error rather than an empty match — hence the
        // sentinel token, which cannot appear in any indexed document.
        assert_eq!(fts5_escape("the"), "\"__acdp_empty_query__\"");
        assert_eq!(fts5_escape("AND OR NOT"), "\"__acdp_empty_query__\"");
        assert_eq!(fts5_escape("of the and a"), "\"__acdp_empty_query__\"");

        // A stopword next to a real term must leave the real term working —
        // the whole query is not discarded just because part of it was.
        assert_eq!(fts5_escape("the report"), "\"report\"");
    }

    #[test]
    fn fts5_escape_empty_yields_sentinel() {
        assert_eq!(fts5_escape("   "), "\"__acdp_empty_query__\"");
        assert_eq!(fts5_escape(""), "\"__acdp_empty_query__\"");
    }

    /// P0 #5: two concurrent publishes sharing one Idempotency-Key must yield
    /// exactly ONE persisted context. `ctx_id` is random per request, so before
    /// the claim-gate (ON CONFLICT DO NOTHING + rollback-and-replay) both racers
    /// would INSERT distinct contexts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_publish_same_idempotency_key_creates_one_context() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::producer::Producer;
        use acdp::registry::store::{
            PendingIdempotencyCommit, PublishCommit, PublishCommitOutcome, RegistryStore,
        };
        use acdp::types::primitives::{AgentDid, ContextType, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 4).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[9u8; 32]),
            AgentDid::new("did:web:agents.test:race".to_string()),
            "did:web:agents.test:race#key-1".to_string(),
        );
        let req = p
            .publish_request()
            .title("race")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        let spawn = |store: Arc<SqliteStore>, req: acdp::types::publish::PublishRequest| {
            tokio::task::spawn_blocking(move || {
                store.commit_publish(PublishCommit {
                    req: &req,
                    authority: "reg.test",
                    idempotency: Some(PendingIdempotencyCommit {
                        key: "race-key",
                        ttl: chrono::Duration::hours(1),
                    }),
                    tenant: None,
                    receipt_minter: None,
                    predecessor_admission: None,
                })
            })
        };

        let h1 = spawn(store.clone(), req.clone());
        let h2 = spawn(store.clone(), req.clone());
        let (r1, r2) = tokio::join!(h1, h2);
        let r1 = r1.unwrap().expect("publish 1 ok");
        let r2 = r2.unwrap().expect("publish 2 ok");

        let ctx_id = |o: &PublishCommitOutcome| match o {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id.as_str().to_string()
            }
        };
        assert_eq!(
            ctx_id(&r1),
            ctx_id(&r2),
            "both racers must resolve to the same ctx_id"
        );
        let inserted = [&r1, &r2]
            .iter()
            .filter(|o| matches!(o, PublishCommitOutcome::Inserted(_)))
            .count();
        assert_eq!(
            inserted, 1,
            "exactly one publish inserts; the other replays"
        );

        let page = store
            .list_contexts(100, None, None, None, true)
            .await
            .unwrap();
        assert_eq!(
            page.items.len(),
            1,
            "exactly one context row must exist after a concurrent idempotent publish"
        );
    }

    /// P0 #3: a tenant-scoped publish must write `tenant_id` in the same
    /// transaction as the context row (no separate stamping UPDATE that a
    /// crash could leave stranded in the 'default' bucket).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_publish_writes_tenant_atomically() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();

        let p = Producer::new(
            SigningKey::from_bytes(&[11u8; 32]),
            AgentDid::new("did:web:agents.test:tenant".to_string()),
            "did:web:agents.test:tenant#key-1".to_string(),
        );
        let req = p
            .publish_request()
            .title("scoped")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        let req2 = req.clone();
        let store2 = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            store2.commit_publish(PublishCommit {
                req: &req2,
                authority: "reg.test",
                idempotency: None,
                tenant: Some("tenant-x"),
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        let ctx_id = match &outcome {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id.as_str().to_string()
            }
        };

        let tenant = store.tenant_of_ctx(&ctx_id).await.unwrap();
        assert_eq!(
            tenant.as_deref(),
            Some("tenant-x"),
            "tenant_id must be persisted atomically with the context row"
        );
    }

    /// Supersession-ownership plan (tenant dimension): a successor must live in
    /// the same tenant as its predecessor. A v2 committed under a different
    /// tenant than v1 is rejected (NotFound shape); same-tenant succeeds.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commit_publish_rejects_cross_tenant_supersession() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::{AcdpError, SupersessionReason};
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[13u8; 32]),
            AgentDid::new("did:web:agents.test:rebind".to_string()),
            "did:web:agents.test:rebind#key-1".to_string(),
        );

        // v1 committed under tenant-a.
        let v1 = p
            .publish_request()
            .title("v1")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let s = store.clone();
        let v1r = v1.clone();
        let v1_resp = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v1r,
                authority: "reg.test",
                idempotency: None,
                tenant: Some("tenant-a"),
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        let v1_ctx = match v1_resp {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id
            }
        };
        let v1_body = store
            .get(&CtxId(v1_ctx.as_str().to_string()))
            .unwrap()
            .unwrap()
            .body;

        // v2 superseding v1 but committed under tenant-b → rejected.
        let v2 = p
            .supersede_body(&v1_body)
            .title("v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let s = store.clone();
        let v2c = v2.clone();
        let cross = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2c,
                authority: "reg.test",
                idempotency: None,
                tenant: Some("tenant-b"),
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap();
        assert!(
            matches!(
                cross,
                Err(AcdpError::SupersededTarget {
                    reason: SupersessionReason::NotFound,
                    ..
                })
            ),
            "cross-tenant supersession must be rejected (NotFound), got {cross:?}"
        );

        // v2 under the same tenant (tenant-a) → succeeds.
        let s = store.clone();
        let ok = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: Some("tenant-a"),
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap();
        assert!(
            ok.is_ok(),
            "same-tenant supersession must succeed, got {ok:?}"
        );
    }

    /// RFC-ACDP-0010 §7 crash-consistency: a failing receipt minter must
    /// abort the WHOLE publish — no context row, no idempotency record. A
    /// crash between "insert context" and "mint receipt" is structurally
    /// unobservable because both ride one INSERT in one transaction; this
    /// test pins the only seam left (the minter erroring mid-transaction).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receipt_minter_failure_aborts_the_whole_publish() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::producer::Producer;
        use acdp::registry::store::{PendingIdempotencyCommit, PublishCommit, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();

        let p = Producer::new(
            SigningKey::from_bytes(&[21u8; 32]),
            AgentDid::new("did:web:agents.test:mintfail".to_string()),
            "did:web:agents.test:mintfail#key-1".to_string(),
        );
        let req = p
            .publish_request()
            .title("doomed")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        let s = store.clone();
        let failing_minter = |_: &acdp::types::body::Body| {
            Err(acdp::error::AcdpError::RegistryInternal(
                "simulated KMS outage".into(),
            ))
        };
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &req,
                authority: "reg.test",
                idempotency: Some(PendingIdempotencyCommit {
                    key: "mintfail-key",
                    ttl: chrono::Duration::hours(1),
                }),
                tenant: None,
                receipt_minter: Some(&failing_minter),
                predecessor_admission: None,
            })
        })
        .await
        .unwrap();
        assert!(outcome.is_err(), "minting failure must fail the publish");

        let page = store
            .list_contexts(100, None, None, None, true)
            .await
            .unwrap();
        assert!(
            page.items.is_empty(),
            "no context row may survive a failed receipt mint"
        );
        assert_eq!(
            store.count_idempotency_records().await.unwrap(),
            Some(0),
            "no idempotency record may survive a failed receipt mint"
        );
    }

    // ── RFC-ACDP-0014 §4 `predecessor_admission` enforcement ──────────────
    //
    // These eight tests exist because nothing else in this repo would catch the
    // admission hook being removed, defanged, hoisted or fast-pathed. acdp 0.10.0 added the
    // hook as a plain (non-`#[non_exhaustive]`) struct field, so a store that
    // binds it and never calls it COMPILES CLEANLY and silently stops enforcing
    // a normative MUST — no failure, no warning, and the conformance fixtures
    // do not cover the reject path (spec issue #57).
    //
    // Each test kills a specific mutation. Do not weaken one without checking
    // which mutation it was the only guard against:
    //
    //   delete the `admit(..)?` call            -> tests 1, 2, 2b fail
    //   hoist above the `!is_owner` gate        -> test 3 fails
    //   hoist above the AlreadySuperseded gate  -> test 4 fails
    //   swap the parse `?` for `if let Ok`      -> test 5 fails
    //   swallow only SchemaViolation            -> test 2b fails
    //   read the lineage-head row instead       -> test 2 fails
    //   skip on a key-revocation PREDECESSOR    -> test 2c fails
    //   skip on tenant / minter / non-public /
    //     key-revocation SUCCESSOR              -> test 2d fails
    //
    // Deliberately NOT guarded, because it is an equivalent mutant rather than a
    // defect: `if prev_status == "active"`. The `status` column tracks
    // supersession ONLY (see migrations/010_lifecycle_events.sql) and is written
    // in exactly one shape, `SET status = 'superseded'`, so it is always either
    // 'active' or 'superseded' — and the guard directly above returns Err on
    // 'superseded'. At this line `prev_status` is necessarily "active", so the
    // condition is a tautology. No test can kill it and none should try.

    /// Test 1 — refusal aborts the WHOLE publish. The closure's error must
    /// propagate unwrapped, and nothing may survive: no successor row, no
    /// supersession of the predecessor, no idempotency record.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_refusal_aborts_the_whole_publish() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::AcdpError;
        use acdp::producer::Producer;
        use acdp::registry::store::{PendingIdempotencyCommit, PublishCommit, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Status, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[41u8; 32]),
            AgentDid::new("did:web:agents.test:admitdeny".to_string()),
            "did:web:agents.test:admitdeny#key-1".to_string(),
        );

        let (v1_ctx, v1_body) = publish_v1(&store, &p, "v1").await;

        let v2 = p
            .supersede_body(&v1_body)
            .title("v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        // A refusal shape the store itself never produces, so the assertion
        // below cannot pass by accident on some unrelated rejection.
        let deny = |_: &acdp::types::body::Body| {
            Err(AcdpError::NotAuthorized(
                "succession refused by policy".into(),
            ))
        };
        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: Some(PendingIdempotencyCommit {
                    key: "admit-deny-key",
                    ttl: chrono::Duration::hours(1),
                }),
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&deny),
            })
        })
        .await
        .unwrap();

        // Propagated unwrapped — NOT remapped to RegistryInternal.
        assert!(
            matches!(outcome, Err(AcdpError::NotAuthorized(_))),
            "the admission closure's own error must propagate unwrapped, got {outcome:?}"
        );

        // The predecessor must NOT have been marked superseded.
        let v1_after = store.get(&CtxId(v1_ctx.clone())).unwrap().unwrap();
        assert_eq!(
            v1_after.registry_state.status,
            Status::Active,
            "a refused succession must not supersede the predecessor"
        );

        // The successor must NOT have been inserted.
        let page = store
            .list_contexts(100, None, None, None, true)
            .await
            .unwrap();
        assert_eq!(
            page.items.len(),
            1,
            "a refused succession must not insert the successor"
        );

        // No idempotency record may survive either.
        assert_eq!(
            store.count_idempotency_records().await.unwrap(),
            Some(0),
            "a refused succession must not claim its idempotency key"
        );
    }

    /// Test 2 — the hook receives the PREDECESSOR's body, not the successor's.
    /// A mis-wiring that passes the new body satisfies test 1 while checking
    /// the wrong thing entirely.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_receives_the_predecessors_body() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::AcdpError;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::{Arc, Mutex};

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[42u8; 32]),
            AgentDid::new("did:web:agents.test:admitbody".to_string()),
            "did:web:agents.test:admitbody#key-1".to_string(),
        );

        // A THREE-deep lineage on purpose. With only v1->v2 the immediate
        // predecessor is ALSO the lineage head, so a store that decoded the
        // lineage-head row — the `first_row` SELECT sits ~10 lines below the
        // hook in the same transaction — would be indistinguishable from a
        // correct one. RFC-ACDP-0014 §4 keys on the IMMEDIATE predecessor's
        // context_type and trust_class, so handing over v1 while superseding v2
        // would both wrongly admit and wrongly refuse.
        let (v1_ctx, v1_body) = publish_v1(&store, &p, "the-lineage-head").await;

        let v2 = p
            .supersede_body(&v1_body)
            .title("the-immediate-predecessor")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        // Captured from the in-memory REQUEST. Comparing the hook's body against
        // `store.get(..).body` instead would be decode-vs-decode: that value
        // round-trips through the same `from_str::<Body>` the hook uses, so a
        // lossy decode would corrupt both sides identically and still pass.
        let v2_req_hash = v2.content_hash.clone();
        let s = store.clone();
        let v2c = v2.clone();
        let v2_out = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2c,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        let v2_ctx = match v2_out {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id.as_str().to_string()
            }
        };
        let v2_body = store.get(&CtxId(v2_ctx.clone())).unwrap().unwrap().body;
        assert_ne!(
            v1_ctx, v2_ctx,
            "fixture sanity: the lineage must actually be three deep"
        );

        // v3 supersedes v2 — so the hook must be handed V2's body, not v1's.
        let v3 = p
            .supersede_body(&v2_body)
            .title("the-successor")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        #[allow(clippy::type_complexity)]
        let seen: Arc<
            Mutex<Option<(String, String, acdp::types::primitives::ContentHash)>>,
        > = Arc::new(Mutex::new(None));
        let seen_c = seen.clone();
        let capture = move |b: &acdp::types::body::Body| {
            *seen_c.lock().unwrap() = Some((
                b.ctx_id.as_str().to_string(),
                b.title.clone(),
                b.content_hash.clone(),
            ));
            Err(AcdpError::NotAuthorized("stop here".into()))
        };

        let s = store.clone();
        let _ = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v3,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&capture),
            })
        })
        .await
        .unwrap();

        let got = seen.lock().unwrap().clone();
        let (got_ctx, got_title, got_hash) =
            got.expect("the admission hook must have been invoked");
        assert_eq!(
            got_ctx, v2_ctx,
            "the hook must receive the IMMEDIATE predecessor (v2), not the lineage head (v1)"
        );
        assert_eq!(
            got_title, "the-immediate-predecessor",
            "the hook must receive the IMMEDIATE predecessor (v2), not the lineage head (v1)"
        );
        // content_hash pins every producer-controlled field at once — including
        // `context_type`, the FIRST field the real rule reads (arm 4 admits
        // unconditionally when the predecessor is not a key-revocation), so a
        // re-derived or normalized body cannot slip through.
        assert_eq!(
            v2_body.content_hash.as_str(),
            v2_req_hash.as_str(),
            "fixture sanity: the stored hash equals the one the producer signed, which is \
             what makes the assertion below independent of the decode path"
        );
        assert_eq!(
            got_hash, v2_req_hash,
            "the hook's body must carry the hash the PRODUCER signed — compared against the \
             request, not another decode of the same row, so this genuinely proves the \
             stored-body decode is faithful"
        );
    }

    /// Test 2b — the REAL refusal variant propagates. Every other test here
    /// refuses with `NotAuthorized`, chosen because this store never produces
    /// it, so those assertions cannot pass on an unrelated rejection. But the
    /// real closure — upstream `check_revocation_supersession` — can ONLY ever
    /// return `SchemaViolation`. Without this test, a variant-selective
    /// swallow passes the entire suite while disabling enforcement in
    /// production 100% of the time:
    ///
    /// ```ignore
    /// if let Err(e) = admit(&prev_body) {
    ///     if !matches!(e, AcdpError::SchemaViolation(_)) { return Err(e); }
    /// }
    /// ```
    ///
    /// That reads like error normalization rather than deletion, which makes it
    /// more plausible than simply removing the call — and it is precisely the
    /// refusal the RFC-ACDP-0014 §4 rule actually emits.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_propagates_the_real_schema_violation_refusal() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::AcdpError;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Status, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[47u8; 32]),
            AgentDid::new("did:web:agents.test:schemaviol".to_string()),
            "did:web:agents.test:schemaviol#key-1".to_string(),
        );
        let (v1_ctx, v1_body) = publish_v1(&store, &p, "v1").await;

        let v2 = p
            .supersede_body(&v1_body)
            .title("v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        // Exactly what `check_revocation_supersession` returns on both of its
        // refusal arms (§4 arm 2 trust-class mismatch, arm 3 type mismatch).
        let deny = |_: &acdp::types::body::Body| {
            Err(AcdpError::SchemaViolation(
                "supersedes target is a key-revocation; successor must be one too".into(),
            ))
        };
        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&deny),
            })
        })
        .await
        .unwrap();

        assert!(
            matches!(outcome, Err(AcdpError::SchemaViolation(_))),
            "the production refusal variant MUST propagate — a store that swallows \
             SchemaViolation specifically would pass every other test here while \
             enforcing nothing in production. Got {outcome:?}"
        );
        assert_eq!(
            store
                .get(&CtxId(v1_ctx))
                .unwrap()
                .unwrap()
                .registry_state
                .status,
            Status::Active,
            "a SchemaViolation refusal must not supersede the predecessor either"
        );
    }

    /// Test 2c — the hook fires for a KEY-REVOCATION predecessor. Every other
    /// fixture here uses `ContextType::DataSnapshot`, so a guard keyed on the
    /// predecessor's type survives the entire rest of the suite:
    ///
    /// ```ignore
    /// if !prev_body.context_type.is_key_revocation() { admit(&prev_body)?; }
    /// ```
    ///
    /// Every other test still passes — their predecessors are all non-revocations,
    /// so `admit` still runs — while production skips the hook for EXACTLY the
    /// case RFC-ACDP-0014 §4 constrains: arms 1/2/3/6 all sit behind
    /// `prev.context_type.is_key_revocation()`. Same total-disablement profile as
    /// the swallow in test 2b.
    ///
    /// This is a store-level test, so the predecessor need not be a spec-valid
    /// revocation — `commit_publish` never inspects `context_type`, it only hands
    /// the stored body to the closure. What matters is that the type reaches the
    /// hook intact.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_fires_for_a_key_revocation_predecessor() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::{Arc, Mutex};

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[48u8; 32]),
            AgentDid::new("did:web:agents.test:revpred".to_string()),
            "did:web:agents.test:revpred#key-1".to_string(),
        );

        // v1 IS a key-revocation — the one predecessor type §4 actually constrains.
        let v1 = p
            .publish_request()
            .title("revocation-of-key-1")
            .context_type(ContextType::KeyRevocation)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let s = store.clone();
        let v1c = v1.clone();
        let v1_out = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v1c,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        let v1_ctx = match v1_out {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id.as_str().to_string()
            }
        };
        let v1_body = store.get(&CtxId(v1_ctx)).unwrap().unwrap().body;
        assert_eq!(
            v1_body.context_type,
            ContextType::KeyRevocation,
            "fixture sanity: the predecessor must really be a key-revocation"
        );

        let v2 = p
            .supersede_body(&v1_body)
            .title("successor-of-a-revocation")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        let seen: Arc<Mutex<Option<ContextType>>> = Arc::new(Mutex::new(None));
        let seen_c = seen.clone();
        let capture = move |b: &acdp::types::body::Body| {
            *seen_c.lock().unwrap() = Some(b.context_type.clone());
            Ok(())
        };
        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&capture),
            })
        })
        .await
        .unwrap();
        assert!(
            outcome.is_ok(),
            "the closure admitted, so the publish must succeed: {outcome:?}"
        );

        assert_eq!(
            *seen.lock().unwrap(),
            Some(ContextType::KeyRevocation),
            "the hook MUST run when the predecessor is a key-revocation — that is the \
             only case RFC-ACDP-0014 §4 constrains, so a type-keyed guard here would \
             disable the rule entirely while passing every other test"
        );
    }

    /// Test 2d — the hook fires across the axes every other test holds
    /// constant. The rest of this suite always passes `tenant: None`,
    /// `receipt_minter: None`, a `DataSnapshot` successor and
    /// `Visibility::Public`, so a fast path keyed on any of those survives the
    /// whole suite while disabling the rule in production:
    ///
    /// ```ignore
    /// predecessor_admission.filter(|_| tenant.is_none())
    /// predecessor_admission.filter(|_| receipt_minter.is_none())
    /// predecessor_admission.filter(|_| !matches!(req.context_type, ContextType::KeyRevocation))
    /// predecessor_admission.filter(|_| matches!(req.visibility, Visibility::Public))
    /// ```
    ///
    /// All four are production configurations, and the successor-keyed one is
    /// the "optimization" §4's own wording ("successor must be one too")
    /// invites. Note this is the SUCCESSOR's type — test 2c covers the
    /// predecessor's, and the two are independent fast paths. The real gate
    /// (`key_revocation_gate_applies(acdp_version) && req.supersedes.is_some()`)
    /// has no visibility term at all, so a visibility-keyed skip would disable
    /// the MUST for every non-public supersession.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_fires_with_tenant_receipts_and_a_key_revocation_successor() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[49u8; 32]),
            AgentDid::new("did:web:agents.test:axes".to_string()),
            "did:web:agents.test:axes#key-1".to_string(),
        );

        // v1 under a real tenant, with a real receipt minter.
        let minter = |_: &acdp::types::body::Body| Ok(serde_json::json!({"kind": "test-receipt"}));
        let v1 = p
            .publish_request()
            .title("v1-tenanted")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Restricted)
            .audience(vec![AgentDid::new(
                "did:web:agents.test:audience".to_string(),
            )])
            .build()
            .unwrap();
        let s = store.clone();
        let v1_out = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v1,
                authority: "reg.test",
                idempotency: None,
                tenant: Some("tenant-axes"),
                receipt_minter: Some(&minter),
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        let v1_ctx = match v1_out {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id.as_str().to_string()
            }
        };
        let v1_body = store.get(&CtxId(v1_ctx)).unwrap().unwrap().body;

        // Successor is a KEY-REVOCATION, same tenant, minter present.
        let v2 = p
            .supersede_body(&v1_body)
            .title("revocation-successor")
            .context_type(ContextType::KeyRevocation)
            .visibility(Visibility::Restricted)
            .audience(vec![AgentDid::new(
                "did:web:agents.test:audience".to_string(),
            )])
            .build()
            .unwrap();

        let fired = Arc::new(AtomicBool::new(false));
        let fired_c = fired.clone();
        let tripwire = move |_: &acdp::types::body::Body| {
            fired_c.store(true, Ordering::SeqCst);
            Ok(())
        };
        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: Some("tenant-axes"),
                receipt_minter: Some(&minter),
                predecessor_admission: Some(&tripwire),
            })
        })
        .await
        .unwrap();

        assert!(outcome.is_ok(), "the closure admitted: {outcome:?}");
        assert!(
            fired.load(Ordering::SeqCst),
            "the hook MUST fire with a tenant set, a receipt minter present, and a \
             key-revocation SUCCESSOR under RESTRICTED visibility — four axes every other \
             test here holds constant, so a fast path keyed on any of them would otherwise \
             ship silently"
        );
    }

    /// Test 3 — ORDERING / anti-oracle guard. A non-owner superseding someone
    /// else's context must be turned away as `superseded_target{NotFound}`
    /// WITHOUT the admission hook ever firing. If the hook ran first, publish
    /// would become a non-owner existence-and-context_type oracle on the
    /// predecessor — the exact leak the upstream contract forbids. The error
    /// alone does not discriminate (a correct and an oracle-leaking store both
    /// return NotFound), so the captured flag is this test's entire point.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_does_not_fire_for_a_non_owner() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::{AcdpError, SupersessionReason};
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let owner = Producer::new(
            SigningKey::from_bytes(&[43u8; 32]),
            AgentDid::new("did:web:agents.test:owner".to_string()),
            "did:web:agents.test:owner#key-1".to_string(),
        );
        let (_v1_ctx, v1_body) = publish_v1(&store, &owner, "owned").await;

        // A DIFFERENT producer attempts the supersession.
        let stranger = Producer::new(
            SigningKey::from_bytes(&[44u8; 32]),
            AgentDid::new("did:web:agents.test:stranger".to_string()),
            "did:web:agents.test:stranger#key-1".to_string(),
        );
        let v2 = stranger
            .supersede_body(&v1_body)
            .title("hostile-v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        let fired = Arc::new(AtomicBool::new(false));
        let fired_c = fired.clone();
        let tripwire = move |_: &acdp::types::body::Body| {
            fired_c.store(true, Ordering::SeqCst);
            Ok(())
        };

        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&tripwire),
            })
        })
        .await
        .unwrap();

        assert!(
            matches!(
                outcome,
                Err(AcdpError::SupersededTarget {
                    reason: SupersessionReason::NotFound,
                    ..
                })
            ),
            "a non-owner must get the uniform NotFound shape, got {outcome:?}"
        );
        assert!(
            !fired.load(Ordering::SeqCst),
            "the admission hook MUST NOT run for a non-owner — doing so leaks the \
             predecessor's existence and context_type to a caller who owns nothing"
        );
    }

    /// Test 4 — ORDERING guard for the AlreadySuperseded gate. The upstream
    /// contract puts admission strictly after it ("never earlier"), and arm 5
    /// of RFC-ACDP-0014 §4 is explicitly out of the hook's scope. Without this
    /// test, hoisting the call to the ownership gate passes tests 1-3 while
    /// violating the documented caller contract.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_does_not_fire_for_an_already_superseded_predecessor() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::{AcdpError, SupersessionReason};
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[45u8; 32]),
            AgentDid::new("did:web:agents.test:doubleseq".to_string()),
            "did:web:agents.test:doubleseq#key-1".to_string(),
        );
        let (v1_ctx, v1_body) = publish_v1(&store, &p, "v1").await;

        // First supersession succeeds: v1 becomes `superseded`.
        let v2 = p
            .supersede_body(&v1_body)
            .title("v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let s = store.clone();
        let first = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap();
        assert!(
            first.is_ok(),
            "the first supersession must succeed: {first:?}"
        );
        assert_eq!(
            store
                .get(&CtxId(v1_ctx.clone()))
                .unwrap()
                .unwrap()
                .registry_state
                .status,
            acdp::types::primitives::Status::Superseded,
            "precondition: v1 must now be superseded"
        );

        // A SECOND supersession of the same, now-superseded v1.
        let v2b = p
            .supersede_body(&v1_body)
            .title("v2-again")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        let fired = Arc::new(AtomicBool::new(false));
        let fired_c = fired.clone();
        let tripwire = move |_: &acdp::types::body::Body| {
            fired_c.store(true, Ordering::SeqCst);
            Ok(())
        };
        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2b,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&tripwire),
            })
        })
        .await
        .unwrap();

        assert!(
            matches!(
                outcome,
                Err(AcdpError::SupersededTarget {
                    reason: SupersessionReason::AlreadySuperseded,
                    ..
                })
            ),
            "a second supersession must report AlreadySuperseded, got {outcome:?}"
        );
        assert!(
            !fired.load(Ordering::SeqCst),
            "the admission hook MUST NOT run once AlreadySuperseded has rejected — \
             arm 5 of RFC-ACDP-0014 §4 is out of the hook's scope and the upstream \
             contract says 'never earlier'"
        );
    }

    /// Test 5 — a predecessor whose stored body will not deserialize must FAIL
    /// the publish, never silently skip the check. Swallowing the decode (e.g.
    /// `if let Ok(b) = ... { admit(&b)?; }`) passes every other test here,
    /// because their fixtures all hold valid bodies — and turns a corrupt row
    /// into a bypass of the MUST. Deliberately does not assert *which* error,
    /// so it survives the RegistryInternal-vs-SchemaViolation decision.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admission_is_not_skipped_when_the_predecessor_body_is_undecodable() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::error::AcdpError;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();
        let store = Arc::new(store);

        let p = Producer::new(
            SigningKey::from_bytes(&[46u8; 32]),
            AgentDid::new("did:web:agents.test:corrupt".to_string()),
            "did:web:agents.test:corrupt#key-1".to_string(),
        );
        let (v1_ctx, v1_body) = publish_v1(&store, &p, "v1").await;

        let v2 = p
            .supersede_body(&v1_body)
            .title("v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();

        // Corrupt ONLY the predecessor's stored body, behind the store's back.
        // Valid JSON, but not a `Body` — so the failure under test is the
        // decode, not a malformed column.
        sqlx::query("UPDATE contexts SET body_json = ? WHERE ctx_id = ?")
            .bind(r#"{"not":"a body"}"#)
            .bind(&v1_ctx)
            .execute(store.pool())
            .await
            .unwrap();

        let fired = Arc::new(AtomicBool::new(false));
        let fired_c = fired.clone();
        let admit_all = move |_: &acdp::types::body::Body| {
            fired_c.store(true, Ordering::SeqCst);
            Ok(())
        };
        let s = store.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v2,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: Some(&admit_all),
            })
        })
        .await
        .unwrap();

        // Pin the DECODE specifically: `is_err()` alone is also satisfied by the
        // ownership / lineage / version / AlreadySuperseded gates, all of which
        // run before the hook, so it would not prove the decode is what failed.
        match &outcome {
            Err(AcdpError::RegistryInternal(msg)) => assert!(
                msg.contains("decode body"),
                "the failure must be the predecessor-body decode, got RegistryInternal({msg})"
            ),
            other => panic!(
                "an undecodable predecessor body must fail the publish with the decode \
                 error rather than silently skipping the RFC-ACDP-0014 §4 admission \
                 check, got {other:?}"
            ),
        }
        assert!(
            !fired.load(Ordering::SeqCst),
            "the hook cannot have run: there was no decodable body to hand it"
        );
    }

    /// Publish a v1 for the admission tests and return `(ctx_id, body)`.
    async fn publish_v1(
        store: &std::sync::Arc<crate::SqliteStore>,
        p: &acdp::producer::Producer,
        title: &str,
    ) -> (String, acdp::types::body::Body) {
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{ContextType, CtxId, Visibility};

        let v1 = p
            .publish_request()
            .title(title)
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let s = store.clone();
        let resp = tokio::task::spawn_blocking(move || {
            s.commit_publish(PublishCommit {
                req: &v1,
                authority: "reg.test",
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        let ctx = match resp {
            PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
                r.ctx_id.as_str().to_string()
            }
        };
        let body = store.get(&CtxId(ctx.clone())).unwrap().unwrap().body;
        (ctx, body)
    }

    /// Receipts persist atomically with the row and round-trip on every
    /// read path: the publish response, `get`, and `lineage`. Receipt
    /// fields minted from the assigned body equal the row's identifiers
    /// byte-for-byte, and every version in a receipts-era lineage carries
    /// exactly one receipt.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receipt_round_trips_through_publish_get_and_lineage() {
        use crate::SqliteStore;
        use acdp::crypto::SigningKey;
        use acdp::producer::Producer;
        use acdp::registry::store::{PublishCommit, PublishCommitOutcome, RegistryStore};
        use acdp::types::primitives::{AgentDid, ContextType, Visibility};
        use acdp_registry_store::ExtendedRegistryStore;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let store = SqliteStore::connect(tmp.path(), 2).await.unwrap();
        store.migrate().await.unwrap();

        let p = Producer::new(
            SigningKey::from_bytes(&[22u8; 32]),
            AgentDid::new("did:web:agents.test:rcpt".to_string()),
            "did:web:agents.test:rcpt#key-1".to_string(),
        );
        // Minter mirroring the shape acdp's ReceiptSigner emits, built
        // from the body the store hands back — so equality below proves
        // the store invoked it against the FINAL assigned identifiers.
        let minter = |body: &acdp::types::body::Body| {
            Ok(serde_json::json!({
                "ctx_id": body.ctx_id.as_str(),
                "lineage_id": body.lineage_id.as_str(),
                "origin_registry": body.origin_registry,
                "content_hash": body.content_hash.0,
            }))
        };

        let publish = |req: acdp::types::publish::PublishRequest| {
            let s = store.clone();
            tokio::task::spawn_blocking(move || {
                s.commit_publish(PublishCommit {
                    req: &req,
                    authority: "reg.test",
                    idempotency: None,
                    tenant: None,
                    receipt_minter: Some(&minter),
                    predecessor_admission: None,
                })
            })
        };

        let v1 = p
            .publish_request()
            .title("rcpt-v1")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        let r1 = match publish(v1).await.unwrap().unwrap() {
            PublishCommitOutcome::Inserted(r) => r,
            other => panic!("expected Inserted, got {other:?}"),
        };
        let receipt = r1
            .registry_receipt
            .clone()
            .expect("publish response carries the minted receipt");
        assert_eq!(receipt["ctx_id"], r1.ctx_id.as_str());
        assert_eq!(receipt["lineage_id"], r1.lineage_id.as_str());

        // get() surfaces the same stored receipt.
        let s = store.clone();
        let ctx_id = r1.ctx_id.clone();
        let fetched = tokio::task::spawn_blocking(move || s.get(&ctx_id))
            .await
            .unwrap()
            .unwrap()
            .expect("context exists");
        assert_eq!(fetched.registry_receipt, Some(receipt));

        // v2 supersession also mints; the whole lineage is receipt-complete.
        let v2 = p
            .supersede(r1.ctx_id.clone())
            .version(2)
            .title("rcpt-v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .unwrap();
        publish(v2).await.unwrap().unwrap();

        let s = store.clone();
        let lid = r1.lineage_id.clone();
        let chain = tokio::task::spawn_blocking(move || s.lineage(&lid))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chain.len(), 2);
        for ctx in &chain {
            let rcpt = ctx
                .registry_receipt
                .as_ref()
                .expect("every receipts-era context carries exactly one receipt");
            assert_eq!(rcpt["ctx_id"], ctx.body.ctx_id.as_str());
            assert_eq!(rcpt["lineage_id"], ctx.body.lineage_id.as_str());
        }
    }
}
