//! Extension trait layered on top of [`acdp::registry::RegistryStore`].
//!
//! Adds operations the HTTP layer needs but that aren't part of the
//! protocol-level store contract — paginated listing, a backend health
//! check, and migration runner.

pub mod cursor;
pub mod fulltext;
pub mod lifecycle;
pub mod log;

/// Cross-backend parity assertions shared by every backend's test suite.
/// Gated so nothing here ships in a normal build; see the module docs for
/// why the assertions live here instead of being duplicated per backend.
#[cfg(feature = "test-support")]
pub mod parity;

use acdp::error::AcdpError;
use acdp::registry::RegistryStore;
use acdp::types::body::{Body, FullContext};
use acdp::types::lifecycle::LifecycleEvent;
use acdp::types::primitives::{AgentDid, CtxId, Visibility};
use acdp::types::search::{SearchParams, SearchResponse};
use acdp_registry_types::config::RESERVED_TENANT;
use async_trait::async_trait;

pub use cursor::{decode_cursor, encode_cursor};
pub use log::{build_leaf_record, LogEntryRecord};

/// Cursor-keyed page returned by [`ExtendedRegistryStore::list_contexts`].
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// Extension trait for the registry HTTP layer.
///
/// Methods are `async` because real backends (Postgres, SQLite) sit
/// behind `sqlx` and need awaiting. The synchronous `RegistryStore`
/// methods inherited from `acdp` are run via `spawn_blocking` inside
/// implementations.
#[async_trait]
pub trait ExtendedRegistryStore: RegistryStore + Send + Sync {
    /// Paginated listing for admin / debug. Visibility follows the same
    /// RFC-ACDP-0008 §4.5 retrieval-style predicate `retrieve` and `search`
    /// already enforce: `restricted`/`private` bodies require `requester` to
    /// be the producer or a named audience member. The `public` arm is
    /// gated by `public_arm_open`, mirroring `RegistryStore::search`'s
    /// third parameter of the same name — when `requester` is `None` and
    /// `public_arm_open` is `false`, the page is empty; when `true`,
    /// public bodies are returned exactly as before. A non-`None` requester's
    /// results are unaffected by this flag either way.
    ///
    /// `GET /admin/contexts` requires an `auth.admin_tokens` bearer; the
    /// admin caller is authenticated but unnamed (`requester = None`). It
    /// reaches the §4.5 **public arm only** because `admin_list` passes
    /// `public_arm_open = true` unconditionally — the caller's local is
    /// spelled `admin_sees_public_arm`
    /// (`crates/acdp-registry-core/src/handlers/admin.rs:87`), not because a
    /// `None` requester alone implies that arm — the restricted and private
    /// arms remain unreachable to it regardless. The tenant filter below
    /// applies when a tenant is asserted; otherwise the listing spans
    /// tenants.
    ///
    /// `tenant` (plan §7): when `Some`, the backend MUST filter rows
    /// at the storage layer so the returned page contains only that
    /// tenant's contexts. This eliminates the "short pages" wart of
    /// the prior post-query filter — a caller asking for `?limit=20`
    /// against a mixed-tenant registry now receives `min(20, available)`
    /// rows for their tenant, not `min(20, available_across_all_tenants)`
    /// reduced by an in-Rust retain.
    ///
    /// `None` preserves V0 behavior (no tenant filter). Implementations
    /// SHOULD pair this with a composite `(tenant_id, created_at)`
    /// index so the WHERE clause stays selective on busy registries —
    /// `idx_ctx_tenant_created` from migration 006/007 (PG/SQLite) is
    /// already in place.
    async fn list_contexts(
        &self,
        limit: u32,
        cursor: Option<&str>,
        requester: Option<&AgentDid>,
        tenant: Option<&str>,
        public_arm_open: bool,
    ) -> Result<Page<FullContext>, AcdpError>;

    /// Storage backend health check. `Ok(())` on success.
    async fn health(&self) -> Result<(), AcdpError>;

    /// Count of currently-stored idempotency records (including not-yet-evicted
    /// expired ones). Surfaced by the admin status endpoint for operational
    /// visibility. The default returns `None` ("not tracked") so backends
    /// without an idempotency table stay compatible; SQL backends override.
    async fn count_idempotency_records(&self) -> Result<Option<u64>, AcdpError> {
        Ok(None)
    }

    /// Run pending migrations. Called at server startup.
    async fn migrate(&self) -> Result<(), AcdpError>;

    /// Tenant-aware lookup: returns the `tenant_id` recorded for a
    /// ctx_id, or `None` if the row doesn't exist. The default
    /// implementation returns `Some("default")` so backends that
    /// haven't migrated to tenant tagging yet still satisfy the
    /// trait without claiming a wrong answer. Production backends
    /// override.
    async fn tenant_of_ctx(&self, _ctx_id: &str) -> Result<Option<String>, AcdpError> {
        Ok(Some("default".into()))
    }

    /// Stamp the tenant_id for a ctx_id. Called by the publish handler
    /// AFTER the protocol-level publish succeeds (the protocol
    /// `RegistryStore::upsert_context` doesn't carry tenancy). When the
    /// requested tenant is `"default"`, this is a no-op since the
    /// migration's default is already `'default'`. Returns Ok on
    /// success; the default impl returns Ok so untenanted backends
    /// stay compatible.
    async fn set_tenant_of_ctx(&self, _ctx_id: &str, _tenant_id: &str) -> Result<(), AcdpError> {
        Ok(())
    }

    /// A context's lifecycle events (RFC-ACDP-0013 §4.1), in registry
    /// acceptance order — the exact array served as
    /// `registry_state.lifecycle_events`. Empty when the context has no
    /// events (or does not exist; callers resolve existence separately).
    ///
    /// The default reads through the protocol-level
    /// [`RegistryStore::get`] projection so backends that already
    /// populate `registry_state.lifecycle_events` (the SQL stores, the
    /// SDK `InMemoryStore`) need no override; a backend with a cheaper
    /// direct query may override.
    async fn lifecycle_events_of_ctx(
        &self,
        ctx_id: &str,
    ) -> Result<Vec<LifecycleEvent>, AcdpError> {
        let ctx = self.get(&acdp::types::primitives::CtxId(ctx_id.to_string()))?;
        Ok(ctx
            .and_then(|c| c.registry_state.lifecycle_events)
            .unwrap_or_default())
    }

