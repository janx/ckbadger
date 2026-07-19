use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use governor::{clock::DefaultClock, Quota, RateLimiter};
use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
    task::{Context, Poll},
};
use tower::{Layer, Service};

pub struct RateLimitError;

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": "Too many requests",
            "code": "RATE_LIMIT_EXCEEDED",
            "retry_after_seconds": 1
        });

        (
            StatusCode::TOO_MANY_REQUESTS,
            [("content-type", "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}

#[derive(Clone)]
pub struct IpRateLimitLayer {
    requests_per_second: u32,
    burst_size: u32,
}

impl IpRateLimitLayer {
    pub fn new(requests_per_second: u32, burst_size: u32) -> Self {
        Self {
            requests_per_second,
            burst_size,
        }
    }
}

use governor::state::keyed::DashMapStateStore;

type IpLimiter = RateLimiter<IpAddr, DashMapStateStore<IpAddr>, DefaultClock>;

impl<S> Layer<S> for IpRateLimitLayer {
    type Service = IpRateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let requests_per_second = self.requests_per_second.max(1);
        let burst_size = self.burst_size.max(1);
        let quota = Quota::per_second(
            NonZeroU32::new(requests_per_second).expect("max(1) ensures non-zero"),
        )
        .allow_burst(NonZeroU32::new(burst_size).expect("max(1) ensures non-zero"));

        let limiter = Arc::new(RateLimiter::dashmap(quota));

        IpRateLimitService { inner, limiter }
    }
}

#[derive(Clone)]
pub struct IpRateLimitService<S> {
    inner: S,
    limiter: Arc<IpLimiter>,
}

impl<S> Service<Request<Body>> for IpRateLimitService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();

        let ip = extract_client_ip(&req);

        Box::pin(async move {
            let ip = ip.unwrap_or(IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
            // Skip rate limiting for loopback (localhost) requests
            if !ip.is_loopback() && limiter.check_key(&ip).is_err() {
                return Ok(RateLimitError.into_response());
            }
            inner.call(req).await
        })
    }
}

fn extract_client_ip<B>(req: &Request<B>) -> Option<IpAddr> {
    let peer_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());

    // Forwarded addresses are trusted only across the local frontend→API hop.
    // A remote socket peer is authoritative and cannot claim loopback through a
    // caller-controlled header.
    match peer_ip {
        Some(ip) if !ip.is_loopback() => return Some(ip),
        None => return None,
        Some(_) => {}
    }

    let forwarded = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse().ok())
        });

    forwarded.or(peer_ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::ConnectInfo;

    fn request(peer: &str, forwarded: &str) -> Request<()> {
        let mut request = Request::builder()
            .header("x-forwarded-for", forwarded)
            .body(())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            peer.parse::<SocketAddr>().expect("valid test peer"),
        ));
        request
    }

    #[test]
    fn direct_remote_peer_cannot_spoof_forwarded_loopback() {
        let request = request("203.0.113.10:8080", "127.0.0.1");
        assert_eq!(
            extract_client_ip(&request),
            Some("203.0.113.10".parse().unwrap())
        );
    }

    #[test]
    fn local_frontend_proxy_can_forward_remote_peer() {
        let request = request("127.0.0.1:8080", "203.0.113.11");
        assert_eq!(
            extract_client_ip(&request),
            Some("203.0.113.11".parse().unwrap())
        );
    }

    #[test]
    fn forwarding_header_without_socket_peer_is_not_trusted() {
        let request = Request::builder()
            .header("x-forwarded-for", "127.0.0.1")
            .body(())
            .unwrap();
        assert_eq!(extract_client_ip(&request), None);
    }
}
