use crate::common::*;

/// Regression test for the "API down for a long time after restart" bug.
///
/// `warmup_assets_cache_once` performs full-store scans that previously ran on
/// the listener-bind path, so port 8101 stayed closed for the whole warmup. In
/// production (`start_background_tasks == true`) the refresh loops own the
/// single seeding path, so the bind path must do no warmup work at all —
/// `dispatch_initial_warmup(.., true)` returns without touching the caches and
/// without creating a blocking `cache_warmup` job.
#[tokio::test]
async fn test_initial_warmup_does_no_work_on_production_bind_path() {
    let store = test_store();
    let state = test_app_state(test_config(store));

    assert!(state.token_cache.load().is_none());

    // defer = true (production): the refresh loops seed the caches, not this.
    dispatch_initial_warmup(state.clone(), true).await;

    assert!(
        state.token_cache.load().is_none(),
        "production bind path must not build caches; the refresh loops do"
    );
    let has_warmup_job = {
        let data = state.background_tasks.read().unwrap();
        data.tasks.iter().any(|t| t.name == "cache_warmup")
    };
    assert!(
        !has_warmup_job,
        "production bind path must not create a blocking cache_warmup job"
    );
}

/// Counterpart: with background loops disabled (tests / embedded use), the
/// warmup must run synchronously so caches are seeded before the first request,
/// and the `cache_warmup` task surfaces it.
#[tokio::test]
async fn test_initial_warmup_blocks_when_background_tasks_disabled() {
    let store = test_store();
    let state = test_app_state(test_config(store));

    assert!(state.token_cache.load().is_none());

    // defer = false: warmup is awaited, so the cache is warm on return.
    dispatch_initial_warmup(state.clone(), false).await;

    assert!(
        state.token_cache.load().is_some(),
        "blocking warmup should seed the cache before returning"
    );
    let completed = {
        let data = state.background_tasks.read().unwrap();
        data.tasks
            .iter()
            .find(|t| t.name == "cache_warmup")
            .map(|t| t.state)
    };
    assert_eq!(
        completed,
        Some(BackgroundTaskState::Completed),
        "cache_warmup task should be marked Completed after the blocking warmup"
    );
}

/// In production the refresh loops are the single seeding path. Verify the
/// assets loop seeds the cache on its first iteration (the build the deferred
/// bind path intentionally skips), so warming is never lost — just moved off
/// the bind path with no redundant second build.
#[tokio::test]
async fn test_refresh_assets_loop_seeds_cache_in_background() {
    let store = test_store();
    let state = test_app_state(test_config(store));

    assert!(state.token_cache.load().is_none());

    let loop_state = state.clone();
    let handle =
        tokio::spawn(
            async move { ckbadger_api::warmup::refresh_assets_cache_loop(loop_state).await },
        );

    let mut populated = false;
    for _ in 0..200 {
        if state.token_cache.load().is_some() {
            populated = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    handle.abort();

    assert!(
        populated,
        "the assets refresh loop should seed the cache on its first iteration"
    );
}

/// A failed asset build is not a permanent error. While the store breaches the
/// "live items ⇒ daily capacity history" invariant, `/tokens` answers 500
/// naming the collection (fail fast, no stale list). A failure never advances
/// the loop's `last_refreshed_tip`, so the loop rebuilds on its next iteration
/// (30 s later) whatever the tip, and the first successful build clears the
/// recorded error with no restart.
///
/// Runs on the real clock: it waits out one 30 s loop interval.
#[tokio::test]
async fn failed_asset_build_is_retried_by_the_refresh_loop() {
    let store = test_store();
    let class_id = [0x3cu8; 24];
    let class_hex = hex::encode(class_id);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_mnft_collection_aggregate(
        &class_id,
        &MnftCollectionAggregate {
            standard: ObjectStandard::MnftToken,
            total_count: 1,
            live_count: 1,
            holders_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let state = test_app_state(test_config(store.clone()));
    let app = router_from_state(state.clone());
    let loop_state = state.clone();
    let handle =
        tokio::spawn(
            async move { ckbadger_api::warmup::refresh_assets_cache_loop(loop_state).await },
        );

    let recorded_error = || state.asset_cache_warmup_error.read().unwrap().clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while recorded_error().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the first build must fail on the breached store"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains(&format!(
            "object collection 0x{class_hex} has 1 live items but no daily capacity history"
        )),
        "{message}"
    );

    // The indexer's row for the class lands; the store is consistent again.
    store
        .put_mnft_daily_delta(
            &class_id,
            20240115,
            &MnftDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while recorded_error().is_some() || state.token_cache.load().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the loop must retry a failed build: {:?}",
            recorded_error()
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    handle.abort();

    let (status, json) = get_json(&app, "/tokens?limit=2").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, json) = get_json(&app, "/assets?type=object").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["data"][0]["id"], format!("0x{class_hex}"));
    assert_eq!(json["data"][0]["ownedCapacity"], "100");
}

