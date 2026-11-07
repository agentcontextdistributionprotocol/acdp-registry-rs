//! End to end against the SHIPPED BINARY on a real Postgres.
//!
//! `pg_integration.rs` drives an in-process router over `PgStore`, and every
//! other HTTP suite in this directory compiles for SQLite only. So nothing
//! booted `run()`'s Postgres arm (`main.rs`), and with it `PgChallengeStore`,
//! and nothing read the Postgres SQL for several security-relevant paths over
//! HTTP. Each test here spawns `CARGO_BIN_EXE_acdp-registry` with
//! `storage.backend = "postgres"` and reads it the way a client does:
//!
//! - the RFC-ACDP-0008 §3 challenge/token handshake, and that a redeemed
//!   nonce cannot be redeemed again (`PgChallengeStore`);
//! - `bearer_static` read tokens (§6.2) against `private` and `restricted`
//!   rows: retrieval and search disclosure, which Postgres decides in SQL;
//! - strict multi-tenancy: `set_tenant_of_ctx`, `search_in_tenant` and
//!   `tenants_of_ctxs` on Postgres, which no test exercised at all;
//! - the RFC-ACDP-0018 change feed's §6.1 bucket filters, a key revocation
//!   among the entries, checked against the SDK's own filter as the oracle;
//! - transparency-log inclusion and consistency proofs built from Postgres
//!   leaves, verified against the checkpoint signature;
//! - the `auth.did_methods` gate (dk-003) on the binary as configured.
//!
//! # Gating
//!
//! Compiled only with `--features storage-pg`. No CI step selects this file:
//! the tests job and the coverage job run `--test pg_integration` alone under
//! that feature, so plain `cargo test` and CI are unchanged. It skips (passes
//! with a message) unless `ACDP_REGISTRY_TEST_PG_URL` is set; `ACDP_REQUIRE_PG`
//! turns the skip into a failure, as in `pg_integration.rs`.
//!
//! Each test TRUNCATES the registry tables first, so point it at a disposable
//! database only. `docker/docker-compose.yml` with a port published for
//! `postgres` is the intended target:
//!
//! ```bash
//! ACDP_REGISTRY_TEST_PG_URL=postgres://acdp:acdp@127.0.0.1:55432/acdp_registry \
//! ACDP_REQUIRE_PG=1 \
//!   cargo test -p acdp-registry-server --no-default-features --features storage-pg \
//!   --test pg_e2e
//! ```

#![cfg(feature = "storage-pg")]

mod spawned_registry;

use acdp::crypto::SigningKey;
use acdp::producer::Producer;
use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
use base64::Engine as _;
use serde_json::{json, Value};
use serial_test::serial;
use spawned_registry::SpawnedRegistry;

fn require_pg() -> bool {
    std::env::var("ACDP_REQUIRE_PG").is_ok()
}

/// `ACDP_REGISTRY_TEST_PG_URL`, or `None` (skip). Under `ACDP_REQUIRE_PG` the
/// skip is a failure: an early return from a test is a pass, and a suite that
/// asserted nothing must not read as one that ran.
fn pg_url_or_skip() -> Option<String> {
    match std::env::var("ACDP_REGISTRY_TEST_PG_URL") {
        Ok(u) => Some(u),
        Err(_) => {
            assert!(
                !require_pg(),
                "ACDP_REQUIRE_PG is set but ACDP_REGISTRY_TEST_PG_URL is not"
            );
            eprintln!("SKIP pg_e2e: ACDP_REGISTRY_TEST_PG_URL not set");
            None
        }
    }
}

/// Empty every table a test writes, so feeds, log sizes and search results
/// start from zero. `IF EXISTS` covers a database the binary has not yet
/// migrated; the binary migrates on start.
fn truncate(url: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let pool = sqlx::PgPool::connect(url).await.expect("connect to postgres");
        sqlx::query(
            "DO $$
             DECLARE t text;
             BEGIN
               FOREACH t IN ARRAY ARRAY['contexts','idempotency_records','auth_challenges',
                 'issued_tokens','lifecycle_events','log_leaves','log_witness_cosignatures',
                 'key_attestations','subscriptions','changes'] LOOP
                 IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = t) THEN
                   EXECUTE format('TRUNCATE TABLE %I RESTART IDENTITY CASCADE', t);
                 END IF;
               END LOOP;
               IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'changes_meta') THEN
                 UPDATE changes_meta SET pruned_through = 0;
               END IF;
             END $$;",
        )
        .execute(&pool)
        .await
        .expect("truncate");
        pool.close().await;
    });
}

