mod common;
use common::*;

#[tokio::test]
async fn test_charts_average_block_time_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/average-block-time")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_average_block_time_chart_recomputes_after_initial_empty_response() {
    let store = test_store();
    let config = test_config(store.clone());
    let app = create_router(config).await;

    let first_request = Request::builder()
        .uri("/api/v1/charts/average-block-time")
        .body(Body::empty())
        .unwrap();
    let first_response = app.clone().oneshot(first_request).await.unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(first_json["data"], serde_json::json!([]));

    store
        .put_daily_stats(
            "20240115",
            &DailyStats {
                block_time_sum_ms: 12_000,
                block_time_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let second_request = Request::builder()
        .uri("/api/v1/charts/average-block-time")
        .body(Body::empty())
        .unwrap();
    let second_response = app.oneshot(second_request).await.unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    let second_data = second_json["data"].as_array().unwrap();
    assert_eq!(second_data.len(), 1);
    // Canonical chart date format, converted from the RocksDB day key.
    assert_eq!(second_data[0]["date"], "2024-01-15");
    assert_eq!(second_data[0]["value"], "12.00");
}

#[tokio::test]
async fn test_new_capacity_charts_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/charts/capacity-turnover-ratio",
        "/api/v1/charts/cell-size-distribution",
        "/api/v1/charts/address-cohort-retention",
        "/api/v1/charts/most-utilized-scripts",
        "/api/v1/charts/most-utilized-assets",
    ] {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "uri={uri}");
    }
}

#[tokio::test]
async fn test_removed_cell_age_chart_routes_return_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/charts/cell-age-vs-occupied-capacity",
        "/api/v1/charts/cell-age-vs-used-capacity",
    ] {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri={uri}");
    }
}

