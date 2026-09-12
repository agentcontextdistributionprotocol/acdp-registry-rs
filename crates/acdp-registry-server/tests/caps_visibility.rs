//! H-O: the harness invariant that makes every `caps=false` test trustworthy.
//!
//! `RegistryServer` gates `anonymous_public_reads` off the `CapabilitiesDocument`
//! baked in at `try_new`, **not** off `RegistryConfig`. A test that flips the
//! config value and observes a 200 has measured the harness's caps/config split
//! and nothing about the binary — the invalid "wire probe" that let a shipping
//! anonymous-disclosure bug reach review (#255).
//!
//! Three caps-gated surfaces already have `caps=false` coverage:
//! `retrieve` and `search` (via `conformance.rs`'s `vis009_*` and
//! `seeded_harness_rebuild_changes_router_behavior_and_preserves_seeded_state`)
//! and `/log/entries` (via `http_integration.rs`'s
//! `log_entries_honours_anonymous_public_reads_from_caps`). This file does not
//! duplicate any of them. It pins the thing all three depend on and none of them
//! asserts: that **setting the knob actually reaches the predicate**, through one
//! call that cannot set only half of it.

mod common;

use acdp::crypto::SigningKey;
use acdp::producer::Producer;
use acdp::types::capabilities::{CapabilitiesDocument, Limits};
use acdp::types::primitives::{ContextType, Visibility};
use acdp_registry_types::{
    AuthConfig, LimitsConfig, PlaygroundConfig, RegistryConfig, RegistrySection, StorageBackend,
    StorageConfig, WebhookConfig,
};
use axum::http::StatusCode;
use common::{
    build_harness_with_webhook, publish, with_anonymous_public_reads, Harness, StoreMode,
};

const AUTHORITY: &str = "registry.test";

/// Base capabilities with `anonymous_public_reads` **deliberately left `true`**,
/// matching every other `caps()` helper in this crate's tests. The point of this
/// file is that callers move it through [`with_anonymous_public_reads`] rather
/// than by hand, so the base value is the permissive one on purpose: a test that
/// forgets to narrow it gets the *pre-fix* behaviour and its assertion fails,
/// rather than passing because the base happened to be safe.
fn base_caps() -> CapabilitiesDocument {
    CapabilitiesDocument {
        acdp_version: "0.1.0".into(),
        registry_did: format!("did:web:{AUTHORITY}"),
        supported_signature_algorithms: vec!["ed25519".into(), "ecdsa-p256".into()],
        // The validator gates the producer's DID method on THIS list, not on
        // `cfg.auth.did_methods` — both are set so a `did:key` publish is accepted.
        supported_did_methods: vec!["did:web".into(), "did:key".into()],
        profiles: vec!["acdp-registry-core".into()],
        limits: Limits {
            max_payload_bytes: 1_048_576,
            max_embedded_bytes: 65_536,
            idempotency_key_ttl_seconds: Some(86_400),
            max_publish_per_minute: None,
        },
        read_authentication_methods: vec![],
        anonymous_public_reads: true,
        supports_idempotency_key: true,
        extensions: Default::default(),
    }
}

fn base_config() -> RegistryConfig {
    RegistryConfig {
        registry: RegistrySection {
            authority: AUTHORITY.into(),
            port: 8443,
            bind: "0.0.0.0".into(),
            allow_public_bind: false,
            profiles: vec!["acdp-registry-core".into()],
            tls: Default::default(),
            cross_registry_resolution: true,
            cors: Default::default(),
            base_url: String::new(),
        },
        storage: StorageConfig {
            backend: StorageBackend::Sqlite,
            postgres_url: None,
            sqlite_path: None,
            max_connections: 1,
        },
        auth: AuthConfig {
            anonymous_public_reads: true,
            // `did:key` is self-contained, so publishing needs no DNS.
            did_methods: vec!["did:web".into(), "did:key".into()],
            ..AuthConfig::default()
        },
        webhook: WebhookConfig::default(),
        limits: LimitsConfig::default(),
        rate_limit: Default::default(),
        metrics: Default::default(),
        playground: PlaygroundConfig::default(),
        receipt: Default::default(),
        lifecycle: Default::default(),
        log: Default::default(),
        witnesses: Vec::new(),
    }
}

async fn harness(allow_anonymous: bool) -> Harness {
    let (cfg, caps) = with_anonymous_public_reads(base_config(), base_caps(), allow_anonymous);
    build_harness_with_webhook(cfg, caps, AUTHORITY, StoreMode::Memory, None, None).await
}

