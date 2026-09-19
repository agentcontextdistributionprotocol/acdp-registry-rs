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
//! **Two providers is the permanent state, by decision** — recorded in
//! `docs/ENGINEERING-LOG.md` under U-560, which is where to read the reasoning
//! rather than re-deriving it here. In short: dropping the second provider was
//! proposed and refused, because this crate wants `ring` specifically
//! (`Cargo.toml:41-43` — `aws-lc-rs` drags `aws-lc-sys` + `prebuilt-nasm`, a
//! C/asm build dependency, into the shipped binary), and rustls declares
//! `prefer-post-quantum = ["aws_lc_rs"]`, so a `ring`-only build gives up
//! post-quantum hybrid key exchange. Post-quantum won.
//!
//! Note what is *not* part of that trade, because an earlier draft of this
//! comment said it was: **TLS 1.2 is provider-independent**
//! (`rustls-0.23.45/Cargo.toml:97`, `tls12 = []`). A single-provider build
//! would have to re-add `tls12` explicitly only because it reaches
//! single-provider via `default-features = false`, which drops every default —
//! that is a curable detail of the mechanism, not a cost of choosing one
//! provider.
//!
//! So `install_crypto_provider()` (`main.rs:99-108`, called at `:114`) is
//! load-bearing for good rather than a workaround awaiting a manifest fix, and
//! [`rustls_is_a_normal_dependency`] asserts the invariant that makes it
//! necessary.
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

