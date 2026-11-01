//! RFC-ACDP-0008 §6.2: a `bearer_static` read token is never logged.
//!
//! Its own test binary for the reason `acdp-registry-auth/tests/nonce_logging.rs`
//! gives: `tracing` callsite interest is process-global, so in a shared binary
//! another test can reach the request-span callsite first with no subscriber
//! installed and leave it permanently disabled. One process, one test, and a
//! precondition assertion that the capture saw the request logs at all (an
//! empty buffer would trivially "not contain the token").

#![cfg(feature = "storage-sqlite")]

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp_registry_types::config::ReadTokenEntry;
use acdp_registry_types::RegistryConfig;
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

const AUTHORITY: &str = "registry.test";

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

fn caps() -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: "0.5.0".into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        supported_signature_algorithms: vec!["ed25519".into()],
        supported_did_methods: vec!["did:web".into()],
        profiles: vec!["acdp-registry-core".into()],
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
            change_retention_seconds: None,
            max_change_filters: None,
            max_changes_page: None,
            max_subscriptions_per_requester: None,
            max_reads_per_minute: None,
            max_anonymous_reads_per_minute: None,
            extensions: Default::default(),
        },
        read_authentication_methods: vec!["bearer_jwt".into(), "bearer_static".into()],
        anonymous_public_reads: true,
        supports_idempotency_key: true,
        extensions: Default::default(),
    }
}

/// Valid, unknown, query-borne and path-borne tokens all pass through the
/// request span and the 403/400 paths; none of their secret bodies reaches
/// the sink, at any level the binary could be configured to (TRACE).
#[test]
fn read_tokens_never_reach_a_log_sink() {
    let (valid, sha256) = acdp_registry_auth::generate_read_token();
    let (unknown, _) = acdp_registry_auth::generate_read_token();
    let mut cfg = RegistryConfig::defaults();
    cfg.registry.authority = AUTHORITY.into();
    cfg.auth.enabled = true;
    cfg.auth.anonymous_public_reads = true;
    cfg.auth.read_tokens = vec![ReadTokenEntry {
        id: "logged-never".into(),
        did: "did:web:ops.example".into(),
        sha256,
        expires_at: None,
        tenant: None,
        operator_administered: true,
    }];

    let sink = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(Buf(sink.clone()))
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    tracing::subscriber::with_default(subscriber, || {
        rt.block_on(async {
            let h = common::build_harness_with_webhook(
                cfg,
                caps(),
                AUTHORITY,
                common::StoreMode::Memory,
                None,
                None,
            )
            .await;
            let requests = [
                ("/contexts/search".to_string(), Some(valid.as_str())),
                ("/contexts/search".to_string(), Some(unknown.as_str())),
                (format!("/contexts/search?access_token={valid}"), None),
                (format!("/contexts/search?q={valid}"), None),
                (format!("/contexts/{unknown}"), None),
            ];
            for (uri, bearer) in requests {
                let mut b = Request::builder().uri(&uri);
                if let Some(t) = bearer {
                    b = b.header("authorization", format!("Bearer {t}"));
                }
                let resp = h
                    .router
                    .clone()
                    .oneshot(b.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let _ = axum::body::to_bytes(resp.into_body(), usize::MAX).await;
            }
        })
    });

    let logged = String::from_utf8(sink.lock().unwrap().clone()).expect("utf8");
    // PRECONDITION: the capture saw the request spans (the redacted URI is in
    // them), so the absence below means something.
    assert!(
        logged.contains("http_request") && logged.contains("[redacted]"),
        "precondition: the log capture must have seen the request spans; got {logged:?}"
    );
    for token in [&valid, &unknown] {
        let body = token.trim_start_matches(acdp_registry_auth::READ_TOKEN_PREFIX);
        assert!(
            !logged.contains(body),
            "a read token reached a log sink: {logged}"
        );
    }
}
