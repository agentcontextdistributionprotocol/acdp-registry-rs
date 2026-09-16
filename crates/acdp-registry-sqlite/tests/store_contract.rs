//! Concurrency contract tests for the atomic publish commit (REG-3.3).
//!
//! Ports the SDK's `tests/store_contract.rs` scenarios (written against
//! the reference `InMemoryStore`) to the SQLite backend, driving the
//! same `RegistryStore::commit_publish` contract:
//!
//! 1. **Idempotency atomicity** — N concurrent publishes sharing an
//!    `(agent_id, idempotency_key)` mint exactly ONE `ctx_id`; every
//!    racer observes the winner's exact response (RFC-ACDP-0003 §6.2.2).
//! 2. **No idempotency key** — the same N racing publishes each mint a
//!    distinct context (the key is absent, not half-honored).
//! 3. **Supersession serialization** — N concurrent v2 publishes racing
//!    to supersede the same v1 produce exactly ONE winner; every loser
//!    fails with `superseded_target` (RFC-ACDP-0003 §3.1 step 6,
//!    RFC-ACDP-0008 §3.10).
//!
//! `commit_publish` is a sync API that drives async sqlx through
//! `block_in_place`, so every racer runs on the tokio blocking pool
//! (`spawn_blocking`) under a multi-threaded runtime — the same setup as
//! the crate's in-module `commit_publish` race tests.

use std::sync::Arc;

use acdp::crypto::SigningKey;
use acdp::error::AcdpError;
use acdp::producer::Producer;
use acdp::registry::store::{
    PendingIdempotencyCommit, PublishCommit, PublishCommitOutcome, RegistryStore,
};
use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
use acdp::types::publish::{PublishRequest, PublishResponse};
use acdp::types::search::SearchParams;
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use chrono::{Duration, Utc};

const THREADS: usize = 16;
const AUTHORITY: &str = "reg.test";

/// The database file name inside each test's own temp DIRECTORY.
///
/// Mirrors `acdp-registry-server`'s `common::DB_FILE_NAME`, which cannot be
/// imported here — it lives in a different crate's test support module. The
/// value is arbitrary; what matters is that the directory, not the file, is
/// what the test owns.
const DB_FILE_NAME: &str = "registry.sqlite";

/// Own the temp DIRECTORY, not the temp FILE.
///
/// `tempfile::NamedTempFile` deletes exactly the path it owns, while SQLite
/// writes two sidecars beside it (`-wal` and `-shm`). Those are not the owned
/// path, so they outlive the test. #309 fixed this in the server harness;
/// measured here at `7b3797e`, this crate's suite still leaked **34** files
/// into `$TMPDIR` per run. A `TempDir` removes the whole directory, sidecars
/// included.
///
/// `acdp-registry-server/tests/tmpdir_hygiene.rs` is the standing guard for
/// the server side; it does not cover this crate, which is why the leak
/// survived #309.
async fn store() -> (Arc<SqliteStore>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::connect(&dir.path().join(DB_FILE_NAME), 4)
        .await
        .unwrap();
    store.migrate().await.unwrap();
    (Arc::new(store), dir)
}

fn producer(seed: u8) -> Producer {
    Producer::new(
        SigningKey::from_bytes(&[seed; 32]),
        AgentDid::new(format!("did:web:agents.test:contract-{seed}")),
        format!("did:web:agents.test:contract-{seed}#key-1"),
    )
}

/// The DID `producer(seed)` signs as. Mirrors the construction in `producer`
/// above — kept beside it so the two cannot drift.
fn did(seed: u8) -> AgentDid {
    AgentDid::new(format!("did:web:agents.test:contract-{seed}"))
}

fn request(p: &Producer, title: &str) -> PublishRequest {
    p.publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid publish request")
}

fn commit(
    store: Arc<SqliteStore>,
    req: PublishRequest,
    idem_key: Option<String>,
) -> tokio::task::JoinHandle<Result<PublishCommitOutcome, AcdpError>> {
    tokio::task::spawn_blocking(move || {
        store.commit_publish(PublishCommit {
            req: &req,
            authority: AUTHORITY,
            idempotency: idem_key.as_deref().map(|key| PendingIdempotencyCommit {
                key,
                ttl: chrono::Duration::hours(1),
            }),
            tenant: None,
            receipt_minter: None,
            predecessor_admission: None,
        })
    })
}

/// Publish one context through the real commit path and hand back its `ctx_id`.
fn publish(
    store: &Arc<SqliteStore>,
    req: PublishRequest,
) -> impl std::future::Future<Output = CtxId> + use<> {
    let store = Arc::clone(store);
    async move {
        let outcome = commit(store, req, None)
            .await
            .expect("join")
            .expect("commit ok");
        response(&outcome).ctx_id.clone()
    }
}

fn tagged(p: &Producer, title: &str, tags: Vec<&str>) -> PublishRequest {
    p.publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .tags(tags)
        .build()
        .expect("valid publish request")
}

fn typed(p: &Producer, title: &str, t: ContextType) -> PublishRequest {
    p.publish_request()
        .title(title)
        .context_type(t)
        .visibility(Visibility::Public)
        .build()
        .expect("valid publish request")
}

fn expiring(p: &Producer, title: &str, at: chrono::DateTime<Utc>) -> PublishRequest {
    p.publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .expires_at(at)
        .build()
        .expect("valid publish request")
}

/// Run a search as an anonymous reader with the public arm open, and return the
/// matched `ctx_id`s as a set. Every U-550 assertion is on an EXACT set, because
/// several of the mutants WIDEN the result rather than emptying it.
fn search_ids(store: &Arc<SqliteStore>, params: SearchParams) -> std::collections::HashSet<String> {
    store
        .search(&params, None, true)
        .expect("search ok")
        .matches
        .iter()
        .map(|m| m.ctx_id.as_str().to_string())
        .collect()
}

fn response(outcome: &PublishCommitOutcome) -> &PublishResponse {
    match outcome {
        PublishCommitOutcome::Inserted(r) | PublishCommitOutcome::IdempotentReplay(r) => r,
    }
}

/// An UNEXPIRED idempotency record must REPLAY, and the TTL comparison is what
/// decides that.
///
/// `store.rs:994` is `if expires_at > now`. U-540 measured `<`, `==` and `>=`
/// all surviving there, and U-544 concluded all three were outcome-equivalent.
///
/// **U-552 corrected that: `<` and `==` are KILLABLE and are now killed** — by
/// `a_keyed_superseding_publish_replays_instead_of_failing_as_already_superseded`
/// below, not by this test. Both come back `CaughtMutant` under
/// `cargo mutants -F '^…$'`, with that test the sole failure.
///
/// The probe that produced the old label was CORRECT, and its generalisation was
/// not. With `<` applied, *this* test still passes: skipping the TTL branch lets
/// the publish proceed to `INSERT … ON CONFLICT(agent_id, key) DO NOTHING`, which
/// collides with the live record, reports zero rows, rolls the new context back
/// and replays the stored response — the SAME `IdempotentReplay`, with the SAME
/// `ctx_id`, by a second route.
///
/// That holds **only because this request does not supersede.** The second
/// enforcement lives at `store.rs:1284-1318`, which is reached only AFTER step 2;
/// a superseding replay hits step 2's coherence check at `store.rs:1118-1125`
/// first and returns `Err(SupersededTarget { AlreadySuperseded })`. So the
/// redundancy this docstring describes is real but partial, and "the branch is
/// redundant" was true of the case probed rather than of the branch.
///
/// This test therefore still does NOT kill any of the three, and is not claimed
/// to. It pins the contract itself, which was otherwise asserted nowhere at this
/// layer: a repeated keyed publish returns the original context rather than
/// minting a second one.
///
/// The assertion is on `ctx_id` EQUALITY across the two calls, not merely on the
/// `IdempotentReplay` variant: a replay that returned a different context would
/// satisfy the variant while breaking the guarantee.
///
/// `>` → `>=` IS genuinely equivalent and remains a budgeted survivor. The reason
/// is stronger than the millisecond-boundary argument previously given here: step
/// 1 first runs `DELETE … WHERE expires_at_ms <= ?` bound to `now`
/// (`store.rs:963-966`), so any surviving row has `expires_at_ms >= now_ms + 1`
/// and `expires_at > now` is unconditionally TRUE for it. `>=` is true on exactly
/// the same inputs — no claim about clock resolution needed, because the DELETE
/// makes the boundary case unreachable outright.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unexpired_idempotency_record_replays_rather_than_minting_again() {
    let (store, _tmp) = store().await;
    let p = producer(65);
    let key = "ttl-replay-key".to_string();

    let first = commit(
        Arc::clone(&store),
        request(&p, "ttl replay row"),
        Some(key.clone()),
    )
    .await
    .unwrap()
    .expect("first publish ok");
    assert!(
        matches!(first, PublishCommitOutcome::Inserted(_)),
        "the first publish under a fresh key must INSERT"
    );
    let first_ctx = response(&first).ctx_id.clone();

    // Same key, same content, well inside the 1h TTL the helper sets.
    let second = commit(
        Arc::clone(&store),
        request(&p, "ttl replay row"),
        Some(key.clone()),
    )
    .await
    .unwrap()
    .expect("second publish ok");
    assert!(
        matches!(second, PublishCommitOutcome::IdempotentReplay(_)),
        "an unexpired record must replay, returning the original context. NOTE: \
         `< now` / `== now` do NOT fail here -- step 7's ON CONFLICT fallback \
         replays by a second route for a NON-superseding request, which is why \
         those two mutants are killed by the superseding test below and not by \
         this one. A failure here is a break in the idempotency contract itself"
    );
    assert_eq!(
        response(&second).ctx_id,
        first_ctx,
        "the replay must return the ORIGINAL context; a different ctx_id is a          duplicate publish wearing a replay's variant"
    );

    // Exactly one context was minted, which is the property the key sells.
    assert_eq!(
        store.count_idempotency_records().await.expect("count ok"),
        Some(1),
        "one key, one retained record"
    );
}

