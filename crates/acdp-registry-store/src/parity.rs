//! Cross-backend parity assertions, shared by every `ExtendedRegistryStore`.
//!
//! # Why this module exists
//!
//! The SQLite and Postgres backends are meant to be interchangeable behind
//! one store contract, and for `GET /contexts/search` they were not: date
//! filters compared RFC 3339 **lexicographically as TEXT** on SQLite while
//! Postgres compared real `timestamptz` values. Both backends had a contract
//! suite; neither suite exercised a single search filter, so CI was green the
//! whole time.
//!
//! Duplicating assertions into each backend's `tests/` would not fix that —
//! two copies drift, and the drift is invisible precisely when it matters. So
//! the assertions live here **once**, generic over the trait, and each backend
//! contributes a thin `tests/parity.rs` that constructs its own store and
//! calls them. A divergence then fails *both* suites.
//!
//! This crate deliberately does **not** depend on `acdp-registry-sqlite` or
//! `acdp-registry-pg` — they depend on it. Being generic over
//! `ExtendedRegistryStore` is what keeps that direction intact; importing the
//! concrete backends here to build a "both backends" test would invert the
//! dependency graph.
//!
//! Gated behind the `test-support` feature so nothing here ships in a normal
//! build.

use std::sync::Arc;

use acdp::crypto::SigningKey;
use acdp::producer::Producer;
use acdp::registry::store::{PublishCommit, PublishCommitOutcome};
use acdp::types::body::DataPeriod;
use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
use acdp::types::primitives::{AgentDid, ContextType, CtxId, Status, Visibility};
use acdp::types::search::SearchParams;
use chrono::{DateTime, TimeZone, Utc};

use crate::{decode_cursor, ExtendedRegistryStore};

const AUTHORITY: &str = "reg.test";

/// A distinct producer per scenario so parallel backends and repeated runs
/// never collide on `agent_id` or lineage.
fn producer(seed: u8) -> Producer {
    Producer::new(
        SigningKey::from_bytes(&[seed; 32]),
        AgentDid::new(agent_did(seed)),
        format!("{}#key-1", agent_did(seed)),
    )
}

/// The DID `producer(seed)` publishes under. Derived from the seed rather than
/// read back off `Producer` (whose `agent_id` is private), and used to scope
/// every search below to one producer so unrelated rows cannot decide a result.
fn agent_did(seed: u8) -> String {
    format!("did:web:agents.test:parity-{seed}")
}

/// `2026-01-01T00:00:00Z` plus `ms` milliseconds.
fn t(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(1_767_225_600_000 + ms)
        .single()
        .expect("valid timestamp")
}

/// Publish one context carrying `data_period`, returning its `ctx_id`.
///
/// `commit_publish` is a sync API that drives async sqlx internally, so it
/// runs on the blocking pool — the same shape both backends' existing
/// contract suites use.
async fn publish_with_period<S>(
    store: &Arc<S>,
    seed: u8,
    title: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> String
where
    S: ExtendedRegistryStore + 'static,
{
    let p = producer(seed);
    let req = p
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .data_period(DataPeriod { start, end })
        .build()
        .expect("valid publish request");
    let s = Arc::clone(store);
    let outcome = tokio::task::spawn_blocking(move || {
        s.commit_publish(PublishCommit {
            req: &req,
            authority: AUTHORITY,
            idempotency: None,
            tenant: None,
            receipt_minter: None,
            predecessor_admission: None,
        })
    })
    .await
    .expect("publish task")
    .expect("publish succeeds");
    match outcome {
        PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
            r.ctx_id.as_str().to_string()
        }
    }
}

/// Run one `search` and report whether `ctx_id` is in the result set.
fn search_contains<S>(store: &Arc<S>, params: &SearchParams, ctx_id: &str) -> bool
where
    S: ExtendedRegistryStore,
{
    let resp = store
        .search(params, None, true)
        .expect("search must not error");
    resp.matches.iter().any(|r| r.ctx_id.as_str() == ctx_id)
}

