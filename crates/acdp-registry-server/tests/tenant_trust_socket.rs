//! #391: one real-socket, end-to-end test of
//! `auth.tenant_header_trust = "trusted_proxies"`.
//!
//! Every #374 test in `http_integration.rs` drives the router with `oneshot`
//! and injects `ConnectInfo<SocketAddr>` by hand, so none of them proves that
//! the shipped binary actually records the TCP peer: if `serve_with_store`
//! stopped using `into_make_service_with_connect_info`, every request would
//! carry no peer, `trusted_proxies` would trust nobody, and those tests would
//! still pass. This spawns the real binary (the `lifecycle_refuse_start.rs`
//! pattern, including its files-not-pipes rule for the child's stdio) and
//! talks to it over a loopback socket, so the peer the trust test sees is
//! `127.0.0.1` because that is where the connection came from.
//!
//! Three runs:
//! 1. `trusted_proxies = ["127.0.0.1"]` — the header is honoured: a publish
//!    lands in `tenant-a`, readable under `tenant-a` and not under `tenant-b`.
//! 2. a list that does not contain the peer — the same header is refused with
//!    `403 not_authorized`.
//! 3. an empty list — startup refuses the config (no peer could ever be
//!    trusted), so the "empty list" case never reaches a request.
#![cfg(feature = "storage-sqlite")]

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use acdp::crypto::SigningKey;
use acdp::producer::Producer;
use acdp::types::primitives::{ContextType, Visibility};
use serde_json::Value;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// A lax, auth-off SQLite registry on loopback whose tenant header is
/// trusted only from `proxies` (a TOML array literal).
fn write_config(dir: &Path, port: u16, proxies: &str) -> PathBuf {
    let path = dir.join("registry.toml");
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
tenant_header_trust = "trusted_proxies"

[rate_limit]
trusted_proxies = {proxies}
"#,
            dir.join("registry.sqlite").display()
        ),
    )
    .expect("write config");
    path
}

/// One spawned registry. Stdio goes to files, never pipes: an undrained pipe
/// wedges a chatty child (see `tls_startup.rs`).
struct Registry {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Registry {
    fn spawn(dir: &Path, cfg: &Path) -> Self {
        let stdout = dir.join("registry.stdout");
        let stderr = dir.join("registry.stderr");
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

    fn wait_ready(&mut self, port: u16) {
        for _ in 0..150 {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                panic!(
                    "registry exited during startup with {status}\n{}",
                    self.output()
                );
            }
            if let Ok((200, _)) = http(port, "GET", "/livez", None, None) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        panic!("registry never became ready\n{}", self.output());
    }

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
}

impl Drop for Registry {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Minimal HTTP/1.1 over a plain loopback socket (`Connection: close`, read
/// to EOF), with an optional `X-Tenant-Id`.
fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&Value>,
    tenant: Option<&str>,
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
    if let Some(t) = tenant {
        req.push_str(&format!("X-Tenant-Id: {t}\r\n"));
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

fn publish_body(seed: u8, title: &str) -> Value {
    let producer = Producer::new_did_key(SigningKey::from_bytes(&[seed; 32]));
    let req = producer
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .expect("publish request");
    serde_json::to_value(&req).unwrap()
}

#[test]
fn a_listed_loopback_peer_is_trusted_and_an_unlisted_one_is_refused() {
    // ── 1. The peer (127.0.0.1) is the declared gateway: header honoured. ──
    let dir = tempfile::tempdir().expect("tempdir");
    let port = free_port();
    let cfg = write_config(dir.path(), port, r#"["127.0.0.1"]"#);
    let mut reg = Registry::spawn(dir.path(), &cfg);
    reg.wait_ready(port);

    let (status, v) = http(
        port,
        "POST",
        "/contexts",
        Some(&publish_body(91, "socket trusted")),
        Some("tenant-a"),
    )
    .expect("publish");
    assert_eq!(status, 201, "publish: {v}\n{}", reg.output());
    let ctx_id = v["ctx_id"].as_str().expect("ctx_id").to_string();
    let path = format!("/contexts/{}", pct(&ctx_id));
    let (status, v) = http(port, "GET", &path, None, Some("tenant-a")).expect("get a");
    assert_eq!(status, 200, "the owning tenant reads it: {v}");
    let (status, v) = http(port, "GET", &path, None, Some("tenant-b")).expect("get b");
    assert_eq!(
        status, 404,
        "the header really selected the tenant: another one is withheld: {v}"
    );
    drop(reg);

    // ── 2. The peer is NOT in the list: the same header is refused. ───────
    let dir = tempfile::tempdir().expect("tempdir");
    let port = free_port();
    let cfg = write_config(dir.path(), port, r#"["10.0.0.0/8"]"#);
    let mut reg = Registry::spawn(dir.path(), &cfg);
    reg.wait_ready(port);
    let (status, v) = http(
        port,
        "POST",
        "/contexts",
        Some(&publish_body(92, "socket untrusted")),
        Some("tenant-a"),
    )
    .expect("publish");
    assert_eq!(status, 403, "untrusted peer: {v}\n{}", reg.output());
    assert_eq!(v["error"]["code"], "not_authorized", "{v}");
    let msg = v["error"]["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("not trusted from this peer") && msg.contains("trusted_proxies"),
        "{v}"
    );
    // Without the header the same registry serves the request.
    let (status, v) = http(
        port,
        "POST",
        "/contexts",
        Some(&publish_body(92, "socket untrusted")),
        None,
    )
    .expect("publish");
    assert_eq!(status, 201, "no header, no refusal: {v}");
    drop(reg);

    // ── 3. An empty list cannot trust anyone: refused at startup. ─────────
    let dir = tempfile::tempdir().expect("tempdir");
    let port = free_port();
    let cfg = write_config(dir.path(), port, "[]");
    let mut reg = Registry::spawn(dir.path(), &cfg);
    let status = reg
        .wait_exit()
        .unwrap_or_else(|| panic!("an empty list must refuse to start\n{}", reg.output()));
    let out = reg.output();
    assert!(!status.success(), "must exit non-zero, got {status}\n{out}");
    assert!(
        out.contains("rate_limit.trusted_proxies is empty"),
        "the refusal must name the empty list\n{out}"
    );
}
