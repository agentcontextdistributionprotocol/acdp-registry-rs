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
    let (mut outcome, mut last_err) = (None::<TlsProbe>, None::<ProbeError>);
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
        match tls_probe(port, cert.cert.der()).await {
            Ok(probe) => {
                outcome = Some(probe);
                break;
            }
            // Only a connect/handshake failure is worth retrying: it means the
            // listener is not up YET. An exchange failure means TLS already
            // worked, so retrying cannot help, and it wastes five seconds.
            //
            // This rule USED to be documented as also protecting the child's
            // undrained stdout pipe. Measured, that justification was wrong: the
            // pipe grows to 64 KiB, and 50 retried requests at the default log
            // filter emit ~40 KB and still pass. The variable that actually fills
            // it is RUST_LOG, which the child inherits -- one probe at `trace`
            // already uses ~20 KB. See the U-556 follow-up note on `tls_probe`.
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
         defects in this test itself. Last probe error: {}",
        outcome.is_some(),
        last_err
            .as_ref()
            .map(ProbeError::describe)
            .unwrap_or_else(|| "none recorded".to_string()),
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
/// An enum rather than a `&'static str` on purpose. The retry rule below is
/// load-bearing -- it is what keeps "only one HTTP request is ever issued" true,
/// which in turn is why this test does not need to drain the child's stdout. A
/// stringly-typed stage would let a single typo at any call site (`"Exchange"`,
/// `"exchg"`) silently flip an exchange error to retryable, and neither the
/// compiler nor the suite would notice. Spelled this way, it is unspellable.
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
    /// **This rule is NOT what protects the child's undrained stdout**, despite an
    /// earlier comment here saying so. Measured: the pipe grows to 64 KiB and 50
    /// retried requests at the default filter emit ~40 KB, well under it. The real
    /// exposure is `RUST_LOG`, inherited by the child -- at `trace` a single probe
    /// uses ~20 KB and 50 would wedge. A `matches!` arm cannot gate that anyway;
    /// the structural fix is file-backed child stdio, which touches the protected
    /// early-exit block and so belongs to a follow-up unit, not this one.
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