/// **B1 — `data_period_*` filters must agree across backends.**
///
/// SQLite stored `body_json` as TEXT and compared
/// `json_extract(...) >= ?` — a lexicographic compare — while the bound was
/// bound as `to_rfc3339()`. The two sides used *different serializers*: the
/// stored value comes from chrono's serde impl (`Z` suffix, 0/3/6/9
/// fractional digits) and the bound from `to_rfc3339()` (`+00:00` suffix).
/// Since `'+'`(0x2B) `< '.'`(0x2E) `<` digits `< 'Z'`(0x5A), a stored value
/// sorted *after* a bound naming the very same instant.
///
/// Measured in sqlite3 before the fix:
/// `'2026-01-01T00:00:00Z' <= '2026-01-01T00:00:00+00:00'` → **0**.
///
/// Each case below is written so the *correct* answer is obvious from the
/// instants alone, independent of either backend's storage representation —
/// that is what makes this a parity test rather than two hand-tuned
/// expectations.
///
/// `label` is echoed into failures so a red run names the backend.
pub async fn assert_data_period_filter_parity<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    // ── Case 1: an inclusive upper bound must include an EQUAL instant.
    //
    // This is the whole-second common case and the one the original audit
    // did not state. period.end == bound exactly, and `end_before` is
    // inclusive (`<=`), so the row must come back.
    let ctx = publish_with_period(store, 201, "parity dp equal bound", t(0), t(0)).await;
    let params = SearchParams {
        data_period_end_before: Some(fmt(t(0))),
        ..Default::default()
    };
    assert!(
        search_contains(store, &params, &ctx),
        "[{label}] data_period_end_before must INCLUDE a context whose \
         data_period.end is exactly the bound (both {} ) — an inclusive \
         bound that excludes an equal instant is the lexicographic-compare \
         bug",
        fmt(t(0))
    );

    // ── Case 2: a whole-second stored end, a sub-second bound strictly after it.
    //
    // end = …:00.000Z, bound = …:00.500Z, so end < bound and the row must
    // come back. Lexicographically 'Z' > '.', so TEXT compare wrongly
    // excluded it.
    let ctx = publish_with_period(store, 202, "parity dp frac bound", t(0), t(0)).await;
    let params = SearchParams {
        data_period_end_before: Some(fmt(t(500))),
        ..Default::default()
    };
    assert!(
        search_contains(store, &params, &ctx),
        "[{label}] data_period_end_before={} must INCLUDE a context whose \
         data_period.end is {} — it is strictly earlier",
        fmt(t(500)),
        fmt(t(0))
    );

    // ── Case 3: the mirror — a row that must be EXCLUDED.
    //
    // start = …:00.000Z, start_after = …:00.500Z. The start is strictly
    // before the lower bound, so the row must NOT match. Lexicographically
    // 'Z' > '.' made `start >= bound` wrongly TRUE, so this is the
    // false-positive direction — the one a test asserting only presence
    // would miss.
    let ctx = publish_with_period(store, 203, "parity dp excluded", t(0), t(1000)).await;
    let params = SearchParams {
        data_period_start_after: Some(fmt(t(500))),
        ..Default::default()
    };
    assert!(
        !search_contains(store, &params, &ctx),
        "[{label}] data_period_start_after={} must EXCLUDE a context whose \
         data_period.start is {} — it is strictly earlier, so matching it is \
         a false positive",
        fmt(t(500)),
        fmt(t(0))
    );

    // ── Case 4: a bound that genuinely selects, so the filter is not
    // vacuously excluding everything. Without this, a fix that made the
    // predicate always-false would pass cases 1-3's inverse and case 3.
    let ctx = publish_with_period(store, 204, "parity dp selects", t(0), t(1000)).await;
    let params = SearchParams {
        data_period_start_after: Some(fmt(t(0))),
        data_period_end_before: Some(fmt(t(1000))),
        ..Default::default()
    };
    assert!(
        search_contains(store, &params, &ctx),
        "[{label}] a context whose data_period sits exactly on both inclusive \
         bounds must match; if this fails the predicate has become \
         always-false rather than correct"
    );
}

