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

/// **#390 — a durable backend's `has_lifecycle_state` must see a retraction.**
///
/// The server's startup refusal (RFC-ACDP-0013 §6: no lifecycle state may be
/// served while `acdp-registry-lifecycle` is not advertised) asks the store
/// [`ExtendedRegistryStore::has_lifecycle_state`]. That method is *defaulted*
/// to `Ok(false)` so non-durable backends keep compiling — which also means a
/// durable backend that forgets to override it compiles cleanly and silently
/// disables the refusal. Each backend's own contract suite pins its SQL; this
/// pins the obligation itself, once, so a new backend that runs the kit cannot
/// inherit the default unnoticed.
///
/// # Only the "sees it" direction
///
/// There is deliberately no "a fresh store reports `false`" leg. The Postgres
/// suite runs against one shared database that accretes retractions from
/// every test and every run, so a fresh-false leg would fail there for a
/// reason unrelated to the probe. Each backend's isolated contract suite owns
/// that direction.
///
/// # Pass the raw store
///
/// Call this with the backend itself, never a delegating wrapper: a wrapper
/// that does not forward `has_lifecycle_state` (the kit's own `DefaultOnly`
/// does not) answers with the default and fails this assertion — which is
/// exactly the defect it exists to catch.
pub async fn assert_lifecycle_state_probe_sees_retraction<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    let ctx_id = publish_then_retract(store, 250, "lifecycle probe fixture").await;
    let seen = store
        .has_lifecycle_state()
        .await
        .expect("has_lifecycle_state must not error");
    assert!(
        seen,
        "[{label}] has_lifecycle_state() returned false after {ctx_id} was retracted. \
         The server's RFC-ACDP-0013 §6 startup refusal relies on this probe; a \
         durable backend that leaves it at the trait default (Ok(false)) lets a \
         registry restarted with [lifecycle] off serve retracted state. Override \
         `ExtendedRegistryStore::has_lifecycle_state` to check both the event \
         store and the retracted flag."
    );
}

/// **RFC-ACDP-0013 §6 — a retry is "identical" only if `signature.value` matches too.**
///
/// A lifecycle retry carrying an already-appended `event_id` is an idempotent
/// replay only when the whole event — signature included — is byte-identical;
/// any other difference is "different content" and is refused with
/// `SchemaViolation`. Both backends implement that as `*prior == event`, and
/// each backend's contract suite pins a difference in `reason`. Nothing pinned
/// a difference confined to the signature, which is the case that matters for
/// ECDSA P-256: a high-S twin of a valid signature is a *different, still
/// valid* signature over the same preimage (the server test
/// `lc001b_p256_malleated_signature_retry_is_different_content` covers it end
/// to end on SQLite only). A backend whose comparison drops the signature —
/// say, comparing a projection of the stored row that omits the signature
/// column — would replay the twin as success and serve the original
/// signature in its response.
///
/// The store does not verify signatures (that is the server's §6 step 3), so
/// the twin here only needs to be *well formed*: the same `algorithm` and
/// `key_id`, and a `value` that is a real Ed25519 signature produced by a
/// different key. Only `signature.value` differs.
///
/// Legs, in order: the first commit applies; the signature-only twin is
/// refused with `SchemaViolation`; the byte-identical original still replays
/// idempotently with nothing appended and the original signature stored.
pub async fn assert_lifecycle_retry_requires_identical_signature<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::error::AcdpError;
    use acdp::registry::LifecycleCommitOutcome;

    const SEED: u8 = 251;
    let ctx_id = publish_titled(store, SEED, "lifecycle signature retry fixture").await;
    // Same event_id derivation as `publish_then_retract`: unique per run and a
    // canonical UUID by construction.
    let event_id = ctx_id
        .rsplit('/')
        .next()
        .expect("ctx_id ends in a UUID segment")
        .to_string();
    let key_id = format!("{}#key-1", agent_did(SEED));
    let unsigned = LifecycleEvent::new(
        event_id,
        CtxId(ctx_id.clone()),
        LifecycleEventType::Retracted,
        Utc::now(),
        AgentDid::new(agent_did(SEED)),
        Some("parity: signature retry fixture".to_string()),
    )
    .expect("valid lifecycle event");
    let original = unsigned
        .clone()
        .sign_with(SigningKey::from_bytes(&[SEED; 32]), key_id.clone())
        .expect("sign original");
    // A real signature over the same preimage by another key: well formed,
    // same algorithm, same key_id field — only `value` differs.
    let other_value = unsigned
        .sign_with(SigningKey::from_bytes(&[SEED ^ 0xff; 32]), key_id)
        .expect("sign twin")
        .signature
        .expect("signed")
        .value;
    let mut twin = original.clone();
    let original_sig = original.signature.clone().expect("signed");
    {
        let sig = twin.signature.as_mut().expect("signed");
        assert_ne!(
            sig.value, other_value,
            "fixture is wrong: the twin's signature must differ"
        );
        sig.value = other_value;
    }
    assert_eq!(
        (
            &twin.event_id,
            &twin.ctx_id,
            &twin.event_type,
            twin.occurred_at,
            &twin.actor,
            &twin.reason
        ),
        (
            &original.event_id,
            &original.ctx_id,
            &original.event_type,
            original.occurred_at,
            &original.actor,
            &original.reason
        ),
        "fixture is wrong: the twin must differ from the original ONLY in signature.value"
    );

    let commit = |event: LifecycleEvent| {
        let s = Arc::clone(store);
        async move {
            tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
                .await
                .expect("commit task")
        }
    };

    // Leg 1: the first commit applies.
    match commit(original.clone()).await {
        Ok(LifecycleCommitOutcome::Applied(_)) => {}
        other => panic!("[{label}] fixture is wrong: the first commit must apply, got {other:?}"),
    }

    // Leg 2: signature-only difference → different content.
    match commit(twin).await {
        Err(AcdpError::SchemaViolation(_)) => {}
        Ok(LifecycleCommitOutcome::IdempotentReplay(_)) => panic!(
            "[{label}] a retry differing ONLY in signature.value was replayed as \
             idempotent. RFC-ACDP-0013 §6: a retry is identical only if the whole \
             event, signature included, matches; a signature-only difference (e.g. a \
             malleated P-256 high-S twin) is different content and MUST be refused"
        ),
        other => panic!(
            "[{label}] a retry differing ONLY in signature.value must be refused with \
             SchemaViolation (different content), got {other:?}"
        ),
    }

    // Leg 3: the byte-identical retry still replays, nothing appended, and the
    // stored signature is the original one.
    let ctx = match commit(original).await {
        Ok(LifecycleCommitOutcome::IdempotentReplay(c)) => c,
        other => panic!(
            "[{label}] a byte-identical lifecycle retry must be an IdempotentReplay, \
             got {other:?}"
        ),
    };
    let events = ctx
        .registry_state
        .lifecycle_events
        .as_deref()
        .unwrap_or(&[]);
    assert_eq!(
        events.len(),
        1,
        "[{label}] the refused twin or the replay appended an event"
    );
    assert_eq!(
        events[0].signature.as_ref(),
        Some(&original_sig),
        "[{label}] the stored signature must be the original one"
    );
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

    // Every guarantee below is EVALUATED on every run, and violations are
    // collected rather than asserted one at a time. `assert!` aborts the test at
    // the first failure, so four guarantees behind four asserts means one verdict
    // for four promises and three that may never have executed (CHARTER rule 51).
    // Accumulating them means a single mutation reports exactly which guarantees
    // it broke — which is both a stronger proof that each is live and a far more
    // useful failure message.
    let mut violations: Vec<String> = Vec::new();

    // (a) No foreign row may appear in the page itself.
    let leaked: Vec<&str> = resp
        .matches
        .iter()
        .map(|m| m.ctx_id.as_str())
        .filter(|id| b_ids.iter().any(|b| b == id))
        .collect();
    if !leaked.is_empty() {
        violations.push(format!(
            "(a) page content: a search scoped to {tenant_a} returned {tenant_b}'s rows {leaked:?}"
        ));
    }

    // (b) The cursor anchor — the actual defect. Check the anchored row's
    // IDENTITY, not merely that a cursor came back: a cursor anchored on a
    // foreign row is indistinguishable from a correct one until you decode it.
    match resp.next_cursor.as_deref() {
        None => violations.push(format!(
            "(b) cursor absent: {GROUP} rows exist in {tenant_a} and the limit is {LIMIT}, so the \
             page must be resumable. With no cursor this guarantee cannot be observed at all, \
             which is a failure of the fixture as much as of the code."
        )),
        Some(cursor) => match decode_cursor(cursor) {
            Err(e) => violations.push(format!("(b) next_cursor did not decode: {e:?}")),
            Ok(None) => violations.push("(b) next_cursor decoded to None".to_string()),
            Ok(Some((_, anchor_id))) => {
                if b_ids.contains(&anchor_id) {
                    violations.push(format!(
                        "(b) CURSOR ANCHOR: next_cursor is anchored on {tenant_b}'s row \
                         {anchor_id} — a caller scoped to {tenant_a} must never receive a token \
                         encoding another tenant's (created_at, ctx_id). This is SECURITY \
                         follow-up #14: the row itself was filtered out, but its position leaked."
                    ));
                } else if !a_ids.contains(&anchor_id) {
                    violations.push(format!(
                        "(b) anchor {anchor_id} belongs to neither tenant in this fixture — it \
                         must be one of the caller's own rows"
                    ));
                }
            }
        },
    }

    // (c) total_estimate must be the TENANT-scoped count, not the global one.
    // `COUNT(*) OVER ()` rides the same scan, so this is the observable proof
    // that the predicate is in the WHERE clause rather than applied afterwards.
    if resp.total_estimate != Some(GROUP as u64) {
        violations.push(format!(
            "(c) total_estimate is {:?}, must be Some({GROUP}) — only {tenant_a}'s rows. Counting \
             ANY row outside that tenant is a cross-tenant count oracle (the A2 finding), and it \
             is exactly what a post-query filter produces, because the count was computed before \
             the filter ran. Note the observed number can exceed this fixture's own row count: on \
             a persistent backend the agent_id filter also matches earlier runs' rows, which is \
             itself a demonstration that the oracle grows with the table.",
            resp.total_estimate
        ));
    }

    // (d) The mirror. Without it, (a)-(c) would also pass an implementation that
    // ignored `tenant` entirely and happened to be scanning only A's rows.
    let empty = store
        .search_in_tenant(&params, None, true, Some("tenant-with-no-rows-at-all"))
        .await
        .unwrap_or_else(|e| panic!("[{backend}] search_in_tenant must not error: {e:?}"));
    if !empty.matches.is_empty() {
        violations.push(format!(
            "(d) a tenant with no rows matched {} contexts",
            empty.matches.len()
        ));
    }
    if empty.total_estimate != Some(0) {
        violations.push(format!(
            "(d) a tenant with no rows reported total_estimate {:?}, must be Some(0)",
            empty.total_estimate
        ));
    }
    if empty.next_cursor.is_some() {
        violations.push("(d) a tenant with no rows must not receive a cursor".to_string());
    }

    assert!(
        violations.is_empty(),
        "[{backend}] {} tenant-scoping guarantee(s) violated:\n  - {}",
        violations.len(),
        violations.join("\n  - ")
    );
}

// ── H-I-s: batched retrieval visibility ──────────────────────────────────────

/// Publish one context with an explicit disclosure shape, into `tenant`.
///
/// `audience` and `contributors` are separate parameters because the entire
/// point of one fixture below is that they are NOT interchangeable.
#[allow(clippy::too_many_arguments)]
async fn publish_visible<S>(
    store: &Arc<S>,
    seed: u8,
    title: &str,
    tenant: &str,
    visibility: Visibility,
    audience: Vec<AgentDid>,
    contributors: Vec<AgentDid>,
) -> String
where
    S: ExtendedRegistryStore + 'static,
{
    let p = producer(seed);
    let mut b = p
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(visibility);
    if !audience.is_empty() {
        b = b.audience(audience);
    }
    if !contributors.is_empty() {
        b = b.contributors(contributors);
    }
    let req = b.build().expect("valid publish request");
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

/// A store that delegates everything **except** [`ExtendedRegistryStore::visible_ctx_ids`],
/// which it leaves to the trait default.
///
/// # Why this exists
///
/// `visible_ctx_ids` has three implementations in this workspace — the Rust
/// default here, SQLite's SQL predicate, and Postgres's. The backends' own
/// suites exercise only their own override, and the unit tests exercise the
/// default against a hand-built in-memory fixture, so nothing else compares
/// them to each other.
///
/// **What this wrapper does and does not establish.** The default body is
/// `retrieve_visible` plus a tenant check, and `visible_by_n_calls` calls
/// `retrieve_visible` too — so this comparison and the N-call differential
/// share one centre. On its own that made the pair blind to a defect IN
/// `retrieve_visible`, and worse: because the SQL was derived separately, a
/// central defect would have surfaced as "the SQL disagrees", pointing at the
/// wrong implementation. `EXPECTED_BY_SPEC` is the anchor that fixes this; this
/// wrapper is retained for what it genuinely catches, which is the SQL drifting
/// away from the Rust rule.
///
/// **The upstream authority is NOT compared here — this is a standing MANUAL
/// check.** The normative rule is `can_retrieve` in `acdp-server`
/// (`src/registry/server.rs`), and it is `pub(crate)`, so no test in this
/// workspace can call it. `retrieve_visible` is a local transcription of it.
/// Hand-diffed against `acdp-server` **0.13.1** on **2026-09-13**, normalising
/// only the renamed parameter (`caps.anonymous_public_reads` → `public_arm_open`):
/// the two match arms were **textually identical**. Treat that as uncompared
/// rather than covered — it is true as of that version and date, and nothing
/// automated will notice if a future bump changes it. Re-run the diff when the
/// `acdp-server` pin moves.
///
/// Wrapping a *real* backend and declining to override the one method makes the
/// default body run against the same rows the override just answered for, so the
/// two can be diffed directly.
///
/// `tenant_of_ctx` is delegated deliberately. Left to its own default it reports
/// every row as the reserved tenant, and the comparison would then fail on the
/// tenant gate for a reason that has nothing to do with visibility — a test
/// failing for the wrong reason is worse than no test.
struct DefaultOnly<S>(Arc<S>);

impl<S> acdp::registry::RegistryStore for DefaultOnly<S>
where
    S: ExtendedRegistryStore + 'static,
{
    fn put(&self, body: acdp::types::body::Body) -> Result<(), acdp::error::AcdpError> {
        self.0.put(body)
    }
    fn get(
        &self,
        ctx_id: &CtxId,
    ) -> Result<Option<acdp::types::body::FullContext>, acdp::error::AcdpError> {
        self.0.get(ctx_id)
    }
    fn lineage(
        &self,
        lineage_id: &acdp::types::primitives::LineageId,
    ) -> Result<Vec<acdp::types::body::FullContext>, acdp::error::AcdpError> {
        self.0.lineage(lineage_id)
    }
    fn current(
        &self,
        lineage_id: &acdp::types::primitives::LineageId,
    ) -> Result<Option<acdp::types::body::FullContext>, acdp::error::AcdpError> {
        self.0.current(lineage_id)
    }
    fn mark_superseded(&self, ctx_id: &CtxId) -> Result<(), acdp::error::AcdpError> {
        self.0.mark_superseded(ctx_id)
    }
    fn first_version_ctx_id(
        &self,
        lineage_id: &acdp::types::primitives::LineageId,
    ) -> Result<Option<CtxId>, acdp::error::AcdpError> {
        self.0.first_version_ctx_id(lineage_id)
    }
    fn search(
        &self,
        params: &SearchParams,
        requester: Option<&AgentDid>,
        public_arm_open: bool,
    ) -> Result<acdp::types::search::SearchResponse, acdp::error::AcdpError> {
        self.0.search(params, requester, public_arm_open)
    }
    fn idempotency_lookup(
        &self,
        agent_id: &AgentDid,
        key: &str,
    ) -> Result<Option<acdp::registry::IdempotencyRecord>, acdp::error::AcdpError> {
        self.0.idempotency_lookup(agent_id, key)
    }
    fn idempotency_record(
        &self,
        agent_id: &AgentDid,
        key: &str,
        hash: &acdp::types::primitives::ContentHash,
        response: &acdp::types::publish::PublishResponse,
        expires_at: DateTime<Utc>,
    ) -> Result<(), acdp::error::AcdpError> {
        self.0
            .idempotency_record(agent_id, key, hash, response, expires_at)
    }
    fn idempotency_evict_expired(&self, now: DateTime<Utc>) -> Result<(), acdp::error::AcdpError> {
        self.0.idempotency_evict_expired(now)
    }
    fn commit_publish(
        &self,
        commit: PublishCommit<'_>,
    ) -> Result<PublishCommitOutcome, acdp::error::AcdpError> {
        self.0.commit_publish(commit)
    }
    fn commit_lifecycle_event(
        &self,
        event: &LifecycleEvent,
    ) -> Result<acdp::registry::LifecycleCommitOutcome, acdp::error::AcdpError> {
        self.0.commit_lifecycle_event(event)
    }
}

#[async_trait::async_trait]
impl<S> ExtendedRegistryStore for DefaultOnly<S>
where
    S: ExtendedRegistryStore + 'static,
{
    async fn health(&self) -> Result<(), acdp::error::AcdpError> {
        self.0.health().await
    }
    async fn migrate(&self) -> Result<(), acdp::error::AcdpError> {
        self.0.migrate().await
    }
    async fn list_contexts(
        &self,
        limit: u32,
        cursor: Option<&str>,
        requester: Option<&AgentDid>,
        tenant: Option<&str>,
        public_arm_open: bool,
    ) -> Result<crate::Page<acdp::types::body::FullContext>, acdp::error::AcdpError> {
        self.0
            .list_contexts(limit, cursor, requester, tenant, public_arm_open)
            .await
    }
    /// Delegated on purpose — see the type's docs.
    async fn tenant_of_ctx(&self, ctx_id: &str) -> Result<Option<String>, acdp::error::AcdpError> {
        self.0.tenant_of_ctx(ctx_id).await
    }
    // `visible_ctx_ids` deliberately NOT overridden: the default body is the
    // subject of the three-way comparison.
}

/// Retract an already-published context in place. `publish_then_retract` exists
/// but publishes into the default tenant, and this fixture needs the retracted
/// row inside the scoped tenant.
async fn retract_in_place<S>(store: &Arc<S>, ctx_id: &str, seed: u8)
where
    S: ExtendedRegistryStore + 'static,
{
    let event_id = ctx_id
        .rsplit('/')
        .next()
        .expect("ctx_id ends in a UUID segment")
        .to_string();
    let event = LifecycleEvent::new(
        event_id,
        CtxId(ctx_id.to_string()),
        LifecycleEventType::Retracted,
        Utc::now(),
        AgentDid::new(agent_did(seed)),
        Some("parity: retracted rows stay retrievable".to_string()),
    )
    .expect("valid lifecycle event");
    let s = Arc::clone(store);
    tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
        .await
        .expect("retract task")
        .expect("retract succeeds");
}

/// The N-call reference: what the caller did before batching existed — read
/// each context and apply the rule in Rust.
///
/// This is deliberately a **different implementation** of the same rule from the
/// one under test: the backend answers with a SQL predicate, this answers with
/// [`crate::retrieve_visible`] over a decoded body. A hand-written expected set
/// could only encode what its author believed; a second implementation disagrees
/// whenever either side is wrong, including in ways nobody anticipated.
async fn visible_by_n_calls<S>(
    store: &Arc<S>,
    ctx_ids: &[&str],
    requester: Option<&AgentDid>,
    tenant: Option<&str>,
    public_arm_open: bool,
) -> std::collections::HashSet<String>
where
    S: ExtendedRegistryStore + 'static,
{
    let mut out = std::collections::HashSet::new();
    for id in ctx_ids {
        let s = Arc::clone(store);
        let parsed = CtxId((*id).to_string());
        let got = tokio::task::spawn_blocking(move || s.get(&parsed))
            .await
            .expect("get task")
            .expect("get ok");
        let Some(ctx) = got else { continue };
        if !crate::retrieve_visible(&ctx.body, requester, public_arm_open) {
            continue;
        }
        if let Some(want) = tenant {
            let stored = store
                .tenant_of_ctx(id)
                .await
                .expect("tenant_of_ctx ok")
                .unwrap_or_else(|| "default".to_string());
            if stored != want {
                continue;
            }
        }
        out.insert((*id).to_string());
    }
    out
}

/// A fixture row, by name. The spec table below is written against these names
/// so that transcribing RFC-ACDP-0008 §4.5 never requires knowing a `ctx_id`,
/// and so a reader can check the table against the RFC without reading any
/// Rust that computes visibility.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    Public,
    PrivateWithAudience,
    PrivateOwnerOnly,
    RestrictedWithAudience,
    PrivateWithContributor,
    PublicRetracted,
    ForeignTenantPublic,
}