    // ── Transparency log reads (ACDP 0.3, RFC-ACDP-0012) ──────────────
    //
    // Leaves are APPENDED only inside `RegistryStore::commit_publish`
    // (same transaction as the context row + receipt, §7.1 — there is
    // deliberately no standalone append API). These are the read
    // projections the `/log/*` endpoints need. Defaults return
    // `NotImplemented` so backends without a `log_leaves` table stay
    // compatible; a deployment MUST NOT enable `[log]` against such a
    // backend (startup validation enforces this for the memory backend).

    /// Current tree size — the number of appended leaves (§5.2).
    async fn log_tree_size(&self) -> Result<u64, AcdpError> {
        Err(AcdpError::NotImplemented(
            "this backend does not implement the transparency log (RFC-ACDP-0012)".into(),
        ))
    }

    /// The first `up_to` §5.1 leaf hashes as raw 32-byte digests, in
    /// leaf-index order — the sole input to every root / inclusion-path /
    /// consistency-path computation (§5.2, §8.3). Errors if the stored
    /// log is not dense over `[0, up_to)`.
    async fn log_leaf_hashes(&self, up_to: u64) -> Result<Vec<[u8; 32]>, AcdpError> {
        let _ = up_to;
        Err(AcdpError::NotImplemented(
            "this backend does not implement the transparency log (RFC-ACDP-0012)".into(),
        ))
    }

    /// The leaf for `ctx_id`, if logged (§8.2 inclusion mode — the
    /// consumer surface). Callers apply retrieval visibility (§8.2 /
    /// RFC-ACDP-0008 §4.5) BEFORE disclosing anything about the result.
    async fn log_leaf_by_ctx(&self, ctx_id: &str) -> Result<Option<LogEntryRecord>, AcdpError> {
        let _ = ctx_id;
        Err(AcdpError::NotImplemented(
            "this backend does not implement the transparency log (RFC-ACDP-0012)".into(),
        ))
    }

    /// The leaf at `leaf_index`, if present (§8.2 inclusion mode — the
    /// auditor surface; hash-only data needs no visibility gate, but the
    /// `leaf` echo does).
    async fn log_leaf_by_index(
        &self,
        leaf_index: u64,
    ) -> Result<Option<LogEntryRecord>, AcdpError> {
        let _ = leaf_index;
        Err(AcdpError::NotImplemented(
            "this backend does not implement the transparency log (RFC-ACDP-0012)".into(),
        ))
    }

    /// Leaves `[start, end)` in leaf-index order (§8.3).
    async fn log_entries(&self, start: u64, end: u64) -> Result<Vec<LogEntryRecord>, AcdpError> {
        let _ = (start, end);
        Err(AcdpError::NotImplemented(
            "this backend does not implement the transparency log (RFC-ACDP-0012)".into(),
        ))
    }

    // ── Witness cosignature aggregation (ACDP 0.4, RFC-ACDP-0015 §6.1) ─
    //
    // The registry MAY collect witness cosignatures of its checkpoints and
    // serve them alongside the checkpoint as the reserved
    // `witness_signatures` member. Only VERIFIED cosignatures are ever
    // stored: the aggregator (`acdp-registry-core::witness`) resolves the
    // witness DID and checks each cosignature's signature AND that its
    // `witnessed_checkpoint` matches this registry's own root at that
    // `tree_size` before calling `upsert_witness_cosignature`. The table
    // is keyed by `(log_id, tree_size, root_hash, witness_did)` so at most
    // one (freshest) cosignature per witness per exact checkpoint tuple is
    // retained; the checkpoint handler reads back by the exact tuple it is
    // serving, so a cosignature can never be mis-attached to a different
    // root.

    /// Insert (or refresh) one VERIFIED witness cosignature for the
    /// checkpoint tuple `(log_id, tree_size, root_hash)`. `cosignature_json`
    /// is the exact wire bytes of the §4 cosignature object (served back
    /// verbatim); `witnessed_at` is its canonical RFC 3339 UTC timestamp,
    /// stored for freshness/newest-wins on re-observation. Upserts on the
    /// `(log_id, tree_size, root_hash, witness_did)` key.
    ///
    /// The default errors: witness aggregation requires a durable backend
    /// with the `log_witness_cosignatures` table (SQLite / Postgres).
    async fn upsert_witness_cosignature(
        &self,
        log_id: &str,
        tree_size: u64,
        root_hash: &str,
        witness_did: &str,
        witnessed_at: &str,
        cosignature_json: &str,
    ) -> Result<(), AcdpError> {
        let _ = (
            log_id,
            tree_size,
            root_hash,
            witness_did,
            witnessed_at,
            cosignature_json,
        );
        Err(AcdpError::NotImplemented(
            "this backend does not implement witness cosignature aggregation (RFC-ACDP-0015)"
                .into(),
        ))
    }

    /// The verified cosignatures stored for the exact checkpoint tuple
    /// `(log_id, tree_size, root_hash)`, as raw §4 wire values, ordered by
    /// `witness_did` for a stable response. Empty when none — the default
    /// returns empty so a backend without the table simply serves a bare
    /// checkpoint (aggregation is optional; RFC-ACDP-0015 §6.1, §11).
    async fn witness_cosignatures_for(
        &self,
        log_id: &str,
        tree_size: u64,
        root_hash: &str,
    ) -> Result<Vec<serde_json::Value>, AcdpError> {
        let _ = (log_id, tree_size, root_hash);
        Ok(Vec::new())
    }

