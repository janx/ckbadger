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
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{
    CloseFrame as AxCloseFrame, Message as AxMessage, WebSocket, WebSocketUpgrade,
};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::CloseFrame as TgCloseFrame;
use tokio_tungstenite::tungstenite::Message as TgMessage;

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

/// Forward `/api/{network}/v1/{*rest}` (with query string) to the network's API
/// server at `http://127.0.0.1:{port}/api/v1/{rest}`.
///
/// Both directions stream: the request body is wrapped with
/// [`reqwest::Body::wrap_stream`] and the upstream response is returned via
/// [`Body::from_stream`], so neither is fully buffered in the proxy.
pub async fn proxy_api(
    State(state): State<Arc<ProxyState>>,
    Path((network, rest)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some(port) = state.upstream_port(&network) else {
        return unknown_network(&network, &state);
    };

    let (parts, body) = req.into_parts();
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let url = format!("http://127.0.0.1:{port}/api/v1/{rest}{query}");

    let client = crate::utils::http::shared_http_client();
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    let mut builder = client.request(parts.method, &url).body(reqwest_body);
    for (name, value) in parts.headers.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        builder = builder.header(name, value);
    }

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
async fn relay_ws(client: WebSocket, port: u16) {
    let url = format!("ws://127.0.0.1:{port}/ws");
    let Ok((upstream, _)) = tokio_tungstenite::connect_async(&url).await else {
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
    use axum::http::Uri;
    use axum::routing::post;
    use http_body_util::BodyExt;
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
            .route("/ws", get(mock_upstream_ws));

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

    #[tokio::test]
    async fn proxy_forwards_get_to_known_network() {
        let port = spawn_mock_upstream().await;
        let resp = testnet_proxy(port)
            .oneshot(
                Request::builder()
                    .uri("/api/testnet/v1/ping")
                    .body(Body::empty())
                    .unwrap(),
            )
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
            .oneshot(
                Request::builder()
                    .uri("/api/testnet/v1/echo-query?limit=5&cursor=abc%20def")
                    .body(Body::empty())
                    .unwrap(),
            )
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
    async fn proxy_forwards_post_body_round_trip() {
        let port = spawn_mock_upstream().await;
        let payload = r#"{"hello":"world","n":42}"#;
        let resp = testnet_proxy(port)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/testnet/v1/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(String::from_utf8(body.to_vec()).unwrap(), payload);
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
