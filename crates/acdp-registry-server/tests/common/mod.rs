//! Shared HTTP-integration test harness, `mod`-included by both
//! `http_integration.rs` and `conformance.rs`.
//!
//! Rust integration tests are compiled into separate binaries, so this
//! module is compiled once *per* test binary that includes it via
//! `mod common;`. Some items here (e.g. [`forged_bearer`]) are only used
//! by one of the two files today — the `#![allow(dead_code)]` below is
//! required so that doesn't turn into an unused-item warning (promoted to
//! a hard error under this workspace's `RUSTFLAGS: "-D warnings"`) in the
//! binary that doesn't yet use it.
#![allow(dead_code)]
// Under a non-sqlite build this module is already a hard error (see the
// `compile_error!` below), so the eight imports that only the gated items use
// would add eight `unused import` errors under `-D warnings` and bury the one
// message that tells the author what to do. Measured: without this the probe
// reported 9 errors; with it, 1.
#![cfg_attr(not(feature = "storage-sqlite"), allow(unused_imports))]

// This module is SQLite-backed and cannot declare the feature its includers must
// have, so the requirement used to be maintained by hand in every `tests/*.rs`
// that says `mod common;`. Four files agreed and the fifth did not (#256), and
// what its author saw was `E0432: unresolved import acdp_registry_sqlite`
// pointing into THIS file, which they had never opened — a diagnostic that names
// neither the cause nor the fix.
//
// This fires only when `common` is actually compiled, which happens only when
// some test file pulls it in. A correctly gated includer compiles `mod common;`
// out of existence and never reaches this line, so the cost to every existing
// caller is zero.
//
// Measured, and the measurement changed the design. 4 of 16 top-level items
// touch `SqliteStore` -- `build_harness_with_webhook`, `SeededHarness` (struct
// and impl) and `wire_server` -- so item-level `#[cfg]` is cheap. I first took
// that as the REJECTED alternative and shipped only the `compile_error!`; the
// probe refuted it. `compile_error!` alone does NOT replace the confusing
// diagnostic, it merely precedes it: rustc still resolves the unconditional
// SQLite import below and still reports E0432, so the author sees BOTH
// ("2 previous errors").
//
// So both halves are here and each does a different job, and the division was
// measured on a throwaway ungated probe rather than reasoned about. The
// `#[cfg]`s below remove E0432 by making the SQLite-dependent items not exist.
// What the `compile_error!` adds is not "a nicer error" but coverage of a case
// the `#[cfg]`s alone fail SILENTLY. Three shapes, all three measured:
//
//   * a correctly gated includer compiles `mod common;` out of existence and
//     reaches none of this -- every CI leg unchanged;
//   * an UNGATED includer that touches only the twelve store-agnostic helpers
//     compiles CLEANLY without the `compile_error!` (rc=0) -- the gate is simply
//     missing, nothing says so, and the mistake stays latent until somebody adds
//     the first harness call;
//   * an ungated includer that does touch the harness gets
//     `E0425: cannot find function build_harness_with_webhook in module common`
//     -- a second diagnostic that names neither the cause nor the fix.
//
// With the `compile_error!`, the second and third both become the directive
// below. I had originally written that the `#[cfg]`s alone would produce the
// E0425; the probe showed they can also produce nothing at all, which is the
// stronger reason for keeping both.
#[cfg(not(feature = "storage-sqlite"))]
compile_error!(
    "tests/common is SQLite-backed. Gate your test file with \
     `#![cfg(feature = \"storage-sqlite\")]` above `mod common;` (see \
     http_integration.rs, conformance.rs, metrics_integration.rs) or it will \
     fail the storage-pg and storage-memory CI legs."
);

use std::sync::Arc;

