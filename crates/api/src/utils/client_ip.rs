use std::net::IpAddr;

use axum::http::HeaderMap;

/// Resolve the originating client at a local reverse-proxy trust boundary.
///
/// A non-loopback socket peer is authoritative: its forwarding headers are
/// caller-controlled and ignored. A loopback peer is a trusted local proxy hop,
/// so the first valid `X-Forwarded-For` address (or `X-Real-IP`) identifies the
/// remote client. Supplying no socket peer never makes headers trustworthy.
pub(crate) fn resolve_client_ip(peer_ip: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
    match peer_ip {
        Some(ip) if !ip.is_loopback() => return Some(ip),
        None => return None,
        Some(_) => {}
    }

    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.trim().parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse().ok())
        });

    forwarded.or(peer_ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(forwarded: &str, real: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", forwarded.parse().unwrap());
        headers.insert("x-real-ip", real.parse().unwrap());
        headers
    }

    #[test]
    fn direct_remote_peer_is_authoritative() {
        let resolved = resolve_client_ip(
            Some("203.0.113.10".parse().unwrap()),
            &headers("127.0.0.1", "127.0.0.1"),
        );
        assert_eq!(resolved, Some("203.0.113.10".parse().unwrap()));
    }

    #[test]
    fn local_proxy_uses_the_first_forwarded_address() {
        let resolved = resolve_client_ip(
            Some("127.0.0.1".parse().unwrap()),
            &headers("203.0.113.11, 127.0.0.1", "203.0.113.12"),
        );
        assert_eq!(resolved, Some("203.0.113.11".parse().unwrap()));
    }

    #[test]
    fn headers_without_a_socket_peer_are_untrusted() {
        assert_eq!(
            resolve_client_ip(None, &headers("203.0.113.11", "203.0.113.12")),
            None
        );
    }
}