#[tokio::test]
async fn test_router_drop_cleans_up_temp_ckb_db() {
    let store = test_store();
    let config = test_config(store);
    let db_path = std::path::PathBuf::from(config.ckb_db_path.clone());
    assert!(db_path.exists());

    let app = create_router_without_warmup(config);
    assert!(db_path.exists());

    drop(app);

    assert!(
        !db_path.exists(),
        "temporary ckb db path should be removed when router state is dropped: {}",
        db_path.display()
    );
}

#[tokio::test]
async fn test_network_stats_includes_api_background_tasks() {
    let store = test_store();
    seed_block_headers_at(&store, &[(0, 1_700_000_000_000)]);
    let mut header = store.get_block_header(0).unwrap().unwrap();
    header.compact_target = 0x190df964;
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(0, &header);
    batch.commit().unwrap();
    let mut config = test_config(store);
    let rpc = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"method":"get_tip_block_number"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "result": "0x0"
        })))
        .mount(&rpc)
        .await;
    config.ckb_rpc_url = rpc.uri();

    // Build AppState through the shared helper so this test keeps compiling as
    // state fields are added, and holds a reference to the state it serves.
    let state = test_app_state(config);

    // Register a watcher-shaped background task.
    state.update_background_task("api_cache_refresh", |entry| {
        entry.kind = BackgroundTaskKind::Watcher;
        entry.state = BackgroundTaskState::Waiting;
        entry.message = Some("Idle".to_string());
        entry.elapsed_ms = Some(2100.0);
        entry.last_success_at = Some(1_711_100_123);
        entry.last_trigger_reason = Some("tip_unchanged".to_string());
    });

    let app = axum::Router::new()
        .nest("/api/v1", api_routes())
        .with_state(state);

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // The apiBackgroundTasks field should be present with our registered task.
    let tasks = json["apiBackgroundTasks"]
        .as_array()
        .expect("apiBackgroundTasks should be an array");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["name"], "api_cache_refresh");
    assert_eq!(tasks[0]["kind"], "Watcher");
    assert_eq!(tasks[0]["state"], "Waiting");
    assert_eq!(tasks[0]["message"], "Idle");
    assert_eq!(tasks[0]["elapsedMs"], 2100.0);
    assert_eq!(tasks[0]["lastSuccessAt"], 1_711_100_123);
    assert_eq!(tasks[0]["lastTriggerReason"], "tip_unchanged");
}

/// Verify that the initial startup warmup seeds the script cache independently
/// of the (potentially slow) asset cache build, so `/api/v1/scripts` returns
/// 200 immediately after startup rather than 503.
#[tokio::test]
async fn test_scripts_cache_seeded_at_startup_independently_of_asset_cache() {
    let store = test_store();
    // Use create_router (WITH warmup) — this runs warmup_assets_cache_once
    // which should seed the script cache even though refresh_assets_cache_sync
    // no longer calls refresh_named_script_cache_sync.
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=10")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    // Must be 200 (empty list), NOT 503 (warmup_pending).
    assert_eq!(response.status(), StatusCode::OK);
}