    /// Batch tenant lookup. Returns a map of `ctx_id → tenant_id`.
    /// Used by handlers that filter result sets (search / lineage /
    /// list) — one round-trip beats N. Default impl falls back to N
    /// single-row queries via [`Self::tenant_of_ctx`] so untenanted
    /// backends remain compatible.
    async fn tenants_of_ctxs(
        &self,
        ctx_ids: &[&str],
    ) -> Result<std::collections::HashMap<String, String>, AcdpError> {
        let mut out = std::collections::HashMap::with_capacity(ctx_ids.len());
        for id in ctx_ids {
            if let Some(t) = self.tenant_of_ctx(id).await? {
                out.insert((*id).to_string(), t);
            }
        }
        Ok(out)
    }

    /// Batched retrieval-visibility check. Returns the subset of `ctx_ids`
    /// the `requester` is allowed to **retrieve**, under the same rule
    /// `GET /contexts/{ctx_id}` applies: RFC-ACDP-0008 §4.5 visibility plus
    /// this registry's tenant gate.
    ///
    /// # Why this exists
    ///
    /// `GET /log/entries` gates every `ctx_id` it echoes on that rule, and it
    /// did so **one context at a time** — a blocking `RegistryServer::retrieve`
    /// per entry, up to a 256-entry page. One query answers the whole page.
    ///
    /// # It is `retrieve` semantics, NOT `search` semantics
    ///
    /// These genuinely differ, and the difference is a disclosure boundary in
    /// both directions. Under §4.5 an audience member **may** retrieve a
    /// `private` context; `search` deliberately requires *ownership* for
    /// `private` and excludes the audience. That asymmetry is specified, not
    /// accidental — conformance fixture `vis-004` pins it, including that a
    /// listed *contributor* who is not in the audience is refused, because
    /// contributors are not authorization. So this method must mirror the
    /// **list/retrieve**-shaped predicate, and an implementation that reuses a
    /// search predicate will under-disclose.
    ///
    /// Note also what is **absent** from the rule: status. A `retracted`
    /// context is still retrievable (its status is projected, not hidden), so
    /// implementations MUST NOT filter on status here.
    ///
    /// # The default implementation
    ///
    /// Behaviour-preserving, not fail-closed: it does exactly what the caller
    /// did before — one `RegistryStore::get` per id, then [`retrieve_visible`],
    /// then the tenant gate via [`Self::tenant_of_ctx`]. N round-trips, same
    /// answers. A fail-closed default was considered and rejected: it would
    /// make an untenanted backend disagree with both SQL backends about ids it
    /// can see perfectly well, which is a correctness defect wearing safety's
    /// clothing. SQL backends override with a single query.
    ///
    /// Because `RegistryStore::get` is synchronous, the default blocks; callers
    /// on an async runtime should treat it the way they already treat
    /// `retrieve` and hand it to `spawn_blocking`.
    async fn visible_ctx_ids(
        &self,
        ctx_ids: &[&str],
        requester: Option<&AgentDid>,
        tenant: Option<&str>,
        public_arm_open: bool,
    ) -> Result<std::collections::HashSet<String>, AcdpError> {
        let mut out = std::collections::HashSet::with_capacity(ctx_ids.len());
        for id in ctx_ids {
            // An unparseable ctx_id is not visible, matching the caller's own
            // `CtxId::parse` failure path rather than raising.
            let Ok(parsed) = CtxId::parse((*id).to_string()) else {
                continue;
            };
            let Some(ctx) = self.get(&parsed)? else {
                continue;
            };
            if !retrieve_visible(&ctx.body, requester, public_arm_open) {
                continue;
            }
            if let Some(want) = tenant {
                let stored = self
                    .tenant_of_ctx(id)
                    .await?
                    .unwrap_or_else(|| RESERVED_TENANT.to_string());
                if stored != want {
                    continue;
                }
            }
            out.insert((*id).to_string());
        }
        Ok(out)
    }

    /// Tenant-scoped search: like [`RegistryStore::search`], but the backend
    /// MUST apply the tenant predicate **in storage**, not to the result set.
    ///
    /// # Why this exists as a separate method
    ///
    /// [`RegistryStore::search`] is declared by the upstream `acdp` crate, so
    /// its signature cannot grow a `tenant` parameter here. This is the
    /// additive sibling, shaped after [`Self::list_contexts`]'s `tenant`
    /// parameter so the trait has one tenancy idiom rather than two.
    ///
    /// # What the storage-layer requirement buys
    ///
    /// Filtering after the query is not merely slower — it is **disclosing**.
    /// `acdp::pagination` anchors `next_cursor` on the last *raw scanned row*,
    /// deliberately, so that a page whose rows are all dropped by post-SQL
    /// filters does not halt pagination early. A tenant filter applied after
    /// the scan therefore leaves the cursor anchored on whatever row the scan
    /// last touched — possibly another tenant's — encoding its `created_at`
    /// and `ctx_id` into a token handed to the caller. That is an ordering and
    /// existence oracle over foreign rows, walkable one page at a time.
    ///
    /// Putting `tenant_id = ?` in the `WHERE` clause fixes that **by
    /// construction** rather than by adding a second check: every raw row the
    /// scan sees already belongs to the caller, so the anchor cannot be
    /// foreign, and `COUNT(*) OVER ()` on the same scan yields a
    /// tenant-correct `total_estimate` without a second query.
    ///
    /// Implementations SHOULD rely on the composite `(tenant_id, created_at)`
    /// index — `idx_ctx_tenant_created`, migration 006 (PG) / 007 (SQLite) —
    /// which already covers the `created_at DESC, ctx_id ASC` order this query
    /// uses.
    ///
    /// # The contract, stated once
    ///
    /// `Some(t)` means **exactly** "the rows whose tenant is `t`" — never more.
    /// `None` means no narrowing, spanning tenants, matching
    /// [`Self::list_contexts`].
    ///
    /// # The default implementation
    ///
    /// The default treats the backend as **untenanted**, consistent with
    /// [`Self::tenant_of_ctx`]'s default reporting every row as
    /// [`RESERVED_TENANT`]. Under that model these are the correct answers, not
    /// degraded ones:
    ///
    /// - `None` — no narrowing requested.
    /// - `Some(RESERVED_TENANT)` — every row on such a backend *is* in that
    ///   tenant, so "the rows whose tenant is `default`" is the whole table.
    ///   Delegating satisfies the contract above rather than sidestepping it;
    ///   the SQL backends reach the same answer the other way, via
    ///   `WHERE tenant_id = 'default'`, which on a tenanted backend selects
    ///   only the untenanted bucket.
    /// - `Some(other)` — no row can belong to a tenant this backend never
    ///   assigns, so the answer is the empty page: `total_estimate` of `0`, no
    ///   cursor.
    ///
    /// A backend that DOES record tenants MUST override this. Handing back
    /// unfiltered rows for a foreign tenant would be a silent cross-tenant
    /// disclosure, which is exactly why the default does not delegate
    /// unconditionally: the one shape that must never be reachable by accident
    /// is a caller asking for tenant B and receiving tenant A's rows.
    ///
    /// # What this deliberately does NOT do
    ///
    /// It does not reject `Some(RESERVED_TENANT)` as an illegitimate assertion.
    /// That rule is real — `RESERVED_TENANT`'s own docs call it MUST NOT — but
    /// it is an **authorization** decision and it already has exactly one
    /// enforcement point: `reject_reserved_tenant`
    /// (`crates/acdp-registry-core/src/handlers/context.rs:189`), which refuses
    /// it from header or token so untenanted rows stay reachable only through
    /// the *absence* of an assertion. Re-deciding it here would put an auth
    /// judgement in the storage layer and would make `search_in_tenant`
    /// inconsistent with `list_contexts`, which applies the predicate for any
    /// `Some`. One rule, one place.
    async fn search_in_tenant(
        &self,
        params: &SearchParams,
        requester: Option<&AgentDid>,
        public_arm_open: bool,
        tenant: Option<&str>,
    ) -> Result<SearchResponse, AcdpError> {
        match tenant {
            None | Some(RESERVED_TENANT) => self.search(params, requester, public_arm_open),
            Some(_) => Ok(SearchResponse {
                matches: Vec::new(),
                total_estimate: Some(0),
                next_cursor: None,
            }),
        }
    }
}