/// Canonical RFC 3339 millisecond form with a `Z` suffix — the same shape
/// `acdp`'s own typed search builder emits, so these tests exercise the form
/// a real client sends rather than a convenient one.
fn fmt(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Publish one context with a searchable title, returning its `ctx_id`.
async fn publish_titled<S>(store: &Arc<S>, seed: u8, title: &str) -> String
where
    S: ExtendedRegistryStore + 'static,
{
    let p = producer(seed);
    let req = p
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid publish request");
    let s = Arc::clone(store);
    let outcome = tokio::task::spawn_blocking(move || {
        s.commit_publish(PublishCommit {
            req: &req,
            authority: AUTHORITY,
            idempotency: None,
            tenant: None,
            receipt_minter: None,
            predecessor_admission: None,
        })
    })
    .await
    .expect("publish task")
    .expect("publish succeeds");
    match outcome {
        PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
            r.ctx_id.as_str().to_string()
        }
    }
}

/// **B2 — `q=` must mean the same thing on every backend.**
///
/// SQLite used FTS5's default `unicode61` tokenizer (no stemmer, no stopwords)
/// while Postgres used `plainto_tsquery('english', …)` over an `english`
/// tsvector (snowball stemming, stopwords removed). Measured before the fix:
/// `q=running` matched "run report" on pg and nothing on sqlite; `q=the`
/// matched on sqlite and nothing on pg.
///
/// Postgres's semantics won; SQLite was raised to them via the `porter`
/// tokenizer plus query-side stopword removal. See
/// [`crate::fulltext`] for the decision and its cost.
///
/// **What this pins, and what it deliberately does not.** FTS5 `porter` and
/// snowball `english` are different implementations and will not agree on
/// every word in the language, so asserting stemmer identity would be an
/// overclaim. These cases pin the *mechanisms* instead — that a stemmed match
/// happens at all, that stopwords are dropped, that dropping them does not
/// destroy the rest of the query, that term conjunction holds, and that case
/// folding applies. A backend losing any of those fails here.
pub async fn assert_fulltext_parity<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    // Scope every query to one producer so unrelated rows — including rows
    // left by earlier runs against a persistent database — cannot decide the
    // outcome either way.
    let run = publish_titled(store, 210, "run report").await;
    let agent_run = agent_did(210);
    let q = |query: &str| SearchParams {
        q: Some(query.to_string()),
        agent_id: Some(agent_run.clone()),
        ..Default::default()
    };

    // 1. Stemming, the case that was broken on sqlite: an inflected query term
    //    must reach an uninflected document.
    assert!(
        search_contains(store, &q("running"), &run),
        "[{label}] q=running must match the document titled \"run report\" — \
         this is the stemming case that diverged (pg matched, sqlite did not)"
    );

    // 2. Stemming the other way: an uninflected query must reach an inflected
    //    document. Asserting only direction 1 would pass on a backend that
    //    stems the query but not the index.
    let running = publish_titled(store, 211, "running reports").await;
    let agent_running = agent_did(211);
    let q2 = |query: &str| SearchParams {
        q: Some(query.to_string()),
        agent_id: Some(agent_running.clone()),
        ..Default::default()
    };
    assert!(
        search_contains(store, &q2("report"), &running),
        "[{label}] q=report must match \"running reports\" — the index side \
         must stem too, not just the query side"
    );

    // 3. Stopwords: a stopword-only query matches nothing, because Postgres
    //    produces an empty tsquery for it.
    let the = publish_titled(store, 212, "the quarterly figures").await;
    let agent_the = agent_did(212);
    let q3 = |query: &str| SearchParams {
        q: Some(query.to_string()),
        agent_id: Some(agent_the.clone()),
        ..Default::default()
    };
    assert!(
        !search_contains(store, &q3("the"), &the),
        "[{label}] q=the must match NOTHING even though the document contains \
         the word — pg drops stopwords and yields an empty tsquery, so sqlite \
         must drop them too"
    );

    // 4. A stopword must not poison the rest of the query. Dropping "the"
    //    has to leave "figures" doing its job; a naive implementation that
    //    bails out on any stopword would fail here while passing case 3.
    assert!(
        search_contains(store, &q3("the figures"), &the),
        "[{label}] q='the figures' must still match — removing the stopword \
         must leave the remaining term active, not empty the whole query"
    );

    // 5. Case folding.
    assert!(
        search_contains(store, &q("RUNNING"), &run),
        "[{label}] q=RUNNING must match \"run report\" — matching is case-insensitive"
    );

    // 6. Term conjunction: all terms must be present (AND), which is the one
    //    thing the backends always agreed on. Pinned so a tokenizer change
    //    cannot silently turn it into OR.
    assert!(
        search_contains(store, &q("running report"), &run),
        "[{label}] q='running report' must match a document containing both terms"
    );
    assert!(
        !search_contains(store, &q("running elephant"), &run),
        "[{label}] q='running elephant' must NOT match — terms are AND-ed, so a \
         missing term excludes the document; if this passes, conjunction has \
         become disjunction"
    );
}

