//! Live smoke test for a deployed registry: publish -> resolve -> retract ->
//! resolve, with every read going through `acdp::client::CrossRegistryResolver`
//! configured the way Seam's `seam-context-acdp` configures it (SSRF policy,
//! an authority allowlist, and fail-closed RFC-ACDP-0014 revocation discovery).
//!
//! Two subcommands:
//!
//! ```text
//! live-smoke did-doc --did did:web:<host>
//! live-smoke run --base-url https://<registry> --producer-did did:web:<host>
//! live-smoke run --base-url https://<registry> --producer-did did:key
//! ```
//!
//! Both read the producer's Ed25519 seed (standard base64, 32 bytes) from
//! `SMOKE_PRODUCER_SEED_B64`. `did-doc` prints the `did.json` to serve at the
//! producer DID's `did:web` URL; `run` needs that document to be live, because
//! the registry and the resolver both fetch it to check the producer signature.
//! `--producer-did did:key` publishes as the seed's own did:key instead, which
//! needs nothing hosted but does need the registry to list `did:key` in
//! `auth.did_methods`.
//! The publish is `public`, so the registry must run with
//! `auth.anonymous_public_reads = true` for the resolver to read it back.

use std::time::Duration;

use acdp::client::{
    CrossRegistryResolver, DiscoveryFailurePolicy, RegistryClient, RevocationCache,
    RevocationDiscovery, RevocationPolicy,
};
use acdp::crypto::SigningKey;
use acdp::did::key::DidKeyMaterial;
use acdp::producer::Producer;
use acdp::safe_http::SsrfPolicy;
use acdp::types::lifecycle::{LifecycleEvent, LifecycleEventType};
use acdp::types::primitives::{AgentDid, ContextType, CtxId, Visibility};
use acdp::types::EventReceipt;
use anyhow::{bail, ensure, Context as _, Result};
use base64::Engine as _;
use serde_json::{json, Value};

const KEY_FRAGMENT: &str = "key-1";

fn seed() -> Result<SigningKey> {
    let b64 = std::env::var("SMOKE_PRODUCER_SEED_B64")
        .context("SMOKE_PRODUCER_SEED_B64 is not set (openssl rand -base64 32)")?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .context("SMOKE_PRODUCER_SEED_B64 is not standard base64")?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| anyhow::anyhow!("SMOKE_PRODUCER_SEED_B64 must decode to 32 bytes"))?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn arg(args: &[String], name: &str) -> Result<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
        .with_context(|| format!("missing {name} <value>"))
}

/// The `did.json` for a `did:web` producer holding `key`. The Ed25519
/// multibase is the same string a `did:key` carries after `did:key:`.
fn did_doc(did: &str, key: &SigningKey) -> Value {
    let did_key = acdp::did::key::did_key_from_ed25519(&key.verifying_key_bytes());
    let multibase = did_key.trim_start_matches("did:key:");
    let vm = format!("{did}#{KEY_FRAGMENT}");
    json!({
        "@context": ["https://www.w3.org/ns/did/v1", "https://w3id.org/security/multikey/v1"],
        "id": did,
        "verificationMethod": [{
            "id": vm,
            "type": "Multikey",
            "controller": did,
            "publicKeyMultibase": multibase,
        }],
        "authentication": [vm],
        "assertionMethod": [vm],
    })
}

/// Seam's resolver shape (`seam-context-acdp::with_remote_registry` +
/// `revocation_policy`), restricted to the one registry under test.
fn resolver(authority: &str) -> CrossRegistryResolver {
    let mut d = RevocationDiscovery::all_trust_classes();
    d.freshness = acdp::client::RECOMMENDED_NEGATIVE_FRESHNESS;
    d.max_requests = std::num::NonZeroUsize::new(32);
    d.max_bytes = Some(1024 * 1024);
    d.total_timeout = Duration::from_secs(10);
    d.on_failure = DiscoveryFailurePolicy::FailClosed;
    CrossRegistryResolver::new()
        .with_ssrf_policy(SsrfPolicy::default())
        .with_allowlist([authority.to_string()])
        .with_revocation_policy(RevocationPolicy::default().with_discovery(d))
        .with_revocation_cache(RevocationCache::new())
}

/// The library's retraction predicate (`RegistryState::retraction`): status
/// first, then the event scan. A registry that advertises
/// `acdp-registry-lifecycle` omits `lifecycle_events` when there are none, so
/// for it an absent list reads `not_retracted`, not `unknown`.
fn retraction(rs: &acdp::types::RegistryState, lifecycle_advertised: bool) -> &'static str {
    match rs.retraction(lifecycle_advertised) {
        None => "unknown",
        Some(true) => "retracted",
        Some(false) => "not_retracted",
    }
}

