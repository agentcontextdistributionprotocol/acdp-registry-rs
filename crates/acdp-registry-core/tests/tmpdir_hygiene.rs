//! U-545: this crate's file-backed SQLite fixtures must leave nothing in `$TMPDIR`.
//!
//! `acdp-registry-core` was **unguarded** until this file existed, and it was
//! leaking: `src/witness.rs`'s `store_with_size` handed a
//! `tempfile::NamedTempFile`'s path to `SqliteStore::connect`, and SQLite
//! creates `-wal` and `-shm` *beside* that path, so both were orphaned on
//! every call. #309 fixed the same defect in `acdp-registry-server` and, by
//! existing, made the class look closed — this crate was in nobody's list.
//!
//! ## What this file asserts, and what the scan next door asserts
//!
//! These are two different questions and neither covers the other:
//!
//! - **Here (runtime):** the idiom this crate uses actually cleans up — the
//!   database, the `-wal` and the `-shm`. Real behaviour, real SQLite, on this
//!   platform and this version.
//! - **`acdp-registry-server/tests/tmpdir_hygiene.rs` (source scan):** *every*
//!   site in the workspace uses that idiom. A runtime test can only exercise
//!   the sites it calls, and an integration test cannot reach a `#[cfg(test)]`
//!   module inside `src/` at all — a separate binary — which is precisely where
//!   this crate's leak lived.
//!
//! Neither alone closes the class. Together: the scan proves every site takes
//! the shape, this proves the shape is correct.
//!
//! ## The liveness control
//!
//! Asserting "nothing remains" is worthless if nothing was ever created — a
//! passing run would be indistinguishable from one where SQLite never opened a
//! WAL. So the sidecars are asserted **present while the store is open**,
//! before any claim is made about their removal.

use std::path::PathBuf;

use acdp_registry_sqlite::SqliteStore;
// `migrate` is a trait method; the trait must be in scope to call it.
use acdp_registry_store::ExtendedRegistryStore;

/// The two files SQLite creates beside a database in WAL mode.
fn sidecars(db: &std::path::Path) -> (PathBuf, PathBuf) {
    let s = db.as_os_str().to_string_lossy().to_string();
    (
        PathBuf::from(format!("{s}-wal")),
        PathBuf::from(format!("{s}-shm")),
    )
}

#[tokio::test]
async fn file_backed_store_leaves_no_sqlite_files_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("witness.sqlite");
    let (wal, shm) = sidecars(&db);

    let store = SqliteStore::connect(&db, 1).await.expect("connect");
    store.migrate().await.expect("migrate");

    // Liveness control. If SQLite were not in WAL mode, or the store never
    // touched disk, the post-drop assertions below would pass without having
    // exercised anything. Fail loudly here instead of reporting a vacuous
    // success.
    assert!(
        db.exists(),
        "database was never created at {} — this test cannot prove cleanup",
        db.display()
    );
    assert!(
        wal.exists() && shm.exists(),
        "no -wal/-shm were created beside {}, so this test would pass whether \
         or not cleanup works. Either SQLite is not in WAL mode here or the \
         store never wrote; investigate rather than trusting a green run.",
        db.display()
    );

    drop(store);
    let root = dir.path().to_path_buf();
    drop(dir);

    // Equality against zero, not a bound: "no more than before" is satisfied
    // by the very leak this exists to catch.
    let leaked: Vec<String> = [db, wal, shm]
        .iter()
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .collect();

    assert_eq!(
        leaked.len(),
        0,
        "dropping the owning TempDir left {} file(s) behind: {}",
        leaked.len(),
        leaked.join(", ")
    );
    assert!(
        !root.exists(),
        "the temporary directory itself survived at {}",
        root.display()
    );
}
