//! Test-build-only outbound override for the spawned-binary keyless tests.
//!
//! `tests/keyless_e2e.rs` runs the shipped binary against a loopback HTTPS
//! issuer and DID host. The binary's outbound posture refuses exactly that
//! (loopback is forbidden, and only the webpki roots are trusted), so the
//! test needs one host it can point at itself. This module is that hook,
//! and it is compiled in only with the `test-egress` feature.
//!
//! **Nothing in a production build can reach it.** The feature is enabled
//! only by `acdp-registry-server`'s `[dev-dependencies]` line for this
//! crate. Under the workspace's `resolver = "2"`, dev-dependency features
//! are unified only into builds that compile test targets, so
//! `cargo build` and `cargo build --release` produce a binary in which
//! [`web_resolver`] and [`keyless_outbound`] return `None` without reading
//! anything. `tests/keyless_e2e.rs` pins that manifest shape.
//!
//! In a test build the override is still off unless `ACDP_TEST_EGRESS` is
//! set. Its value is JSON:
//! `{"ca_pem_file": "<path>", "host": "<name>", "addr": "127.0.0.1:<port>"}`.
//! Then `host` (and only `host`) resolves to `addr`, loopback is permitted
//! for it, and the CA in `ca_pem_file` is trusted beside the webpki roots.
//! It applies to two clients: the DID resolver (`auth.resolver`, which
//! keyless uses for controller documents) and the keyless issuer-key
//! fetcher. Every other outbound client is unchanged.

#[cfg(feature = "test-egress")]
mod imp {
    use std::net::SocketAddr;
    use std::sync::Arc;

    /// The parsed `ACDP_TEST_EGRESS` value.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Egress {
        ca_pem_file: std::path::PathBuf,
        host: String,
        addr: SocketAddr,
    }

    /// `ACDP_TEST_EGRESS`, or `None` when unset. A set but unusable value
    /// panics: it only ever comes from a test, and a silent fallback would
    /// run that test against the production posture.
    fn egress() -> Option<(Egress, Vec<u8>)> {
        let raw = std::env::var("ACDP_TEST_EGRESS").ok()?;
        let e: Egress = serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("ACDP_TEST_EGRESS is not valid: {err}"));
        let pem = std::fs::read(&e.ca_pem_file).unwrap_or_else(|err| {
            panic!(
                "ACDP_TEST_EGRESS ca_pem_file {}: {err}",
                e.ca_pem_file.display()
            )
        });
        tracing::warn!(
            host = %e.host,
            addr = %e.addr,
            "ACDP_TEST_EGRESS is set: a test-build outbound override is active"
        );
        Some((e, pem))
    }

    pub fn web_resolver() -> Option<acdp::did::WebResolver> {
        let (e, pem) = egress()?;
        Some(
            acdp::did::WebResolver::with_test_endpoint(&pem, &e.host, e.addr)
                .unwrap_or_else(|err| panic!("ACDP_TEST_EGRESS resolver: {err}")),
        )
    }

    pub fn keyless_outbound() -> Option<crate::keyless::Outbound> {
        let (e, pem) = egress()?;
        let outbound = crate::keyless::Outbound {
            policy: acdp::safe_http::SsrfPolicy {
                allow_loopback_resolved: true,
                ..acdp::safe_http::SsrfPolicy::default()
            },
            resolver: Arc::new(
                crate::callback::StaticResolver::new().with(&e.host, &[e.addr.ip()]),
            ),
            extra_roots: Vec::new(),
        };
        Some(
            outbound
                .with_root_pem(&pem)
                .unwrap_or_else(|err| panic!("ACDP_TEST_EGRESS ca_pem_file: {err}")),
        )
    }
}

/// The DID resolver `ACDP_TEST_EGRESS` asks for, in a test build with it
/// set; `None` otherwise (always `None` in a production build).
pub fn web_resolver() -> Option<acdp::did::WebResolver> {
    #[cfg(feature = "test-egress")]
    {
        imp::web_resolver()
    }
    #[cfg(not(feature = "test-egress"))]
    {
        None
    }
}

/// The keyless issuer-key outbound `ACDP_TEST_EGRESS` asks for, in a test
/// build with it set; `None` otherwise (always `None` in a production
/// build).
pub fn keyless_outbound() -> Option<crate::keyless::Outbound> {
    #[cfg(feature = "test-egress")]
    {
        imp::keyless_outbound()
    }
    #[cfg(not(feature = "test-egress"))]
    {
        None
    }
}