/// The requester perspective a table row is written for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Who {
    Producer,
    Audience,
    Contributor,
    Outsider,
    AnonReadsOn,
    AnonReadsOff,
}

/// **The independent anchor: RFC-ACDP-0008 §4.5, transcribed by hand.**
///
/// # Why this is a literal table and not a computation
///
/// Every other check in this file is *relational* — it asserts that two
/// expressions of the §4.5 rule agree. That family of assertion is blind to any
/// change that moves both sides together, and until this table existed the two
/// differentials below were not even independent of each other: the trait
/// default body IS `retrieve_visible` (`lib.rs`), and `visible_by_n_calls` calls
/// `retrieve_visible` too. So both legs bottomed out in one expression.
///
/// The consequence was worse than missing a defect. If `retrieve_visible` were
/// wrong, the default would agree with the reference perfectly, that leg would
/// stay green, and the only implementation able to disagree would be the SQL —
/// which was derived separately. **The suite would have reported the SQL as the
/// broken side.** A shared-centre differential inverts the blame; it does not
/// merely lose sensitivity.
///
/// This table is therefore written from the *specification text*, not from any
/// implementation. Deriving it by calling anything in this workspace would
/// reintroduce exactly the defect it exists to remove — the convenience of
/// computing it is the trap.
///
/// # Transcribed from (spec repo, read 2026-09-13)
///
/// `rfcs/RFC-ACDP-0008-security.md` §4.5, whose rules are:
///
///   * `public` — any *authenticated* requester; an anonymous requester only
///     when the registry advertises `anonymous_public_reads` (§6.3).
///   * `restricted` — retrieval: `agent_id`, plus every DID in `audience`.
///   * `private` — retrieval: `agent_id`, plus every DID in `audience`.
///     (Search is strictly narrower and is NOT this method's rule.)
///   * `contributors` are **never** authorization — attribution only. A
///     producer granting a contributor read access must list them in
///     `audience`.
///   * §4.5 states no `status` clause, so a **retracted** context stays
///     retrievable.
///   * The tenant gate is separate from §4.5: a request scoped to a tenant
///     sees only that tenant's rows.
///
/// The `bool` is `tenant_scoped` — `true` means the call passes
/// `Some(tenant)`, `false` means `None`.
const EXPECTED_BY_SPEC: &[(Who, bool, &[Row])] = &[
    // ── The producer: owns every row, so §4.5 authorizes all of them. The
    //    only thing that can remove one is the tenant gate.
    (
        Who::Producer,
        true,
        &[
            Row::Public,
            Row::PrivateWithAudience,
            Row::PrivateOwnerOnly,
            Row::RestrictedWithAudience,
            Row::PrivateWithContributor,
            Row::PublicRetracted,
        ],
    ),
    (
        Who::Producer,
        false,
        &[
            Row::Public,
            Row::PrivateWithAudience,
            Row::PrivateOwnerOnly,
            Row::RestrictedWithAudience,
            Row::PrivateWithContributor,
            Row::PublicRetracted,
            Row::ForeignTenantPublic,
        ],
    ),
    // ── An audience member: authenticated, so `public` and the retracted
    //    public row; named in `audience`, so the private and restricted rows
    //    that name them. NOT the owner-only private row, and NOT the private
    //    row whose only extra DID is a contributor.
    (
        Who::Audience,
        true,
        &[
            Row::Public,
            Row::PrivateWithAudience,
            Row::RestrictedWithAudience,
            Row::PublicRetracted,
        ],
    ),
    (
        Who::Audience,
        false,
        &[
            Row::Public,
            Row::PrivateWithAudience,
            Row::RestrictedWithAudience,
            Row::PublicRetracted,
            Row::ForeignTenantPublic,
        ],
    ),
    // ── A listed contributor: authenticated, and that is ALL it buys. §4.5
    //    says `contributors` is attribution, never authorization — so this
    //    perspective sees exactly what any authenticated stranger sees.
    (Who::Contributor, true, &[Row::Public, Row::PublicRetracted]),
    (
        Who::Contributor,
        false,
        &[Row::Public, Row::PublicRetracted, Row::ForeignTenantPublic],
    ),
    // ── An outsider: authenticated, in no audience, owns nothing.
    (Who::Outsider, true, &[Row::Public, Row::PublicRetracted]),
    (
        Who::Outsider,
        false,
        &[Row::Public, Row::PublicRetracted, Row::ForeignTenantPublic],
    ),
    // ── Anonymous with `anonymous_public_reads` ON: the public arm opens,
    //    and nothing else does — `restricted`/`private` require a requester.
    (Who::AnonReadsOn, true, &[Row::Public, Row::PublicRetracted]),
    (
        Who::AnonReadsOn,
        false,
        &[Row::Public, Row::PublicRetracted, Row::ForeignTenantPublic],
    ),
    // ── Anonymous with the flag OFF: §4.5 authorizes nothing at all.
    (Who::AnonReadsOff, true, &[]),
    (Who::AnonReadsOff, false, &[]),
];

