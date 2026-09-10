//! HMAC-signed webhook emitter.
//!
//! `WebhookEmitter::spawn` starts a background worker that drains an mpsc
//! channel and POSTs each event to the configured URL. Failed deliveries
//! are retried with exponential backoff up to `max_retries`. Signing
//! follows GitHub's convention: `X-ACDP-Signature: sha256=<hex>` over the
//! raw JSON body.

use std::time::Duration;

use acdp::safe_http::SsrfPolicy;
use acdp_registry_types::{WebhookConfig, WebhookEvent};
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha2::Sha256;
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Wire schema version of the webhook *envelope* — the `event_id` +
/// `schema_version` wrapper itself, not the flattened per-variant payloads.
/// Bump on a backwards-incompatible change to that envelope shape so the
/// control plane can branch on it.
///
/// Deliberately does **not** move for a per-variant field change — including
/// one that touches every variant. This value is stamped on each *delivery*,
/// so it describes that delivery's envelope, not the event stream: a
/// `search_executed` body carrying a bumped version would assert that
/// something about *that* delivery changed, when nothing did. Note receivers
/// parse all five types through one shared model, so "only some variants
/// changed" is a distinction that exists here and in no receiver — the reason
/// not to bump is per-delivery truthfulness, not blast radius.
///
/// The scope is not local convention: RFC-ACDP-0009 §2.10 reserves this
/// profile's version field as the schema version of the event *envelope*,
/// independent of `acdp_version`. That section is **reserved, not normative** —
/// it says implementations must not depend on its sketch for interoperability,
/// and this envelope already diverges from it (`schema_version` vs the reserved
/// `event_version`, `type` vs `event_type`). It corroborates the *scope* chosen
/// here rather than mandating it. Expect the next real move to come from that
/// section's promotion, not from a variant edit.
///
/// Per-variant wire changes are recorded in `docs/WEBHOOKS.md` under "Wire
/// change history" instead. Precedent: #179 renamed the `context.retracted` /
/// `context.republished` lifecycle id to `lifecycle_event_id` and left this
/// at `1.0`.
pub const WEBHOOK_SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("send channel closed")]
    Closed,
    #[error("encode: {0}")]
    Encode(String),
    #[error("config: {0}")]
    Config(String),
}

type HmacSha256 = Hmac<Sha256>;

/// An event plus out-of-band routing metadata carried to the worker. The
/// `tenant_id` travels as the `X-Tenant-Id` request header — NOT in the
/// signed JSON body — so the GitHub-compatible signature scheme and the
/// stable event schema are both preserved while still letting a
/// multi-tenant control plane attribute the delivery.
///
/// `event_id` is minted once at emit time (REG-P2-6) and reused across every
/// delivery retry, so the control plane can dedupe re-deliveries.
#[derive(Debug, Clone)]
struct Delivery {
    event_id: String,
    event: WebhookEvent,
    tenant_id: Option<String>,
}

/// What actually goes on the wire: the event flattened under a small envelope
/// carrying `event_id` + `schema_version`. Flattening keeps the historical
/// shape (top-level `type` + variant fields) so existing consumers keep
/// working while gaining the two dedupe/versioning fields.
#[derive(Debug, Serialize)]
struct WireEnvelope<'a> {
    event_id: &'a str,
    schema_version: &'a str,
    #[serde(flatten)]
    event: &'a WebhookEvent,
}

/// Handle held by the HTTP layer. `emit` is non-blocking; the worker
/// drains events asynchronously.
#[derive(Clone)]
pub struct WebhookEmitter {
    tx: mpsc::Sender<Delivery>,
}

impl WebhookEmitter {
    /// Validate the webhook configuration and spawn the worker.
    ///
    /// SEC-03: the URL is checked against the same SSRF policy
    /// (`acdp::safe_http::SsrfPolicy::default()`) that the DID resolver
    /// uses — HTTPS-only, no IP literals, hostnames only. Without this
    /// gate a misconfigured or maliciously set webhook URL turns the
    /// registry into an SSRF proxy against internal services like the
    /// AWS / GCP metadata endpoint.
    ///
    /// SEC-04: when the webhook is enabled and has a non-empty URL, the
    /// shared HMAC secret must be non-empty. The HMAC primitive will
    /// happily compute over a zero-length key, which means a receiver
    /// that checks `X-ACDP-Signature` will accept every event as
    /// authentic — defeating the integrity guarantee.
    pub fn try_spawn(config: WebhookConfig) -> Result<Self, WebhookError> {
        if config.enabled && !config.url.is_empty() {
            acdp::safe_http::SsrfPolicy::default()
                .check_url(&config.url)
                .map_err(|e| {
                    WebhookError::Config(format!(
                        "webhook.url '{}' rejected by SSRF policy: {e}",
                        config.url
                    ))
                })?;
            if config.secret.trim().is_empty() {
                return Err(WebhookError::Config(
                    "webhook.secret must be non-empty when webhook.enabled and webhook.url \
                     are set; HMAC over an empty key accepts every signature"
                        .into(),
                ));
            }
        }
        Ok(Self::spawn(config))
    }