/// Racing publishes that share one idempotency key but carry DIFFERENT content
/// must yield exactly one winner and reject every loser as a duplicate.
///
/// `store.rs:1306` (`if prior_hash != req.content_hash.0`, where U-540 measured
/// `!=` → `==` surviving) sits behind this path. **This test does not reach it,
/// and does not claim to.** Probed: an `eprintln!` at `inserted == 0` fires
/// ZERO times across the whole suite, this test included. SQLite's
/// `BEGIN IMMEDIATE` serialises the racers, so every loser finds the committed
/// record at the step-1 read and is refused there (`store.rs:1003`) instead.
///
/// That leaves :1306 reachable only under interleaving this harness does not
/// produce — recorded in #307 as needing a seam, not as a coverage gap a test
/// can close by trying harder.
///
/// It has to be a real race. The obvious deterministic route — pre-expire the
/// record so the `ON CONFLICT DO NOTHING` collides — does not work: step 1
/// DELETEs an expired record for this key (`store.rs:964`) precisely so the
/// claim in step 7 cannot collide with a stale row. Tried, and it published
/// cleanly. So :1306 is reachable only when a record is absent at the read and
/// present at the insert, which is the race window itself.
///
/// The existing `concurrent_identical_idempotency_key_mints_exactly_one_ctx_id`
/// races the SAME content, where the hashes match and this comparison is never
/// the deciding branch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_publishes_sharing_a_key_with_different_content_reject_the_losers() {
    let (store, _tmp) = store().await;
    let p = producer(66);
    let key = "race-different-content".to_string();

    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            commit(
                Arc::clone(&store),
                request(&p, &format!("racer content {i}")),
                Some(key.clone()),
            )
        })
        .collect();

    let mut inserted = 0usize;
    let mut duplicates = 0usize;
    let mut other: Vec<String> = Vec::new();
    for h in handles {
        match h.await.unwrap() {
            Ok(PublishCommitOutcome::Inserted(_)) => inserted += 1,
            Err(AcdpError::DuplicatePublish(_)) => duplicates += 1,
            Ok(o) => other.push(format!("unexpected Ok: {o:?}")),
            Err(e) => other.push(format!("unexpected Err: {e:?}")),
        }
    }

    assert!(
        other.is_empty(),
        "every racer must either win outright or be refused as a duplicate;          got {other:?}. An `IdempotentReplay` here is the `==` inversion: the          loser's DIFFERENT content was matched against the winner's hash and          accepted, handing it the winner's ctx_id"
    );
    assert_eq!(inserted, 1, "exactly one racer may mint a context");
    assert_eq!(
        duplicates,
        THREADS - 1,
        "every other racer carries different content under the same key and          must be refused"
    );
    assert_eq!(
        store.count_idempotency_records().await.expect("count ok"),
        Some(1),
        "one key, one retained record"
    );
}

/// A CONTRIBUTOR on v1 may supersede it — and that is what the `==` at
/// `store.rs:1090` decides.
///
/// Ownership for supersession is `prev_agent == req.agent_id
/// || prev_contributors.iter().any(|c| c == req.agent_id)`. U-540 measured the
/// contributor arm's `==` → `!=` surviving the whole suite, because every
/// existing supersession test supersedes as the ORIGINAL PRODUCER, where the
/// first arm already returns true and the second is never consulted.
///
/// Inverted, the arm means "any contributor who is NOT you", so:
///   * a genuine contributor is refused (this test fails), and
///   * worse, any signer is admitted whenever v1 lists at least one
///     contributor other than them — the lineage takeover the comment at that
///     site says the check exists to prevent (RFC-ACDP-0001 §5.9).
///
/// Both directions are asserted below, because the refusal alone would also be
/// produced by a broken lookup, while the pair pins the comparison itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contributor_may_supersede_but_an_unrelated_signer_may_not() {
    let (store, _tmp) = store().await;
    let owner = producer(67);
    let contributor = producer(68);
    let stranger = producer(69);

    // v1 is owned by `owner` and lists `contributor` — and NOT `stranger`.
    let v1_req = owner
        .publish_request()
        .title("contributor supersession v1")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .contributors(vec![did(68)])
        .build()
        .expect("valid v1 request");
    let v1 = commit(Arc::clone(&store), v1_req, None)
        .await
        .unwrap()
        .expect("v1 publish");
    let v1_body = store
        .get(&response(&v1).ctx_id)
        .expect("retrieve ok")
        .expect("v1 present")
        .body;

    // A signer who is neither the producer nor a contributor is refused, and
    // is told only "not found" — no existence oracle.
    let stranger_req = stranger
        .supersede_body(&v1_body)
        .title("takeover attempt")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid request");
    let err = commit(Arc::clone(&store), stranger_req, None)
        .await
        .unwrap()
        .expect_err("a non-owner, non-contributor must not supersede");
    assert!(
        matches!(err, AcdpError::SupersededTarget { .. }),
        "expected SupersededTarget for an unrelated signer, got {err:?}. With          the contributor arm inverted this signer is ADMITTED whenever v1          lists any contributor other than them — a lineage takeover"
    );

    // The listed contributor succeeds. This is the half no existing test
    // covers, because every other supersession runs as the original producer.
    let contrib_req = contributor
        .supersede_body(&v1_body)
        .title("contributor supersession v2")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid request");
    let v2 = commit(Arc::clone(&store), contrib_req, None)
        .await
        .unwrap()
        .expect("a listed contributor MUST be allowed to supersede");
    assert_eq!(
        response(&v2).lineage_id,
        response(&v1).lineage_id,
        "the contributor's v2 continues v1's lineage"
    );
}

/// `connect` must create a missing parent directory chain.
///
/// `store.rs:49` is `if !parent.as_os_str().is_empty()`, guarding
/// `create_dir_all(parent)`. U-540 measured `delete !` surviving the whole
/// suite, and the reason is that every other test hands `connect` a path whose
/// parent ALREADY EXISTS — `tempfile::tempdir()` creates it. When the directory
/// is already there, `create_dir_all` is a no-op, so skipping it changes
/// nothing and the mutant is invisible.
///
/// Inverted, the guard means "create the parent only when there ISN'T one",
/// so a real deployment pointed at `/var/lib/acdp/registry.sqlite` before that
/// directory exists fails to start, while the no-parent case calls
/// `create_dir_all("")`.
///
/// The precondition is asserted explicitly: without it this test would pass
/// against the mutant the moment someone changed the fixture to a directory
/// that happens to exist.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_creates_a_missing_parent_directory_chain() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join(DB_FILE_NAME);
    let parent = nested
        .parent()
        .expect("nested path has a parent")
        .to_path_buf();

    assert!(
        !parent.exists(),
        "precondition: the parent chain must be ABSENT, or this test passes \
         without exercising create_dir_all at all"
    );

    let store = SqliteStore::connect(&nested, 2)
        .await
        .expect("connect must create the missing parent chain, not fail on it");
    store.migrate().await.expect("migrate");

    assert!(parent.is_dir(), "the parent chain was created");
    assert!(
        nested.is_file(),
        "the database exists at the requested path"
    );
}

/// `count_idempotency_records` must count the rows that exist.
///
/// Measured in U-540: replacing the whole method with `Ok(Some(0))` left the
/// suite green. The count feeds operational reporting, so a constant zero
/// reads as "no idempotency records are being retained" — the shape of answer
/// that makes an eviction bug invisible rather than loud.
///
/// Asserted as a DELTA across a publish, not as a single figure. `Some(0)` on
/// an empty store is the correct answer, so a test that only checked the
/// populated case would pass against a method that always returns zero for
/// exactly one of its two observations; pinning both ends removes that.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn count_idempotency_records_counts_the_rows_that_exist() {
    let (store, _tmp) = store().await;

    let empty = store.count_idempotency_records().await.expect("count ok");
    assert_eq!(
        empty,
        Some(0),
        "a fresh store retains no idempotency records"
    );

    let p = producer(64);
    commit(
        Arc::clone(&store),
        request(&p, "counted row"),
        Some("count-key-1".to_string()),
    )
    .await
    .unwrap()
    .expect("publish ok");

    let populated = store.count_idempotency_records().await.expect("count ok");
    assert_eq!(
        populated,
        Some(1),
        "one keyed publish retains exactly one idempotency record; a constant          `Some(0)` here would report an empty table over a populated one"
    );
}

/// Race N identical publishes sharing one idempotency key: exactly one
/// context is minted; every racer observes the winner's exact response.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_identical_idempotency_key_mints_exactly_one_ctx_id() {
    let (store, _tmp) = store().await;
    let p = producer(21);
    let req = request(&p, "idempotent-under-race");

    let handles: Vec<_> = (0..THREADS)
        .map(|_| commit(Arc::clone(&store), req.clone(), Some("contract-key".into())))
        .collect();
    let mut outcomes = Vec::new();
    for h in handles {
        outcomes.push(
            h.await
                .unwrap()
                .expect("every replay of an identical publish must succeed"),
        );
    }

    let winner = response(&outcomes[0]).clone();
    for o in &outcomes {
        let r = response(o);
        assert_eq!(r.ctx_id, winner.ctx_id, "all racers observe one ctx_id");
        assert_eq!(r.lineage_id, winner.lineage_id);
        assert_eq!(r.created_at, winner.created_at, "replay is byte-identical");
        assert_eq!(r.version, winner.version);
    }
    let inserted = outcomes
        .iter()
        .filter(|o| matches!(o, PublishCommitOutcome::Inserted(_)))
        .count();
    assert_eq!(inserted, 1, "exactly one publish inserts; the rest replay");

    // Exactly one context persisted under the lineage.
    let lineage = store.lineage(&winner.lineage_id).expect("lineage query");
    assert_eq!(lineage.len(), 1, "exactly one persisted context");
}

/// Without an idempotency key, the same N racing publishes all mint
/// distinct contexts.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishes_without_idempotency_all_mint_distinct() {
    let (store, _tmp) = store().await;
    let p = producer(22);
    let req = request(&p, "no-idem-under-race");

    let handles: Vec<_> = (0..THREADS)
        .map(|_| commit(Arc::clone(&store), req.clone(), None))
        .collect();
    let mut ctx_ids = Vec::new();
    for h in handles {
        let outcome = h.await.unwrap().expect("publish succeeds");
        ctx_ids.push(response(&outcome).ctx_id.as_str().to_string());
    }

    ctx_ids.sort();
    ctx_ids.dedup();
    assert_eq!(
        ctx_ids.len(),
        THREADS,
        "every racing publish mints its own ctx_id when no idempotency key is given"
    );
}

/// Race N distinct v2 publishes superseding the same v1: exactly one
/// winner; every loser fails with `superseded_target`; the stored
/// lineage is exactly [v1, winning v2].
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_supersession_has_exactly_one_winner() {
    let (store, _tmp) = store().await;
    let p = producer(23);

    let v1_outcome = commit(Arc::clone(&store), request(&p, "v1"), None)
        .await
        .unwrap()
        .expect("v1 publish");
    let v1_resp = response(&v1_outcome).clone();
    let v1_body = store
        .get(&v1_resp.ctx_id)
        .expect("retrieve ok")
        .expect("v1 present")
        .body;

    // N DISTINCT v2 requests (different titles → different content
    // hashes), all targeting the same predecessor.
    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let req = p
                .supersede_body(&v1_body)
                .title(format!("v2-candidate-{i}"))
                .context_type(ContextType::DataSnapshot)
                .visibility(Visibility::Public)
                .build()
                .expect("valid v2 request");
            commit(Arc::clone(&store), req, None)
        })
        .collect();
    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }

    let (winners, losers): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| r.is_ok());
    assert_eq!(winners.len(), 1, "exactly one supersession wins the race");
    for loser in &losers {
        let err = loser.as_ref().unwrap_err();
        assert!(
            matches!(err, AcdpError::SupersededTarget { .. }),
            "losers MUST fail with superseded_target, got {err:?}"
        );
    }

    let winner = response(&winners.into_iter().next().unwrap().unwrap()).clone();
    assert_eq!(winner.version, 2);
    assert_eq!(winner.lineage_id, v1_resp.lineage_id, "same lineage");

    // Lineage is exactly [v1 superseded, winning v2 active].
    let lineage = store.lineage(&v1_resp.lineage_id).expect("lineage query");
    assert_eq!(lineage.len(), 2, "exactly v1 + the single winning v2");
    assert_eq!(lineage[1].body.ctx_id, winner.ctx_id);
    let current = store
        .current(&v1_resp.lineage_id)
        .expect("current query")
        .expect("current exists");
    assert_eq!(current.body.ctx_id, winner.ctx_id);
}

