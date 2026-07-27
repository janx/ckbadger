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
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{
    CloseFrame as AxCloseFrame, Message as AxMessage, WebSocket, WebSocketUpgrade,
};
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};

use tokio_tungstenite::tungstenite::protocol::CloseFrame as TgCloseFrame;
use tokio_tungstenite::tungstenite::Message as TgMessage;

use crate::response::ApiError;

/// The upstream path prefix every proxied API request must stay inside.
///
/// `/api/{network}/v1/{*rest}` maps onto `/api/v1/{rest}`, so a request that
/// ends up outside this prefix has escaped the advertised contract.
const UPSTREAM_API_PREFIX: &str = "/api/v1/";

/// Per-network backend routing table: network name → local `ckbadger-api` port.
pub struct ProxyState {
    pub ports: HashMap<String, u16>,
}

impl ProxyState {
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

    let client = crate::utils::http::shared_http_client();
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    let mut builder = client.request(parts.method, url).body(reqwest_body);
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
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("upstream network '{network}' unreachable: {e}"),
        )
            .into_response(),
    }
}

/// Relay `/ws/{network}` to the network's API server at `ws://127.0.0.1:{port}/ws`.
pub async fn proxy_ws(
    State(state): State<Arc<ProxyState>>,
    Path(network): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(port) = state.upstream_port(&network) else {
        return unknown_network(&network, &state);
    };
    ws.on_upgrade(move |client| relay_ws(client, port))
}

/// Pump frames in both directions between the browser client and the upstream API
/// WebSocket until either side closes or errors.
async fn relay_ws(mut client: WebSocket, port: u16) {
    let url = format!("ws://127.0.0.1:{port}/ws");
    let Ok((upstream, _)) = tokio_tungstenite::connect_async(&url).await else {
        // Upstream is unreachable: tell the browser why with an explicit Close
        // frame (1011 = server terminating due to an internal error) instead of
        // silently dropping the socket, which surfaces as a bare 1006 in the
        // browser. Ignore the send error — the client may already be gone.
        let _ = client
            .send(AxMessage::Close(Some(AxCloseFrame {
                code: 1011,
                reason: "upstream unreachable".into(),
            })))
            .await;
        return;
    };
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

    /// Upstream WS echo handler: replies to each text frame with `echo:<text>`.
    async fn mock_upstream_ws(ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(|mut socket| async move {
            while let Some(Ok(msg)) = socket.recv().await {
                match msg {
                    AxMessage::Text(t) => {
                        let reply = format!("echo:{}", t.as_str());
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

    fn testnet_proxy(port: u16) -> Router {
        proxy_router(Arc::new(ProxyState {
            ports: HashMap::from([("testnet".to_string(), port)]),
        }))
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
    async fn ws_unknown_network_returns_404() {
        // proxy_ws returns 404 for an unknown network *before* on_upgrade — but
        // the check sits after WebSocketUpgrade extraction, so it only fires for
        // a real upgrade request (a plain GET is rejected by the extractor with
        // a 400 first). Drive it with a real WS client; the proxy rejects the
        // handshake with a 404 whose body names the unknown network.
        let upstream_port = spawn_mock_upstream().await;
        let proxy = testnet_proxy(upstream_port);
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(proxy_listener, proxy).await.unwrap();
        });

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
        let proxy = testnet_proxy(upstream_port);
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(proxy_listener, proxy).await.unwrap();
        });

        let (mut ws, _resp) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{proxy_port}/ws/testnet"))
                .await
                .expect("client should connect through the proxy");

        ws.send(TgMessage::Text("hi".to_owned())).await.unwrap();
        let reply = ws.next().await.expect("a reply frame").unwrap();
        assert_eq!(reply, TgMessage::Text("echo:hi".to_owned()));
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
