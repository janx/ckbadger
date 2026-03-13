use std::sync::LazyLock;

/// Shared HTTP client for all RPC calls in the API crate.
///
/// Using a single `reqwest::Client` allows connection pooling and avoids
/// creating a new client (and TLS context) on every request.
static SHARED_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

/// Return a reference to the shared HTTP client.
pub fn shared_http_client() -> &'static reqwest::Client {
    &SHARED_CLIENT
}