/// **H-I-s — a batched visibility check must answer exactly what N individual
/// retrieve checks answer.**
///
/// # What makes this worth a cross-backend assertion
///
/// The failure mode is silent in both directions. Too permissive and one
/// tenant's audit log discloses another's `ctx_id`s; too strict and entries the
/// caller is entitled to vanish with no error. Neither shows up as a crash, and
/// a page of 256 ids that is wrong about one of them looks exactly like a page
/// that is right.
///
/// # `retrieve` semantics, not `search` semantics
///
/// The fixture's load-bearing row is `private` with an `audience`. Under
/// RFC-ACDP-0008 §4.5 an audience member MAY retrieve it — and `search`
/// deliberately refuses that exact case, requiring ownership for `private`
/// (conformance `vis-004`, "private/audience retrieval asymmetry"). A backend
/// that answers this method with its search predicate therefore under-discloses,
/// and the assertion below is what says so. A listed **contributor** who is not
/// in the audience is refused: contributors are provenance, not authorization.
///
/// # Why a retracted row is in the fixture
///
/// §4.5 has no status clause and `retrieve` returns a retracted context, so a
/// status filter here would look like hardening and would actually hide log
/// entries. Included so that "helpfully" adding one fails.
pub async fn assert_batched_visibility_parity<S>(store: &Arc<S>, backend: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    // Unique per run: these fixtures live in a persistent Postgres across runs,
    // and in H-H an exact-count assertion against accumulated rows is exactly
    // how I shipped a flake.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let tenant = format!("tenant-vis-{nonce}");

    // One producer for every row, so `agent_id` never accidentally separates
    // the fixtures — visibility and audience are the only distinguishing facts.
    const OWNER: u8 = 71;
    const AUD: u8 = 72;
    const CONTRIB: u8 = 73;
    const OUTSIDER: u8 = 74;
    let owner = AgentDid::new(agent_did(OWNER));
    let aud = AgentDid::new(agent_did(AUD));
    let contrib = AgentDid::new(agent_did(CONTRIB));
    let outsider = AgentDid::new(agent_did(OUTSIDER));

    let public = publish_visible(
        store,
        OWNER,
        &format!("vis-public-{nonce}"),
        &tenant,
        Visibility::Public,
        vec![],
        vec![],
    )
    .await;
    let private_with_audience = publish_visible(
        store,
        OWNER,
        &format!("vis-private-aud-{nonce}"),
        &tenant,
        Visibility::Private,
        vec![aud.clone()],
        vec![],
    )
    .await;
    let private_owner_only = publish_visible(
        store,
        OWNER,
        &format!("vis-private-owner-{nonce}"),
        &tenant,
        Visibility::Private,
        vec![],
        vec![],
    )
    .await;
    let restricted_with_audience = publish_visible(
        store,
        OWNER,
        &format!("vis-restricted-aud-{nonce}"),
        &tenant,
        Visibility::Restricted,
        vec![aud.clone()],
        vec![],
    )
    .await;
    let private_with_contributor = publish_visible(
        store,
        OWNER,
        &format!("vis-private-contrib-{nonce}"),
        &tenant,
        Visibility::Private,
        vec![],
        vec![contrib.clone()],
    )
    .await;
    // Public, then RETRACTED. §4.5 has no status clause and `retrieve` still
    // returns a retracted context, so this row must remain visible — a status
    // filter added here would look like hardening and would actually hide audit
    // entries the caller is entitled to.
    let public_retracted = publish_visible(
        store,
        OWNER,
        &format!("vis-retracted-{nonce}"),
        &tenant,
        Visibility::Public,
        vec![],
        vec![],
    )
    .await;
    retract_in_place(store, &public_retracted, OWNER).await;

    // A row in a DIFFERENT tenant, to prove the tenant half of the contract is
    // applied and not merely accepted as a parameter.
    let foreign_tenant_public = publish_visible(
        store,
        OWNER,
        &format!("vis-foreign-{nonce}"),
        &format!("tenant-other-{nonce}"),
        Visibility::Public,
        vec![],
        vec![],
    )
    .await;

    let never_stored = format!("acdp://{AUTHORITY}/00000000-0000-4000-8000-0000{nonce:08x}");
    let ids_owned: Vec<String> = vec![
        public.clone(),
        private_with_audience.clone(),
        private_owner_only.clone(),
        restricted_with_audience.clone(),
        private_with_contributor.clone(),
        public_retracted.clone(),
        foreign_tenant_public.clone(),
        never_stored,
        "not-a-ctx-id-at-all".to_string(),
    ];
    let ids: Vec<&str> = ids_owned.iter().map(String::as_str).collect();

    let mut violations: Vec<String> = Vec::new();

    // (a) The differential, per requester perspective. Accumulate rather than
    // assert inside the loop, so one divergence does not hide the others.
    for (who, requester, anon) in [
        ("the producer", Some(&owner), false),
        ("an audience member", Some(&aud), false),
        ("a listed contributor", Some(&contrib), false),
        ("an outsider", Some(&outsider), false),
        ("an anonymous caller, anon reads ON", None, true),
        ("an anonymous caller, anon reads OFF", None, false),
    ] {
        for scope in [None, Some(tenant.as_str())] {
            let batched = store
                .visible_ctx_ids(&ids, requester, scope, anon)
                .await
                .expect("visible_ctx_ids ok");
            let reference = visible_by_n_calls(store, &ids, requester, scope, anon).await;
            if batched != reference {
                let only_batched: Vec<_> = batched.difference(&reference).collect();
                let only_ref: Vec<_> = reference.difference(&batched).collect();
                violations.push(format!(
                    "[{backend}] batched != N-call for {who} (tenant scope {scope:?}): \n                         DISCLOSED BY THE BATCH ONLY (over-disclosure): {only_batched:?}\n                         SEEN ONLY BY THE N-CALL REFERENCE (under-disclosure): {only_ref:?}"
                ));
            }
        }
    }

    // (b) The spec behaviour, stated absolutely rather than only differentially.
    // A pure differential passes if BOTH implementations are wrong the same way;
    // these two pin the actual §4.5 outcome per backend.
    let as_audience = store
        .visible_ctx_ids(&ids, Some(&aud), Some(&tenant), false)
        .await
        .expect("visible_ctx_ids ok");
    if !as_audience.contains(&private_with_audience) {
        violations.push(format!(
            "[{backend}] an audience member could NOT retrieve the private context naming them \
             ({private_with_audience}). Under RFC-ACDP-0008 §4.5 they MUST be able to \
             (conformance vis-004). This is the exact case `search` refuses, so the most likely \
             cause is that this backend answered with its SEARCH predicate instead of its \
             LIST/retrieve one — which under-discloses."
        ));
    }
    let as_contributor = store
        .visible_ctx_ids(&ids, Some(&contrib), Some(&tenant), false)
        .await
        .expect("visible_ctx_ids ok");
    if as_contributor.contains(&private_with_contributor) {
        violations.push(format!(
            "[{backend}] a listed CONTRIBUTOR retrieved a private context ({private_with_contributor}). \
             Contributors are provenance, never authorization (conformance vis-004)."
        ));
    }

    let as_outsider = store
        .visible_ctx_ids(&ids, Some(&outsider), Some(&tenant), false)
        .await
        .expect("visible_ctx_ids ok");
    // The two rows this fixture publishes precisely so that an outsider must NOT
    // see them. Before U-535 both appeared ONLY in `ids_owned` -- published,
    // passed in, and never named in an assertion. Membership in a computed set is
    // not a check; these say what the outsider sees, by name.
    if as_outsider.contains(&private_owner_only) {
        violations.push(format!(
            "[{backend}] an OUTSIDER retrieved a PRIVATE owner-only context \
             ({private_owner_only}). Under RFC-ACDP-0008 §4.5 `private` authorizes \
             `agent_id` plus any DID in `audience`; this outsider is neither. This is \
             the core disclosure failure the predicate exists to prevent."
        ));
    }
    if as_outsider.contains(&restricted_with_audience) {
        violations.push(format!(
            "[{backend}] an OUTSIDER retrieved a RESTRICTED context \
             ({restricted_with_audience}) whose `audience` does not name them. §4.5 \
             authorizes `agent_id` plus every DID in `audience` for `restricted`, and \
             an outsider is in neither set."
        ));
    }
    if !as_outsider.contains(&public_retracted) {
        violations.push(format!(
            "[{backend}] a RETRACTED public context ({public_retracted}) was not retrievable. \
             §4.5 has no status clause and `retrieve` returns retracted contexts, so filtering on \
             status here hides log entries the caller is entitled to."
        ));
    }

    // (c) The tenant half of the contract. A batch that applies visibility and
    // silently ignores `tenant` passes every assertion above except this one.
    let scoped = store
        .visible_ctx_ids(&ids, Some(&owner), Some(&tenant), true)
        .await
        .expect("visible_ctx_ids ok");
    if scoped.contains(&foreign_tenant_public) {
        violations.push(format!(
            "[{backend}] a request scoped to {tenant} returned {foreign_tenant_public}, which \
             belongs to another tenant. The tenant gate is half of what this method replaces; \
             batching only the visibility half is a cross-tenant disclosure."
        ));
    }

    // (d) Ids the store does not hold are absent, not errors and not present.
    if scoped.iter().any(|id| id == "not-a-ctx-id-at-all") {
        violations.push(format!(
            "[{backend}] an unparseable ctx_id was reported visible"
        ));
    }

    // (f)+(g) THE INDEPENDENT ANCHOR. Everything above this point is
    // relational: it asserts that two expressions of §4.5 agree. `EXPECTED_BY_SPEC`
    // is the one check that does not — it is transcribed from the RFC by hand, so
    // it survives a change that moves every implementation together, and it names
    // the side that is wrong instead of inferring it.
    //
    // BOTH implementations are compared against it, and that is the whole point.
    // Comparing only the SQL would leave the inversion in place: a defect in
    // `retrieve_visible` would still surface as "the SQL disagrees".
    let row_id = |r: Row| -> String {
        match r {
            Row::Public => public.clone(),
            Row::PrivateWithAudience => private_with_audience.clone(),
            Row::PrivateOwnerOnly => private_owner_only.clone(),
            Row::RestrictedWithAudience => restricted_with_audience.clone(),
            Row::PrivateWithContributor => private_with_contributor.clone(),
            Row::PublicRetracted => public_retracted.clone(),
            Row::ForeignTenantPublic => foreign_tenant_public.clone(),
        }
    };
    // Failures report fixture ROLE names. A raw `ctx_id` carries a nonce and
    // tells the reader nothing about which rule was broken.
    let named: Vec<(&str, &str)> = vec![
        (public.as_str(), "public"),
        (private_with_audience.as_str(), "private_with_audience"),
        (private_owner_only.as_str(), "private_owner_only"),
        (
            restricted_with_audience.as_str(),
            "restricted_with_audience",
        ),
        (
            private_with_contributor.as_str(),
            "private_with_contributor",
        ),
        (public_retracted.as_str(), "public_retracted"),
        (foreign_tenant_public.as_str(), "foreign_tenant_public"),
    ];
    let label = |id: &str| -> String {
        named
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| (*n).to_string())
            .unwrap_or_else(|| format!("<not a fixture row: {id}>"))
    };

    let anchor_default = DefaultOnly(Arc::clone(store));
    for (who, tenant_scoped, expected_rows) in EXPECTED_BY_SPEC {
        let (requester, anon): (Option<&AgentDid>, bool) = match *who {
            Who::Producer => (Some(&owner), false),
            Who::Audience => (Some(&aud), false),
            Who::Contributor => (Some(&contrib), false),
            Who::Outsider => (Some(&outsider), false),
            Who::AnonReadsOn => (None, true),
            Who::AnonReadsOff => (None, false),
        };
        let scope = if *tenant_scoped {
            Some(tenant.as_str())
        } else {
            None
        };
        let expected: std::collections::HashSet<String> =
            expected_rows.iter().map(|r| row_id(*r)).collect();

        let via_sql = store
            .visible_ctx_ids(&ids, requester, scope, anon)
            .await
            .expect("override ok");
        let via_rust = anchor_default
            .visible_ctx_ids(&ids, requester, scope, anon)
            .await
            .expect("default ok");

        for (leg, got) in [
            ("this backend's SQL predicate", via_sql),
            ("the Rust `retrieve_visible` rule", via_rust),
        ] {
            if got == expected {
                continue;
            }
            let mut over: Vec<String> = got.difference(&expected).map(|i| label(i)).collect();
            let mut under: Vec<String> = expected.difference(&got).map(|i| label(i)).collect();
            over.sort();
            under.sort();
            violations.push(format!(
                "[{backend}] {leg} disagrees with RFC-ACDP-0008 §4.5 as transcribed in \
                 `EXPECTED_BY_SPEC`, for {who:?} (tenant scope {scope:?}).\n                     \
                 DISCLOSED BUT NOT AUTHORIZED BY THE SPEC (over-disclosure): {over:?}\n                     \
                 AUTHORIZED BY THE SPEC BUT WITHHELD (under-disclosure): {under:?}\n                     \
                 The named leg is the one that is wrong: the expectation is a hand \
                 transcription of the RFC and consults no implementation."
            ));
        }
    }

    // (e) SQL override vs the Rust rule. `DefaultOnly` wraps THIS store and
    // declines to override the method, so the default body answers for the very
    // rows the override just answered for.
    //
    // NOTE what this leg is and is not. It compares two expressions that are NOT
    // independent of the N-call differential above: the default body is
    // `retrieve_visible` + tenant, and `visible_by_n_calls` also calls
    // `retrieve_visible`. So (a) and (e) share a centre, and neither can see a
    // defect inside it — (f)/(g) above is what covers that. This leg is kept
    // because it still catches the thing it was written for: one backend's SQL
    // drifting away from the Rust rule.
    let default_only = DefaultOnly(Arc::clone(store));
    for (who, requester, anon) in [
        ("the producer", Some(&owner), false),
        ("an audience member", Some(&aud), false),
        ("an outsider", Some(&outsider), false),
        ("an anonymous caller, anon reads ON", None, true),
    ] {
        for scope in [None, Some(tenant.as_str())] {
            let via_override = store
                .visible_ctx_ids(&ids, requester, scope, anon)
                .await
                .expect("override ok");
            let via_default = default_only
                .visible_ctx_ids(&ids, requester, scope, anon)
                .await
                .expect("default ok");
            if via_override != via_default {
                let only_override: Vec<_> = via_override.difference(&via_default).collect();
                let only_default: Vec<_> = via_default.difference(&via_override).collect();
                violations.push(format!(
                    "[{backend}] the SQL override and the Rust DEFAULT disagree for {who} \
                     (tenant scope {scope:?}). The §4.5 rule is written in both places and they \
                     have drifted. Only the override disclosed: {only_override:?}. Only the \
                     default disclosed: {only_default:?}"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "batched visibility diverged from the retrieve contract:\n{}",
        violations.join("\n")
    );
}

/// **RFC-ACDP-0017 §10 — event receipts are minted, stored and served
/// identically by every durable backend.**
///
/// `plain` and `minting` MUST be two handles on the SAME database: `plain`
/// built without an event-receipt minter (a pre-upgrade registry, or one
/// with the profile off), `minting` with
/// `with_event_receipt_minter(<ReceiptSigner for did:web:reg.test>)`. The
/// builder is inherent to each backend, so the caller attaches it; `pubkey`
/// is that signer's Ed25519 public key.
///
/// Legs:
/// 1. **Fill-on-append (AC8).** Three events committed through `plain`
///    carry no receipt and `plain` never serves the member; a fourth through
///    `minting` receipts all four in one transaction. The fixture ends on a
///    republish so the context is the lineage head `current` serves.
/// 2. **Binding + signature (AC2).** Every served receipt passes
///    `check_binding` against its served event at its position, verifies
///    under `pubkey`, and `accepted_at` is non-decreasing from `created_at`.
/// 3. **Replay (AC3).** A byte-identical retry returns the same receipt
///    bytes and mints nothing.
/// 4. **Read parity (AC5).** `get`, `lineage` and `current` serve the same
///    array.
pub async fn assert_lifecycle_event_receipts<S>(
    plain: &Arc<S>,
    minting: &Arc<S>,
    pubkey: [u8; 32],
    label: &str,
) where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::registry::LifecycleCommitOutcome;

    const SEED: u8 = 252;
    let ctx_id = publish_titled(plain, SEED, "event receipt parity fixture").await;
    let ctx = CtxId(ctx_id.clone());
    let commit = |store: &Arc<S>, event: LifecycleEvent| {
        let s = Arc::clone(store);
        async move {
            tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
                .await
                .expect("commit task")
                .expect("lifecycle commit succeeds")
        }
    };
    let event = |n: u8, kind: LifecycleEventType| {
        // A canonical lowercase v4 UUID, unique per run: the context's own
        // UUID with its last hex digit replaced by `n`.
        let uuid = ctx_id.rsplit('/').next().expect("ctx_id ends in a UUID");
        let event_id = format!("{}{n:x}", &uuid[..uuid.len() - 1]);
        LifecycleEvent::new(
            event_id,
            ctx.clone(),
            kind,
            Utc::now(),
            AgentDid::new(agent_did(SEED)),
            None,
        )
        .expect("valid lifecycle event")
    };

    // Leg 1: a legacy prefix with no receipts, then fill-on-append.
    commit(plain, event(1, LifecycleEventType::Retracted)).await;
    commit(plain, event(2, LifecycleEventType::Republished)).await;
    let legacy = commit(plain, event(3, LifecycleEventType::Retracted)).await;
    let LifecycleCommitOutcome::Applied(legacy) = legacy else {
        panic!("[{label}] fixture is wrong: the third event must apply");
    };
    assert!(
        legacy.registry_state.event_receipts.is_none(),
        "[{label}] a store without a minter must never serve event_receipts"
    );
    let fourth = event(4, LifecycleEventType::Republished);
    let applied = match commit(minting, fourth.clone()).await {
        LifecycleCommitOutcome::Applied(c) => c,
        other => panic!("[{label}] the fourth event must apply, got {other:?}"),
    };

    // Leg 2: every receipt binds to its served event and verifies.
    let check = |c: &acdp::types::body::FullContext, what: &str| -> Vec<serde_json::Value> {
        verified_event_receipts(c, &ctx, pubkey, 4, &format!("{label}] [{what}"))
    };
    let on_commit = check(&applied, "commit response");

    // Leg 3: a byte-identical replay serves the stored receipts, mints none.
    let replay = match commit(minting, fourth).await {
        LifecycleCommitOutcome::IdempotentReplay(c) => c,
        other => panic!("[{label}] a byte-identical retry must replay, got {other:?}"),
    };
    assert_eq!(
        check(&replay, "replay"),
        on_commit,
        "[{label}] the replay must serve the stored receipts byte-for-byte"
    );

    // Leg 4: every read path serves the same array.
    let s = Arc::clone(minting);
    let c2 = ctx.clone();
    let got = tokio::task::spawn_blocking(move || s.get(&c2))
        .await
        .expect("get task")
        .expect("get")
        .expect("context exists");
    assert_eq!(check(&got, "get"), on_commit, "[{label}] get diverges");
    let lineage_id = got.body.lineage_id.clone();
    let s = Arc::clone(minting);
    let l2 = lineage_id.clone();
    let members = tokio::task::spawn_blocking(move || s.lineage(&l2))
        .await
        .expect("lineage task")
        .expect("lineage");
    let member = members
        .iter()
        .find(|m| m.body.ctx_id == ctx)
        .unwrap_or_else(|| panic!("[{label}] lineage omits the context"));
    assert_eq!(
        check(member, "lineage"),
        on_commit,
        "[{label}] lineage diverges"
    );
    let s = Arc::clone(minting);
    let current = tokio::task::spawn_blocking(move || s.current(&lineage_id))
        .await
        .expect("current task")
        .expect("current")
        .expect("lineage has a current member");
    assert_eq!(
        check(&current, "current"),
        on_commit,
        "[{label}] current diverges"
    );

    // `plain` reads the same rows but, minting off, never serves the member.
    let s = Arc::clone(plain);
    let got = tokio::task::spawn_blocking(move || s.get(&ctx))
        .await
        .expect("get task")
        .expect("get")
        .expect("context exists");
    assert!(
        got.registry_state.event_receipts.is_none(),
        "[{label}] a store without a minter must not serve stored receipts"
    );
}

/// The served `event_receipts` of `c`, each checked: parallel to
/// `lifecycle_events` with `expected` entries, `check_binding` OK at its
/// position, signed by `pubkey`, and `accepted_at` non-decreasing from
/// `created_at`. Returns the receipts as JSON for byte comparisons.
fn verified_event_receipts(
    c: &acdp::types::body::FullContext,
    ctx: &CtxId,
    pubkey: [u8; 32],
    expected: usize,
    what: &str,
) -> Vec<serde_json::Value> {
    use acdp::types::EventReceipt;

    let events = c.registry_state.lifecycle_events.as_deref().unwrap_or(&[]);
    let receipts = c
        .registry_state
        .event_receipts
        .clone()
        .unwrap_or_else(|| panic!("[{what}] event_receipts is absent"));
    assert_eq!(
        receipts.len(),
        events.len(),
        "[{what}] event_receipts is not parallel to lifecycle_events"
    );
    assert_eq!(events.len(), expected, "[{what}] unexpected event count");
    let mut prev = c.body.created_at;
    let mut raw = Vec::new();
    for (i, (r, e)) in receipts.iter().zip(events).enumerate() {
        let served = serde_json::to_value(e).expect("event serializes");
        r.check_binding(AUTHORITY, ctx, &served, i as u64, c.body.created_at)
            .unwrap_or_else(|err| panic!("[{what}] receipt {i} binding: {err}"));
        let value = serde_json::to_value(r).expect("receipt serializes");
        let hash = EventReceipt::preimage_hash_of_value(&value).expect("preimage hash");
        r.verify_signature_against_hash(&hash, Some(&pubkey), None)
            .unwrap_or_else(|err| panic!("[{what}] receipt {i} signature: {err}"));
        assert!(
            r.accepted_at >= prev,
            "[{what}] receipt {i} accepted_at {} precedes {prev}",
            r.accepted_at
        );
        prev = r.accepted_at;
        raw.push(value);
    }
    raw
}

/// Raw access to a backend's stored receipt slots, which the trait does not
/// expose: the sweep assertion needs to plant a hole and to prove rows were
/// left untouched. Implemented by each backend's own test suite.
#[async_trait::async_trait]
pub trait RawReceiptSlots: Send + Sync {
    /// The stored `receipt` of each of `ctx_id`'s events, in `seq` order.
    async fn receipt_slots(&self, ctx_id: &str) -> Vec<Option<serde_json::Value>>;
    /// Set `event_id`'s stored receipt to NULL.
    async fn clear_receipt(&self, event_id: &str);
}

/// **RFC-ACDP-0017 §10.2 — the startup sweep receipts legacy events
/// identically on every durable backend.**
///
/// `plain` and `minting` are two handles on the same database, as for
/// [`assert_lifecycle_event_receipts`]; `raw` reads and writes its slots.
/// Assertions are per fixture context, and the report is checked only as
/// lower bounds, so the kit is safe on a database shared with other tests
/// (Postgres). Exact report counts belong to a test on a fresh database.
///
/// Legs:
/// 1. A store without a minter sweeps nothing and touches no row.
/// 2. **AC1:** a context of legacy events only is fully receipted, in
///    order; each receipt binds, verifies, and `accepted_at` is
///    non-decreasing.
/// 3. **AC3:** a receipted prefix plus a NULL tail: only the tail is
///    filled, and the prefix bytes are unchanged.
/// 4. **AC2:** a NULL before a receipted slot is skipped: no row changes
///    and reads omit the member.
/// 5. A second sweep changes no row.
/// 6. **AC4:** a later append on the holed context leaves the hole,
///    receipts only the new event at its true index, and reads still omit.
pub async fn assert_event_receipt_sweep<S>(
    plain: &Arc<S>,
    minting: &Arc<S>,
    raw: &dyn RawReceiptSlots,
    pubkey: [u8; 32],
    label: &str,
) where
    S: ExtendedRegistryStore + 'static,
{
    let commit = |store: &Arc<S>, event: LifecycleEvent| {
        let s = Arc::clone(store);
        async move {
            tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
                .await
                .expect("commit task")
                .expect("lifecycle commit succeeds");
        }
    };
    let get = |store: &Arc<S>, ctx: &CtxId| {
        let s = Arc::clone(store);
        let ctx = ctx.clone();
        async move {
            tokio::task::spawn_blocking(move || s.get(&ctx))
                .await
                .expect("get task")
                .expect("get")
                .expect("context exists")
        }
    };
    let event = |ctx_id: &str, seed: u8, n: u8, kind: LifecycleEventType| {
        let uuid = ctx_id.rsplit('/').next().expect("ctx_id ends in a UUID");
        LifecycleEvent::new(
            format!("{}{n:x}", &uuid[..uuid.len() - 1]),
            CtxId(ctx_id.to_string()),
            kind,
            Utc::now(),
            AgentDid::new(agent_did(seed)),
            None,
        )
        .expect("valid lifecycle event")
    };
    use LifecycleEventType::{Republished, Retracted};

    // Fixtures. A: legacy only. B: receipted prefix, NULL tail. C: a hole.
    let a = publish_titled(plain, 247, "sweep legacy-only fixture").await;
    commit(plain, event(&a, 247, 1, Retracted)).await;
    commit(plain, event(&a, 247, 2, Republished)).await;
    let b = publish_titled(plain, 248, "sweep prefix-and-tail fixture").await;
    commit(minting, event(&b, 248, 1, Retracted)).await;
    commit(plain, event(&b, 248, 2, Republished)).await;
    let c = publish_titled(plain, 249, "sweep hole fixture").await;
    commit(minting, event(&c, 249, 1, Retracted)).await;
    commit(minting, event(&c, 249, 2, Republished)).await;
    raw.clear_receipt(&event(&c, 249, 1, Retracted).event_id)
        .await;

    let a_before = raw.receipt_slots(&a).await;
    let b_before = raw.receipt_slots(&b).await;
    let c_before = raw.receipt_slots(&c).await;
    assert_eq!(a_before, vec![None, None], "[{label}] fixture A");
    assert!(
        b_before[0].is_some() && b_before[1].is_none(),
        "[{label}] fixture B"
    );
    assert!(
        c_before[0].is_none() && c_before[1].is_some(),
        "[{label}] fixture C"
    );

    // Leg 1: no minter, no sweep.
    let report = plain.backfill_event_receipts().await.expect("plain sweep");
    assert_eq!(
        report,
        crate::SweepReport::default(),
        "[{label}] a store without a minter must sweep nothing"
    );
    assert_eq!(
        raw.receipt_slots(&a).await,
        a_before,
        "[{label}] plain sweep wrote"
    );

    let report = minting.backfill_event_receipts().await.expect("sweep");
    assert!(
        report.contexts_filled >= 2 && report.receipts_filled >= 3 && report.contexts_skipped >= 1,
        "[{label}] the report undercounts the fixtures: {report:?}"
    );

    // Leg 2 (AC1).
    let a_ctx = CtxId(a.clone());
    let got = get(minting, &a_ctx).await;
    verified_event_receipts(&got, &a_ctx, pubkey, 2, &format!("{label}] [legacy-only"));

    // Leg 3 (AC3).
    let b_ctx = CtxId(b.clone());
    let got = get(minting, &b_ctx).await;
    let served = verified_event_receipts(&got, &b_ctx, pubkey, 2, &format!("{label}] [tail"));
    let b_after = raw.receipt_slots(&b).await;
    assert_eq!(
        b_after[0], b_before[0],
        "[{label}] the receipted prefix was rewritten"
    );
    assert_eq!(
        b_after[0].as_ref(),
        Some(&served[0]),
        "[{label}] prefix not served as stored"
    );

    // Leg 4 (AC2).
    let c_ctx = CtxId(c.clone());
    assert_eq!(
        raw.receipt_slots(&c).await,
        c_before,
        "[{label}] the holed context was written"
    );
    assert!(
        get(minting, &c_ctx)
            .await
            .registry_state
            .event_receipts
            .is_none(),
        "[{label}] a holed context must omit event_receipts"
    );

    // Leg 5: idempotent rerun.
    let a_after = raw.receipt_slots(&a).await;
    let report = minting
        .backfill_event_receipts()
        .await
        .expect("second sweep");
    assert!(
        report.contexts_skipped >= 1,
        "[{label}] the hole was not re-skipped: {report:?}"
    );
    assert_eq!(
        raw.receipt_slots(&a).await,
        a_after,
        "[{label}] rerun rewrote A"
    );
    assert_eq!(
        raw.receipt_slots(&b).await,
        b_after,
        "[{label}] rerun rewrote B"
    );
    assert_eq!(
        raw.receipt_slots(&c).await,
        c_before,
        "[{label}] rerun wrote C"
    );

    // Leg 6 (AC4).
    commit(minting, event(&c, 249, 3, Retracted)).await;
    let slots = raw.receipt_slots(&c).await;
    assert_eq!(slots.len(), 3, "[{label}] the append was not stored");
    assert!(slots[0].is_none(), "[{label}] the append filled the hole");
    assert_eq!(
        slots[1], c_before[1],
        "[{label}] the append rewrote a receipt"
    );
    assert_eq!(
        slots[2].as_ref().and_then(|r| r["index"].as_u64()),
        Some(2),
        "[{label}] the new event must be receipted at its true index"
    );
    assert!(
        get(minting, &c_ctx)
            .await
            .registry_state
            .event_receipts
            .is_none(),
        "[{label}] a holed context must still omit event_receipts"
    );
}

/// Publish `req` through `store`, minting a registry receipt with `signer`
/// when given, and return the response.
async fn publish_req<S>(
    store: &Arc<S>,
    req: acdp::types::publish::PublishRequest,
    signer: Option<acdp::types::ReceiptSigner>,
) -> acdp::types::publish::PublishResponse
where
    S: ExtendedRegistryStore + 'static,
{
    let s = Arc::clone(store);
    let outcome = tokio::task::spawn_blocking(move || {
        let mint = signer.map(|signer| {
            move |body: &acdp::types::body::Body| {
                let receipt = signer.mint(
                    &body.ctx_id,
                    &body.lineage_id,
                    &body.origin_registry,
                    body.created_at,
                    &body.content_hash,
                    &format!("sha256:{}", "c".repeat(64)),
                )?;
                serde_json::to_value(receipt).map_err(acdp::error::AcdpError::from)
            }
        });
        s.commit_publish(PublishCommit {
            req: &req,
            authority: AUTHORITY,
            idempotency: None,
            tenant: None,
            receipt_minter: mint
                .as_ref()
                .map(|m| m as &(dyn Fn(&acdp::types::body::Body) -> _ + Send + Sync)),
            predecessor_admission: None,
        })
    })
    .await
    .expect("publish task")
    .expect("publish succeeds");
    match outcome {
        PublishCommitOutcome::Inserted(r) => r,
        PublishCommitOutcome::IdempotentReplay(_) => panic!("fixture publish must insert"),
    }
}

/// The feed rows past `after` about any of `ctx_ids`, in seq order.
async fn feed_rows_for<S>(store: &Arc<S>, after: u64, ctx_ids: &[&str]) -> Vec<crate::FeedRow>
where
    S: ExtendedRegistryStore + 'static,
{
    let page = store
        .changes_page(&crate::FeedQuery {
            after,
            ctx_ids: ctx_ids.iter().map(|s| s.to_string()).collect(),
            scan_limit: 1000,
            ..Default::default()
        })
        .await
        .expect("changes_page");
    assert!(page.reached_head, "fixture rows fit in one scan");
    page.rows
}

/// **RFC-ACDP-0018 §4/§5.3 — every durable backend appends the same feed
/// rows for the same changes, with the evidence `registry_state` serves.**
///
/// `plain` and `minting` are two handles on one database, as for
/// [`assert_lifecycle_event_receipts`]. Assertions are scoped to fixture
/// `ctx_id`s and to rows past the head read at the start, so the kit is
/// safe on a database shared with other tests (Postgres).
///
/// Legs:
/// 1. A first publish, with or without a registry receipt, appends nothing.
/// 2. A lifecycle event without an event receipt appends nothing; one with
///    a receipt appends one `lifecycle` row whose `event` and `receipt` are
///    byte-identical (as JSON) to what `get` then serves at that index.
/// 3. A superseding publish appends one `superseded` row: `ctx_id` is the
///    older version, `superseded_by` the new one, `agent_id` the older
///    version's producer, `receipt` the new version's registry receipt.
///    Without a registry receipt it appends nothing.
/// 4. A key revocation appends one `key_revocation` row whose `agent_id` is
///    `metadata.revoked_key_controller` when present, else the body's.
/// 5. Rows are strictly increasing in seq, the head covers them, every row
///    converts to a valid `Change`, and the filters agree with
///    [`crate::FeedQuery::matches`].
pub async fn assert_change_feed<S>(plain: &Arc<S>, minting: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::registry::LifecycleCommitOutcome;
    use acdp::types::change::ChangeKind;

    let signer = || {
        acdp::types::ReceiptSigner::new(
            SigningKey::from_bytes(&[91u8; 32]),
            "did:web:reg.test",
            "did:web:reg.test#receipt-key-1",
        )
        .expect("signer")
    };
    let start = plain.changes_head().await.expect("changes_head").head;

    // Leg 1: first publishes append nothing.
    let p = producer(230);
    let build = |title: &str| {
        p.publish_request()
            .title(title)
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid publish request")
    };
    let a = publish_req(plain, build("feed fixture a"), Some(signer())).await;
    let b = publish_req(plain, build("feed fixture b"), None).await;
    let (a_id, b_id) = (a.ctx_id.as_str().to_string(), b.ctx_id.as_str().to_string());
    assert!(
        feed_rows_for(plain, start, &[&a_id, &b_id])
            .await
            .is_empty(),
        "[{label}] a first publish must append no feed row"
    );

    // Leg 2: lifecycle rows only with an event receipt.
    let uuid = a_id.rsplit('/').next().expect("ctx_id ends in a UUID");
    let event = |n: u8, kind: LifecycleEventType| {
        LifecycleEvent::new(
            format!("{}{n:x}", &uuid[..uuid.len() - 1]),
            a.ctx_id.clone(),
            kind,
            Utc::now(),
            AgentDid::new(agent_did(230)),
            None,
        )
        .expect("valid lifecycle event")
    };
    let commit = |store: &Arc<S>, e: LifecycleEvent| {
        let s = Arc::clone(store);
        async move {
            tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&e))
                .await
                .expect("commit task")
                .expect("lifecycle commit succeeds")
        }
    };
    commit(plain, event(1, LifecycleEventType::Retracted)).await;
    assert!(
        feed_rows_for(plain, start, &[&a_id]).await.is_empty(),
        "[{label}] an event without a receipt must append no feed row"
    );
    let retry = event(2, LifecycleEventType::Republished);
    let LifecycleCommitOutcome::Applied(served) = commit(minting, retry.clone()).await else {
        panic!("[{label}] the republish must apply");
    };
    // A byte-identical retry replays and appends nothing.
    let LifecycleCommitOutcome::IdempotentReplay(_) = commit(minting, retry).await else {
        panic!("[{label}] the retry must replay");
    };
    let rows = feed_rows_for(plain, start, &[&a_id]).await;
    assert_eq!(
        rows.len(),
        1,
        "[{label}] one receipted event, one row: {rows:?}"
    );
    let row = &rows[0];
    let events = served.registry_state.lifecycle_events.clone().unwrap();
    let receipts = served.registry_state.event_receipts.clone().unwrap();
    assert_eq!(row.kind, ChangeKind::Lifecycle, "[{label}]");
    assert_eq!(row.lineage_id, served.body.lineage_id.as_str(), "[{label}]");
    assert_eq!(row.agent_id, agent_did(230), "[{label}]");
    assert_eq!(
        row.event.as_ref(),
        Some(&serde_json::to_value(&events[1]).unwrap()),
        "[{label}] the row's event must be the served event"
    );
    assert_eq!(
        row.receipt,
        serde_json::to_value(&receipts[1]).unwrap(),
        "[{label}] the row's receipt must be the served receipt"
    );

    // Leg 3: supersession.
    let a_body = served.body.clone();
    let succ = |title: &str| {
        p.supersede_body(&a_body)
            .title(title)
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid successor request")
    };
    let a2 = publish_req(plain, succ("feed fixture a v2"), Some(signer())).await;
    let a2_id = a2.ctx_id.as_str().to_string();
    let rows = feed_rows_for(plain, start, &[&a_id, &a2_id]).await;
    assert_eq!(rows.len(), 2, "[{label}] lifecycle + superseded: {rows:?}");
    let sup = &rows[1];
    assert_eq!(sup.kind, ChangeKind::Superseded, "[{label}]");
    assert_eq!(sup.ctx_id, a_id, "[{label}] ctx_id is the older version");
    assert_eq!(
        sup.superseded_by.as_deref(),
        Some(a2_id.as_str()),
        "[{label}]"
    );
    assert_eq!(sup.agent_id, agent_did(230), "[{label}]");
    assert_eq!(sup.lineage_id, a_body.lineage_id.as_str(), "[{label}]");
    assert_eq!(
        Some(&sup.receipt),
        a2.registry_receipt.as_ref(),
        "[{label}] the superseded row carries the new version's receipt"
    );
    let b_body = plain.get(&b.ctx_id).expect("get").expect("b exists").body;
    let q = producer(231);
    let b2 = publish_req(
        plain,
        p.supersede_body(&b_body)
            .title("feed fixture b v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid successor request"),
        None,
    )
    .await;
    assert!(
        feed_rows_for(plain, start, &[&b_id, b2.ctx_id.as_str()])
            .await
            .is_empty(),
        "[{label}] a supersession without a registry receipt appends nothing"
    );

    // Leg 4: key revocations, with and without the declared controller.
    // The second supersedes the first, so it appends two rows: superseded
    // first, then key_revocation.
    let r1 = publish_req(
        plain,
        q.publish_request()
            .title("feed fixture revocation")
            .context_type(ContextType::KeyRevocation)
            .visibility(Visibility::Public)
            .build()
            .expect("valid revocation request"),
        Some(signer()),
    )
    .await;
    let r1_body = plain.get(&r1.ctx_id).expect("get").expect("r1").body;
    let r2 = publish_req(
        plain,
        q.supersede_body(&r1_body)
            .title("feed fixture revocation v2")
            .context_type(ContextType::KeyRevocation)
            .visibility(Visibility::Public)
            .metadata(serde_json::json!({
                "revoked_key_controller": agent_did(232),
            }))
            .build()
            .expect("valid revocation request"),
        Some(signer()),
    )
    .await;
    let rows = feed_rows_for(plain, start, &[r1.ctx_id.as_str(), r2.ctx_id.as_str()]).await;
    assert_eq!(
        rows.len(),
        3,
        "[{label}] revocation, then supersede + revocation: {rows:?}"
    );
    assert_eq!(
        rows[1].kind,
        ChangeKind::Superseded,
        "[{label}] superseded first"
    );
    assert_eq!(rows[1].ctx_id, r1.ctx_id.as_str(), "[{label}]");
    assert_eq!(
        rows[1].superseded_by.as_deref(),
        Some(r2.ctx_id.as_str()),
        "[{label}]"
    );
    for (row, resp, controller) in [
        (&rows[0], &r1, agent_did(231)),
        (&rows[2], &r2, agent_did(232)),
    ] {
        assert_eq!(row.kind, ChangeKind::KeyRevocation, "[{label}]");
        assert_eq!(row.ctx_id, resp.ctx_id.as_str(), "[{label}]");
        assert_eq!(row.agent_id, controller, "[{label}] revoked key controller");
        assert_eq!(
            Some(&row.receipt),
            resp.registry_receipt.as_ref(),
            "[{label}]"
        );
    }

    // Leg 5: ordering, head, conversion and filters.
    let ids = [
        a_id.as_str(),
        a2_id.as_str(),
        r1.ctx_id.as_str(),
        r2.ctx_id.as_str(),
    ];
    let all = feed_rows_for(plain, start, &ids).await;
    assert_eq!(all.len(), 5, "[{label}] {all:?}");
    assert!(
        all.windows(2).all(|w| w[0].seq < w[1].seq),
        "[{label}] seq must be strictly increasing in append order"
    );
    let head = plain.changes_head().await.expect("changes_head");
    assert!(
        head.head >= all.last().unwrap().seq && head.pruned_through <= start,
        "[{label}] the head covers every row: {head:?}"
    );
    for row in &all {
        let change = row
            .to_change()
            .unwrap_or_else(|e| panic!("[{label}] row {} is not servable: {e}", row.seq));
        assert_eq!(change.seq(), row.seq, "[{label}]");
        assert_stored_hashes(row, label);
    }
    let lineage_lifecycle = plain
        .changes_page(&crate::FeedQuery {
            after: start,
            lineage_ids: vec![a_body.lineage_id.as_str().to_string()],
            kinds: vec![ChangeKind::Lifecycle],
            scan_limit: 1000,
            ..Default::default()
        })
        .await
        .expect("changes_page");
    assert_eq!(
        lineage_lifecycle
            .rows
            .iter()
            .map(|r| r.seq)
            .collect::<Vec<_>>(),
        vec![all[0].seq],
        "[{label}] lineage AND kind filter"
    );
    let revocations = plain
        .changes_page(&crate::FeedQuery {
            after: start,
            revocation_agents: vec![agent_did(232)],
            scan_limit: 1000,
            ..Default::default()
        })
        .await
        .expect("changes_page");
    assert_eq!(
        revocations.rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![all[4].seq],
        "[{label}] revocation-agent filter matches only key_revocation rows"
    );
    // A page capped at one row stops at that row, not the head.
    let capped = plain
        .changes_page(&crate::FeedQuery {
            after: start,
            ctx_ids: ids.iter().map(|s| s.to_string()).collect(),
            scan_limit: 1,
            ..Default::default()
        })
        .await
        .expect("changes_page");
    assert_eq!(capped.rows.len(), 1, "[{label}]");
    assert!(
        !capped.reached_head,
        "[{label}] a full page has not reached the head"
    );
    assert_eq!(capped.last_scanned, all[0].seq, "[{label}]");

    // Leg 6: an idempotent retry of a superseding publish appends nothing.
    let a2_body = plain.get(&a2.ctx_id).expect("get").expect("a2").body;
    let a3_req = p
        .supersede_body(&a2_body)
        .title("feed fixture a v3")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid successor request");
    let key = format!("feed-idem-{}", &uuid[..8]);
    let publish_keyed = |req: acdp::types::publish::PublishRequest| {
        let s = Arc::clone(plain);
        let key = key.clone();
        let signer = signer();
        async move {
            tokio::task::spawn_blocking(move || {
                let mint = move |body: &acdp::types::body::Body| {
                    let receipt = signer.mint(
                        &body.ctx_id,
                        &body.lineage_id,
                        &body.origin_registry,
                        body.created_at,
                        &body.content_hash,
                        &format!("sha256:{}", "c".repeat(64)),
                    )?;
                    serde_json::to_value(receipt).map_err(acdp::error::AcdpError::from)
                };
                s.commit_publish(PublishCommit {
                    req: &req,
                    authority: AUTHORITY,
                    idempotency: Some(acdp::registry::store::PendingIdempotencyCommit {
                        key: &key,
                        ttl: chrono::Duration::hours(1),
                    }),
                    tenant: None,
                    receipt_minter: Some(&mint),
                    predecessor_admission: None,
                })
            })
            .await
            .expect("publish task")
            .expect("publish succeeds")
        }
    };
    let first = publish_keyed(a3_req.clone()).await;
    let PublishCommitOutcome::Inserted(a3) = first else {
        panic!("[{label}] the first keyed publish must insert");
    };
    let before = feed_rows_for(plain, start, &[&a2_id, a3.ctx_id.as_str()]).await;
    assert_eq!(
        before.len(),
        1,
        "[{label}] one superseded row for v2: {before:?}"
    );
    let again = publish_keyed(a3_req).await;
    assert!(
        matches!(again, PublishCommitOutcome::IdempotentReplay(_)),
        "[{label}] the keyed retry must replay"
    );
    assert_eq!(
        feed_rows_for(plain, start, &[&a2_id, a3.ctx_id.as_str()]).await,
        before,
        "[{label}] an idempotent retry must append no row"
    );
}

