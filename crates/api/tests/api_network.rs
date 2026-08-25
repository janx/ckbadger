mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use ckbadger_store::{
    ActiveCandidateProbe, ActiveCrawl, AddressObservationHistogram, CompletedPeerOutcomes,
    DirectSessionEvidence, DirectSessionObservation, DirectSessionObservationSummary, LatestStatus,
    LocalObserverEvidence, LocalObserverProtocol, SessionInitiator,
};
use common::*;

#[tokio::test]
async fn summary_reports_disabled_and_no_data_by_default() {
    let app = create_router_without_warmup(test_config(test_store()));
    let (code, body) = get_json(&app, "/network/summary").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["hasData"], false);
    assert!(body["lastRound"].is_null());
    assert!(body["activeRound"].is_null());
}

#[tokio::test]
async fn summary_uses_exact_verification_terms_and_checked_projections() {
    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    let (_, body) = get_json(&app, "/network/summary").await;

    assert_eq!(body["lastRound"]["candidatePeers"], 3);
    assert_eq!(body["lastRound"]["verifiedRetainedPeers"], 2);
    assert_eq!(body["lastRound"]["reachablePeers"], 1);
    assert_eq!(body["lastRound"]["verifiedUnavailablePeers"], 1);
    assert_eq!(body["lastRound"]["exhaustedCandidates"], 2);
    assert_eq!(body["lastRound"]["addressAttempts"], 3);
    assert_eq!(body["lastRound"]["nonSuccessfulAddressAttempts"], 2);
    assert!(body["lastRound"].get("totalKnown").is_none());
    assert!(body["lastRound"].get("unreachablePeers").is_none());
}

#[tokio::test]
async fn summary_exposes_local_observer_and_directional_session_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(ckbadger_store::CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir);
    network
        .put_network_status(&LatestStatus {
            round_id: 9,
            started: 100,
            finished: 200,
            local_observer: Some(LocalObserverEvidence {
                peer_id: b"observer".to_vec(),
                first_observed_at: 100,
                last_observed_at: 200,
                first_observed_round: 8,
                last_observed_round: 9,
                observation_count: 2,
                client_version: "ckb-observer".into(),
                active: true,
                addresses: vec!["/ip4/127.0.0.1/tcp/8115".into()],
                protocols: vec![LocalObserverProtocol {
                    id: 1,
                    name: "identify".into(),
                    support_versions: vec!["0.0.1".into()],
                }],
                connections: 5,
            }),
            direct_session_observations: DirectSessionObservationSummary {
                observer_initiated: 2,
                peer_initiated: 3,
            },
            ..Default::default()
        })
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));

    let (code, body) = get_json(&app, "/network/summary").await;
    assert_eq!(code, StatusCode::OK);
    let round = &body["lastRound"];
    assert_eq!(round["localObserver"]["peerId"], "6f62736572766572");
    assert_eq!(round["localObserver"]["observationCount"], 2);
    assert_eq!(
        round["localObserver"]["protocols"][0]["supportVersions"][0],
        "0.0.1"
    );
    assert_eq!(round["directSessionObservations"]["observerInitiated"], 2);
    assert_eq!(round["directSessionObservations"]["peerInitiated"], 3);
}

#[tokio::test]
async fn summary_preserves_the_observed_144_57_87_58_semantic_split() {
    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(ckbadger_store::CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir);
    network
        .put_network_status(&LatestStatus {
            round_id: 53,
            started: 100,
            finished: 200,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 57,
                exhausted_with_retained_verification: 1,
                exhausted_without_retained_verification: 86,
                ..Default::default()
            },
            address_observations: AddressObservationHistogram {
                same_network_identified: 57,
                dial_request_failed: 359,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));
    let (_, body) = get_json(&app, "/network/summary").await;
    let round = &body["lastRound"];

    assert_eq!(round["candidatePeers"], 144);
    assert_eq!(round["reachablePeers"], 57);
    assert_eq!(round["exhaustedCandidates"], 87);
    assert_eq!(round["verifiedRetainedPeers"], 58);
    assert_eq!(round["verifiedUnavailablePeers"], 1);
    assert_eq!(
        round["peerOutcomes"]["exhausted"]["withoutRetainedVerification"],
        86
    );
}