// ─── Lifecycle events (RFC-ACDP-0013): the commit_lifecycle_event contract ───

mod lifecycle {
    use super::*;
    use acdp::registry::LifecycleCommitOutcome;
    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    use acdp::types::primitives::{CtxId, LineageId, Status};

    fn event(
        actor: &AgentDid,
        ctx_id: &CtxId,
        event_type: LifecycleEventType,
        reason: Option<&str>,
    ) -> LifecycleEvent {
        event_with_id(
            &uuid::Uuid::new_v4().to_string(),
            actor,
            ctx_id,
            event_type,
            reason,
        )
    }

    fn event_with_id(
        event_id: &str,
        actor: &AgentDid,
        ctx_id: &CtxId,
        event_type: LifecycleEventType,
        reason: Option<&str>,
    ) -> LifecycleEvent {
        // Signature verification is the SERVER's §6 step 3; the store
        // contract is exercised with unsigned events.
        LifecycleEvent::new(
            event_id.to_string(),
            ctx_id.clone(),
            event_type,
            chrono::Utc::now(),
            actor.clone(),
            reason.map(str::to_string),
        )
        .expect("valid event")
    }

    async fn published_ctx(store: &Arc<SqliteStore>, seed: u8, title: &str) -> (CtxId, LineageId) {
        let p = producer(seed);
        let outcome = commit(Arc::clone(store), request(&p, title), None)
            .await
            .unwrap()
            .unwrap();
        let r = response(&outcome);
        (r.ctx_id.clone(), r.lineage_id.clone())
    }

    /// `lifecycle_events_of_ctx` must return the events that were committed.
    ///
    /// Measured in U-540: replacing the WHOLE METHOD BODY with `Ok(vec![])`
    /// left the entire workspace suite green. Every other assertion about
    /// lifecycle state in this file reads the PROJECTED context (status,
    /// `retracted_at`, §7.2 precedence) rather than the event list itself, so
    /// the store could report "this context has no lifecycle history" and
    /// nothing noticed.
    ///
    /// The assertion is on CONTENT, not just length: a length check alone
    /// would be satisfied by any one event, which is a weaker claim than the
    /// method actually returning what was written.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_events_of_ctx_returns_the_committed_events() {
        let (store, _tmp) = store().await;
        let actor = AgentDid::new("did:web:agents.test:contract-63".to_string());
        let (ctx_id, _lineage) = published_ctx(&store, 63, "event list row").await;

        // Before any event, the list is empty for a real, existing context —
        // which also proves the emptiness asserted after the commit would be a
        // genuine change of state rather than a constant.
        let before = store
            .lifecycle_events_of_ctx(ctx_id.as_str())
            .await
            .expect("events ok");
        assert!(
            before.is_empty(),
            "a freshly published context has no events"
        );

        let retract = event(
            &actor,
            &ctx_id,
            LifecycleEventType::Retracted,
            Some("event list reason"),
        );
        let expected_event_id = retract.event_id.clone();
        store
            .commit_lifecycle_event(&retract)
            .expect("retract applied");

        let after = store
            .lifecycle_events_of_ctx(ctx_id.as_str())
            .await
            .expect("events ok");
        assert_eq!(
            after.len(),
            1,
            "one event was committed, so one must come back; `Ok(vec![])`              reports a context with no lifecycle history at all"
        );
        assert_eq!(
            after[0].event_id, expected_event_id,
            "the returned event must be the one committed, not merely some event"
        );
        assert_eq!(after[0].event_type, LifecycleEventType::Retracted);
        assert_eq!(after[0].ctx_id, ctx_id);
    }

    /// The documented 4-step atomic contract: resolve, retry-idempotency,
    /// strict alternation, append+status-effect — plus the read-side
    /// projections (§7.2 precedence, §8.2 search exclusion, §8.3 head
    /// exclusion) driven by the same committed state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_commit_contract() {
        let (store, _tmp) = store().await;
        let actor = AgentDid::new("did:web:agents.test:contract-61".to_string());
        let (ctx_id, lineage_id) = published_ctx(&store, 61, "lifecycle contract row").await;

        // Unknown ctx → NotFound (visibility is the server's job).
        let ghost = CtxId(format!(
            "acdp://{AUTHORITY}/00000000-0000-4000-8000-0000000000bb"
        ));
        let ghost_event = event(&actor, &ghost, LifecycleEventType::Retracted, None);
        assert!(matches!(
            store.commit_lifecycle_event(&ghost_event),
            Err(AcdpError::NotFound(_))
        ));

        // Retract → Applied with the §7.2-projected state.
        let retract = event(
            &actor,
            &ctx_id,
            LifecycleEventType::Retracted,
            Some("bad data"),
        );
        let applied = store.commit_lifecycle_event(&retract).unwrap();
        let ctx = match applied {
            LifecycleCommitOutcome::Applied(c) => c,
            other => panic!("expected Applied, got {other:?}"),
        };
        assert!(matches!(ctx.registry_state.status, Status::Retracted));
        assert_eq!(
            ctx.registry_state
                .lifecycle_events
                .as_deref()
                .unwrap()
                .len(),
            1
        );

        // Projections: get() serves the retracted status with the body
        // intact and the events attached; current() excludes the head;
        // default search excludes, status=retracted finds.
        let got = store.get(&ctx_id).unwrap().unwrap();
        assert!(matches!(got.registry_state.status, Status::Retracted));
        assert_eq!(got.body.title, "lifecycle contract row");
        assert_eq!(
            got.registry_state.lifecycle_events.as_deref().unwrap(),
            std::slice::from_ref(&retract)
        );
        assert!(store.current(&lineage_id).unwrap().is_none());
        let default_search = store
            .search(
                &acdp::types::search::SearchParams {
                    q: Some("lifecycle contract row".into()),
                    ..Default::default()
                },
                None,
                true,
            )
            .unwrap();
        assert!(default_search.matches.is_empty());
        let retracted_search = store
            .search(
                &acdp::types::search::SearchParams {
                    q: Some("lifecycle contract row".into()),
                    status: Some("retracted".into()),
                    ..Default::default()
                },
                None,
                true,
            )
            .unwrap();
        assert_eq!(retracted_search.matches.len(), 1);

        // Byte-identical retry → IdempotentReplay, nothing appended.
        let replay = store.commit_lifecycle_event(&retract).unwrap();
        let ctx = match replay {
            LifecycleCommitOutcome::IdempotentReplay(c) => c,
            other => panic!("expected IdempotentReplay, got {other:?}"),
        };
        assert_eq!(
            ctx.registry_state
                .lifecycle_events
                .as_deref()
                .unwrap()
                .len(),
            1
        );

        // Same event_id, different content → SchemaViolation.
        let divergent = event_with_id(
            &retract.event_id,
            &actor,
            &ctx_id,
            LifecycleEventType::Retracted,
            Some("a different reason"),
        );
        assert!(matches!(
            store.commit_lifecycle_event(&divergent),
            Err(AcdpError::SchemaViolation(_))
        ));

        // Double retract (fresh id) → InvalidLifecycleTransition.
        let double = event(&actor, &ctx_id, LifecycleEventType::Retracted, None);
        assert!(matches!(
            store.commit_lifecycle_event(&double),
            Err(AcdpError::InvalidLifecycleTransition(_))
        ));

        // Unregistered event_type → SchemaViolation (§7.3).
        let unregistered = event(
            &actor,
            &ctx_id,
            LifecycleEventType::Other("annotated".into()),
            None,
        );
        assert!(matches!(
            store.commit_lifecycle_event(&unregistered),
            Err(AcdpError::SchemaViolation(_))
        ));

        // Republish reverses; both events retained; head restored.
        let republish = event(&actor, &ctx_id, LifecycleEventType::Republished, None);
        let ctx = store
            .commit_lifecycle_event(&republish)
            .unwrap()
            .into_context();
        assert!(matches!(ctx.registry_state.status, Status::Active));
        assert_eq!(
            ctx.registry_state
                .lifecycle_events
                .as_deref()
                .unwrap()
                .len(),
            2
        );
        assert!(store.current(&lineage_id).unwrap().is_some());

        // Republish of a not-retracted context → InvalidLifecycleTransition.
        let spurious = event(&actor, &ctx_id, LifecycleEventType::Republished, None);
        assert!(matches!(
            store.commit_lifecycle_event(&spurious),
            Err(AcdpError::InvalidLifecycleTransition(_))
        ));
    }

    /// N concurrent retracts (distinct event_ids) racing the same
    /// context: exactly ONE applies; every loser gets the contract
    /// outcome (`invalid_lifecycle_transition`), never a lost update or
    /// a double append.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_retracts_have_exactly_one_winner() {
        let (store, _tmp) = store().await;
        let actor = AgentDid::new("did:web:agents.test:contract-62".to_string());
        let (ctx_id, _) = published_ctx(&store, 62, "race retract").await;

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let store = Arc::clone(&store);
                let e = event(&actor, &ctx_id, LifecycleEventType::Retracted, None);
                tokio::task::spawn_blocking(move || store.commit_lifecycle_event(&e))
            })
            .collect();
        let mut applied = 0usize;
        let mut conflicts = 0usize;
        for h in handles {
            match h.await.unwrap() {
                Ok(LifecycleCommitOutcome::Applied(_)) => applied += 1,
                Ok(LifecycleCommitOutcome::IdempotentReplay(_)) => {
                    panic!("distinct event_ids can never replay")
                }
                Err(AcdpError::InvalidLifecycleTransition(_)) => conflicts += 1,
                Err(other) => panic!("unexpected error under race: {other:?}"),
            }
        }
        assert_eq!(applied, 1, "exactly one retract wins");
        assert_eq!(conflicts, THREADS - 1, "every loser gets the 409 outcome");

        let ctx = store.get(&ctx_id).unwrap().unwrap();
        assert_eq!(
            ctx.registry_state
                .lifecycle_events
                .as_deref()
                .unwrap()
                .len(),
            1,
            "append-only history carries exactly the winner"
        );
    }
}

// ─── ACDP 0.3.0: transparency log (RFC-ACDP-0012) ──────────────────────────

mod transparency_log {
    use super::*;
    use acdp::types::body::Body;
    use acdp::types::log::{decode_sha256_hex, encode_sha256_hex};
    use acdp::types::receipt::ReceiptSigner;

    const REGISTRY_DID: &str = "did:web:reg.test";

    /// Owns the directory, not the file — see `super::store`.
    async fn log_store() -> (Arc<SqliteStore>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::connect(&dir.path().join(super::DB_FILE_NAME), 4)
            .await
            .unwrap()
            .with_transparency_log();
        store.migrate().await.unwrap();
        (Arc::new(store), dir)
    }