/// A stored row's §6.1 bucket hashes are over the ids it serves.
fn assert_stored_hashes(row: &crate::FeedRow, label: &str) {
    use crate::change_bucket::bucket_hash;
    let change = row
        .to_change()
        .unwrap_or_else(|e| panic!("[{label}] row {} is not servable: {e}", row.seq));
    assert_eq!(
        row.lineage_hash,
        bucket_hash(change.lineage_id().as_str()),
        "[{label}] row {} lineage_hash",
        row.seq
    );
    assert_eq!(
        row.agent_hash,
        bucket_hash(change.agent_id().as_str()),
        "[{label}] row {} agent_hash",
        row.seq
    );
}

/// **RFC-ACDP-0018 §6.1 — every backend serves bucket filters the same
/// way.**
///
/// Producer P publishes candidates until two lineages La and Lb share their
/// first hash digit but not their second, then supersedes both (rows S_a,
/// S_b, agent P). Producer Q then publishes a key revocation whose
/// controller is P (row K, agent P, its own lineage). Assertions are scoped
/// to those three rows past the start head, as for [`assert_change_feed`];
/// rows written concurrently by other tests (Postgres) may share a bucket,
/// so every returned row is checked against [`crate::FeedQuery::matches`]
/// and its stored hashes instead of being counted.
///
/// Legs:
/// 1. Every returned row carries hashes over the ids it serves.
/// 2. A 16-digit lineage bucket returns its lineage's row only; the shared
///    1-digit bucket returns both; the 2-digit bucket of the lower hash
///    excludes the other lineage, which sorts just above its range (the
///    range's upper bound decides it).
/// 3. An agent bucket hears P's `key_revocation` by controller, and `kind`
///    is AND'd with the buckets.
/// 4. Buckets are OR'd with `ctx_id`, `lineage_id` and `agent_id`.
/// 5. A lineage bucket does NOT pull in the key revocation of the watched
///    lineages' producer: §6.1 has no counterpart of the `ctx_id` rule.
/// 6. Overlapping buckets return each row once, and a bucket nothing
///    matches returns nothing.
pub async fn assert_change_feed_bucket_filter<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use crate::change_bucket::{bucket_hash, Bucket};
    use acdp::types::change::{bucket, ChangeKind};

    let signer = || {
        acdp::types::ReceiptSigner::new(
            SigningKey::from_bytes(&[92u8; 32]),
            "did:web:reg.test",
            "did:web:reg.test#receipt-key-1",
        )
        .expect("signer")
    };
    let start = store.changes_head().await.expect("changes_head").head;
    let p = producer(238);
    let q = producer(239);

    // Two lineages sharing hash digit 1 and differing at digit 2: about
    // one candidate pair in 16 shares the first digit, so 64 is ample.
    let mut seen: Vec<(String, acdp::types::publish::PublishResponse)> = Vec::new();
    let mut pair = None;
    for i in 0..64 {
        let r = publish_req(
            store,
            p.publish_request()
                .title(format!("bucket candidate {i}"))
                .context_type(ContextType::DataSnapshot)
                .visibility(Visibility::Public)
                .build()
                .expect("valid publish request"),
            None,
        )
        .await;
        let h = bucket_hash(r.lineage_id.as_str());
        if let Some(j) = seen
            .iter()
            .position(|(o, _)| o[..1] == h[..1] && o[1..2] != h[1..2])
        {
            pair = Some((seen.swap_remove(j).1, r));
            break;
        }
        seen.push((h, r));
    }
    let Some((a, b)) = pair else {
        panic!(
            "[{label}] no two of 64 lineages share a first hash digit: {:?}",
            seen.iter().map(|(h, _)| &h[..2]).collect::<Vec<_>>()
        );
    };
    let supersede = |r: &acdp::types::publish::PublishResponse| {
        let body = store.get(&r.ctx_id).expect("get").expect("candidate").body;
        p.supersede_body(&body)
            .title("bucket successor")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid successor request")
    };
    publish_req(store, supersede(&a), Some(signer())).await;
    publish_req(store, supersede(&b), Some(signer())).await;
    let k = publish_req(
        store,
        q.publish_request()
            .title("bucket revocation")
            .context_type(ContextType::KeyRevocation)
            .visibility(Visibility::Public)
            .metadata(serde_json::json!({ "revoked_key_controller": agent_did(238) }))
            .build()
            .expect("valid revocation request"),
        Some(signer()),
    )
    .await;
    let rows = feed_rows_for(
        store,
        start,
        &[a.ctx_id.as_str(), b.ctx_id.as_str(), k.ctx_id.as_str()],
    )
    .await;
    assert_eq!(rows.len(), 3, "[{label}] S_a, S_b, K: {rows:?}");
    let (s_a, s_b, rev) = (rows[0].seq, rows[1].seq, rows[2].seq);
    assert_eq!(
        (rows[0].kind, rows[1].kind, rows[2].kind),
        (
            ChangeKind::Superseded,
            ChangeKind::Superseded,
            ChangeKind::KeyRevocation
        ),
        "[{label}]"
    );
    assert_eq!(
        rows[2].agent_id,
        agent_did(238),
        "[{label}] K is P's revocation"
    );
    let fixture = [s_a, s_b, rev];
    let (la, lb) = (a.lineage_id.as_str(), b.lineage_id.as_str());
    let lr = rows[2].lineage_id.clone();

    let lin = |v: &str, n: usize| Bucket::parse(&bucket(v, n), 16).expect("valid bucket");
    // The fixture rows `query` returns, after checking every returned row.
    let run = |query: crate::FeedQuery| async move {
        let page = store
            .changes_page(&crate::FeedQuery {
                after: start,
                scan_limit: 1000,
                ..query.clone()
            })
            .await
            .expect("changes_page");
        assert!(page.reached_head, "[{label}] fixture rows fit in one scan");
        assert!(
            page.rows.windows(2).all(|w| w[0].seq < w[1].seq),
            "[{label}] each row once, ascending"
        );
        for row in &page.rows {
            assert!(
                query.matches(row),
                "[{label}] row {} does not match {query:?}",
                row.seq
            );
            assert_stored_hashes(row, label);
        }
        page.rows
            .iter()
            .map(|r| r.seq)
            .filter(|s| fixture.contains(s))
            .collect::<Vec<_>>()
    };
    let by = |lineage: Vec<Bucket>, agent: Vec<Bucket>| crate::FeedQuery {
        lineage_buckets: lineage,
        agent_buckets: agent,
        ..Default::default()
    };

    // Leg 2: whole buckets, narrowing, and the upper bound.
    assert_eq!(
        run(by(vec![lin(la, 16)], vec![])).await,
        vec![s_a],
        "[{label}] La/16"
    );
    assert_eq!(
        run(by(vec![lin(lb, 16)], vec![])).await,
        vec![s_b],
        "[{label}] Lb/16"
    );
    let shared = run(by(vec![lin(la, 1)], vec![])).await;
    assert!(
        shared.starts_with(&[s_a, s_b]) && shared.len() <= 3,
        "[{label}] a 1-digit bucket is the whole bucket: {shared:?}"
    );
    let (lower, upper_seq) = if bucket_hash(la) < bucket_hash(lb) {
        (la, s_b)
    } else {
        (lb, s_a)
    };
    let narrow = run(by(vec![lin(lower, 2)], vec![])).await;
    assert!(
        !narrow.contains(&upper_seq) && narrow.contains(&(s_a + s_b - upper_seq)),
        "[{label}] the lineage just above a 2-digit range stays out: {narrow:?}"
    );

    // Leg 3: agent buckets hear revocations by controller; kind is AND'd.
    assert_eq!(
        run(by(vec![], vec![lin(&agent_did(238), 16)])).await,
        vec![s_a, s_b, rev],
        "[{label}] P's agent bucket: both supersessions and the revocation"
    );
    assert_eq!(
        run(crate::FeedQuery {
            kinds: vec![ChangeKind::KeyRevocation],
            ..by(vec![lin(la, 16)], vec![lin(&agent_did(238), 16)])
        })
        .await,
        vec![rev],
        "[{label}] buckets AND kind"
    );
    assert!(
        run(by(vec![], vec![lin(&agent_did(239), 16)]))
            .await
            .is_empty(),
        "[{label}] Q published the revocation but controls none of these rows"
    );

    // Leg 4: OR with the id arms.
    assert_eq!(
        run(crate::FeedQuery {
            ctx_ids: vec![b.ctx_id.as_str().to_string()],
            ..by(vec![lin(la, 16)], vec![])
        })
        .await,
        vec![s_a, s_b],
        "[{label}] bucket OR ctx_id"
    );
    assert_eq!(
        run(crate::FeedQuery {
            lineage_ids: vec![lr.clone()],
            ..by(vec![lin(la, 16)], vec![])
        })
        .await,
        vec![s_a, rev],
        "[{label}] bucket OR lineage_id"
    );
    assert_eq!(
        run(crate::FeedQuery {
            agent_ids: vec![agent_did(238)],
            kinds: vec![ChangeKind::KeyRevocation],
            ..by(vec![lin(lb, 16)], vec![])
        })
        .await,
        vec![rev],
        "[{label}] bucket OR agent_id, AND kind"
    );

    // Leg 5: no ctx_id-style revocation rule for buckets. The revocation
    // of P (the watched lineages' producer) is not pulled in; the same
    // watch through `revocation_agents` (what a ctx_id watch resolves to)
    // is.
    assert_eq!(
        run(by(vec![lin(la, 16), lin(lb, 16)], vec![])).await,
        vec![s_a, s_b],
        "[{label}] a lineage bucket must not match its producer's key revocation"
    );
    assert_eq!(
        run(crate::FeedQuery {
            revocation_agents: vec![agent_did(238)],
            ..by(vec![lin(la, 16), lin(lb, 16)], vec![])
        })
        .await,
        vec![s_a, s_b, rev],
        "[{label}] contrast: the ctx_id rule's revocation arm"
    );

    // Leg 6: overlap and an empty bucket. A 16-digit bucket of a value no
    // row carries is empty but for a 2^-64 collision.
    assert_eq!(
        run(by(vec![lin(la, 1), lin(la, 4), lin(la, 16)], vec![])).await,
        run(by(vec![lin(la, 1)], vec![])).await,
        "[{label}] overlapping buckets are one OR"
    );
    assert!(
        run(by(
            vec![lin("bucket parity: no row has this id", 16)],
            vec![]
        ))
        .await
        .is_empty(),
        "[{label}] a bucket no fixture row is in"
    );
}