/// `rustls` must be a **normal** dependency of this crate, and this process
/// must still be unable to pick a provider on its own.
///
/// Two assertions, and they are reachable from different places on purpose.
///
/// **Normal-vs-dev-only is readable only from the manifest.** Cargo exposes
/// `[dev-dependencies]` to test targets, so a `rustls` that had slipped back
/// into that section would still compile and run *here* while the shipped
/// binary had none — no runtime check in this binary can see the difference.
/// Read from the manifest rather than from `cargo tree`'s output on purpose:
/// the manifest is declarative and order-independent, whereas a scrape of
/// output a tool formatted for human reading has to be re-verified every time
/// that formatting changes, and counting lines in it is how a query says
/// `<none>` when it means `rc=1`.
///
/// **The provider count is NOT readable from that manifest line**, which is
/// why this test used to assert a falsehood. It asserted that the line does
/// not mention `aws-lc-rs` and called that "the two-provider defect is
/// prevented" — while the crate resolved both providers anyway:
///
/// ```text
/// cargo tree -e features -p acdp-registry-server -i rustls@0.23.45 -f '{p} [{f}]'
/// rustls v0.23.45 [aws-lc-rs,aws_lc_rs,default,log,logging,prefer-post-quantum,ring,std,tls12]
/// ```
///
/// Neither provider has a single enabler, which is the deeper reason a
/// one-line scan cannot answer this. `aws_lc_rs` has **two** independent
/// sources: `axum-server`'s `tls-rustls = ["tls-rustls-no-provider",
/// "rustls/aws-lc-rs"]`, declared in `axum-server-0.8.0/Cargo.toml` and merely
/// *switched on* by root `Cargo.toml:35`; and this crate's own line, which
/// omits `default-features = false`, so rustls' `default` set — which contains
/// `aws_lc_rs` — applies. `ring` has **five**: this crate's direct dependency,
/// `reqwest` (`__rustls-ring`), `hyper-rustls`, `tokio-rustls` and `sqlx-core`.
///
/// Exactly one of those seven edges is printed on the line the scan reads —
/// `features = ["ring"]` — and it is the one that needs no outside knowledge.
/// The edge that decides this test's subject is the opposite kind: `aws_lc_rs`
/// arrives through an *absence*, the missing `default-features = false`, and
/// seeing it requires knowing rustls' own default set, which the line does not
/// carry. A scan can read a token; it cannot read a token that is not there
/// and know what its absence enables. So the property is asserted where it is
/// actually observable: from the consequence, below.
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
        "this crate must REQUEST `ring`, which is the recorded choice (D-W5-105 — the \
         reasoning is inline at `crates/acdp-registry-server/Cargo.toml:41-43`, since \
         that decision id resolves nowhere else in this repo). Note \
         what this does and does not say: it asserts the request, not the resolution — \
         the graph resolves `ring` AND `aws_lc_rs`, deliberately and permanently, which \
         is what the next assertion is about. A line naming both would satisfy this one. \
         Manifest line: {rustls_line}"
    );
    // Assert the CONSEQUENCE of the resolved feature set, which this process
    // can observe, rather than a spelling on a manifest line, which it cannot.
    //
    // **State precisely what the panic proves, because it is not "exactly two
    // providers".** `ClientConfig::builder()` panics with the message below
    // whenever `CryptoProvider::from_crate_features()` returns `None`
    // (`rustls-0.23.45/src/crypto/mod.rs:249`), and that function's own
    // documentation (`:259-263`) gives THREE such states: the features name two
    // providers, or they name none, or `custom-provider` is enabled. A green
    // here is consistent with all three.
    //
    // That is still exactly the invariant worth asserting, because all three
    // mean the same operational thing: **rustls cannot pick a provider on its
    // own, so `main` must install one explicitly.** That — not the provider
    // count — is why `install_crypto_provider()` exists in `main.rs:99-108`.
    // The assertion below is deliberately written to that claim and no wider.
    //
    // The "none" reading is ruled out separately, one line above the panic
    // check: naming `rustls::crypto::ring::default_provider` fails to COMPILE
    // if the `ring` feature is gone, which is the legible failure for a
    // provider this crate requests directly. (Naming `aws_lc_rs` the same way
    // would be wrong: its absence is a desirable end state, so a compile error
    // is the wrong failure shape.) `custom-provider` is left unruled-out; it
    // would present as the `Some(_)` branch of the message below, or as its
    // branch (b).
    //
    // It is an invariant rather than a snapshot: the decision to keep both
    // providers is recorded in `docs/ENGINEERING-LOG.md` under U-560.
    //
    // Soundness precondition, and it is a property of this file: nothing in
    // this test binary installs a process default. There are no `mod`
    // declarations here, so no sibling test module is compiled in, and the
    // spawn test's probe builds its client with `builder_with_provider`, which
    // installs nothing. Adding `mod didweb;` would break that — it is branch
    // (a) of the message below.
    //
    // The payload is bound rather than discarded because the causes of a red
    // are distinguished by *what was observed*: no panic at all versus a panic
    // with different words. A message that cannot print the payload would
    // pre-diagnose one cause for both.
    //
    // Control for the "no providers at all" reading of the panic below: this
    // is a compile-time reference, so it costs nothing at runtime and fails
    // loudly at build time if `ring` stops resolving.
    let _ring_is_compiled_in: fn() -> rustls::crypto::CryptoProvider =
        rustls::crypto::ring::default_provider;

    let caught = std::panic::catch_unwind(rustls::ClientConfig::builder);
    let payload: Option<String> = caught.err().map(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "<panic payload was neither String nor &str>".to_string())
    });
    assert!(
        payload.as_deref().is_some_and(|m| {
            m.contains("Could not automatically determine the process-level CryptoProvider")
        }),
        "`rustls::ClientConfig::builder()` did not panic with the process-level \
         CryptoProvider message, so this process CAN now pick a crypto provider \
         unaided. Observed panic payload: {payload:?}\n\
         \n\
         A red here is not necessarily breakage. Read the payload first — it \
         splits the causes, and they are not ranked, because nothing here \
         measures which is likelier:\n\
         \n\
         Payload `None` (nothing panicked) means either:\n\
         (a) something in THIS test binary now installs a process-level default. \
         Adding `mod didweb;` is how that happens — `tests/didweb/mod.rs:228-235` \
         installs one under a `Once`. Check this file's `mod` declarations FIRST; \
         it is the one cause that says nothing about the dependency graph.\n\
         (b) the graph now resolves exactly one provider, so rustls can choose. \
         This needs BOTH of `aws_lc_rs`'s enablers to be gone at once — see the \
         two-enabler paragraph in this function's docstring. Neither alone does \
         it: drop `axum-server`'s explicit `rustls/aws-lc-rs` and this crate's \
         own line still inherits `aws_lc_rs` through rustls' `default`; change \
         rustls' `default` and `axum-server`'s explicit edge survives, since \
         `axum-server` sets `default-features = false` on rustls and that edge \
         is its only contribution. So do not stop at checking one of them, and \
         do not assume a decision was revisited — go and look. If one provider \
         really does resolve, `install_crypto_provider()` at `main.rs:99-108` \
         may no longer be required, and the right response is to RE-EVALUATE \
         that call against the U-560 entry in `docs/ENGINEERING-LOG.md`.\n\
         \n\
         Payload `Some(_)` with different text means rustls reworded this panic, \
         or panicked for another reason. The invariant is intact; update the \
         substring this assertion looks for, and check the new text is still \
         about provider selection.\n\
         \n\
         In none of these cases is deleting this test the fix: it is the only \
         executable record of why that install call exists. Manifest line, for \
         reference (it cannot show the resolved set): {rustls_line}"
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
/// **Compiled only for the backends this test can actually drive**, rather
/// than skipped at runtime. A `#[cfg]` leaves no skip branch that could
/// quietly swallow a real failure; a runtime `return` would.
///
/// The excluded builds, and why each is excluded rather than fixed:
/// - `storage-pg` needs a live database to get past storage init, and CI's
///   `postgres` job runs only `--test pg_integration`, so this would never
///   execute there.
/// - **no storage backend at all** (`--no-default-features`, a real CI
///   configuration) exits 1 at startup with `no storage backend feature
///   enabled` before reaching any TLS work — measured in U-532.
///
/// Stated as a positive list on purpose. The first version said
/// `not(feature = "storage-pg")`, which silently included the no-backend
/// build and failed `clippy (no storage backend)` with `cannot find value
/// backend` — an exclusion list is only correct for the variants its author
/// happened to enumerate.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
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
    //
    // **Files, not pipes.** A pipe couples this test to how much the child says.
    // That coupling was documented as safe because "only one request is ever
    // issued", and measured, that is exactly right -- the margin is just far
    // thinner than the note implied. With `RUST_LOG=trace`, which the child
    // inherits because this test sets only `ACDP_REGISTRY_CONFIG`, and a pipe
    // nobody reads, the child stops responding without exiting, and the probe
    // hits its 5s read timeout. Measured, on a pipe macOS grows to 64 KiB
    // (65,536 B exactly -- the unread pipe was observed stopping there):
    // startup alone is ~35.6 KB, and one probe by this test's own rustls
    // client adds ~11.1 KB (startup-plus-one-probe measured directly at
    // 46,708 / 46,710 / 46,743 B). Per-request cost is constant to within a
    // couple of bytes across eight requests, so the ~29.9 KB left after
    // startup holds **two** probes and wedges on the **third**. Startup itself
    // never wedges; it is served requests that do.
    //
    // This test issues at most ONE request -- only `Connect`/`Handshake` probe
    // failures retry, and neither issues HTTP -- so it passes with a margin of
    // exactly one spare request. That is a real bound, not luck, but it is a
    // bound held in place by the retry policy in a different function, and
    // nothing tells whoever relaxes that policy what it costs. Files remove
    // the dependence entirely.
    //
    // A file has no ceiling and never blocks the writer, so request count,
    // retry policy and `RUST_LOG` all stop mattering at once -- the class goes,
    // not the instance. The tempdir these live in is alive for the whole test
    // (`dir`, above), and the parent can read them at any point, whether or not
    // the child has exited. A draining reader thread was rejected: more
    // machinery, still bounded, and it would leave the retry policy and the
    // stdout decision coupled.
    let stdout_path = dir.path().join("registry.stdout");
    let stderr_path = dir.path().join("registry.stderr");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_acdp-registry"))
        .env("ACDP_REGISTRY_CONFIG", &cfg_path)
        .stdout(std::process::Stdio::from(
            std::fs::File::create(&stdout_path).unwrap_or_else(|e| {
                panic!(
                    "create the child's stdout file at {}: {e}",
                    stdout_path.display()
                )
            }),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create(&stderr_path).unwrap_or_else(|e| {
                panic!(
                    "create the child's stderr file at {}: {e}",
                    stderr_path.display()
                )
            }),
        ))
        .spawn()
        .expect("spawn the registry binary");

    // Poll rather than sleep a fixed interval: the failure mode being guarded
    // against is an EARLY exit, so check for it on every attempt instead of
    // racing the unwind. (A single 2s liveness check reported "running" while
    // the process was mid-panic during the investigation for this fix.)
    let (mut outcome, mut last_err) = (None::<TlsProbe>, None::<ProbeError>);
    for _ in 0..50 {
        if let Some(status) = child.try_wait().expect("try_wait") {
            // The child has exited, so both files are complete -- no flush or
            // ordering hazard on this path. `read` + `from_utf8_lossy` rather
            // than `read_to_string`, to preserve the old UTF-8 handling exactly:
            // `read_to_string` would ERROR on invalid UTF-8 where the pipe
            // version silently replaced it, and turning a diagnostic path into
            // a new failure mode is the wrong trade. The error path is
            // deliberately NOT identical: `wait_with_output()` panicked, while
            // `ChildStream::read` substitutes a `<could not read ...>` marker,
            // because losing the whole diagnostic to an unreadable file is the
            // worse outcome on a path that only runs when something is already
            // wrong.
            let stdout = ChildStream::read(&stdout_path);
            let stderr = ChildStream::read(&stderr_path);
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
        match tls_probe(port, cert.cert.der()).await {
            Ok(probe) => {
                outcome = Some(probe);
                break;
            }
            // Only a connect/handshake failure is worth retrying: it means the
            // listener is not up YET. An exchange failure means TLS already
            // worked, so retrying cannot help, and it wastes five seconds.
            //
            // That is the whole justification now, and it is enough. This rule
            // was twice documented as ALSO protecting the child's undrained
            // stdout pipe -- first as its purpose, then as a correction that
            // still discussed pipe pressure. There is no pipe to protect: the
            // child writes to files (see the spawn above), so nothing about
            // this rule depends on how much it says.
            Err(err) if err.retryable => last_err = Some(err),
            Err(err) => {
                last_err = Some(err);
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let still_running = child.try_wait().expect("try_wait").is_none();
    let _ = child.kill();
    let _ = child.wait();

    // Read the child's own output for the assertion below. Until this unit, the
    // probe-failure path -- the failure a reader actually MEETS -- carried none
    // of it, while the early-exit path carried all of it.
    //
    // Those two paths are not the alternatives they look like. The loop above
    // breaks on a NON-RETRYABLE probe error, and a timeout is non-retryable
    // (`ProbeError::io`: `retryable: stage.io_failure_may_be_transient() &&
    // !timed_out`). So the early-exit branch can be skipped EVEN THOUGH the
    // child has already exited: the first probe times out, the loop breaks
    // before its next `try_wait`, and nothing ever observes the exit.
    //
    // Measured, and the distinction matters more than it looks. A listener
    // holding 18443 that does NOT accept makes the child exit in 0.06s with
    // `Address already in use`, and the test lands here, 3 runs of 3. A holder
    // that DOES accept and then closes is a different story: the peer close is
    // `UnexpectedEof`, which is retryable, so the loop survives to its next
    // `try_wait` and the early-exit branch fires normally.
    //
    // So the reachable-ness turns on whether the holder accepts, NOT on "port
    // in use" as a cause -- an earlier version of this comment said the latter
    // and was too wide, since the likelier real holder is a server, which
    // accepts. What is true is narrower and still worth the code below: a probe
    // failure can arrive with the child already dead and its output unread.
    //
    // Read AFTER the kill so the files are as complete as they will get. The
    // child was still alive a moment ago on the `still_running` path, so its
    // last line may be torn -- said plainly below rather than left for a reader
    // to discover by disbelieving the output.
    let child_stdout = ChildStream::read(&stdout_path);
    let child_stderr = ChildStream::read(&stderr_path);

    // The claim and the observation are now the same thing. This message used to
    // assert a TLS connection had been established while the loop above only
    // completed a bare TCP connect, so a rustls handshake regression left it
    // green -- anything that bound the port satisfied it (U-556).
    assert!(
        outcome.is_some() && still_running,
        "the TLS probe never succeeded against 127.0.0.1:{port} \
         (probe_ok={}, still_running={still_running}). The stage tag below says which \
         step failed, and all five are covered: `connect` means it never bound the port, \
         `handshake` and `exchange` indict the registry, while `config` and `task` are \
         defects in this test itself. Last probe error: {}\n\
         {}{}\n\
         --- child stdout ---\n{child_stdout}\n\
         --- child stderr ---\n{child_stderr}",
        outcome.is_some(),
        last_err
            .as_ref()
            .map(ProbeError::describe)
            .unwrap_or_else(|| "none recorded".to_string()),
        if still_running {
            concat!(
                "The child was STILL RUNNING when the probe loop gave up and was killed just ",
                "before this read, so its output may be incomplete and its last line may be ",
                "torn. Absence of a line below is not proof the registry never logged it.",
            )
        } else {
            concat!(
                "The child had already exited when this was read, so the files hold everything ",
                "it wrote.",
            )
        },
        if child_stdout.is_partial() || child_stderr.is_partial() {
            concat!(
                " NOTE: at least one stream below is NOT shown in full -- it was truncated or ",
                "could not be read. Each such stream says so at its own end; do not read the ",
                "sentence above as a claim about what this message contains.",
            )
        } else {
            ""
        },
    );

    let probe = outcome.expect("asserted present immediately above");

    // Defence in depth, and deliberately kept even though the client is pinned to
    // 1.3 at `with_protocol_versions` below -- which means a SERVER that lost 1.3
    // fails the handshake and is caught by the assertion above, never by this one.
    // What this guards is a future weakening of that pin: the moment the client
    // accepts 1.2, "the handshake succeeded" stops implying "over TLS 1.3", and
    // this line is what still notices. Measured: with a 1.2-only client the
    // handshake completes and only this assertion fails.
    assert_eq!(
        probe.version,
        Some(rustls::ProtocolVersion::TLSv1_3),
        "handshake completed but not over TLS 1.3 (negotiated {:?}, suite {:?})",
        probe.version,
        probe.suite,
    );
    assert_eq!(
        probe.alpn.as_deref(),
        Some("http/1.1"),
        "the server did not negotiate the ALPN protocol we offered. `None` here means it \
         advertised no ALPN at all -- which silently breaks HTTP/2 for every real client, \
         and this is the only place in the repo that observes the server's ALPN. Do not \
         delete this assertion for looking unfireable. Note what it does NOT cover: if the \
         server's list narrowed from [h2, http/1.1] to [http/1.1], h2 is lost and this still \
         passes -- catching that needs a second probe offering h2, which is out of scope for \
         a TLS startup test"
    );
    assert_eq!(
        probe.status, 200,
        "GET {PROBE_PATH} over TLS returned {}, body: {:?}",
        probe.status, probe.body
    );

    let doc: serde_json::Value = serde_json::from_str(&probe.body)
        .unwrap_or_else(|e| panic!("GET {PROBE_PATH} body was not JSON ({e}): {:?}", probe.body));
    assert_eq!(
        doc["status"], "ok",
        "GET {PROBE_PATH} over TLS did not report healthy: {:?}",
        probe.body
    );
}

/// The endpoint the TLS probe requests. Named once so the request line and every
/// failure message that mentions it cannot drift apart -- a message naming a path
/// the code no longer requests diagnoses the wrong thing.
///
/// **`/livez` specifically, and do not "strengthen" this to a richer endpoint.**
/// This test's subject is the transport: does the shipped binary install a provider
/// and complete a real TLS exchange. `livez()` takes no `State`, so it answers 200 on
/// a cold, empty store under every storage arm. `/healthz` has a 503 arm and would
/// import a storage-init failure into a test whose red state must mean "TLS broke" --
/// the exact confusion the branch-on-exit-code work above exists to remove. The
/// endpoints' own behaviour is already covered in-process over plain HTTP by
/// `http_integration.rs`; nothing is gained here by duplicating it. Parsing the body
/// as JSON is the transport property worth having: it proves the bytes survived the
/// TLS record layer intact.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
const PROBE_PATH: &str = "/livez";

/// How much of each child stream may enter a panic message.
///
/// A file has no ceiling, which is the point of writing to one -- but a panic
/// message still has a reader.
///
/// **The path that actually overflows this is the retry loop, not the request
/// this test serves.** A successful run leaves the child at ~46.7 KB at
/// `RUST_LOG=trace`, which never reaches the cap. A run whose handshake keeps
/// failing retries 50 times, and each failed handshake costs the child several
/// KB: measured 343,032 B of stdout for one such run at trace. That is the
/// case someone meets while already debugging, and it is the reason this
/// constant exists.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
const CHILD_OUTPUT_CAP: usize = 64 * 1024;

/// One of the child's output streams, read for inclusion in a panic message.
///
/// This is an enum rather than a `String` so the message can say **which of
/// these three things it is showing**. A caller holding only a `String` has to
/// assert something about it, and the only sentence available was "complete" --
/// which is false for a truncated or unreadable stream, and was being printed
/// above the truncation marker that contradicted it.
///
/// Reading never panics, and that is the point: this is only ever called from
/// inside a failure path, so a second failure here would replace the diagnosis
/// with an unrelated one.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
enum ChildStream {
    Complete(String),
    /// `total` is the size of the file; the rendered head may be a byte or two
    /// longer, because a multi-byte character cut by the cap becomes U+FFFD.
    /// The disclosure therefore talks about bytes the CHILD WROTE, which is the
    /// quantity a reader can act on, not about the length of the rendered text.
    Truncated {
        head: String,
        total: usize,
    },
    Unreadable(String),
}

#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
impl ChildStream {
    /// `read` + `from_utf8_lossy` rather than `read_to_string`, deliberately.
    /// `read_to_string` errors on invalid UTF-8; the pipe-based version this
    /// replaced used `from_utf8_lossy` and silently substituted replacement
    /// characters. The binary emits JSON logs so the case is remote, but a
    /// diagnostic path is the wrong place to introduce a new way to fail.
    fn read(path: &std::path::Path) -> Self {
        match std::fs::read(path) {
            Ok(raw) if raw.len() > CHILD_OUTPUT_CAP => Self::Truncated {
                // `from_utf8_lossy` absorbs a multi-byte character cut in half
                // by the slice, so the cap needs no char-boundary search.
                head: String::from_utf8_lossy(&raw[..CHILD_OUTPUT_CAP]).into_owned(),
                total: raw.len(),
            },
            Ok(raw) => Self::Complete(String::from_utf8_lossy(&raw).into_owned()),
            Err(err) => Self::Unreadable(format!("<could not read {}: {err}>", path.display())),
        }
    }

    /// True when this stream is not shown in full, for any reason.
    fn is_partial(&self) -> bool {
        !matches!(self, Self::Complete(_))
    }
}

/// **The cap keeps the HEAD, not the tail, and says so.**
///
/// This is primarily a *startup* diagnostic, so the signal is usually the first
/// thing the binary said. **That is not universally true and the cost is worth
/// naming:** on the early-exit path the interesting line is the *last* one
/// before the process died, and a head-keep cap would drop it. It does not in
/// practice, because the fatal line goes to **stderr**, which is a few hundred
/// bytes even at `RUST_LOG=trace` (measured: 200 B for a forced early exit,
/// 0 B when the child does not die) and so is never capped. One helper with one
/// behaviour is still right -- two reads of the same files truncating
/// differently is the inconsistency this unit exists to remove -- but it is a
/// trade, not a free win.
///
/// An undisclosed cut is the same defect class this unit exists to remove, and
/// the tempdir is deleted when the test returns, so a "full log at <path>"
/// pointer would dangle by the time anyone read it. The counts go in the
/// message instead.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
impl std::fmt::Display for ChildStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Complete(text) | Self::Unreadable(text) => f.write_str(text),
            Self::Truncated { head, total } => write!(
                f,
                "{head}\n[truncated: this is the first {CHILD_OUTPUT_CAP} of {total} bytes the \
                 child wrote. The tail is dropped, not the head, because the signal in a startup \
                 diagnostic is usually the first thing the binary said.]"
            ),
        }
    }
}

/// The name the client verifies the certificate against. Must match the SAN the
/// test's `rcgen` cert is generated for; the socket still connects to 127.0.0.1.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
const SERVER_NAME: &str = "localhost";

/// What a successful TLS probe observed. Every field is something the old test
/// claimed in its failure message and never actually looked at.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
struct TlsProbe {
    version: Option<rustls::ProtocolVersion>,
    alpn: Option<String>,
    /// Captured for failure messages, deliberately never asserted. An "is it a TLS 1.3
    /// AEAD suite" floor would be **unfireable**: the 1.3 registry is AEAD-only by
    /// construction and the client is pinned to 1.3 below, so such a check cannot fail on
    /// any run where the version assertion passes. It would read as coverage and prove
    /// nothing.
    suite: Option<rustls::CipherSuite>,
    status: u16,
    body: String,
}

/// Which step of the probe failed.
///
/// An enum rather than a `&'static str` on purpose. The retry rule below decides
/// whether a failed probe is worth another pass, and a stringly-typed stage would
/// let a single typo at any call site (`"Exchange"`, `"exchg"`) silently flip an
/// exchange error to retryable, with neither the compiler nor the suite noticing.
/// Spelled this way, it is unspellable.
///
/// This docstring used to add that the rule "is what keeps *only one HTTP request
/// is ever issued* true, which in turn is why this test does not need to drain the
/// child's stdout". The second half is gone: the child's output goes to files, so
/// draining is not a thing this test does or needs. The first half was true and is
/// simply no longer load-bearing for anything.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Building the client. Always a defect in this test, never in the registry.
    Config,
    /// Opening the TCP connection.
    Connect,
    /// The TLS handshake itself.
    Handshake,
    /// The HTTP request/response over an established TLS connection.
    Exchange,
    /// The blocking probe task panicked.
    Task,
}