/// RFC-ACDP-0008 §4.5 retrieval disclosure rule, in Rust.
///
/// | visibility   | may retrieve                                        |
/// |--------------|-----------------------------------------------------|
/// | `public`     | anyone, when `public_arm_open`; else any DID  |
/// | `restricted` | the producer (`agent_id`) **or** a DID in `audience` |
/// | `private`    | the producer (`agent_id`) **or** a DID in `audience` |
///
/// `private` and `restricted` are deliberately the same row: the audience is
/// authorization for both. `contributors` appears nowhere — it is provenance,
/// not access.
///
/// # Why this is written out here
///
/// The authoritative copy is `can_retrieve` in the upstream `acdp` crate, which
/// is `pub(crate)` there and so cannot be called from this one. This is
/// therefore a deliberate re-expression of someone else's rule, which is exactly
/// the kind of duplication that drifts silently. Two things guard it: the SQL
/// backends express the same rule as a predicate rather than calling this, and
/// the parity suite asserts all three agree on one fixture. If this function
/// and upstream `can_retrieve` ever disagree, that suite is what says so.
pub fn retrieve_visible(body: &Body, requester: Option<&AgentDid>, public_arm_open: bool) -> bool {
    match body.visibility {
        Visibility::Public => public_arm_open || requester.is_some(),
        Visibility::Restricted | Visibility::Private => match requester {
            None => false,
            Some(r) => {
                r == &body.agent_id
                    || body
                        .audience
                        .as_deref()
                        .is_some_and(|a| a.iter().any(|d| d == r))
            }
        },
    }
}

