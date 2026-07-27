//! Network-aware reverse proxy for the shared frontend server.
//!
//! The shared frontend serves multiple CKB networks (mainnet, testnet, …) from a
//! single origin. Each network runs its own read-only `ckbadger-api` server on a
//! distinct local port, and this module routes network-prefixed requests to the
//! matching backend:
//!
//! - `<method> /api/{network}/v1/{*rest}` → `http://127.0.0.1:{port}/api/v1/{rest}`
//!   (both request and response bodies are streamed via reqwest, so large
//!   payloads never buffer fully inside the proxy).
//! - `GET /ws/{network}` → `ws://127.0.0.1:{port}/ws` (bidirectional frame relay
//!   via tokio-tungstenite).
//!
//! Upstream targets match how `ckbadger-api` mounts itself: the API nests its
//! routes under `/api/v1` and exposes the socket at `/ws`.
//!
//! The per-network port map lives in [`ProxyState`]. An unknown network fails
//! fast with an actionable `404` rather than silently misrouting.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{
    CloseFrame as AxCloseFrame, Message as AxMessage, WebSocket, WebSocketUpgrade,
};
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Response as TgHandshakeResponse;
use tokio_tungstenite::tungstenite::protocol::CloseFrame as TgCloseFrame;
use tokio_tungstenite::tungstenite::{Error as TgError, Message as TgMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::response::ApiError;

/// The upstream path prefix every proxied API request must stay inside.
///
/// `/api/{network}/v1/{*rest}` maps onto `/api/v1/{rest}`, so a request that
/// ends up outside this prefix has escaped the advertised contract.
const UPSTREAM_API_PREFIX: &str = "/api/v1/";

/// Per-network backend routing table: network name → local `ckbadger-api` port,
/// plus the HTTP client used to reach those backends.
pub struct ProxyState {
    ports: HashMap<String, u16>,
    client: reqwest::Client,
}

impl ProxyState {
    /// Routing table wired to the process-wide proxy HTTP client (production).
    pub fn new(ports: HashMap<String, u16>) -> Self {
        Self {
            ports,
            client: crate::utils::http::proxy_http_client().clone(),
        }
    }

    /// Same routing table with a caller-supplied client. Exists so tests can
    /// exercise the timeout paths on a budget measured in milliseconds instead
    /// of the production minute-scale one.
    pub fn with_client(ports: HashMap<String, u16>, client: reqwest::Client) -> Self {
        Self { ports, client }
    }

    fn upstream_port(&self, network: &str) -> Option<u16> {
        self.ports.get(network).copied()
    }
}

/// Build the proxy sub-router carrying its own [`ProxyState`].
///
/// `.with_state` erases the state type back to `Router<()>`, so the returned
/// router can be `.merge`d into the stateless frontend router ahead of the SPA
/// fallback (the fallback then only catches non-`/api`/`/ws` paths).
pub fn proxy_router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/api/{network}/v1/{*rest}", any(proxy_api))
        .route("/ws/{network}", get(proxy_ws))
        .with_state(state)
}

/// Headers that must not be copied verbatim when re-framing a proxied message.
///
/// The standard hop-by-hop headers, plus `content-length`: both the forwarded
/// request body and the returned response body are re-wrapped as fresh streams,
/// so any inherited `content-length`/`transfer-encoding` would contradict the new
/// framing. reqwest (outbound) and axum (inbound) set correct framing themselves.
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

fn is_client_forwarding_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("x-forwarded-for") || name.eq_ignore_ascii_case("x-real-ip")
}

/// Slice the still-percent-encoded `{*rest}` tail out of the raw request path.
///
/// axum's `Path` extractor percent-decodes its captures. That is correct for a
/// routing key like `{network}`, but lossy for a path that is immediately
/// re-serialized into an upstream URL: `..%2F` would decode to `../` (which the
/// URL parser then normalizes *out* of the `/api/v1` prefix) and an encoded
/// `%2F` inside a parameter would reach the upstream as a real separator,
/// resolving to a different endpoint than a direct API call. The raw path is the
/// authoritative wire form, so the tail is taken from it verbatim.
///
/// Returns `None` when the path is not the `/api/{network}/v1/…` shape the route
/// pattern guarantees.
fn raw_rest_path(path: &str) -> Option<&str> {
    let after_api = path.strip_prefix("/api/")?;
    let (_network, tail) = after_api.split_once('/')?;
    tail.strip_prefix("v1/")
}

