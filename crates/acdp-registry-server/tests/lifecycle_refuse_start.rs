//! #373: a registry restarted with `[lifecycle] enabled = false` over a
//! database that holds lifecycle state refuses to start, and restarting with
//! the flag back on serves that state exactly as before.
//!
//! RFC-ACDP-0013 §6: a registry that does not advertise
//! `acdp-registry-lifecycle` MUST NOT emit `lifecycle_events` or the
//! `retracted` status. The read paths project stored lifecycle state
//! unconditionally, so before #373 turning the flag off after a retraction
//! kept serving it. The startup gate (`lifecycle_state_gate` in `main.rs`)
//! makes that combination unbootable instead.
//!
//! ## Why this spawns the real binary
//!
//! The property is about a RESTART: state written by one process, refused by
//! the next one. An in-process router test cannot see it, because the gate
//! runs in `serve_with_store`, which binds a socket and never returns. So
//! this follows `tls_startup.rs`'s spawn pattern (`CARGO_BIN_EXE_acdp-registry`)
//! — including its **files, not pipes** rule for the child's stdio (read the
//! note above that test's spawn: an undrained pipe wedges a chatty child after
//! ~64 KiB on macOS, and this test issues several requests per child).
//!
//! SQLite only (`storage-sqlite`): the memory backend cannot carry state
//! across a restart (its `has_lifecycle_state` is the trait default,
//! `false`), and Postgres needs a live database the default test step does
//! not provide — its probe is covered in `acdp-registry-pg`'s
//! `store_contract.rs` instead.
#![cfg(feature = "storage-sqlite")]

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use acdp::crypto::SigningKey;
use acdp::producer::Producer;
use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
use serde_json::{json, Value};

const SEED: u8 = 73;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn write_config(dir: &Path, port: u16, lifecycle: bool) -> PathBuf {
    let path = dir.join(format!("registry-{lifecycle}.toml"));
    std::fs::write(
        &path,
        format!(
            r#"
[registry]
authority = "localhost"
port = {port}
bind = "127.0.0.1"

[storage]
backend = "sqlite"
sqlite_path = "{}"

[auth]
did_methods = ["did:web", "did:key"]
anonymous_public_reads = true

[lifecycle]
enabled = {lifecycle}
"#,
            dir.join("registry.sqlite").display()
        ),
    )
    .expect("write config");
    path
}

/// One spawned registry. Stdio goes to files, never pipes (see module docs).
struct Registry {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Registry {
    fn spawn(dir: &Path, cfg: &Path, tag: &str) -> Self {
        let stdout = dir.join(format!("{tag}.stdout"));
        let stderr = dir.join(format!("{tag}.stderr"));
        let child = Command::new(env!("CARGO_BIN_EXE_acdp-registry"))
            .env("ACDP_REGISTRY_CONFIG", cfg)
            .stdout(Stdio::from(
                std::fs::File::create(&stdout).expect("child stdout file"),
            ))
            .stderr(Stdio::from(
                std::fs::File::create(&stderr).expect("child stderr file"),
            ))
            .spawn()
            .expect("spawn the registry binary");
        Self {
            child,
            stdout,
            stderr,
        }
    }

    fn output(&self) -> String {
        let read = |p: &Path| {
            std::fs::read(p)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|e| format!("<could not read {}: {e}>", p.display()))
        };
        format!(
            "--- stdout ---\n{}\n--- stderr ---\n{}",
            read(&self.stdout),
            read(&self.stderr)
        )
    }

    /// Poll `/livez` until it answers, failing fast (with the child's output)
    /// if the process exits first.
    fn wait_ready(&mut self, port: u16) {
        for _ in 0..150 {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                panic!(
                    "registry exited during startup with {status}\n{}",
                    self.output()
                );
            }
            if let Ok((200, _)) = http(port, "GET", "/livez", None) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        panic!("registry never became ready\n{}", self.output());
    }

    /// Wait for the process to exit on its own; kill it after the deadline.
    fn wait_exit(&mut self) -> Option<std::process::ExitStatus> {
        for _ in 0..150 {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        None
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Minimal HTTP/1.1 over a plain socket (`Connection: close`, read to EOF).
/// The workspace has no HTTP client among this crate's dev-dependencies, and
/// one request per connection keeps this trivially correct.
fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> std::io::Result<(u16, Value)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let payload = body
        .map(|b| serde_json::to_vec(b).unwrap())
        .unwrap_or_default();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Length: {}\r\n",
        payload.len()
    );
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes())?;
    s.write_all(&payload)?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw);
    let (head, rest) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other(format!("no header terminator: {text:?}")))?;
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| std::io::Error::other(format!("bad status line: {head:?}")))?;
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(rest)
    } else {
        rest.to_string()
    };
    let v = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or(Value::String(body))
    };
    Ok((status, v))
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn signed_retract(ctx_id: &str) -> Value {
    let key = SigningKey::from_bytes(&[SEED; 32]);
    let did = acdp::did::key::did_key_from_ed25519(&key.verifying_key_bytes());
    let key_id = acdp::did::key::did_key_url(&did).expect("did:key url");
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        CtxId(ctx_id.to_string()),
        LifecycleEventType::Retracted,
        chrono::Utc::now(),
        AgentDid::new(did),
        Some("issue 373 restart test".into()),
    )
    .expect("valid event")
    .sign_with(key, key_id)
    .expect("signed event");
    json!({ "event": serde_json::to_value(&event).unwrap() })
}