#[tokio::test]
async fn test_most_utilized_scripts_chart_ranks_by_used_and_capacity() {
    let store = test_store();

    let code_hash_a1 = vec![0x11; 32];
    let code_hash_a2 = vec![0x12; 32];
    let code_hash_b = vec![0x21; 32];
    let code_hash_unknown = vec![0x31; 32];

    // Entities are derived from per-form reference counters; ScriptInfos
    // provide the labels for loose (non-family) references.
    for (code_hash, name) in [
        (&code_hash_a1, Some("Script A")),
        (&code_hash_a2, Some("Script A")),
        (&code_hash_b, Some("Script B")),
        (&code_hash_unknown, None),
    ] {
        store
            .put_script_info_direct(
                code_hash,
                &ScriptInfo {
                    code_hash: (*code_hash).clone(),
                    name: name.map(str::to_string),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    store
        .put_script_reference_info_direct(
            1,
            &code_hash_a1,
            &ScriptReferenceInfo {
                reference_hash: code_hash_a1.clone(),
                hash_type: 1,
                lock_cells_count: 10,
                lock_live_cells_count: 8,
                lock_owned_capacity_sum: 500,
                lock_owned_knowledge_sum: 300,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_reference_info_direct(
            1,
            &code_hash_a2,
            &ScriptReferenceInfo {
                reference_hash: code_hash_a2.clone(),
                hash_type: 1,
                type_cells_count: 6,
                type_live_cells_count: 5,
                type_owned_capacity_sum: 700,
                type_owned_knowledge_sum: 500,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_reference_info_direct(
            1,
            &code_hash_b,
            &ScriptReferenceInfo {
                reference_hash: code_hash_b.clone(),
                hash_type: 1,
                lock_cells_count: 9,
                lock_live_cells_count: 7,
                lock_owned_capacity_sum: 800,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_reference_info_direct(
            1,
            &code_hash_unknown,
            &ScriptReferenceInfo {
                reference_hash: code_hash_unknown.clone(),
                hash_type: 1,
                lock_cells_count: 4,
                lock_live_cells_count: 4,
                lock_owned_capacity_sum: 600,
                lock_owned_knowledge_sum: 550,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a1,
            1,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 500,
                owned_knowledge_delta: 300,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a2,
            1,
            true,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 700,
                owned_knowledge_delta: 500,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            1,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 800,
                owned_knowledge_delta: 200,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_unknown,
            1,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 600,
                owned_knowledge_delta: 550,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/most-utilized-scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["title"], "Scripts Used & Total CKBytes");
    // a1 and a2 share the "Script A" label but are different code hashes, so
    // they are different scripts and stay different entities.
    let used_share = &json["usedShare"];
    let used_series = used_share["series"].as_array().unwrap();
    assert_eq!(used_series.len(), 5);
    assert_eq!(
        used_series[0]["label"],
        format!("0x{} (type)", hex::encode(&code_hash_unknown))
    );
    assert_eq!(used_series[1]["label"], "Script A (type)");
    assert_eq!(used_series[2]["label"], "Script A (type)");
    assert_eq!(used_series[3]["label"], "Script B (type)");
    assert_eq!(used_series[4]["label"], "Others");

    let used_data = used_share["data"].as_array().unwrap();
    assert_eq!(used_data.len(), 1);
    assert_eq!(used_data[0]["date"], "2024-01-01");
    assert_eq!(used_data[0]["values"]["top0"], "550");
    assert_eq!(used_data[0]["values"]["top1"], "500");
    assert_eq!(used_data[0]["values"]["top2"], "300");
    assert_eq!(used_data[0]["values"]["top3"], "200");
    assert_eq!(used_data[0]["values"]["others"], "0");

    let capacity_share = &json["capacityShare"];
    let capacity_series = capacity_share["series"].as_array().unwrap();
    assert_eq!(capacity_series.len(), 5);
    assert_eq!(capacity_series[0]["label"], "Script B (type)");
    assert_eq!(capacity_series[1]["label"], "Script A (type)");
    assert_eq!(
        capacity_series[2]["label"],
        format!("0x{} (type)", hex::encode(&code_hash_unknown))
    );
    assert_eq!(capacity_series[3]["label"], "Script A (type)");
    assert_eq!(capacity_series[4]["label"], "Others");

    let capacity_data = capacity_share["data"].as_array().unwrap();
    assert_eq!(capacity_data[0]["values"]["top0"], "800");
    assert_eq!(capacity_data[0]["values"]["top1"], "700");
    assert_eq!(capacity_data[0]["values"]["top2"], "600");
    assert_eq!(capacity_data[0]["values"]["top3"], "500");
    assert_eq!(capacity_data[0]["values"]["others"], "0");
}

#[tokio::test]
async fn test_most_utilized_scripts_chart_groups_family_members_like_usage() {
    // Entity grouping must follow the same family resolution the usage
    // counters use: references resolving into a family version are ONE entity
    // (the family), even when a member reference carries a different
    // ScriptInfo label (the USDI-inside-xUDT case). Loose references without a
    // family keep their own label.
    let store = test_store();

    let version_hash = vec![0x1c; 32];
    let r_main = vec![0x2a; 32];
    let r_alt = vec![0x2b; 32];
    seed_two_reference_script_family(
        &store,
        "xUDT",
        "family:xudt",
        &version_hash,
        &r_main,
        &r_alt,
        "USDI",
    );

    store
        .put_script_daily_delta(
            &r_main,
            1,
            true,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 200,
                owned_knowledge_delta: 120,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &r_alt,
            1,
            true,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/most-utilized-scripts")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let capacity_share = &json["capacityShare"];
    let labels: Vec<String> = capacity_share["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|series| series["label"].as_str().unwrap().to_string())
        .collect();
    assert!(
        labels.contains(&"xUDT".to_string()),
        "family entity must be present, got labels {labels:?}"
    );
    assert!(
        !labels.contains(&"USDI".to_string()),
        "family member reference must not appear as its own entity, got labels {labels:?}"
    );

    let capacity_data = capacity_share["data"].as_array().unwrap();
    assert_eq!(capacity_data.len(), 1);
    let xudt_index = labels.iter().position(|label| label == "xUDT").unwrap();
    assert_eq!(
        capacity_data[0]["values"][format!("top{xudt_index}")],
        "300",
        "family entity capacity must merge all member reference forms"
    );

    let used_share = &json["usedShare"];
    let used_labels: Vec<String> = used_share["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|series| series["label"].as_str().unwrap().to_string())
        .collect();
    let used_index = used_labels
        .iter()
        .position(|label| label == "xUDT")
        .unwrap();
    assert_eq!(
        used_share["data"].as_array().unwrap()[0]["values"][format!("top{used_index}")],
        "180"
    );

    // Same-source lock: chart family totals == usage endpoint totals.
    let request = Request::builder()
        .uri("/api/v1/scripts/xUDT/usage")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let usage: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(usage["ownedCapacitySum"], "300");
    assert_eq!(usage["ownedKnowledgeSum"], "180");
}

#[tokio::test]
async fn test_most_utilized_scripts_chart_keeps_junk_form_out_of_family_bucket() {
    // Aggregation keys must be identities, not display names. A junk data-form
    // usage of a family's type reference hash (the mainnet secp case: ScriptInfo
    // is keyed by code_hash, so the junk form inherits the family-named label)
    // must stay its own entity instead of folding back into the family bucket.
    let store = test_store();

    let version_hash = vec![0x1c; 32];
    let r_main = vec![0x9b; 32];
    let r_alt = vec![0x2b; 32];
    seed_two_reference_script_family(
        &store,
        "SecpFamily",
        "family:secp",
        &version_hash,
        &r_main,
        &r_alt,
        "USDI",
    );

    store
        .put_script_daily_delta(
            &r_main,
            1,
            true,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 200,
                owned_knowledge_delta: 120,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &r_alt,
            1,
            true,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    // Junk data form: same reference bytes as the family's type form, but no
    // code cell carries data hashing to it and no persisted mapping resolves
    // it into the family. Its ScriptInfo label (keyed by code_hash) is the
    // family name, which must NOT merge it into the family bucket.
    store
        .put_script_reference_info_direct(
            0,
            &r_main,
            &ScriptReferenceInfo {
                reference_hash: r_main.clone(),
                hash_type: 0,
                lock_cells_count: 3,
                lock_live_cells_count: 3,
                lock_capacity_sum: 400,
                lock_owned_capacity_sum: 400,
                lock_used_capacity_sum: 250,
                lock_owned_knowledge_sum: 250,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &r_main,
            0,
            false,
            20240101,
            &ScriptDailyDelta {
                owned_capacity_delta: 400,
                owned_knowledge_delta: 250,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/most-utilized-scripts")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let buckets = |share: &serde_json::Value| -> Vec<(String, String)> {
        let values = &share["data"].as_array().unwrap()[0]["values"];
        share["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|series| {
                (
                    series["label"].as_str().unwrap().to_string(),
                    values[series["key"].as_str().unwrap()]
                        .as_str()
                        .unwrap()
                        .to_string(),
                )
            })
            .collect()
    };

    // Family bucket keeps the member forms only (300/180); the junk data form
    // is a separate entity (400/250) whose label carries its form. A single
    // merged 700/430 bucket is the bug.
    let capacity = buckets(&json["capacityShare"]);
    assert!(
        capacity.contains(&("SecpFamily".to_string(), "300".to_string())),
        "family bucket must keep only its member forms, got {capacity:?}"
    );
    assert!(
        capacity.contains(&("SecpFamily (data)".to_string(), "400".to_string())),
        "junk data form must be its own bucket, got {capacity:?}"
    );
    let used = buckets(&json["usedShare"]);
    assert!(
        used.contains(&("SecpFamily".to_string(), "180".to_string())),
        "family bucket must keep only its member forms, got {used:?}"
    );
    assert!(
        used.contains(&("SecpFamily (data)".to_string(), "250".to_string())),
        "junk data form must be its own bucket, got {used:?}"
    );

    // Same-source lock: one of the buckets is the family and matches the
    // usage endpoint totals exactly.
    let request = Request::builder()
        .uri("/api/v1/scripts/SecpFamily/usage")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let usage: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(usage["ownedCapacitySum"], "300");
    assert_eq!(usage["ownedKnowledgeSum"], "180");
}

#[tokio::test]
async fn test_most_utilized_assets_chart_ranks_mixed_asset_types() {
    let store = test_store();

    let token_a = vec![0x41; 32];
    let token_b = vec![0x42; 32];
    let cluster_id = vec![0x51; 32];
    let nft_collection_id = vec![0x61; 32];

    store
        .put_token_direct(
            &token_a,
            &TokenInfo {
                type_code_hash: vec![0x01; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("Token A".to_string()),
                symbol: Some("A".to_string()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_a,
            20240101,
            &TokenDailyDelta {
                owned_capacity_delta: 300,
                owned_knowledge_delta: 250,
            },
        )
        .unwrap();

    store
        .put_token_direct(
            &token_b,
            &TokenInfo {
                type_code_hash: vec![0x03; 32],
                hash_type: 1,
                type_args: vec![0x04; 20],
                standard: "xudt".to_string(),
                name: Some("Token B".to_string()),
                symbol: Some("B".to_string()),
                decimals: Some(8),
                max_supply: None,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_b,
            20240101,
            &TokenDailyDelta {
                owned_capacity_delta: 900,
                owned_knowledge_delta: 100,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: Some("DOB Cluster".to_string()),
            description: None,
            total_count: 5,
            live_count: 5,
            owner_count: 3,
            ..Default::default()
        },
    );
    batch.put_mnft_collection_aggregate(
        &nft_collection_id,
        &MnftCollectionAggregate {
            name: Some("NFT Collection".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 6,
            live_count: 6,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240101,
            &ClusterDailyDelta {
                owned_capacity_delta: 500,
                owned_knowledge_delta: 400,
            },
        )
        .unwrap();
    store
        .put_mnft_daily_delta(
            &nft_collection_id,
            20240101,
            &MnftDailyDelta {
                owned_capacity_delta: 700,
                owned_knowledge_delta: 600,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/most-utilized-assets")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["title"], "Assets Used & Total CKBytes");
    let used_share = &json["usedShare"];
    let used_series = used_share["series"].as_array().unwrap();
    assert_eq!(used_series[0]["label"], "NFT Collection (object)");
    assert_eq!(used_series[1]["label"], "DOB Cluster (object)");
    assert_eq!(used_series[2]["label"], "A (token)");
    assert_eq!(used_series[3]["label"], "B (token)");
    assert_eq!(used_series[4]["label"], "Others");

    let used_data = used_share["data"].as_array().unwrap();
    assert_eq!(used_data[0]["date"], "2024-01-01");
    assert_eq!(used_data[0]["values"]["top0"], "600");
    assert_eq!(used_data[0]["values"]["top1"], "400");
    assert_eq!(used_data[0]["values"]["top2"], "250");
    assert_eq!(used_data[0]["values"]["top3"], "100");
    assert_eq!(used_data[0]["values"]["others"], "0");

    let capacity_share = &json["capacityShare"];
    let capacity_series = capacity_share["series"].as_array().unwrap();
    assert_eq!(capacity_series[0]["label"], "B (token)");
    assert_eq!(capacity_series[1]["label"], "NFT Collection (object)");
    assert_eq!(capacity_series[2]["label"], "DOB Cluster (object)");
    assert_eq!(capacity_series[3]["label"], "A (token)");
    assert_eq!(capacity_series[4]["label"], "Others");

    let capacity_data = capacity_share["data"].as_array().unwrap();
    assert_eq!(capacity_data[0]["values"]["top0"], "900");
    assert_eq!(capacity_data[0]["values"]["top1"], "700");
    assert_eq!(capacity_data[0]["values"]["top2"], "500");
    assert_eq!(capacity_data[0]["values"]["top3"], "300");
    assert_eq!(capacity_data[0]["values"]["others"], "0");
}

#[tokio::test]
async fn test_charts_block_time_distribution_with_data() {
    let store = test_store();

    // Blocks in epoch 0 (complete) + tip in epoch 1 so epoch 0 counts
    let mut batch = StoreBatch::new(store.as_ref());
    for (number, ts_ms, epoch) in [
        (0i64, 0i64, 0i64),
        (1, 1_000, 0),
        (2, 3_000, 0),
        (3, 4_000, 1),
    ] {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                parent_hash: vec![0u8; 32],
                timestamp: ts_ms,
                epoch_number: epoch,
                epoch_index: 0,
                epoch_length: 3,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/block-time-distribution")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 501);

    // Epoch 0 deltas: 0→1 = 1s, 1→2 = 2s
    let point_1s = data.iter().find(|point| point["date"] == "1.0").unwrap();
    let point_2s = data.iter().find(|point| point["date"] == "2.0").unwrap();
    assert_eq!(point_1s["value"], "50.000");
    assert_eq!(point_2s["value"], "50.000");
}

/// Regression: every date-keyed chart emits ONE canonical date format
/// (`YYYY-MM-DD`). Five endpoints used to leak the raw RocksDB `YYYYMMDD` day
/// key because their formatter only replaced `-` with `/`, which is a no-op on
/// a dash-less key, so chart pages disagreed on date presentation.
#[tokio::test]
async fn test_date_keyed_charts_emit_canonical_iso_dates() {
    let store = test_store();

    for (date, difficulty) in [("20240115", 1_000_000.0), ("20240116", 2_000_000.0)] {
        store
            .put_daily_block_stats(
                date,
                &DailyBlockStats {
                    avg_difficulty: difficulty,
                    block_count: 100,
                    total_uncles: 2,
                    block_time_sum_ms: 100 * 10_000,
                    block_time_count: 100,
                },
            )
            .unwrap();
        store
            .put_daily_stats(
                date,
                &DailyStats {
                    blocks_count: 100,
                    transactions_count: 500,
                    cells_created: 10,
                    total_all_cells: 10,
                    total_live_cells: 8,
                    total_dead_cells: 2,
                    block_time_sum_ms: 100 * 10_000,
                    block_time_count: 100,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    for uri in [
        "/api/v1/charts/hash-rate",
        "/api/v1/charts/difficulty",
        "/api/v1/charts/uncle-rate",
        "/api/v1/charts/transaction-count",
        "/api/v1/charts/average-block-time",
        "/api/v1/charts/cell-count",
    ] {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "uri={uri}");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let data = json["data"].as_array().unwrap();
        assert!(!data.is_empty(), "uri={uri} returned no points");
        for point in data {
            let date = point["date"].as_str().unwrap();
            assert!(
                date.starts_with("2024-01-1") && date.len() == 10,
                "uri={uri} must emit YYYY-MM-DD dates, got {date}"
            );
        }
    }
}