/// **RFC-ACDP-0018 §5.3 (rec-008) — a feed row commits with its change or
/// not at all.** Durable backends only: they append inside the change's
/// transaction. A key revocation whose `revoked_key_controller` is not a
/// DID fails the append, which must roll back the whole publish — no
/// context, no superseded predecessor, no row.
pub async fn assert_change_feed_rolls_back_with_its_change<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    let signer = || {
        acdp::types::ReceiptSigner::new(
            SigningKey::from_bytes(&[91u8; 32]),
            "did:web:reg.test",
            "did:web:reg.test#receipt-key-1",
        )
        .expect("signer")
    };
    let start = store.changes_head().await.expect("changes_head").head;
    let p = producer(236);
    let v1 = publish_req(
        store,
        p.publish_request()
            .title("feed rollback fixture")
            .context_type(ContextType::KeyRevocation)
            .visibility(Visibility::Public)
            .build()
            .expect("valid request"),
        Some(signer()),
    )
    .await;
    let v1_body = store.get(&v1.ctx_id).expect("get").expect("v1").body;
    let bad = p
        .supersede_body(&v1_body)
        .title("feed rollback fixture v2")
        .context_type(ContextType::KeyRevocation)
        .visibility(Visibility::Public)
        .metadata(serde_json::json!({ "revoked_key_controller": 7 }))
        .build()
        .expect("valid request");
    let s = Arc::clone(store);
    let signer = signer();
    let err = tokio::task::spawn_blocking(move || {
        let mint = move |body: &acdp::types::body::Body| {
            let receipt = signer.mint(
                &body.ctx_id,
                &body.lineage_id,
                &body.origin_registry,
                body.created_at,
                &body.content_hash,
                &format!("sha256:{}", "c".repeat(64)),
            )?;
            serde_json::to_value(receipt).map_err(acdp::error::AcdpError::from)
        };
        s.commit_publish(PublishCommit {
            req: &bad,
            authority: AUTHORITY,
            idempotency: None,
            tenant: None,
            receipt_minter: Some(&mint),
            predecessor_admission: None,
        })
    })
    .await
    .expect("publish task")
    .expect_err("an unservable row must fail the publish");
    assert!(
        matches!(&err, acdp::error::AcdpError::SchemaViolation(m)
            if m.contains("revoked_key_controller")),
        "[{label}] the feed append, not an earlier check, must refuse it: {err:?}"
    );
    let v1_now = store.get(&v1.ctx_id).expect("get").expect("v1");
    assert_eq!(
        v1_now.registry_state.status,
        Status::Active,
        "[{label}] the predecessor must not stay superseded by a rolled-back publish"
    );
    let rows = feed_rows_for(store, start, &[v1.ctx_id.as_str()]).await;
    assert_eq!(
        rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
        vec![acdp::types::change::ChangeKind::KeyRevocation],
        "[{label}] only v1's own row: the failed publish left none"
    );
    let s = Arc::clone(store);
    let lineage = v1_body.lineage_id.clone();
    let members = tokio::task::spawn_blocking(move || s.lineage(&lineage))
        .await
        .expect("lineage task")
        .expect("lineage");
    assert_eq!(
        members.len(),
        1,
        "[{label}] the failed successor was not stored"
    );
}

/// **RFC-ACDP-0018 §5.4 — pruning removes an old seq prefix and moves the
/// marker; it never reorders or renumbers.** Needs a store no other test
/// writes to (it prunes the whole feed).
pub async fn assert_change_feed_prune<S>(plain: &Arc<S>, minting: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    let empty = plain.changes_head().await.expect("changes_head");
    assert_eq!(
        (empty.head, empty.pruned_through),
        (0, 0),
        "[{label}] a fresh feed is empty"
    );
    assert_eq!(
        (empty.head_fp, empty.pruned_through_fp),
        (Some(crate::FEED_ORIGIN_FP), None),
        "[{label}] an empty feed's head is the origin"
    );
    assert_eq!(
        plain.prune_changes(i64::MAX, 100).await.expect("prune"),
        0,
        "[{label}] pruning an empty feed deletes nothing"
    );
    let mut ids = Vec::new();
    for seed in 233..=235u8 {
        let ctx_id = publish_titled(plain, seed, "prune fixture").await;
        let uuid = ctx_id.rsplit('/').next().unwrap().to_string();
        let event = LifecycleEvent::new(
            uuid,
            CtxId(ctx_id.clone()),
            LifecycleEventType::Retracted,
            Utc::now(),
            AgentDid::new(agent_did(seed)),
            None,
        )
        .unwrap();
        let s = Arc::clone(minting);
        tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
            .await
            .unwrap()
            .expect("retract");
        ids.push(ctx_id);
    }
    let head = plain.changes_head().await.unwrap();
    assert_eq!(head.head, 3, "[{label}]");
    // §5.3 anchors: every backend fingerprints the same columns the same way.
    let all = plain
        .changes_page(&crate::FeedQuery {
            after: 0,
            scan_limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    let fp: Vec<_> = all.rows.iter().map(crate::FeedRow::fingerprint).collect();
    assert_eq!(fp.len(), 3, "[{label}]");
    assert_eq!(
        head.head_fp,
        Some(fp[2]),
        "[{label}] the head row's fingerprint"
    );
    assert_eq!(all.head_fp, head.head_fp, "[{label}] a page carries it too");
    assert_eq!(
        plain.prune_changes(0, 100).await.unwrap(),
        0,
        "[{label}] nothing is older than the epoch"
    );
    assert_eq!(
        plain.prune_changes(i64::MAX, 2).await.unwrap(),
        2,
        "[{label}] the batch bounds a prune"
    );
    let after = plain.changes_head().await.unwrap();
    assert_eq!((after.head, after.pruned_through), (3, 2), "[{label}]");
    assert_eq!(
        (after.head_fp, after.pruned_through_fp),
        (Some(fp[2]), Some(fp[1])),
        "[{label}] the prune recorded the boundary row's fingerprint"
    );
    let page = plain
        .changes_page(&crate::FeedQuery {
            after: 0,
            scan_limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![3],
        "[{label}] the survivor keeps its seq"
    );
    assert_eq!(
        plain.prune_changes(i64::MAX, 100).await.unwrap(),
        1,
        "[{label}]"
    );
    let drained = plain.changes_head().await.unwrap();
    assert_eq!(
        (drained.head, drained.pruned_through),
        (3, 3),
        "[{label}] an emptied feed keeps its head: positions are never reused"
    );
    assert_eq!(
        (drained.head_fp, drained.pruned_through_fp),
        (Some(fp[2]), Some(fp[2])),
        "[{label}] a drained head keeps its fingerprint through the marker"
    );
    // The next append continues past the pruned positions.
    let ctx_id = &ids[0];
    let uuid = ctx_id.rsplit('/').next().unwrap();
    let event = LifecycleEvent::new(
        // A fresh event_id: the context's UUID with a different last digit.
        format!(
            "{}{}",
            &uuid[..uuid.len() - 1],
            if uuid.ends_with('0') { '1' } else { '0' }
        ),
        CtxId(ctx_id.clone()),
        LifecycleEventType::Republished,
        Utc::now(),
        AgentDid::new(agent_did(233)),
        None,
    )
    .unwrap();
    let s = Arc::clone(minting);
    tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
        .await
        .unwrap()
        .expect("republish");
    let page = plain
        .changes_page(&crate::FeedQuery {
            after: 3,
            scan_limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1, "[{label}]");
    assert!(
        page.rows[0].seq > 3,
        "[{label}] seq is never reused after a prune"
    );
    assert_eq!(
        plain.changes_head().await.unwrap().head_fp,
        Some(page.rows[0].fingerprint()),
        "[{label}] the new head row anchors cursors again"
    );
}

/// **RFC-ACDP-0018 §5.2 — `seq` order is commit order.** Writers commit
/// lifecycle events on several contexts at once while a reader polls with
/// `after` = the last `seq` it saw. The reader must never see a `seq` at or
/// below one it already passed, and must end up with exactly the rows a
/// full scan returns: a row that commits under a lower `seq` after the
/// reader moved past it would be skipped for good.
pub async fn assert_change_feed_commit_order<S>(plain: &Arc<S>, minting: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    const CONTEXTS: usize = 8;
    const EVENTS: u8 = 24;
    let start = plain.changes_head().await.expect("changes_head").head;
    let p = producer(237);
    let mut ctxs = Vec::new();
    for i in 0..CONTEXTS {
        let resp = publish_req(
            plain,
            p.publish_request()
                .title(format!("feed order fixture {i}"))
                .context_type(ContextType::DataSnapshot)
                .visibility(Visibility::Public)
                .build()
                .expect("valid publish request"),
            None,
        )
        .await;
        ctxs.push(resp.ctx_id);
    }
    let ids: Vec<String> = ctxs.iter().map(|c| c.as_str().to_string()).collect();

    let writers: Vec<_> = ctxs
        .iter()
        .cloned()
        .map(|ctx_id| {
            let s = Arc::clone(minting);
            tokio::task::spawn_blocking(move || {
                let uuid = ctx_id
                    .as_str()
                    .rsplit('/')
                    .next()
                    .expect("uuid")
                    .to_string();
                for n in 1..=EVENTS {
                    let kind = if n % 2 == 1 {
                        LifecycleEventType::Retracted
                    } else {
                        LifecycleEventType::Republished
                    };
                    let event = LifecycleEvent::new(
                        format!("{}{n:02x}", &uuid[..uuid.len() - 2]),
                        ctx_id.clone(),
                        kind,
                        Utc::now(),
                        AgentDid::new(agent_did(237)),
                        None,
                    )
                    .expect("valid lifecycle event");
                    s.commit_lifecycle_event(&event)
                        .expect("lifecycle commit succeeds");
                }
            })
        })
        .collect();

    let poll = |after: u64| {
        let q = crate::FeedQuery {
            after,
            ctx_ids: ids.clone(),
            scan_limit: 1000,
            ..Default::default()
        };
        let s = Arc::clone(plain);
        async move { s.changes_page(&q).await.expect("changes_page").rows }
    };
    let mut seen: Vec<crate::FeedRow> = Vec::new();
    let mut after = start;
    let mut absorb = |rows: Vec<crate::FeedRow>, after: &mut u64| {
        for row in rows {
            assert!(
                row.seq > *after,
                "[{label}] seq {} surfaced after the reader passed {}",
                row.seq,
                *after
            );
            *after = row.seq;
            seen.push(row);
        }
    };
    while writers.iter().any(|w| !w.is_finished()) {
        absorb(poll(after).await, &mut after);
        tokio::task::yield_now().await;
    }
    for w in writers {
        w.await.expect("writer task");
    }
    absorb(poll(after).await, &mut after);

    let full = poll(start).await;
    assert_eq!(
        full.len(),
        CONTEXTS * usize::from(EVENTS),
        "[{label}] one row per committed event"
    );
    assert_eq!(
        seen, full,
        "[{label}] the incremental reader must see exactly the full scan"
    );
}

/// **RFC-ACDP-0018 §4 — every receipted lifecycle event is one row, in
/// order, carrying exactly what `registry_state` serves at its index.**
/// Three receipted events (retract, republish, retract) on one context must
/// yield three `lifecycle` rows, row *i* matching `lifecycle_events[i]` and
/// `event_receipts[i]` of the context as `get` serves it afterwards. Also
/// pins [`ExtendedRegistryStore::producers_of`]: a known id maps to its
/// producer; an unknown or unparseable id is omitted.
pub async fn assert_change_feed_lifecycle_sequence<S>(plain: &Arc<S>, minting: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::registry::LifecycleCommitOutcome;
    use acdp::types::change::ChangeKind;

    let start = plain.changes_head().await.expect("changes_head").head;
    let p = producer(233);
    let signer = acdp::types::ReceiptSigner::new(
        SigningKey::from_bytes(&[91u8; 32]),
        "did:web:reg.test",
        "did:web:reg.test#receipt-key-1",
    )
    .expect("signer");
    let a = publish_req(
        plain,
        p.publish_request()
            .title("feed sequence fixture")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid publish request"),
        Some(signer),
    )
    .await;
    let a_id = a.ctx_id.as_str().to_string();
    let uuid = a_id.rsplit('/').next().expect("ctx_id ends in a UUID");
    let mut last = None;
    for (n, kind) in [
        LifecycleEventType::Retracted,
        LifecycleEventType::Republished,
        LifecycleEventType::Retracted,
    ]
    .into_iter()
    .enumerate()
    {
        // Distinct, strictly later `occurred_at` per event.
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        let e = LifecycleEvent::new(
            format!("{}{n:x}", &uuid[..uuid.len() - 1]),
            a.ctx_id.clone(),
            kind,
            Utc::now(),
            AgentDid::new(agent_did(233)),
            None,
        )
        .expect("valid lifecycle event");
        let s = Arc::clone(minting);
        let out = tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&e))
            .await
            .expect("commit task")
            .expect("lifecycle commit succeeds");
        let LifecycleCommitOutcome::Applied(ctx) = out else {
            panic!("[{label}] event {n} must apply");
        };
        last = Some(ctx);
    }
    let served = last.expect("three events applied");
    let events = served.registry_state.lifecycle_events.clone().unwrap();
    let receipts = served.registry_state.event_receipts.clone().unwrap();
    assert_eq!(
        (events.len(), receipts.len()),
        (3, 3),
        "[{label}] three events served"
    );
    let rows = feed_rows_for(plain, start, &[&a_id]).await;
    assert_eq!(rows.len(), 3, "[{label}] one row per event: {rows:?}");
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.kind, ChangeKind::Lifecycle, "[{label}] row {i}");
        assert_eq!(row.ctx_id, a_id, "[{label}] row {i}");
        assert_eq!(
            row.event.as_ref(),
            Some(&serde_json::to_value(&events[i]).unwrap()),
            "[{label}] row {i} must carry lifecycle_events[{i}]"
        );
        assert_eq!(
            row.receipt,
            serde_json::to_value(&receipts[i]).unwrap(),
            "[{label}] row {i} must carry event_receipts[{i}]"
        );
    }
    assert!(
        rows.windows(2).all(|w| w[0].seq < w[1].seq),
        "[{label}] rows follow event order"
    );

    let unknown = format!("acdp://{AUTHORITY}/00000000-0000-4000-8000-00000000f00d");
    let producers = plain
        .producers_of(&[a_id.as_str(), unknown.as_str(), "not a ctx id"])
        .await
        .expect("producers_of");
    assert_eq!(
        producers,
        std::collections::HashMap::from([(a_id.clone(), agent_did(233))]),
        "[{label}] only the existing id resolves"
    );
    assert!(
        plain.producers_of(&[]).await.expect("empty").is_empty(),
        "[{label}] no ids, no producers"
    );
}

/// **RFC-ACDP-0018 §5.3 with RFC-ACDP-0006 idempotency — concurrent retries
/// of one keyed superseding publish append exactly one row and never fail.**
/// Eight tasks race the same `Idempotency-Key` and request. Exactly one
/// inserts; the rest replay (whether they lost before or after the winner
/// committed); no task errors; the feed holds one `superseded` row.
pub async fn assert_change_feed_concurrent_idempotent_supersede<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::types::change::ChangeKind;

    let signer = || {
        acdp::types::ReceiptSigner::new(
            SigningKey::from_bytes(&[91u8; 32]),
            "did:web:reg.test",
            "did:web:reg.test#receipt-key-1",
        )
        .expect("signer")
    };
    let start = store.changes_head().await.expect("changes_head").head;
    let p = producer(234);
    let v1 = publish_req(
        store,
        p.publish_request()
            .title("feed race fixture")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid publish request"),
        Some(signer()),
    )
    .await;
    let v1_body = store.get(&v1.ctx_id).expect("get").expect("v1").body;
    let req = p
        .supersede_body(&v1_body)
        .title("feed race fixture v2")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid successor request");
    let key = format!(
        "feed-race-{}",
        &v1.ctx_id.as_str()[v1.ctx_id.as_str().len() - 12..]
    );
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let s = Arc::clone(store);
            let req = req.clone();
            let key = key.clone();
            let signer = signer();
            let barrier = Arc::clone(&barrier);
            tokio::task::spawn_blocking(move || {
                let mint = move |body: &acdp::types::body::Body| {
                    let receipt = signer.mint(
                        &body.ctx_id,
                        &body.lineage_id,
                        &body.origin_registry,
                        body.created_at,
                        &body.content_hash,
                        &format!("sha256:{}", "c".repeat(64)),
                    )?;
                    serde_json::to_value(receipt).map_err(acdp::error::AcdpError::from)
                };
                barrier.wait();
                s.commit_publish(PublishCommit {
                    req: &req,
                    authority: AUTHORITY,
                    idempotency: Some(acdp::registry::store::PendingIdempotencyCommit {
                        key: &key,
                        ttl: chrono::Duration::hours(1),
                    }),
                    tenant: None,
                    receipt_minter: Some(&mint),
                    predecessor_admission: None,
                })
            })
        })
        .collect();
    let mut inserted = Vec::new();
    let mut replayed = Vec::new();
    for t in tasks {
        match t.await.expect("publish task") {
            Ok(PublishCommitOutcome::Inserted(r)) => inserted.push(r),
            Ok(PublishCommitOutcome::IdempotentReplay(r)) => replayed.push(r),
            Err(e) => panic!("[{label}] a concurrent keyed retry must not fail: {e}"),
        }
    }
    assert_eq!(inserted.len(), 1, "[{label}] exactly one insert");
    let winner = inserted.remove(0);
    assert!(
        replayed.iter().all(|r| r.ctx_id == winner.ctx_id),
        "[{label}] every replay returns the winner"
    );
    let rows = feed_rows_for(store, start, &[v1.ctx_id.as_str(), winner.ctx_id.as_str()]).await;
    assert_eq!(rows.len(), 1, "[{label}] one superseded row: {rows:?}");
    assert_eq!(rows[0].kind, ChangeKind::Superseded, "[{label}]");
    assert_eq!(
        rows[0].superseded_by.as_deref(),
        Some(winner.ctx_id.as_str()),
        "[{label}]"
    );
}

/// Falsification for [`assert_lifecycle_state_probe_sees_retraction`]: the
/// assertion is only worth running if a backend that inherits the trait's
/// `Ok(false)` default actually fails it. Also the falsification for
/// [`assert_lifecycle_retry_requires_identical_signature`]: a store that
/// ignores signatures must fail it.
/// A subscription row for owner `owner`, unique per call. Ids are v4
/// UUIDs built from the clock and a counter, so a shared Postgres database
/// never sees one twice.
fn subscription_fixture(owner: &str, n: u32) -> crate::NewSubscription {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
    let k = NEXT.fetch_add(1, Ordering::Relaxed) ^ nanos.rotate_left(17);
    let hex = format!("{:016x}{:016x}", nanos, k);
    let id = format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    );
    let filter: acdp::types::change::WatchFilter = serde_json::from_value(serde_json::json!({
        "agent_id": [format!("did:web:producer-{n}.example")],
        "kind": ["lifecycle"],
    }))
    .expect("fixture filter");
    crate::NewSubscription {
        subscription_id: id,
        owner_did: owner.to_owned(),
        tenant_id: Some("tenant-a".into()),
        filter,
        callback_url: format!("https://hooks.example.com/acdp/{n}"),
        secret_ct: vec![n as u8; 60],
        secret_key_id: 3,
        created_at_ms: 1_790_985_602_000 + i64::from(n),
        last_hinted_seq: 57 + u64::from(n),
    }
}

/// An owner DID no other run of the suite shares.
fn unique_owner(label: &str) -> String {
    format!(
        "did:web:owner-{label}-{}.example",
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    )
}

