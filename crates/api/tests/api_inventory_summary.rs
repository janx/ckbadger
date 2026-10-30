use crate::common::*;

#[tokio::test]
async fn inventory_summary_and_pending_filter_use_full_collection_scope() {
    let store = test_store();
    let mut batch = StoreBatch::new(&store);
    for (id, tier) in [
        (0x41, CompositionTier::DecodePending),
        (0x42, CompositionTier::PureCkb),
    ] {
        batch.put_cluster_aggregate(
            &[id; 32],
            &ClusterAggregate {
                name: Some(format!("Cluster {id}")),
                total_count: 2,
                live_count: 2,
                owner_count: 1,
                pure_ckb_count: if id == 0x42 { 2 } else { 0 },
                unknown_count: if id == 0x41 { 2 } else { 0 },
                ..Default::default()
            },
        );
        for item in 0..2 {
            let mut key = [id; 32];
            key[31] = item;
            batch.put_spore(
                &key,
                &ObjectEntry {
                    standard: ObjectStandard::Spore,
                    collection_id: Some(vec![id; 32]),
                    token_id: None,
                    owner_lock_hash: Some(vec![0x51; 32]),
                    name: None,
                    description: None,
                    is_live: true,
                    created_at_block: 0,
                    created_at_tx: vec![0x61; 32],
                    extra: ObjectExtra::Spore {
                        content_type: "dob/0".into(),
                        content_length: 4,
                        media_profile: SporeMediaProfile {
                            tier,
                            sources: vec![],
                            issues: vec![],
                        },
                    },
                },
            );
        }
    }
    batch.commit().unwrap();
    for id in [0x41, 0x42] {
        store
            .put_cluster_daily_delta(
                &[id; 32],
                20261001,
                &ClusterDailyDelta {
                    owned_capacity_delta: 2000,
                    owned_knowledge_delta: 1200,
                },
            )
            .unwrap();
    }
    let app = create_router(test_config(store)).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/assets/summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["counts"]["object"], 2);
    assert_eq!(body["fullyOnchainObjectsCount"], 2);
    assert_eq!(body["source"], "canonical_asset_indexes");
    assert_eq!(body["pendingItemsCount"], 2);
    let rows = body["objectComposition"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .find(|row| row["tier"] == "decode_pending")
            .unwrap()["collectionsCount"],
        1
    );
    let (status, page) = get_json(
        &app,
        "/assets?type=object&composition_tier=decode_pending&limit=1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["total"], 1);
    assert_eq!(page["data"][0]["compositionTier"], "decode_pending");
    if let Ok(directory) = std::env::var("CKBADGER_CONTRACT_OUTPUT") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("inventory-producer.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"summary":body,"pendingPage":page}))
                .unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn catalog_visibility_ignores_zero_holders_but_finds_later_positive_holder() {
    let store = test_store();
    for id in 1..=3u8 {
        store
            .put_token_direct(
                &[id; 32],
                &TokenInfo {
                    type_code_hash: vec![0x22; 32],
                    hash_type: 1,
                    type_args: vec![0; 32],
                    standard: "sudt".into(),
                    name: if id == 3 {
                        Some("Named empty token".into())
                    } else {
                        None
                    },
                    symbol: None,
                    decimals: None,
                    max_supply: None,
                    first_seen_block: 0,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                    metadata_source: (id == 3)
                        .then_some(ckbadger_store::types::TokenMetadataSource::Label),
                    chain_metadata: None,
                },
            )
            .unwrap();
    }
    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&[1; 32], &[1; 32], 0);
    batch.put_token_holder(&[1; 32], &[2; 32], 1);
    batch.put_token_holder(&[2; 32], &[1; 32], 0);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, summary) = get_json(&app, "/assets/summary").await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["counts"]["token"], 2);
    let (status, list) = get_json(&app, "/assets?type=token").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["total"], 2);
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == format!("0x{}", hex::encode([1; 32]))));
}