fn b64_32(byte: u8) -> String {
    base64::engine::general_purpose::STANDARD.encode([byte; 32])
}

fn storage(url: &str) -> String {
    format!("backend = \"postgres\"\npostgres_url = \"{url}\"\nmax_connections = 5\n")
}

/// Truncate, then spawn the binary on Postgres with `extra` as the rest of
/// its config (every table except `[registry]` and `[storage]`).
fn start(dir: &std::path::Path, tag: &str, url: &str, extra: &str) -> SpawnedRegistry {
    truncate(url);
    SpawnedRegistry::start_with_storage(dir, tag, "", &storage(url), extra)
}

/// `[auth]` with auth on and did:key accepted, followed by `more` (whole
/// lines inside the same table, or further `[[auth.*]]` arrays).
fn auth_on(more: &str) -> String {
    format!(
        "[auth]\nenabled = true\njwt_secret = \"{}\"\n\
         did_methods = [\"did:web\", \"did:key\"]\nanonymous_public_reads = true\n{more}\n",
        b64_32(0x5a)
    )
}

/// Receipts, lifecycle, the transparency log and the change feed with §6.1
/// bucket filters up to 8 digits. Same keys as `conformance.rs`'s
/// `served_full_features_toml`, so the receipt key is seed `0x2c`.
fn full_features() -> String {
    format!(
        "[receipt]\nsigning_key_seed_b64 = \"{}\"\nhead_receipts = true\n\n\
         [lifecycle]\nenabled = true\n\n[log]\nenabled = true\n\n\
         [changes]\nenabled = true\nkey_b64 = \"{}\"\nmax_change_bucket_length = 8\n",
        b64_32(0x2c),
        b64_32(0x2d)
    )
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn did_of(seed: u8) -> String {
    acdp::did::key::did_key_from_ed25519(&key(seed).verifying_key_bytes())
}

fn producer(seed: u8) -> Producer {
    Producer::new_did_key(key(seed))
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// GET with an optional bearer.
fn get_as(reg: &SpawnedRegistry, path: &str, token: Option<&str>) -> (u16, Value) {
    match token {
        None => reg.get(path),
        Some(t) => {
            let auth = bearer(t);
            let (s, _, v) = reg.get_with_headers(path, &[("Authorization", &auth)]);
            (s, v)
        }
    }
}

/// The signed `/auth/token` request for `seed`'s did:key, from a fresh
/// challenge. Returned unsent so a test can replay it byte for byte.
fn signed_token_request(reg: &SpawnedRegistry, seed: u8) -> Value {
    let did = did_of(seed);
    let (status, challenge) = reg.post("/auth/challenge", &json!({ "agent_id": did }));
    assert_eq!(status, 200, "challenge: {challenge}\n{}", reg.output());
    json!({
        "nonce": challenge["nonce"],
        "agent_id": did,
        "expires_at": challenge["expires_at"],
        "algorithm": "ed25519",
        "key_id": acdp::did::key::did_key_url(&did).expect("did:key url"),
        "signature": key(seed).sign_string(challenge["signing_input"].as_str().expect("signing_input")),
    })
}

/// A session token for `seed`'s did:key through the full handshake.
fn login(reg: &SpawnedRegistry, seed: u8) -> String {
    let req = signed_token_request(reg, seed);
    let (status, v) = reg.post("/auth/token", &req);
    assert_eq!(status, 200, "token: {v}\n{}", reg.output());
    v["token"].as_str().expect("token").to_string()
}

/// Publish `req` (a publish request or a body), with a session token when
/// given. Returns the status and response.
fn post_context(
    reg: &SpawnedRegistry,
    req: &impl serde::Serialize,
    token: Option<&str>,
) -> (u16, Value) {
    let body = serde_json::to_value(req).expect("serialize the publish request");
    match token {
        None => reg.post("/contexts", &body),
        Some(t) => reg.post_with_headers("/contexts", &[("Authorization", &bearer(t))], &body),
    }
}

/// Publish and expect 201; returns `(ctx_id, response)`.
fn publish(
    reg: &SpawnedRegistry,
    seed: u8,
    title: &str,
    visibility: Visibility,
    audience: &[u8],
    token: Option<&str>,
) -> (String, Value) {
    let p = producer(seed);
    let mut b = p
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(visibility);
    if !audience.is_empty() {
        b = b.audience(audience.iter().map(|s| AgentDid::new(did_of(*s))).collect());
    }
    let req = b.build().expect("publish request");
    let (status, v) = post_context(reg, &req, token);
    assert_eq!(status, 201, "publish {title}: {v}\n{}", reg.output());
    (v["ctx_id"].as_str().expect("ctx_id").to_string(), v)
}

/// The `ctx_id`s a search returns.
fn search_ids(reg: &SpawnedRegistry, q: &str, token: Option<&str>) -> Vec<String> {
    let (status, v) = get_as(
        reg,
        &format!("/contexts/search?q={}&limit=50", enc(q)),
        token,
    );
    assert_eq!(status, 200, "search {q}: {v}");
    v["matches"]
        .as_array()
        .unwrap_or_else(|| panic!("matches: {v}"))
        .iter()
        .map(|m| m["ctx_id"].as_str().expect("ctx_id").to_string())
        .collect()
}

/// A `[[auth.read_tokens]]` entry for `seed`'s DID, and its token.
fn read_token(id: &str, seed: u8, tenant: Option<&str>) -> (String, String) {
    let (token, sha256) = acdp_registry_auth::generate_read_token();
    let tenant = tenant
        .map(|t| format!("tenant = \"{t}\"\n"))
        .unwrap_or_default();
    let entry = format!(
        "[[auth.read_tokens]]\nid = \"{id}\"\ndid = \"{}\"\nsha256 = \"{sha256}\"\n\
         operator_administered = true\n{tenant}\n",
        did_of(seed)
    );
    (entry, token)
}

// ─── RFC-ACDP-0008 §3: the handshake on PgChallengeStore ────────────────────

/// The full challenge → signed response → token handshake on the Postgres
/// binary, and the replay guard: the same signed response, sent again, is
/// refused because the nonce was consumed. `PgChallengeStore::take` is what
/// makes a nonce single-use across replicas; nothing tested it before.
#[test]
#[serial]
fn pg_binary_handshake_tokens_are_single_use_and_publish() {
    let Some(url) = pg_url_or_skip() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    let reg = start(dir.path(), "handshake", &url, &auth_on(""));

    let req = signed_token_request(&reg, 0x11);
    let (status, v) = reg.post("/auth/token", &req);
    assert_eq!(status, 200, "first redemption: {v}\n{}", reg.output());
    let token = v["token"].as_str().expect("token").to_string();

    let (status, v) = reg.post("/auth/token", &req);
    assert_eq!(
        status, 403,
        "a replayed challenge response must be refused: {v}"
    );
    assert_eq!(v["error"]["code"], "not_authorized", "{v}");

    // The token is a working credential on this binary.
    let (ctx_id, _) = publish(
        &reg,
        0x11,
        "pg handshake private",
        Visibility::Private,
        &[],
        Some(&token),
    );
    let (status, v) = get_as(&reg, &format!("/contexts/{}", enc(&ctx_id)), Some(&token));
    assert_eq!(status, 200, "the owner reads its private context: {v}");
    let (status, _) = get_as(&reg, &format!("/contexts/{}", enc(&ctx_id)), None);
    assert_eq!(status, 404, "anonymous never sees a private context");
}

// ─── RFC-ACDP-0008 §4.5 / §6.2: disclosure through bearer_static ────────────

/// `private` and `restricted` rows over `bearer_static` read tokens: the
/// owner sees both, the audience member sees the restricted one only, a
/// stranger and an anonymous reader see neither, by GET or by search. On
/// Postgres the search half is SQL (`search` with a requester), which no
/// HTTP test read before.
#[test]
#[serial]
fn pg_binary_read_tokens_gate_private_and_restricted_rows() {
    const OWNER: u8 = 0x21;
    const MEMBER: u8 = 0x22;
    const STRANGER: u8 = 0x23;
    let Some(url) = pg_url_or_skip() else { return };
    let (e_owner, t_owner) = read_token("owner", OWNER, None);
    let (e_member, t_member) = read_token("member", MEMBER, None);
    let (e_stranger, t_stranger) = read_token("stranger", STRANGER, None);
    let dir = tempfile::tempdir().expect("tempdir");
    let reg = start(
        dir.path(),
        "read-tokens",
        &url,
        &auth_on(&format!("{e_owner}{e_member}{e_stranger}")),
    );
    let session = login(&reg, OWNER);
    let (public, _) = publish(
        &reg,
        OWNER,
        "pgdisc public",
        Visibility::Public,
        &[],
        Some(&session),
    );
    let (private, _) = publish(
        &reg,
        OWNER,
        "pgdisc private",
        Visibility::Private,
        &[],
        Some(&session),
    );
    let (restricted, _) = publish(
        &reg,
        OWNER,
        "pgdisc restricted",
        Visibility::Restricted,
        &[MEMBER],
        Some(&session),
    );

    let cases: [(&str, Option<&str>, &[&String]); 4] = [
        ("owner", Some(&t_owner), &[&public, &private, &restricted]),
        ("member", Some(&t_member), &[&public, &restricted]),
        ("stranger", Some(&t_stranger), &[&public]),
        ("anonymous", None, &[&public]),
    ];
    for (who, token, visible) in cases {
        for ctx in [&public, &private, &restricted] {
            let (status, v) = get_as(&reg, &format!("/contexts/{}", enc(ctx)), token);
            let want = if visible.contains(&ctx) { 200 } else { 404 };
            assert_eq!(status, want, "{who} GET {ctx}: {v}");
        }
        let mut got = search_ids(&reg, "pgdisc", token);
        got.sort();
        let mut want: Vec<String> = visible.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(got, want, "{who}: search discloses exactly what GET does");
    }

    // RFC-ACDP-0008 §6.2: a token that fails validation is refused with
    // 403 `not_authorized`, decided before the lookup (identical for a row
    // that exists and one that does not), never downgraded to anonymous.
    let unknown = "acdp_rt_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let mut refusals = Vec::new();
    for path in [
        format!("/contexts/{}", enc(&public)),
        format!("/contexts/{}", enc("acdp://localhost/no-such-context")),
        "/contexts/search?q=pgdisc".to_string(),
    ] {
        let (status, v) = get_as(&reg, &path, Some(unknown));
        assert_eq!(status, 403, "an unknown read token on {path}: {v}");
        assert_eq!(v["error"]["code"], "not_authorized", "{path}: {v}");
        refusals.push(v);
    }
    assert_eq!(
        refusals[0], refusals[1],
        "existence must not change the refusal"
    );
}

// ─── strict multi-tenancy on Postgres ───────────────────────────────────────

/// `require_tenant = true` with two bound producers. Each tenant's reader
/// finds its own tenant's row and not the other's, by search, by GET and by
/// `GET /lineages/{id}/current`; the rows are public, so only the tenant
/// binding (`set_tenant_of_ctx`, `search_in_tenant`, `tenants_of_ctxs` on
/// Postgres) keeps them apart.
#[test]
#[serial]
fn pg_binary_tenants_are_isolated() {
    const ACME: u8 = 0x31;
    const GLOBEX: u8 = 0x32;
    const ACME_READER: u8 = 0x33;
    const GLOBEX_READER: u8 = 0x34;
    let Some(url) = pg_url_or_skip() else { return };
    let (e_a, t_acme) = read_token("acme-reader", ACME_READER, Some("acme"));
    let (e_g, t_globex) = read_token("globex-reader", GLOBEX_READER, Some("globex"));
    let bindings = format!(
        "require_tenant = true\n\n\
         [[auth.tenant_agents]]\nagent_did = \"{}\"\ntenant_id = \"acme\"\n\n\
         [[auth.tenant_agents]]\nagent_did = \"{}\"\ntenant_id = \"globex\"\n\n{e_a}{e_g}",
        did_of(ACME),
        did_of(GLOBEX)
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let reg = start(dir.path(), "tenants", &url, &auth_on(&bindings));

    let (acme_ctx, acme_v) = publish(
        &reg,
        ACME,
        "pgtenant acme",
        Visibility::Public,
        &[],
        Some(&login(&reg, ACME)),
    );
    let (globex_ctx, globex_v) = publish(
        &reg,
        GLOBEX,
        "pgtenant globex",
        Visibility::Public,
        &[],
        Some(&login(&reg, GLOBEX)),
    );
    let lineage = |v: &Value| v["lineage_id"].as_str().map(str::to_string);

    for (who, token, mine, theirs, theirs_v) in [
        ("acme", &t_acme, &acme_ctx, &globex_ctx, &globex_v),
        ("globex", &t_globex, &globex_ctx, &acme_ctx, &acme_v),
    ] {
        assert_eq!(
            search_ids(&reg, "pgtenant", Some(token)),
            vec![mine.clone()],
            "{who} searches its own tenant only"
        );
        let (status, v) = get_as(&reg, &format!("/contexts/{}", enc(mine)), Some(token));
        assert_eq!(status, 200, "{who} reads its own row: {v}");
        let (status, v) = get_as(&reg, &format!("/contexts/{}", enc(theirs)), Some(token));
        assert_eq!(
            status, 404,
            "{who} must not read the other tenant's row: {v}"
        );
        if let Some(l) = lineage(theirs_v) {
            let (status, v) = get_as(&reg, &format!("/lineages/{}/current", enc(&l)), Some(token));
            assert_eq!(status, 404, "{who}: foreign lineage head: {v}");
        }
    }

    // Strict mode: a request that resolves to no tenant is not served data.
    let (status, v) = reg.get("/contexts/search?q=pgtenant");
    assert_ne!(status, 200, "an untenanted read under require_tenant: {v}");
}

// ─── RFC-ACDP-0018 §6.1: bucket filters over Postgres ───────────────────────

/// Drain `/changes` with `filters`, `limit` entries a page.
fn drain(reg: &SpawnedRegistry, filters: &str, limit: u64) -> Vec<Value> {
    let mut out = Vec::new();
    let mut since: Option<String> = None;
    for _ in 0..200 {
        let mut q = format!("/changes?limit={limit}");
        if let Some(c) = &since {
            q.push_str(&format!("&since={}", enc(c)));
        }
        if !filters.is_empty() {
            q.push('&');
            q.push_str(filters);
        }
        let (status, v) = reg.get(&q);
        assert_eq!(status, 200, "{q}: {v}");
        out.extend(v["changes"].as_array().expect("changes").iter().cloned());
        since = Some(v["next_cursor"].as_str().expect("next_cursor").to_string());
        if !v["has_more"].as_bool().expect("has_more") {
            return out;
        }
    }
    panic!("the feed never reached has_more=false");
}

fn seqs(entries: &[Value]) -> Vec<u64> {
    entries
        .iter()
        .map(|e| e["seq"].as_u64().expect("seq"))
        .collect()
}

/// A signed lifecycle event retracting `ctx_id`, sent and expected to land.
fn retract(reg: &SpawnedRegistry, seed: u8, ctx_id: &str) {
    use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
    let did = did_of(seed);
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        CtxId(ctx_id.to_string()),
        LifecycleEventType::Retracted,
        chrono::Utc::now(),
        AgentDid::new(did.clone()),
        None,
    )
    .expect("valid event")
    .sign_with(
        key(seed),
        acdp::did::key::did_key_url(&did).expect("key url"),
    )
    .expect("signed event");
    let (status, v) = reg.post(
        &format!("/contexts/{}/retract", enc(ctx_id)),
        &json!({ "event": event }),
    );
    assert_eq!(status, 200, "retract {ctx_id}: {v}\n{}", reg.output());
}

/// rec-010 against Postgres: publishes, a supersession, a retraction and a
/// key revocation from several producers, then every bucket query must
/// return exactly the entries the SDK's own §6.1 rule selects from the full
/// feed, whole and one entry a page. The revocation sits in its producer's
/// agent bucket and in no lineage bucket.
#[test]
#[serial]
fn pg_binary_change_feed_bucket_filters_match_the_sdk() {
    use acdp::types::change::{bucket, Change, WatchFilter};
    const PRODUCERS: [u8; 4] = [0x41, 0x42, 0x43, 0x44];
    const REVOKED: u8 = 0x4f;
    let Some(url) = pg_url_or_skip() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    let extra = format!(
        "[auth]\nenabled = false\ndid_methods = [\"did:web\", \"did:key\"]\n\
         anonymous_public_reads = true\n\n{}",
        full_features()
    );
    let reg = start(dir.path(), "buckets", &url, &extra);

    let mut lineages = Vec::new();
    for (i, seed) in PRODUCERS.iter().enumerate() {
        let (ctx, v) = publish(
            &reg,
            *seed,
            &format!("pgbucket {i}"),
            Visibility::Public,
            &[],
            None,
        );
        lineages.push(v["lineage_id"].as_str().expect("lineage_id").to_string());
        match i {
            0 => retract(&reg, *seed, &ctx),
            1 => {
                let (_, served) = reg.get(&format!("/contexts/{}", enc(&ctx)));
                let prev: acdp::types::body::Body =
                    serde_json::from_value(served["body"].clone()).expect("served body");
                let next = producer(*seed)
                    .supersede_body(&prev)
                    .title("pgbucket 1 v2")
                    .context_type(ContextType::DataSnapshot)
                    .visibility(Visibility::Public)
                    .build()
                    .expect("supersede");
                let (status, v) = post_context(&reg, &next, None);
                assert_eq!(status, 201, "supersede: {v}");
            }
            _ => {}
        }
    }
    let revocation = producer(PRODUCERS[0])
        .publish_request()
        .title("pgbucket revocation")
        .context_type(ContextType::KeyRevocation)
        .visibility(Visibility::Public)
        .metadata(json!({
            "revoked_key_fingerprint": acdp::crypto::fingerprint::fingerprint_ed25519(
                &key(REVOKED).verifying_key_bytes()
            ),
            "compromised_since": "2026-10-01T00:00:00.000Z",
        }))
        .build()
        .expect("revocation");
    let (status, v) = post_context(&reg, &revocation, None);
    assert_eq!(status, 201, "key revocation: {v}\n{}", reg.output());
    let revocation_ctx = v["ctx_id"].as_str().expect("ctx_id").to_string();

    let full: Vec<Change> = drain(&reg, "", 100)
        .iter()
        .map(|v| Change::from_value(v).expect("closed-schema change"))
        .collect();
    assert!(
        full.len() >= 3,
        "lifecycle, superseded and revocation entries: {full:?}"
    );
    let revocation_seq = full
        .iter()
        .find(|c| c.ctx_id().as_str() == revocation_ctx)
        .map(Change::seq)
        .expect("the key revocation is in the feed");

    let mut filters = Vec::new();
    for n in [1usize, 4, 8] {
        filters.push(WatchFilter::new().lineage_bucket(bucket(&lineages[0], n)));
        filters.push(WatchFilter::new().lineage_bucket(bucket(&lineages[1], n)));
        filters.push(WatchFilter::new().agent_bucket(bucket(&did_of(PRODUCERS[0]), n)));
        filters.push(
            WatchFilter::new()
                .lineage_bucket(bucket(&lineages[1], n))
                .agent_bucket(bucket(&did_of(PRODUCERS[0]), n)),
        );
    }
    for filter in &filters {
        let q = filter
            .query_pairs()
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let want: Vec<u64> = full
            .iter()
            .filter(|c| filter.matches(c, |_| None))
            .map(Change::seq)
            .collect();
        assert_eq!(seqs(&drain(&reg, &q, 100)), want, "?{q}");
        assert_eq!(seqs(&drain(&reg, &q, 1)), want, "?{q} one a page");
    }

    let by_agent = seqs(&drain(
        &reg,
        &format!("agent_bucket={}", bucket(&did_of(PRODUCERS[0]), 8)),
        100,
    ));
    assert!(by_agent.contains(&revocation_seq), "{by_agent:?}");
    let by_lineage = seqs(&drain(
        &reg,
        &format!("lineage_bucket={}", bucket(&lineages[0], 8)),
        100,
    ));
    assert!(
        !by_lineage.is_empty() && !by_lineage.contains(&revocation_seq),
        "{by_lineage:?}"
    );
}

// ─── RFC-ACDP-0015: log proofs from Postgres leaves ─────────────────────────

/// Every published context has an inclusion proof that folds to the
/// checkpoint root, and the size-3 tree is a prefix of the size-5 tree,
/// both under the receipt key's checkpoint signature. Postgres supplies the
/// leaves and their order; only store-level tests read them before.
#[test]
#[serial]
fn pg_binary_log_proofs_verify() {
    use acdp::types::log::{LogCheckpoint, LogConsistencyProof, LogInclusion};
    let Some(url) = pg_url_or_skip() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    let extra = format!(
        "[auth]\nenabled = false\ndid_methods = [\"did:web\", \"did:key\"]\n\
         anonymous_public_reads = true\n\n{}",
        full_features()
    );
    let reg = start(dir.path(), "log", &url, &extra);
    let receipt_key = key(0x2c).verifying_key_bytes();

    let mut ctxs = Vec::new();
    for i in 0..3u8 {
        ctxs.push(
            publish(
                &reg,
                0x51 + i,
                &format!("pglog {i}"),
                Visibility::Public,
                &[],
                None,
            )
            .0,
        );
    }
    let (status, v3) = reg.get("/log/checkpoint");
    assert_eq!(status, 200, "checkpoint: {v3}");
    let retained = LogCheckpoint::from_value(&v3).expect("closed-schema checkpoint");
    assert_eq!(retained.tree_size, 3);
    for i in 3..5u8 {
        ctxs.push(
            publish(
                &reg,
                0x51 + i,
                &format!("pglog {i}"),
                Visibility::Public,
                &[],
                None,
            )
            .0,
        );
    }

    for (i, ctx) in ctxs.iter().enumerate() {
        let (status, v) = reg.get(&format!("/log/proof?ctx_id={}", enc(ctx)));
        assert_eq!(status, 200, "inclusion for {ctx}: {v}");
        let inclusion = LogInclusion::from_value(&v).expect("closed-schema inclusion");
        assert_eq!(inclusion.leaf_index, i as u64, "acceptance order (§5.3)");
        assert_eq!(inclusion.tree_size, 5);
        let leaf = inclusion
            .leaf
            .as_ref()
            .expect("leaf echoed for a public context");
        inclusion
            .verify_reconstructed_leaf(leaf)
            .unwrap_or_else(|e| panic!("leaf {i} does not fold to the root: {e:?}"));
        let hash = LogCheckpoint::preimage_hash_of_value(&v["log_checkpoint"]).expect("preimage");
        inclusion
            .log_checkpoint
            .verify_signature_against_hash(&hash, Some(&receipt_key), None)
            .expect("checkpoint signature");
    }

    let (status, v) = reg.get("/log/proof?first=3&second=5");
    assert_eq!(status, 200, "consistency: {v}");
    let proof = LogConsistencyProof::from_value(&v).expect("closed-schema consistency proof");
    proof
        .verify_against_first_root(&retained.root_hash)
        .expect("the size-3 tree is a prefix of the size-5 tree (§9.2)");
    proof
        .log_checkpoint
        .verify_signature_with_key(Some(&receipt_key), None)
        .expect("second checkpoint signature");
}

// ─── dk-003: the did_methods gate on the binary ─────────────────────────────

/// With `did_methods = ["did:web"]` (what `docker/config.docker.toml`
/// shipped through 0.11.1) the binary does not advertise did:key and refuses
/// a did:key publish with `key_resolution_failed`; listing did:key makes the
/// same publish land. Both on Postgres, through the binary's own config path.
#[test]
#[serial]
fn pg_binary_did_key_follows_did_methods() {
    let Some(url) = pg_url_or_skip() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    let req = producer(0x61)
        .publish_request()
        .title("pg did:key gate")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("publish request");

    for (methods, advertised, want) in [
        ("[\"did:web\"]", false, 400),
        ("[\"did:web\", \"did:key\"]", true, 201),
    ] {
        let reg = start(
            dir.path(),
            if advertised { "dk-on" } else { "dk-off" },
            &url,
            &format!(
                "[auth]\nenabled = false\ndid_methods = {methods}\nanonymous_public_reads = true\n"
            ),
        );
        let (status, caps) = reg.get("/.well-known/acdp.json");
        assert_eq!(status, 200, "{caps}");
        let listed = caps["supported_did_methods"]
            .as_array()
            .expect("supported_did_methods")
            .iter()
            .any(|m| m == "did:key");
        assert_eq!(listed, advertised, "did_methods = {methods}: {caps}");
        let (status, v) = post_context(&reg, &req, None);
        assert_eq!(
            status,
            want,
            "did_methods = {methods}: {v}\n{}",
            reg.output()
        );
        if !advertised {
            assert_eq!(v["error"]["code"], "key_resolution_failed", "dk-003: {v}");
        }
    }
}