#[tokio::test]
async fn active_round_remains_separate_from_completed_candidate_evidence() {
    let network = test_network_store();
    let mut candidate = network.get_crawl_candidate(b"peerC").unwrap().unwrap();
    candidate.active = Some(ActiveCandidateProbe {
        round_id: 6,
        ..Default::default()
    });
    network
        .checkpoint_crawl(
            &ActiveCrawl {
                round_id: 6,
                started_at: 300,
                last_checkpoint_at: 320,
                blocked_reason: Some("frontier capacity exceeded".into()),
                ..Default::default()
            },
            &[(b"peerC".to_vec(), candidate)],
        )
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));

    let (_, summary) = get_json(&app, "/network/summary").await;
    assert_eq!(summary["lastRound"]["roundId"], 5);
    assert_eq!(summary["activeRound"]["roundId"], 6);
    assert_eq!(summary["activeRound"]["candidatePeers"], 1);
    let (_, detail) = get_json(&app, "/network/peers/7065657243").await;
    assert_eq!(detail["lastCompleted"]["roundId"], 5);
    assert_eq!(detail["active"]["roundId"], 6);
}

#[tokio::test]
async fn summary_observes_network_store_attached_after_router_startup() {
    let config = test_config(test_store());
    let slot = config.network_store.clone();
    let app = create_router_without_warmup(config);
    let (_, before) = get_json(&app, "/network/summary").await;
    assert_eq!(before["hasData"], false);
    slot.store(Some(test_network_store()));
    let (_, after) = get_json(&app, "/network/summary").await;
    assert_eq!(after["lastRound"]["verifiedRetainedPeers"], 2);
}

#[tokio::test]
async fn distributions_are_explicitly_scoped_to_verified_records() {
    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    let (_, body) = get_json(&app, "/network/distributions").await;
    assert_eq!(body["verifiedRetained"], 2);
    assert_eq!(body["sameNetworkReachable"], 1);
    assert_eq!(body["verifiedUnavailable"], 1);
    assert!(body.get("totalKnown").is_none());
    assert!(body["countries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|bucket| bucket["label"] == "US" && bucket["count"] == 1));
}

#[tokio::test]
async fn history_uses_verified_peer_metric_names() {
    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    let (_, body) = get_json(
        &app,
        "/network/history?metric=verifiedPeers&granularity=hour",
    )
    .await;
    assert_eq!(body["metric"], "verifiedPeers");
    assert!(body["points"]
        .as_array()
        .unwrap()
        .iter()
        .any(|point| point["scalar"] == 2));
    let (old_code, _) = get_json(&app, "/network/history?metric=totalNodes&granularity=hour").await;
    assert_eq!(old_code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn history_day_excludes_the_incomplete_current_day() {
    use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
    use ckbadger_store::{CkbadgerStore, HistoryPoint};

    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let current = bucket_of(now, Granularity::Day);
    for (bucket, scalar) in [(current, 99), (current - 1, 5)] {
        network
            .put_history_point(
                Metric::VerifiedPeers,
                Granularity::Day,
                bucket,
                &HistoryPoint {
                    scalar,
                    buckets: vec![],
                },
            )
            .unwrap();
    }
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));
    let (_, body) = get_json(
        &app,
        "/network/history?metric=verifiedPeers&granularity=day",
    )
    .await;
    let points = body["points"].as_array().unwrap();
    assert!(points.iter().any(|point| point["scalar"] == 5));
    assert!(!points.iter().any(|point| point["scalar"] == 99));
}

#[tokio::test]
async fn history_fails_fast_when_bucket_timestamp_overflows() {
    use ckbadger_store::network_keys::{Granularity, Metric};
    use ckbadger_store::{CkbadgerStore, HistoryPoint};

    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir);
    network
        .put_history_point(
            Metric::VerifiedPeers,
            Granularity::Hour,
            u64::MAX,
            &HistoryPoint::default(),
        )
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));

    let (status, body) = get_json(
        &app,
        "/network/history?metric=verifiedPeers&granularity=hour",
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.to_string().contains("history timestamp overflow"));
    assert!(body.to_string().contains("bucket=18446744073709551615"));
}

