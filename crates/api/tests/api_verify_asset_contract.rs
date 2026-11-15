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

const RACE_CLASS: [u8; 24] = [0x51; 24];

/// A burned m-NFT class of `total` tokens: no live item, no holder, so the
/// object checks read counts only.
fn put_burned_class(store: &CkbadgerStore, total: u32) {
    let mut batch = StoreBatch::new(store);
    batch.put_mnft(
        &RACE_CLASS,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(vec![0x50; 20]),
            token_id: None,
            owner_lock_hash: None,
            name: Some("Raced collection".to_string()),
            description: None,
            is_live: false,
            created_at_block: 1,
            created_at_tx: vec![0x52; 32],
            extra: ObjectExtra::MnftClass {
                description: None,
                renderer: None,
                total,
                issued: total,
                configure: 0,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );
    for token_index in 0..total {
        let mut token_id = RACE_CLASS.to_vec();
        token_id.extend_from_slice(&token_index.to_be_bytes());
        batch.put_mnft(
            &token_id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(RACE_CLASS.to_vec()),
                token_id: Some(token_id.clone()),
                owner_lock_hash: None,
                name: None,
                description: None,
                is_live: false,
                created_at_block: 2,
                created_at_tx: vec![0x53; 32],
                extra: ObjectExtra::MnftToken {
                    token_index,
                    characteristic: vec![0; 8],
                    configure: 0,
                    state: 0,
                },
            },
        );
        batch.put_mnft_by_collection(&RACE_CLASS, &token_id);
    }
    batch.put_mnft_collection_aggregate(
        &RACE_CLASS,
        &MnftCollectionAggregate {
            name: Some("Raced collection".to_string()),
            standard: ObjectStandard::MnftToken,
            total_count: i64::from(total),
            live_count: 0,
            holders_count: 0,
            activities_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
}

/// Serve `state`'s production router and run the object list/detail check
/// against it; `during` runs beside the check, after it has started.
async fn run_object_check_against(
    state: Arc<AppState>,
    during: impl std::future::Future<Output = ()>,
) -> ckbadger_indexer::verify::checks::CheckResult {
    let app = router_from_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let check = tokio::task::spawn_blocking(move || {
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
        ObjectAssetCollectionConsistency.run(&ctx, &ProgressReporter::new(None))
    });
    during.await;
    let result = check.await.unwrap();
    server.abort();
    let _ = server.await;
    result.expect("the check must reach a verdict")
}

/// Regression (mainnet 2026-11-15, `detail holdersCount=865 != list
/// holdersCount=864` on USDC): the real router serves a list snapshot one
/// block behind the store. The verifier must not report the block that landed
/// in between as a mismatch; once the API rebuilds the snapshot, the two agree.
#[tokio::test]
async fn asset_contract_real_api_list_snapshot_behind_the_tip_is_not_a_mismatch() {
    let store = test_store();
    put_burned_class(&store, 7);
    seed_block_headers_at(&store, &[(0, 1_000), (1, 2_000), (2, 3_000)]);
    let state = test_app_state(test_config(store.clone()));
    dispatch_initial_warmup(state.clone(), false).await;

    // Block 3 mints and burns an eighth token after the snapshot was built.
    put_burned_class(&store, 8);
    seed_block_headers_at(&store, &[(3, 4_000)]);

    let refresh = {
        let state = state.clone();
        async move {
            // After the check's first re-read, as the API's refresh loop would.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            ckbadger_api::warmup::warmup_assets_cache_once(state)
                .await
                .unwrap();
        }
    };
    let result = run_object_check_against(state, refresh).await;
    assert!(result.passed(), "{result:?}");
    assert_eq!(result.items_checked, 1);
    assert!(
        result
            .detail
            .as_deref()
            .unwrap()
            .contains("1 asset(s) agreed on a re-read"),
        "{result:?}"
    );
}

/// The counterpart: a list row that differs from the store at the very block
/// the snapshot names is a real mismatch, and still fails.
#[tokio::test]
async fn asset_contract_real_api_mismatch_at_the_snapshot_block_is_a_failure() {
    let store = test_store();
    put_burned_class(&store, 7);
    seed_block_headers_at(&store, &[(0, 1_000), (1, 2_000), (2, 3_000)]);
    let state = test_app_state(test_config(store.clone()));
    dispatch_initial_warmup(state.clone(), false).await;

    // The store changes under an unchanged tip: no block explains it.
    put_burned_class(&store, 8);

    let result = run_object_check_against(state, async {}).await;
    assert!(!result.passed(), "{result:?}");
    let details: Vec<&str> = result
        .findings
        .iter()
        .flat_map(|finding| &finding.details)
        .map(String::as_str)
        .collect();
    assert!(
        details.contains(&"detail totalCount=8 != list totalSupply=7"),
        "{details:?}"
    );
    assert!(
        details
            .iter()
            .any(|detail| detail.starts_with("every read at block 2 ")),
        "{details:?}"
    );
}