/// Forward `/api/{network}/v1/{*rest}` (with query string) to the network's API
/// server at `http://127.0.0.1:{port}/api/v1/{rest}`.
///
/// Both directions stream: the request body is wrapped with
/// [`reqwest::Body::wrap_stream`] and the upstream response is returned via
/// [`Body::from_stream`], so neither is fully buffered in the proxy.
pub async fn proxy_api(
    State(state): State<Arc<ProxyState>>,
    // Only `{network}` is read from the decoded extractor: it is a routing key,
    // so percent-decoding is exactly right there. The `{*rest}` tail is re-read
    // RAW from the URI below and must stay encoded end to end.
    Path((network, _decoded_rest)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some(port) = state.upstream_port(&network) else {
        return unknown_network(&network, &state);
    };

    let (parts, body) = req.into_parts();
    let Some(peer) = parts.extensions.get::<ConnectInfo<SocketAddr>>() else {
        return proxy_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "proxy client address unavailable".to_string(),
        );
    };
    let client_ip = peer.0.ip().to_string();
    let Some(rest) = raw_rest_path(parts.uri.path()) else {
        return proxy_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!(
                "proxy route matched but path '{}' is not /api/{{network}}/v1/…",
                parts.uri.path()
            ),
        );
    };
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let Ok(url) = reqwest::Url::parse(&format!(
        "http://127.0.0.1:{port}{UPSTREAM_API_PREFIX}{rest}{query}"
    )) else {
        return proxy_error(
            StatusCode::BAD_REQUEST,
            "invalid_path",
            format!(
                "request path is not a valid upstream URL under {UPSTREAM_API_PREFIX}: {}",
                parts.uri
            ),
        );
    };
    // A literal `../` tail is normalized away while the URL is parsed. Anything
    // that lands outside the prefix would silently reach an unrelated upstream
    // route, so refuse it instead of forwarding a request the mapping never
    // promised to serve.
    if !url.path().starts_with(UPSTREAM_API_PREFIX) {
        return proxy_error(
            StatusCode::BAD_REQUEST,
            "invalid_path",
            format!(
                "request path escapes the {UPSTREAM_API_PREFIX} prefix: {}",
                parts.uri.path()
            ),
        );
    }

    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    let mut builder = state.client.request(parts.method, url).body(reqwest_body);
    for (name, value) in parts.headers.iter() {
        if is_hop_by_hop(name.as_str()) || is_client_forwarding_header(name.as_str()) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder = builder
        .header("x-forwarded-for", &client_ip)
        .header("x-real-ip", &client_ip);

    match builder.send().await {
        Ok(upstream) => {
            let mut resp = Response::builder().status(upstream.status());
            for (name, value) in upstream.headers().iter() {
                if is_hop_by_hop(name.as_str()) {
                    continue;
                }
                resp = resp.header(name, value);
            }
            // Infallible: the status and header values were just validated by
            // reqwest while parsing the upstream response.
            resp.body(Body::from_stream(upstream.bytes_stream()))
                .expect("proxied response builder with validated status/headers")
        }
        // A stalled upstream is a different failure from an absent one: the
        // backend is up, it just stopped talking. Report it as such so the cause
        // is visible in the browser and in logs.
        Err(e) if e.is_timeout() => proxy_error(
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            format!("upstream network '{network}' timed out: {e}"),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("upstream network '{network}' unreachable: {e}"),
        )
            .into_response(),
    }
}

