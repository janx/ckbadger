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

#[tokio::test]
async fn distributions_aggregates_from_nodes() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (_c, v) = get_json(&app, "/network/distributions").await;
    assert_eq!(v["totalKnown"], 2);
    assert_eq!(v["reachable"], 1);
    assert_eq!(v["unreachable"], 1);
    // peerA=US, peerB=None -> Unknown
    let countries = v["countries"].as_array().unwrap();
    assert!(countries
        .iter()
        .any(|c| c["label"] == "US" && c["count"] == 1));
    assert!(countries
        .iter()
        .any(|c| c["label"] == "Unknown" && c["count"] == 1));
    // versions: 0.119.0 and 0.118.0 one each
    assert_eq!(v["versions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn distributions_empty_when_no_store() {
    let app = create_router_without_warmup(test_config(test_store()));
    let (_c, v) = get_json(&app, "/network/distributions").await;
    assert_eq!(v["totalKnown"], 0);
    assert_eq!(v["versions"].as_array().unwrap().len(), 0);
}