#[test]
fn lifecycle_state_with_the_flag_off_refuses_to_start_and_survives_re_enable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let port = free_port();
    let on = write_config(dir.path(), port, true);
    let off = write_config(dir.path(), port, false);

    // ── 1. Flag ON: publish, then retract over HTTP. ──────────────────────
    let mut reg = Registry::spawn(dir.path(), &on, "first-on");
    reg.wait_ready(port);
    let producer = Producer::new_did_key(SigningKey::from_bytes(&[SEED; 32]));
    let req = producer
        .publish_request()
        .title("lc373 restart")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("publish request");
    let (status, v) = http(
        port,
        "POST",
        "/contexts",
        Some(&serde_json::to_value(&req).unwrap()),
    )
    .expect("publish");
    assert_eq!(status, 200, "publish: {v}\n{}", reg.output());
    let ctx_id = v["ctx_id"].as_str().expect("ctx_id").to_string();
    let lineage_id = v["lineage_id"].as_str().expect("lineage_id").to_string();
    let (status, v) = http(
        port,
        "POST",
        &format!("/contexts/{}/retract", pct(&ctx_id)),
        Some(&signed_retract(&ctx_id)),
    )
    .expect("retract");
    assert_eq!(status, 200, "retract: {v}\n{}", reg.output());
    assert_eq!(v["registry_state"]["status"], "retracted");
    reg.stop();

    // ── 2. Flag OFF over the same database: refuse, non-zero, named. ──────
    let mut reg = Registry::spawn(dir.path(), &off, "off");
    let status = reg.wait_exit().unwrap_or_else(|| {
        panic!(
            "with lifecycle state present and [lifecycle] off the registry must exit \
             on its own, but it was still running (and was killed)\n{}",
            reg.output()
        )
    });
    let out = reg.output();
    assert!(!status.success(), "must exit non-zero, got {status}\n{out}");
    for needle in [
        "lifecycle.enabled=false",
        "enabled = true",
        "purge",
        "IRREVERSIBLE",
    ] {
        assert!(
            out.contains(needle),
            "the refusal must name {needle:?}\n{out}"
        );
    }
    assert!(
        out.contains("ERROR") && out.contains("refusing to start"),
        "the refusal must be logged at ERROR before exit\n{out}"
    );
    assert!(
        http(port, "GET", "/livez", None).is_err(),
        "nothing may be listening after the refusal"
    );
    drop(reg);

    // ── 3. Flag back ON: the state is served exactly as before. ───────────
    let mut reg = Registry::spawn(dir.path(), &on, "second-on");
    reg.wait_ready(port);

    let (status, full) =
        http(port, "GET", &format!("/contexts/{}", pct(&ctx_id)), None).expect("get");
    assert_eq!(status, 200, "get: {full}");
    assert_eq!(full["registry_state"]["status"], "retracted");
    assert_eq!(
        full["registry_state"]["lifecycle_events"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "events still attached: {full}"
    );

    let (_, res) = http(
        port,
        "GET",
        "/contexts/search?q=lc373&status=retracted",
        None,
    )
    .expect("search retracted");
    assert_eq!(
        res["matches"].as_array().map(Vec::len),
        Some(1),
        "status=retracted finds it: {res}"
    );
    let (_, res) = http(port, "GET", "/contexts/search?q=lc373", None).expect("search");
    assert_eq!(
        res["matches"].as_array().map(Vec::len),
        Some(0),
        "default search excludes it: {res}"
    );

    let (status, arr) = http(
        port,
        "GET",
        &format!("/lineages/{}", pct(&lineage_id)),
        None,
    )
    .expect("lineage");
    assert_eq!(status, 200, "lineage: {arr}");
    let arr = arr.as_array().expect("lineage array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["registry_state"]["status"], "retracted");
    assert_eq!(
        arr[0]["registry_state"]["lifecycle_events"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let (status, v) = http(
        port,
        "GET",
        &format!("/lineages/{}/current", pct(&lineage_id)),
        None,
    )
    .expect("current");
    assert_eq!(status, 404, "lc-003: a retracted head has no current: {v}");
    reg.stop();
}