    fn signer() -> ReceiptSigner {
        ReceiptSigner::new(
            acdp::crypto::SigningKey::from_bytes(&[99u8; 32]),
            REGISTRY_DID,
            format!("{REGISTRY_DID}#receipt-key-1"),
        )
        .unwrap()
    }

    /// RFC-ACDP-0010 minter closure of the exact shape `RegistryServer`
    /// threads through `PublishCommit::receipt_minter`.
    fn mint_fn(
        signer: ReceiptSigner,
    ) -> impl Fn(&Body) -> Result<serde_json::Value, AcdpError> + Send + Sync {
        move |body: &Body| {
            let receipt = signer.mint(
                &body.ctx_id,
                &body.lineage_id,
                &body.origin_registry,
                body.created_at,
                &body.content_hash,
                &format!("sha256:{}", "c".repeat(64)),
            )?;
            serde_json::to_value(receipt).map_err(AcdpError::from)
        }
    }

    async fn counts(store: &SqliteStore) -> (i64, i64) {
        let (contexts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM contexts")
            .fetch_one(store.pool())
            .await
            .unwrap();
        let (leaves,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM log_leaves")
            .fetch_one(store.pool())
            .await
            .unwrap();
        (contexts, leaves)
    }

    /// §7.1: the leaf commits with the publish; §5.3: dense acceptance-
    /// order indexes; §4/§5.1: the stored bytes reproduce the stored
    /// hash; leaf count always equals context count under the profile.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn leaf_commits_atomically_and_is_reproducible() {
        let (store, _tmp) = log_store().await;
        let mint = mint_fn(signer());
        let mut ctx_ids = Vec::new();
        for i in 0..3u8 {
            let p = producer(140 + i);
            let req = request(&p, &format!("log-{i}"));
            let outcome = tokio::task::block_in_place(|| {
                store.commit_publish(PublishCommit {
                    req: &req,
                    authority: AUTHORITY,
                    idempotency: None,
                    tenant: None,
                    receipt_minter: Some(&mint),
                    predecessor_admission: None,
                })
            })
            .expect("logged publish succeeds");
            ctx_ids.push(response(&outcome).ctx_id.as_str().to_string());
        }

        assert_eq!(store.log_tree_size().await.unwrap(), 3);
        let (contexts, leaves) = counts(&store).await;
        assert_eq!(contexts, 3);
        assert_eq!(
            leaves, 3,
            "leaf count ≡ context count under the profile (§7.1)"
        );

        let entries = store.log_entries(0, 3).await.unwrap();
        assert_eq!(entries.len(), 3);
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(
                e.leaf_index, i as u64,
                "dense acceptance-order indexes (§5.3)"
            );
            assert_eq!(
                e.ctx_id, ctx_ids[i],
                "one leaf per ctx_id, in acceptance order"
            );
            // Reproducibility: rehash the exact stored bytes (§5.1).
            let rehashed = acdp::crypto::merkle::leaf_hash(e.leaf_json.as_bytes());
            assert_eq!(encode_sha256_hex(&rehashed), e.leaf_hash);
            // The stored bytes parse through the closed §4 schema and
            // hash identically through the typed path.
            assert_eq!(e.leaf().unwrap().leaf_hash_hex().unwrap(), e.leaf_hash);
            // Point lookups agree.
            let by_ctx = store.log_leaf_by_ctx(&e.ctx_id).await.unwrap().unwrap();
            assert_eq!(by_ctx.leaf_index, e.leaf_index);
            assert_eq!(by_ctx.leaf_json, e.leaf_json);
            let by_idx = store
                .log_leaf_by_index(e.leaf_index)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(by_idx.ctx_id, e.ctx_id);
        }
        // The density-checked hash projection succeeds and matches.
        let hashes = store.log_leaf_hashes(3).await.unwrap();
        assert_eq!(hashes.len(), 3);
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(hashes[i], decode_sha256_hex(&e.leaf_hash).unwrap());
        }
    }

    /// §7.1/§11 no degraded mode: a log-enabled store REFUSES a publish
    /// that arrives without a receipt minter — nothing is persisted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn publish_without_receipt_minter_is_refused_entirely() {
        let (store, _tmp) = log_store().await;
        let p = producer(150);
        let req = request(&p, "no-receipt");
        let err = tokio::task::block_in_place(|| {
            store.commit_publish(PublishCommit {
                req: &req,
                authority: AUTHORITY,
                idempotency: None,
                tenant: None,
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .expect_err("log-enabled publish without a receipt must fail");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
        assert_eq!(counts(&store).await, (0, 0), "nothing persists (§7.1)");
    }

    /// §7.1 crash-consistency: a failing receipt minter aborts the whole
    /// publish — no context row, no orphan leaf.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failing_minter_leaves_no_orphan_leaf() {
        let (store, _tmp) = log_store().await;
        let p = producer(151);
        let req = request(&p, "minter-fails");
        let failing = |_: &Body| -> Result<serde_json::Value, AcdpError> {
            Err(AcdpError::RegistryInternal("kms outage".into()))
        };
        let err = tokio::task::block_in_place(|| {
            store.commit_publish(PublishCommit {
                req: &req,
                authority: AUTHORITY,
                idempotency: None,
                tenant: None,
                receipt_minter: Some(&failing),
                predecessor_admission: None,
            })
        })
        .expect_err("failing minter must abort the publish");
        assert!(matches!(err, AcdpError::RegistryInternal(_)), "{err:?}");
        assert_eq!(
            counts(&store).await,
            (0, 0),
            "no orphan leaf or context (§7.1)"
        );
    }

    /// §5.3 under concurrency: racing logged publishes serialize on
    /// BEGIN IMMEDIATE and still assign dense, unique leaf indexes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_logged_publishes_assign_dense_indexes() {
        let (store, _tmp) = log_store().await;
        let handles: Vec<_> = (0..8u8)
            .map(|i| {
                let store = Arc::clone(&store);
                tokio::task::spawn_blocking(move || {
                    let p = producer(160 + i);
                    let req = request(&p, &format!("race-{i}"));
                    let mint = mint_fn(signer());
                    store.commit_publish(PublishCommit {
                        req: &req,
                        authority: AUTHORITY,
                        idempotency: None,
                        tenant: None,
                        receipt_minter: Some(&mint),
                        predecessor_admission: None,
                    })
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap().expect("every racer commits");
        }
        assert_eq!(store.log_tree_size().await.unwrap(), 8);
        // log_leaf_hashes enforces density over [0, 8).
        assert_eq!(store.log_leaf_hashes(8).await.unwrap().len(), 8);
        assert_eq!(counts(&store).await, (8, 8));
    }

    // ── Witness cosignature aggregation (RFC-ACDP-0015 §6.1) ───────────

    const LOG_ID: &str = "did:web:reg.test/log/1";
    const ROOT_5: &str = "sha256:0b00000000000000000000000000000000000000000000000000000000000000";
    const WITNESS_A: &str = "did:web:witness-a.example.org";
    const WITNESS_B: &str = "did:web:witness-b.example.org";

    fn cosig_json(witness: &str, at: &str) -> String {
        // A stand-in wire object; the store treats it as opaque bytes and
        // serves it back verbatim (verification happens in the aggregator).
        format!(r#"{{"witness_id":"{witness}","witnessed_at":"{at}"}}"#)
    }

    /// Upsert + read-back by the exact tuple, distinct witnesses counted
    /// once each, and tuple isolation (a cosignature for another
    /// tree_size/root is never returned for this one).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn witness_cosignatures_store_and_read_by_tuple() {
        let (store, _tmp) = log_store().await;

        // Two witnesses over the same (log_id, 5, ROOT_5) tuple.
        store
            .upsert_witness_cosignature(
                LOG_ID,
                5,
                ROOT_5,
                WITNESS_A,
                "2026-07-05T00:00:00.000Z",
                &cosig_json(WITNESS_A, "2026-07-05T00:00:00.000Z"),
            )
            .await
            .unwrap();
        store
            .upsert_witness_cosignature(
                LOG_ID,
                5,
                ROOT_5,
                WITNESS_B,
                "2026-07-05T00:00:01.000Z",
                &cosig_json(WITNESS_B, "2026-07-05T00:00:01.000Z"),
            )
            .await
            .unwrap();
        // A cosignature over a DIFFERENT tuple (size 6) must not leak in.
        let root_6 = format!("sha256:{}", "a".repeat(64));
        store
            .upsert_witness_cosignature(
                LOG_ID,
                6,
                &root_6,
                WITNESS_A,
                "2026-07-05T00:00:02.000Z",
                &cosig_json(WITNESS_A, "2026-07-05T00:00:02.000Z"),
            )
            .await
            .unwrap();

        let got = store
            .witness_cosignatures_for(LOG_ID, 5, ROOT_5)
            .await
            .unwrap();
        assert_eq!(got.len(), 2, "both distinct witnesses over the tuple");
        // Ordered by witness_did — A before B.
        assert_eq!(got[0]["witness_id"], WITNESS_A);
        assert_eq!(got[1]["witness_id"], WITNESS_B);

        // The size-6 tuple returns only its own cosignature.
        let got6 = store
            .witness_cosignatures_for(LOG_ID, 6, &root_6)
            .await
            .unwrap();
        assert_eq!(got6.len(), 1);
        assert_eq!(got6[0]["witness_id"], WITNESS_A);

        // A tuple with no cosignatures is empty, never an error.
        assert!(store
            .witness_cosignatures_for(LOG_ID, 99, ROOT_5)
            .await
            .unwrap()
            .is_empty());
    }

    /// A fresh re-observation from the same witness at the same tuple
    /// UPSERTs (newest witnessed_at wins) — one row per witness per tuple,
    /// cosignatures being ephemeral per-observation evidence (§4).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn witness_cosignature_reobservation_upserts() {
        let (store, _tmp) = log_store().await;
        store
            .upsert_witness_cosignature(
                LOG_ID,
                5,
                ROOT_5,
                WITNESS_A,
                "2026-07-05T00:00:00.000Z",
                &cosig_json(WITNESS_A, "2026-07-05T00:00:00.000Z"),
            )
            .await
            .unwrap();
        store
            .upsert_witness_cosignature(
                LOG_ID,
                5,
                ROOT_5,
                WITNESS_A,
                "2026-07-05T01:00:00.000Z",
                &cosig_json(WITNESS_A, "2026-07-05T01:00:00.000Z"),
            )
            .await
            .unwrap();
        let got = store
            .witness_cosignatures_for(LOG_ID, 5, ROOT_5)
            .await
            .unwrap();
        assert_eq!(got.len(), 1, "one row per (tuple, witness)");
        assert_eq!(
            got[0]["witnessed_at"], "2026-07-05T01:00:00.000Z",
            "newest wins"
        );
    }

    // ── log-001 golden fixture (spec schemas/conformance) ─────────────
    //
    // The fixture's ctx_ids live on registry.example.com, not this test
    // authority, so its leaves cannot arrive through publish; per the
    // fixture's own guidance the root reproduction is exercised at the
    // store layer: the pinned JCS leaf encodings are inserted as stored
    // rows and the store's hash projection must reproduce every pinned
    // §5.1 leaf hash, the pinned tree-size-5 root, and the pinned
    // inclusion path for leaf 0.

    const LOG_001_LEAVES: [&str; 5] = [
        r#"{"content_hash":"sha256:f170150ddbf59d99794e7797824591b374d459782084597b644ecc57a41031b5","created_at":"2026-04-16T10:30:15.123Z","ctx_id":"acdp://registry.example.com/12345678-1234-4321-8123-123456781234","key_fingerprint":"sha256:139e3940e64b5491722088d9a0d741628fc826e09475d341a780acde3c4b8070","leaf_version":"acdp-log-leaf/1","lineage_id":"lin:sha256:c7fef01c000f8edaa9cb46122ceb5d7bca38328f002fb0f40e362e3b289bbb2a","origin_registry":"registry.example.com","receipt_hash":"sha256:9deaa52778ad3b6be27a96d607c3017e9e11442905891a8972f34d8c2dbca9cf"}"#,
        r#"{"content_hash":"sha256:5b8be477da9b3e1354ebf2868494acb702301aaa825c1c3af3f92c5536ba7bd1","created_at":"2026-07-01T01:00:00.000Z","ctx_id":"acdp://registry.example.com/00000000-0000-4000-8000-000000000001","key_fingerprint":"sha256:139e3940e64b5491722088d9a0d741628fc826e09475d341a780acde3c4b8070","leaf_version":"acdp-log-leaf/1","lineage_id":"lin:sha256:a65dce2bc7d3d2f52513c14c9d7262903c960490b17308b272981240a76c2d42","origin_registry":"registry.example.com","receipt_hash":"sha256:2b8fa37afe87358aa039e78802f4a9b9fb4bc5df2a814a3f7cf5200f7f64b3df"}"#,
        r#"{"content_hash":"sha256:a0c8d76890ec38db8791e82d7a8e24194f84c13ae67bdaa167540b58cb95507b","created_at":"2026-07-02T02:00:00.000Z","ctx_id":"acdp://registry.example.com/00000000-0000-4000-8000-000000000002","key_fingerprint":"sha256:139e3940e64b5491722088d9a0d741628fc826e09475d341a780acde3c4b8070","leaf_version":"acdp-log-leaf/1","lineage_id":"lin:sha256:518c191ba24d2fea433a768e232cb1d0ff152a39b38f28ac7f91960c9f8f7aba","origin_registry":"registry.example.com","receipt_hash":"sha256:591fa4c29669546b777bd1a4583aa724e9586b083c096d4b62f68b630dd18834"}"#,
        r#"{"content_hash":"sha256:acbd2ea0c5608db56e1bd38bb0145a6f8363b30d8610abb746014a11f1a53c55","created_at":"2026-07-03T03:00:00.000Z","ctx_id":"acdp://registry.example.com/00000000-0000-4000-8000-000000000003","key_fingerprint":"sha256:139e3940e64b5491722088d9a0d741628fc826e09475d341a780acde3c4b8070","leaf_version":"acdp-log-leaf/1","lineage_id":"lin:sha256:1d941fb2ecdad88db6f9f3ecd5993178ab94f72e1061e685441d11ef04d92c05","origin_registry":"registry.example.com","receipt_hash":"sha256:342e57dc6d174cc7fe974c99f16c19ba598dfa31f41e560112db3f5ef21c5d91"}"#,
        r#"{"content_hash":"sha256:6f72132b15b294cea2e753efc9b7a105d6d7ebd1527adecd9f2bfc7a677a129b","created_at":"2026-07-04T04:00:00.000Z","ctx_id":"acdp://registry.example.com/00000000-0000-4000-8000-000000000004","key_fingerprint":"sha256:139e3940e64b5491722088d9a0d741628fc826e09475d341a780acde3c4b8070","leaf_version":"acdp-log-leaf/1","lineage_id":"lin:sha256:c1987e0ba3e82db332daaafd64547aa6cbb66f191d53d2023a0ff78dc6c07063","origin_registry":"registry.example.com","receipt_hash":"sha256:88ee7b664509a56dbd597ccd2f8e19c39e0aaf2c75133d0b73781ce14cf5169f"}"#,
    ];

    const LOG_001_LEAF_HASHES: [&str; 5] = [
        "sha256:95d99654d4d3de54a4d7cc04e079de61135023c78bb8192bdb79a09253afb8c1",
        "sha256:846b4d6c07ca099eea348c1e219345ddd426c0531cc30d3dd626d0fa34ec7704",
        "sha256:db94dd74b5c68f6d362129703ea587c8756d65cad0cc9859829021746a114451",
        "sha256:dc309b7856483acb5b2a92323dd9c1571a778bdb7b446587100022b49ee5fb3b",
        "sha256:6f673f8532d24869047264d89e2ad65f6ff2fa3c2674bb2fb9fa02855e090b3a",
    ];

    const LOG_001_ROOT: &str =
        "sha256:0b5978172c671ca050b44790a749b18fc29d58a7a17495fbb4e0f86eb885f731";

    const LOG_001_INCLUSION_PATH_LEAF_0: [&str; 3] = [
        "sha256:846b4d6c07ca099eea348c1e219345ddd426c0531cc30d3dd626d0fa34ec7704",
        "sha256:54d7edc4ba9d151eedd7f4bb872884f0af5ff32b39f98866d67873b00687c605",
        "sha256:6f673f8532d24869047264d89e2ad65f6ff2fa3c2674bb2fb9fa02855e090b3a",
    ];

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn log_001_fixture_leaves_reproduce_pinned_root() {
        let (store, _tmp) = log_store().await;

        for (i, leaf_json) in LOG_001_LEAVES.iter().enumerate() {
            let leaf: serde_json::Value = serde_json::from_str(leaf_json).unwrap();
            let ctx_id = leaf["ctx_id"].as_str().unwrap();
            // Stub context row to satisfy the log_leaves FK (the fixture
            // leaves did not arrive through publish).
            sqlx::query(
                "INSERT INTO contexts (ctx_id, lineage_id, agent_id, origin_registry, \
                 created_at, visibility, context_type, version, title, content_hash, body_json) \
                 VALUES (?, ?, 'did:key:fixture', 'registry.example.com', ?, 'public', \
                 'data_snapshot', 1, 'log-001 fixture', ?, '{}')",
            )
            .bind(ctx_id)
            .bind(leaf["lineage_id"].as_str().unwrap())
            .bind(leaf["created_at"].as_str().unwrap())
            .bind(leaf["content_hash"].as_str().unwrap())
            .execute(store.pool())
            .await
            .unwrap();

            // The store persists the exact canonical bytes + their hash —
            // recomputed here exactly as commit_publish computes them.
            let hash = acdp::crypto::merkle::leaf_hash(leaf_json.as_bytes());
            let hash_hex = encode_sha256_hex(&hash);
            assert_eq!(
                hash_hex, LOG_001_LEAF_HASHES[i],
                "leaf {i}: §5.1 hash over the pinned JCS bytes must match the fixture"
            );
            sqlx::query(
                "INSERT INTO log_leaves (leaf_index, ctx_id, leaf_json, leaf_hash) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(i as i64)
            .bind(ctx_id)
            .bind(leaf_json)
            .bind(&hash_hex)
            .execute(store.pool())
            .await
            .unwrap();
        }

        // Root reproduction through the store's read path.
        assert_eq!(store.log_tree_size().await.unwrap(), 5);
        let hashes = store.log_leaf_hashes(5).await.unwrap();
        let root = acdp::crypto::merkle::merkle_tree_hash(&hashes);
        assert_eq!(
            encode_sha256_hex(&root),
            LOG_001_ROOT,
            "tree-size-5 root over the stored fixture leaves must match log-001"
        );

        // Pinned inclusion path for leaf 0 at size 5.
        let path = acdp::crypto::merkle::inclusion_path(0, &hashes).unwrap();
        let path_hex: Vec<String> = path.iter().map(encode_sha256_hex).collect();
        assert_eq!(path_hex, LOG_001_INCLUSION_PATH_LEAF_0);

        // The stored rows round-trip through the typed leaf and rehash
        // identically (byte-exact reproducibility).
        for e in store.log_entries(0, 5).await.unwrap() {
            assert_eq!(
                e.leaf().unwrap().leaf_hash_hex().unwrap(),
                e.leaf_hash,
                "stored leaf bytes reproduce the stored hash"
            );
        }
    }
}

// ─── DESIGN-01: §4.5 visibility disclosure pushed into SQL ──────────────────
//
// The store now gates retrieval-style disclosure (`list_contexts`) and
// search-style disclosure (`search`) in the SQL `WHERE`, not in a post-query
// Rust `retain`. These tests are the SECURITY equivalence proof: an
// INDEPENDENT re-statement of RFC-ACDP-0008 §4.5 (not the implementation's
// former predicate) is the oracle, and the SQL result set MUST equal it
// across the full matrix {public, restricted, private} × {anonymous, owner,
// audience-reader, unauthorized-other} × public_arm_open.

mod visibility_sql {
    use super::*;
    use acdp::types::search::SearchParams;
    use std::collections::HashSet;

    const OWNER: u8 = 70;
    const READER: u8 = 71;
    const OTHER: u8 = 72;

    fn agent(seed: u8) -> AgentDid {
        AgentDid::new(format!("did:web:agents.test:contract-{seed}"))
    }

    /// Publish one context owned by `OWNER` with the given visibility and
    /// audience, tagged with `domain`/`tenant` so the test can isolate its
    /// own rows on a shared backend. Returns the assigned ctx_id.
    async fn publish(
        store: &Arc<SqliteStore>,
        title: &str,
        vis: Visibility,
        audience: &[AgentDid],
        domain: &str,
        tenant: &str,
    ) -> String {
        let p = producer(OWNER);
        let mut b = p
            .publish_request()
            .title(title)
            .context_type(ContextType::DataSnapshot)
            .domain(domain)
            .visibility(vis);
        if !audience.is_empty() {
            b = b.audience(audience.to_vec());
        }
        let req = b.build().expect("valid publish request");
        let store = Arc::clone(store);
        let tenant = tenant.to_string();
        let outcome = tokio::task::spawn_blocking(move || {
            store.commit_publish(PublishCommit {
                req: &req,
                authority: AUTHORITY,
                idempotency: None,
                tenant: Some(&tenant),
                receipt_minter: None,
                predecessor_admission: None,
            })
        })
        .await
        .unwrap()
        .expect("publish ok");
        response(&outcome).ctx_id.as_str().to_string()
    }

    // ── Independent RFC-ACDP-0008 §4.5 oracle ───────────────────────────

    /// Search/discovery disclosure (§4.5): public surfaces to any
    /// authenticated caller, or anonymously iff `anon_reads`; restricted to
    /// producer or audience; private to the PRODUCER ONLY (audience is
    /// retrieval-only, strictly narrower than retrieval for private).
    fn oracle_search(
        vis: Visibility,
        is_owner: bool,
        is_audience: bool,
        authed: bool,
        anon_reads: bool,
    ) -> bool {
        match vis {
            Visibility::Public => authed || anon_reads,
            Visibility::Restricted => authed && (is_owner || is_audience),
            Visibility::Private => authed && is_owner,
        }
    }

    /// Retrieval-style disclosure (§4.5) used by the admin/debug listing:
    /// public surfaces for any authenticated caller, or anonymously iff
    /// `anon_reads` (REG-11 Phase 2 restores this term — it mirrors
    /// `oracle_search`'s public arm); restricted/private require producer or
    /// audience membership.
    fn oracle_list(
        vis: Visibility,
        is_owner: bool,
        is_audience: bool,
        authed: bool,
        anon_reads: bool,
    ) -> bool {
        match vis {
            Visibility::Public => authed || anon_reads,
            Visibility::Restricted | Visibility::Private => authed && (is_owner || is_audience),
        }
    }

    /// Row-for-row equivalence of the SQL disclosure predicate to the §4.5
    /// oracle across the full matrix, for BOTH `search` and `list_contexts`,
    /// plus the honest `total_estimate`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn sql_disclosure_matches_rfc_4_5_across_the_matrix() {
        let (store, _tmp) = store().await;
        let dom = "design01-matrix";
        let tenant = "design01-matrix-tenant";
        let reader = agent(READER);

        // One context per visibility, owned by OWNER, audience = [READER]
        // for the audience-bearing levels (restricted requires non-empty
        // audience; private lists it too, but §4.5 makes it search-invisible).
        let contexts = [
            (
                Visibility::Public,
                publish(&store, "pub", Visibility::Public, &[], dom, tenant).await,
            ),
            (
                Visibility::Restricted,
                publish(
                    &store,
                    "restr",
                    Visibility::Restricted,
                    std::slice::from_ref(&reader),
                    dom,
                    tenant,
                )
                .await,
            ),
            (
                Visibility::Private,
                publish(
                    &store,
                    "priv",
                    Visibility::Private,
                    std::slice::from_ref(&reader),
                    dom,
                    tenant,
                )
                .await,
            ),
        ];

        let roles: [(&str, Option<AgentDid>); 4] = [
            ("anonymous", None),
            ("owner", Some(agent(OWNER))),
            ("audience-reader", Some(agent(READER))),
            ("unauthorized-other", Some(agent(OTHER))),
        ];

        for (role, requester) in &roles {
            let authed = requester.is_some();
            let is_owner = requester.as_ref() == Some(&agent(OWNER));
            let is_reader = requester.as_ref() == Some(&agent(READER));

            // ── search × public_arm_open ──
            for anon_reads in [true, false] {
                let params = SearchParams {
                    domain: Some(dom.to_string()),
                    ..Default::default()
                };
                let resp = store
                    .search(&params, requester.as_ref(), anon_reads)
                    .expect("search ok");
                let got: HashSet<&str> = resp.matches.iter().map(|m| m.ctx_id.as_str()).collect();

                let mut want: HashSet<&str> = HashSet::new();
                for (vis, id) in &contexts {
                    let is_aud = is_reader && !matches!(vis, Visibility::Public);
                    if oracle_search(vis.clone(), is_owner, is_aud, authed, anon_reads) {
                        want.insert(id.as_str());
                    }
                }
                assert_eq!(
                    got, want,
                    "SEARCH disclosure diverges from §4.5: role={role} anon_reads={anon_reads}"
                );
                // All rows are active with no tag/derived_from refinement, so
                // the §4.5-scoped pre-page total is exact here.
                assert_eq!(
                    resp.total_estimate,
                    Some(want.len() as u64),
                    "total_estimate must equal the §4.5-visible count: role={role} anon_reads={anon_reads}"
                );

                // ── list_contexts × public_arm_open (REG-11 Phase 2) ──
                let page = store
                    .list_contexts(50, None, requester.as_ref(), Some(tenant), anon_reads)
                    .await
                    .expect("list ok");
                let got: HashSet<&str> =
                    page.items.iter().map(|c| c.body.ctx_id.as_str()).collect();
                let mut want: HashSet<&str> = HashSet::new();
                for (vis, id) in &contexts {
                    let is_aud = is_reader && !matches!(vis, Visibility::Public);
                    if oracle_list(vis.clone(), is_owner, is_aud, authed, anon_reads) {
                        want.insert(id.as_str());
                    }
                }
                assert_eq!(
                    got, want,
                    "LIST disclosure diverges from §4.5: role={role} anon_reads={anon_reads}"
                );
            }
        }
    }

    /// The `LIMIT limit + 1` SENTINEL in `list_contexts`, which is what tells
    /// `try_paginate_rows` whether another page exists. Every other caller of
    /// `list_contexts` in this repository passes a limit of 50 or 100 — larger
    /// than any fixture set — so the boundary is never reached and the sentinel
    /// is never exercised. Measured in U-540: mutating `limit + 1` to
    /// `limit * 1` or `limit - 1` left the entire suite green.
    ///
    /// The repo's other pagination coverage is on `search`, a DIFFERENT method,
    /// which is why this gap survived review: `list_contexts` has ten call
    /// sites and reads as covered.
    ///
    /// This test pins the boundary itself: three rows, a limit of two.
    ///   * `limit * 1` fetches 2, so no sentinel row is seen and `next_cursor`
    ///     comes back `None` — caught by the assertion below.
    ///   * `limit - 1` fetches 1, so the first page is short — caught too.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn list_contexts_signals_more_rows_through_the_limit_sentinel() {
        let (store, _tmp) = store().await;
        let dom = "sentinel-domain";
        let tenant = "sentinel-tenant";
        let owner = agent(OWNER);

        for i in 0..3 {
            publish(
                &store,
                &format!("sentinel row {i}"),
                Visibility::Public,
                &[],
                dom,
                tenant,
            )
            .await;
        }

        // limit=2 over 3 visible rows: the page must be FULL and must announce
        // that more remain. `total_estimate` is deliberately not asserted here —
        // it is a different mechanism with its own test above, and folding it in
        // would make a sentinel failure indistinguishable from a count failure.
        let page1 = store
            .list_contexts(2, None, Some(&owner), Some(tenant), true)
            .await
            .expect("list ok");
        assert_eq!(
            page1.items.len(),
            2,
            "page must fill to the limit; a short page means the LIMIT bind is              below `limit` (e.g. `limit - 1`)"
        );
        assert!(
            page1.next_cursor.is_some(),
            "3 rows exist and the limit is 2, so the `limit + 1` sentinel must              have seen a third row and set a cursor. `None` here means the              sentinel is gone (e.g. `limit * 1`) and pagination silently ends              one page early"
        );

        // And the cursor must actually resume: the tail page carries the third
        // row and closes the walk. Without this, a cursor that is merely
        // non-None would satisfy the assertion above.
        let page2 = store
            .list_contexts(
                2,
                page1.next_cursor.as_deref(),
                Some(&owner),
                Some(tenant),
                true,
            )
            .await
            .expect("list page 2 ok");
        assert_eq!(
            page2.items.len(),
            1,
            "the tail page holds the remaining row"
        );
        assert!(
            page2.next_cursor.is_none(),
            "the walk is complete, so no further cursor"
        );

        // The two pages together are the whole visible set, with no row
        // repeated or dropped across the boundary.
        let seen: HashSet<&str> = page1
            .items
            .iter()
            .chain(page2.items.iter())
            .map(|c| c.body.ctx_id.as_str())
            .collect();
        assert_eq!(
            seen.len(),
            3,
            "the paged walk must yield all 3 rows exactly once"
        );
    }

    /// Pages fill to `limit` even when the ordered scan interleaves rows the
    /// requester may not see, and `total_estimate` is the honest count of
    /// visible rows — not the page size. Pre-DESIGN-01 the in-Rust filter
    /// dropped the private rows AFTER the page was cut, yielding short pages
    /// and a `None` total.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn pages_are_full_and_total_is_honest_under_mixed_visibility() {
        let (store, _tmp) = store().await;
        let dom = "design01-fullpage";
        let tenant = "design01-fullpage-tenant";

        // 6 public interleaved with 6 private (owned by OWNER, no audience),
        // published in alternating order so an unfiltered scan would hit the
        // private rows between the public ones.
        let mut public_ids: HashSet<String> = HashSet::new();
        for i in 0..12 {
            if i % 2 == 0 {
                let id = publish(
                    &store,
                    &format!("pub-{i}"),
                    Visibility::Public,
                    &[],
                    dom,
                    tenant,
                )
                .await;
                public_ids.insert(id);
            } else {
                publish(
                    &store,
                    &format!("priv-{i}"),
                    Visibility::Private,
                    &[],
                    dom,
                    tenant,
                )
                .await;
            }
        }

        // Anonymous caller with the public arm open: only the 6 public
        // rows are disclosable. Ask for a page of 4.
        let params = SearchParams {
            domain: Some(dom.to_string()),
            limit: Some(4),
            ..Default::default()
        };
        let page1 = store.search(&params, None, true).expect("search ok");
        assert_eq!(
            page1.matches.len(),
            4,
            "page must fill to `limit` from the visible set, not be trimmed by a post-filter"
        );
        assert_eq!(
            page1.total_estimate,
            Some(6),
            "total_estimate is the §4.5-visible count (6 public), independent of page size"
        );
        assert!(page1.next_cursor.is_some(), "more visible rows remain");

        // Every returned row is one of the public contexts (no private leak).
        for m in &page1.matches {
            assert!(
                public_ids.contains(m.ctx_id.as_str()),
                "a private row leaked into an anonymous search result"
            );
        }

        // Drain the rest and confirm exactly the 6 public rows surface.
        let mut seen: HashSet<String> = page1
            .matches
            .iter()
            .map(|m| m.ctx_id.as_str().to_string())
            .collect();
        let mut cursor = page1.next_cursor;
        while let Some(c) = cursor {
            let params = SearchParams {
                domain: Some(dom.to_string()),
                limit: Some(4),
                cursor: Some(c),
                ..Default::default()
            };
            let page = store.search(&params, None, true).expect("search ok");
            for m in &page.matches {
                seen.insert(m.ctx_id.as_str().to_string());
            }
            cursor = page.next_cursor;
        }
        assert_eq!(
            seen, public_ids,
            "pagination drains exactly the visible public set"
        );
    }
}

