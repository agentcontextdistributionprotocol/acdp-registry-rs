//! An in-process HTTPS server serving a producer's `did:web` document.
//!
//! # Why this exists
//!
//! U-504's mutation baseline left `handlers/context.rs:1542` as an accepted
//! survivor: deleting the `LifecycleEventType::Retracted` arm of
//! `lifecycle_transition`'s **did:web** dispatch turns a retract into a
//! republish, and no test noticed. The reason was structural, not an oversight —
//! `signed_event_envelope` can only sign as `did:key`, so
//! `actor.starts_with("did:key:")` was true in every lifecycle test in the repo.
//!
//! Writing the did:web-signed retract was not enough: it died at
//! `key_resolution_unreachable`, because `retract_verified` resolves the actor
//! through a real [`WebResolver`] and playground mode does **not** bypass that.
//! Covering the branch therefore needs a real HTTPS endpoint, which is what this
//! module is.
//!
//! # How it is wired
//!
//! `WebResolver::with_test_endpoint(pem, host, addr)` (gated behind the
//! `test-transport` feature, already enabled as a dev-dependency of this crate)
//! maps the authority `agents.test` to a loopback socket through reqwest's
//! `.resolve()` hook and trusts `pem` as a root. So the DID keeps its real
//! authority — no port, no rewritten URL — and the resolution is a genuine TLS
//! request to a genuine server. The only thing faked is where DNS points.

use std::net::SocketAddr;
use std::sync::Once;

use acdp::crypto::SigningKey;
use axum::{routing::get, Json, Router};
use axum_server::tls_rustls::RustlsConfig;

/// The **leaf** certificate the fixture server presents for `agents.test`.
/// `CA:FALSE`, `serverAuth`, `SAN=DNS:agents.test`.
pub const CERT_PEM: &[u8] = include_bytes!("../fixtures/didweb/agents-test-cert.pem");

/// The **CA** that signed [`CERT_PEM`], and the only certificate the resolver
/// trusts.
///
/// These are two certificates on purpose, and the first shape I tried was wrong
/// in a way worth recording: one self-signed `CA:TRUE` cert doing both jobs.
/// TLS refused it with `CaUsedAsEndEntity` — rustls will not accept a CA
/// certificate as an end-entity, so "make it a CA so it can also be the trust
/// root" does not work. `with_test_endpoint(pem, ..)` takes a single *root*, so
/// the fixture has to be a real chain: the CA here, the leaf above signed by it.
pub const CA_PEM: &[u8] = include_bytes!("../fixtures/didweb/agents-test-ca.pem");

/// The leaf's private key. Test-only; `.test` is IANA-reserved and cannot resolve.
pub const KEY_PEM: &[u8] = include_bytes!("../fixtures/didweb/agents-test-key.pem");

/// The authority the served DIDs live under.
pub const DIDWEB_AUTHORITY: &str = "agents.test";

/// The `did:web` DID for the producer `common::producer("smoke", seed)` signs as.
pub fn didweb_for_seed(seed: u8) -> String {
    format!("did:web:{DIDWEB_AUTHORITY}:smoke-{seed}")
}

/// Its `#key-1` verification-method id — the `key_id` that producer signs with.
pub fn didweb_key_id_for_seed(seed: u8) -> String {
    format!("{}#key-1", didweb_for_seed(seed))
}

/// The DID document for `smoke-<seed>`, shaped as `receipt.rs`'s
/// `verification_method_entry` shapes the registry's own.
///
/// `publicKeyMultibase` is derived from the did:key encoder, whose
/// method-specific identifier **is** that multibase string — the same derivation
/// `receipt.rs::ed25519_multibase` uses, so this fixture cannot encode the key
/// one way while the registry encodes it another.
fn did_document_for_seed(seed: u8) -> serde_json::Value {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let multibase = acdp::did::key::did_key_from_ed25519(&key.verifying_key_bytes())
        .strip_prefix("did:key:")
        .expect("did_key_from_ed25519 always returns a did:key: prefix")
        .to_string();
    let did = didweb_for_seed(seed);
    let key_id = didweb_key_id_for_seed(seed);
    serde_json::json!({
        "@context": [
            "https://www.w3.org/ns/did/v1",
            "https://w3id.org/security/suites/ed25519-2020/v1"
        ],
        "id": did,
        "verificationMethod": [{
            "id": key_id,
            "type": "Ed25519VerificationKey2020",
            "controller": did,
            "publicKeyMultibase": multibase,
        }],
        "assertionMethod": [key_id],
    })
}

/// Serve `https://agents.test/smoke-<seed>/did.json` on a loopback port and
/// return the socket the resolver must be pointed at.
///
/// The path shape is did:web's own: `did:web:agents.test:smoke-63` resolves to
/// `https://agents.test/smoke-63/did.json`. That is not a guess — it is the URL
/// the failure this module exists to fix printed.
pub async fn spawn_didweb_server() -> SocketAddr {
    install_crypto_provider();

    let app = Router::new().route(
        "/{name}/did.json",
        get(
            |axum::extract::Path(name): axum::extract::Path<String>| async move {
                let seed: u8 = name
                    .strip_prefix("smoke-")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                Json(did_document_for_seed(seed))
            },
        ),
    );

    let config = RustlsConfig::from_pem(CERT_PEM.to_vec(), KEY_PEM.to_vec())
        .await
        .expect("fixture cert/key must load; see tests/fixtures/didweb/README.md");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind did:web listener");
    let addr = listener.local_addr().expect("addr");
    // `from_tcp_rustls` hands the fd to tokio, which refuses a BLOCKING socket
    // ("Registering a blocking socket with the tokio runtime is unsupported").
    // Port 0 gives us the address before the server starts, which is why we bind
    // by hand rather than letting axum-server bind -- so this conversion is ours
    // to do too.
    listener
        .set_nonblocking(true)
        .expect("did:web listener must be nonblocking for tokio");
    tokio::spawn(async move {
        let _ = axum_server::from_tcp_rustls(listener, config)
            .expect("tls server from listener")
            .serve(app.into_make_service())
            .await;
    });
    addr
}

/// Pick a rustls [`CryptoProvider`] explicitly, because this workspace enables
/// two and rustls will not choose for you.
///
/// `aws-lc-rs` arrives via `axum-server/tls-rustls` (the server side of this
/// fixture) and `ring` via `hyper-rustls` <- `reqwest` <- `acdp-client` (the
/// resolver side). With both enabled and no explicit choice, rustls's
/// process-global default is never set and **every** handshake fails -- the
/// server's `RustlsConfig::from_pem` and the resolver's request alike. Nothing
/// in `crates/` calls `install_default`, so without this the fixture cannot
/// work, which is why the dev-dependency exists at all.
///
/// `install_default` is process-global and fallible-once. `Once` makes the call
/// site idempotent across the several tests that spawn a server, and `.ok()`
/// absorbs the residual race where something else won -- in that case a provider
/// IS installed, which is all this function is for.
///
/// **The production binary has the same gap and this does not fix it.** `main.rs`
/// reaches `axum_server::bind_rustls` whenever `tls.cert_path`/`key_path` are
/// set, with no provider installed; every test in the repo sets `tls:
/// Default::default()`, so nothing enters that branch. The shipped
/// `docker/config.docker.toml` disables in-process TLS deliberately (an edge
/// terminates) and `config/registry.example.toml` ships those two keys commented
/// out, so no current deployment reaches it -- it is latent on a documented
/// configuration path, not a live outage. Filed as U-530; `main.rs` is not this
/// unit's to edit.
fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
    });
}