#[tokio::test]
async fn peers_unifies_verified_and_candidate_only_rows() {
    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    let (_, all) = get_json(&app, "/network/peers").await;
    assert_eq!(all["items"].as_array().unwrap().len(), 3);
    let (_, unverified) = get_json(&app, "/network/peers?state=advertisedUnverified").await;
    let items = unverified["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["peerId"], "7065657243");
    assert!(items[0]["version"].is_null());
    assert!(items[0]["country"].is_null());
    let (_, unavailable) = get_json(&app, "/network/peers?state=verifiedUnavailable").await;
    assert_eq!(unavailable["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn peers_paginate_and_reject_invalid_limits_or_cursors() {
    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    let (_, first) = get_json(&app, "/network/peers?limit=1").await;
    let cursor = first["nextCursor"].as_str().unwrap();
    let (_, second) = get_json(&app, &format!("/network/peers?limit=1&cursor={cursor}")).await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        get_json(&app, "/network/peers?limit=0").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get_json(&app, "/network/peers?cursor=deadbeef").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn peers_reject_unknown_state_and_observation_before_store_access() {
    let app = create_router_without_warmup(test_config(test_store()));

    assert_eq!(
        get_json(&app, "/network/peers?state=unreachable").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get_json(&app, "/network/peers?observation=timeout").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn peers_fail_when_persisted_candidate_has_no_positive_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(ckbadger_store::CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir);
    network
        .checkpoint_crawl(
            &ActiveCrawl {
                round_id: 1,
                ..Default::default()
            },
            &[(
                b"peerA".to_vec(),
                ckbadger_store::CrawlCandidate {
                    active: Some(ActiveCandidateProbe {
                        round_id: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )],
        )
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));

    let (code, _) = get_json(&app, "/network/peers").await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    let (detail_code, _) = get_json(&app, "/network/peers/7065657241").await;
    assert_eq!(detail_code, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn peer_detail_exposes_typed_alias_evidence_and_advertisers() {
    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    let (code, body) = get_json(&app, "/network/peers/7065657243").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["crawlerDialState"], "advertisedUnverified");
    assert_eq!(
        body["observationVantage"],
        "configuredLocalCkbRpcObserverAndThisCrawler"
    );
    assert!(body["verified"].is_null());
    assert_eq!(body["lastCompleted"]["outcome"], "exhausted");
    assert_eq!(
        body["lastCompleted"]["observations"][0]["result"],
        "dialRequestFailed"
    );
    assert_eq!(body["advertisers"][0]["advertiserPeerId"], "7065657241");
    assert_eq!(body["advertisers"][0]["alias"], "addrpeerC");
    assert_eq!(body["advertisers"][0]["firstObservedAt"], 200);
    assert_eq!(body["advertisers"][0]["lastObservedAt"], 200);
    assert_eq!(body["advertisers"][0]["firstObservedRound"], 5);
    assert_eq!(body["advertisers"][0]["lastObservedRound"], 5);
    assert_eq!(body["advertisers"][0]["observationCount"], 1);
}

#[tokio::test]
async fn peer_routes_expose_addressless_direct_session_as_orthogonal_positive_evidence() {
    let network = test_network_store();
    network
        .checkpoint_crawl(
            &ActiveCrawl {
                round_id: 6,
                started_at: 240,
                last_checkpoint_at: 250,
                ..Default::default()
            },
            &[(
                b"peerD".to_vec(),
                ckbadger_store::CrawlCandidate {
                    direct_sessions: vec![DirectSessionEvidence {
                        observer_peer_id: b"local-observer".to_vec(),
                        initiator: SessionInitiator::Peer,
                        first_observed_at: 250,
                        last_observed_at: 250,
                        first_observed_round: 5,
                        last_observed_round: 5,
                        observation_count: 1,
                        client_version: "ckb-direct".into(),
                        session_addresses: vec![],
                        connected_duration_ms: 12_000,
                        last_ping_duration_ms: Some(3),
                        protocols: vec![],
                    }],
                    ..Default::default()
                },
            )],
        )
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));

    let (list_code, list) = get_json(&app, "/network/peers").await;
    assert_eq!(list_code, StatusCode::OK);
    let item = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["peerId"] == "7065657244")
        .unwrap();
    assert!(item["primaryAddr"].is_null());
    assert!(item["lastAdvertisedAt"].is_null());
    assert_eq!(item["latestPositiveObservedAt"], 250);
    assert_eq!(item["crawlerDialState"], "noCompletedObservation");
    assert_eq!(item["participation"]["directSessionObserved"], true);
    assert_eq!(item["participation"]["crawlerIdentified"], false);
    assert_eq!(item["sessionInitiators"][0], "peerInitiated");

    let (detail_code, detail) = get_json(&app, "/network/peers/7065657244").await;
    assert_eq!(detail_code, StatusCode::OK);
    assert!(detail["firstDiscoveredAt"].is_null());
    assert!(detail["lastAdvertisedAt"].is_null());
    assert!(detail["verified"].is_null());
    assert!(detail["lastCompleted"].is_null());
    assert_eq!(detail["directSessions"][0]["initiator"], "peerInitiated");
    assert_eq!(
        detail["directSessions"][0]["observerPeerId"],
        "6c6f63616c2d6f62736572766572"
    );
    assert!(detail["directSessions"][0]["sessionAddresses"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn staged_addressless_direct_session_is_hidden_until_completed_publication() {
    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(ckbadger_store::CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir);
    network
        .checkpoint_crawl(
            &ActiveCrawl {
                round_id: 1,
                started_at: 10,
                last_checkpoint_at: 10,
                direct_session_targets: vec![b"peerD".to_vec()],
                ..Default::default()
            },
            &[(
                b"peerD".to_vec(),
                ckbadger_store::CrawlCandidate {
                    staged_direct_sessions: vec![DirectSessionObservation {
                        round_id: 1,
                        observed_at: 10,
                        observer_peer_id: b"observer".to_vec(),
                        initiator: SessionInitiator::Peer,
                        client_version: "ckb-direct".into(),
                        session_addresses: vec![],
                        connected_duration_ms: 1,
                        last_ping_duration_ms: None,
                        protocols: vec![],
                    }],
                    ..Default::default()
                },
            )],
        )
        .unwrap();
    let app = create_router_without_warmup(test_config_with_network(test_store(), network, true));

    let (list_code, list) = get_json(&app, "/network/peers").await;
    assert_eq!(list_code, StatusCode::OK);
    assert!(list["items"].as_array().unwrap().is_empty());
    assert_eq!(
        get_json(&app, "/network/peers/7065657244").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn peer_routes_handle_empty_malformed_and_removed_node_contracts() {
    let empty = create_router_without_warmup(test_config(test_store()));
    let (code, page) = get_json(&empty, "/network/peers").await;
    assert_eq!(code, StatusCode::OK);
    assert!(page["items"].as_array().unwrap().is_empty());

    let app = create_router_without_warmup(test_config_with_network(
        test_store(),
        test_network_store(),
        true,
    ));
    assert_eq!(
        get_json(&app, "/network/peers/xyz").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get_json(&app, "/network/peers/deadbeef").await.0,
        StatusCode::NOT_FOUND
    );
}