    /// Spawn without configuration validation. Prefer `try_spawn` —
    /// retained for tests and for cases where the caller has already
    /// validated the config. Uses the strict default SSRF policy.
    pub fn spawn(config: WebhookConfig) -> Self {
        Self::spawn_with_policy(config, acdp::safe_http::SsrfPolicy::default())
    }

    /// Like [`spawn`](Self::spawn) but with an explicit SSRF policy for the
    /// delivery client. Production uses the strict default (via `spawn` /
    /// `try_spawn`); tests that POST to a local listener pass
    /// `SsrfPolicy::allow_test_loopback()`.
    pub fn spawn_with_policy(config: WebhookConfig, policy: acdp::safe_http::SsrfPolicy) -> Self {
        let capacity = config.queue_capacity.max(1);
        let (tx, rx) = mpsc::channel::<Delivery>(capacity);
        tokio::spawn(worker(config, rx, policy));
        Self { tx }
    }

    /// Snapshot of the delivery queue for the admin status endpoint:
    /// `(in_flight, capacity)`. `in_flight` is how many events are buffered
    /// and not yet delivered; nearing `capacity` means the worker is falling
    /// behind and events are at risk of being dropped.
    pub fn queue_status(&self) -> (usize, usize) {
        let capacity = self.tx.max_capacity();
        let in_flight = capacity.saturating_sub(self.tx.capacity());
        (in_flight, capacity)
    }

    /// Fire and forget. The channel is bounded; if the worker can't keep
    /// up, the event is dropped with a warn log rather than blocking the
    /// HTTP handler.
    pub fn emit(&self, event: WebhookEvent) {
        self.emit_with_tenant(event, None);
    }

    /// Like [`emit`](Self::emit) but tags the delivery with a tenant id,
    /// forwarded as the `X-Tenant-Id` header so a multi-tenant control
    /// plane can attribute the event. The id never enters the signed body.
    pub fn emit_with_tenant(&self, event: WebhookEvent, tenant_id: Option<String>) {
        let delivery = Delivery {
            event_id: Uuid::new_v4().to_string(),
            event,
            tenant_id,
        };
        match self.tx.try_send(delivery) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("webhook queue full; event dropped");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!("webhook channel closed; event dropped");
            }
        }
    }
}

async fn worker(config: WebhookConfig, mut rx: mpsc::Receiver<Delivery>, policy: SsrfPolicy) {
    // SEC (#6): the delivery client filters every resolved IP through the SSRF
    // policy at DNS time and refuses redirects, so a webhook URL whose DNS
    // answers (or 3xx redirects to) a private/IMDS address cannot turn the
    // registry into an SSRF proxy. The earlier plain client did neither.
    let client =
        match acdp::safe_http::safe_client(&policy, Duration::from_secs(config.timeout_seconds)) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "webhook: cannot build HTTP client; disabling");
                return;
            }
        };
    while let Some(delivery) = rx.recv().await {
        if !config.enabled || config.url.is_empty() {
            continue;
        }
        if let Err(e) = deliver(&client, &config, &delivery).await {
            tracing::warn!(error = %e, event = delivery.event.name(), "webhook delivery failed");
        }
    }
}

async fn deliver(
    client: &reqwest::Client,
    config: &WebhookConfig,
    delivery: &Delivery,
) -> Result<(), WebhookError> {
    let event = &delivery.event;
    let envelope = WireEnvelope {
        event_id: &delivery.event_id,
        schema_version: WEBHOOK_SCHEMA_VERSION,
        event,
    };
    let body = serde_json::to_vec(&envelope).map_err(|e| WebhookError::Encode(e.to_string()))?;
    let sig = sign(&config.secret, &body);

    let mut backoff = Duration::from_millis(250);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let mut builder = client
            .post(&config.url)
            .header("Content-Type", "application/json")
            .header("X-ACDP-Signature", &sig)
            .header("X-ACDP-Event", event.name())
            .header("X-ACDP-Event-Id", &delivery.event_id);
        if let Some(tenant) = &delivery.tenant_id {
            builder = builder.header("X-Tenant-Id", tenant);
        }
        let resp = builder.body(body.clone()).send().await;
        match resp {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) => {
                let status = r.status();
                if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
                    // 4xx (non-429) won't change on retry — treat as permanent
                    // failure so operators can grep for `webhook_4xx`.
                    tracing::warn!(
                        event = "webhook_4xx",
                        status = %status,
                        url = %config.url,
                        attempt,
                        "webhook 4xx; giving up"
                    );
                    return Ok(());
                }
                tracing::warn!(
                    status = %status,
                    attempt,
                    "webhook non-2xx response"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, attempt, "webhook transport error");
            }
        }
        if attempt >= config.max_retries {
            return Ok(());
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(15));
    }
}