/// Publish a context and retract it, returning its `ctx_id`. Both backends
/// reach this through the same trait methods, so the setup cannot drift.
pub async fn publish_then_retract<S>(store: &Arc<S>, seed: u8, title: &str) -> String
where
    S: ExtendedRegistryStore + 'static,
{
    let ctx_id = publish_titled(store, seed, title).await;
    // `event_id` must be a canonical lowercase RFC 9562 UUID and must be
    // globally unique in `lifecycle_events`. Reuse the UUID the registry
    // already minted for this context: unique per run without pulling in a
    // uuid dependency, and valid by construction. (A ULID-shaped id was
    // rejected here with a SchemaViolation — the constraint is real.)
    let event_id = ctx_id
        .rsplit('/')
        .next()
        .expect("ctx_id ends in a UUID segment")
        .to_string();
    let event = LifecycleEvent::new(
        event_id,
        CtxId(ctx_id.clone()),
        LifecycleEventType::Retracted,
        Utc::now(),
        AgentDid::new(agent_did(seed)),
        Some("parity: torn-read fixture".to_string()),
    )
    .expect("valid lifecycle event");
    let s = Arc::clone(store);
    tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
        .await
        .expect("retract task")
        .expect("retract succeeds");
    ctx_id
}

/// **B3 — a context and its lifecycle events must never contradict each other.**
///
/// `get()` and `lineage()` read the context row and its lifecycle events as two
/// separate queries with no shared snapshot. A retraction committing between
/// them produced `registry_state.status: "active"` served *alongside* a
/// `retracted` event, violating the RFC-ACDP-0013 §7.2 precedence
/// (`retracted > superseded > expired > active`) that both backends'
/// `row_to_context` is documented to guarantee. A consumer trusting `status`
/// would act on withdrawn data.
///
/// **Why this does not race.** Driving the real interleaving means landing a
/// commit inside a window measured in microseconds; such a test is flaky, and a
/// flaky guard gets deleted. So the *state a tear produces* is constructed
/// directly instead — the caller desynchronizes the denormalized `retracted`
/// column from the event log with one UPDATE, which is exactly what the read
/// path would have observed mid-tear — and this asserts the response is still
/// coherent. That tests the property the race threatens rather than the timing.
///
/// The caller supplies `ctx_id` already in that state because clearing the
/// column takes backend-specific SQL; the assertion itself is shared so neither
/// backend can quietly diverge on what "coherent" means.
pub async fn assert_desynced_retraction_is_not_served_active<S>(
    store: &Arc<S>,
    label: &str,
    ctx_id: &str,
) where
    S: ExtendedRegistryStore + 'static,
{
    let s = Arc::clone(store);
    let id = CtxId(ctx_id.to_string());
    let ctx = tokio::task::spawn_blocking(move || s.get(&id))
        .await
        .expect("get task")
        .expect("get must not error")
        .expect("the context exists");

    let events = ctx
        .registry_state
        .lifecycle_events
        .as_deref()
        .unwrap_or(&[]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e.event_type, LifecycleEventType::Retracted)),
        "[{label}] fixture is wrong: the retraction event should still be in the \
         log — this test proves nothing if the event is absent"
    );
    assert_eq!(
        ctx.registry_state.status,
        Status::Retracted,
        "[{label}] a context whose event log carries a retraction must NOT be \
         served as {:?}. The row's denormalized flag said otherwise, which is \
         exactly what a torn read between the row query and the event query \
         produces, and the served pair must still be self-consistent.",
        ctx.registry_state.status
    );
}