/// Relay `/ws/{network}` to the network's API server at `ws://127.0.0.1:{port}/ws`.
///
/// The upstream socket is opened *before* the client's own upgrade is completed.
/// An upstream that refuses the handshake — the pre-sync `503 initializing`
/// router, the WS connection cap, a backend that is not there — is then answered
/// as a real HTTP status on the client's upgrade request, instead of being
/// flattened into a post-upgrade `Close(1011, "upstream unreachable")` that
/// asserts one cause for every failure and hides the machine-readable code the
/// SPA keys its initializing UX on.
pub async fn proxy_ws(
    State(state): State<Arc<ProxyState>>,
    Path(network): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(port) = state.upstream_port(&network) else {
        return unknown_network(&network, &state);
    };

    match connect_upstream_ws(port, peer.ip()).await {
        Ok(upstream) => ws.on_upgrade(move |client| relay_ws(client, upstream)),
        Err(TgError::Http(rejection)) => upstream_handshake_rejection(&network, rejection),
        Err(e) => proxy_error(
            StatusCode::BAD_GATEWAY,
            "upstream_unreachable",
            format!("upstream network '{network}' websocket unreachable: {e}"),
        ),
    }
}

/// The upstream socket type produced by [`tokio_tungstenite::connect_async`].
type UpstreamWs = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Open the upstream WebSocket, forwarding the client's identity.
///
/// Without these headers the upgrade reaches the API from the loopback proxy
/// peer, and `extract_client_ip` treats a loopback peer with no forwarded
/// address as local — so the rate limiter skips it and every proxied socket is
/// effectively un-limited. The HTTP path already forwards them; this mirrors it.
async fn connect_upstream_ws(port: u16, client_ip: IpAddr) -> Result<UpstreamWs, TgError> {
    let mut request = format!("ws://127.0.0.1:{port}/ws").into_client_request()?;
    let client_ip = HeaderValue::from_str(&client_ip.to_string())
        .expect("an IP address is always a valid header value");
    request
        .headers_mut()
        .insert("x-forwarded-for", client_ip.clone());
    request.headers_mut().insert("x-real-ip", client_ip);

    let (upstream, _response) = tokio_tungstenite::connect_async(request).await?;
    Ok(upstream)
}

/// Re-emit an upstream handshake rejection on the client's own upgrade request.
///
/// The upstream body is forwarded verbatim whenever it has one: the API's
/// pre-sync router answers `{"error":"initializing", …}` and the WS connection
/// cap answers its own message, so propagating beats guessing which flavour of
/// rejection this was. An empty body becomes the proxy's own `{error, message}`
/// payload so a client never has to interpret a bodyless 5xx.
fn upstream_handshake_rejection(network: &str, rejection: TgHandshakeResponse) -> Response {
    let status = rejection.status();
    let content_type = rejection.headers().get(header::CONTENT_TYPE).cloned();

    match rejection.into_body() {
        Some(body) if !body.is_empty() => {
            let mut response = Response::builder().status(status);
            if let Some(content_type) = content_type {
                response = response.header(header::CONTENT_TYPE, content_type);
            }
            // Infallible: the status and header value both came from a response
            // tungstenite already parsed.
            response
                .body(Body::from(body))
                .expect("upstream handshake rejection with validated status/headers")
        }
        _ => proxy_error(
            status,
            "upstream_rejected",
            format!("upstream network '{network}' rejected the websocket handshake with {status}"),
        ),
    }
}

/// Pump frames in both directions between the browser client and the upstream API
/// WebSocket until either side closes or errors.
async fn relay_ws(client: WebSocket, upstream: UpstreamWs) {
    let (mut up_tx, mut up_rx) = upstream.split();
    let (mut cl_tx, mut cl_rx) = client.split();

    let client_to_upstream = async {
        while let Some(Ok(msg)) = cl_rx.next().await {
            if up_tx.send(axum_msg_to_tungstenite(msg)).await.is_err() {
                break;
            }
        }
    };
    let upstream_to_client = async {
        while let Some(Ok(msg)) = up_rx.next().await {
            if let Some(m) = tungstenite_msg_to_axum(msg) {
                if cl_tx.send(m).await.is_err() {
                    break;
                }
            }
        }
    };

    tokio::select! {
        _ = client_to_upstream => {}
        _ = upstream_to_client => {}
    }
}

