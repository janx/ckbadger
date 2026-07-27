use std::sync::LazyLock;
use std::time::Duration;

/// Shared HTTP client for all RPC calls in the API crate.
///
/// Using a single `reqwest::Client` allows connection pooling and avoids
/// creating a new client (and TLS context) on every request.
static SHARED_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

/// Return a reference to the shared HTTP client.
pub fn shared_http_client() -> &'static reqwest::Client {
    &SHARED_CLIENT
}

/// How long the frontend proxy waits for a co-located `ckbadger-api` to accept
/// the connection. Loopback either answers immediately or is not there.
const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the frontend proxy tolerates *silence* from an upstream.
///
/// This bounds the idle gap between reads (including the wait for the response
/// head), not the total transfer, so an arbitrarily long streaming response
/// stays safe while a wedged upstream is released instead of pinning the
/// connection forever.
const PROXY_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Dedicated HTTP client for the frontend reverse proxy.
///
/// Deliberately separate from [`shared_http_client`]: that one serves CKB
/// JSON-RPC calls whose timeout policy is a different concern, whereas the
/// proxy relays browser traffic and must never let one stalled backend hold a
/// browser connection (and a pooled socket) open indefinitely.
static PROXY_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(PROXY_CONNECT_TIMEOUT)
        .read_timeout(PROXY_READ_TIMEOUT)
        .build()
        .expect("proxy HTTP client with static timeouts")
});

/// Return a reference to the frontend reverse-proxy HTTP client.
pub fn proxy_http_client() -> &'static reqwest::Client {
    &PROXY_CLIENT
}