/// RFC-ACDP-0018 §8.1: subscription create / get / delete, owner-scoped.
///
/// A stored row round-trips every member, including the sealed secret
/// bytes and their key id; another owner can neither read nor delete it;
/// deletion is reported once.
pub async fn assert_subscription_crud<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use crate::CreateSubscriptionOutcome;
    use acdp::types::change::SubscriptionStatus;

    let owner = unique_owner(label);
    let other = unique_owner(&format!("{label}-other"));
    let new = subscription_fixture(&owner, 7);
    assert_eq!(
        store.create_subscription(&new, 10).await.expect("create"),
        CreateSubscriptionOutcome::Created,
        "{label}"
    );
    let got = store
        .get_subscription(&owner, &new.subscription_id)
        .await
        .expect("get")
        .unwrap_or_else(|| panic!("{label}: the owner reads its subscription"));
    assert_eq!(got.subscription_id, new.subscription_id, "{label}");
    assert_eq!(got.owner_did, owner, "{label}");
    assert_eq!(got.tenant_id, new.tenant_id, "{label}");
    assert_eq!(got.filter, new.filter, "{label}");
    assert_eq!(got.callback_url, new.callback_url, "{label}");
    assert_eq!(
        got.secret_ct, new.secret_ct,
        "{label}: sealed bytes verbatim"
    );
    assert_eq!(got.secret_key_id, 3, "{label}");
    assert_eq!(got.status, SubscriptionStatus::Active, "{label}");
    assert_eq!(got.created_at_ms, new.created_at_ms, "{label}");
    assert_eq!(got.last_hinted_seq, new.last_hinted_seq, "{label}");
    assert_eq!(
        got.next_attempt_at_ms, new.created_at_ms,
        "{label}: due at creation"
    );
    assert_eq!(got.failing_since_ms, None, "{label}");
    assert_eq!(got.attempts, 0, "{label}");
    assert_eq!(got.lease_until_ms, None, "{label}");
    let sub = got
        .to_subscription()
        .expect("a stored row is a valid subscription");
    assert_eq!(sub.secret, None, "{label}: GET form carries no secret");

    assert_eq!(
        store
            .get_subscription(&other, &new.subscription_id)
            .await
            .expect("get other"),
        None,
        "{label}: another owner cannot read it"
    );
    assert!(
        !store
            .delete_subscription(&other, &new.subscription_id)
            .await
            .expect("delete other"),
        "{label}: another owner cannot delete it"
    );
    assert!(
        store
            .get_subscription(&owner, &new.subscription_id)
            .await
            .expect("get after foreign delete")
            .is_some(),
        "{label}: a foreign delete leaves it in place"
    );
    assert_eq!(
        store
            .get_subscription(&owner, "not-a-uuid")
            .await
            .expect("get malformed"),
        None,
        "{label}"
    );
    assert!(
        store
            .delete_subscription(&owner, &new.subscription_id)
            .await
            .expect("delete"),
        "{label}: the owner deletes it"
    );
    assert!(
        !store
            .delete_subscription(&owner, &new.subscription_id)
            .await
            .expect("delete again"),
        "{label}: deletion is reported once"
    );
    assert_eq!(
        store
            .get_subscription(&owner, &new.subscription_id)
            .await
            .expect("get after delete"),
        None,
        "{label}"
    );
}

/// RFC-ACDP-0018 §8.1 `max_subscriptions_per_requester`: concurrent creates
/// by one owner never exceed the limit, one owner's count does not consume
/// another's, and a delete frees a slot.
pub async fn assert_subscription_limit<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use crate::CreateSubscriptionOutcome;

    const MAX: u64 = 3;
    const RACERS: u32 = 12;
    let owner = unique_owner(label);
    let mut tasks = Vec::new();
    for n in 0..RACERS {
        let store = Arc::clone(store);
        let new = subscription_fixture(&owner, n);
        tasks.push(tokio::spawn(async move {
            let outcome = store.create_subscription(&new, MAX).await.expect("create");
            (new.subscription_id, outcome)
        }));
    }
    let mut created = Vec::new();
    for t in tasks {
        let (id, outcome) = t.await.expect("join");
        if outcome == CreateSubscriptionOutcome::Created {
            created.push(id);
        }
    }
    assert_eq!(
        created.len() as u64,
        MAX,
        "{label}: exactly the limit is created under a race"
    );

    let other = unique_owner(&format!("{label}-other"));
    assert_eq!(
        store
            .create_subscription(&subscription_fixture(&other, 99), MAX)
            .await
            .expect("other owner"),
        CreateSubscriptionOutcome::Created,
        "{label}: the limit is per owner"
    );
    assert_eq!(
        store
            .create_subscription(&subscription_fixture(&owner, 100), MAX)
            .await
            .expect("over"),
        CreateSubscriptionOutcome::LimitReached,
        "{label}"
    );
    assert!(store
        .delete_subscription(&owner, &created[0])
        .await
        .expect("delete"));
    assert_eq!(
        store
            .create_subscription(&subscription_fixture(&owner, 101), MAX)
            .await
            .expect("after delete"),
        CreateSubscriptionOutcome::Created,
        "{label}: a delete frees a slot"
    );
}

/// RFC-ACDP-0018 §8.2 delivery leases: `claim_due_subscriptions` hands each
/// due row to exactly one claimant, even when claimants race; a leased,
/// caught-up, not-yet-due or disabled row is not claimed; a lapsed lease is
/// claimable again; and `finish_subscription_attempt` writes only for the
/// lease holder.
///
/// The suite's clock is decades in the past, so the only due rows are this
/// run's own (every other fixture is due in 2026): a shared Postgres
/// database's other rows are never claimed, and the assertions look at this
/// run's ids alone.
pub async fn assert_subscription_claims<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use crate::{CreateSubscriptionOutcome, SubscriptionProgress};
    use acdp::types::change::SubscriptionStatus;
    use std::collections::HashSet;

    const T0: i64 = 1_000_000;
    const HEAD: u64 = 1_000;
    const LEASE: i64 = 60_000;
    let owner = unique_owner(label);
    let mut ids = Vec::new();
    let mut add = |n: u32, last_hinted_seq: u64| {
        let mut new = subscription_fixture(&owner, n);
        new.created_at_ms = T0 - 1;
        new.last_hinted_seq = last_hinted_seq;
        ids.push(new.subscription_id.clone());
        new
    };
    let racing: Vec<_> = (0..8).map(|n| add(n, 5)).collect();
    let caught_up = add(20, HEAD);
    let mut all = racing.clone();
    all.push(caught_up.clone());
    for new in &all {
        assert_eq!(
            store.create_subscription(new, 100).await.expect("create"),
            CreateSubscriptionOutcome::Created,
            "{label}"
        );
    }
    let ours: HashSet<String> = ids.iter().cloned().collect();
    let racing_ids: HashSet<String> = racing.iter().map(|n| n.subscription_id.clone()).collect();

    // Concurrent claimants, each taking at most 3: every due row goes to
    // exactly one of them, and the caught-up row to none.
    let mut tasks = Vec::new();
    for _ in 0..6 {
        let store = Arc::clone(store);
        tasks.push(tokio::spawn(async move {
            store
                .claim_due_subscriptions(T0, HEAD, T0 + LEASE, 3)
                .await
                .expect("claim")
        }));
    }
    let mut claimed = Vec::new();
    for t in tasks {
        claimed.extend(t.await.expect("join"));
    }
    let mine: Vec<_> = claimed
        .into_iter()
        .filter(|r| ours.contains(&r.subscription_id))
        .collect();
    let mut seen = HashSet::new();
    for r in &mine {
        assert!(
            seen.insert(r.subscription_id.clone()),
            "{label}: {} claimed twice",
            r.subscription_id
        );
        assert_eq!(
            r.lease_until_ms,
            Some(T0 + LEASE),
            "{label}: the claim returns its lease"
        );
    }
    assert_eq!(
        seen, racing_ids,
        "{label}: every due row is claimed, the caught-up one is not"
    );

    // Leased rows are not claimed again until the lease lapses.
    let again = store
        .claim_due_subscriptions(T0 + 1, HEAD, T0 + 1 + LEASE, 100)
        .await
        .expect("claim leased");
    assert!(
        again.iter().all(|r| !ours.contains(&r.subscription_id)),
        "{label}: a leased row is claimed again"
    );

    let [a, b, c, d, ..] = &racing[..] else {
        unreachable!()
    };
    // Only the lease holder finishes.
    let healthy = SubscriptionProgress::healthy(HEAD, T0 + 10 * LEASE);
    assert!(
        !store
            .finish_subscription_attempt(&a.subscription_id, T0 + LEASE + 1, &healthy)
            .await
            .expect("finish with a stale lease"),
        "{label}: a foreign lease writes nothing"
    );
    assert!(
        store
            .finish_subscription_attempt(&a.subscription_id, T0 + LEASE, &healthy)
            .await
            .expect("finish"),
        "{label}: the holder finishes"
    );
    let got = store
        .get_subscription(&owner, &a.subscription_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(got.last_hinted_seq, HEAD, "{label}");
    assert_eq!(got.next_attempt_at_ms, T0 + 10 * LEASE, "{label}");
    assert_eq!(
        got.lease_until_ms, None,
        "{label}: finishing releases the lease"
    );
    assert!(
        !store
            .finish_subscription_attempt(&a.subscription_id, T0 + LEASE, &healthy)
            .await
            .expect("finish twice"),
        "{label}: a released lease cannot finish again"
    );

    // A failure keeps the position and schedules a retry; the retry is due
    // only when its time comes.
    let failing = SubscriptionProgress {
        last_hinted_seq: 5,
        status: SubscriptionStatus::Active,
        next_attempt_at_ms: T0 + 2 * LEASE,
        failing_since_ms: Some(T0),
        attempts: 1,
    };
    assert!(store
        .finish_subscription_attempt(&b.subscription_id, T0 + LEASE, &failing)
        .await
        .expect("finish failing"));
    let got = store
        .get_subscription(&owner, &b.subscription_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        (got.last_hinted_seq, got.failing_since_ms, got.attempts),
        (5, Some(T0), 1),
        "{label}: failure history round-trips"
    );

    // Disabled rows are never claimed.
    let mut c_rec = store
        .get_subscription(&owner, &c.subscription_id)
        .await
        .expect("get")
        .expect("exists");
    c_rec.failing_since_ms = Some(T0 - 5);
    assert!(store
        .finish_subscription_attempt(
            &c.subscription_id,
            T0 + LEASE,
            &SubscriptionProgress::disabled(&c_rec, T0)
        )
        .await
        .expect("disable"));
    let got = store
        .get_subscription(&owner, &c.subscription_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(got.status, SubscriptionStatus::Disabled, "{label}");
    assert_eq!(
        got.failing_since_ms,
        Some(T0 - 5),
        "{label}: kept for the operator"
    );
    let counts = store.subscription_counts(HEAD).await.expect("counts");
    assert!(
        counts.disabled >= 1 && counts.active >= 1,
        "{label}: {counts:?}"
    );
    // `b` is active and behind HEAD, so a hint is owed no later than its
    // retry; nothing sits behind seq 0.
    assert!(
        counts
            .oldest_due_at_ms
            .is_some_and(|due| due <= T0 + 2 * LEASE),
        "{label}: backlog {counts:?}"
    );
    assert_eq!(
        store
            .subscription_counts(0)
            .await
            .expect("counts")
            .oldest_due_at_ms,
        None,
        "{label}: no subscription is behind seq 0"
    );

    // After every lease lapses: `d` (never finished) and `b` (its retry now
    // due) are claimable; `a` (caught up and not yet due), `c` (disabled)
    // and the caught-up row are not.
    let later = T0 + 2 * LEASE;
    let reclaimed: HashSet<String> = store
        .claim_due_subscriptions(later, HEAD, later + LEASE, 100)
        .await
        .expect("reclaim")
        .into_iter()
        .map(|r| r.subscription_id)
        .filter(|id| ours.contains(id))
        .collect();
    assert!(
        reclaimed.contains(&d.subscription_id),
        "{label}: a lapsed lease is claimable"
    );
    assert!(
        reclaimed.contains(&b.subscription_id),
        "{label}: a due retry is claimable"
    );
    for (who, id) in [
        ("caught up", &a.subscription_id),
        ("disabled", &c.subscription_id),
        ("at head", &caught_up.subscription_id),
    ] {
        assert!(!reclaimed.contains(id), "{label}: {who} row claimed");
    }
    // The first claim's holder lost `d` to the reclaim.
    assert!(
        !store
            .finish_subscription_attempt(&d.subscription_id, T0 + LEASE, &healthy)
            .await
            .expect("finish lost lease"),
        "{label}: a lapsed lease cannot finish over its successor"
    );

    for id in &ids {
        store
            .delete_subscription(&owner, id)
            .await
            .expect("cleanup");
    }
}

// ── RFC-ACDP-0021 key attestations ──────────────────────────────────────────
//
// Every kit below drives real `commit_publish` calls, with receipts, against
// the backend under test. Attestation bodies are committed under
// `did:web:reg.test` (the registry DID of `AUTHORITY`), exactly as the mint
// path will commit them. Stores do not verify producer signatures, so a
// keyless successor is an ordinary request whose `signature.key_id` and
// `key_attestation` are then pointed at an attestation.
//
// Refusal order (pinned by `assert_keyless_refusal_order`): idempotency
// replay, then the `supersedes` ownership / AlreadySuperseded checks, then
// `attestation_not_found` / `attestation_withdrawn`, then §7
// `keyless_identity_mismatch`, then `outside_attestation_window`. See
// `crate::key_attestations`.

/// kl-001 vector 5 (spec bf37195 / 546b8c8), the §7 decision table, as the
/// `cases` array. Embedded so the kit needs no `ACDP_SPEC_DIR`; the
/// conformance suite re-asserts it against the fixture itself.
pub const KL_001_VECTOR_5_CASES: &str = r#"[
 {"name": "S1 same identity",
  "predecessor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "successor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "expected": {"result": "allowed"}},
 {"name": "S2 different subject",
  "predecessor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "successor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/other:ref:refs/heads/main"}, "org_wide": false},
  "expected": {"result": "refused", "reason": "keyless_identity_mismatch"}},
 {"name": "S3 different identity type",
  "predecessor": {"kind": "keyless", "identity": {"type": "spiffe", "subject": "spiffe://prod.example.com/ns/agents/sa/publisher"}, "org_wide": false},
  "successor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "expected": {"result": "refused", "reason": "keyless_identity_mismatch"}},
 {"name": "S4 org-wide successor",
  "predecessor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "successor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/other:ref:refs/heads/main"}, "org_wide": true},
  "expected": {"result": "allowed"}},
 {"name": "S5 controller key supersedes keyless",
  "predecessor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "successor": {"kind": "controller"},
  "expected": {"result": "allowed"}},
 {"name": "S6 keyless supersedes controller-signed",
  "predecessor": {"kind": "controller"},
  "successor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": false},
  "expected": {"result": "refused", "reason": "keyless_identity_mismatch"}},
 {"name": "S7 org-wide supersedes controller-signed",
  "predecessor": {"kind": "controller"},
  "successor": {"kind": "keyless", "identity": {"type": "spiffe", "subject": "spiffe://prod.example.com/ns/agents/sa/publisher"}, "org_wide": true},
  "expected": {"result": "allowed"}},
 {"name": "S8 org-wide predecessor does not widen the successor",
  "predecessor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/pipeline:ref:refs/heads/main"}, "org_wide": true},
  "successor": {"kind": "keyless", "identity": {"type": "oidc", "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/other:ref:refs/heads/main"}, "org_wide": false},
  "expected": {"result": "refused", "reason": "keyless_identity_mismatch"}},
 {"name": "S9 controller supersedes controller",
  "predecessor": {"kind": "controller"},
  "successor": {"kind": "controller"},
  "expected": {"result": "allowed"}}
]"#;

/// Fixtures shared by the keyless kits.
pub mod keyless_fixtures {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use acdp::crypto::SigningKey;
    use acdp::error::AcdpError;
    use acdp::producer::Producer;
    use acdp::registry::store::{PendingIdempotencyCommit, PublishCommit, PublishCommitOutcome};
    use acdp::types::body::Body;
    use acdp::types::keyless::{
        keyless_key_id, token_hash, Jwk, KeyAttestation, KeyAttestationRef, WorkloadIdentity,
        ATTESTATION_VERSION, KEY_ATTESTATION_SCHEMA_URI, KEY_ATTESTATION_TYPE,
    };
    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    use acdp::types::primitives::{AgentDid, ContentHash, ContextType, CtxId, Visibility};
    use acdp::types::publish::PublishRequest;
    use chrono::{DateTime, Duration, Utc};

    use crate::ExtendedRegistryStore;

    use super::AUTHORITY;

