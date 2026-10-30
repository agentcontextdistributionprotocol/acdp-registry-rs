//! RFC-ACDP-0021 Phase 7: the shipped binary with `[keyless] enabled = true`,
//! end to end.
//!
//! The real `acdp-registry` process serves HTTPS as `agents.test` (the
//! `didweb` fixture leaf). The kit's outside server (also `agents.test`, on
//! another loopback port) plays the OIDC issuer (`/jwks`), the controller's
//! DID document and, for the consumer, the registry's own DID document. The
//! binary reaches the outside server through `ACDP_TEST_EGRESS`
//! (`acdp_registry_core::test_egress`, compiled in only for test builds).
//!
//! The test then mints over HTTP with the SDK client, publishes a keyless
//! body, verifies it on the SDK consumer path, and gets the R13 refusal for
//! a lifecycle event signed under the keyless key id.
//!
//! No receipt-key rotation happens here, so acdp 0.18.2's strict
//! `check_attestation` (no historical arm for a rotated receipt key until
//! the SDK implements 0.5.0-draft.4, `kl-005` M/N) does not come into play.

#![cfg(feature = "storage-sqlite")]

#[path = "keyless_kit/mod.rs"]
mod keyless_kit;
#[allow(dead_code)]
mod spawned_registry;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use acdp::client::{KeylessPolicy, RegistryClient, VerificationPolicy, VerifiedContext};
use acdp::did::WebResolver;
use acdp::producer::keyless::EphemeralKey;
use acdp::types::keyless::{IdentityDescriptor, KeyAttestation, WorkloadIdentity};
use base64::Engine as _;
use keyless_kit::didweb;
use serde_json::{json, Value};

const RECEIPT_SEED: [u8; 32] = [0x11; 32];
const REGISTRY_DID: &str = "did:web:agents.test";
const SUBJECT: &str = "repo:acme/app:ref:refs/heads/main";