#[cfg(test)]
mod default_search_in_tenant_tests {
    use super::*;
    use acdp::registry::store::{PublishCommit, PublishCommitOutcome};
    use acdp::registry::{IdempotencyRecord, LifecycleCommitOutcome};
    use acdp::types::body::Body;
    use acdp::types::primitives::{ContentHash, ContextType, CtxId, LineageId, Status, Visibility};
    use acdp::types::publish::PublishResponse;
    use acdp::types::search::SearchResult;
    use chrono::{DateTime, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Values chosen to be impossible to produce by accident, so a passing
    /// assertion can only mean "the delegate branch ran".
    const SENTINEL_TOTAL: u64 = 4242;
    const SENTINEL_CURSOR: &str = "SENTINEL-DELEGATED-TO-SEARCH";
    const SENTINEL_CTX: &str = "acdp://sentinel.invalid/00000000-0000-0000-0000-00000000beef";

    /// One recognizable row, so that "did a foreign-tenant request return rows?"
    /// is a question the sentinel can actually answer. With an empty `matches`
    /// the emptiness assertion was structurally unfalsifiable — no mutation of
    /// the default could ever make it fail — which is an assertion masquerading
    /// as a guard (CHARTER rule 51).
    fn sentinel_row() -> SearchResult {
        SearchResult {
            ctx_id: CtxId(SENTINEL_CTX.to_string()),
            lineage_id: LineageId("sentinel-lineage".to_string()),
            agent_id: AgentDid::new("did:web:sentinel.invalid".to_string()),
            title: "sentinel".to_string(),
            summary: None,
            context_type: ContextType::DataSnapshot,
            domain: None,
            created_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"),
            status: Status::Active,
            visibility: Some(Visibility::Public),
        }
    }

    /// A backend that records tenants for nothing and overrides no
    /// `ExtendedRegistryStore` method — the shape `search_in_tenant`'s default
    /// exists to serve. `search` returns a sentinel and counts its own calls,
    /// so each branch is identified by *which response came back* and *whether
    /// the underlying store was touched at all* — not by a row count, which a
    /// wrong-but-empty implementation would also satisfy.
    struct UntenantedBackend {
        search_calls: AtomicUsize,
    }

    impl UntenantedBackend {
        fn new() -> Self {
            Self {
                search_calls: AtomicUsize::new(0),
            }
        }
        fn calls(&self) -> usize {
            self.search_calls.load(Ordering::SeqCst)
        }
    }

    impl RegistryStore for UntenantedBackend {
        fn search(
            &self,
            _params: &SearchParams,
            _requester: Option<&AgentDid>,
            _public_arm_open: bool,
        ) -> Result<SearchResponse, AcdpError> {
            self.search_calls.fetch_add(1, Ordering::SeqCst);
            Ok(SearchResponse {
                matches: vec![sentinel_row()],
                total_estimate: Some(SENTINEL_TOTAL),
                next_cursor: Some(SENTINEL_CURSOR.to_string()),
            })
        }

        // Everything below is outside what this test exercises. `unimplemented!`
        // rather than a plausible stub on purpose: if the default impl ever
        // reaches one of these, the test must fail loudly rather than quietly
        // succeed against a fake.
        fn put(&self, _body: Body) -> Result<(), AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn get(&self, _ctx_id: &CtxId) -> Result<Option<FullContext>, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn lineage(&self, _lineage_id: &LineageId) -> Result<Vec<FullContext>, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn current(&self, _lineage_id: &LineageId) -> Result<Option<FullContext>, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn mark_superseded(&self, _ctx_id: &CtxId) -> Result<(), AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn first_version_ctx_id(
            &self,
            _lineage_id: &LineageId,
        ) -> Result<Option<CtxId>, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn idempotency_lookup(
            &self,
            _agent_id: &AgentDid,
            _key: &str,
        ) -> Result<Option<IdempotencyRecord>, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn idempotency_record(
            &self,
            _agent_id: &AgentDid,
            _key: &str,
            _hash: &ContentHash,
            _response: &PublishResponse,
            _expires_at: DateTime<Utc>,
        ) -> Result<(), AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn idempotency_evict_expired(&self, _now: DateTime<Utc>) -> Result<(), AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn commit_publish(
            &self,
            _commit: PublishCommit<'_>,
        ) -> Result<PublishCommitOutcome, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        fn commit_lifecycle_event(
            &self,
            _event: &LifecycleEvent,
        ) -> Result<LifecycleCommitOutcome, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
    }

    #[async_trait]
    impl ExtendedRegistryStore for UntenantedBackend {
        async fn health(&self) -> Result<(), AcdpError> {
            Ok(())
        }
        async fn migrate(&self) -> Result<(), AcdpError> {
            Ok(())
        }
        async fn list_contexts(
            &self,
            _limit: u32,
            _cursor: Option<&str>,
            _requester: Option<&AgentDid>,
            _tenant: Option<&str>,
            _public_arm_open: bool,
        ) -> Result<Page<FullContext>, AcdpError> {
            unimplemented!("not reached by search_in_tenant's default")
        }
        // `search_in_tenant` deliberately NOT overridden — the default is the
        // subject under test.
    }

    #[tokio::test]
    async fn no_tenant_asserted_delegates_to_search() {
        let s = UntenantedBackend::new();
        let r = s
            .search_in_tenant(&SearchParams::default(), None, true, None)
            .await
            .expect("search_in_tenant ok");
        assert_eq!(
            r.total_estimate,
            Some(SENTINEL_TOTAL),
            "`None` must delegate to RegistryStore::search — the sentinel total is how we know \
             the delegate branch ran rather than a fabricated empty page"
        );
        assert_eq!(r.next_cursor.as_deref(), Some(SENTINEL_CURSOR));
        assert_eq!(s.calls(), 1, "search must have been called exactly once");
    }

    #[tokio::test]
    async fn the_reserved_tenant_delegates_because_every_row_is_in_it() {
        let s = UntenantedBackend::new();
        let r = s
            .search_in_tenant(&SearchParams::default(), None, true, Some(RESERVED_TENANT))
            .await
            .expect("search_in_tenant ok");
        assert_eq!(
            r.total_estimate,
            Some(SENTINEL_TOTAL),
            "on an untenanted backend every row IS `{RESERVED_TENANT}`, so \"the rows whose \
             tenant is default\" is the whole table"
        );
        assert_eq!(s.calls(), 1);
    }

    /// Helper: the default's answer for a tenant this backend cannot hold.
    async fn foreign_tenant_response() -> (UntenantedBackend, SearchResponse) {
        let s = UntenantedBackend::new();
        let r = s
            .search_in_tenant(
                &SearchParams::default(),
                None,
                true,
                Some("tenant-that-this-backend-never-assigns"),
            )
            .await
            .expect("search_in_tenant ok");
        (s, r)
    }

    // One guarantee per test, deliberately. These four were originally four
    // asserts in ONE test, which meant a single mutation reported one verdict for
    // four promises and the three after the first were never evaluated (CHARTER
    // rule 51). Split, a single mutation reddens all four independently, so each
    // is demonstrably a guard rather than decoration.
    //
    // Rule 52: the line under test is `search_in_tenant`'s DEFAULT body, and it
    // demonstrably executes here — `UntenantedBackend` overrides the method
    // nowhere, and mutating that default reddens every test below. The overriding
    // path is covered separately, by the SQL backends' parity suite.

    /// The strongest claim: a tenant the backend cannot hold must not even reach
    /// the store. A query whose rows are discarded afterwards still leaks a cursor.
    #[tokio::test]
    async fn a_foreign_tenant_never_consults_the_store() {
        let (s, _r) = foreign_tenant_response().await;
        assert_eq!(
            s.calls(),
            0,
            "the store must never be consulted for a tenant it cannot hold — a call here means \
             the implementation is post-filtering, which is the defect, not the fix"
        );
    }

    #[tokio::test]
    async fn a_foreign_tenant_returns_no_rows() {
        let (_s, r) = foreign_tenant_response().await;
        let ids: Vec<&str> = r.matches.iter().map(|m| m.ctx_id.as_str()).collect();
        assert!(
            r.matches.is_empty(),
            "a foreign tenant must match no rows, got {ids:?} — the sentinel ctx_id appearing \
             here is the page-level leak"
        );
    }

    #[tokio::test]
    async fn a_foreign_tenant_reports_a_zero_total() {
        let (_s, r) = foreign_tenant_response().await;
        assert_eq!(
            r.total_estimate,
            Some(0),
            "total_estimate must be 0, not the sentinel {SENTINEL_TOTAL} — a non-zero total is \
             itself a count oracle over another tenant's rows"
        );
    }

    #[tokio::test]
    async fn a_foreign_tenant_receives_no_cursor() {
        let (_s, r) = foreign_tenant_response().await;
        assert_eq!(
            r.next_cursor.as_deref(),
            None,
            "next_cursor must be absent; the sentinel cursor leaking here is exactly the \
             cross-tenant anchor disclosure this method exists to prevent"
        );
    }
}

#[cfg(test)]
mod default_visible_ctx_ids_tests {
    use super::*;
    use acdp::crypto::SigningKey;
    use acdp::producer::Producer;
    use acdp::registry::store::{PublishCommit, PublishCommitOutcome};
    use acdp::registry::{IdempotencyRecord, LifecycleCommitOutcome};
    use acdp::types::body::RegistryState;
    use acdp::types::primitives::{ContentHash, ContextType, LineageId, Status};
    use acdp::types::publish::PublishResponse;
    use chrono::{DateTime, Utc};
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PRODUCER: &str = "did:web:reg.test:producer";
    const AUDIENCE: &str = "did:web:reg.test:audience-member";
    const CONTRIB: &str = "did:web:reg.test:contributor";
    const OUTSIDER: &str = "did:web:reg.test:outsider";

