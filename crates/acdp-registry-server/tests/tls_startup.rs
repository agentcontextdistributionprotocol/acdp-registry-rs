//! TLS startup: the crypto provider must be installed, and it must be
//! installed in the **shipped binary**, not merely in a test build.
//!
//! ## What went wrong, so the shape of these tests is legible
//!
//! This binary's graph enables **two** rustls providers through edges nobody
//! selects per-build: `axum-server`'s `tls-rustls` turns on `aws-lc-rs` (root
//! `Cargo.toml:35`) and `reqwest`'s `rustls-tls` turns on `ring` via
//! `hyper-rustls` (`:72`). Both are unconditional. rustls therefore refuses to
//! choose, and with `registry.tls.enabled = true` the process panicked
//! (**rc=101**) inside the provider lookup — *after* `main` had logged
//! `listening` with the bind address. The operator-visible symptom was "it
//! says it is listening and the port refuses connections".
//!
//! ## Two checks, because they fail for different reasons
//!
//! [`tls_startup_installs_a_provider_and_serves`] spawns the real binary and
//! is the one that would have caught the defect. [`rustls_is_a_normal_dependency`]
//! is cheap, needs no cert, and catches the narrower thing the spawn test
//! cannot distinguish: that the manifest move actually landed, rather than
//! `rustls` silently remaining dev-only — the state in which the panic could
//! not reach any test binary in the first place.
//!
//! **Neither test asserts the ORDER** in which the provider install and the
//! `listening` log occur. Measured, not assumed: moving
//! `install_crypto_provider()` below that log leaves both tests **green**.
//!
//! That limit is deliberate, and the reason is worth more than the limit. The
//! ordering only becomes observable if the install itself *fails*, and with a
//! single call site it cannot — `install_default` fails only on a second
//! install. So the ordering is a **defensive** property with no reachable
//! failure, and a test for it would have to manufacture one. Keeping it is
//! still right: the original defect's worst feature was that `listening`
//! appeared before a fatal error, and an operator reading logs deserves that
//! order to hold if this ever does fail. But it is pinned by the comment at
//! the call site in `main.rs` and by nothing executable, and a reader should
//! not infer coverage that is not here.

/// `rustls` must be a **normal** dependency of this crate, carrying exactly
/// one provider feature.
///
/// Read from the manifest rather than from `cargo tree`'s output on purpose.
/// The manifest is declarative and order-independent; a scrape of output a
/// tool formatted for human reading has to be re-verified every time that
/// formatting changes, and counting lines in it is how a query says `<none>`
/// when it means `rc=1`.
#[test]
fn rustls_is_a_normal_dependency() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("this crate's Cargo.toml is readable");

    // Section-scan: collect the lines of `[dependencies]` only, stopping at
    // the next table header so `[dev-dependencies]` can never satisfy this.
    let mut in_deps = false;
    let mut deps: Vec<&str> = Vec::new();
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]";
            continue;
        }
        if in_deps {
            deps.push(t);
        }
    }
    // The scan must be shown a positive: a section-parser that silently
    // matched nothing would make every assertion below vacuous.
    assert!(
        deps.iter().any(|l| l.starts_with("axum-server")),
        "the [dependencies] section scan found no `axum-server` line, so it is not reading \
         the section it thinks it is — every assertion below would be vacuous"
    );

    let rustls_line = deps
        .iter()
        .find(|l| l.starts_with("rustls") && !l.starts_with("rustls-"))
        .unwrap_or_else(|| {
            panic!(
                "`rustls` is not in [dependencies]. If it is only in [dev-dependencies], the \
                 shipped binary has no crypto provider and `tls.enabled = true` panics at \
                 startup with rc=101 — which no test build can observe, because a dev build \
                 gets the dev-dependency and the real binary does not."
            )
        });
    assert!(
        rustls_line.contains("\"ring\""),
        "`rustls` must request exactly one provider feature, and `ring` is the recorded \
         choice (D-W5-105): {rustls_line}"
    );
    assert!(
        !rustls_line.contains("aws-lc-rs"),
        "requesting both providers is the original defect: rustls cannot choose and panics \
         at startup: {rustls_line}"
    );
}