/// Emit a proxy-level failure using the API's `{error, message}` JSON contract.
///
/// The SPA json-parses every failed response and discards anything that is not
/// JSON, so a plain-text body would collapse an actionable message into a bare
/// "API error: <status>" in the UI.
fn proxy_error(status: StatusCode, code: &'static str, message: String) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// Actionable `404` for a network absent from the routing table. Lists the known
/// networks so the caller can see the valid choices immediately.
fn unknown_network(network: &str, state: &ProxyState) -> Response {
    let mut known: Vec<&str> = state.ports.keys().map(String::as_str).collect();
    known.sort_unstable();
    (
        StatusCode::NOT_FOUND,
        format!(
            "unknown network '{network}'; known networks: [{}]",
            known.join(", ")
        ),
    )
        .into_response()
}

/// Convert an axum client frame into the tungstenite frame sent upstream.
///
/// axum 0.8 models payloads as `Utf8Bytes`/`Bytes` and close codes as `u16`;
/// tungstenite 0.24 uses `String`/`Vec<u8>` and a `CloseCode` enum, so each
/// variant is re-boxed explicitly.
fn axum_msg_to_tungstenite(msg: AxMessage) -> TgMessage {
    match msg {
        AxMessage::Text(text) => TgMessage::Text(text.as_str().to_owned()),
        AxMessage::Binary(data) => TgMessage::Binary(data.to_vec()),
        AxMessage::Ping(data) => TgMessage::Ping(data.to_vec()),
        AxMessage::Pong(data) => TgMessage::Pong(data.to_vec()),
        AxMessage::Close(frame) => TgMessage::Close(frame.map(|f| TgCloseFrame {
            code: f.code.into(),
            reason: f.reason.as_str().to_owned().into(),
        })),
    }
}

/// Convert an upstream tungstenite frame into the axum frame sent to the client.
///
/// Raw `Frame` variants are dropped (`None`) as recommended by the tungstenite
/// maintainers — they never surface through the high-level message stream.
fn tungstenite_msg_to_axum(msg: TgMessage) -> Option<AxMessage> {
    match msg {
        TgMessage::Text(text) => Some(AxMessage::Text(text.into())),
        TgMessage::Binary(data) => Some(AxMessage::Binary(data.into())),
        TgMessage::Ping(data) => Some(AxMessage::Ping(data.into())),
        TgMessage::Pong(data) => Some(AxMessage::Pong(data.into())),
        TgMessage::Close(frame) => Some(AxMessage::Close(frame.map(|f| AxCloseFrame {
            code: f.code.into(),
            reason: f.reason.into_owned().into(),
        }))),
        TgMessage::Frame(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderMap, Uri};
    use axum::routing::post;
    use http_body_util::BodyExt;
    use std::net::SocketAddr;
    use tower::ServiceExt; // for `oneshot`

    /// Spin a mock upstream `ckbadger-api`-shaped server on an ephemeral port.
    ///
    /// It mounts the same `/api/v1/*` and `/ws` shapes the real API uses, with a
    /// few echo routes so the proxy behavior can be asserted end to end. Returns
    /// the bound port; the server task lives for the duration of the test.
    async fn spawn_mock_upstream() -> u16 {
        let app = Router::new()
            .route("/api/v1/ping", get(|| async { "pong-from-upstream" }))
            .route(
                "/api/v1/echo-query",
                get(|uri: Uri| async move { uri.query().unwrap_or("").to_owned() }),
            )
            .route("/api/v1/echo", post(|body: Bytes| async move { body }))
            .route(
                "/api/v1/echo-forwarded",
                get(|headers: HeaderMap| async move {
                    let forwarded = headers
                        .get("x-forwarded-for")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("missing");
                    let real = headers
                        .get("x-real-ip")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("missing");
                    format!("{forwarded}|{real}")
                }),
            )
            // Never answers: stands in for an upstream that accepted the
            // connection and then wedged (the case a connect-only failure check
            // cannot see).
            .route(
                "/api/v1/hang",
                get(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    "unreachable"
                }),
            )
            .route("/ws", get(mock_upstream_ws))
            // Catch-all echo of the *raw* path the upstream actually received, so
            // tests can assert byte-for-byte what the proxy spliced into the
            // upstream URL (percent-encoding included).
            .fallback(|uri: Uri| async move { format!("path={}", uri.path()) });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    }

    /// Upstream WS echo handler: replies to each text frame with `echo:<text>`,
    /// except `whoami`, which reports the client-identity headers the upgrade
    /// request carried (`missing` when absent).
    async fn mock_upstream_ws(ws: WebSocketUpgrade, headers: HeaderMap) -> Response {
        let header_value = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("missing")
                .to_owned()
        };
        let identity = format!(
            "xff:{}|{}",
            header_value("x-forwarded-for"),
            header_value("x-real-ip")
        );

        ws.on_upgrade(move |mut socket| async move {
            while let Some(Ok(msg)) = socket.recv().await {
                match msg {
                    AxMessage::Text(t) => {
                        let reply = if t.as_str() == "whoami" {
                            identity.clone()
                        } else {
                            format!("echo:{}", t.as_str())
                        };
                        if socket.send(AxMessage::Text(reply.into())).await.is_err() {
                            break;
                        }
                    }
                    AxMessage::Close(_) => break,
                    _ => {}
                }
            }
        })
    }

    /// Mock upstream that rejects every request the way the API's pre-sync router
    /// does: 503 with the `{"error":"initializing", …}` body the SPA keys its
    /// initializing UX on.
    async fn spawn_presync_upstream() -> u16 {
        let app = Router::new().fallback(|| async {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "initializing",
                    "network": "testnet",
                    "message": "This network has not started syncing yet",
                })),
            )
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    }

    /// Serve a proxy router on an ephemeral port *with* connect info, exactly as
    /// `run_frontend_server` does, so client-address handling behaves as in
    /// production.
    async fn serve_proxy(router: Router) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        port
    }

    fn testnet_proxy(port: u16) -> Router {
        proxy_router(Arc::new(ProxyState::new(HashMap::from([(
            "testnet".to_string(),
            port,
        )]))))
    }

    /// Same routing table, but with an HTTP client whose read budget is measured
    /// in milliseconds so a stalled-upstream test bounds its own wait.
    fn testnet_proxy_with_short_timeouts(port: u16) -> Router {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .read_timeout(std::time::Duration::from_millis(200))
            .build()
            .expect("test proxy client");
        proxy_router(Arc::new(ProxyState::with_client(
            HashMap::from([("testnet".to_string(), port)]),
            client,
        )))
    }

    fn with_peer(mut request: Request, peer: &str) -> Request {
        request.extensions_mut().insert(ConnectInfo(
            peer.parse::<SocketAddr>().expect("valid test peer"),
        ));
        request
    }

    #[tokio::test]
    async fn proxy_replaces_spoofed_forwarding_headers_with_socket_peer() {
        let port = spawn_mock_upstream().await;
        let request = Request::builder()
            .uri("/api/testnet/v1/echo-forwarded")
            .header("x-forwarded-for", "127.0.0.1")
            .header("x-real-ip", "127.0.0.1")
            .body(Body::empty())
            .unwrap();
        let resp = testnet_proxy(port)
            .oneshot(with_peer(request, "203.0.113.42:4242"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"203.0.113.42|203.0.113.42");
    }

    #[tokio::test]
    async fn proxy_forwards_get_to_known_network() {
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/ping")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.1:4001",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"pong-from-upstream");
    }

    #[tokio::test]
    async fn proxy_unknown_network_returns_actionable_404() {
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(
                Request::builder()
                    .uri("/api/devnet/v1/ping")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        // Actionable: names the missing network AND lists the known ones.
        assert!(
            text.contains("devnet"),
            "should name the bad network: {text}"
        );
        assert!(
            text.contains("testnet"),
            "should list known networks: {text}"
        );
    }

    #[tokio::test]
    async fn proxy_forwards_query_string() {
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/echo-query?limit=5&cursor=abc%20def")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.2:4002",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            String::from_utf8(body.to_vec()).unwrap(),
            "limit=5&cursor=abc%20def"
        );
    }

    #[tokio::test]
    async fn proxy_keeps_encoded_path_segments_encoded_upstream() {
        // `%2F` inside a path parameter is data, not a separator: it must reach
        // upstream still encoded, otherwise the proxied route resolves to a
        // different (usually missing) endpoint than a direct API call would.
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/cell/a%2Fb")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.5:4005",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            String::from_utf8(body.to_vec()).unwrap(),
            "path=/api/v1/cell/a%2Fb"
        );
    }

    #[tokio::test]
    async fn proxy_encoded_traversal_stays_under_api_v1() {
        // `..%2F` must NOT decode into `../` — decoding then re-parsing lets the
        // URL normalizer walk out of the `/api/v1` prefix and hit unrelated
        // upstream routes (here: `/ws`).
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/..%2F..%2Fws")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.6:4006",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(
            text, "path=/api/v1/..%2F..%2Fws",
            "encoded traversal must stay one opaque segment under /api/v1"
        );
    }

    #[tokio::test]
    async fn proxy_rejects_literal_traversal_out_of_api_v1() {
        // A literal `../` tail normalizes out of the prefix while the URL is
        // built; that violates the advertised mapping, so fail fast instead of
        // silently forwarding to an unrelated upstream path.
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/../../ws")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.7:4007",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_path");
        assert!(
            json["message"].as_str().unwrap().contains("/api/v1"),
            "message should name the prefix that was escaped: {json}"
        );
    }

    #[test]
    fn raw_rest_path_keeps_original_encoding() {
        assert_eq!(
            raw_rest_path("/api/testnet/v1/cell/a%2Fb"),
            Some("cell/a%2Fb")
        );
        assert_eq!(
            raw_rest_path("/api/mainnet/v1/blocks/42"),
            Some("blocks/42")
        );
        // An encoded network segment still slices the tail correctly.
        assert_eq!(raw_rest_path("/api/test%2Dnet/v1/ping"), Some("ping"));
        // Empty tail is legal (`/api/{network}/v1/`).
        assert_eq!(raw_rest_path("/api/testnet/v1/"), Some(""));
        // Shapes the route pattern can never produce.
        assert_eq!(raw_rest_path("/api/testnet/v2/ping"), None);
        assert_eq!(raw_rest_path("/ws/testnet"), None);
    }

    #[tokio::test]
    async fn proxy_forwards_post_body_round_trip() {
        let port = spawn_mock_upstream().await;
        let payload = r#"{"hello":"world","n":42}"#;
        let resp = testnet_proxy(port)
            .oneshot(with_peer(
                Request::builder()
                    .method("POST")
                    .uri("/api/testnet/v1/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
                "203.0.113.3:4003",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(String::from_utf8(body.to_vec()).unwrap(), payload);
    }

    #[tokio::test]
    async fn proxy_unreachable_upstream_returns_502() {
        // Bind an ephemeral port, capture it, then drop the listener so nothing
        // is listening on it: the upstream is guaranteed unreachable.
        let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_port = dead_listener.local_addr().unwrap().port();
        drop(dead_listener);

        let resp = testnet_proxy(dead_port)
            .oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/ping")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.4:4004",
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        // Actionable: names the unreachable network and the failure mode.
        assert!(
            text.contains("testnet") && text.contains("unreachable"),
            "502 body should explain the unreachable upstream: {text}"
        );
    }

    #[tokio::test]
    async fn upstream_read_timeout_bounds_a_response_head_that_never_arrives() {
        // Pins the property the proxy client's timeout policy is chosen for: an
        // upstream that accepts the connection and then goes silent is cut loose
        // by the read timeout even though no response byte ever arrives. The
        // outer guard is 25x the read budget, so a regression fails instead of
        // hanging the suite.
        let port = spawn_mock_upstream().await;
        let client = reqwest::Client::builder()
            .read_timeout(std::time::Duration::from_millis(200))
            .build()
            .expect("test client");

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client
                .get(format!("http://127.0.0.1:{port}/api/v1/hang"))
                .send(),
        )
        .await
        .expect("read timeout must fire well before the outer guard");

        let err = result.expect_err("a stalled upstream must not resolve");
        assert!(err.is_timeout(), "expected a timeout error, got {err}");
    }

    #[tokio::test]
    async fn proxy_stalled_upstream_returns_504() {
        // An upstream that accepts the connection and then never answers must be
        // cut loose with a distinct 504, not held open forever nor reported as
        // "unreachable" (which it demonstrably is not).
        let port = spawn_mock_upstream().await;
        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            testnet_proxy_with_short_timeouts(port).oneshot(with_peer(
                Request::builder()
                    .uri("/api/testnet/v1/hang")
                    .body(Body::empty())
                    .unwrap(),
                "203.0.113.8:4008",
            )),
        )
        .await
        .expect("the proxy must bound a stalled upstream, not hang on it")
        .unwrap();

        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "upstream_timeout");
        assert!(
            json["message"].as_str().unwrap().contains("testnet"),
            "504 body should name the stalled network: {json}"
        );
    }

    #[tokio::test]
    async fn ws_unknown_network_returns_404() {
        // proxy_ws returns 404 for an unknown network *before* on_upgrade — but
        // the check sits after WebSocketUpgrade extraction, so it only fires for
        // a real upgrade request (a plain GET is rejected by the extractor with
        // a 400 first). Drive it with a real WS client; the proxy rejects the
        // handshake with a 404 whose body names the unknown network.
        let upstream_port = spawn_mock_upstream().await;
        let proxy_port = serve_proxy(testnet_proxy(upstream_port)).await;

        let err =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{proxy_port}/ws/devnet"))
                .await
                .expect_err("unknown network must reject the handshake before on_upgrade");

        match err {
            tokio_tungstenite::tungstenite::Error::Http(resp) => {
                assert_eq!(resp.status(), StatusCode::NOT_FOUND);
                let body = resp.body().as_ref().expect("404 handshake body present");
                let text = String::from_utf8_lossy(body);
                assert!(
                    text.contains("devnet"),
                    "should name the bad network: {text}"
                );
            }
            other => panic!("expected an HTTP 404 handshake rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ws_relay_round_trips_text_frames() {
        // Upstream WS echo server.
        let upstream_port = spawn_mock_upstream().await;

        // Serve the proxy on a real port so a real WS client can upgrade against it.
        let proxy_port = serve_proxy(testnet_proxy(upstream_port)).await;

        let (mut ws, _resp) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{proxy_port}/ws/testnet"))
                .await
                .expect("client should connect through the proxy");

        ws.send(TgMessage::Text("hi".to_owned())).await.unwrap();
        let reply = ws.next().await.expect("a reply frame").unwrap();
        assert_eq!(reply, TgMessage::Text("echo:hi".to_owned()));
    }

    #[tokio::test]
    async fn ws_relay_forwards_client_identity_to_upstream() {
        // Without forwarded identity the upgrade reaches the API as a loopback
        // peer, which `extract_client_ip` treats as local and the rate limiter
        // therefore skips — every proxied socket would be un-limited.
        let upstream_port = spawn_mock_upstream().await;
        let proxy_port = serve_proxy(testnet_proxy(upstream_port)).await;

        let (mut ws, _resp) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{proxy_port}/ws/testnet"))
                .await
                .expect("client should connect through the proxy");

        ws.send(TgMessage::Text("whoami".to_owned())).await.unwrap();
        let reply = ws.next().await.expect("a reply frame").unwrap();
        // The client's socket peer is loopback in-test; what matters is that the
        // upstream is told the client's address at all, on both headers, exactly
        // as the HTTP path already does.
        assert_eq!(reply, TgMessage::Text("xff:127.0.0.1|127.0.0.1".to_owned()));
    }

    #[tokio::test]
    async fn ws_upstream_503_reaches_the_client_as_a_real_503() {
        // A pre-sync upstream rejects the upgrade with 503 + the `initializing`
        // payload. Collapsing that into a post-upgrade Close(1011) would tell the
        // client the proxy failed and hide the code the SPA keys its UX on, so
        // the rejection must arrive as an HTTP status on the upgrade itself.
        let upstream_port = spawn_presync_upstream().await;
        let proxy_port = serve_proxy(testnet_proxy(upstream_port)).await;

        let err =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{proxy_port}/ws/testnet"))
                .await
                .expect_err("a pre-sync upstream must reject the handshake, not upgrade it");

        match err {
            tokio_tungstenite::tungstenite::Error::Http(resp) => {
                assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
                let body = resp.body().as_ref().expect("503 handshake body present");
                let json: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(json["error"], "initializing");
                assert_eq!(json["network"], "testnet");
            }
            other => panic!("expected an HTTP 503 handshake rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ws_unreachable_upstream_rejects_the_handshake_with_502() {
        // Nothing listening upstream: the client must learn that from its own
        // upgrade request rather than from a 101 followed immediately by a close.
        let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_port = dead_listener.local_addr().unwrap().port();
        drop(dead_listener);
        let proxy_port = serve_proxy(testnet_proxy(dead_port)).await;

        let err =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{proxy_port}/ws/testnet"))
                .await
                .expect_err("an absent upstream must reject the handshake");

        match err {
            tokio_tungstenite::tungstenite::Error::Http(resp) => {
                assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
                let body = resp.body().as_ref().expect("502 handshake body present");
                let json: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(json["error"], "upstream_unreachable");
                assert!(
                    json["message"].as_str().unwrap().contains("testnet"),
                    "502 body should name the unreachable network: {json}"
                );
            }
            other => panic!("expected an HTTP 502 handshake rejection, got {other:?}"),
        }
    }

    #[test]
    fn axum_to_tungstenite_covers_all_variants() {
        assert_eq!(
            axum_msg_to_tungstenite(AxMessage::Text("hello".into())),
            TgMessage::Text("hello".to_owned())
        );
        assert_eq!(
            axum_msg_to_tungstenite(AxMessage::Binary(Bytes::from_static(&[1, 2, 3]))),
            TgMessage::Binary(vec![1, 2, 3])
        );
        assert_eq!(
            axum_msg_to_tungstenite(AxMessage::Ping(Bytes::from_static(&[9]))),
            TgMessage::Ping(vec![9])
        );
        assert_eq!(
            axum_msg_to_tungstenite(AxMessage::Pong(Bytes::from_static(&[8]))),
            TgMessage::Pong(vec![8])
        );
        let tg_close = axum_msg_to_tungstenite(AxMessage::Close(Some(AxCloseFrame {
            code: 1000,
            reason: "bye".into(),
        })));
        match tg_close {
            TgMessage::Close(Some(f)) => {
                assert_eq!(u16::from(f.code), 1000);
                assert_eq!(f.reason.as_ref(), "bye");
            }
            other => panic!("expected close frame, got {other:?}"),
        }
        assert_eq!(
            axum_msg_to_tungstenite(AxMessage::Close(None)),
            TgMessage::Close(None)
        );
    }

    #[test]
    fn tungstenite_to_axum_covers_all_variants() {
        assert_eq!(
            tungstenite_msg_to_axum(TgMessage::Text("hello".to_owned())),
            Some(AxMessage::Text("hello".into()))
        );
        assert_eq!(
            tungstenite_msg_to_axum(TgMessage::Binary(vec![1, 2, 3])),
            Some(AxMessage::Binary(Bytes::from_static(&[1, 2, 3])))
        );
        assert_eq!(
            tungstenite_msg_to_axum(TgMessage::Ping(vec![9])),
            Some(AxMessage::Ping(Bytes::from_static(&[9])))
        );
        assert_eq!(
            tungstenite_msg_to_axum(TgMessage::Pong(vec![8])),
            Some(AxMessage::Pong(Bytes::from_static(&[8])))
        );
        let ax_close = tungstenite_msg_to_axum(TgMessage::Close(Some(TgCloseFrame {
            code: 1001u16.into(),
            reason: "later".into(),
        })));
        match ax_close {
            Some(AxMessage::Close(Some(f))) => {
                assert_eq!(f.code, 1001);
                assert_eq!(f.reason.as_str(), "later");
            }
            other => panic!("expected close frame, got {other:?}"),
        }
        assert_eq!(
            tungstenite_msg_to_axum(TgMessage::Close(None)),
            Some(AxMessage::Close(None))
        );
        // Raw frames never surface through the high-level stream.
        assert!(tungstenite_msg_to_axum(TgMessage::Frame(
            tokio_tungstenite::tungstenite::protocol::frame::Frame::pong(vec![])
        ))
        .is_none());
    }
}