/// Publish one context into `tenant`, returning its `ctx_id`.
///
/// Unlike [`publish_with_period`] this carries a `tenant`, which is what makes
/// the tenant-scoping assertion below possible at all. `created_at` is not
/// settable through the publish builder — it is stamped from the body — so
/// ordering between rows is publish order at millisecond resolution. The
/// caller is responsible for any gap it needs between groups.
async fn publish_in_tenant<S>(store: &Arc<S>, seed: u8, title: &str, tenant: &str) -> String
where
    S: ExtendedRegistryStore + 'static,
{
    let p = producer(seed);
    let req = p
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid publish request");
    let s = Arc::clone(store);
    let tenant = tenant.to_string();
    let outcome = tokio::task::spawn_blocking(move || {
        s.commit_publish(PublishCommit {
            req: &req,
            authority: AUTHORITY,
            idempotency: None,
            tenant: Some(&tenant),
            receipt_minter: None,
            predecessor_admission: None,
        })
    })
    .await
    .expect("publish task")
    .expect("publish succeeds");
    match outcome {
        PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => {
            r.ctx_id.as_str().to_string()
        }
    }
}

/// **H-H — a tenant-scoped search must not disclose a foreign tenant's rows,
/// including through its cursor.**
///
/// # What this is actually testing
///
/// Not "are foreign rows filtered out" — they always were, by the handler's
/// post-query `retain`. The defect this pins is subtler and survived that
/// filter: `acdp::pagination` anchors `next_cursor` on the **last raw scanned
/// row**, deliberately, so that a page whose rows are all dropped by post-SQL
/// filters does not halt pagination early. Filter by tenant *after* the scan
/// and the anchor can name another tenant's row — handing the caller its
/// `(created_at, ctx_id)` in a token. That is an ordering and existence oracle
/// over rows the caller must not know exist, walkable one page at a time.
///
/// # Why the fixture is shaped like this
///
/// Both groups are published by the **same producer**, so an `agent_id` filter
/// scopes the search to this scenario without also separating the tenants —
/// leaving `tenant` as the only thing that can distinguish them. A different
/// producer per tenant would have made the test pass for the wrong reason.
///
/// Tenant B is published **last**, so its rows carry the greater `created_at`
/// and sort first under `ORDER BY created_at DESC`. With `limit` below the
/// group size, a post-filtering implementation therefore scans B's rows first
/// and anchors on one of them. That ordering is the whole reason the fixture
/// can observe the defect, and it is verified by falsification rather than
/// assumed: with the predicate removed, this assertion must fail naming a
/// tenant-B `ctx_id`.
pub async fn assert_tenant_scoped_search_parity<S>(store: &Arc<S>, backend: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    const SEED: u8 = 241;
    const GROUP: usize = 3;
    const LIMIT: u32 = 2;

    // Tenant names are UNIQUE PER RUN, and that is load-bearing rather than
    // tidiness. This assertion checks an exact `total_estimate`, so it is
    // sensitive to rows left behind by earlier runs — and Postgres is a
    // PERSISTENT fixture, unlike SQLite's fresh tempfile. With fixed names the
    // second run against the same database saw 6 rows in `tenant-a`, the third
    // 9, and the assertion failed for a reason that had nothing to do with the
    // defect under test. The sibling assertions in this module dodge that by
    // testing membership rather than counts; this one cannot, so it isolates by
    // namespace instead.
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let tenant_a = format!("tenant-a-{run}");
    let tenant_b = format!("tenant-b-{run}");
    let (tenant_a, tenant_b) = (tenant_a.as_str(), tenant_b.as_str());

    let mut a_ids = Vec::new();
    for i in 0..GROUP {
        a_ids.push(publish_in_tenant(store, SEED, &format!("tenant a row {i}"), tenant_a).await);
    }
    // Strictly separate the two groups in `created_at` (millisecond
    // resolution), so tenant B reliably sorts first. See the doc above.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let mut b_ids = Vec::new();
    for i in 0..GROUP {
        b_ids.push(publish_in_tenant(store, SEED, &format!("tenant b row {i}"), tenant_b).await);
    }

    let params = SearchParams {
        agent_id: Some(agent_did(SEED)),
        limit: Some(LIMIT),
        ..Default::default()
    };
    let resp = store
        .search_in_tenant(&params, None, true, Some(tenant_a))
        .await
        .unwrap_or_else(|e| panic!("[{backend}] search_in_tenant must not error: {e:?}"));

    // (a) No foreign row may appear in the page itself.
    for m in &resp.matches {
        let id = m.ctx_id.as_str().to_string();
        assert!(
            !b_ids.contains(&id),
            "[{backend}] a search scoped to {tenant_a} returned {tenant_b}'s row {id}"
        );
    }

    // (b) The cursor anchor — the actual defect. Assert the anchored row's
    // IDENTITY, not merely that a cursor came back: a cursor anchored on a
    // foreign row is indistinguishable from a correct one until you decode it.
    let cursor = resp.next_cursor.as_deref().unwrap_or_else(|| {
        panic!(
            "[{backend}] expected a next_cursor: {GROUP} rows exist in {tenant_a} and the limit is \
             {LIMIT}, so the page must be resumable. Without a cursor this assertion cannot \
             observe the defect it exists to catch."
        )
    });
    let (_, anchor_id) = decode_cursor(cursor)
        .unwrap_or_else(|e| panic!("[{backend}] next_cursor must decode: {e:?}"))
        .unwrap_or_else(|| panic!("[{backend}] next_cursor decoded to None"));
    assert!(
        !b_ids.contains(&anchor_id),
        "[{backend}] next_cursor is anchored on {tenant_b}'s row {anchor_id} — a caller scoped to {tenant_a} \
         must never receive a token encoding another tenant's (created_at, ctx_id). This is \
         SECURITY follow-up #14: the row itself was filtered out, but its position leaked."
    );
    assert!(
        a_ids.contains(&anchor_id),
        "[{backend}] next_cursor anchor {anchor_id} belongs to neither tenant in this fixture — \
         the anchor must be one of the caller's own rows"
    );

    // (c) total_estimate must be the TENANT-scoped count, not the global one.
    // `COUNT(*) OVER ()` rides the same scan, so this is the observable proof
    // that the predicate is in the WHERE clause rather than applied afterwards.
    assert_eq!(
        resp.total_estimate,
        Some(GROUP as u64),
        "[{backend}] total_estimate must count only {tenant_a}'s {GROUP} rows. Counting all \
         {} rows is a cross-tenant count oracle — the A2 finding — and it is what a \
         post-query filter produces, because the count was computed before the filter ran.",
        GROUP * 2
    );

    // (d) The mirror: a tenant with no rows must see nothing and offer no
    // cursor. Without this, (a)-(c) would also pass an implementation that
    // ignored `tenant` and happened to be scanning only A's rows.
    let empty = store
        .search_in_tenant(&params, None, true, Some("tenant-with-no-rows-at-all"))
        .await
        .unwrap_or_else(|e| panic!("[{backend}] search_in_tenant must not error: {e:?}"));
    assert!(
        empty.matches.is_empty(),
        "[{backend}] a tenant with no rows matched {} contexts",
        empty.matches.len()
    );
    assert_eq!(
        empty.total_estimate,
        Some(0),
        "[{backend}] a tenant with no rows must have total_estimate 0"
    );
    assert!(
        empty.next_cursor.is_none(),
        "[{backend}] a tenant with no rows must not receive a cursor"
    );
}
