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
use acdp::types::primitives::{AgentDid, ContextType, Visibility};
use acdp::types::search::SearchParams;
use chrono::{DateTime, TimeZone, Utc};

use crate::ExtendedRegistryStore;

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
