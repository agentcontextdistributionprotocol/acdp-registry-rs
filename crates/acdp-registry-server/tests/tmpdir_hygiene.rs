//! U-542: the file-backed test harness must leave nothing behind in `$TMPDIR`.
//!
//! `tempfile::NamedTempFile` deletes exactly the file it owns. SQLite creates
//! `-wal` and `-shm` **beside** that path, so a guard that owns only the
//! database file orphans two files per test. Measured before this guard
//! existed: 312,898 `acdp-*` files in one `$TMPDIR`, of which 143,466 were
//! `.sqlite-wal` and 143,428 `.sqlite-shm` against **14** surviving bare
//! `.sqlite` — i.e. the parents were being cleaned and the siblings were not.
//!
//! ## Why this asserts on one harness's own paths, not on a `$TMPDIR` count
//!
//! The obvious shape — count `acdp-*` in `$TMPDIR` before and after — is
//! unusable here for two independent reasons:
//!
//! 1. **It races.** Several test binaries (and, on a developer machine, several
//!    checkouts) run concurrently against the same `$TMPDIR`. A process-wide
//!    count would be perturbed by unrelated processes and would fail randomly,
//!    which is the fastest way to teach everyone to ignore it.
//! 2. **It is slow and it cannot distinguish.** The directory already holds
//!    hundreds of thousands of entries from before the fix; scanning them per
//!    test costs real time and still cannot attribute a delta to *this* test.
//!
//! So the property is kept and sharpened. Each harness gets a unique temporary
//! name, so the three paths it could possibly leave are known exactly. Before
//! the harness is built none of them exist; after it is dropped none of them
//! may exist. That is the same before/after equality, scoped to something this
//! test owns outright — immune to concurrent processes and to the pre-existing
//! population.
//!
//! The assertion is an **equality against zero**, deliberately. A `<=` bound,
//! or "no more than before", would be satisfied by the very leak this exists to
//! catch.

// `common`'s harness is SQLite-backed, so this binary cannot compile under
// `--no-default-features --features storage-pg` or `storage-memory`.
// `http_integration.rs`, `conformance.rs` and `caps_visibility.rs` all carry
// the identical gate for the identical reason.
#![cfg(feature = "storage-sqlite")]

mod common;

use std::path::{Path, PathBuf};

use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp_registry_types::{
    AuthConfig, LimitsConfig, PlaygroundConfig, RegistryConfig, RegistrySection, StorageBackend,
    StorageConfig, WebhookConfig,
};
use common::{build_harness_with_webhook, StoreMode};

const AUTHORITY: &str = "registry.test";

fn base_caps() -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: "0.1.0".into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        supported_signature_algorithms: vec!["ed25519".into(), "ecdsa-p256".into()],
        supported_did_methods: vec!["did:web".into(), "did:key".into()],
        profiles: vec!["acdp-registry-core".into()],
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
        },
        read_authentication_methods: vec![],
        anonymous_public_reads: true,
        supports_idempotency_key: true,
        extensions: Default::default(),
    }
}

fn base_config() -> RegistryConfig {
    RegistryConfig {
        registry: RegistrySection {
            authority: AUTHORITY.into(),
            port: 8443,
            bind: "0.0.0.0".into(),
            allow_public_bind: false,
            profiles: vec!["acdp-registry-core".into()],
            tls: Default::default(),
            cross_registry_resolution: true,
            cors: Default::default(),
            base_url: String::new(),
        },
        storage: StorageConfig {
            backend: StorageBackend::Sqlite,
            postgres_url: None,
            sqlite_path: None,
            max_connections: 1,
        },
        auth: AuthConfig {
            anonymous_public_reads: true,
            did_methods: vec!["did:web".into(), "did:key".into()],
            ..AuthConfig::default()
        },
        webhook: WebhookConfig::default(),
        limits: LimitsConfig::default(),
        rate_limit: Default::default(),
        metrics: Default::default(),
        playground: PlaygroundConfig::default(),
        receipt: Default::default(),
        lifecycle: Default::default(),
        log: Default::default(),
        witnesses: Vec::new(),
    }
}

/// Every path a SQLite database at `db` can occupy on disk: the file itself
/// and the two sidecars the engine creates next to it in WAL mode.
fn sqlite_triplet(db: &Path) -> [PathBuf; 3] {
    let s = db.as_os_str().to_string_lossy().to_string();
    [
        db.to_path_buf(),
        PathBuf::from(format!("{s}-wal")),
        PathBuf::from(format!("{s}-shm")),
    ]
}

fn existing(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .collect()
}

/// Dropping a `StoreMode::File` harness must remove the database **and both
/// SQLite sidecars**.
///
/// This fails on the pre-U-542 harness, which owned a `NamedTempFile`: the
/// `-wal` and `-shm` files are created by SQLite beside that path and survive
/// the drop. It passes once the harness owns the *directory* containing the
/// database, because removing the directory removes everything SQLite put in
/// it.
#[tokio::test]
async fn file_backed_harness_leaves_no_sqlite_files_behind() {
    let db_path: PathBuf = {
        let h = build_harness_with_webhook(
            base_config(),
            base_caps(),
            AUTHORITY,
            StoreMode::File,
            None,
            None,
        )
        .await;

        let p = h.db_path().to_path_buf();

        // Guard the guard: if the database itself were missing while the
        // harness is still alive, this test would "pass" after the drop
        // without ever having exercised anything.
        assert!(
            p.exists(),
            "harness is alive but its database does not exist at {}; \
             this test cannot prove anything about cleanup",
            p.display()
        );

        p
    };

    // The harness is dropped. Nothing it created may remain.
    let leaked = existing(&sqlite_triplet(&db_path));

    assert_eq!(
        leaked.len(),
        0,
        "dropping the harness left {} file(s) behind: {}\n\
         `tempfile::NamedTempFile` removes only the path it owns; SQLite's \
         `-wal` and `-shm` are created beside it and are not covered. Own the \
         containing directory (`tempfile::TempDir`) so the whole tree goes.",
        leaked.len(),
        leaked.join(", ")
    );
}