/// RFC-ACDP-0017 §10.3, run on the context as the registry serves it: every
/// event receipt parses under the closed schema, binds the lifecycle event at
/// its position (steps 2-6, 9), and is signed under the key the registry's
/// `did.json` publishes for its `key_id` (step 7). Returns the receipts.
async fn verify_event_receipts(
    http: &reqwest::Client,
    base_url: &str,
    ctx: &Value,
) -> Result<Vec<Value>> {
    let Some(raws) = ctx["registry_state"]["event_receipts"].as_array() else {
        return Ok(Vec::new());
    };
    let events = ctx["registry_state"]["lifecycle_events"]
        .as_array()
        .context("event_receipts served without lifecycle_events")?;
    let did_doc: Value = http
        .get(format!("{base_url}/.well-known/did.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let origin = ctx["body"]["origin_registry"]
        .as_str()
        .context("body has no origin_registry")?;
    let ctx_id = CtxId(
        ctx["body"]["ctx_id"]
            .as_str()
            .context("body has no ctx_id")?
            .into(),
    );
    let created_at = chrono::DateTime::parse_from_rfc3339(
        ctx["body"]["created_at"]
            .as_str()
            .context("body has no created_at")?,
    )?
    .with_timezone(&chrono::Utc);
    for (index, raw) in raws.iter().enumerate() {
        let receipt = EventReceipt::from_value(raw)?;
        let event = events
            .get(index)
            .with_context(|| format!("event receipt {index} has no lifecycle event"))?;
        receipt.check_binding(origin, &ctx_id, event, index as u64, created_at)?;
        let multibase = did_doc["verificationMethod"]
            .as_array()
            .and_then(|vms| vms.iter().find(|vm| vm["id"] == receipt.signature.key_id))
            .and_then(|vm| vm["publicKeyMultibase"].as_str())
            .with_context(|| format!("did.json publishes no key {}", receipt.signature.key_id))?;
        let hash = EventReceipt::preimage_hash_of_value(raw)?;
        match acdp::did::key::resolve_did_key(&format!("did:key:{multibase}"))? {
            DidKeyMaterial::Ed25519(pk) => {
                receipt.verify_signature_against_hash(&hash, Some(&pk), None)?
            }
            DidKeyMaterial::EcdsaP256(pk) => {
                receipt.verify_signature_against_hash(&hash, None, Some(&pk))?
            }
        }
    }
    Ok(raws.clone())
}

async fn resolve_and_report(
    r: &CrossRegistryResolver,
    ctx: &CtxId,
    lifecycle_advertised: bool,
) -> Result<Value> {
    let v = r
        .resolve(ctx)
        .await
        .with_context(|| format!("CrossRegistryResolver::resolve({ctx})"))?;
    if let Some(Err(e)) = v.receipt_verdict() {
        bail!("receipt did not verify: {e}");
    }
    let rs = v.registry_state();
    Ok(json!({
        "status": serde_json::to_value(&rs.status)?,
        "retraction": retraction(rs, lifecycle_advertised),
        "lifecycle_events": rs.lifecycle_events.as_ref().map(|e| e.len()),
        "event_receipts": match rs.event_receipts() {
            None => Value::Null,
            Some(Ok(r)) => json!(r.len()),
            Some(Err(e)) => json!({ "error": e.to_string() }),
        },
        "receipt_verified": v.verified_receipt().is_some(),
        "receipt_key_status": format!("{:?}", v.receipt_key_status()),
        "key_status": format!("{:?}", v.key_status()),
        "revocation_verdict": format!("{:?}", v.revocation_verdict()),
        "title": v.body().title,
    }))
}

async fn run(base_url: &str, producer_did: &str) -> Result<()> {
    let key = seed()?;
    // `did:key` means "the did:key of the seed": self-verifying, nothing to
    // host. Anything else must be a did:web serving `did-doc`'s output.
    let (producer_did, key_id) = if producer_did == "did:key" {
        let did = acdp::did::key::did_key_from_ed25519(&key.verifying_key_bytes());
        let key_id = acdp::did::key::did_key_url(&did)?;
        (did, key_id)
    } else {
        AgentDid::parse_web(producer_did)?;
        (
            producer_did.to_string(),
            format!("{producer_did}#{KEY_FRAGMENT}"),
        )
    };
    let producer_did = producer_did.as_str();
    let base_url = base_url.trim_end_matches('/');
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // 1. Capabilities advertise what Seam needs.
    let caps: Value = http
        .get(format!("{base_url}/.well-known/acdp.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let profiles: Vec<&str> = caps["profiles"]
        .as_array()
        .context("capabilities have no profiles array")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if producer_did.starts_with("did:key:") {
        ensure!(
            caps["supported_did_methods"]
                .as_array()
                .is_some_and(|m| m.iter().any(|v| v == "did:key")),
            "capabilities do not list did:key in supported_did_methods"
        );
    }
    for p in ["acdp-registry-lifecycle", "acdp-registry-receipts"] {
        ensure!(
            profiles.contains(&p),
            "capabilities do not advertise {p}: {profiles:?}"
        );
    }
    ensure!(
        caps["anonymous_public_reads"] == true,
        "anonymous_public_reads is off; the resolver cannot read a public context back"
    );
    // Event receipts (RFC-ACDP-0017 §10) are minted from acdp_version 0.5.0.
    let receipts_required = caps["acdp_version"]
        .as_str()
        .and_then(|v| {
            let mut parts = v.split('.').map(str::parse::<u64>);
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .is_some_and(|version| version >= (0, 5));
    println!("capabilities: {}", serde_json::to_string(&caps)?);

    // 2. Publish a public context signed by the did:web producer.
    let producer = Producer::new_ed25519(seed()?, AgentDid::parse(producer_did)?, key_id.clone());
    let req = producer
        .publish_request()
        .title(format!("live smoke {}", chrono::Utc::now().to_rfc3339()))
        .summary("acdp-registry-rs tools/live-smoke: publish -> retract round trip")
        .context_type(ContextType::DataSnapshot)
        .visibility(Visibility::Public)
        .tags(vec!["smoke-test"])
        .build()?;
    let published = RegistryClient::new(base_url)?
        .publish(&req)
        .await
        .context("publish")?;
    let ctx = published.ctx_id.clone();
    println!("published: {ctx} (lineage {})", published.lineage_id);
    ensure!(
        published.registry_receipt.is_some(),
        "publish response carries no receipt"
    );

    // 3. Resolve through the cross-registry resolver (DNS -> capabilities ->
    //    registry DID -> body + receipt + producer DID + revocation).
    let r = resolver(ctx.authority());
    let lifecycle_advertised = profiles.contains(&"acdp-registry-lifecycle");
    let before = resolve_and_report(&r, &ctx, lifecycle_advertised).await?;
    println!("resolve before retract: {before}");
    ensure!(
        before["receipt_verified"] == true,
        "receipt was not verified"
    );
    // A fresh context has no events, so `lifecycle_events` is omitted
    // (RFC-ACDP-0013: omitted, not [], when empty); from a lifecycle registry
    // that reads `not_retracted`.
    ensure!(
        before["status"] == "active" && before["retraction"] == "not_retracted",
        "expected an active, unretracted context: {before}"
    );

    // 4. Producer-signed retraction (RFC-ACDP-0013 §5).
    let event = LifecycleEvent::new(
        uuid::Uuid::new_v4().to_string(),
        ctx.clone(),
        LifecycleEventType::Retracted,
        chrono::Utc::now(),
        AgentDid::parse(producer_did)?,
        Some("live smoke test".into()),
    )?
    .sign_with(key, key_id)?;
    let path: String = ctx
        .as_str()
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    let resp = http
        .post(format!("{base_url}/contexts/{path}/retract"))
        .json(&json!({ "event": event }))
        .send()
        .await?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    ensure!(status.is_success(), "retract returned {status}: {body}");
    println!(
        "retract: {status}, registry status {}",
        body["registry_state"]["status"]
    );

    // 4b. RFC-ACDP-0017 §10: a 0.5 registry advertising lifecycle + receipts
    //     mints one receipt per accepted event; verify it as served.
    let served: Value = http
        .get(format!("{base_url}/contexts/{path}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let receipts = verify_event_receipts(&http, base_url, &served)
        .await
        .context("event receipt verification (RFC-ACDP-0017 §10.3)")?;
    let events = served["registry_state"]["lifecycle_events"]
        .as_array()
        .map_or(0, Vec::len);
    if receipts_required {
        ensure!(
            receipts.len() == events,
            "{} event receipts for {events} lifecycle events: {served}",
            receipts.len()
        );
    }
    println!(
        "event_receipts ({} verified: binding + signature): {}",
        receipts.len(),
        serde_json::to_string(&receipts)?
    );

    // 5. Resolve again: the body is still served, the retraction is visible.
    let after = resolve_and_report(&resolver(ctx.authority()), &ctx, lifecycle_advertised).await?;
    println!("resolve after retract: {after}");
    ensure!(
        after["retraction"] == "retracted",
        "expected retracted: {after}"
    );
    ensure!(
        after["receipt_verified"] == true,
        "receipt was not verified after retract"
    );

    println!("SMOKE OK {ctx}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("did-doc") => {
            let did = arg(&args, "--did")?;
            println!("{}", serde_json::to_string_pretty(&did_doc(&did, &seed()?))?);
            Ok(())
        }
        Some("run") => run(&arg(&args, "--base-url")?, &arg(&args, "--producer-did")?).await,
        _ => bail!("usage: live-smoke did-doc --did <did:web> | run --base-url <url> --producer-did <did:web>|did:key"),
    }
}