    /// The registry DID of `AUTHORITY`.
    pub const REGISTRY: &str = "did:web:reg.test";

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A value no other call, run or backend shares.
    pub fn unique(label: &str) -> String {
        format!(
            "{label}-{}-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// A controller DID no other run shares.
    pub fn controller(label: &str) -> String {
        format!("did:web:agents.test:kl-{}", unique(label))
    }

    pub fn oidc(subject: &str) -> WorkloadIdentity {
        WorkloadIdentity::Oidc {
            issuer: "https://token.actions.githubusercontent.com".into(),
            subject: subject.into(),
        }
    }

    pub fn spiffe(subject: &str) -> WorkloadIdentity {
        WorkloadIdentity::Spiffe {
            subject: subject.into(),
        }
    }

    /// `now` truncated to milliseconds, the precision of every window.
    pub fn now() -> DateTime<Utc> {
        acdp::time::trunc_ms(Utc::now())
    }

    /// A window that contains the next ten minutes.
    pub fn open_window() -> (DateTime<Utc>, DateTime<Utc>) {
        let n = now();
        (n - Duration::seconds(60), n + Duration::seconds(540))
    }

    /// A ten-minute window that ended ten minutes ago.
    pub fn past_window() -> (DateTime<Utc>, DateTime<Utc>) {
        let n = now();
        (n - Duration::minutes(20), n - Duration::minutes(10))
    }

    /// One attestation to mint.
    #[derive(Debug, Clone)]
    pub struct Spec {
        pub controller: String,
        pub identity: WorkloadIdentity,
        pub org_wide: bool,
        pub window: (DateTime<Utc>, DateTime<Utc>),
        /// The token whose hash the attestation records.
        pub token: String,
    }

    impl Spec {
        pub fn new(controller: &str, identity: WorkloadIdentity) -> Self {
            Self {
                controller: controller.to_string(),
                identity,
                org_wide: false,
                window: open_window(),
                token: unique("token"),
            }
        }

        pub fn org_wide(mut self) -> Self {
            self.org_wide = true;
            self
        }

        pub fn window(mut self, window: (DateTime<Utc>, DateTime<Utc>)) -> Self {
            self.window = window;
            self
        }

        pub fn token(mut self, token: &str) -> Self {
            self.token = token.to_string();
            self
        }

        /// The §5.3 metadata, for an ephemeral key derived from `seed`.
        pub fn attestation(&self, seed: u8) -> KeyAttestation {
            let key = SigningKey::from_bytes(&[seed; 32]);
            let jwk = Jwk::from_ed25519(&key.verifying_key_bytes());
            let fp = jwk.fingerprint().expect("fingerprint");
            KeyAttestation {
                attestation_version: ATTESTATION_VERSION.into(),
                controller: self.controller.clone(),
                identity: self.identity.clone(),
                org_wide: self.org_wide,
                key_id: keyless_key_id(&self.controller, &fp).expect("key id"),
                algorithm: "ed25519".into(),
                public_key_jwk: jwk,
                key_fingerprint: fp,
                not_before: self.window.0,
                not_after: self.window.1,
                token_hash: token_hash(&self.token),
            }
        }

        /// The attestation body's publish request, under the registry DID.
        pub fn request(&self, seed: u8) -> (PublishRequest, KeyAttestation) {
            let att = self.attestation(seed);
            let metadata = serde_json::to_value(&att).expect("metadata");
            (attestation_request(metadata), att)
        }
    }

    /// An `acdp:key-attestation` request under the registry DID carrying
    /// `metadata` as is (which a kit may make invalid on purpose).
    pub fn attestation_request(metadata: serde_json::Value) -> PublishRequest {
        Producer::new(
            SigningKey::from_bytes(&[91; 32]),
            AgentDid::new(REGISTRY.to_string()),
            format!("{REGISTRY}#receipt-key-1"),
        )
        .publish_request()
        .title("Key attestation")
        .summary("reg.test attests an ephemeral ed25519 key (RFC-ACDP-0021).")
        .context_type(ContextType::Custom(KEY_ATTESTATION_TYPE.into()))
        .visibility(Visibility::Public)
        .metadata(metadata)
        .schema_uri(KEY_ATTESTATION_SCHEMA_URI)
        .acdp_version("0.5.0")
        .build()
        .expect("valid attestation request")
    }

    /// A committed attestation.
    #[derive(Debug, Clone)]
    pub struct Minted {
        pub ctx_id: CtxId,
        pub content_hash: ContentHash,
        pub attestation: KeyAttestation,
    }

    impl Minted {
        pub fn reference(&self) -> KeyAttestationRef {
            KeyAttestationRef {
                ctx_id: self.ctx_id.clone(),
                content_hash: self.content_hash.clone(),
            }
        }
    }

    /// The registry's receipt signer.
    pub fn receipt_signer() -> acdp::types::ReceiptSigner {
        acdp::types::ReceiptSigner::new(
            SigningKey::from_bytes(&[91u8; 32]),
            REGISTRY,
            "did:web:reg.test#receipt-key-1",
        )
        .expect("signer")
    }

    /// `commit_publish` with a receipt minter, an optional idempotency key
    /// and an optional RFC-ACDP-0014 admission that records it ran.
    pub async fn commit<S>(
        store: &Arc<S>,
        req: PublishRequest,
        idem: Option<String>,
    ) -> Result<PublishCommitOutcome, AcdpError>
    where
        S: ExtendedRegistryStore + 'static,
    {
        let s = Arc::clone(store);
        tokio::task::spawn_blocking(move || {
            let signer = receipt_signer();
            let mint = move |body: &Body| {
                let receipt = signer.mint(
                    &body.ctx_id,
                    &body.lineage_id,
                    &body.origin_registry,
                    body.created_at,
                    &body.content_hash,
                    &format!("sha256:{}", "c".repeat(64)),
                )?;
                serde_json::to_value(receipt).map_err(AcdpError::from)
            };
            s.commit_publish(PublishCommit {
                req: &req,
                authority: AUTHORITY,
                idempotency: idem.as_deref().map(|key| PendingIdempotencyCommit {
                    key,
                    ttl: chrono::Duration::hours(1),
                }),
                tenant: None,
                receipt_minter: Some(&mint),
                predecessor_admission: None,
            })
        })
        .await
        .expect("publish task")
    }

    pub fn inserted(outcome: Result<PublishCommitOutcome, AcdpError>, what: &str) -> CtxId {
        match outcome {
            Ok(PublishCommitOutcome::Inserted(r)) => r.ctx_id,
            other => panic!("{what}: expected Inserted, got {other:?}"),
        }
    }

    /// Mint `spec` and return the committed attestation.
    pub async fn mint<S>(store: &Arc<S>, spec: &Spec) -> Minted
    where
        S: ExtendedRegistryStore + 'static,
    {
        let (req, attestation) = spec.request(7);
        let content_hash = req.content_hash.clone();
        let ctx_id = inserted(commit(store, req, None).await, "mint");
        Minted {
            ctx_id,
            content_hash,
            attestation,
        }
    }

    /// The controller's own producer.
    pub fn controller_producer(controller: &str) -> Producer {
        Producer::new(
            SigningKey::from_bytes(&[42; 32]),
            AgentDid::new(controller.to_string()),
            format!("{controller}#key-1"),
        )
    }

    /// A controller-signed request, first version or superseding `prev`.
    pub fn controller_request(controller: &str, prev: Option<(&CtxId, u32)>) -> PublishRequest {
        let p = controller_producer(controller);
        let b = match prev {
            Some((ctx, v)) => p.supersede(ctx.clone()).version(v),
            None => p.publish_request(),
        };
        b.title(unique("keyless fixture"))
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid request")
    }

    /// `controller_request` re-pointed at `att`: a keyless-signed request.
    pub fn keyless_request(
        controller: &str,
        att: &KeyAttestationRef,
        key_id: &str,
        prev: Option<(&CtxId, u32)>,
    ) -> PublishRequest {
        let mut req = controller_request(controller, prev);
        req.signature.key_id = key_id.to_string();
        req.key_attestation = Some(att.clone());
        req
    }

    /// `keyless_request` under a minted attestation.
    pub fn signed_under(m: &Minted, prev: Option<(&CtxId, u32)>) -> PublishRequest {
        keyless_request(
            &m.attestation.controller,
            &m.reference(),
            &m.attestation.key_id,
            prev,
        )
    }

    /// Retract `ctx_id` with a registry-actor event.
    pub async fn retract<S>(store: &Arc<S>, ctx_id: &CtxId)
    where
        S: ExtendedRegistryStore + 'static,
    {
        let event_id = ctx_id
            .as_str()
            .rsplit('/')
            .next()
            .expect("ctx_id ends in a UUID")
            .to_string();
        let event = LifecycleEvent::new(
            event_id,
            ctx_id.clone(),
            LifecycleEventType::Retracted,
            Utc::now(),
            AgentDid::new(REGISTRY.to_string()),
            Some("keyless parity: withdraw".to_string()),
        )
        .expect("valid lifecycle event");
        let s = Arc::clone(store);
        tokio::task::spawn_blocking(move || s.commit_lifecycle_event(&event))
            .await
            .expect("retract task")
            .expect("retract succeeds");
    }

    /// Assert `res` is a refusal carrying keyless `reason`.
    pub fn assert_reason<T: std::fmt::Debug>(
        res: Result<T, AcdpError>,
        reason: acdp::types::keyless::KeylessReason,
        what: &str,
    ) -> AcdpError {
        let err = res.expect_err(&format!("{what}: expected {reason}"));
        assert_eq!(
            acdp::types::keyless::KeylessReason::of(&err),
            Some(reason),
            "{what}: wrong refusal {err:?}"
        );
        err
    }
}

/// What a store has committed, for "nothing was persisted" checks: the
/// number of searchable contexts under the registry DID (every mint) and
/// under `controller` (every successor), and, where there is one, the log
/// size. The registry DID and the log are store-wide, so the backend passes
/// a store no other test writes to.
async fn keyless_watermark<S>(store: &Arc<S>, controller: &str, has_log: bool) -> (u64, Option<u64>)
where
    S: ExtendedRegistryStore + 'static,
{
    let mut contexts = 0u64;
    for agent in [keyless_fixtures::REGISTRY, controller] {
        let mut cursor: Option<String> = None;
        loop {
            let params = SearchParams {
                agent_id: Some(agent.to_string()),
                limit: Some(100),
                cursor: cursor.take(),
                ..Default::default()
            };
            let page = store.search(&params, None, true).expect("search");
            contexts += page.matches.len() as u64;
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
    }
    let log = if has_log {
        Some(store.log_tree_size().await.expect("log_tree_size"))
    } else {
        None
    };
    (contexts, log)
}

/// **Index derivation and token replay (D4).** A mint indexes its body in
/// the publish transaction. A second mint with the same token is refused
/// `token_replayed` and leaves no body, feed row or log leaf; of two racing
/// mints with one token exactly one commits; a corrupt attestation aborts.
/// The log (when `has_log`) grows by exactly one leaf per accepted mint.
///
/// Pass a store with the transparency log enabled when the backend has one
/// (`has_log = true`), on a database no other test writes to (this kit and
/// [`assert_keyless_window_and_withdrawal`] count the whole store).
pub async fn assert_key_attestation_index_and_replay<S>(store: &Arc<S>, has_log: bool, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::error::AcdpError;
    use acdp::types::keyless::{token_hash, KeylessReason};
    use keyless_fixtures::*;

    if !has_log {
        assert!(
            matches!(
                store.log_tree_size().await,
                Err(AcdpError::NotImplemented(_))
            ),
            "[{label}] has_log = false is only for a backend with no log"
        );
    }
    let c = controller(label);
    let spec = Spec::new(&c, oidc("repo:example/pipeline:ref:refs/heads/main"));
    let before = keyless_watermark(store, &c, has_log).await;
    assert!(
        !store
            .key_attestation_token_seen(&token_hash(&spec.token))
            .await
            .expect("token_seen"),
        "[{label}] a fresh token is unseen"
    );
    let first = mint(store, &spec).await;
    let after_one = keyless_watermark(store, &c, has_log).await;
    assert_eq!(after_one.0, before.0 + 1, "[{label}] one context per mint");
    assert_eq!(
        after_one.1,
        before.1.map(|n| n + 1),
        "[{label}] the log grows by exactly one leaf per mint"
    );
    assert!(
        store
            .key_attestation_token_seen(&token_hash(&spec.token))
            .await
            .expect("token_seen"),
        "[{label}] the minted token is seen"
    );
    let record = store
        .key_attestation(first.ctx_id.as_str())
        .await
        .expect("key_attestation")
        .expect("the mint is indexed");
    assert_eq!(record.attestation, first.attestation, "[{label}]");

    // Same token, a different key and window: a different body.
    let replay = Spec {
        window: past_window(),
        ..spec.clone()
    };
    let (req, _) = replay.request(8);
    assert_ne!(req.content_hash, first.content_hash);
    let err = assert_reason(
        commit(store, req, None).await,
        KeylessReason::TokenReplayed,
        &format!("[{label}] replayed token"),
    );
    assert!(
        matches!(err, AcdpError::KeyNotAuthorized(_)),
        "[{label}] {err:?}"
    );
    assert_eq!(
        keyless_watermark(store, &c, has_log).await,
        after_one,
        "[{label}] a replayed mint persists no body or log leaf"
    );
    let listed = store
        .list_key_attestations(&c, &Default::default(), None, 50)
        .await
        .expect("list");
    assert_eq!(listed.items.len(), 1, "[{label}] the replay is not indexed");

    // A corrupt attestation (a code bug in the mint path) aborts the commit.
    let mut bad = serde_json::to_value(spec.attestation(9)).expect("metadata");
    bad["key_fingerprint"] = serde_json::json!(format!("sha256:{}", "0".repeat(64)));
    bad["token_hash"] = serde_json::json!(token_hash(&unique("corrupt")));
    let err = commit(store, attestation_request(bad), None)
        .await
        .expect_err("a corrupt attestation must abort");
    assert!(
        matches!(err, AcdpError::RegistryInternal(_)),
        "[{label}] {err:?}"
    );
    assert_eq!(
        keyless_watermark(store, &c, has_log).await,
        after_one,
        "[{label}] an aborted mint persists nothing"
    );

    // Two racing mints with one token: exactly one commits.
    let racing = Spec::new(&c, spiffe("spiffe://prod.example.com/ns/agents/sa/race"));
    let (a, _) = racing.request(10);
    let (b, _) = racing.request(11);
    let (ra, rb) = tokio::join!(commit(store, a, None), commit(store, b, None));
    let won = [&ra, &rb].iter().filter(|r| r.is_ok()).count();
    assert_eq!(
        won, 1,
        "[{label}] exactly one racing mint wins: {ra:?} / {rb:?}"
    );
    let loser = if ra.is_ok() { rb } else { ra };
    assert_reason(
        loser,
        KeylessReason::TokenReplayed,
        &format!("[{label}] race"),
    );
    let end = keyless_watermark(store, &c, has_log).await;
    assert_eq!(
        end.0,
        before.0 + 2,
        "[{label}] two accepted mints, two contexts"
    );
    assert_eq!(
        end.1,
        before.1.map(|n| n + 2),
        "[{label}] two accepted mints, two log leaves"
    );
}

/// **§6.3 steps 3-6 in the store.** Inside the window → Inserted; a
/// reference that does not bind the request → `attestation_mismatch`;
/// `created_at` at or past `not_after`, or before `not_before` →
/// `outside_attestation_window` with nothing persisted; a retracted
/// attestation → `attestation_withdrawn`; an unindexed one →
/// `attestation_not_found`. Pass a store no other test writes to.
pub async fn assert_keyless_window_and_withdrawal<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::error::AcdpError;
    use acdp::types::keyless::KeylessReason;
    use acdp::types::primitives::{ContentHash, CtxId};
    use chrono::Duration;
    use keyless_fixtures::*;

    let c = controller(label);
    let id = oidc("repo:example/pipeline:ref:refs/heads/main");
    let open = mint(store, &Spec::new(&c, id.clone())).await;
    inserted(
        commit(store, signed_under(&open, None), None).await,
        &format!("[{label}] inside the window"),
    );

    // §6.3 step 4, re-checked in the store: a reference to a live, indexed
    // attestation that does not bind the request (another controller,
    // another key, another algorithm) is refused with the handler's
    // `attestation_mismatch` and persists nothing.
    let bound = keyless_watermark(store, &c, false).await;
    let other = controller(&format!("{label}-other"));
    let other_fp = format!("sha256:{}", "e".repeat(64));
    let mut wrong_alg = signed_under(&open, None);
    wrong_alg.signature.algorithm = "ecdsa-p256".into();
    for (req, what) in [
        (
            keyless_request(
                &other,
                &open.reference(),
                &acdp::types::keyless::keyless_key_id(&other, &open.attestation.key_fingerprint)
                    .expect("key id"),
                None,
            ),
            "another controller",
        ),
        (
            keyless_request(
                &c,
                &open.reference(),
                &acdp::types::keyless::keyless_key_id(&c, &other_fp).expect("key id"),
                None,
            ),
            "another key",
        ),
        (wrong_alg, "another algorithm"),
    ] {
        let err = assert_reason(
            commit(store, req, None).await,
            KeylessReason::AttestationMismatch,
            &format!("[{label}] store binding: {what}"),
        );
        assert!(
            matches!(err, AcdpError::KeyNotAuthorized(_)),
            "[{label}] {what}: {err:?}"
        );
    }
    assert_eq!(
        keyless_watermark(store, &c, false).await,
        bound,
        "[{label}] an unbound keyless publish persists nothing"
    );

    let before = keyless_watermark(store, &c, false).await;
    let past = mint(store, &Spec::new(&c, id.clone()).window(past_window())).await;
    let n = now();
    let future = mint(
        store,
        &Spec::new(&c, id.clone()).window((n + Duration::minutes(10), n + Duration::minutes(20))),
    )
    .await;
    let mid = keyless_watermark(store, &c, false).await;
    assert_eq!(mid.0, before.0 + 2);
    for (m, what) in [
        (&past, "created_at >= not_after"),
        (&future, "created_at < not_before"),
    ] {
        let err = assert_reason(
            commit(store, signed_under(m, None), None).await,
            KeylessReason::OutsideAttestationWindow,
            &format!("[{label}] {what}"),
        );
        assert!(
            matches!(err, AcdpError::KeyNotAuthorized(_)),
            "[{label}] {err:?}"
        );
    }
    assert_eq!(
        keyless_watermark(store, &c, false).await,
        mid,
        "[{label}] a refused keyless publish persists nothing"
    );

    retract(store, &open.ctx_id).await;
    let err = assert_reason(
        commit(store, signed_under(&open, None), None).await,
        KeylessReason::AttestationWithdrawn,
        &format!("[{label}] retracted attestation"),
    );
    assert!(
        matches!(err, AcdpError::KeyNotAuthorized(_)),
        "[{label}] {err:?}"
    );

    let missing = KeyAttestationRefFixture::missing();
    let err = assert_reason(
        commit(
            store,
            keyless_request(&c, &missing, &open.attestation.key_id, None),
            None,
        )
        .await,
        KeylessReason::AttestationNotFound,
        &format!("[{label}] unindexed attestation"),
    );
    assert!(
        matches!(err, AcdpError::KeyResolution(_)),
        "[{label}] {err:?}"
    );
    // A controller-signed body under the same controller is never checked.
    inserted(
        commit(store, controller_request(&c, None), None).await,
        &format!("[{label}] controller-signed"),
    );

    struct KeyAttestationRefFixture;
    impl KeyAttestationRefFixture {
        fn missing() -> acdp::types::keyless::KeyAttestationRef {
            acdp::types::keyless::KeyAttestationRef {
                ctx_id: CtxId(format!(
                    "acdp://{AUTHORITY}/00000000-0000-4000-8000-00000000dead"
                )),
                content_hash: ContentHash(format!("sha256:{}", "d".repeat(64))),
            }
        }
    }
}

/// **The cross-store refusal order.** Every backend refuses a keyless
/// successor in the order `crate::key_attestations` documents. Each leg
/// below makes two checks fail at once and pins which one answers:
///
/// 1. an idempotent replay beats a since-withdrawn attestation;
/// 2. `supersedes` ownership beats a withdrawn attestation;
/// 3. AlreadySuperseded beats a withdrawn attestation, and so does a
///    version mismatch (`version != previous + 1`);
/// 4. `attestation_withdrawn` beats `keyless_identity_mismatch`;
/// 5. `keyless_identity_mismatch` beats `outside_attestation_window`;
/// 6. and with §7 satisfied, the window still refuses.
pub async fn assert_keyless_refusal_order<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::error::{AcdpError, SupersessionReason};
    use acdp::registry::store::PublishCommitOutcome;
    use acdp::types::keyless::KeylessReason;
    use keyless_fixtures::*;

    let c = controller(label);
    let x = oidc("repo:example/pipeline:ref:refs/heads/main");
    let y = oidc("repo:example/other:ref:refs/heads/main");

    // 1. Idempotent replay first.
    let a = mint(store, &Spec::new(&c, x.clone())).await;
    let req = signed_under(&a, None);
    let key = unique("idem");
    inserted(
        commit(store, req.clone(), Some(key.clone())).await,
        &format!("[{label}] keyed keyless publish"),
    );
    retract(store, &a.ctx_id).await;
    assert!(
        matches!(
            commit(store, req, Some(key)).await,
            Ok(PublishCommitOutcome::IdempotentReplay(_))
        ),
        "[{label}] an idempotent replay is answered before the attestation is re-checked"
    );

    // 2. Ownership first: another controller's predecessor.
    let other = controller(label);
    let foreign = inserted(
        commit(store, controller_request(&other, None), None).await,
        "foreign v1",
    );
    let err = commit(store, signed_under(&a, Some((&foreign, 2))), None)
        .await
        .expect_err("non-owner");
    assert!(
        matches!(
            err,
            AcdpError::SupersededTarget {
                reason: SupersessionReason::NotFound,
                ..
            }
        ),
        "[{label}] ownership is checked before the attestation: {err:?}"
    );

    // 3. AlreadySuperseded first.
    let v1 = inserted(
        commit(store, controller_request(&c, None), None).await,
        "v1",
    );
    inserted(
        commit(store, controller_request(&c, Some((&v1, 2))), None).await,
        "v2",
    );
    let err = commit(store, signed_under(&a, Some((&v1, 2))), None)
        .await
        .expect_err("already superseded");
    assert!(
        matches!(
            err,
            AcdpError::SupersededTarget {
                reason: SupersessionReason::AlreadySuperseded,
                ..
            }
        ),
        "[{label}] AlreadySuperseded is checked before the attestation: {err:?}"
    );

    // 4. Withdrawn before §7: a controller-signed predecessor (S6 would
    // refuse) and a withdrawn successor attestation.
    let p1 = inserted(
        commit(store, controller_request(&c, None), None).await,
        "p1",
    );
    let err = commit(store, signed_under(&a, Some((&p1, 5))), None)
        .await
        .expect_err("version mismatch");
    assert!(
        matches!(
            err,
            AcdpError::SupersededTarget {
                reason: SupersessionReason::VersionMismatch,
                ..
            }
        ),
        "[{label}] a version mismatch is checked before the attestation: {err:?}"
    );
    assert_reason(
        commit(store, signed_under(&a, Some((&p1, 2))), None).await,
        KeylessReason::AttestationWithdrawn,
        &format!("[{label}] withdrawn beats mismatch"),
    );

    // 5. §7 before the window: a non-org-wide successor whose window is
    // over, against the same controller-signed predecessor.
    let stale = mint(store, &Spec::new(&c, y.clone()).window(past_window())).await;
    assert_reason(
        commit(store, signed_under(&stale, Some((&p1, 2))), None).await,
        KeylessReason::KeylessIdentityMismatch,
        &format!("[{label}] mismatch beats the window"),
    );

    // 6. With §7 satisfied (org-wide), the window answers.
    let stale_org = mint(store, &Spec::new(&c, y).org_wide().window(past_window())).await;
    assert_reason(
        commit(store, signed_under(&stale_org, Some((&p1, 2))), None).await,
        KeylessReason::OutsideAttestationWindow,
        &format!("[{label}] the window after §7"),
    );
}

/// **§7 through real commits.** Replays every case of kl-001 vector 5
/// (`cases_json`, normally [`KL_001_VECTOR_5_CASES`]): an allowed successor
/// is Inserted and supersedes its predecessor; a refused one is
/// `not_authorized` + `keyless_identity_mismatch` and leaves the
/// predecessor active.
pub async fn assert_keyless_supersession_table<S>(store: &Arc<S>, cases_json: &str, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::error::AcdpError;
    use acdp::types::keyless::{KeylessReason, WorkloadIdentity};
    use acdp::types::primitives::{CtxId, Status};
    use keyless_fixtures::*;

    let cases: Vec<serde_json::Value> = serde_json::from_str(cases_json).expect("cases JSON");
    assert_eq!(cases.len(), 9, "kl-001 vector 5 has nine cases");
    for case in &cases {
        let name = case["name"].as_str().expect("name");
        let c = controller(label);
        let side = |v: &serde_json::Value| -> Option<Spec> {
            match v["kind"].as_str().expect("kind") {
                "controller" => None,
                "keyless" => {
                    let identity: WorkloadIdentity =
                        serde_json::from_value(v["identity"].clone()).expect("identity");
                    let spec = Spec::new(&c, identity);
                    Some(if v["org_wide"].as_bool().expect("org_wide") {
                        spec.org_wide()
                    } else {
                        spec
                    })
                }
                other => panic!("unknown kind {other}"),
            }
        };
        let publish = |spec: Option<Spec>, prev: Option<CtxId>| {
            let c = c.clone();
            async move {
                let prev = prev.as_ref().map(|p| (p, 2));
                match spec {
                    None => commit(store, controller_request(&c, prev), None).await,
                    Some(spec) => {
                        let m = mint(store, &spec).await;
                        commit(store, signed_under(&m, prev), None).await
                    }
                }
            }
        };
        let v1 = inserted(
            publish(side(&case["predecessor"]), None).await,
            &format!("[{label}] {name} predecessor"),
        );
        let res = publish(side(&case["successor"]), Some(v1.clone())).await;
        let stored = store.get(&v1).expect("get").expect("v1 exists");
        match case["expected"]["result"].as_str().expect("result") {
            "allowed" => {
                inserted(res, &format!("[{label}] {name}"));
                assert_eq!(
                    stored.registry_state.status,
                    Status::Superseded,
                    "[{label}] {name}"
                );
            }
            "refused" => {
                assert_eq!(
                    case["expected"]["reason"].as_str(),
                    Some("keyless_identity_mismatch")
                );
                let err = assert_reason(
                    res,
                    KeylessReason::KeylessIdentityMismatch,
                    &format!("[{label}] {name}"),
                );
                assert!(
                    matches!(err, AcdpError::NotAuthorized(_)),
                    "[{label}] {name}: {err:?}"
                );
                assert_eq!(
                    stored.registry_state.status,
                    Status::Active,
                    "[{label}] {name}: a refused successor supersedes nothing"
                );
            }
            other => panic!("unknown result {other}"),
        }
    }
}

/// **A stored keyless predecessor with a malformed `key_attestation`
/// member fails closed.** No publish path stores one, so the backend's
/// `plant` stores `req` (a keyless-signed body whose member names a
/// `ctx_id` and `content_hash` that do not parse) by hand and returns its
/// `ctx_id`. A valid keyless v2 superseding it is refused with
/// `RegistryInternal`, never admitted as controller-signed, and v1 stays
/// active.
pub async fn assert_malformed_keyless_predecessor_fails_closed<S, F, Fut>(
    store: &Arc<S>,
    label: &str,
    plant: F,
) where
    S: ExtendedRegistryStore + 'static,
    F: FnOnce(acdp::types::publish::PublishRequest) -> Fut,
    Fut: std::future::Future<Output = CtxId>,
{
    use acdp::error::AcdpError;
    use acdp::types::keyless::KeyAttestationRef;
    use acdp::types::primitives::ContentHash;
    use keyless_fixtures::*;

    let c = controller(label);
    let id = oidc("repo:example/pipeline:ref:refs/heads/main");
    let key_id = Spec::new(&c, id.clone()).attestation(7).key_id;
    let malformed = KeyAttestationRef {
        ctx_id: CtxId("not-a-ctx-id".into()),
        content_hash: ContentHash("sha256:not-hex".into()),
    };
    assert!(
        malformed.validate().is_err(),
        "the fixture member is malformed"
    );
    let v1 = plant(keyless_request(&c, &malformed, &key_id, None)).await;
    let stored = store.get(&v1).expect("get").expect("planted v1");
    assert!(
        stored.body.key_attestation().is_err(),
        "[{label}] the planted body carries the malformed member"
    );
    let s = mint(store, &Spec::new(&c, id)).await;
    let err = commit(store, signed_under(&s, Some((&v1, 2))), None)
        .await
        .expect_err("a malformed keyless predecessor must fail closed");
    assert!(
        matches!(err, AcdpError::RegistryInternal(_)),
        "[{label}] {err:?}"
    );
    let stored = store.get(&v1).expect("get").expect("v1");
    assert_eq!(
        stored.registry_state.status,
        Status::Active,
        "[{label}] the refused successor supersedes nothing"
    );
}

/// **A keyless predecessor whose attestation row is gone fails closed.**
/// Publishes a keyless v1, then calls `unindex` with its attestation's
/// `ctx_id` (the backend deletes that index row by hand, the state a bug
/// would leave), and asserts a keyless v2 superseding it is refused with
/// `RegistryInternal`, not admitted as controller-signed.
pub async fn assert_unindexed_keyless_predecessor_fails_closed<S, F, Fut>(
    store: &Arc<S>,
    label: &str,
    unindex: F,
) where
    S: ExtendedRegistryStore + 'static,
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    use acdp::error::AcdpError;
    use keyless_fixtures::*;

