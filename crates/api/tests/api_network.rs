mod common;
use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn summary_reports_disabled_and_no_data_by_default() {
    let cfg = test_config(test_store()); // empty network-store slot, crawler disabled
    let app = create_router_without_warmup(cfg);
    let res = get_json(&app, "/network/summary").await;
    assert_eq!(res.0, StatusCode::OK);
    assert_eq!(res.1["enabled"], false);
    assert_eq!(res.1["hasData"], false);
    assert!(res.1["lastRound"].is_null());
    assert!(res.1["activeRound"].is_null());
}

#[tokio::test]
async fn summary_reports_has_data_when_seeded() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let res = get_json(&app, "/network/summary").await;
    assert_eq!(res.1["enabled"], true);
    assert_eq!(res.1["hasData"], true);
    assert_eq!(res.1["lastRound"]["totalKnown"], 2);
    assert_eq!(res.1["lastRound"]["reachablePeers"], 1);
    assert_eq!(res.1["lastRound"]["addressAttempts"], 3);
    assert!(res.1["activeRound"].is_null());
}

#[tokio::test]
async fn summary_observes_network_store_attached_after_router_startup() {
    let cfg = test_config(test_store());
    let network_store = cfg.network_store.clone();
    let app = create_router_without_warmup(cfg);

    let (_, before) = get_json(&app, "/network/summary").await;
    assert_eq!(before["hasData"], false);
    assert!(before["lastRound"].is_null());

    network_store.store(Some(test_network_store()));

    let (_, after) = get_json(&app, "/network/summary").await;
    assert_eq!(after["hasData"], true);
    assert_eq!(after["lastRound"]["roundId"], 5);
    assert_eq!(after["lastRound"]["totalKnown"], 2);
}

#[tokio::test]
async fn summary_reports_active_progress_separately_from_completed_round() {
    use ckbadger_store::{ActiveCrawl, CrawlAddress, CrawlCandidate};

    let network = test_network_store();
    network
        .checkpoint_crawl(
            &ActiveCrawl {
                round_id: 6,
                started_at: 300,
                last_checkpoint_at: 320,
                address_attempts: 1,
                blocked_reason: Some("frontier capacity exceeded".into()),
                ..Default::default()
            },
            &[(
                b"peerC".to_vec(),
                CrawlCandidate {
                    addresses: vec![CrawlAddress {
                        addr: "addrC".into(),
                        last_advertised_at: 300,
                        attempted_round: 0,
                    }],
                    first_discovered_at: 300,
                    last_advertised_at: 300,
                    round_id: 6,
                    ..Default::default()
                },
            )],
        )
        .unwrap();
    let cfg = test_config_with_network(test_store(), network, true);
    let app = create_router_without_warmup(cfg);
    let (_, body) = get_json(&app, "/network/summary").await;

    assert_eq!(body["lastRound"]["roundId"], 5);
    assert_eq!(body["activeRound"]["roundId"], 6);
    assert_eq!(body["activeRound"]["candidatePeers"], 1);
    assert_eq!(body["activeRound"]["completedPeers"], 0);
    assert_eq!(body["activeRound"]["addressAttempts"], 1);
    assert_eq!(
        body["activeRound"]["blockedReason"],
        "frontier capacity exceeded"
    );
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

#[tokio::test]
async fn history_returns_scalar_series() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (_c, v) = get_json(&app, "/network/history?metric=totalNodes&granularity=hour").await;
    assert_eq!(v["metric"], "totalNodes");
    let pts = v["points"].as_array().unwrap();
    assert!(pts.iter().any(|p| p["scalar"] == 2));
}