#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Connect => "connect",
            Self::Handshake => "handshake",
            Self::Exchange => "exchange",
            Self::Task => "task",
        }
    }

    /// Only these two stages can mean "the listener is not up YET", which is the
    /// only thing another pass of the poll loop could fix. An exchange failure
    /// means TLS already worked, so retrying cannot help.
    ///
    /// Nothing here has anything to do with the child's stdout any more. Two
    /// earlier versions of this comment said it did -- one claiming this rule
    /// protected an undrained pipe, one correcting that while still arguing
    /// about pipe capacity. The file-backed stdio it called a follow-up unit is
    /// the spawn above, so the question is closed rather than re-answered: a
    /// file has no ceiling and never blocks its writer.
    fn io_failure_may_be_transient(self) -> bool {
        matches!(self, Self::Connect | Self::Handshake)
    }
}

/// A probe failure, tagged with the stage it failed at.
///
/// The stage is not decoration. It decides whether the poll loop retries, and it
/// keeps the final assertion honest: reporting an HTTP-exchange error under a
/// message that says "handshake never completed" would be a smaller copy of the
/// exact defect this test was widened to fix.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
struct ProbeError {
    stage: Stage,
    detail: String,
    retryable: bool,
}

#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
impl ProbeError {
    fn describe(&self) -> String {
        format!("[{}] {}", self.stage.label(), self.detail)
    }