/// The child process, killed on drop; stdio goes to files.
struct Registry {
    child: Child,
    addr: SocketAddr,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Drop for Registry {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Registry {
    fn output(&self) -> String {
        let read = |p: &Path| {
            std::fs::read(p)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        format!(
            "--- stdout ---\n{}\n--- stderr ---\n{}",
            read(&self.stdout),
            read(&self.stderr)
        )
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// An HTTPS client that reaches `agents.test` at `addr`, trusting the
/// fixture CA.
fn https_to(addr: SocketAddr) -> reqwest::Client {
    reqwest::Client::builder()
        .resolve(didweb::DIDWEB_AUTHORITY, addr)
        .add_root_certificate(
            reqwest::Certificate::from_pem(didweb::ca_pem().as_bytes()).expect("CA"),
        )
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client")
}

/// Spawn the binary with keyless on, the issuer `issuer`, and its outbound
/// pointed at `outside`.
async fn spawn_keyless_registry(dir: &Path, issuer: &str, outside: SocketAddr) -> Registry {
    let fx = didweb::fixture();
    let (ca, cert, key) = (
        dir.join("ca.pem"),
        dir.join("leaf.pem"),
        dir.join("leaf.key"),
    );
    std::fs::write(&ca, &fx.ca_pem).unwrap();
    std::fs::write(&cert, &fx.leaf_pem).unwrap();
    std::fs::write(&key, &fx.leaf_key_pem).unwrap();
    let port = free_port();
    let cfg = dir.join("keyless.toml");
    std::fs::write(
        &cfg,
        format!(
            r#"
[registry]
authority = "agents.test"
port = {port}
bind = "127.0.0.1"

[registry.tls]
enabled = true
cert_path = "{cert}"
key_path = "{key}"

[storage]
backend = "sqlite"
sqlite_path = "{db}"

[auth]
did_methods = ["did:web", "did:key"]
anonymous_public_reads = true

[receipt]
signing_key_seed_b64 = "{seed}"

[lifecycle]
enabled = true

[log]
enabled = true

[keyless]
enabled = true

[[keyless.oidc_issuers]]
issuer = "{issuer}"
jwks_uri = "{issuer}/jwks"
"#,
            cert = cert.display(),
            key = key.display(),
            db = dir.join("keyless.sqlite").display(),
            seed = base64::engine::general_purpose::STANDARD.encode(RECEIPT_SEED),
        ),
    )
    .unwrap();
    let egress = json!({
        "ca_pem_file": ca,
        "host": didweb::DIDWEB_AUTHORITY,
        "addr": outside.to_string(),
    });
    let stdout = dir.join("keyless.stdout");
    let stderr = dir.join("keyless.stderr");
    let child = Command::new(env!("CARGO_BIN_EXE_acdp-registry"))
        .env("ACDP_REGISTRY_CONFIG", &cfg)
        .env("ACDP_TEST_EGRESS", egress.to_string())
        .stdout(Stdio::from(std::fs::File::create(&stdout).unwrap()))
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()))
        .spawn()
        .expect("spawn the registry binary");
    let mut reg = Registry {
        child,
        addr: SocketAddr::from(([127, 0, 0, 1], port)),
        stdout,
        stderr,
    };
    let http = https_to(reg.addr);
    for _ in 0..300 {
        if let Some(status) = reg.child.try_wait().unwrap() {
            panic!("registry exited during startup: {status}\n{}", reg.output());
        }
        if let Ok(r) = http.get("https://agents.test/livez").send().await {
            if r.status() == 200 {
                return reg;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("registry never became ready\n{}", reg.output());
}

#[tokio::test(flavor = "multi_thread")]
async fn keyless_end_to_end_against_the_real_binary() {
    let (outside, out_addr) = keyless_kit::spawn_outside().await;
    let issuer = format!("https://{}:{}", didweb::DIDWEB_AUTHORITY, out_addr.port());
    let oidc = keyless_kit::ed_signer(7, "oidc-1");
    outside.set_jwks(json!({ "keys": [oidc.jwk.clone()] }));

    let dir = tempfile::tempdir().unwrap();
    let reg = spawn_keyless_registry(dir.path(), &issuer, out_addr).await;
    let http = https_to(reg.addr);

    // Capabilities: the profile and the member, together.
    let caps: Value = http
        .get("https://agents.test/.well-known/acdp.json")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        caps["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "acdp-registry-keyless"),
        "{caps}"
    );
    assert_eq!(
        caps["keyless"]["accepted_oidc_issuers"],
        json!([issuer]),
        "{caps}"
    );

    // The registry's DID document, served to the consumer by the outside
    // host (the consumer's resolver for `agents.test` points there).
    let registry_doc: Value = http
        .get("https://agents.test/.well-known/did.json")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    outside.set_doc(".well-known", registry_doc);

    // The controller lists the workload identity.
    let controller = "did:web:agents.test:acme";
    let identity = WorkloadIdentity::Oidc {
        issuer: issuer.clone(),
        subject: SUBJECT.into(),
    };
    outside.set_doc(
        "acme",
        keyless_kit::controller_document(controller, &[(identity, false)], &[("key-1", 3)]),
    );

    // 1. Mint over HTTP with the SDK client.
    let ca = didweb::ca_pem().as_bytes();
    let client = RegistryClient::with_test_endpoint("https://agents.test", reg.addr, ca)
        .expect("registry client");
    let now = chrono::Utc::now().timestamp();
    let token = oidc.sign(&json!({
        "iss": issuer, "sub": SUBJECT, "aud": REGISTRY_DID,
        "exp": now + 300, "iat": now, "jti": keyless_kit::unique("jti"),
    }));
    let key = EphemeralKey::from_key(keyless_kit::signing_key(41));
    let req = key
        .attestation_request(
            controller,
            REGISTRY_DID,
            IdentityDescriptor::oidc(&issuer, SUBJECT, token),
            Some(600),
        )
        .expect("request");
    let minted = client
        .request_key_attestation(&req)
        .await
        .unwrap_or_else(|e| panic!("mint: {e}\n{}", reg.output()));
    let att = KeyAttestation::from_body(&minted.body).expect("an attestation body");
    assert_eq!(att.controller, controller);
    assert_eq!(minted.body.agent_id.to_string(), REGISTRY_DID);
    assert!(outside.hits("jwks") >= 1, "the issuer keys were fetched");

    // 2. Publish a keyless body.
    let producer = key.into_producer(&minted.body).expect("keyless producer");
    let publish = keyless_kit::plain(&producer, "keyless e2e");
    let published = client
        .publish(&publish)
        .await
        .unwrap_or_else(|e| panic!("keyless publish: {e}\n{}", reg.output()));

    // 3. The SDK consumer path verifies it with this registry accepted as
    //    the attester.
    let resolver = WebResolver::with_test_endpoint(ca, didweb::DIDWEB_AUTHORITY, out_addr)
        .expect("consumer resolver");
    let policy = VerificationPolicy {
        keyless: KeylessPolicy::attesters([REGISTRY_DID]),
        ..VerificationPolicy::default()
    };
    let verified =
        VerifiedContext::fetch_with_policy(&client, &resolver, &published.ctx_id, &policy)
            .await
            .unwrap_or_else(|e| panic!("consumer verification: {e}\n{}", reg.output()));
    assert_eq!(verified.body().ctx_id, published.ctx_id);
    assert_eq!(verified.body().signature.key_id, att.key_id);
    // Control: a consumer accepting no attester refuses the same body.
    assert!(VerifiedContext::fetch_with_policy(
        &client,
        &resolver,
        &published.ctx_id,
        &VerificationPolicy::default()
    )
    .await
    .is_err());

    // 4. R13: a lifecycle event under the keyless key id is refused.
    let envelope =
        keyless_kit::event_under(&published.ctx_id.0, controller, &att.key_id, "retracted");
    let resp = http
        .post(format!(
            "https://agents.test/contexts/{}/retract",
            keyless_kit::urlencode(&published.ctx_id.0)
        ))
        .header("content-type", "application/json")
        .json(&envelope)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let v: Value = resp.json().await.unwrap();
    assert_eq!(
        (
            status,
            &v["error"]["code"],
            &v["error"]["details"]["reason"]
        ),
        (
            400,
            &json!("key_resolution_failed"),
            &json!("keyless_event_unsupported")
        ),
        "{v}"
    );
}

/// A default-config binary: no `keyless` member, and both
/// `/key-attestations` methods answer 501.
#[test]
fn a_default_binary_does_not_serve_key_attestations() {
    let dir = tempfile::tempdir().unwrap();
    let reg = spawned_registry::SpawnedRegistry::start(dir.path(), "kl-default", "", "");
    let (status, caps) = reg.get("/.well-known/acdp.json");
    assert_eq!(status, 200);
    assert!(caps.get("keyless").is_none(), "{caps}");
    let (status, v) = reg.post("/key-attestations", &json!({}));
    assert_eq!(
        (status, &v["error"]["code"]),
        (501, &json!("not_implemented")),
        "{v}"
    );
    let (status, v) = reg.get("/key-attestations?controller=did:web:agents.test:x");
    assert_eq!(
        (status, &v["error"]["code"]),
        (501, &json!("not_implemented")),
        "{v}"
    );
}

/// `test-egress` reaches the binary only through `[dev-dependencies]`:
/// no workspace crate enables it from `[dependencies]`, and core's own
/// default features do not include it.
#[test]
fn test_egress_is_reachable_only_from_dev_dependencies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Every crate manifest, plus the workspace root (whose
    // `[workspace.dependencies]` could enable it for every member) and the
    // live-smoke tool, which sit outside `crates/`.
    let mut manifests: Vec<_> = std::fs::read_dir(root.join("crates"))
        .unwrap()
        .map(|e| e.unwrap().path().join("Cargo.toml"))
        .filter(|m| m.is_file())
        .collect();
    let crate_count = manifests.len();
    for extra in ["Cargo.toml", "tools/live-smoke/Cargo.toml"] {
        let m = root.join(extra);
        assert!(m.is_file(), "{} is gone: update this guard", m.display());
        manifests.push(m);
    }
    assert!(crate_count >= 2, "found {crate_count} crate manifests");
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest).unwrap();
        let mut section = String::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                section = t.to_string();
                continue;
            }
            if t.starts_with('#') || !t.contains("test-egress") {
                continue;
            }
            let allowed = section == "[dev-dependencies]"
                || (section == "[features]" && t.starts_with("test-egress"));
            assert!(
                allowed,
                "{}: `test-egress` outside [dev-dependencies] ({section}): {t}",
                manifest.display()
            );
            if section == "[features]" {
                assert!(!t.starts_with("default"), "{t}");
            }
        }
    }
    let server =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(
        server.contains("features = [\"test-egress\"]"),
        "the dev-dependency line this guard protects is gone"
    );
}