/// Spawn the **real binary** with `tls.enabled = true` and require that it
/// serves HTTPS.
///
/// This is the test that would have caught the defect, and it has to spawn the
/// binary rather than build a router in-process: the failure was that the
/// *shipped* binary had no crypto provider. A test build links whatever the
/// test's own dependency set provides, so an in-process assertion can pass
/// against a binary that panics.
///
/// **Its red state was measured before it was written**, which is the only
/// reason it is worth anything: on `main` immediately before this change, the
/// same spawn exited **101** with
/// `Could not automatically determine the process-level CryptoProvider from
/// Rustls crate features`, *after* logging `listening`.
///
/// The cert is generated here rather than committed: `.gitignore:39-43`
/// refuses certificate material, and a checked-in PEM would expire.
///
/// **Compiled out under `storage-pg`** rather than skipped at runtime. That
/// build needs a live database to get past storage init, and CI's `postgres`
/// job runs only `--test pg_integration`, so this test would never execute
/// there anyway. A `#[cfg]` leaves no skip branch that could quietly swallow a
/// real failure; a runtime `return` would.
#[cfg(not(feature = "storage-pg"))]
#[tokio::test(flavor = "multi_thread")]
async fn tls_startup_installs_a_provider_and_serves() {
    use std::io::Write as _;

    // The backend MUST be derived from the compiled feature set, not written
    // as a literal. A spawned binary inherits the entire config surface, so a
    // fixture naming a backend the binary was not built with is an undeclared
    // `#[cfg]`: this test first shipped with `backend = "sqlite"` hard-coded
    // and failed CI's `--no-default-features --features storage-memory` job,
    // where the binary exits 1 at storage init long before any TLS work.
    // The features are mutually exclusive (see `main.rs`'s `compile_error!`),
    // so exactly one arm applies.
    let dir = tempfile::tempdir().expect("tempdir");

    #[cfg(feature = "storage-sqlite")]
    let (backend, extra) = (
        "sqlite",
        format!("sqlite_path = \"{}\"", dir.path().join("t.db").display()),
    );
    #[cfg(feature = "storage-memory")]
    let (backend, extra) = ("memory", String::new());

    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("self-signed cert");
    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");
    std::fs::write(&cert_path, cert.cert.pem()).expect("write cert");
    std::fs::write(&key_path, cert.signing_key.serialize_pem()).expect("write key");

    // Port 0 would be ideal, but the binary logs the resolved address and we
    // need to connect: pick a high port unlikely to collide in CI.
    let port = 18443u16;
    let cfg_path = dir.path().join("tls.toml");
    let mut f = std::fs::File::create(&cfg_path).expect("create config");
    write!(
        f,
        r#"
[registry]
authority = "localhost"
port = {port}
bind = "127.0.0.1"

[registry.tls]
enabled = true
cert_path = "{}"
key_path = "{}"

[storage]
backend = "{backend}"
{extra}
"#,
        cert_path.display(),
        key_path.display(),
    )
    .expect("write config");
    drop(f);

    // Captured, not discarded: in CI the panic message is usually the only
    // artifact anyone reads, so a failure has to carry the binary's own output.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_acdp-registry"))
        .env("ACDP_REGISTRY_CONFIG", &cfg_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the registry binary");

    // Poll rather than sleep a fixed interval: the failure mode being guarded
    // against is an EARLY exit, so check for it on every attempt instead of
    // racing the unwind. (A single 2s liveness check reported "running" while
    // the process was mid-panic during the investigation for this fix.)
    let mut served = false;
    for _ in 0..50 {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let out = child.wait_with_output().expect("collect output");
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            // **Branch on the observed code.** The first version of this test
            // printed the 101 explanation for EVERY early exit, so when it
            // failed with exit 1 for an unrelated reason the message said
            // "exit code 101 is the rustls panic this test exists for" and a
            // reader concluded the fix had failed. A fixed explanation of the
            // INTENDED failure makes an unintended one look diagnosed.
            if status.code() == Some(101) {
                panic!(
                    "the registry exited during startup with {status}. Exit code 101 IS the \
                     rustls panic this test exists for: the binary enables two crypto \
                     providers (aws-lc-rs via axum-server, ring via reqwest) and must \
                     install one explicitly in `main` before it logs `listening`.\n\
                     --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
                );
            }
            panic!(
                "the registry exited during startup with {status}, which is NOT the 101 this \
                 test exists for — do not read this as the crypto-provider defect. Something \
                 else stopped startup (config, storage init, port in use).\n\
                 --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
            );
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            served = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let still_running = child.try_wait().expect("try_wait").is_none();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        served && still_running,
        "the registry never accepted a TLS connection on 127.0.0.1:{port} \
         (served={served}, still_running={still_running})"
    );
}
