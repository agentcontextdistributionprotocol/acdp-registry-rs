//! E4 — the challenge nonce must never reach a log sink.
//!
//! # Why this is its own test binary
//!
//! `tracing` resolves a subscriber through process-global state: callsite
//! interest is cached globally the first time a callsite is hit, and the default
//! dispatcher is per-thread. In a shared unit-test binary another test can reach
//! the `challenge issued` callsite first, with no subscriber installed, and the
//! event is then permanently uninteresting for the rest of the process.
//!
//! That is not hypothetical here. The first version of this guard lived in
//! `service.rs`'s test module and **passed 100% of the time in isolation while
//! failing ~75% of the time in the full suite** — a pure test-ordering flake, and
//! one I initially misdiagnosed twice (thread-locality, then interest caching;
//! rebuilding the interest cache did not fix it either). An integration test is
//! its own process with one test in it, which removes the shared state rather
//! than fighting it.
//!
//! The precondition assertion below is what turned the flake into a visible
//! failure instead of a silent pass: without it an empty capture buffer trivially
//! satisfies "the nonce is absent".

use std::io::Write;
use std::sync::{Arc, Mutex};

use acdp_registry_auth::{
    AuthService, ChallengeStore, InMemoryChallengeStore, JwtSecret, JwtSigner,
};
use acdp_registry_types::config::AuthConfig;

#[derive(Clone)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
    type Writer = Buf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The nonce is the value the agent must **sign**, so disclosure alone forges
/// nothing without the agent's key — this is hygiene, not a live hole. It is
/// worth guarding because it is a short-TTL credential-shaped value and logs are
/// routinely shipped somewhere the registry does not control.
///
/// The assertion is on **what reaches the sink** — the nonce's actual value,
/// anywhere in the formatted output — rather than on the field set. A field-level
/// check would miss the nonce being interpolated into a message string, which
/// falsification confirms is a real regression path.
#[test]
fn issue_challenge_does_not_log_the_nonce() {
    let sink = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(Buf(sink.clone()))
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();

    let challenges: Arc<dyn ChallengeStore> = Arc::new(InMemoryChallengeStore::default());
    let signer = JwtSigner::new(
        JwtSecret::from_bytes(&[7u8; 32]),
        "did:web:registry.test".into(),
        "registry.test".into(),
        30,
    );
    let svc = AuthService::new(
        AuthConfig::default(),
        challenges,
        signer,
        Arc::new(acdp::did::WebResolver::new()),
        "registry.test".into(),
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let challenge = tracing::subscriber::with_default(subscriber, || {
        rt.block_on(svc.issue_challenge("did:web:agents.test:alice"))
    })
    .expect("challenge issued");

    let logged = String::from_utf8(sink.lock().unwrap().clone()).expect("utf8");

    // PRECONDITION, asserted rather than assumed (Rule 67): the capture must
    // actually have seen the event. An empty buffer satisfies the real assertion
    // below while proving nothing at all.
    assert!(
        logged.contains("challenge issued"),
        "precondition: the log capture must have seen the event; got {logged:?}"
    );
    assert!(
        !logged.contains(&challenge.nonce),
        "the challenge nonce must never reach a log sink; found it in: {logged}"
    );
}