// ── U-550: the four survivor clusters from the full-138 mutation run ─────────
//
// U-549 ran all 138 mutants of `store.rs` under a valid harness and found 19
// survivors, 11 of which had never been judged. Those 11 are four test-shaped
// clusters, not eleven problems. Each test below names the mutants it kills and
// was falsified against every one of them by hand-application before the oracle
// was consulted -- two instruments, because U-548 is the standing argument
// against trusting one.

/// The tag filter is post-SQL (tags are stored as JSON), and nothing exercised
/// it. Three mutants lived here.
///
/// - `1585:37` deletes the `!` in `.filter(|s| !s.is_empty())`. `want` then
///   keeps only the EMPTY segments. For `tags=alpha` that leaves `want` empty,
///   `all()` over an empty iterator is `true`, the `!` makes it `false`, the
///   early return never fires and **every context passes the tag filter**.
/// - `1588:24` deletes the `!` in `if !want.iter().all(...)`, inverting the
///   test so contexts that DO carry every tag are the ones rejected.
/// - `1588:74` flips `bt == w` to `bt != w`, so "has any tag other than this
///   one" stands in for "has this one".
///
/// Asserting an EXACT set is what kills all three: each mutant changes WHICH
/// contexts come back, and two of them widen the result rather than emptying
/// it, so a `contains` assertion would pass against them.
///
/// The second query pins the trimming specifically. `"alpha, ,beta"` has an
/// empty middle segment, which the filter drops; under `1585:37` `want` becomes
/// `[""]`, no context carries an empty tag, and the result is empty instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tag_filter_admits_only_contexts_carrying_every_requested_tag() {
    let (store, _dir) = store().await;
    let p = producer(60);

    let alpha_beta = publish(&store, tagged(&p, "alpha-beta", vec!["alpha", "beta"])).await;
    let beta_only = publish(&store, tagged(&p, "beta-only", vec!["beta"])).await;
    let untagged = publish(&store, tagged(&p, "untagged", vec![])).await;

    // Precondition: without three distinct contexts the widening mutants
    // (1585:37, 1588:74) would have nothing extra to wrongly admit.
    assert_eq!(
        [&alpha_beta, &beta_only, &untagged]
            .iter()
            .map(|c| c.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3,
        "precondition: three distinct contexts"
    );

    assert_eq!(
        search_ids(
            &store,
            SearchParams {
                tags: Some("alpha".into()),
                ..Default::default()
            }
        ),
        std::collections::HashSet::from([alpha_beta.as_str().to_string()]),
        "tags=alpha must admit exactly the context carrying alpha -- not every \
         context (1585:37), not the complement (1588:24), not 'has some other \
         tag' (1588:74)"
    );

    assert_eq!(
        search_ids(
            &store,
            SearchParams {
                tags: Some("alpha, ,beta".into()),
                ..Default::default()
            }
        ),
        std::collections::HashSet::from([alpha_beta.as_str().to_string()]),
        "empty segments are trimmed away, and the remaining tags are ANDed"
    );

    assert_eq!(
        search_ids(
            &store,
            SearchParams {
                tags: Some("beta".into()),
                ..Default::default()
            }
        ),
        std::collections::HashSet::from([
            alpha_beta.as_str().to_string(),
            beta_only.as_str().to_string()
        ]),
        "tags=beta admits BOTH carriers -- pins the AND semantics as a subset \
         relation rather than equality of tag lists"
    );
}

