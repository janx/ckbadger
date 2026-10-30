use crate::common::*;
use ckbadger_indexer::verify::{
    api_checks::{AssetTopHoldersAddressConsistency, ObjectAssetCollectionConsistency},
    checks::{Check, CheckContext, ProgressReporter},
};

/// Exercise the public verifier against the real router/serializer, so mocks cannot
/// accidentally require a token-only field on object collections again.
#[tokio::test]
async fn asset_contract_real_api_verifies_burned_collection_without_token_transfers() {
    let store = test_store();
    let class_id = [0x39; 24];
    let mut batch = StoreBatch::new(&store);
    batch.put_mnft(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(vec![0x38; 20]),
            token_id: None,
            owner_lock_hash: None,
            name: Some("Burned collection".to_string()),
            description: None,
            is_live: false,
            created_at_block: 1,
            created_at_tx: vec![0x40; 32],
            extra: ObjectExtra::MnftClass {
                description: None,
                renderer: None,
                total: 7,
                issued: 7,
                configure: 0,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );
    for token_index in 0u32..7 {
        let mut token_id = class_id.to_vec();
        token_id.extend_from_slice(&token_index.to_be_bytes());
        batch.put_mnft(
            &token_id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(class_id.to_vec()),
                token_id: Some(token_id.clone()),
                owner_lock_hash: None,
                name: None,
                description: None,
                is_live: false,
                created_at_block: 2,
                created_at_tx: vec![0x41; 32],
                extra: ObjectExtra::MnftToken {
                    token_index,
                    characteristic: vec![0; 8],
                    configure: 0,
                    state: 0,
                },
            },
        );
        batch.put_mnft_by_collection(&class_id, &token_id);
    }
    batch.put_mnft_collection_aggregate(
        &class_id,
        &MnftCollectionAggregate {
            name: Some("Burned collection".to_string()),
            standard: ObjectStandard::MnftToken,
            total_count: 7,
            live_count: 0,
            holders_count: 0,
            activities_count: 19,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let (status, assets) = get_json(&app, "/assets?type=object").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(assets["data"].as_array().unwrap().len(), 1);
    let asset = &assets["data"][0];
    assert_eq!(asset["totalSupply"], "7");
    assert_eq!(asset["liveCount"], 0);
    assert_eq!(asset["activitiesCount"], 19);
    assert!(asset.get("transfersCount").is_none());
    let id = asset["id"].as_str().unwrap();
    let (status, items) = get_json(&app, &format!("/assets/objects/{id}/items")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items["total"], 7);
    assert_eq!(items["data"].as_array().unwrap().len(), 7);
    assert!(items["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["isLive"] == false));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let results = tokio::task::spawn_blocking(move || {
        let ctx = CheckContext {
            network: ckbadger_common::hardfork::NETWORK_MAINNET,
            api_url: format!("http://{address}/api/v1"),
            rpc_url: None,
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: None,
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        };
        let progress = ProgressReporter::new(None);
        [
            (
                ObjectAssetCollectionConsistency.name(),
                ObjectAssetCollectionConsistency.run(&ctx, &progress),
                1,
            ),
            (
                AssetTopHoldersAddressConsistency.name(),
                AssetTopHoldersAddressConsistency.run(&ctx, &progress),
                0,
            ),
        ]
    })
    .await
    .unwrap();
    server.abort();
    let _ = server.await;
    let errors: Vec<_> = results
        .iter()
        .filter_map(|(name, result, _)| {
            result
                .as_ref()
                .err()
                .map(|error| format!("{name}: {error:#}"))
        })
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    for (name, result, checked) in results {
        let result = result.unwrap();
        assert!(result.passed(), "{name}: {:?}", result.findings);
        assert_eq!(result.items_checked, checked, "{name}");
    }
}