    // Fixture ids. Named for what each one is *for*, so a failure message
    // points at the guarantee rather than at a UUID.
    const PUBLIC: &str = "acdp://reg.test/00000000-0000-4000-8000-00000000a001";
    const PRIVATE_WITH_AUDIENCE: &str = "acdp://reg.test/00000000-0000-4000-8000-00000000a002";
    const PRIVATE_OWNER_ONLY: &str = "acdp://reg.test/00000000-0000-4000-8000-00000000a003";
    const RESTRICTED_WITH_AUDIENCE: &str = "acdp://reg.test/00000000-0000-4000-8000-00000000a004";
    const PRIVATE_WITH_CONTRIBUTOR: &str = "acdp://reg.test/00000000-0000-4000-8000-00000000a005";
    const PUBLIC_RETRACTED: &str = "acdp://reg.test/00000000-0000-4000-8000-00000000a006";
    const NEVER_STORED: &str = "acdp://reg.test/00000000-0000-4000-8000-0000000000ff";
    const NOT_A_CTX_ID: &str = "this is not a ctx_id at all";

    fn did(s: &str) -> AgentDid {
        AgentDid::new(s.to_string())
    }

    /// `RegistryState::is_retracted()` is derived from `lifecycle_events`, NOT
    /// from `status`. A fixture that sets only `status` is therefore not
    /// retracted by the definition half the codebase uses — so the retracted
    /// fixture below carries both, and a status filter written either way is
    /// caught.
    fn retracted_event(ctx_id: &str) -> LifecycleEvent {
        serde_json::from_value(serde_json::json!({
            "event_id": "01J0000000000000000000000A",
            "ctx_id": ctx_id,
            "event_type": "retracted",
            "occurred_at": "2026-01-01T00:00:00.000Z",
            "actor": "did:web:agents.test:actor",
        }))
        .expect("valid lifecycle event")
    }

    /// One stored context. `audience` / `contributors` are passed separately
    /// because the whole point of the contributor fixture is that the two
    /// fields are NOT interchangeable.
    fn ctx(
        ctx_id: &str,
        visibility: Visibility,
        audience: Vec<AgentDid>,
        contributors: Vec<AgentDid>,
        status: Status,
    ) -> FullContext {
        let p = Producer::new(
            SigningKey::from_bytes(&[11u8; 32]),
            did(PRODUCER),
            format!("{PRODUCER}#key-1"),
        );
        let mut b = p
            .publish_request()
            .title("visibility-fixture")
            .context_type(ContextType::DataSnapshot)
            .visibility(visibility);
        if !audience.is_empty() {
            b = b.audience(audience);
        }
        if !contributors.is_empty() {
            b = b.contributors(contributors);
        }
        let req = b.build().expect("valid publish request");
        let id = CtxId(ctx_id.to_string());
        let lineage_id = acdp::crypto::derive_lineage_id(&id);
        let body = Body::from_publish_request(
            &req,
            id,
            lineage_id,
            "reg.test".to_string(),
            acdp::time::trunc_ms(Utc::now()),
        );
        let lifecycle_events = if status == Status::Retracted {
            Some(vec![retracted_event(ctx_id)])
        } else {
            None
        };
        FullContext {
            body,
            registry_state: RegistryState {
                status,
                lifecycle_events,
                extensions: serde_json::Map::new(),
            },
            registry_receipt: None,
            lineage_head_receipt: None,
            log_inclusion: None,
            extensions: serde_json::Map::new(),
        }
    }

    /// A backend that stores contexts and overrides **no**
    /// `ExtendedRegistryStore` method — so `visible_ctx_ids`' default body is
    /// what every test below executes (CHARTER rule 52). It also counts `get`
    /// calls, which is how the N-round-trip shape of the default is asserted
    /// rather than assumed.
    struct FixtureBackend {
        by_id: HashMap<String, FullContext>,
        get_calls: AtomicUsize,
    }

    impl FixtureBackend {
        fn new() -> Self {
            let mut by_id = HashMap::new();
            for c in [
                ctx(PUBLIC, Visibility::Public, vec![], vec![], Status::Active),
                ctx(
                    PRIVATE_WITH_AUDIENCE,
                    Visibility::Private,
                    vec![did(AUDIENCE)],
                    vec![],
                    Status::Active,
                ),
                ctx(
                    PRIVATE_OWNER_ONLY,
                    Visibility::Private,
                    vec![],
                    vec![],
                    Status::Active,
                ),
                ctx(
                    RESTRICTED_WITH_AUDIENCE,
                    Visibility::Restricted,
                    vec![did(AUDIENCE)],
                    vec![],
                    Status::Active,
                ),
                // A contributor and NO audience: the fixture that proves
                // contributors are provenance, not authorization.
                ctx(
                    PRIVATE_WITH_CONTRIBUTOR,
                    Visibility::Private,
                    vec![],
                    vec![did(CONTRIB)],
                    Status::Active,
                ),
                ctx(
                    PUBLIC_RETRACTED,
                    Visibility::Public,
                    vec![],
                    vec![],
                    Status::Retracted,
                ),
            ] {
                by_id.insert(c.body.ctx_id.as_str().to_string(), c);
            }
            Self {
                by_id,
                get_calls: AtomicUsize::new(0),
            }
        }