/// `1596:86` flips `c.as_str() == df` to `!=` in the `derived_from` filter, so
/// "derives from this context" becomes "derives from something else".
///
/// The mutant EMPTIES the result rather than widening it: the only element of
/// the child's `derived_from` equals the query, so `any(|c| c != df)` is false,
/// `is_none_or` yields false, and the child is rejected. The unrelated context
/// has an empty `derived_from`, and `any()` over empty is also false, so it
/// stays out too. An assertion that merely required the child to be absent
/// would therefore pass against the mutant -- the exact-set form is load-bearing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_derived_from_filter_admits_only_direct_descendants() {
    let (store, _dir) = store().await;
    let p = producer(61);

    let parent = publish(&store, tagged(&p, "parent", vec![])).await;
    let child = publish(
        &store,
        p.publish_request()
            .title("child")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .derived_from(vec![parent.clone()])
            .build()
            .expect("valid publish request"),
    )
    .await;
    let unrelated = publish(&store, tagged(&p, "unrelated", vec![])).await;

    let got = search_ids(
        &store,
        SearchParams {
            derived_from: Some(parent.as_str().to_string()),
            ..Default::default()
        },
    );
    assert_eq!(
        got,
        std::collections::HashSet::from([child.as_str().to_string()]),
        "derived_from=<parent> admits exactly the child; got {got:?}"
    );
    assert!(
        !got.contains(unrelated.as_str()),
        "an unrelated context never derives from the parent"
    );
}