    let c = controller(label);
    let p = mint(
        store,
        &Spec::new(&c, oidc("repo:example/pipeline:ref:refs/heads/main")),
    )
    .await;
    let v1 = inserted(
        commit(store, signed_under(&p, None), None).await,
        "keyless v1",
    );
    unindex(p.ctx_id.as_str().to_string()).await;
    let s = mint(
        store,
        &Spec::new(&c, oidc("repo:example/pipeline:ref:refs/heads/main")),
    )
    .await;
    let err = commit(store, signed_under(&s, Some((&v1, 2))), None)
        .await
        .expect_err("an unindexed keyless predecessor must fail closed");
    assert!(
        matches!(err, AcdpError::RegistryInternal(_)),
        "[{label}] {err:?}"
    );
}

/// **§5.4 listing.** Newest first (`created_at` descending, `ctx_id`
/// ascending), only the controller's own, narrowed by issuer and subject
/// byte for byte; cursor pages are disjoint and complete; a malformed
/// cursor is `invalid_cursor`; a retracted attestation is still listed; and
/// each item's `content_hash` and `metadata` are the stored body's.
pub async fn assert_key_attestation_listing<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use crate::KeyAttestationFilter;
    use acdp::error::AcdpError;
    use keyless_fixtures::*;

    let c = controller(label);
    let a = "repo:example/pipeline:ref:refs/heads/main";
    let specs = [
        Spec::new(&c, oidc(a)),
        Spec::new(&c, oidc(a)),
        Spec::new(&c, oidc("repo:example/other:ref:refs/heads/main")),
        Spec::new(
            &c,
            spiffe("spiffe://prod.example.com/ns/agents/sa/publisher"),
        ),
        Spec::new(&c, oidc(a)),
    ];
    let mut minted = Vec::new();
    for spec in &specs {
        minted.push(mint(store, spec).await);
    }
    // Another controller's attestation never shows up.
    mint(store, &Spec::new(&controller(label), oidc(a))).await;
    retract(store, &minted[1].ctx_id).await;

    let mut expected = Vec::new();
    for m in &minted {
        let r = store
            .key_attestation(m.ctx_id.as_str())
            .await
            .expect("record")
            .expect("indexed");
        expected.push((r.created_at_ms, m.ctx_id.as_str().to_string()));
    }
    expected.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.cmp(&y.1)));
    let expected: Vec<String> = expected.into_iter().map(|(_, id)| id).collect();

    let list = |filter: KeyAttestationFilter, cursor: Option<String>, limit: u32| {
        let c = c.clone();
        async move {
            store
                .list_key_attestations(&c, &filter, cursor.as_deref(), limit)
                .await
        }
    };
    let whole = list(Default::default(), None, 50).await.expect("list");
    let ids: Vec<String> = whole
        .items
        .iter()
        .map(|i| i.ctx_id.as_str().to_string())
        .collect();
    assert_eq!(
        ids, expected,
        "[{label}] newest first, the controller's own, retracted included"
    );
    assert!(
        whole.next_cursor.is_none(),
        "[{label}] one page holds everything"
    );

    let mut paged = Vec::new();
    let mut cursor = None;
    for _ in 0..10 {
        let page = list(Default::default(), cursor.take(), 2)
            .await
            .expect("page");
        assert!(page.items.len() <= 2, "[{label}]");
        paged.extend(page.items.iter().map(|i| i.ctx_id.as_str().to_string()));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(
        paged, expected,
        "[{label}] cursor pages are disjoint and complete"
    );

    for item in &whole.items {
        let stored = store.get(&item.ctx_id).expect("get").expect("stored");
        assert_eq!(item.content_hash, stored.body.content_hash, "[{label}]");
        assert_eq!(
            Some(&item.metadata),
            stored.body.metadata.as_ref(),
            "[{label}]"
        );
    }

    let filter = |issuer: Option<&str>, subject: Option<&str>| KeyAttestationFilter {
        issuer: issuer.map(str::to_string),
        subject: subject.map(str::to_string),
    };
    let gh = "https://token.actions.githubusercontent.com";
    let count =
        |f: KeyAttestationFilter| async { list(f, None, 50).await.expect("list").items.len() };
    assert_eq!(
        count(filter(Some(gh), Some(a))).await,
        3,
        "[{label}] issuer + subject"
    );
    assert_eq!(
        count(filter(Some(gh), None)).await,
        4,
        "[{label}] issuer excludes SPIFFE"
    );
    assert_eq!(
        count(filter(
            None,
            Some("spiffe://prod.example.com/ns/agents/sa/publisher")
        ))
        .await,
        1,
        "[{label}] subject alone matches SPIFFE"
    );
    assert_eq!(
        count(filter(Some(&format!("{gh}/")), None)).await,
        0,
        "[{label}] the issuer is compared byte for byte"
    );
    assert_eq!(
        list(Default::default(), None, 50)
            .await
            .map(|p| p.items.len())
            .unwrap_or_default(),
        5
    );
    assert_eq!(
        store
            .list_key_attestations(&controller(label), &Default::default(), None, 50)
            .await
            .expect("list")
            .items
            .len(),
        0,
        "[{label}] an unknown controller lists nothing"
    );
    let err = list(Default::default(), Some("not a cursor!".into()), 2)
        .await
        .expect_err("malformed cursor");
    assert!(
        matches!(err, AcdpError::InvalidCursor(_)),
        "[{label}] {err:?}"
    );
}

/// **D19: the record is the index row joined to `contexts`.** Its
/// `content_hash` is the stored body's, its status follows the context
/// (retracted after an admin retraction), and its JWK is the minted one.
/// A ctx_id with no index row (absent, or an ordinary body) is `None`.
pub async fn assert_key_attestation_record_join<S>(store: &Arc<S>, label: &str)
where
    S: ExtendedRegistryStore + 'static,
{
    use acdp::types::primitives::Status;
    use keyless_fixtures::*;

    let c = controller(label);
    let m = mint(
        store,
        &Spec::new(
            &c,
            spiffe("spiffe://prod.example.com/ns/agents/sa/publisher"),
        )
        .org_wide(),
    )
    .await;
    let stored = store.get(&m.ctx_id).expect("get").expect("stored");
    let r = store
        .key_attestation(m.ctx_id.as_str())
        .await
        .expect("record")
        .expect("indexed");
    assert_eq!(r.ctx_id, m.ctx_id, "[{label}]");
    assert_eq!(r.content_hash, stored.body.content_hash, "[{label}]");
    assert_eq!(r.content_hash, m.content_hash, "[{label}]");
    assert_eq!(r.status, Status::Active, "[{label}]");
    assert_eq!(r.attestation, m.attestation, "[{label}]");
    assert_eq!(
        r.attestation.public_key_jwk, m.attestation.public_key_jwk,
        "[{label}] the JWK §6.3 step 8 verifies under"
    );
    assert_eq!(
        Some(&r.metadata),
        stored.body.metadata.as_ref(),
        "[{label}]"
    );
    assert_eq!(
        r.created_at_ms,
        stored.body.created_at.timestamp_millis(),
        "[{label}]"
    );

    retract(store, &m.ctx_id).await;
    let r = store
        .key_attestation(m.ctx_id.as_str())
        .await
        .expect("record")
        .expect("indexed");
    assert_eq!(
        r.status,
        Status::Retracted,
        "[{label}] the status follows the context"
    );

    let plain = inserted(
        commit(store, controller_request(&c, None), None).await,
        "plain",
    );
    assert!(
        store
            .key_attestation(plain.as_str())
            .await
            .expect("record")
            .is_none(),
        "[{label}] an ordinary body is no attestation"
    );
    assert!(
        store
            .key_attestation(&format!(
                "acdp://{AUTHORITY}/00000000-0000-4000-8000-00000000beef"
            ))
            .await
            .expect("record")
            .is_none(),
        "[{label}]"
    );
}

#[cfg(test)]
mod lifecycle_probe_tests {
    use super::*;
    use acdp::error::AcdpError;
    use acdp::registry::{IdempotencyRecord, InMemoryStore, LifecycleCommitOutcome};
    use acdp::types::body::{Body, FullContext};
    use acdp::types::primitives::{ContentHash, LineageId};
    use acdp::types::publish::PublishResponse;
    use acdp::types::search::SearchResponse;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The SDK's in-memory store, plus an honest `has_lifecycle_state`: it
    /// remembers whether a lifecycle event was ever committed. That stands in
    /// for a durable backend that overrides the probe correctly.
    ///
    /// With `ignore_signatures` set it is instead the double for
    /// [`assert_lifecycle_retry_requires_identical_signature`]: every event
    /// has its signature stripped before it reaches the inner store, so a
    /// retry differing only in `signature.value` compares equal — the defect
    /// that assertion exists to catch.
    #[derive(Default)]
    struct Recording {
        inner: InMemoryStore,
        saw_event: AtomicBool,
        ignore_signatures: bool,
    }

    impl acdp::registry::RegistryStore for Recording {
        fn put(&self, body: Body) -> Result<(), AcdpError> {
            self.inner.put(body)
        }
        fn get(&self, ctx_id: &CtxId) -> Result<Option<FullContext>, AcdpError> {
            self.inner.get(ctx_id)
        }
        fn lineage(&self, lineage_id: &LineageId) -> Result<Vec<FullContext>, AcdpError> {
            self.inner.lineage(lineage_id)
        }
        fn current(&self, lineage_id: &LineageId) -> Result<Option<FullContext>, AcdpError> {
            self.inner.current(lineage_id)
        }
        fn mark_superseded(&self, ctx_id: &CtxId) -> Result<(), AcdpError> {
            self.inner.mark_superseded(ctx_id)
        }
        fn first_version_ctx_id(&self, lineage_id: &LineageId) -> Result<Option<CtxId>, AcdpError> {
            self.inner.first_version_ctx_id(lineage_id)
        }
        fn search(
            &self,
            params: &SearchParams,
            requester: Option<&AgentDid>,
            public_arm_open: bool,
        ) -> Result<SearchResponse, AcdpError> {
            self.inner.search(params, requester, public_arm_open)
        }
        fn idempotency_lookup(
            &self,
            agent_id: &AgentDid,
            key: &str,
        ) -> Result<Option<IdempotencyRecord>, AcdpError> {
            self.inner.idempotency_lookup(agent_id, key)
        }
        fn idempotency_record(
            &self,
            agent_id: &AgentDid,
            key: &str,
            hash: &ContentHash,
            response: &PublishResponse,
            expires_at: DateTime<Utc>,
        ) -> Result<(), AcdpError> {
            self.inner
                .idempotency_record(agent_id, key, hash, response, expires_at)
        }
        fn idempotency_evict_expired(&self, now: DateTime<Utc>) -> Result<(), AcdpError> {
            self.inner.idempotency_evict_expired(now)
        }
        fn commit_publish(
            &self,
            commit: PublishCommit<'_>,
        ) -> Result<PublishCommitOutcome, AcdpError> {
            self.inner.commit_publish(commit)
        }
        fn commit_lifecycle_event(
            &self,
            event: &LifecycleEvent,
        ) -> Result<LifecycleCommitOutcome, AcdpError> {
            let outcome = if self.ignore_signatures {
                let mut blind = event.clone();
                blind.signature = None;
                self.inner.commit_lifecycle_event(&blind)?
            } else {
                self.inner.commit_lifecycle_event(event)?
            };
            self.saw_event.store(true, Ordering::SeqCst);
            Ok(outcome)
        }
    }

    #[async_trait::async_trait]
    impl ExtendedRegistryStore for Recording {
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
        ) -> Result<crate::Page<FullContext>, AcdpError> {
            unimplemented!("not reached by the lifecycle probe kit")
        }
        async fn has_lifecycle_state(&self) -> Result<bool, AcdpError> {
            Ok(self.saw_event.load(Ordering::SeqCst))
        }
    }

    /// Positive control: a store that overrides the probe passes. Without
    /// this, the `should_panic` below could be passing because the fixture
    /// itself panics (a publish or retract failing), not because the probe
    /// answered `false`.
    #[tokio::test]
    async fn a_store_that_overrides_the_probe_passes() {
        let store = Arc::new(Recording::default());
        assert_lifecycle_state_probe_sees_retraction(&store, "recording").await;
    }

    /// A backend that inherits the trait defaults has no change feed: every
    /// feed method is `NotImplemented` (served as 501), never an empty feed
    /// a consumer would mistake for "nothing changed".
    #[tokio::test]
    async fn a_store_that_inherits_the_feed_defaults_is_not_implemented() {
        use acdp::error::AcdpError;
        let store = DefaultOnly(Arc::new(Recording::default()));
        assert!(matches!(
            store.changes_head().await,
            Err(AcdpError::NotImplemented(_))
        ));
        assert!(matches!(
            store.changes_page(&crate::FeedQuery::default()).await,
            Err(AcdpError::NotImplemented(_))
        ));
        assert!(matches!(
            store.prune_changes(0, 1).await,
            Err(AcdpError::NotImplemented(_))
        ));
    }

    /// The falsification: the same store behind `DefaultOnly`, which forwards
    /// every write but does NOT forward `has_lifecycle_state`, so the trait
    /// default answers. The retraction lands in the inner store, the probe
    /// still says `false`, and the kit must fail.
    #[tokio::test]
    #[should_panic(expected = "has_lifecycle_state() returned false")]
    async fn a_store_that_inherits_the_default_probe_fails() {
        let store = Arc::new(DefaultOnly(Arc::new(Recording::default())));
        assert_lifecycle_state_probe_sees_retraction(&store, "default-only").await;
    }

    /// Positive control for the signature-retry assertion: the SDK's
    /// in-memory store compares whole events, so it passes. Without this the
    /// `should_panic` below could be passing on a fixture failure.
    #[tokio::test]
    async fn a_store_that_compares_signatures_passes_the_retry_check() {
        let store = Arc::new(Recording::default());
        assert_lifecycle_retry_requires_identical_signature(&store, "recording").await;
    }

    /// The falsification: a store that ignores signatures replays the
    /// signature-only twin as idempotent, and the kit must fail on exactly
    /// that leg.
    #[tokio::test]
    #[should_panic(expected = "a retry differing ONLY in signature.value was replayed")]
    async fn a_store_that_ignores_signatures_fails_the_retry_check() {
        let store = Arc::new(Recording {
            ignore_signatures: true,
            ..Recording::default()
        });
        assert_lifecycle_retry_requires_identical_signature(&store, "signature-blind").await;
    }
}
