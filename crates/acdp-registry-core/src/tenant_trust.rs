//! Who may select a tenant with `X-Tenant-Id` (#374).
//!
//! RFC-ACDP-0008 §6.4: a registry MUST NOT trust an unauthenticated tenant
//! indicator unless a boundary the client cannot cross vouches for it. The
//! registry cannot see that boundary itself, so the operator declares it with
//! `[auth] tenant_header_trust`, and this module answers one question per
//! request: did the header arrive across the declared boundary?
//!
//! The answer is keyed on the **immediate TCP peer** — the socket address the
//! connection came from — never on `X-Forwarded-For`, which the client
//! controls. A request with no recorded peer is untrusted under
//! `trusted_proxies`; it does NOT fall back to the `/auth/*` limiter's
//! `0.0.0.0` shared-bucket key, which would be a trust grant by accident.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use acdp_registry_types::TenantHeaderTrust;
use axum::extract::{ConnectInfo, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;

use crate::rate_limit::{canonical_ip, TrustedProxies};

/// The immediate TCP peer of a request, canonicalised (IPv4-mapped IPv6 →
/// IPv4). `None` when the connection recorded no peer address.
///
/// Stamped onto every request by [`stamp_peer_ip`]; as an extractor it reads
/// that stamp and never fails — a missing stamp yields `PeerIp(None)`, which
/// the trust test treats as untrusted, so a route mounted without the
/// middleware fails closed rather than 500ing or trusting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeerIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for PeerIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<PeerIp>()
            .copied()
            .unwrap_or_default())
    }
}

/// Middleware: record the immediate TCP peer as a [`PeerIp`] extension.
///
/// Applied to the whole router in `build_router`, so every handler that
/// resolves a tenant (contexts, log, admin) sees it. Reads only
/// `ConnectInfo<SocketAddr>`, which `into_make_service_with_connect_info`
/// inserts per connection; no header contributes.
pub async fn stamp_peer_ip(mut req: Request, next: Next) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| canonical_ip(ci.0.ip()));
    req.extensions_mut().insert(PeerIp(peer));
    next.run(req).await
}

/// Is an `X-Tenant-Id` header from `peer` trusted under `mode`?
///
/// * `none` — never.
/// * `trusted_proxies` — only when the peer is known and inside `proxies`.
///   An unknown peer is untrusted, and so is every peer when `proxies` is
///   empty (startup refuses that combination; this is the backstop).
/// * `any_peer` — always: the operator asserts a boundary the registry
///   cannot observe.
pub fn tenant_header_trusted(
    mode: TenantHeaderTrust,
    peer: Option<IpAddr>,
    proxies: &TrustedProxies,
) -> bool {
    match mode {
        TenantHeaderTrust::None => false,
        TenantHeaderTrust::AnyPeer => true,
        TenantHeaderTrust::TrustedProxies => {
            peer.is_some_and(|ip| proxies.contains(canonical_ip(ip)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxies(entries: &[&str]) -> TrustedProxies {
        TrustedProxies::parse(&entries.iter().map(|e| e.to_string()).collect::<Vec<_>>()).unwrap()
    }

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn none_never_trusts() {
        let p = proxies(&["10.0.0.0/8"]);
        for peer in [ip("10.1.2.3"), ip("203.0.113.9"), ip("127.0.0.1"), None] {
            assert!(!tenant_header_trusted(TenantHeaderTrust::None, peer, &p));
        }
    }

    #[test]
    fn any_peer_always_trusts_even_without_a_recorded_peer() {
        let empty = TrustedProxies::default();
        for peer in [ip("10.1.2.3"), ip("203.0.113.9"), None] {
            assert!(tenant_header_trusted(
                TenantHeaderTrust::AnyPeer,
                peer,
                &empty
            ));
        }
    }

    #[test]
    fn trusted_proxies_trusts_exactly_the_listed_peers() {
        let p = proxies(&["10.0.0.0/8", "2001:db8::/32"]);
        let m = TenantHeaderTrust::TrustedProxies;
        assert!(tenant_header_trusted(m, ip("10.1.2.3"), &p), "in CIDR");
        assert!(
            tenant_header_trusted(m, ip("2001:db8::1"), &p),
            "in v6 CIDR"
        );
        assert!(
            tenant_header_trusted(m, ip("::ffff:10.1.2.3"), &p),
            "an IPv4-mapped peer is canonicalised before the CIDR test"
        );
        assert!(!tenant_header_trusted(m, ip("11.0.0.1"), &p), "out of CIDR");
        assert!(
            !tenant_header_trusted(m, ip("2001:db9::1"), &p),
            "out of v6 CIDR"
        );
    }

    #[test]
    fn trusted_proxies_distrusts_an_unknown_peer_and_an_empty_list() {
        let m = TenantHeaderTrust::TrustedProxies;
        // No ConnectInfo: untrusted, never the limiter's 0.0.0.0 fallback —
        // not even when 0.0.0.0/0 is (absurdly) listed.
        assert!(!tenant_header_trusted(m, None, &proxies(&["0.0.0.0/0"])));
        assert!(!tenant_header_trusted(
            m,
            ip("10.1.2.3"),
            &TrustedProxies::default()
        ));
    }

    #[tokio::test]
    async fn the_extractor_defaults_to_an_unknown_peer() {
        let (mut parts, ()) = axum::http::Request::new(()).into_parts();
        let PeerIp(peer) = PeerIp::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(peer, None, "no stamp → unknown peer → untrusted");
        parts.extensions.insert(PeerIp(ip("10.0.0.1")));
        let PeerIp(peer) = PeerIp::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(peer, ip("10.0.0.1"));
    }
}