/// `1632:9` and `1644:9` both replace an idempotency-eviction body with
/// `Ok(())`. `evict_idempotency` is a thin public wrapper over
/// `idempotency_evict_inner`, so ONE test kills both: stubbing either leaves the
/// row in place.
///
/// Both directions are asserted. Evicting at a moment BEFORE the TTL must keep
/// the record -- that half passes against the mutants and is not claimed to kill
/// them; it is here so the test asserts the mechanism (`expires_at_ms <= now`)
/// rather than merely "the table got smaller". Evicting AFTER the TTL is the
/// half that reddens.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn evicting_idempotency_drops_expired_records_and_spares_live_ones() {
    let (store, _dir) = store().await;
    let p = producer(62);

    let before = Utc::now();
    commit(
        Arc::clone(&store),
        request(&p, "keyed"),
        Some("evict-key".into()),
    )
    .await
    .expect("join")
    .expect("commit ok");

    assert_eq!(
        store.count_idempotency_records().await.unwrap(),
        Some(1),
        "precondition: the keyed publish stored exactly one record"
    );

    // The record's TTL is one hour (see `commit`). Evicting now must spare it.
    store.evict_idempotency(before).await.expect("evict ok");
    assert_eq!(
        store.count_idempotency_records().await.unwrap(),
        Some(1),
        "a record whose expires_at is still in the future must survive eviction"
    );

    store
        .evict_idempotency(before + Duration::hours(2))
        .await
        .expect("evict ok");
    assert_eq!(
        store.count_idempotency_records().await.unwrap(),
        Some(0),
        "past its TTL the record must actually be DELETED -- stubbing either \
         evict_idempotency (1644:9) or idempotency_evict_inner (1632:9) to \
         Ok(()) leaves it behind"
    );
}

/// `1869:5` replaces the whole of `context_type_str` with a constant, twice:
/// `String::new()` and `"xyzzy".into()`. Every context then lands in the
/// `context_type` COLUMN under the same string.
///
/// The seam is the FILTER, not a read-back. `SearchResult.context_type` is
/// rebuilt from the stored body JSON (`store.rs:1615`), so asserting on a
/// returned context's type passes against both mutants and proves nothing.
/// Search filters on the column instead (`store.rs:1416`,
/// `AND context_type = ?`), which is the only place the mutated value is read.
/// Two types are published and each is queried, so a constant cannot satisfy
/// both: collapsing them makes each query return the wrong set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_context_type_column_keeps_each_type_distinguishable() {
    let (store, _dir) = store().await;
    let p = producer(63);

    let snapshot = publish(&store, typed(&p, "a-snapshot", ContextType::DataSnapshot)).await;
    let analysis = publish(&store, typed(&p, "an-analysis", ContextType::Analysis)).await;

    assert_eq!(
        search_ids(
            &store,
            SearchParams {
                context_type: Some("data_snapshot".into()),
                ..Default::default()
            }
        ),
        std::collections::HashSet::from([snapshot.as_str().to_string()]),
        "type=data_snapshot must select exactly the snapshot; a constant \
         context_type_str stores both rows under one string and breaks this"
    );
    assert_eq!(
        search_ids(
            &store,
            SearchParams {
                context_type: Some("analysis".into()),
                ..Default::default()
            }
        ),
        std::collections::HashSet::from([analysis.as_str().to_string()]),
        "and type=analysis must select exactly the analysis"
    );
}

/// `project_status_inline` decides whether an Active context reads as Expired,
/// and three mutants lived in its one guard, `Some(exp) if exp <= now`:
/// `1897:26` replacing the guard with `true` and with `false`, and `1897:30`
/// flipping `<=` to `>`.
///
/// Search projects status and then filters on it, defaulting to `active`, so a
/// past-deadline context must DROP OUT of a default search while a
/// future-deadline one must remain. Both directions are required and neither
/// alone suffices:
///
/// - `true` (always expired) is caught only by the future-deadline context
///   disappearing.
/// - `false` (never expired) is caught only by the past-deadline context
///   appearing.
/// - `>` inverts both, so either half catches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_projection_expires_a_context_only_once_its_deadline_has_passed() {
    let (store, _dir) = store().await;
    let p = producer(64);
    let now = Utc::now();

    let lapsed = publish(&store, expiring(&p, "lapsed", now - Duration::hours(1))).await;
    let live = publish(&store, expiring(&p, "live", now + Duration::hours(24))).await;

    let active = search_ids(&store, SearchParams::default());

    assert!(
        active.contains(live.as_str()),
        "a context whose deadline is 24h away is still ACTIVE -- the `true` \
         guard (1897:26) and the flipped `>` (1897:30) both expire it wrongly"
    );
    assert!(
        !active.contains(lapsed.as_str()),
        "a context whose deadline passed an hour ago must project to EXPIRED \
         and drop out of a default (active) search -- the `false` guard \
         (1897:26) and the flipped `>` (1897:30) both keep it active"
    );
}