#[tokio::test]
async fn standalone_dob_uses_writer_sentinel_and_is_in_inventory_scope() {
    use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;
    let store = test_store();
    let profile = ckbadger_indexer::parser::media_source::analyze_spore_media_profile(
        "dob/0",
        br#"{"dna":"0102"}"#,
        None,
        true,
    );
    assert_eq!(profile.tier, CompositionTier::DecodePending);
    // Both live insert_spore_cell and bulk ObjectOwner normalize absent parsed
    // cluster_id to this persistent sentinel and create its aggregate.
    let mut batch = StoreBatch::new(&store);
    batch.put_spore(
        &[0x19; 32],
        &ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(SOLE_SPORES_SENTINEL_COLLECTION.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x28; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 0,
            created_at_tx: vec![0x29; 32],
            extra: ObjectExtra::Spore {
                content_type: "dob/0".into(),
                content_length: 14,
                media_profile: profile,
            },
        },
    );
    batch.put_cluster_aggregate(
        &SOLE_SPORES_SENTINEL_COLLECTION,
        &ClusterAggregate {
            total_count: 1,
            live_count: 1,
            owner_count: 1,
            unknown_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    store
        .put_cluster_daily_delta(
            &SOLE_SPORES_SENTINEL_COLLECTION,
            20261001,
            &ClusterDailyDelta {
                owned_capacity_delta: 1000,
                owned_knowledge_delta: 600,
            },
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, summary) = get_json(&app, "/assets/summary").await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["counts"]["object"], 1);
    assert_eq!(summary["pendingItemsCount"], 1);
    let (_, rows) = get_json(&app, "/assets?type=object&composition_tier=decode_pending").await;
    assert_eq!(rows["total"], 1);
    assert_eq!(
        rows["data"][0]["id"],
        format!("0x{}", hex::encode(SOLE_SPORES_SENTINEL_COLLECTION))
    );
}

#[tokio::test]
async fn inventory_rejects_overlapping_composition_counts() {
    let store = test_store();
    let mut batch = StoreBatch::new(&store);
    batch.put_cluster_aggregate(
        &[0x91; 32],
        &ClusterAggregate {
            total_count: 2,
            live_count: 2,
            pure_ckb_count: 2,
            decentralized_mixture_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(&app, "/assets/summary").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body.to_string().contains("composition counts exceed live"));
    assert!(body.to_string().contains(&hex::encode([0x91; 32])));
}

#[tokio::test]
async fn inventory_rejects_pending_items_outside_unknown_aggregate() {
    let store = test_store();
    let mut batch = StoreBatch::new(&store);
    batch.put_cluster_aggregate(
        &[0x93; 32],
        &ClusterAggregate {
            total_count: 1,
            live_count: 1,
            pure_ckb_count: 1,
            ..Default::default()
        },
    );
    batch.put_spore(
        &[0x94; 32],
        &ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(vec![0x93; 32]),
            token_id: None,
            owner_lock_hash: Some(vec![0x95; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 0,
            created_at_tx: vec![0x96; 32],
            extra: ObjectExtra::Spore {
                content_type: "dob/0".into(),
                content_length: 0,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::DecodePending,
                    sources: vec![],
                    issues: vec![],
                },
            },
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(&app, "/assets/summary").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(body
        .to_string()
        .contains("pending composition exceeds aggregate"));
    assert!(body.to_string().contains(&hex::encode([0x93; 32])));
}

#[tokio::test]
async fn inventory_projection_counts_all_rows_in_a_bounded_large_fixture() {
    let store = test_store();
    let mut batch = StoreBatch::new(&store);
    let collection = [0xa1; 32];
    batch.put_cluster_aggregate(
        &collection,
        &ClusterAggregate {
            total_count: 8192,
            live_count: 8192,
            unknown_count: 8192,
            owner_count: 1,
            ..Default::default()
        },
    );
    for index in 0..8192u32 {
        let mut key = [0xa2; 32];
        key[28..].copy_from_slice(&index.to_be_bytes());
        batch.put_spore(
            &key,
            &ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(collection.to_vec()),
                token_id: None,
                owner_lock_hash: Some(vec![0xa3; 32]),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 0,
                created_at_tx: vec![0xa4; 32],
                extra: ObjectExtra::Spore {
                    content_type: "dob/0".into(),
                    content_length: 14,
                    media_profile: SporeMediaProfile {
                        tier: CompositionTier::DecodePending,
                        sources: vec![],
                        issues: vec![],
                    },
                },
            },
        );
    }
    batch.commit().unwrap();
    store
        .put_cluster_daily_delta(
            &collection,
            20261001,
            &ClusterDailyDelta {
                owned_capacity_delta: 8_192_000,
                owned_knowledge_delta: 4_915_200,
            },
        )
        .unwrap();
    let app = create_router(test_config(store)).await;
    let mut elapsed = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let (status, summary) = get_json(&app, "/assets/summary").await;
        elapsed.push(start.elapsed().as_micros());
        assert_eq!(status, StatusCode::OK, "{summary}");
        assert_eq!(summary["counts"]["object"], 1);
        assert_eq!(summary["pendingItemsCount"], 8192);
    }
    eprintln!(
        "inventory summary debug fixture: 8192 Spore rows, 1 collection, elapsed_us={elapsed:?}"
    );
}
