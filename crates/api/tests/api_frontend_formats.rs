//! Exercise the production shared-frontend router, not a standalone renderer.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use ckbadger_api::entry::{build_frontend_router, FrontendNetwork, FrontendServiceConfig};
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn production_frontend_serves_advertised_raw_block() {
    let api = MockServer::start().await;
    Mock::given(path("/api/v1/blocks/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "number": 42, "hash": "0xblock", "capacity": "9007199254740993"
        })))
        .mount(&api)
        .await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();
    let router = build_frontend_router(FrontendServiceConfig {
        public_origin: None,
        host: "127.0.0.1".into(),
        port: 8100,
        api_port: api.address().port(),
        ckb_network: "mainnet".into(),
        ckb_rpc_url: String::new(),
        build_version: "test-build".into(),
        frontend_dir: Some(dir.path().to_path_buf()),
        default_network: "mainnet".into(),
        networks: vec![FrontendNetwork {
            ckb_rpc_url: "http://127.0.0.1:8114".into(),
            name: "mainnet".into(),
            api_host: "127.0.0.1".into(),
            api_port: api.address().port(),
        }],
    })
    .unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .uri("/mainnet/blocks/42.raw")
                .header("host", "localhost:8100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/vnd.ckbadger.raw+json; charset=utf-8"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["data"]["block"]["number"], 42);
    assert_eq!(payload["data"]["block"]["capacity"], "9007199254740993");
    assert_eq!(
        payload["meta"]["canonical"],
        "http://localhost:8100/mainnet/blocks/42"
    );
}

#[tokio::test]
async fn every_advertised_format_reaches_its_network_api() {
    let api = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
            "error": "initializing", "message": "network-specific API reached"
        })))
        .mount(&api)
        .await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();
    let router = build_frontend_router(FrontendServiceConfig {
        public_origin: Some("https://explorer.example".into()),
        host: "127.0.0.1".into(),
        port: 8100,
        api_port: api.address().port(),
        ckb_network: "mainnet".into(),
        ckb_rpc_url: String::new(),
        build_version: "test-build".into(),
        frontend_dir: Some(dir.path().to_path_buf()),
        default_network: "mainnet".into(),
        networks: ["mainnet", "testnet"]
            .into_iter()
            .map(|name| FrontendNetwork {
                name: name.into(),
                api_host: "127.0.0.1".into(),
                api_port: api.address().port(),
                ckb_rpc_url: api.uri(),
            })
            .collect(),
    })
    .unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let capabilities: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(capabilities["origin"], "https://explorer.example");
    for network in ["mainnet", "testnet"] {
        for (format, suffix) in [("markdown", "md"), ("raw", "raw")] {
            for pattern in capabilities["routes"][format].as_array().unwrap() {
                let mut path = pattern.as_str().unwrap().to_string();
                while let Some(start) = path.find('{') {
                    let end = path[start..].find('}').unwrap() + start;
                    let value = match &path[start..=end] {
                        "{outpoint}" => "0xabc-0",
                        "{slug}" => "hash-rate",
                        _ => "42",
                    };
                    path.replace_range(start..=end, value);
                }
                let uri = format!("/{network}{path}.{suffix}");
                let response = router
                    .clone()
                    .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = response.status();
                assert_eq!(response.headers()["cache-control"], "no-store", "{uri}");
                assert_eq!(response.headers()["vary"], "Accept", "{uri}");
                let body = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(
                    status,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "{uri}: {}",
                    String::from_utf8_lossy(&body)
                );
                let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert!(
                    error["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("network-specific API reached"),
                    "{uri}: {error}"
                );
            }
        }
    }
}
