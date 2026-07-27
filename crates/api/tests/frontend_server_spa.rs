use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use ckbadger_api::entry::{build_frontend_router, FrontendNetwork, FrontendServiceConfig};
use http_body_util::BodyExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn frontend_server_falls_back_to_index_html_for_spa_route() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets").join("app.js"),
        "console.log('ok');",
    )
    .unwrap();

    let router = build_frontend_router(FrontendServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 8100,
        api_port: 8101,
        ckb_network: "mainnet".to_string(),
        ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
        build_version: "0.1.0+testbuild".to_string(),
        frontend_dir: Some(PathBuf::from(dir.path())),
        default_network: "mainnet".to_string(),
        networks: vec![FrontendNetwork {
            name: "mainnet".to_string(),
            api_port: 8101,
        }],
    })
    .unwrap();

    let script_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/script/0x1234")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(script_response.status(), StatusCode::OK);
    let script_body = script_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(script_body, "<html>spa</html>");

    let asset_response = router
        .oneshot(
            Request::builder()
                .uri("/assets/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(asset_response.status(), StatusCode::OK);
    let asset_body = asset_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(asset_body, "console.log('ok');");
}

/// The SPA fallback catches every path that is not a file, which is exactly what
/// deep links need — and exactly what would silently swallow the proxy routes if
/// they were ever merged after it (or removed). A swallowed `/api/…` does not
/// error: it answers `200 text/html`, which every JSON client reads as a parse
/// failure and every agent reads as a working endpoint. Pin both sides.
#[tokio::test]
async fn frontend_server_routes_api_paths_to_the_proxy_not_the_spa_fallback() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();

    // Bind an ephemeral port and drop it, so the proxy target is guaranteed
    // absent: any proxy-shaped answer proves the request reached the proxy
    // rather than the SPA shell.
    let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = dead_listener.local_addr().unwrap().port();
    drop(dead_listener);

    let router = build_frontend_router(FrontendServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 8100,
        api_port: dead_port,
        ckb_network: "mainnet".to_string(),
        ckb_rpc_url: "http://127.0.0.1:8114".to_string(),
        build_version: "0.1.0+testbuild".to_string(),
        frontend_dir: Some(PathBuf::from(dir.path())),
        default_network: "mainnet".to_string(),
        networks: vec![FrontendNetwork {
            name: "mainnet".to_string(),
            api_port: dead_port,
        }],
    })
    .unwrap();

    let with_peer = |uri: &str| {
        let mut request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "203.0.113.9:5000".parse::<SocketAddr>().unwrap(),
        ));
        request
    };

    // A known network reaches the proxy, which reports the dead upstream.
    let api_response = router
        .clone()
        .oneshot(with_peer("/api/mainnet/v1/statistics/network"))
        .await
        .unwrap();
    assert_eq!(api_response.status(), StatusCode::BAD_GATEWAY);
    let body = api_response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "upstream_unreachable");

    // An unknown network is the proxy's actionable 404, not the SPA shell.
    let unknown_response = router
        .clone()
        .oneshot(with_peer("/api/devnet/v1/statistics/network"))
        .await
        .unwrap();
    assert_eq!(unknown_response.status(), StatusCode::NOT_FOUND);
    let body = unknown_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "unknown_network");

    // `/ws/{network}` is a route too: a plain GET is rejected by the upgrade
    // extractor rather than answered with the SPA shell.
    let ws_response = router
        .clone()
        .oneshot(with_peer("/ws/mainnet"))
        .await
        .unwrap();
    assert_eq!(ws_response.status(), StatusCode::BAD_REQUEST);
    let ws_body = ws_response.into_body().collect().await.unwrap().to_bytes();
    assert_ne!(ws_body, "<html>spa</html>");

    // …while an ordinary asset-less deep link still gets the SPA shell.
    let deep_link_response = router
        .clone()
        .oneshot(with_peer("/mainnet/blocks/12345"))
        .await
        .unwrap();
    assert_eq!(deep_link_response.status(), StatusCode::OK);
    let deep_link_body = deep_link_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(deep_link_body, "<html>spa</html>");
}
