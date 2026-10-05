//! The shipped `acdp-registry` binary, spawned on a loopback port with a TOML
//! config, plus a minimal HTTP/1.1 client to read what it serves.
//!
//! # Why spawn instead of building a router
//!
//! #385: `build_capabilities` lives in `main.rs`, private to the binary, so an
//! integration test cannot call it, and an in-process router built here would
//! be serving a `CapabilitiesDocument` this file wrote, not the one the binary
//! computes from config. Spawning `CARGO_BIN_EXE_acdp-registry` (the pattern
//! `tls_startup.rs` and `lifecycle_refuse_start.rs` use) measures the document
//! an operator's deployment actually serves, including every `with_*` builder
//! that appends profiles after `build_capabilities` returns.
//!
//! Child stdio goes to FILES, never pipes: an undrained pipe wedges a chatty
//! child once the OS buffer fills (see `tls_startup.rs`).
//!
//! Included with `mod spawned_registry;` from `conformance.rs`. A directory
//! module, not a top-level `tests/*.rs`, so Cargo does not compile it as a test
//! binary of its own.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

/// One running registry process. Killed on drop.
pub struct SpawnedRegistry {
    child: Child,
    port: u16,
    stdout: PathBuf,
    stderr: PathBuf,
}

/// An ephemeral loopback port. Racy in principle (released before the child
/// binds it); the child failing to bind surfaces as a startup exit with its
/// own output, not as a hang.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

impl SpawnedRegistry {
    /// Spawn the binary with `[registry]` (authority, port, bind) and
    /// `[storage]` (a SQLite file under `dir`) written here, followed by
    /// `extra_toml` verbatim. `extra_toml` must not repeat those two tables.
    /// Blocks until `/livez` answers; panics with the child's output if it
    /// exits first or never becomes ready.
    pub fn start(dir: &Path, tag: &str, registry_extra: &str, extra_toml: &str) -> Self {
        let port = free_port();
        let cfg = dir.join(format!("{tag}.toml"));
        std::fs::write(
            &cfg,
            format!(
                "[registry]\nauthority = \"localhost\"\nport = {port}\nbind = \"127.0.0.1\"\n\
                 {registry_extra}\n\n[storage]\nbackend = \"sqlite\"\nsqlite_path = \"{}\"\n\n\
                 {extra_toml}\n",
                dir.join(format!("{tag}.sqlite")).display()
            ),
        )
        .expect("write the registry config");
        let stdout = dir.join(format!("{tag}.stdout"));
        let stderr = dir.join(format!("{tag}.stderr"));
        let child = Command::new(env!("CARGO_BIN_EXE_acdp-registry"))
            .env("ACDP_REGISTRY_CONFIG", &cfg)
            .stdout(Stdio::from(
                std::fs::File::create(&stdout).expect("child stdout file"),
            ))
            .stderr(Stdio::from(
                std::fs::File::create(&stderr).expect("child stderr file"),
            ))
            .spawn()
            .expect("spawn the registry binary");
        let mut reg = Self {
            child,
            port,
            stdout,
            stderr,
        };
        reg.wait_ready();
        reg
    }

    /// The child's stdout and stderr so far, for failure messages.
    pub fn output(&self) -> String {
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

    fn wait_ready(&mut self) {
        for _ in 0..300 {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                panic!(
                    "registry exited during startup with {status}\n{}",
                    self.output()
                );
            }
            if let Ok((200, _)) = self.request("GET", "/livez", None) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        panic!("registry never became ready\n{}", self.output());
    }

    /// `GET path`; panics (with the child's output) on a transport error.
    pub fn get(&self, path: &str) -> (u16, Value) {
        self.request("GET", path, None)
            .unwrap_or_else(|e| panic!("GET {path}: {e}\n{}", self.output()))
    }

    /// `POST path` with a JSON body; panics on a transport error.
    pub fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        self.request("POST", path, Some(body))
            .unwrap_or_else(|e| panic!("POST {path}: {e}\n{}", self.output()))
    }

    /// Minimal HTTP/1.1, one request per connection (`Connection: close`, read
    /// to EOF). This crate has no HTTP client among its dev-dependencies. A
    /// non-JSON body comes back as `Value::String`, an empty one as `Null`.
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> std::io::Result<(u16, Value)> {
        let mut s = TcpStream::connect(("127.0.0.1", self.port))?;
        s.set_read_timeout(Some(Duration::from_secs(10)))?;
        let payload = body
            .map(|b| serde_json::to_vec(b).expect("serialize the request body"))
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
}

impl Drop for SpawnedRegistry {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
