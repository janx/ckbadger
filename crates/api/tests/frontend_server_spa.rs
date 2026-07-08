use axum::body::Body;
use axum::http::{Request, StatusCode};
use ckbadger_api::entry::{build_frontend_router, FrontendNetwork, FrontendServiceConfig};
use http_body_util::BodyExt;
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
