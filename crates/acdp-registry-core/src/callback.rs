//! Callback-URL resolution for RFC-ACDP-0018 §8 subscriptions.
//!
//! A `callback_url` is checked against the RFC-ACDP-0006 §7 SSRF policy at
//! creation and again before every delivery. Both checks resolve the host
//! through a [`CallbackResolver`] and refuse the whole answer if any address
//! is outside public unicast (`acdp::safe_http::reject_if_any_forbidden`).
//! The resolver is a trait so tests can make a public-looking host resolve
//! to a private address without touching real DNS.

use std::net::SocketAddr;
use std::time::Duration;

use acdp::error::AcdpError;
use async_trait::async_trait;

/// Resolves a callback host to the addresses a connection would use.
#[async_trait]
pub trait CallbackResolver: Send + Sync {
    /// Every address `host:port` resolves to. An empty answer is not an
    /// error here; callers refuse it.
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, AcdpError>;
}

/// Bound on one callback-host lookup: a subscriber's own nameserver that
/// never answers must not hold a request or a delivery slot.
pub const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Resolve `host:port` within `timeout` (production: [`LOOKUP_TIMEOUT`]) and refuse the whole
/// answer if it is empty or any address is outside what `policy` allows. The `Err` detail names the
/// offending address: log it, never send it to a client.
pub async fn resolve_allowed(
    policy: &acdp::safe_http::SsrfPolicy,
    resolver: &dyn CallbackResolver,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<Vec<SocketAddr>, String> {
    let addrs = tokio::time::timeout(timeout, resolver.lookup(host, port))
        .await
        .map_err(|_| format!("DNS lookup for '{host}' timed out"))?
        .map_err(|e| e.to_string())?;
    if addrs.is_empty() {
        return Err(format!("'{host}' does not resolve"));
    }
    acdp::safe_http::reject_if_any_forbidden(policy, host, &addrs).map_err(|e| e.to_string())?;
    Ok(addrs)
}

/// The system resolver (`getaddrinfo` through tokio).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemResolver;

#[async_trait]
impl CallbackResolver for SystemResolver {
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, AcdpError> {
        tokio::net::lookup_host((host, port))
            .await
            .map(Iterator::collect)
            .map_err(|e| AcdpError::Http(format!("DNS lookup for '{host}' failed: {e}")))
    }
}

/// A fixed host → addresses table, for tests. A host not in the table
/// fails to resolve.
#[derive(Debug, Default, Clone)]
pub struct StaticResolver {
    entries: std::collections::HashMap<String, Vec<std::net::IpAddr>>,
}

impl StaticResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make `host` resolve to `addrs`.
    pub fn with(mut self, host: &str, addrs: &[std::net::IpAddr]) -> Self {
        self.entries
            .insert(host.to_ascii_lowercase(), addrs.to_vec());
        self
    }
}

#[async_trait]
impl CallbackResolver for StaticResolver {
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, AcdpError> {
        self.entries
            .get(&host.to_ascii_lowercase())
            .map(|ips| ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect())
            .ok_or_else(|| AcdpError::Http(format!("DNS lookup for '{host}' failed: no such host")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn static_resolver_answers_only_its_table() {
        let r = StaticResolver::new().with("Hooks.Example.com", &["203.0.113.7".parse().unwrap()]);
        let got = r.lookup("hooks.example.com", 8443).await.unwrap();
        assert_eq!(got, vec!["203.0.113.7:8443".parse().unwrap()]);
        assert!(r.lookup("other.example.com", 443).await.is_err());
    }

    #[tokio::test]
    async fn system_resolver_resolves_localhost() {
        let got = SystemResolver.lookup("localhost", 443).await.unwrap();
        assert!(got.iter().all(|a| a.ip().is_loopback() && a.port() == 443));
        assert!(!got.is_empty());
    }
}