/// The step-1 TTL comparison at `store.rs:994` (`if expires_at > now`) is NOT
/// redundant, and `>` → `<` and `>` → `==` are NOT equivalent mutants.
///
/// **This contradicts what the repo currently says**, so it is settled by
/// running the mutants rather than by argument. `docs/MUTATION-SCOPE-CANDIDATES.md`
/// labels all three `994:35` mutants EQUIVALENT (U-544), and the docstring on
/// `an_unexpired_idempotency_record_replays_rather_than_minting_again` above
/// explains why: with `<` applied, skipping the TTL branch lets the publish fall
/// through to step 7's `INSERT … ON CONFLICT(agent_id, key) DO NOTHING`, which
/// collides, rolls back and replays the stored response by a second route.
///
/// That probe is CORRECT — and it is correct only for a request that does not
/// supersede. The generalisation from it to "all three are outcome-equivalent"
/// is what fails, because the second enforcement lives at `store.rs:1284-1318`,
/// which is reached only AFTER step 2.
///
/// Step 2's supersession-coherence check sits at `store.rs:1118-1125`:
///
/// ```text
///   if prev_status == "superseded" {
///       return Err(AcdpError::SupersededTarget { AlreadySuperseded, … })
/// ```
///
/// and step 6 (`store.rs:1241`) marks the predecessor superseded inside the same
/// transaction. So for a keyed publish that SUPERSEDES:
///
/// * unmutated — step 1 matches the live record and returns `IdempotentReplay`
///   before step 2 ever runs;
/// * `<` or `==` — step 1 is skipped, step 2 reads v1 (now `superseded`) and the
///   call returns `Err(SupersededTarget { AlreadySuperseded })`.
///
/// `Ok(IdempotentReplay)` vs `Err(…)` is trivially observable and needs no clock
/// boundary. Why `<` and `==` make step 1 unconditionally false: step 1 first
/// runs `DELETE … WHERE expires_at_ms <= ?` bound to `now` (`store.rs:963-966`),
/// so any row that survives has `expires_at_ms >= now_ms + 1`, making
/// `expires_at > now` unconditionally TRUE for a surviving row — and its inverses
/// unconditionally false.
///
/// `>` → `>=` remains genuinely equivalent by that same arithmetic, and is still
/// carried in `MUTANTS_SURVIVORS` with that reason. This test is not claimed to
/// kill it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_keyed_superseding_publish_replays_instead_of_failing_as_already_superseded() {
    let (store, _tmp) = store().await;
    let p = producer(73);
    let key = "supersede-replay-key".to_string();

    // v1, unkeyed: the predecessor that step 6 will mark superseded.
    let v1 = commit(Arc::clone(&store), request(&p, "supersede replay v1"), None)
        .await
        .unwrap()
        .expect("v1 publish ok");
    let v1_ctx = response(&v1).ctx_id.clone();
    let v1_body = store
        .get(&v1_ctx)
        .expect("retrieve ok")
        .expect("v1 present")
        .body;

    // v2 supersedes v1 AND carries an idempotency key. This is the combination
    // the existing replay test does not exercise: its request has no
    // `supersedes`, which is exactly why these two mutants survived it.
    let v2_req = || {
        p.supersede_body(&v1_body)
            .title("supersede replay v2")
            .context_type(ContextType::DataSnapshot)
            .visibility(Visibility::Public)
            .build()
            .expect("valid superseding request")
    };

    let first = commit(Arc::clone(&store), v2_req(), Some(key.clone()))
        .await
        .unwrap()
        .expect("the first keyed superseding publish must succeed");
    assert!(
        matches!(first, PublishCommitOutcome::Inserted(_)),
        "the first publish under a fresh key must INSERT, got {first:?}"
    );
    let v2_ctx = response(&first).ctx_id.clone();

    // v1 is now superseded -- the precondition that makes step 2 fatal on replay.
    assert!(
        matches!(
            store
                .get(&v1_ctx)
                .expect("retrieve ok")
                .expect("v1 present")
                .registry_state
                .status,
            acdp::types::primitives::Status::Superseded
        ),
        "step 6 must have marked v1 superseded, or this test proves nothing about \
         step 2 and would pass for the wrong reason"
    );

    // The replay. Byte-identical request, same key, well inside the 1h TTL.
    let second = commit(Arc::clone(&store), v2_req(), Some(key.clone()))
        .await
        .unwrap();

    let second = second.unwrap_or_else(|e| {
        panic!(
            "the replay must return Ok(IdempotentReplay), got Err({e:?}). \
             SupersededTarget{{ AlreadySuperseded }} here means step 1's TTL branch \
             was SKIPPED and control reached step 2's supersession-coherence check \
             at store.rs:1118 -- which is exactly what `expires_at < now` and \
             `expires_at == now` do at store.rs:994. Step 7's ON CONFLICT fallback \
             cannot rescue this case because it lives after step 2."
        )
    });
    assert!(
        matches!(second, PublishCommitOutcome::IdempotentReplay(_)),
        "an unexpired record must REPLAY, got {second:?}"
    );
    assert_eq!(
        response(&second).ctx_id,
        v2_ctx,
        "the replay must return the ORIGINAL v2 context, not a newly minted one"
    );

    // And no third context was minted into the lineage.
    assert_eq!(
        store.count_idempotency_records().await.expect("count ok"),
        Some(1),
        "one key, one retained record"
    );
}

// ---------------------------------------------------------------------------
// The four `RegistryStore` methods with NO ORIGINATING PRODUCTION CALLER.
//
// `568:9 put`, `821:9 mark_superseded`, `832:9 first_version_ctx_id` and
// `923:9 idempotency_evict_expired` survived every prior run, and U-546
// correctly established WHY: each has exactly three call sites, all delegating
// wrapper impls that forward to another implementation
// (`acdp-registry-store/src/parity.rs`, `acdp-registry-server/src/memory_ext.rs`,
// `acdp-registry-server/tests/http_integration.rs`). No originating caller
// exists, and the UFCS form returns nothing either. The twelve-odd `.put(` hits
// in `acdp-registry-auth` are `ChallengeStore::put(ChallengeRecord)`, a
// different trait — the census has to be resolved by TYPE, not by name.
//
// U-552 first proposed carrying them as budgeted survivors under a new label,
// on the argument that a test whose only purpose is to call an uncalled method
// converts a true finding into a permanently green line. **That was reversed,
// and the reason is worth recording rather than quietly dropping.**
//
//   1. `.cargo/mutants.toml`'s governing rule is "PAY FIRST, WIDEN LAST": a
//      survivor a test CAN kill gets the test.
//   2. The repo had already made the opposite call for the identical shape.
//      `store.rs:380:9 lifecycle_events_of_ctx -> Ok(vec![])` is KILLED (U-543)
//      by a direct-call test — and that method has no originating production
//      caller either (three impls, one delegating wrapper, plus U-543's own two
//      calls). Carrying these four while counting that one a win would leave the
//      documentation asserting both positions at once.
//   3. These are backend CONTRACT surface, not dead code:
//      `acdp-registry-pg/src/store.rs` implements all four, and `parity.rs`
//      exists to compare backends. Pinning sqlite's side is real contract
//      coverage, not an invented caller.
//
// The finding is NOT lost by killing them: it stays in
// `docs/MUTATION-SCOPE-CANDIDATES.md`'s U-546 section and in this comment. What
// changes is that the behaviour is now also pinned, so a future originating
// caller inherits a tested contract instead of an untested one.
// ---------------------------------------------------------------------------

/// `568:9` replaces `<SqliteStore as RegistryStore>::put` with `Ok(())`.
///
/// `put` inserts a body as an ACTIVE context (`store.rs:567-578`), so stubbing it
/// reports success while storing nothing. Asserted through `get`, which reads the
/// real row back: the mutant leaves `get` returning `None`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_store_put_actually_stores_a_retrievable_context() {
    let (source, _src_dir) = store().await;
    let p = producer(81);

    // Publish through the real commit path, then lift the stored body out.
    let ctx_id = publish(&source, request(&p, "put round-trip")).await;
    let body = source
        .get(&ctx_id)
        .expect("retrieve ok")
        .expect("published context present")
        .body;

    // A SEPARATE store, so this exercises `put` rather than the commit path that
    // already populated `source`.
    let (target, _tgt_dir) = store().await;
    assert!(
        target.get(&ctx_id).expect("retrieve ok").is_none(),
        "precondition: the target store must not already hold this context, or \
         the assertion below would pass without `put` doing anything"
    );

    target.put(body).expect("put must succeed");

    let got = target
        .get(&ctx_id)
        .expect("retrieve ok")
        .expect("put must make the context retrievable; `put -> Ok(())` stores nothing");
    assert_eq!(
        got.body.ctx_id, ctx_id,
        "put must store the body it was given, under its own ctx_id"
    );
}

/// `821:9` replaces `mark_superseded` with `Ok(())`.
///
/// The body is `UPDATE contexts SET status = 'superseded' WHERE ctx_id = ?`
/// (`store.rs:820-828`), so the mutant reports success and leaves the row ACTIVE.
/// Both directions are asserted: the status must be Active BEFORE the call, or a
/// context that was already superseded would satisfy the assertion after it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_store_mark_superseded_actually_changes_the_status() {
    let (store, _dir) = store().await;
    let p = producer(82);
    let ctx_id = publish(&store, request(&p, "to be superseded")).await;

    assert!(
        matches!(
            store
                .get(&ctx_id)
                .expect("retrieve ok")
                .expect("present")
                .registry_state
                .status,
            acdp::types::primitives::Status::Active
        ),
        "precondition: a freshly published context is Active, or the post-call \
         assertion proves nothing"
    );

    store.mark_superseded(&ctx_id).expect("mark_superseded ok");

    assert!(
        matches!(
            store
                .get(&ctx_id)
                .expect("retrieve ok")
                .expect("present")
                .registry_state
                .status,
            acdp::types::primitives::Status::Superseded
        ),
        "mark_superseded must actually write status='superseded'; the `Ok(())` \
         mutant reports success and leaves the row Active"
    );
}

/// `832:9` replaces `first_version_ctx_id` with `Ok(None)`.
///
/// The body is `ORDER BY version ASC, created_at ASC LIMIT 1` over the lineage
/// (`store.rs:831-845`), so the mutant claims every lineage has no first version.
/// The lineage deliberately holds TWO versions: with one, `Ok(None)` would still
/// be wrong but the test would not distinguish "returns the first" from "returns
/// whatever it found".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_store_first_version_ctx_id_returns_the_lineage_head() {
    let (store, _dir) = store().await;
    let p = producer(83);

    let v1_ctx = publish(&store, request(&p, "lineage head v1")).await;
    let v1_body = store
        .get(&v1_ctx)
        .expect("retrieve ok")
        .expect("v1 present")
        .body;
    let lineage = v1_body.lineage_id.clone();

    let v2_req = p
        .supersede_body(&v1_body)
        .title("lineage head v2")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("valid superseding request");
    let v2_ctx = publish(&store, v2_req).await;
    assert_ne!(v1_ctx, v2_ctx, "precondition: two distinct versions exist");

    let first = store
        .first_version_ctx_id(&lineage)
        .expect("first_version_ctx_id ok")
        .expect("a populated lineage HAS a first version; `Ok(None)` claims it does not");
    assert_eq!(
        first, v1_ctx,
        "the lineage head is v1, not the newest version -- ORDER BY version ASC"
    );
}

/// `923:9` replaces the `RegistryStore` trait method
/// `idempotency_evict_expired` with `Ok(())`.
///
/// It is a one-line delegation to `idempotency_evict_inner` (`store.rs:922-924`).
/// `evicting_idempotency_drops_expired_records_and_spares_live_ones` above
/// already asserts exactly this behaviour in both directions — but through the
/// INHERENT `evict_idempotency`, so the trait method's own body was never
/// executed and its mutant survived. This is the same assertion reached through
/// the trait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_store_idempotency_evict_expired_actually_deletes_through_the_trait() {
    let (store, _dir) = store().await;
    let p = producer(84);

    let before = Utc::now();
    commit(
        Arc::clone(&store),
        request(&p, "trait-evict keyed"),
        Some("trait-evict-key".into()),
    )
    .await
    .expect("join")
    .expect("commit ok");
    assert_eq!(
        store.count_idempotency_records().await.unwrap(),
        Some(1),
        "precondition: the keyed publish stored exactly one record"
    );

    // Before the TTL: the record must survive. This half passes against the
    // mutant and is here so the test asserts the mechanism rather than merely
    // "the table got smaller".
    RegistryStore::idempotency_evict_expired(&*store, before).expect("evict ok");
    assert_eq!(
        store.count_idempotency_records().await.unwrap(),
        Some(1),
        "a record still inside its TTL must survive"
    );

    // Past the TTL: this is the half that reddens under `-> Ok(())`.
    RegistryStore::idempotency_evict_expired(&*store, before + Duration::hours(2))
        .expect("evict ok");
    assert_eq!(
        store.count_idempotency_records().await.unwrap(),
        Some(0),
        "past its TTL the record must actually be DELETED; the trait method \
         stubbed to `Ok(())` reports success and leaves it behind"
    );
}