#[tokio::test]
async fn history_empty_when_no_store() {
    let app = create_router_without_warmup(test_config(test_store()));
    let (_c, v) = get_json(&app, "/network/history?metric=totalNodes&granularity=hour").await;
    assert_eq!(v["points"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn history_day_excludes_current_day_without_to() {
    use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
    use ckbadger_store::{CkbadgerStore, HistoryPoint};
    // Seed a network store with two Day buckets for TotalNodes: the incomplete
    // current day (scalar 99) and the complete previous day (scalar 5).
    let dir = tempfile::tempdir().unwrap();
    let net = Arc::new(CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir); // keep the temp dir alive for the store's lifetime
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let cur = bucket_of(now, Granularity::Day);
    net.put_history_point(
        Metric::TotalNodes,
        Granularity::Day,
        cur,
        &HistoryPoint {
            scalar: 99,
            buckets: vec![],
        },
    )
    .unwrap();
    net.put_history_point(
        Metric::TotalNodes,
        Granularity::Day,
        cur - 1,
        &HistoryPoint {
            scalar: 5,
            buckets: vec![],
        },
    )
    .unwrap();

    let cfg = test_config_with_network(test_store(), net, true);
    let app = create_router_without_warmup(cfg);
    // No `to`: the endpoint must still drop the incomplete current day (server clock).
    let (_c, v) = get_json(&app, "/network/history?metric=totalNodes&granularity=day").await;
    let pts = v["points"].as_array().unwrap();
    // Previous-day point survives; current-day point is excluded.
    assert!(pts.iter().any(|p| p["scalar"] == 5));
    assert!(!pts.iter().any(|p| p["scalar"] == 99));
}

#[tokio::test]
async fn nodes_lists_and_filters() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (_c, all) = get_json(&app, "/network/nodes").await;
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
    let (_c, reach) = get_json(&app, "/network/nodes?reachable=true").await;
    assert_eq!(reach["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn node_by_id_not_found() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (code, _v) = get_json(&app, "/network/nodes/deadbeef").await;
    assert_eq!(code, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nodes_paginates_with_cursor() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    // Deterministic order (last_seen desc, peer_id asc) => peerA first, peerB second.
    let peer_a = "7065657241"; // hex("peerA")
    let peer_b = "7065657242"; // hex("peerB")
    let (_c, p1) = get_json(&app, "/network/nodes?limit=1").await;
    let items1 = p1["items"].as_array().unwrap();
    assert_eq!(items1.len(), 1);
    assert_eq!(items1[0]["peerId"], peer_a);
    assert_eq!(p1["nextCursor"], peer_a); // more remain -> cursor set
    let (_c, p2) = get_json(&app, &format!("/network/nodes?limit=1&cursor={peer_a}")).await;
    let items2 = p2["items"].as_array().unwrap();
    assert_eq!(items2.len(), 1);
    assert_eq!(items2[0]["peerId"], peer_b);
    assert!(p2["nextCursor"].is_null()); // last page -> no cursor
}

#[tokio::test]
async fn nodes_filters_by_country_and_version() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (_c, us) = get_json(&app, "/network/nodes?country=US").await;
    let us_items = us["items"].as_array().unwrap();
    assert_eq!(us_items.len(), 1);
    assert_eq!(us_items[0]["country"], "US");
    let (_c, unknown) = get_json(&app, "/network/nodes?country=Unknown").await;
    assert_eq!(unknown["items"].as_array().unwrap().len(), 1);
    let (_c, ver) = get_json(&app, "/network/nodes?version=0.118.0").await;
    let ver_items = ver["items"].as_array().unwrap();
    assert_eq!(ver_items.len(), 1);
    assert_eq!(ver_items[0]["version"], "0.118.0");
}

#[tokio::test]
async fn nodes_empty_when_no_store() {
    let app = create_router_without_warmup(test_config(test_store()));
    let (code, v) = get_json(&app, "/network/nodes").await;
    assert_eq!(code, axum::http::StatusCode::OK);
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
    assert!(v["nextCursor"].is_null());
}

#[tokio::test]
async fn node_by_id_returns_detail() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (code, v) = get_json(&app, "/network/nodes/7065657241").await; // hex("peerA")
    assert_eq!(code, axum::http::StatusCode::OK);
    assert_eq!(v["peerId"], "7065657241");
    assert_eq!(v["clientVersion"], "0.119.0");
    assert_eq!(v["country"], "US");
    assert_eq!(v["reachable"], true);
    assert_eq!(v["rttMs"], 9);
    assert_eq!(v["knownPeers"], 0);
    assert_eq!(v["ownAddrs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn node_by_id_rejects_malformed_hex() {
    let cfg = test_config_with_network(test_store(), test_network_store(), true);
    let app = create_router_without_warmup(cfg);
    let (code, _v) = get_json(&app, "/network/nodes/xyz").await;
    assert_eq!(code, axum::http::StatusCode::BAD_REQUEST);
}
