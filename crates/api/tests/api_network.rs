mod common;
use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn summary_reports_disabled_and_no_data_by_default() {
    let cfg = test_config(test_store()); // network_store None, crawler_enabled false
    let app = create_router_without_warmup(cfg);
    let res = get_json(&app, "/network/summary").await;
    assert_eq!(res.0, StatusCode::OK);
    assert_eq!(res.1["enabled"], false);
    assert_eq!(res.1["hasData"], false);
    assert!(res.1["lastRound"].is_null());
}

#[tokio::test]
async fn summary_reports_has_data_when_seeded() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let res = get_json(&app, "/network/summary").await;
    assert_eq!(res.1["enabled"], true);
    assert_eq!(res.1["hasData"], true);
    assert_eq!(res.1["lastRound"]["totalKnown"], 2);
    assert_eq!(res.1["lastRound"]["reachable"], 1);
}