        fn gets(&self) -> usize {
            self.get_calls.load(Ordering::SeqCst)
        }

        /// Every id this fixture knows about, plus the two it deliberately
        /// does not. Passing the unknown ids on every call means "absent ids
        /// are skipped" is exercised by every test, not just its own.
        fn all_ids() -> Vec<&'static str> {
            vec![
                PUBLIC,
                PRIVATE_WITH_AUDIENCE,
                PRIVATE_OWNER_ONLY,
                RESTRICTED_WITH_AUDIENCE,
                PRIVATE_WITH_CONTRIBUTOR,
                PUBLIC_RETRACTED,
                NEVER_STORED,
                NOT_A_CTX_ID,
            ]
        }

        async fn visible_to(&self, requester: Option<&str>, anon: bool) -> HashSet<String> {
            let r = requester.map(did);
            self.visible_ctx_ids(&Self::all_ids(), r.as_ref(), None, anon)
                .await
                .expect("visible_ctx_ids ok")
        }
    }

    impl RegistryStore for FixtureBackend {
        fn get(&self, ctx_id: &CtxId) -> Result<Option<FullContext>, AcdpError> {
            self.get_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.by_id.get(ctx_id.as_str()).cloned())
        }

        // `unimplemented!` rather than a plausible stub: if the default ever
        // reaches one of these, the test must fail loudly rather than quietly
        // pass against a fake.
        fn search(
            &self,
            _params: &SearchParams,
            _requester: Option<&AgentDid>,
            _public_arm_open: bool,
        ) -> Result<SearchResponse, AcdpError> {
            unimplemented!("visible_ctx_ids must not reach search — that is the other predicate")
        }
        fn put(&self, _body: Body) -> Result<(), AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn lineage(&self, _lineage_id: &LineageId) -> Result<Vec<FullContext>, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn current(&self, _lineage_id: &LineageId) -> Result<Option<FullContext>, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn mark_superseded(&self, _ctx_id: &CtxId) -> Result<(), AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn first_version_ctx_id(
            &self,
            _lineage_id: &LineageId,
        ) -> Result<Option<CtxId>, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn idempotency_lookup(
            &self,
            _agent_id: &AgentDid,
            _key: &str,
        ) -> Result<Option<IdempotencyRecord>, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn idempotency_record(
            &self,
            _agent_id: &AgentDid,
            _key: &str,
            _hash: &ContentHash,
            _response: &PublishResponse,
            _expires_at: DateTime<Utc>,
        ) -> Result<(), AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn idempotency_evict_expired(&self, _now: DateTime<Utc>) -> Result<(), AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn commit_publish(
            &self,
            _commit: PublishCommit<'_>,
        ) -> Result<PublishCommitOutcome, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        fn commit_lifecycle_event(
            &self,
            _event: &LifecycleEvent,
        ) -> Result<LifecycleCommitOutcome, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
    }

    #[async_trait]
    impl ExtendedRegistryStore for FixtureBackend {
        async fn health(&self) -> Result<(), AcdpError> {
            Ok(())
        }
        async fn migrate(&self) -> Result<(), AcdpError> {
            Ok(())
        }
        async fn list_contexts(
            &self,
            _limit: u32,
            _cursor: Option<&str>,
            _requester: Option<&AgentDid>,
            _tenant: Option<&str>,
            _public_arm_open: bool,
        ) -> Result<Page<FullContext>, AcdpError> {
            unimplemented!("not reached by visible_ctx_ids' default")
        }
        // `visible_ctx_ids` and `tenant_of_ctx` deliberately NOT overridden —
        // the defaults are the subject under test.
    }

    // ---- one guarantee per test (CHARTER rule 51) ----------------------------
    //
    // These are deliberately not four asserts in one test: `assert!` aborts at
    // the first failure, so N guarantees behind N asserts yields one verdict for
    // N promises and never evaluates the rest.

    /// THE load-bearing guarantee of the whole unit. Under §4.5 an audience
    /// member MAY retrieve a `private` context — `search` refuses this exact
    /// case, so an implementation built on a search predicate fails here and
    /// silently under-discloses. Conformance fixture `vis-004` pins it.
    #[tokio::test]
    async fn an_audience_member_may_retrieve_a_private_context() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(AUDIENCE), false).await;
        assert!(
            v.contains(PRIVATE_WITH_AUDIENCE),
            "an audience member MUST be able to retrieve a private context (RFC-ACDP-0008 §4.5, \
             conformance vis-004). This is precisely where `search` semantics differ: search \
             requires ownership for `private`. Missing it means the batch was built on the wrong \
             predicate and under-discloses. Visible set was {v:?}"
        );
    }

    /// The other half of vis-004: `contributors` is provenance, never
    /// authorization. A listed contributor who is not in the audience is
    /// refused.
    #[tokio::test]
    async fn a_listed_contributor_is_not_authorization() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(CONTRIB), false).await;
        assert!(
            !v.contains(PRIVATE_WITH_CONTRIBUTOR),
            "being listed in `contributors` MUST NOT grant retrieval of a private context — \
             contributors is provenance, not authorization (conformance vis-004). Visible set \
             was {v:?}"
        );
    }

    #[tokio::test]
    async fn an_outsider_may_not_retrieve_a_private_context() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(OUTSIDER), false).await;
        assert!(
            !v.contains(PRIVATE_WITH_AUDIENCE) && !v.contains(PRIVATE_OWNER_ONLY),
            "a requester who is neither producer nor audience MUST see no private context; \
             visible set was {v:?}"
        );
    }

    #[tokio::test]
    async fn the_producer_may_retrieve_their_own_private_context() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(PRODUCER), false).await;
        assert!(
            v.contains(PRIVATE_OWNER_ONLY),
            "the producer MUST be able to retrieve their own private context; visible set was {v:?}"
        );
    }

    /// §4.5 says nothing about status, and `retrieve` returns a retracted
    /// context (its status is projected, not hidden). A status filter added
    /// here would look like hardening and would actually be a correctness bug,
    /// hiding audit entries the caller is entitled to.
    #[tokio::test]
    async fn a_retracted_context_is_still_retrievable() {
        let s = FixtureBackend::new();
        // Precondition, asserted rather than assumed: the fixture must actually
        // be retracted by BOTH definitions in play — the `status` column and
        // the lifecycle-event derivation. Set only one and this test quietly
        // stops being about retraction, which is how it first passed against a
        // mutation that should have broken it.
        let c = s
            .by_id
            .get(PUBLIC_RETRACTED)
            .expect("retracted fixture present");
        assert_eq!(
            c.registry_state.status,
            Status::Retracted,
            "fixture precondition: the `status` column must say retracted"
        );
        assert!(
            c.registry_state.is_retracted(),
            "fixture precondition: `is_retracted()` derives from lifecycle_events, so the fixture              must carry a retracted event too"
        );
        let v = s.visible_to(Some(OUTSIDER), false).await;
        assert!(
            v.contains(PUBLIC_RETRACTED),
            "a retracted context is still RETRIEVABLE — status is projected, not hidden, and §4.5 \
             has no status clause. Filtering on status here would hide entries the caller is \
             entitled to. Visible set was {v:?}"
        );
    }

    #[tokio::test]
    async fn an_anonymous_requester_is_refused_public_when_the_flag_is_off() {
        let s = FixtureBackend::new();
        let v = s.visible_to(None, false).await;
        assert!(
            v.is_empty(),
            "with no requester and `public_arm_open` off, NOTHING is retrievable — not even \
             public. Visible set was {v:?}"
        );
    }

    #[tokio::test]
    async fn an_anonymous_requester_is_allowed_public_when_the_flag_is_on() {
        let s = FixtureBackend::new();
        let v = s.visible_to(None, true).await;
        let expected: HashSet<String> = [PUBLIC, PUBLIC_RETRACTED]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(
            v, expected,
            "with `public_arm_open` on, an anonymous requester sees exactly the public \
             contexts and nothing else"
        );
    }

    #[tokio::test]
    async fn an_unknown_ctx_id_is_absent_rather_than_an_error() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(PRODUCER), true).await;
        assert!(
            !v.contains(NEVER_STORED),
            "an id the backend never stored must simply be absent, not an error and not present"
        );
    }

    #[tokio::test]
    async fn an_unparseable_ctx_id_is_absent_rather_than_an_error() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(PRODUCER), true).await;
        assert!(
            !v.contains(NOT_A_CTX_ID),
            "an unparseable ctx_id must be treated as not-visible, mirroring the caller's own \
             CtxId::parse failure path rather than raising"
        );
    }

    /// The tenant gate is half the contract. `tenant_of_ctx`'s default reports
    /// every row as the reserved tenant, so a request for any other tenant must
    /// come back empty — the same "satisfy the trait without claiming a wrong
    /// answer" posture `search_in_tenant`'s default takes.
    #[tokio::test]
    async fn a_foreign_tenant_yields_nothing_on_an_untenanted_backend() {
        let s = FixtureBackend::new();
        let v = s
            .visible_ctx_ids(
                &FixtureBackend::all_ids(),
                Some(&did(PRODUCER)),
                Some("tenant-that-this-backend-never-assigns"),
                true,
            )
            .await
            .expect("visible_ctx_ids ok");
        assert!(
            v.is_empty(),
            "this backend records no tenants, so every row is the reserved tenant and a request \
             scoped to any other tenant must return NOTHING. Returning rows here would be the \
             tenant half of the contract silently dropped. Visible set was {v:?}"
        );
    }

    #[tokio::test]
    async fn the_reserved_tenant_passes_the_gate() {
        let s = FixtureBackend::new();
        let v = s
            .visible_ctx_ids(
                &FixtureBackend::all_ids(),
                Some(&did(PRODUCER)),
                Some(RESERVED_TENANT),
                true,
            )
            .await
            .expect("visible_ctx_ids ok");
        assert!(
            v.contains(PUBLIC),
            "on an untenanted backend every row IS `{RESERVED_TENANT}`, so scoping to it must not \
             filter anything out; visible set was {v:?}"
        );
    }

    /// Equality against the full expected subset, not a `contains` and not a
    /// floor (CHARTER rule 55): under-disclosure and over-disclosure are both
    /// silent failures here, and only an equality assertion catches both.
    #[tokio::test]
    async fn the_result_is_exactly_the_visible_subset() {
        let s = FixtureBackend::new();
        let v = s.visible_to(Some(AUDIENCE), false).await;
        let expected: HashSet<String> = [
            PUBLIC,
            PUBLIC_RETRACTED,
            PRIVATE_WITH_AUDIENCE,
            RESTRICTED_WITH_AUDIENCE,
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert_eq!(
            v, expected,
            "the audience member sees exactly: both public contexts (retracted included), the \
             private one naming them, and the restricted one naming them — and NOT the \
             owner-only private one nor the contributor-only one"
        );
    }

    /// The default is the N-round-trip shape on purpose (behaviour-preserving,
    /// not fail-closed). Asserting the count by equality documents the cost the
    /// SQL overrides exist to remove, and would catch a "clever" default that
    /// silently stopped consulting the store.
    #[tokio::test]
    async fn the_default_costs_one_get_per_parseable_id() {
        let s = FixtureBackend::new();
        let _ = s.visible_to(Some(PRODUCER), true).await;
        let parseable = FixtureBackend::all_ids().len() - 1; // NOT_A_CTX_ID never reaches `get`
        assert_eq!(
            s.gets(),
            parseable,
            "the default impl must do exactly one `get` per parseable id — that N-round-trip cost \
             is the whole reason the SQL backends override this method"
        );
    }
}