/// A `did:key` producer. Self-contained by construction, so `publish` never
/// resolves a DID over the network — `common::producer` mints `did:web:agents.test`
/// identities, whose resolution fails with `key_resolution_unreachable` in a
/// sandbox with no DNS. A test that needs the network to publish is a test that
/// fails for reasons unrelated to what it asserts.
fn did_key_producer(seed: u8) -> Producer {
    Producer::new_did_key(SigningKey::from_bytes(&[seed; 32]))
}

/// Publish one PUBLIC context and return its `ctx_id`.
async fn publish_public(h: &Harness, seed: u8, title: &str) -> String {
    let req = did_key_producer(seed)
        .publish_request()
        .title(title)
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .build()
        .unwrap();
    let (status, v) = publish(&h.router, &req, None).await;
    assert_eq!(status, StatusCode::OK, "publish must succeed: {v}");
    v["ctx_id"]
        .as_str()
        .expect("publish response carries ctx_id")
        .to_string()
}

/// **Unit guard: the helper sets BOTH knobs.**
///
/// If it set only the config field the caller would get the *old* caps value and
/// a silently wrong test; if it set only caps, the harness would be in a state
/// the real binary cannot reach, because `build_capabilities` derives the caps
/// value from the config one. One assertion per field, so a mutation that drops
/// either is named rather than masked by the other.
#[test]
fn the_helper_sets_both_knobs_not_just_one() {
    for allow in [false, true] {
        let (cfg, caps) = with_anonymous_public_reads(base_config(), base_caps(), allow);
        assert_eq!(
            cfg.auth.anonymous_public_reads, allow,
            "the CONFIG knob was not set to {allow}: the real binary derives caps \
             from this field, so a harness with it unset is in a state no \
             deployment can reach"
        );
        assert_eq!(
            caps.anonymous_public_reads, allow,
            "the CAPS knob was not set to {allow}: this is the ONLY field \
             `RegistryServer::retrieve` consults, so leaving it at the base value \
             is exactly the invalid wire probe this helper exists to prevent"
        );
    }

    // The base values are `true`, so the `allow == true` iteration above would
    // pass against a helper that does NOTHING at all. This pins that the `false`
    // case actually changed something, which is the case that matters.
    assert!(
        base_caps().anonymous_public_reads && base_config().auth.anonymous_public_reads,
        "precondition: the base values must both be `true`, or the `false` \
         iteration above proves nothing about the helper"
    );
}

/// **The invariant every `caps=false` test rests on: the knob reaches the
/// predicate.**
///
/// Rule 80 applied to this helper rather than discovered afterwards — a probe
/// that varies a value the code does not read cannot fail, so the value set here
/// is traced to the branch it must flip: `can_retrieve`'s public arm is
/// `anonymous_public_reads || requester.is_some()`, false for an anonymous
/// caller, so a PUBLIC context becomes unretrievable.
///
/// Both directions in one test on purpose. The `false` half alone would pass
/// against a harness that refuses *everything* — a broken build, a failed
/// migration, a publish that never landed — so the `true` half is not a
/// convenience, it is what makes the `false` half mean what it says.
#[tokio::test]
async fn the_caps_knob_reaches_the_retrieve_predicate_in_both_directions() {
    // Permissive: an anonymous caller CAN read a public context.
    let permissive = harness(true).await;
    let allowed_id = publish_public(&permissive, 11, "caps-true-public").await;
    let allowed = common::get_with_auth(&permissive.router, &allowed_id, None, None).await;
    assert_eq!(
        allowed,
        StatusCode::OK,
        "control: with anonymous_public_reads TRUE an anonymous caller must be \
         able to retrieve a public context — if this is not 200 the `false` \
         assertion below proves nothing, because a harness that refuses \
         everything would satisfy it"
    );

    // Narrowed: the same publish, the same anonymous caller, refused.
    let narrowed = harness(false).await;
    let hidden_id = publish_public(&narrowed, 11, "caps-false-public").await;
    let refused = common::get_with_auth(&narrowed.router, &hidden_id, None, None).await;
    assert_eq!(
        refused,
        StatusCode::NOT_FOUND,
        "with anonymous_public_reads FALSE — the SHIPPED default, on which \
         `auth.enabled` is also false so EVERY caller is anonymous — a public \
         context must not be retrievable. A 200 here means the harness built a \
         permissive registry while the caller asked for a narrowed one, and \
         every `caps=false` test in this crate is measuring the caps/config \
         split instead of the binary"
    );
}