    /// A failure that another pass of the loop cannot possibly fix.
    fn fatal(stage: Stage, detail: String) -> Self {
        Self {
            stage,
            detail,
            retryable: false,
        }
    }

    /// A refused/reset connection means "not up yet" and is worth another pass.
    /// A **timeout** is not: it means something IS listening on the port and is
    /// not completing a TLS handshake, which retrying 50 times cannot fix and
    /// which would otherwise stall the loop for minutes.
    fn io(stage: Stage, err: &std::io::Error) -> Self {
        let timed_out = matches!(
            err.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        );
        Self {
            stage,
            detail: format!("{err} (kind: {:?})", err.kind()),
            retryable: stage.io_failure_may_be_transient() && !timed_out,
        }
    }
}

/// Complete a real TLS 1.3 handshake against the spawned binary and read an
/// HTTPS response body.
///
/// Runs on the blocking pool because rustls' `Stream` is synchronous, and
/// **every timeout is set on the std socket inside that closure on purpose**:
/// `tokio::time::timeout` would abandon this task rather than cancel it, and
/// dropping the test's runtime waits for blocking tasks that already started —
/// so a hung handshake would hang the whole test process regardless.
#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
async fn tls_probe(
    port: u16,
    leaf: &rustls::pki_types::CertificateDer<'static>,
) -> Result<TlsProbe, ProbeError> {
    let leaf = leaf.clone();
    match tokio::task::spawn_blocking(move || tls_probe_blocking(port, leaf)).await {
        Ok(result) => result,
        Err(join) => Err(ProbeError::fatal(
            Stage::Task,
            format!("the probe task panicked: {join}"),
        )),
    }
}