use acdp::client::CrossRegistryResolver;
use acdp::crypto::SigningKey;
use acdp::did::WebResolver;
use acdp::producer::Producer;
use acdp::registry::RegistryServer;
use acdp::types::capabilities::CapabilitiesDocument;
use acdp::types::primitives::AgentDid;
use acdp::types::publish::PublishRequest;
use acdp_registry_auth::{
    AuthService, ChallengeStore, InMemoryChallengeStore, JwtSecret, JwtSigner,
};
use acdp_registry_core::{build_router, AppStateInner};
#[cfg(feature = "storage-sqlite")]
use acdp_registry_sqlite::SqliteStore;
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::auth::{AcdpClaims, BearerClaims};
use acdp_registry_types::{AuthConfig, RegistryConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

/// Registry authority used to mint bearers in [`forged_bearer`]. Both
/// `http_integration.rs` and `conformance.rs` build their harnesses
/// against this same literal (each keeps its own local `AUTHORITY`
/// const for the rest of its file); kept private here since only
/// `forged_bearer` needs it.
const AUTHORITY: &str = "registry.test";

/// Selects whether [`build_harness_with_webhook`] backs the registry's
/// `SqliteStore` with a real on-disk tempfile or an in-memory database.
/// `http_integration.rs` needs the former (some tests reach past the HTTP
/// surface, e.g. to age an idempotency record); `conformance.rs` uses the
/// latter for per-fixture speed. See `Harness::db_path`.
pub enum StoreMode {
    Memory,
    File,
}

/// Per-test handle. When built with [`StoreMode::File`], `db` keeps the
/// backing tempfile alive for the duration of the test (the `Router`
/// shares a single SQLite store across all routes; the tempfile is
/// dropped, and the DB file deleted, when the harness is dropped). Built
/// with [`StoreMode::Memory`], there is no backing file and `db` is
/// `None`.
pub struct Harness {
    pub router: axum::Router,
    pub db: Option<tempfile::NamedTempFile>,
}

impl Harness {
    /// Path to the backing SQLite file, for tests that need to reach past
    /// the HTTP surface (e.g. ageing an idempotency record to simulate
    /// expiry without sleeping). Only valid for a [`StoreMode::File`]
    /// harness.
    pub fn db_path(&self) -> &std::path::Path {
        self.db
            .as_ref()
            .expect("db_path() called on a StoreMode::Memory harness")
            .path()
    }
}

/// Builds a full `Harness` (or bare `Router`, via `.router`) the same way
/// the real binary's `run()` wires a `RegistryServer` + `AppStateInner`:
/// transparency log, receipt signer, lineage head receipts, and lifecycle
/// are each attached iff `cfg` turns them on, mirroring `serve_with_store`.
///
/// Lets the caller wire a real, already-spawned `WebhookEmitter` instead
/// of always disabling webhook delivery. REG-3 Phase 6
/// (`plans/reg3-anchors.md`) needs this: proving `anchors[].uri` is never
/// dereferenced is only meaningful if the one subsystem that *does* make
/// outbound HTTP calls near the publish path (webhook delivery) is
/// actually live during the test.
#[cfg(feature = "storage-sqlite")]
pub async fn build_harness_with_webhook(
    cfg: RegistryConfig,
    caps: CapabilitiesDocument,
    authority: &str,
    store_mode: StoreMode,
    cross_registry: Option<Arc<CrossRegistryResolver>>,
    webhook: Option<acdp_registry_webhook::WebhookEmitter>,
) -> Harness {
    let (store, db) = match store_mode {
        StoreMode::File => {
            let db = tempfile::Builder::new()
                .prefix("acdp-test-")
                .suffix(".sqlite")
                .tempfile()
                .unwrap();
            let store = SqliteStore::connect(db.path(), 1).await.unwrap();
            (store, Some(db))
        }
        StoreMode::Memory => {
            let store = SqliteStore::connect_in_memory().await.unwrap();
            (store, None)
        }
    };
    // RFC-ACDP-0012: mirror the binary's run() wiring — an enabled [log]
    // makes every commit_publish append the leaf atomically.
    let store = if cfg.log.enabled {
        store.with_transparency_log()
    } else {
        store
    };
    store.migrate().await.unwrap();
    let server = RegistryServer::try_new(store, caps, authority).unwrap();
    // Mirror the binary's serve_with_store wiring: a configured receipt
    // key attaches the signer (which also appends the receipts profile).
    let server = if cfg.receipt.is_configured() {
        let signer =
            acdp_registry_core::receipt::build_signer(&cfg.receipt, &cfg.registry.authority)
                .expect("receipt signer");
        server.with_receipt_signer(signer).expect("receipt signer")
    } else {
        server
    };
    // RFC-ACDP-0011: head receipts on /current (requires the signer).
    let server = if cfg.receipt.head_receipts {
        server
            .with_lineage_head_receipts()
            .expect("head receipts enabled")
    } else {
        server
    };
    // RFC-ACDP-0013: lifecycle events & retraction.
    let server = if cfg.lifecycle.enabled {
        server.with_lifecycle().expect("lifecycle enabled")
    } else {
        server
    };
    let server = Arc::new(server);
    let challenges: Arc<dyn ChallengeStore> = Arc::new(InMemoryChallengeStore::new());
    let secret = JwtSecret::from_bytes(&[42u8; 32]);
    let signer = JwtSigner::new(secret, format!("did:web:{authority}"), authority.into(), 30);
    let resolver = Arc::new(WebResolver::new());
    let auth = Arc::new(AuthService::new(
        AuthConfig::default(),
        challenges,
        signer,
        resolver,
        authority.into(),
    ));
    let state = AppStateInner::new(server, auth, webhook, cfg, cross_registry);
    Harness {
        router: build_router(state),
        db,
    }
}

/// A per-fixture, stateful harness for Shape D (REG-10 Phase 8's
/// `conformance.rs`). Unlike [`build_harness_with_webhook`] -- a one-shot
/// `Router` built once and shared by every Shape A/B/C exchange -- this
/// keeps the underlying `Arc<RegistryServer<SqliteStore>>` and
/// `Arc<AuthService>` alive so [`SeededHarness::rebuild`] can produce a
/// NEW `Router` (and, when `caps` changes, a NEW `RegistryServer`) against
/// a different `RegistryConfig`/`CapabilitiesDocument` (e.g. to flip
/// `anonymous_public_reads` for one scenario, per `vis-002`/`vis-009`'s
/// `registry_capabilities_subset`) WITHOUT losing already-seeded state --
/// the seeded contexts live in the SQLite store, which `rebuild` clones
/// (cheap: `SqliteStore` is pool-backed) rather than recreates.
#[cfg(feature = "storage-sqlite")]
pub struct SeededHarness {
    server: Arc<RegistryServer<SqliteStore>>,
    auth: Arc<AuthService>,
    authority: String,
    pub router: axum::Router,
}

#[cfg(feature = "storage-sqlite")]
impl SeededHarness {
    /// Build a fresh in-memory store + router (REG-10 Phase 8's isolation
    /// requirement: every Shape D fixture gets its own `SeededHarness`,
    /// never the shared `app` Shapes A/B/C replay against). Mirrors
    /// [`build_harness_with_webhook`]'s wiring, minus the `db`/webhook
    /// plumbing Shape D doesn't need.
    pub async fn new(cfg: RegistryConfig, caps: CapabilitiesDocument, authority: &str) -> Self {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let store = if cfg.log.enabled {
            store.with_transparency_log()
        } else {
            store
        };
        store.migrate().await.unwrap();
        let server = wire_server(store, caps, authority, &cfg);
        let server = Arc::new(server);
        let challenges: Arc<dyn ChallengeStore> = Arc::new(InMemoryChallengeStore::new());
        let secret = JwtSecret::from_bytes(&[42u8; 32]);
        let signer = JwtSigner::new(secret, format!("did:web:{authority}"), authority.into(), 30);
        let resolver = Arc::new(WebResolver::new());
        let auth = Arc::new(AuthService::new(
            AuthConfig::default(),
            challenges,
            signer,
            resolver,
            authority.into(),
        ));
        let router = build_router(AppStateInner::new(
            server.clone(),
            auth.clone(),
            None,
            cfg,
            None,
        ));
        Self {
            server,
            auth,
            authority: authority.to_string(),
            router,
        }
    }

    /// Rebuild `router` against `cfg` AND `caps`, reusing the SAME
    /// underlying store data.
    ///
    /// GAP 3 (REG-10 Phase 8 fixer pass): `RegistryServer::search` /
    /// `::retrieve` gate `anonymous_public_reads` (and every other
    /// capability-driven authorization decision) off `self.caps` -- the
    /// `CapabilitiesDocument` baked in at `RegistryServer::try_new` /
    /// `RegistryServer::new` time, NOT off `RegistryConfig` -- so an
    /// earlier version of this method, which only rebuilt the `Router`
    /// against a new `cfg` while reusing the SAME `Arc<RegistryServer>`
    /// (and therefore the SAME, unchanged `caps`), could never actually
    /// change authorization-relevant behavior. `vis-002`/`vis-009`-style
    /// scenarios (`registry_capabilities_subset` overriding
    /// `anonymous_public_reads`) would have silently kept exercising the
    /// harness's ORIGINAL `anonymous_public_reads` regardless of the
    /// override, which `seeded_harness_rebuild_changes_router_behavior_and_preserves_seeded_state`
    /// (`conformance.rs`) now proves directly.
    ///
    /// The fix: reconstruct `server` too, from a CLONE of the current
    /// store (`SqliteStore` is pool-backed -- cloning it shares the same
    /// underlying connections/data, it does not create a second database)
    /// combined with the new `caps`. This is what actually makes the new
    /// `anonymous_public_reads` value take effect, while every context
    /// published before the rebuild remains readable afterward.
    pub fn rebuild(&mut self, cfg: RegistryConfig, caps: CapabilitiesDocument) {
        let store = self.server.store().clone();
        let server = Arc::new(wire_server(store, caps, &self.authority, &cfg));
        self.server = server.clone();
        self.router = build_router(AppStateInner::new(
            server,
            self.auth.clone(),
            None,
            cfg,
            None,
        ));
    }
}

/// Shared `RegistryServer` construction + `with_*` wiring, mirroring the
/// binary's `serve_with_store` (see [`build_harness_with_webhook`]'s doc
/// comment). Factored out so [`SeededHarness::new`] and
/// [`SeededHarness::rebuild`] apply IDENTICAL wiring -- a rebuild that
/// silently dropped, say, the receipt signer because it hand-rolled a
/// shorter version of this would be its own latent bug.
#[cfg(feature = "storage-sqlite")]
fn wire_server(
    store: SqliteStore,
    caps: CapabilitiesDocument,
    authority: &str,
    cfg: &RegistryConfig,
) -> RegistryServer<SqliteStore> {
    let server = RegistryServer::try_new(store, caps, authority).unwrap();
    let server = if cfg.receipt.is_configured() {
        let signer =
            acdp_registry_core::receipt::build_signer(&cfg.receipt, &cfg.registry.authority)
                .expect("receipt signer");
        server.with_receipt_signer(signer).expect("receipt signer")
    } else {
        server
    };
    let server = if cfg.receipt.head_receipts {
        server
            .with_lineage_head_receipts()
            .expect("head receipts enabled")
    } else {
        server
    };
    if cfg.lifecycle.enabled {
        server.with_lifecycle().expect("lifecycle enabled")
    } else {
        server
    }
}

/// Set `anonymous_public_reads` on **both** the config and the capabilities
/// document from one input, returning them for a harness constructor.
///
/// # Why this exists — the two knobs look redundant and are not
///
/// `RegistryServer::retrieve` / `::search` gate `anonymous_public_reads` off the
/// `CapabilitiesDocument` baked in at `RegistryServer::try_new`, **not** off
/// `RegistryConfig` (documented as GAP 3 on [`SeededHarness::rebuild`]). So a
/// test that flips `cfg.auth.anonymous_public_reads` and observes a 200 has
/// measured the harness's caps/config split and **nothing about the binary** —
/// which is exactly the invalid "wire probe" that let a shipping
/// anonymous-disclosure bug through review (#255). Meanwhile the config value is
/// not dead either: it is what the real binary's `build_capabilities` derives the
/// caps value *from*, so a test that sets only caps is testing a state the
/// deployed system cannot reach.
///
/// Both therefore have to move together, and across this crate's test files that
/// agreement is maintained by hand at every construction site. This is the one
/// call that cannot get it wrong: **there is a single input, so divergence is
/// unrepresentable through this path.**
///
/// # What this deliberately does NOT do
///
/// It does not *enforce* the invariant on callers that set the fields directly.
/// A `debug_assert` in the shared wiring would have, but it would also have
/// reddened `admin_list_returns_rows_under_the_shipped_disclosure_default` in
/// `http_integration.rs` — a file this lane does not own — where
/// `config_shipped_disclosure_default` sets the config flag and leaves caps at
/// `true`. That divergence is currently harmless because `admin_list` reads the
/// flag from *neither* source (`handlers/admin.rs` hardcodes
/// `admin_sees_public_arm = true`), but breaking another lane's test to enforce
/// an invariant constructively available to every new caller is not a trade this
/// unit gets to make. Reported instead.
pub fn with_anonymous_public_reads(
    mut cfg: RegistryConfig,
    mut caps: CapabilitiesDocument,
    allow: bool,
) -> (RegistryConfig, CapabilitiesDocument) {
    cfg.auth.anonymous_public_reads = allow;
    caps.anonymous_public_reads = allow;
    (cfg, caps)
}

/// A signing producer identity, namespaced by `prefix` so different test
/// files (or different fixture families within one file) don't collide on
/// DID/seed space — e.g. `http_integration.rs` uses `"smoke"`,
/// `conformance.rs`'s anc-* tests use `"anc"`.
pub fn producer(prefix: &str, seed: u8) -> Producer {
    Producer::new(
        SigningKey::from_bytes(&[seed; 32]),
        AgentDid::new(format!("did:web:agents.test:{prefix}-{seed}")),
        format!("did:web:agents.test:{prefix}-{seed}#key-1"),
    )
}

/// Forge an untenanted bearer for `sub` expiring `exp_offset_seconds` from
/// now — the exact claim set `AuthService::issue_token` mints, signed
/// against the harness's own `JwtSecret::from_bytes(&[42u8; 32])` (the
/// same HS256 secret the default harness wires), so the harness's
/// `AuthService` accepts it without a challenge/token round-trip. A
/// negative offset yields an already-expired token (the harness signer's
/// leeway is 30s).
pub fn forged_bearer(sub: &str, jti: &str, exp_offset_seconds: i64) -> String {
    let secret = JwtSecret::from_bytes(&[42u8; 32]);
    let signer = JwtSigner::new(secret, format!("did:web:{AUTHORITY}"), AUTHORITY.into(), 30);
    let now = chrono::Utc::now().timestamp();
    let claims = BearerClaims {
        iss: format!("did:web:{AUTHORITY}"),
        sub: sub.to_string(),
        aud: AUTHORITY.into(),
        jti: jti.to_string(),
        iat: now - 60,
        exp: now + exp_offset_seconds,
        acdp: AcdpClaims {
            registry: AUTHORITY.into(),
            key_id: format!("{sub}#key-1"),
        },
        tenant: None,
    };
    signer.sign(&claims).unwrap()
}

/// POST `req` to `/contexts`, optionally with an `Idempotency-Key`.
pub async fn publish(
    app: &axum::Router,
    req: &PublishRequest,
    idem: Option<&str>,
) -> (StatusCode, Value) {
    let body = serde_json::to_vec(req).unwrap();
    let mut builder = Request::builder().method("POST").uri("/contexts");
    if let Some(k) = idem {
        builder = builder.header("Idempotency-Key", k);
    }
    let resp = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let v = body_to_json(resp).await;
    (status, v)
}

/// GET `/contexts/{ctx_id}`, optionally with a bearer and/or
/// `X-Tenant-Id`, and return just the status.
pub async fn get_with_auth(
    app: &axum::Router,
    ctx_id: &str,
    bearer: Option<&str>,
    tenant: Option<&str>,
) -> StatusCode {
    let mut builder =
        Request::builder().uri(format!("/contexts/{}", pct_encode_path_segment(ctx_id)));
    if let Some(t) = bearer {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    if let Some(t) = tenant {
        builder = builder.header("X-Tenant-Id", t);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

/// Drains and parses a response body as JSON, panicking on an empty or
/// non-JSON body. This is the strict, default helper — use it unless a
/// call site specifically needs [`body_to_json_lenient`].
pub async fn body_to_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Drains and parses a response body as JSON, but tolerates an empty or
/// non-JSON body by returning `Value::Null` instead of panicking. Only
/// for replaying arbitrary spec fixtures (e.g. a bare `204 No Content`),
/// where an empty/non-JSON body is legitimately possible and the fixture
/// may assert on status alone; every other call site should use the
/// strict [`body_to_json`].
pub async fn body_to_json_lenient(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// `acdp://authority/uuid`-style ctx_ids contain `/` and `:`, which axum's
/// single-segment `{ctx_id}` route param won't match unless they're percent-
/// encoded by the client. Mirror the encoding rule here (RFC 3986 §2.3).
pub fn pct_encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
