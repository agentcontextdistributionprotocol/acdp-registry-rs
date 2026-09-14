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

// ---------------------------------------------------------------------------
// U-545: the class, not this crate.
// ---------------------------------------------------------------------------

/// No SQLite database anywhere in the workspace may be opened on a path owned
/// by a *file* guard.
///
/// ## Why a source scan, when the test above is a real runtime check
///
/// The runtime test proves that *this crate's harness* cleans up. It cannot
/// prove anything about a site it does not call, and an integration test
/// cannot reach a `#[cfg(test)]` module inside `src/` at all — that is a
/// different binary. Both gaps were real: #309 fixed
/// `acdp-registry-server` and, by existing, made the class look closed, while
/// `acdp-registry-sqlite` kept leaking from `tests/` *and* from 13 sites inside
/// `src/store.rs`, and `acdp-registry-core` kept leaking from
/// `src/witness.rs`'s `#[cfg(test)]` module — a crate that appeared on nobody's
/// list of candidates.
///
/// ## Why it keys on the call shape rather than on file names
///
/// The obvious check — count `acdp-*` files in `$TMPDIR` — is what let this
/// survive. Those sites call `NamedTempFile::new()` with **no prefix**, so they
/// land as `.tmpXXXXXX`: 17,538 `-wal` + 17,538 `-shm` that an `acdp-*` census
/// could not see. A census is only as wide as its filter, and a no-prefix call
/// is the *default* shape, so it is the most likely form the next instance
/// takes. This scan therefore looks at what the code does, not at what it names
/// its files.
///
/// ## What this does NOT catch — read before trusting it
///
/// 1. **A guard that travels.** The pattern matched is a file guard bound and
///    connected within a short window. A `NamedTempFile` returned from a
///    helper, stored in a struct, or passed across a function boundary and
///    connected elsewhere is invisible here.
/// 2. **Other sidecar-writing libraries.** It knows SQLite. A different library
///    that writes siblings next to a path it is given has the same defect and is
///    not checked.
/// 3. **Non-Rust callers**, and any crate outside `crates/`.
/// 4. It is a **source scan**: it fails on the pattern being present, never on
///    a correct-but-absent test. Deleting a fixture entirely leaves it green.
#[test]
fn no_sqlite_database_is_backed_by_a_file_guard() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();

    let crates_dir = root.join("crates");
    assert!(
        crates_dir.is_dir(),
        "expected a crates/ directory at {} — the scan resolved the workspace \
         root wrongly and would otherwise pass by finding nothing",
        crates_dir.display()
    );

    let mut rs_files = Vec::new();
    collect_rs(&crates_dir, &mut rs_files);

    // Guard the guard: if the walk found no files the assertion below is
    // vacuous, and a refactor that moves the tree would silently disable it.
    assert!(
        rs_files.len() > 50,
        "only {} .rs files found under {} — the walk is broken, and an empty \
         walk makes this test pass by construction",
        rs_files.len(),
        crates_dir.display()
    );

    let mut offenders = Vec::new();
    for path in &rs_files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(binding) = file_guard_binding(line) else {
                continue;
            };
            // The database connect follows the guard closely in every real
            // instance. Ten lines covers the multi-line builder form.
            let end = (i + 10).min(lines.len());
            for probe in &lines[i + 1..end] {
                if probe.contains("::connect(") && probe.contains(&format!("{binding}.path()")) {
                    offenders.push(format!(
                        "{}:{} — `{}` is a file guard, and its path is opened as a SQLite \
                         database. SQLite writes `-wal` and `-shm` beside it, which the guard \
                         does not own. Use `tempfile::tempdir()` and put the database inside it.",
                        path.strip_prefix(&root).unwrap_or(path).display(),
                        i + 1,
                        binding,
                    ));
                    break;
                }
            }
        }
    }

    assert_eq!(
        offenders.len(),
        0,
        "{} site(s) open a SQLite database on a file-guarded path:\n  {}",
        offenders.len(),
        offenders.join("\n  "),
    );
}

/// The name bound by a `tempfile` **file** guard on this line, if any.
///
/// `tempfile::tempdir()` and `TempDir` are deliberately not matched: owning the
/// directory is the fix, not the defect.
fn file_guard_binding(line: &str) -> Option<String> {
    if !(line.contains("NamedTempFile::new(") || line.contains(".tempfile()")) {
        return None;
    }
    let after_let = line.trim().strip_prefix("let ")?;
    let name: String = after_let
        .trim_start_matches("mut ")
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            // `target/` holds generated and vendored sources; scanning it is
            // slow and its contents are not ours to fix.
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}