/// `"sha256=" + hex(HMAC-SHA256(secret, body))` — same shape as GitHub.
pub fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac accepts any key len");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use chrono::Utc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Read one full HTTP/1.1 request (headers plus `Content-Length` body)
    /// from the socket. Shared by the probe servers below.
    async fn read_one_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        // Read until we've seen the header terminator and the full body.
        loop {
            let n = socket.read(&mut chunk).await.expect("read");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if let Some(header_end) = text.find("\r\n\r\n") {
                let content_len = text
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length: ")
                            .or_else(|| l.strip_prefix("Content-Length: "))
                    })
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= header_end + 4 + content_len {
                    break;
                }
            }
        }
        buf
    }

    /// Accept exactly one HTTP/1.1 request, return its raw bytes, and
    /// reply `200 OK`. Enough to assert on the delivered headers + body
    /// without pulling in an HTTP server dependency.
    async fn capture_one_request(listener: TcpListener) -> String {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let buf = read_one_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("write response");
        socket.flush().await.ok();
        String::from_utf8_lossy(&buf).to_string()
    }

    /// Serve requests sequentially, answering with each scripted status line
    /// in turn and `200 OK` once the script is exhausted. Every served
    /// request bumps `hits`, so tests can assert on exact attempt counts.
    async fn respond_with_statuses(
        listener: TcpListener,
        script: Vec<&'static str>,
        hits: Arc<AtomicUsize>,
    ) {
        let mut script = script.into_iter();
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            read_one_request(&mut socket).await;
            let status = script.next().unwrap_or("200 OK");
            let resp =
                format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            socket.write_all(resp.as_bytes()).await.ok();
            socket.flush().await.ok();
            hits.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Accept connections but never respond, parking the worker mid-delivery
    /// so the queue backs up deterministically. Sockets are held open for the
    /// life of the task so the client keeps waiting instead of erroring.
    async fn hang_forever(listener: TcpListener) {
        let mut held = Vec::new();
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            held.push(socket);
        }
    }

    /// Poll `cond` every 10ms until it holds, panicking after `deadline`.
    async fn wait_until(deadline: Duration, mut cond: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while !cond() {
            assert!(
                start.elapsed() < deadline,
                "condition not met within {deadline:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn published_event() -> WebhookEvent {
        WebhookEvent::ContextPublished {
            registry_authority: "registry.example.com".into(),
            registry_base_url: "https://registry.example.com".into(),
            ctx_id: "acdp://registry.example.com/abc".into(),
            lineage_id: "lin-1".into(),
            agent_id: "did:web:agent.example.com".into(),
            context_type: "analysis".into(),
            visibility: "public".into(),
            version: 1,
            created_at: Utc::now(),
            derived_from: Vec::new(),
            run_id: None,
            key_fingerprint: None,
            registry_receipt: None,
        }
    }

    #[tokio::test]
    async fn forwards_tenant_header_and_authority_in_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let capture = tokio::spawn(capture_one_request(listener));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 1,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        emitter.emit_with_tenant(published_event(), Some("tenant-x".into()));

        let raw = capture.await.expect("join");
        // Routing metadata travels as a header, not in the signed body.
        assert!(
            raw.contains("x-tenant-id: tenant-x") || raw.contains("X-Tenant-Id: tenant-x"),
            "expected X-Tenant-Id header, got:\n{raw}"
        );
        assert!(
            raw.contains("x-acdp-event: context.published")
                || raw.contains("X-ACDP-Event: context.published"),
            "expected X-ACDP-Event header, got:\n{raw}"
        );
        // Attribution fields land in the JSON body.
        assert!(
            raw.contains("\"registry_authority\":\"registry.example.com\""),
            "expected registry_authority in body, got:\n{raw}"
        );
        assert!(
            raw.contains("\"registry_base_url\":\"https://registry.example.com\""),
            "expected registry_base_url in body, got:\n{raw}"
        );
        // Tenant id must NOT pollute the signed body.
        assert!(
            !raw.contains("tenant-x\""),
            "tenant id leaked into JSON body:\n{raw}"
        );
        // REG-P2-6: envelope carries event_id + schema_version, and event_id
        // is echoed in a header for cheap dedup.
        assert!(
            raw.contains("\"schema_version\":\"1.0\""),
            "expected schema_version in body, got:\n{raw}"
        );
        assert!(
            raw.contains("\"event_id\":\""),
            "expected event_id in body, got:\n{raw}"
        );
        assert!(
            raw.to_ascii_lowercase().contains("x-acdp-event-id:"),
            "expected X-ACDP-Event-Id header, got:\n{raw}"
        );
    }

    #[tokio::test]
    async fn distinct_emits_get_distinct_event_ids() {
        fn event_id_of(raw: &str) -> String {
            let needle = "\"event_id\":\"";
            let start = raw.find(needle).expect("event_id present") + needle.len();
            let rest = &raw[start..];
            let end = rest.find('"').expect("event_id terminated");
            rest[..end].to_string()
        }
        async fn capture_once(emitter: &WebhookEmitter, addr: std::net::SocketAddr) -> String {
            let listener = TcpListener::bind(addr).await.expect("rebind");
            let cap = tokio::spawn(capture_one_request(listener));
            emitter.emit(published_event());
            cap.await.expect("join")
        }

        // First listener to learn a free port, then reuse it sequentially.
        let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = probe.local_addr().expect("addr");
        drop(probe);
        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 1,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        let a = event_id_of(&capture_once(&emitter, addr).await);
        let b = event_id_of(&capture_once(&emitter, addr).await);
        assert_ne!(a, b, "each emit must mint a fresh event_id");
    }

    // ── signature correctness ────────────────────────────────────────

    #[test]
    fn sign_produces_github_shaped_hmac() {
        let sig = sign("topsecret", b"hello world");
        // Shape: "sha256=" + 64 lowercase hex chars.
        let hex = sig.strip_prefix("sha256=").expect("sha256= prefix");
        assert_eq!(hex.len(), 64);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));

        // Recompute independently — the digest must match an HMAC-SHA256 over
        // the same (key, body), proving sign() isn't doing anything bespoke.
        let mut mac = HmacSha256::new_from_slice(b"topsecret").unwrap();
        mac.update(b"hello world");
        let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert_eq!(sig, expected);
    }

    #[test]
    fn sign_is_sensitive_to_body_and_secret() {
        let base = sign("k", b"body");
        assert_ne!(
            base,
            sign("k", b"body!"),
            "a changed body must change the sig"
        );
        assert_ne!(
            base,
            sign("k2", b"body"),
            "a changed secret must change the sig"
        );
    }

    #[tokio::test]
    async fn delivered_signature_authenticates_the_raw_body() {
        // The whole point of X-ACDP-Signature: a receiver recomputing
        // sign(secret, raw_body) gets exactly the header value. This is the
        // contract GitHub-compatible consumers rely on.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let capture = tokio::spawn(capture_one_request(listener));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 1,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        emitter.emit(published_event());

        let raw = capture.await.expect("join");
        let (head, body) = raw.split_once("\r\n\r\n").expect("header/body split");
        let sig_header = head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("x-acdp-signature:"))
            .and_then(|l| l.split_once(':').map(|(_, v)| v))
            .map(str::trim)
            .expect("X-ACDP-Signature header present");
        let recomputed = sign("shhh", body.as_bytes());
        assert_eq!(
            sig_header, recomputed,
            "receiver-side recomputation must match the transmitted signature"
        );
        // A receiver using the wrong secret must NOT validate.
        assert_ne!(sig_header, sign("wrong", body.as_bytes()));
    }

    // ── config validation (SEC-03 / SEC-04) ──────────────────────────

    fn cfg(url: &str, secret: &str, enabled: bool) -> WebhookConfig {
        WebhookConfig {
            enabled,
            url: url.into(),
            secret: secret.into(),
            timeout_seconds: 5,
            max_retries: 1,
            queue_capacity: 8,
        }
    }

    #[tokio::test]
    async fn try_spawn_rejects_empty_secret_when_enabled() {
        // SEC-04: HMAC over an empty key accepts every signature.
        let err = WebhookEmitter::try_spawn(cfg("https://hooks.example.com/acdp", "  ", true))
            .err()
            .expect("empty secret must be rejected");
        assert!(matches!(err, WebhookError::Config(_)), "got {err:?}");
        assert!(err.to_string().contains("secret"));
    }

    #[tokio::test]
    async fn try_spawn_rejects_ssrf_url() {
        // SEC-03: a URL resolving to an internal/IMDS address (or non-HTTPS)
        // is refused before the worker ever starts.
        let err = WebhookEmitter::try_spawn(cfg("http://169.254.169.254/latest", "secret", true))
            .err()
            .expect("SSRF-violating URL must be rejected");
        assert!(matches!(err, WebhookError::Config(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn try_spawn_skips_validation_when_disabled() {
        // A disabled webhook need not carry a secret or a vetted URL.
        assert!(WebhookEmitter::try_spawn(cfg("", "", false)).is_ok());
    }

    #[tokio::test]
    async fn omits_tenant_header_when_absent() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let capture = tokio::spawn(capture_one_request(listener));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 1,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        emitter.emit(published_event());

        let raw = capture.await.expect("join");
        assert!(
            !raw.to_ascii_lowercase().contains("x-tenant-id"),
            "did not expect X-Tenant-Id header, got:\n{raw}"
        );
    }

    // ── retry / backoff classification ───────────────────────────────

    #[tokio::test]
    async fn non_429_client_error_is_permanent_no_retry() {
        // A 4xx (other than 429) won't change on retry: the worker must give
        // up after exactly one attempt even with retries budgeted.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        tokio::spawn(respond_with_statuses(
            listener,
            vec!["404 Not Found"],
            Arc::clone(&hits),
        ));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 5,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        emitter.emit(published_event());

        wait_until(Duration::from_secs(5), || hits.load(Ordering::SeqCst) >= 1).await;
        // A (buggy) retry would land after the initial 250ms backoff; give it
        // 3x that and assert nothing else arrived.
        tokio::time::sleep(Duration::from_millis(750)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "4xx (non-429) must not be retried"
        );
    }

    #[tokio::test]
    async fn retries_429_and_5xx_until_success() {
        // 429 and 5xx are transient: the worker retries with backoff and the
        // event is eventually delivered. Script: 429 → 500 → 200, so the
        // success requires surviving both retryable classifications.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        tokio::spawn(respond_with_statuses(
            listener,
            vec!["429 Too Many Requests", "500 Internal Server Error"],
            Arc::clone(&hits),
        ));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 5,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        emitter.emit(published_event());

        // Backoff for the two retries is 250ms + 500ms; 10s is generous.
        wait_until(Duration::from_secs(10), || hits.load(Ordering::SeqCst) >= 3).await;
        // Brief settle window to catch a spurious 4th attempt after the 200.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            3,
            "expected exactly 429, 500, then a delivered 200"
        );
    }

    // ── queue backpressure & status ──────────────────────────────────

    #[tokio::test]
    async fn queue_full_drops_overflow_without_blocking() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(hang_forever(listener));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            // Long enough that the worker stays parked on the hung delivery
            // for the whole test.
            timeout_seconds: 30,
            max_retries: 1,
            queue_capacity: 1,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());

        // First event: the worker pulls it off the queue and parks on the
        // never-responding server.
        emitter.emit(published_event());
        wait_until(Duration::from_secs(5), || emitter.queue_status().0 == 0).await;

        // Second event fills the single-slot queue.
        emitter.emit(published_event());
        assert_eq!(emitter.queue_status(), (1, 1));

        // Everything past capacity must be dropped immediately: no blocking
        // of the (would-be) HTTP handler, no panic, no queue growth.
        let start = std::time::Instant::now();
        for _ in 0..64 {
            emitter.emit(published_event());
        }
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "emit must never block on a full queue"
        );
        assert_eq!(
            emitter.queue_status(),
            (1, 1),
            "overflow events must be dropped, not queued"
        );
    }

    #[tokio::test]
    async fn queue_status_reports_depth_and_capacity() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(hang_forever(listener));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 30,
            max_retries: 1,
            queue_capacity: 4,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());

        // Idle: nothing buffered, full capacity reported.
        assert_eq!(emitter.queue_status(), (0, 4));

        // Park the worker on the hung endpoint, then populate the queue:
        // the two buffered events are visible as in-flight depth.
        emitter.emit(published_event());
        wait_until(Duration::from_secs(5), || emitter.queue_status().0 == 0).await;
        emitter.emit(published_event());
        emitter.emit(published_event());
        assert_eq!(emitter.queue_status(), (2, 4));

        // queue_capacity: 0 is clamped to 1 so the channel can always hold
        // at least one event.
        let mut zero_cap = cfg("", "", false);
        zero_cap.queue_capacity = 0;
        let clamped =
            WebhookEmitter::spawn_with_policy(zero_cap, SsrfPolicy::allow_test_loopback());
        assert_eq!(clamped.queue_status(), (0, 1));
    }

    // ── wire-format invariants (#179) ────────────────────────────────────

    /// Every duplicated object key in `json`, as a dotted path
    /// (`"event_id"`, `"nested.a"`, `"arr[0].b"`).
    ///
    /// Deliberately not built on `serde_json::Value`: its map keeps only the
    /// last of a repeated key, so parsing into it cannot observe the very
    /// defect this looks for. Driving `MapAccess` directly sees each key as
    /// it appears in the byte stream. Substring counting is no good either —
    /// it would fire on any string *value* that happens to contain the text.
    fn duplicate_json_keys(json: &str) -> Vec<String> {
        use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
        use std::fmt;

        struct Scan<'a> {
            path: String,
            out: &'a mut Vec<String>,
        }

        impl<'de> DeserializeSeed<'de> for Scan<'_> {
            type Value = ();
            fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_any(self)
            }
        }

        impl<'de> Visitor<'de> for Scan<'_> {
            type Value = ();

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }

            fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
            where
                A: MapAccess<'de>,
            {
                let base = self.path;
                let out = self.out;
                let mut seen: Vec<String> = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    let path = if base.is_empty() {
                        key.clone()
                    } else {
                        format!("{base}.{key}")
                    };
                    if seen.contains(&key) {
                        out.push(path.clone());
                    } else {
                        seen.push(key);
                    }
                    // Required: `next_value_seed` is what consumes the `:`
                    // and the value. Skipping it desynchronises the parser.
                    map.next_value_seed(Scan {
                        path,
                        out: &mut *out,
                    })?;
                }
                Ok(())
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<(), A::Error>
            where
                A: SeqAccess<'de>,
            {
                let base = self.path;
                let out = self.out;
                let mut idx = 0usize;
                while seq
                    .next_element_seed(Scan {
                        path: format!("{base}[{idx}]"),
                        out: &mut *out,
                    })?
                    .is_some()
                {
                    idx += 1;
                }
                Ok(())
            }

            // `deserialize_any` dispatches scalars to these; the trait
            // defaults return `invalid_type` errors. JSON `null` arrives as
            // `visit_unit`, not `visit_none`.
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
                Ok(())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
                Ok(())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
                Ok(())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
                Ok(())
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<(), E> {
                Ok(())
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
                Ok(())
            }
        }

        let mut out = Vec::new();
        let mut de = serde_json::Deserializer::from_str(json);
        Scan {
            path: String::new(),
            out: &mut out,
        }
        .deserialize(&mut de)
        .expect("payload must be valid JSON");
        out
    }

    /// The detector is the whole basis of the duplicate-key guarantee, so it
    /// is itself under test — including the `serde_json::Value` blind spot
    /// that makes it necessary.
    #[test]
    fn duplicate_key_detector_sees_what_a_value_parse_hides() {
        let dup = r#"{"event_id":"envelope","type":"context_retracted",
                      "event_id":"lifecycle","nested":{"a":1,"a":2},
                      "arr":[{"b":1,"b":2}]}"#;

        let mut found = duplicate_json_keys(dup);
        found.sort();
        assert_eq!(
            found,
            vec![
                "arr[0].b".to_string(),
                "event_id".to_string(),
                "nested.a".to_string()
            ],
            "detector must report top-level, nested and in-array duplicates"
        );

        // Why the detector exists: a Value parse silently collapses all three.
        let value: serde_json::Value = serde_json::from_str(dup).expect("valid json");
        assert_eq!(
            value.as_object().expect("object").len(),
            4,
            "serde_json::Value is expected to drop the duplicate — if this \
             ever fails, Value could detect duplicates and the hand-rolled \
             scanner above could be replaced"
        );

        assert!(
            duplicate_json_keys(r#"{"event_id":"e","nested":{"a":1},"arr":[{"b":1}]}"#).is_empty(),
            "a clean payload must report no duplicates"
        );
    }

    /// Maps each variant to the snake_case `type` it serialises as.
    ///
    /// Wildcard-free on purpose: a sixth `WebhookEvent` variant must fail to
    /// compile here rather than silently escape the coverage below.
    fn variant_tag(event: &WebhookEvent) -> &'static str {
        match event {
            WebhookEvent::ContextPublished { .. } => "context_published",
            WebhookEvent::ContextRetrieved { .. } => "context_retrieved",
            WebhookEvent::ContextRetracted { .. } => "context_retracted",
            WebhookEvent::ContextRepublished { .. } => "context_republished",
            WebhookEvent::SearchExecuted { .. } => "search_executed",
        }
    }

    fn retrieved_event() -> WebhookEvent {
        WebhookEvent::ContextRetrieved {
            registry_authority: "registry.example.com".into(),
            ctx_id: "acdp://registry.example.com/abc".into(),
            requester_did: Some("did:web:reader.example.com".into()),
            at: Utc::now(),
        }
    }

    fn retracted_event(reason: Option<&str>) -> WebhookEvent {
        WebhookEvent::ContextRetracted {
            registry_authority: "registry.example.com".into(),
            ctx_id: "acdp://registry.example.com/abc".into(),
            lineage_id: "lin-1".into(),
            actor: "did:web:agent.example.com".into(),
            event_id: "01950000-0000-7000-8000-0000000000aa".into(),
            reason: reason.map(Into::into),
            at: Utc::now(),
        }
    }

    fn republished_event(reason: Option<&str>) -> WebhookEvent {
        WebhookEvent::ContextRepublished {
            registry_authority: "registry.example.com".into(),
            ctx_id: "acdp://registry.example.com/abc".into(),
            lineage_id: "lin-1".into(),
            actor: "did:web:agent.example.com".into(),
            event_id: "01950000-0000-7000-8000-0000000000bb".into(),
            reason: reason.map(Into::into),
            at: Utc::now(),
        }
    }

    fn search_event() -> WebhookEvent {
        WebhookEvent::SearchExecuted {
            registry_authority: "registry.example.com".into(),
            query: Some("weather".into()),
            result_count: 12,
            requester_did: None,
            at: Utc::now(),
        }
    }

    /// Every variant, with both `reason` states for the two that have one —
    /// `reason` is `skip_serializing_if`, so present and absent are different
    /// serialised shapes and both must be checked.
    fn every_wire_event() -> Vec<WebhookEvent> {
        vec![
            published_event(),
            retrieved_event(),
            retracted_event(Some("superseded by ctx_def")),
            retracted_event(None),
            republished_event(Some("retraction reversed")),
            republished_event(None),
            search_event(),
        ]
    }

    /// Emit one event through the real emitter and return the raw request.
    ///
    /// A fresh listener and a fresh emitter per call, rather than rebinding
    /// one address: `capture_one_request` answers without `Connection:
    /// close`, so the client may pool a socket the probe server has already
    /// dropped, and a reused ephemeral port can be taken in the gap. Both
    /// turn into a hang, and cargo has no per-test timeout — hence the
    /// explicit one here.
    async fn deliver_one(event: WebhookEvent) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let capture = tokio::spawn(capture_one_request(listener));

        let config = WebhookConfig {
            enabled: true,
            url: format!("http://{addr}/hook"),
            secret: "shhh".into(),
            timeout_seconds: 5,
            max_retries: 1,
            queue_capacity: 8,
        };
        let emitter = WebhookEmitter::spawn_with_policy(config, SsrfPolicy::allow_test_loopback());
        emitter.emit(event);

        tokio::time::timeout(Duration::from_secs(10), capture)
            .await
            .expect("webhook delivery timed out")
            .expect("join")
    }

    /// Case-insensitive lookup of one header value in a raw request head.
    fn header_value(head: &str, name: &str) -> Option<String> {
        head.lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_string())
    }

    /// #179 — the envelope serialises `event_id` and flattens the event, so a
    /// variant field of the same name emitted a second `event_id`. Receivers
    /// took last-wins or first-wins and one meaning was lost. No delivery may
    /// carry a repeated key, for any variant.
    #[tokio::test]
    async fn no_delivery_emits_a_duplicate_json_key() {
        let mut covered: Vec<&'static str> = Vec::new();

        for event in every_wire_event() {
            let tag = variant_tag(&event);
            let dotted = event.name();
            let raw = deliver_one(event).await;
            let (head, body) = raw.split_once("\r\n\r\n").expect("headers then body");

            let dups = duplicate_json_keys(body);
            assert!(
                dups.is_empty(),
                "{tag}: duplicate JSON key(s) {dups:?} on the wire:\n{body}"
            );

            let value: serde_json::Value = serde_json::from_str(body).expect("valid json body");
            assert_eq!(
                value["type"], tag,
                "{tag}: wrong type discriminator:\n{body}"
            );

            // The envelope's id is the delivery dedupe key, and it is what
            // the receiver must see in both places.
            // The dotted header name is a documented wire contract
            // (docs/WEBHOOKS.md) and is deliberately NOT the snake_case `type`
            // carried in the body. Nothing else asserts it.
            assert_eq!(
                header_value(head, "x-acdp-event").as_deref(),
                Some(dotted),
                "{tag}: X-ACDP-Event must carry the dotted event name"
            );

            let body_id = value["event_id"].as_str().expect("event_id in body");
            let header_id = header_value(head, "x-acdp-event-id").expect("X-ACDP-Event-Id header");
            assert_eq!(
                body_id, header_id,
                "{tag}: body event_id must be the envelope id from X-ACDP-Event-Id"
            );

            if !covered.contains(&tag) {
                covered.push(tag);
            }
        }

        covered.sort_unstable();
        assert_eq!(
            covered,
            vec![
                "context_published",
                "context_republished",
                "context_retracted",
                "context_retrieved",
                "search_executed"
            ],
            "all five variants must actually reach the wire in this test"
        );
    }

    /// The retract/republish lifecycle id still ships — under its own key, so
    /// both it and the envelope's dedupe id survive independently.
    #[tokio::test]
    async fn lifecycle_variants_carry_a_distinct_lifecycle_event_id() {
        for (event, minted) in [
            (
                retracted_event(Some("superseded")),
                "01950000-0000-7000-8000-0000000000aa",
            ),
            (
                republished_event(None),
                "01950000-0000-7000-8000-0000000000bb",
            ),
        ] {
            let tag = variant_tag(&event);
            let raw = deliver_one(event).await;
            let (_, body) = raw.split_once("\r\n\r\n").expect("headers then body");
            let value: serde_json::Value = serde_json::from_str(body).expect("valid json body");

            assert_eq!(
                value["lifecycle_event_id"].as_str(),
                Some(minted),
                "{tag}: actor-minted lifecycle id must ship as lifecycle_event_id:\n{body}"
            );
            assert_ne!(
                value["lifecycle_event_id"].as_str(),
                value["event_id"].as_str(),
                "{tag}: lifecycle id and envelope dedupe id must stay distinct"
            );
        }
    }

    /// `reason` is `skip_serializing_if = "Option::is_none"` — absent means
    /// the key is omitted, never `null`.
    #[tokio::test]
    async fn omitted_reason_is_absent_not_null() {
        for event in [retracted_event(None), republished_event(None)] {
            let tag = variant_tag(&event);
            let raw = deliver_one(event).await;
            let (_, body) = raw.split_once("\r\n\r\n").expect("headers then body");
            let value: serde_json::Value = serde_json::from_str(body).expect("valid json body");
            assert!(
                value.get("reason").is_none(),
                "{tag}: reason must be omitted, not null:\n{body}"
            );
        }
    }

    /// `#[serde(rename)]` is symmetric — it moves the `Deserialize` side too.
    /// That is deliberate, and this asserts it rather than arguing it, because
    /// both alternatives fail quietly:
    ///
    /// - `rename(serialize = ...)` would read `event_id` — a key still present
    ///   on the wire, holding the *envelope delivery id* — straight into the
    ///   lifecycle field. No error, wrong value: the exact `event_id` confusion
    ///   #179 removes, resurrected on the read side.
    /// - `alias = "event_id"` maps both names to one field slot, so serde
    ///   rejects every current body with a duplicate-field error.
    ///
    /// Nothing else in the workspace deserialises this type, so without this
    /// the read path is an argument instead of a guarantee.
    #[tokio::test]
    async fn the_emitted_body_round_trips_into_the_lifecycle_field() {
        let minted = "01950000-0000-7000-8000-0000000000aa";
        let raw = deliver_one(retracted_event(Some("superseded"))).await;
        let (_, body) = raw.split_once("\r\n\r\n").expect("headers then body");

        let value: serde_json::Value = serde_json::from_str(body).expect("valid json body");
        let envelope_id = value["event_id"]
            .as_str()
            .expect("envelope event_id")
            .to_owned();

        let parsed: WebhookEvent = serde_json::from_str(body).expect("body must deserialise");
        match parsed {
            WebhookEvent::ContextRetracted { event_id, .. } => {
                assert_eq!(
                    event_id, minted,
                    "the read side must pick up lifecycle_event_id, not the envelope key"
                );
                assert_ne!(
                    event_id, envelope_id,
                    "reading the envelope delivery id into the lifecycle field is the bug"
                );
            }
            other => panic!("deserialised as the wrong variant: {}", variant_tag(&other)),
        }
    }
}