#[cfg(any(feature = "storage-sqlite", feature = "storage-memory"))]
fn tls_probe_blocking(
    port: u16,
    leaf: rustls::pki_types::CertificateDer<'static>,
) -> Result<TlsProbe, ProbeError> {
    use std::io::{Read as _, Write as _};

    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
    const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    // Pin the server's own generated certificate as the sole trust anchor rather
    // than accepting any certificate. `generate_simple_self_signed` emits
    // `IsCa::NoCa` and no basicConstraints extension, so webpki accepts the leaf
    // as its own anchor and full chain + hostname verification stay ON.
    let mut roots = rustls::RootCertStore::empty();
    roots.add(leaf).map_err(|e| {
        ProbeError::fatal(
            Stage::Config,
            format!("pinning the generated certificate failed: {e}"),
        )
    })?;

    // `builder_with_provider`, NOT `builder()`. This binary's graph enables both
    // `ring` and `aws_lc_rs`, so the convenience constructor panics with the very
    // "could not automatically determine the process-level CryptoProvider"
    // message this whole test file exists because of.
    let mut config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|e| {
        ProbeError::fatal(
            Stage::Config,
            format!("restricting the client to TLS 1.3 failed: {e}"),
        )
    })?
    .with_root_certificates(roots)
    .with_no_client_auth();
    // The server advertises h2 first, so offering nothing here would risk
    // negotiating h2 and make the HTTP/1.1 request line below meaningless.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];

    let name = rustls::pki_types::ServerName::try_from(SERVER_NAME).map_err(|e| {
        ProbeError::fatal(
            Stage::Config,
            format!("{SERVER_NAME:?} is not a valid server name: {e}"),
        )
    })?;
    let mut conn =
        rustls::ClientConnection::new(std::sync::Arc::new(config), name).map_err(|e| {
            ProbeError::fatal(
                Stage::Config,
                format!("building the client connection failed: {e}"),
            )
        })?;

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut sock = std::net::TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| ProbeError::io(Stage::Connect, &e))?;
    sock.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| ProbeError::io(Stage::Connect, &e))?;
    sock.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| ProbeError::io(Stage::Connect, &e))?;

    // Drive the handshake to completion on its own so a handshake failure is
    // reported as one, rather than surfacing later as a confusing write error.
    conn.complete_io(&mut sock)
        .map_err(|e| ProbeError::io(Stage::Handshake, &e))?;

    let version = conn.protocol_version();
    let alpn = conn
        .alpn_protocol()
        .map(|p| String::from_utf8_lossy(p).into_owned());
    let suite = conn.negotiated_cipher_suite().map(|s| s.suite());

    // `Connection: close` is required, not stylistic: hyper keeps the connection
    // alive otherwise, and the server's 30s TimeoutLayer is a REQUEST timeout,
    // not an idle-connection one, so `read_to_end` would block until IO_TIMEOUT
    // on every single run.
    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
    tls.write_all(
        format!("GET {PROBE_PATH} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .map_err(|e| ProbeError::io(Stage::Exchange, &e))?;
    tls.flush()
        .map_err(|e| ProbeError::io(Stage::Exchange, &e))?;

    // Bounded on purpose. `read_to_end` straight onto a socket grows without any
    // limit if the peer misbehaves; /livez is ~50 bytes, so 64 KiB is three orders
    // of magnitude of headroom. Overflow is REPORTED, not truncated -- a silent
    // truncation would surface later as a baffling JSON parse error.
    const MAX_RESPONSE: u64 = 64 * 1024;
    let mut raw = Vec::new();
    std::io::Read::take(&mut tls, MAX_RESPONSE + 1)
        .read_to_end(&mut raw)
        .map_err(|e| ProbeError::io(Stage::Exchange, &e))?;
    if raw.len() as u64 > MAX_RESPONSE {
        return Err(ProbeError::fatal(
            Stage::Exchange,
            format!("the HTTPS response exceeded {MAX_RESPONSE} bytes"),
        ));
    }
    let text = String::from_utf8_lossy(&raw).into_owned();

    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| {
        ProbeError::fatal(
            Stage::Exchange,
            format!("no header/body separator in the HTTPS response: {text:?}"),
        )
    })?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| {
            ProbeError::fatal(
                Stage::Exchange,
                format!("could not parse a status line from: {head:?}"),
            )
        })?;

    Ok(TlsProbe {
        version,
        alpn,
        suite,
        status,
        body: body.to_string(),
    })
}
