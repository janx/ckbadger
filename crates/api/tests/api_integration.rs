use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ckbadger_api::{create_router, AppConfig, MIGRATOR};

fn test_config(pool: sqlx::PgPool) -> AppConfig {
    AppConfig {
        pool,
        redis_url: None,
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_network: "mainnet".to_string(),
        rate_limit_per_second: Some(1000),
        rate_limit_burst: Some(2000),
        start_background_tasks: false,
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_health_endpoint(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/status")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_network_stats_endpoint(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_recent_blocks_endpoint_empty_db(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/recent-blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["blocks"].as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_recent_blocks_returns_last_24h(pool: sqlx::PgPool) {
    let now = chrono::Utc::now();
    let hash_old: [u8; 32] = [1; 32];
    let hash_recent: [u8; 32] = [2; 32];
    let parent_hash: [u8; 32] = [0; 32];
    let nonce = vec![0u8; 16];
    let dao = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO blocks (
            number, hash, parent_hash, timestamp, transactions_count, proposals_count,
            uncles_count, epoch_number, epoch_index, epoch_length,
            nonce, transactions_root, proposals_hash, extra_hash, uncles_hash,
            compact_target, version, dao
        ) VALUES ($1, $2, $3, $4, $5, 0, 0, 1, 0, 1800, $6, $2, $2, $2, $2, 0, 0, $7)
        "#,
    )
    .bind(1i64)
    .bind(&hash_old[..])
    .bind(&parent_hash[..])
    .bind(now - chrono::Duration::hours(25))
    .bind(10i32)
    .bind(&nonce)
    .bind(&dao)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO blocks (
            number, hash, parent_hash, timestamp, transactions_count, proposals_count,
            uncles_count, epoch_number, epoch_index, epoch_length,
            nonce, transactions_root, proposals_hash, extra_hash, uncles_hash,
            compact_target, version, dao
        ) VALUES ($1, $2, $3, $4, $5, 0, 0, 1, 1, 1800, $6, $2, $2, $2, $2, 0, 0, $7)
        "#,
    )
    .bind(2i64)
    .bind(&hash_recent[..])
    .bind(&hash_old[..])
    .bind(now - chrono::Duration::hours(1))
    .bind(5i32)
    .bind(&nonce)
    .bind(&dao)
    .execute(&pool)
    .await
    .unwrap();

    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/recent-blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let blocks = json["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["transactionsCount"].as_i64().unwrap(), 5);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_blocks_endpoint_empty_db(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks?limit=10")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["data"].as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_block_not_found(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks/999999999")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_total_supply_chart_empty_db(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/total-supply")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["series"].as_array().unwrap().len(), 3);
    assert_eq!(json["title"], "Total Supply");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_total_supply_chart_with_data(pool: sqlx::PgPool) {
    // total_issuance from dao field C (includes genesis burnt)
    // At genesis: 33.6B CKB = 3_360_000_000_000_000_000 shannons
    let total_issuance = "3360000000000000000";
    let total_deposit = "100000000000000000"; // 1B CKB locked in DAO
    let cumulative_burnt = "50000000000000000"; // 0.5B secondary burnt

    sqlx::query(
        r#"
        INSERT INTO dao_daily_snapshots (
            date, total_deposit, depositors_count, daily_deposit, daily_deposit_count,
            total_issuance, cumulative_burnt
        ) VALUES 
            ($1, $2::numeric, 100, 0, 0, $3::numeric, $4)
        "#,
    )
    .bind(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
    .bind(total_deposit)
    .bind(total_issuance)
    .bind(cumulative_burnt)
    .execute(&pool)
    .await
    .unwrap();

    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/total-supply")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);

    let first_day = &data[0];
    assert_eq!(first_day["date"], "2024/01/01");

    let values = &first_day["values"];

    // Verify the three series exist
    assert!(values["circulating"].is_string());
    assert!(values["locked"].is_string());
    assert!(values["burnt"].is_string());

    // Verify calculation logic:
    // total_issuance = 33.6B
    // genesis_burnt = 8.4B
    // secondary_burnt = 0.5B
    // total_burnt = 8.4B + 0.5B = 8.9B
    // circulating = 33.6B - 8.9B = 24.7B
    // locked = 1B
    // liquid (circulating in chart) = 24.7B - 1B = 23.7B

    let burnt_str = values["burnt"].as_str().unwrap();
    let locked_str = values["locked"].as_str().unwrap();
    let circulating_str = values["circulating"].as_str().unwrap();

    // burnt should be ~8.9B (8.4B genesis + 0.5B secondary)
    let burnt_val: f64 = burnt_str.parse().unwrap();
    assert!(
        (burnt_val - 8_900_000_000.0).abs() < 1.0,
        "burnt should be ~8.9B, got {}",
        burnt_val
    );

    // locked should be 1B
    let locked_val: f64 = locked_str.parse().unwrap();
    assert!(
        (locked_val - 1_000_000_000.0).abs() < 1.0,
        "locked should be 1B, got {}",
        locked_val
    );

    // circulating (liquid) should be ~23.7B
    let circulating_val: f64 = circulating_str.parse().unwrap();
    assert!(
        (circulating_val - 23_700_000_000.0).abs() < 1.0,
        "circulating should be ~23.7B, got {}",
        circulating_val
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_secondary_issuance_chart_empty_db(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/secondary-issuance")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["series"].as_array().unwrap().len(), 3);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_secondary_issuance_chart_with_data(pool: sqlx::PgPool) {
    // Cumulative secondary issuance values (in shannons)
    // Mining: 11.36%, Compensation: 20.45%, Burnt: 68.18%
    let cumulative_mining = "1136000000000000000";
    let cumulative_compensation = "2045000000000000000";
    let cumulative_burnt = "6818000000000000000";

    sqlx::query(
        r#"
        INSERT INTO dao_daily_snapshots (
            date, total_deposit, depositors_count, daily_deposit, daily_deposit_count,
            total_issuance, cumulative_mining_reward, cumulative_deposit_compensation, cumulative_burnt
        ) VALUES 
            ($1, 0, 100, 0, 0, 0, $2, $3, $4)
        "#,
    )
    .bind(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
    .bind(cumulative_mining)
    .bind(cumulative_compensation)
    .bind(cumulative_burnt)
    .execute(&pool)
    .await
    .unwrap();

    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/secondary-issuance")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);

    let first_day = &data[0];
    assert_eq!(first_day["date"], "2024/01/01");

    let values = &first_day["values"];
    let burnt_pct: f64 = values["burnt"].as_str().unwrap().parse().unwrap();
    let mining_pct: f64 = values["mining"].as_str().unwrap().parse().unwrap();
    let compensation_pct: f64 = values["compensation"].as_str().unwrap().parse().unwrap();

    // Percentages from cumulative values: mining=11.36%, compensation=20.45%, burnt=68.18%
    assert!(
        (burnt_pct - 68.18).abs() < 0.1,
        "burnt should be ~68.18%, got {}",
        burnt_pct
    );
    assert!(
        (mining_pct - 11.36).abs() < 0.1,
        "mining should be ~11.36%, got {}",
        mining_pct
    );
    assert!(
        (compensation_pct - 20.45).abs() < 0.1,
        "compensation should be ~20.45%, got {}",
        compensation_pct
    );

    let total = burnt_pct + mining_pct + compensation_pct;
    assert!(
        (total - 100.0).abs() < 0.01,
        "percentages should sum to 100%, got {}",
        total
    );
}

async fn insert_test_script(pool: &sqlx::PgPool, code_hash: &[u8], name: &str) {
    sqlx::query(
        r#"
        INSERT INTO known_scripts (code_hash, name, description, network, hash_type, tag)
        VALUES ($1, $2, 'Test script', 'mainnet', 'type', '')
        ON CONFLICT (code_hash, network, tag) DO NOTHING
        "#,
    )
    .bind(code_hash)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_script_usage_stats(
    pool: &sqlx::PgPool,
    code_hash: &[u8],
    script_kind: &str,
    cells_count: i64,
    live_cells_count: i64,
    capacity_sum: &str,
    live_capacity_sum: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO script_usage_stats (code_hash, script_kind, cells_count, live_cells_count, capacity_sum, live_capacity_sum)
        VALUES ($1, $2, $3, $4, $5::numeric, $6::numeric)
        ON CONFLICT (code_hash, script_kind) DO UPDATE SET 
            cells_count = EXCLUDED.cells_count,
            live_cells_count = EXCLUDED.live_cells_count,
            capacity_sum = EXCLUDED.capacity_sum,
            live_capacity_sum = EXCLUDED.live_capacity_sum
        "#,
    )
    .bind(code_hash)
    .bind(script_kind)
    .bind(cells_count)
    .bind(live_cells_count)
    .bind(capacity_sum)
    .bind(live_capacity_sum)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_scripts_list_returns_system_scripts(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert!(
        !data.is_empty(),
        "Should return system scripts from migration"
    );

    let names: Vec<&str> = data.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(
        names.contains(&"Default Lock"),
        "Should contain Default Lock"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_scripts_list_infers_lock_script_kind(pool: sqlx::PgPool) {
    let lock_code_hash =
        hex::decode("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();

    insert_test_script(&pool, &lock_code_hash, "Test Lock Script").await;
    insert_script_usage_stats(
        &pool,
        &lock_code_hash,
        "lock",
        1,
        1,
        "10000000000",
        "10000000000",
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?search=Test%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["scriptKind"], "lock");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_scripts_list_infers_type_script_kind(pool: sqlx::PgPool) {
    let type_code_hash =
        hex::decode("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890").unwrap();

    insert_test_script(&pool, &type_code_hash, "Test Type Script").await;
    insert_script_usage_stats(
        &pool,
        &type_code_hash,
        "type",
        1,
        1,
        "10000000000",
        "10000000000",
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?search=Test%20Type")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["scriptKind"], "type");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_scripts_list_returns_null_for_unused_script(pool: sqlx::PgPool) {
    let unused_code_hash =
        hex::decode("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();

    insert_test_script(&pool, &unused_code_hash, "Unused Script").await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?search=Unused")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert!(data[0]["scriptKind"].is_null());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_scripts_lookup_infers_script_kind(pool: sqlx::PgPool) {
    let lock_code_hash =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();

    insert_test_script(&pool, &lock_code_hash, "Lookup Test Script").await;
    insert_script_usage_stats(
        &pool,
        &lock_code_hash,
        "lock",
        1,
        1,
        "10000000000",
        "10000000000",
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"codeHashes":["0x1111111111111111111111111111111111111111111111111111111111111111"]}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let script = &json["0x1111111111111111111111111111111111111111111111111111111111111111"];
    assert_eq!(script["name"], "Lookup Test Script");
    assert_eq!(script["scriptKind"], "lock");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_detail_infers_script_kind(pool: sqlx::PgPool) {
    let lock_code_hash =
        hex::decode("2222222222222222222222222222222222222222222222222222222222222222").unwrap();

    insert_test_script(&pool, &lock_code_hash, "Detail Test Script").await;
    insert_script_usage_stats(
        &pool,
        &lock_code_hash,
        "lock",
        1,
        1,
        "10000000000",
        "10000000000",
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/Detail%20Test%20Script")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let scripts = json.as_array().unwrap();
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0]["scriptKind"], "lock");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_not_found(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/NonExistent%20Script")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_code_cell_lookup_with_type_hash(pool: sqlx::PgPool) {
    let tx_hash =
        hex::decode("abcd123456789012345678901234567890123456789012345678901234567890").unwrap();
    let type_script_hash =
        hex::decode("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
    let lock_code_hash = vec![0u8; 32];
    let lock_script_hash = vec![0u8; 32];
    let data_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash, output_index, capacity,
            lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
            type_code_hash, type_hash_type, type_args, type_script_hash,
            data_hash, data_size, status, created_at_block
        ) VALUES ($1, 0, 10000000000, $2, 0, '', $3, $2, 0, '', $4, $5, 100, 0, 1000)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_code_hash)
    .bind(&lock_script_hash)
    .bind(&type_script_hash)
    .bind(&data_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/code-cell?code_hash=0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef&hash_type=type")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json["txHash"],
        "0xabcd123456789012345678901234567890123456789012345678901234567890"
    );
    assert_eq!(json["outputIndex"], 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_code_cell_lookup_with_data_hash(pool: sqlx::PgPool) {
    let tx_hash =
        hex::decode("dcba098765432109876543210987654321098765432109876543210987654321").unwrap();
    let data_hash =
        hex::decode("fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321").unwrap();
    let lock_code_hash = vec![0u8; 32];
    let lock_script_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash, output_index, capacity,
            lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
            type_code_hash, type_hash_type, type_args, type_script_hash,
            data_hash, data_size, status, created_at_block
        ) VALUES ($1, 1, 10000000000, $2, 0, '', $3, NULL, NULL, NULL, NULL, $4, 200, 0, 2000)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_code_hash)
    .bind(&lock_script_hash)
    .bind(&data_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/code-cell?code_hash=0xfedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321&hash_type=data")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json["txHash"],
        "0xdcba098765432109876543210987654321098765432109876543210987654321"
    );
    assert_eq!(json["outputIndex"], 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_code_cell_lookup_not_found(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/code-cell?code_hash=0x0000000000000000000000000000000000000000000000000000000000000000&hash_type=type")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["txHash"].is_null());
    assert!(json["outputIndex"].is_null());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_genesis_special_burn_cell_detection(pool: sqlx::PgPool) {
    use ckbadger_common::dao::SATOSHI_PUBKEY_HASH;

    let tx_hash =
        hex::decode("e2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c").unwrap();
    let lock_code_hash = vec![0u8; 32];
    let lock_script_hash = vec![0u8; 32];
    let data_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash, output_index, capacity,
            lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
            type_code_hash, type_hash_type, type_args, type_script_hash,
            data_hash, data_size, status, created_at_block
        ) VALUES ($1, 6, 840000000000000000, $2, 0, $3, $4, NULL, NULL, NULL, NULL, $5, 0, 0, 0)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_code_hash)
    .bind(&SATOSHI_PUBKEY_HASH[..])
    .bind(&lock_script_hash)
    .bind(&data_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/cells/0xe2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c/6")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["cellType"], "genesis_special_burn");
    assert_eq!(json["virtualOccupiedCapacity"], "504000000000000000");
}

async fn insert_test_block(pool: &sqlx::PgPool, number: i64, tx_count: i32) {
    let hash = vec![number as u8; 32];
    let parent_hash = vec![(number - 1) as u8; 32];
    let dao = vec![0u8; 32];
    let nonce = vec![0u8; 16];

    sqlx::query(
        r#"
        INSERT INTO blocks (
            number, hash, parent_hash, timestamp, transactions_count, proposals_count,
            uncles_count, epoch_number, epoch_index, epoch_length,
            nonce, transactions_root, proposals_hash, extra_hash, uncles_hash,
            compact_target, version, dao
        ) VALUES ($1, $2, $3, NOW(), $4, 0, 0, 100, 50, 1800,
            $5, $2, $2, $2, $2, 0, 0, $6)
        "#,
    )
    .bind(number)
    .bind(&hash)
    .bind(&parent_hash)
    .bind(tx_count)
    .bind(&nonce)
    .bind(&dao)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_transaction(
    pool: &sqlx::PgPool,
    hash: &[u8],
    block_number: i64,
    tx_index: i32,
) {
    sqlx::query(
        r#"
        INSERT INTO transactions (
            hash, block_number, tx_index, version, inputs_count, outputs_count,
            fee, total_input_capacity, total_output_capacity, is_cellbase, timestamp, tx_size, cycles
        ) VALUES ($1, $2, $3, 0, 1, 1, 1000, 100000000, 99999000, false, NOW(), 500, 1000000)
        "#,
    )
    .bind(hash)
    .bind(block_number)
    .bind(tx_index)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_transactions_list_with_block_number_filter(pool: sqlx::PgPool) {
    insert_test_block(&pool, 100, 3).await;
    insert_test_block(&pool, 101, 2).await;

    let tx1 =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let tx2 =
        hex::decode("2222222222222222222222222222222222222222222222222222222222222222").unwrap();
    let tx3 =
        hex::decode("3333333333333333333333333333333333333333333333333333333333333333").unwrap();
    let tx4 =
        hex::decode("4444444444444444444444444444444444444444444444444444444444444444").unwrap();
    let tx5 =
        hex::decode("5555555555555555555555555555555555555555555555555555555555555555").unwrap();

    insert_test_transaction(&pool, &tx1, 100, 0).await;
    insert_test_transaction(&pool, &tx2, 100, 1).await;
    insert_test_transaction(&pool, &tx3, 100, 2).await;
    insert_test_transaction(&pool, &tx4, 101, 0).await;
    insert_test_transaction(&pool, &tx5, 101, 1).await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/transactions?block_number=100")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3, "Should return 3 transactions for block 100");

    let total = json["total"].as_i64().unwrap();
    assert_eq!(
        total, 3,
        "Total should be 3 (block's tx count), not global total"
    );

    for tx in data {
        assert_eq!(tx["blockNumber"], 100);
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_transactions_list_without_filter_returns_global_total(pool: sqlx::PgPool) {
    insert_test_block(&pool, 100, 2).await;

    let tx1 =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let tx2 =
        hex::decode("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();

    insert_test_transaction(&pool, &tx1, 100, 0).await;
    insert_test_transaction(&pool, &tx2, 100, 1).await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/transactions?limit=10")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let total = json["total"].as_i64().unwrap();
    assert_eq!(
        total, 2,
        "Total should be actual transaction count from database"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_non_genesis_satoshi_cell_not_marked_special(pool: sqlx::PgPool) {
    use ckbadger_common::dao::SATOSHI_PUBKEY_HASH;

    let tx_hash =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let lock_code_hash = vec![0u8; 32];
    let lock_script_hash = vec![0u8; 32];
    let data_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash, output_index, capacity,
            lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
            type_code_hash, type_hash_type, type_args, type_script_hash,
            data_hash, data_size, status, created_at_block
        ) VALUES ($1, 0, 10000000000, $2, 0, $3, $4, NULL, NULL, NULL, NULL, $5, 0, 0, 100)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_code_hash)
    .bind(&SATOSHI_PUBKEY_HASH[..])
    .bind(&lock_script_hash)
    .bind(&data_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/cells/0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["cellType"].is_null());
    assert!(json["virtualOccupiedCapacity"].is_null());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_usage_returns_precomputed_stats(pool: sqlx::PgPool) {
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();

    // Insert precomputed stats
    insert_script_usage_stats(
        &pool,
        &code_hash,
        "lock",
        1000,             // cells_count
        800,              // live_cells_count
        "50000000000000", // capacity_sum: 500,000 CKB
        "40000000000000", // live_capacity_sum: 400,000 CKB
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["name"], "Default Lock");
    assert_eq!(json["cellsCount"], 1000);
    assert_eq!(json["liveCellsCount"], 800);
    assert_eq!(json["capacitySum"], "50000000000000");
    assert_eq!(json["liveCapacitySum"], "40000000000000");

    // Check byDeployment array
    let by_deployment = json["byDeployment"].as_array().unwrap();
    assert_eq!(by_deployment.len(), 1);
    assert_eq!(by_deployment[0]["scriptKind"], "lock");
    assert_eq!(by_deployment[0]["cellsCount"], 1000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_usage_aggregates_multiple_deployments(pool: sqlx::PgPool) {
    // Test script with two deployments (e.g., mainnet genesis + later deployment)
    let code_hash_1 =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();

    // Insert stats for both lock and type usage
    insert_script_usage_stats(
        &pool,
        &code_hash_1,
        "lock",
        500,
        400,
        "10000000000000", // 100,000 CKB
        "8000000000000",  // 80,000 CKB
    )
    .await;

    insert_script_usage_stats(
        &pool,
        &code_hash_1,
        "type",
        100,
        50,
        "5000000000000", // 50,000 CKB
        "2500000000000", // 25,000 CKB
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Totals should be aggregated: 500+100=600, 400+50=450, etc.
    assert_eq!(json["cellsCount"], 600);
    assert_eq!(json["liveCellsCount"], 450);
    assert_eq!(json["capacitySum"], "15000000000000");
    assert_eq!(json["liveCapacitySum"], "10500000000000");

    let by_deployment = json["byDeployment"].as_array().unwrap();
    assert_eq!(by_deployment.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_usage_returns_empty_for_unknown_script(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/NonExistent%20Script/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Should return zeros, not an error
    assert_eq!(json["name"], "NonExistent Script");
    assert_eq!(json["cellsCount"], 0);
    assert_eq!(json["liveCellsCount"], 0);
    assert_eq!(json["capacitySum"], "0");
    assert_eq!(json["liveCapacitySum"], "0");
    assert!(json["byDeployment"].as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_forks_list_empty_db(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/forks")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["total"], 0);
}

async fn insert_reorg_event(
    pool: &sqlx::PgPool,
    event_type: &str,
    depth: i32,
    fork_point: i64,
    old_tip: i64,
    new_tip: i64,
) -> i32 {
    let fork_hash = vec![fork_point as u8; 32];
    let old_tip_hash = vec![old_tip as u8; 32];
    let new_tip_hash = vec![new_tip as u8; 32];

    sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO reorg_events (
            event_type, depth, fork_point_number, fork_point_hash,
            old_tip_number, old_tip_hash, new_tip_number, new_tip_hash,
            orphaned_blocks_count, orphaned_txs_count
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING id
        "#,
    )
    .bind(event_type)
    .bind(depth)
    .bind(fork_point)
    .bind(&fork_hash)
    .bind(old_tip)
    .bind(&old_tip_hash)
    .bind(new_tip)
    .bind(&new_tip_hash)
    .bind(depth)
    .bind(depth * 2)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_forks_list_with_events(pool: sqlx::PgPool) {
    insert_reorg_event(&pool, "auto", 3, 100, 103, 104).await;
    insert_reorg_event(&pool, "deep", 50, 200, 250, 255).await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/forks")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(json["total"], 2);

    let first = &data[0];
    assert_eq!(first["eventType"], "deep");
    assert_eq!(first["depth"], 50);

    let second = &data[1];
    assert_eq!(second["eventType"], "auto");
    assert_eq!(second["depth"], 3);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_fork_detail(pool: sqlx::PgPool) {
    let event_id = insert_reorg_event(&pool, "auto", 2, 100, 102, 103).await;

    let block_hash = vec![101u8; 32];
    let parent_hash = vec![100u8; 32];
    sqlx::query(
        r#"
        INSERT INTO orphaned_blocks (
            reorg_event_id, number, hash, parent_hash, timestamp, transactions_count
        ) VALUES ($1, 101, $2, $3, NOW(), 5)
        "#,
    )
    .bind(event_id)
    .bind(&block_hash)
    .bind(&parent_hash)
    .execute(&pool)
    .await
    .unwrap();

    let tx_hash = vec![0xaau8; 32];
    sqlx::query(
        r#"
        INSERT INTO orphaned_transactions (
            reorg_event_id, hash, block_number, block_hash, tx_index,
            inputs_count, outputs_count, total_capacity
        ) VALUES ($1, $2, 101, $3, 0, 2, 3, 100000000)
        "#,
    )
    .bind(event_id)
    .bind(&tx_hash)
    .bind(&block_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri(format!("/api/v1/forks/{}", event_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["event"]["id"], event_id);
    assert_eq!(json["event"]["depth"], 2);

    let orphaned_blocks = json["orphanedBlocks"].as_array().unwrap();
    assert_eq!(orphaned_blocks.len(), 1);
    assert_eq!(orphaned_blocks[0]["number"], 101);
    assert_eq!(orphaned_blocks[0]["transactionsCount"], 5);

    let orphaned_txs = json["orphanedTransactions"].as_array().unwrap();
    assert_eq!(orphaned_txs.len(), 1);
    assert_eq!(orphaned_txs[0]["blockNumber"], 101);
    assert_eq!(orphaned_txs[0]["inputsCount"], 2);
    assert_eq!(orphaned_txs[0]["outputsCount"], 3);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_fork_detail_not_found(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/forks/99999")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_forks_recent_no_events(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/forks/recent")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["hasRecentReorg"], false);
    assert!(json["reorg"].is_null());
    assert_eq!(json["deepFork"]["detected"], false);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_forks_recent_with_deep_fork(pool: sqlx::PgPool) {
    let db_tip_hash = vec![100u8; 32];
    let chain_tip_hash = vec![150u8; 32];

    sqlx::query(
        r#"
        UPDATE sync_status SET
            deep_fork_detected = TRUE,
            deep_fork_at = NOW(),
            deep_fork_db_tip = 100,
            deep_fork_db_tip_hash = $1,
            deep_fork_chain_tip = 150,
            deep_fork_chain_tip_hash = $2,
            deep_fork_depth = 50,
            deep_fork_fork_point = 100
        WHERE id = 1
        "#,
    )
    .bind(&db_tip_hash)
    .bind(&chain_tip_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/forks/recent")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["hasRecentReorg"], true);
    assert_eq!(json["deepFork"]["detected"], true);
    assert_eq!(json["deepFork"]["depth"], 50);
    assert_eq!(json["deepFork"]["dbTip"], 100);
    assert_eq!(json["deepFork"]["chainTip"], 150);
    assert_eq!(json["deepFork"]["forkPoint"], 100);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_network_stats_includes_deep_fork_status(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["deepForkStatus"].is_object());
    assert_eq!(json["deepForkStatus"]["detected"], false);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_resolve_deep_fork_requires_admin_token(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/resolve-deep-fork")
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"adminToken":"wrong","action":"dismiss"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_address_returns_lock_script_info(pool: sqlx::PgPool) {
    let lock_hash = vec![0x11u8; 32];
    let lock_code_hash = vec![0x22u8; 32];
    let lock_args = vec![0x33u8; 20];
    let tx_hash = vec![0x44u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (tx_hash, output_index, capacity, lock_script_hash, lock_code_hash, lock_hash_type, lock_args, data_size, data_hash, status, created_at_block)
        VALUES ($1, 0, 10000000000, $2, $3, 1, $4, 0, $5, 0, 100)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_hash)
    .bind(&lock_code_hash)
    .bind(&lock_args)
    .bind(vec![0u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO address_balances (lock_script_hash, balance, live_cells_count, transactions_count)
        VALUES ($1, 10000000000, 1, 1)
        "#,
    )
    .bind(&lock_hash)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO known_scripts (code_hash, name, network, deprecated, is_system)
        VALUES ($1, 'Test Lock Script', 'mainnet', false, true)
        "#,
    )
    .bind(&lock_code_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let lock_hash_hex = "0x".to_string() + &hex::encode(&lock_hash);
    let request = Request::builder()
        .uri(format!("/api/v1/addresses/{}", lock_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["lockScriptHash"], lock_hash_hex);
    assert_eq!(json["balance"], "10000000000");
    assert_eq!(json["liveCellsCount"], 1);

    assert!(json["lockScript"].is_object());
    assert_eq!(
        json["lockScript"]["codeHash"],
        "0x".to_string() + &hex::encode(&lock_code_hash)
    );
    assert_eq!(json["lockScript"]["hashType"], "type");
    assert_eq!(
        json["lockScript"]["args"],
        "0x".to_string() + &hex::encode(&lock_args)
    );

    assert!(json["lockScriptInfo"].is_object());
    assert_eq!(json["lockScriptInfo"]["name"], "Test Lock Script");
    assert_eq!(json["lockScriptInfo"]["deprecated"], false);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_address_without_known_script(pool: sqlx::PgPool) {
    let lock_hash = vec![0x55u8; 32];
    let lock_code_hash = vec![0x66u8; 32];
    let lock_args = vec![0x77u8; 20];
    let tx_hash = vec![0x88u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (tx_hash, output_index, capacity, lock_script_hash, lock_code_hash, lock_hash_type, lock_args, data_size, data_hash, status, created_at_block)
        VALUES ($1, 0, 5000000000, $2, $3, 0, $4, 0, $5, 0, 200)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_hash)
    .bind(&lock_code_hash)
    .bind(&lock_args)
    .bind(vec![0u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO address_balances (lock_script_hash, balance, live_cells_count, transactions_count)
        VALUES ($1, 5000000000, 1, 2)
        "#,
    )
    .bind(&lock_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let lock_hash_hex = "0x".to_string() + &hex::encode(&lock_hash);
    let request = Request::builder()
        .uri(format!("/api/v1/addresses/{}", lock_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["lockScriptHash"], lock_hash_hex);
    assert_eq!(json["balance"], "5000000000");

    assert!(json["lockScript"].is_object());
    assert_eq!(json["lockScript"]["hashType"], "data");

    assert!(json["lockScriptInfo"].is_null());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_address_tokens_empty(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let lock_hash = "0x".to_string() + &"00".repeat(32);
    let request = Request::builder()
        .uri(format!("/api/v1/addresses/{}/tokens", lock_hash))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["total"], 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_address_tokens_with_data(pool: sqlx::PgPool) {
    let lock_hash = vec![0x01u8; 32];
    let type_hash = vec![0x02u8; 32];
    let code_hash = vec![0x03u8; 32];

    let first_seen_tx = vec![0x05u8; 32];
    sqlx::query(
        r#"
        INSERT INTO tokens (type_script_hash, type_code_hash, type_hash_type, type_args, standard, decimals, name, symbol, total_supply, holders_count, transfers_count, first_seen_block, first_seen_tx)
        VALUES ($1, $2, 1, $3, 'sudt', 8, 'Test Token', 'TEST', '1000000000000', 1, 0, 1, $4)
        "#,
    )
    .bind(&type_hash)
    .bind(&code_hash)
    .bind(vec![0x04u8; 20])
    .bind(&first_seen_tx)
    .execute(&pool)
    .await
    .unwrap();

    let token_id: (i64,) = sqlx::query_as("SELECT id FROM tokens WHERE type_script_hash = $1")
        .bind(&type_hash)
        .fetch_one(&pool)
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
        VALUES ($1, $2, 500000000000, $3, $3)
        "#,
    )
    .bind(token_id.0)
    .bind(&lock_hash)
    .bind(vec![0x05u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let lock_hash_hex = "0x".to_string() + &hex::encode(&lock_hash);
    let request = Request::builder()
        .uri(format!("/api/v1/addresses/{}/tokens", lock_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 1);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["symbol"], "TEST");
    assert_eq!(data[0]["balance"], "500000000000");
    assert_eq!(data[0]["decimals"], 8);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_live_cells_combined_lock_and_type_filter(pool: sqlx::PgPool) {
    let lock_hash_a =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let lock_hash_b =
        hex::decode("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
    let type_hash_1 =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let type_hash_2 =
        hex::decode("2222222222222222222222222222222222222222222222222222222222222222").unwrap();
    let lock_code_hash = vec![0u8; 32];

    let tx_hash_1 =
        hex::decode("0001010101010101010101010101010101010101010101010101010101010101").unwrap();
    let tx_hash_2 =
        hex::decode("0002020202020202020202020202020202020202020202020202020202020202").unwrap();
    let tx_hash_3 =
        hex::decode("0003030303030303030303030303030303030303030303030303030303030303").unwrap();
    let tx_hash_4 =
        hex::decode("0004040404040404040404040404040404040404040404040404040404040404").unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, data_size)
        VALUES 
            ($1, 0, 100, 10000000000, $5, $9, '', $7, 0),
            ($2, 0, 101, 20000000000, $5, $9, '', $8, 0),
            ($3, 0, 102, 30000000000, $6, $9, '', $7, 0),
            ($4, 0, 103, 40000000000, $6, $9, '', NULL, 0)
        "#,
    )
    .bind(&tx_hash_1)
    .bind(&tx_hash_2)
    .bind(&tx_hash_3)
    .bind(&tx_hash_4)
    .bind(&lock_hash_a)
    .bind(&lock_hash_b)
    .bind(&type_hash_1)
    .bind(&type_hash_2)
    .bind(&lock_code_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let lock_a_hex = format!("0x{}", hex::encode(&lock_hash_a));
    let type_1_hex = format!("0x{}", hex::encode(&type_hash_1));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/live?lock_script_hash={}&type_script_hash={}",
            lock_a_hex, type_1_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 1);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(
        data[0]["txHash"],
        "0x0001010101010101010101010101010101010101010101010101010101010101"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_live_cells_lock_only_filter(pool: sqlx::PgPool) {
    let lock_hash =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let type_hash =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let lock_code_hash = vec![0u8; 32];

    let tx_hash_1 =
        hex::decode("0001010101010101010101010101010101010101010101010101010101010101").unwrap();
    let tx_hash_2 =
        hex::decode("0002020202020202020202020202020202020202020202020202020202020202").unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, data_size)
        VALUES 
            ($1, 0, 100, 10000000000, $3, $5, '', $4, 0),
            ($2, 0, 101, 20000000000, $3, $5, '', NULL, 0)
        "#,
    )
    .bind(&tx_hash_1)
    .bind(&tx_hash_2)
    .bind(&lock_hash)
    .bind(&type_hash)
    .bind(&lock_code_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let lock_hex = format!("0x{}", hex::encode(&lock_hash));

    let request = Request::builder()
        .uri(format!("/api/v1/cells/live?lock_script_hash={}", lock_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_live_cells_type_only_filter(pool: sqlx::PgPool) {
    let lock_hash_a =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let lock_hash_b =
        hex::decode("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
    let type_hash =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let lock_code_hash = vec![0u8; 32];

    let tx_hash_1 =
        hex::decode("0001010101010101010101010101010101010101010101010101010101010101").unwrap();
    let tx_hash_2 =
        hex::decode("0002020202020202020202020202020202020202020202020202020202020202").unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, data_size)
        VALUES 
            ($1, 0, 100, 10000000000, $3, $6, '', $5, 0),
            ($2, 0, 101, 20000000000, $4, $6, '', $5, 0)
        "#,
    )
    .bind(&tx_hash_1)
    .bind(&tx_hash_2)
    .bind(&lock_hash_a)
    .bind(&lock_hash_b)
    .bind(&type_hash)
    .bind(&lock_code_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let type_hex = format!("0x{}", hex::encode(&type_hash));

    let request = Request::builder()
        .uri(format!("/api/v1/cells/live?type_script_hash={}", type_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
}

async fn insert_test_spore_cluster(
    pool: &sqlx::PgPool,
    cluster_id: &[u8],
    name: &str,
    owner_lock_hash: &[u8],
    block_number: i64,
) {
    let type_script_hash = vec![0xccu8; 32];
    let tx_hash = vec![block_number as u8; 32];

    sqlx::query(
        r#"
        INSERT INTO spore_clusters (
            cluster_id, type_script_hash, name, description, owner_lock_hash,
            spores_count, created_at_block, created_at_tx
        ) VALUES ($1, $2, $3, 'Test cluster', $4, 0, $5, $6)
        "#,
    )
    .bind(cluster_id)
    .bind(&type_script_hash)
    .bind(name)
    .bind(owner_lock_hash)
    .bind(block_number)
    .bind(&tx_hash)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn insert_test_spore_cell(
    pool: &sqlx::PgPool,
    spore_id: &[u8],
    cluster_id: Option<&[u8]>,
    content_type: &str,
    content_size: i32,
    owner_lock_hash: &[u8],
    tx_hash: &[u8],
    output_index: i16,
    block_number: i64,
    is_live: bool,
) {
    let type_script_hash = vec![0xddu8; 32];

    sqlx::query(
        r#"
        INSERT INTO spore_cells (
            spore_id, type_script_hash, tx_hash, output_index, cluster_id,
            content_type, content_size, owner_lock_hash, is_live,
            created_at_block, created_at_tx
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $3)
        "#,
    )
    .bind(spore_id)
    .bind(&type_script_hash)
    .bind(tx_hash)
    .bind(output_index)
    .bind(cluster_id)
    .bind(content_type)
    .bind(content_size)
    .bind(owner_lock_hash)
    .bind(is_live)
    .bind(block_number)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_assets_dob_filter_empty_db(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["total"], 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_assets_dob_filter_returns_clusters(pool: sqlx::PgPool) {
    let cluster_id_1 = vec![0x01; 32];
    let cluster_id_2 = vec![0x02; 32];
    let owner_lock_hash = vec![0x03; 32];

    insert_test_spore_cluster(&pool, &cluster_id_1, "Collection A", &owner_lock_hash, 100).await;
    insert_test_spore_cluster(&pool, &cluster_id_2, "Collection B", &owner_lock_hash, 101).await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);

    for item in data {
        assert_eq!(item["assetType"], "dob");
        assert_eq!(item["standard"], "spore");
        assert!(item["name"].is_string());
        assert!(item["clusterId"].is_string());
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_assets_dob_filter_search_by_cluster_name(pool: sqlx::PgPool) {
    let cluster_id_1 = vec![0x01; 32];
    let cluster_id_2 = vec![0x02; 32];
    let owner_lock_hash = vec![0x03; 32];

    insert_test_spore_cluster(
        &pool,
        &cluster_id_1,
        "Alpha Collection",
        &owner_lock_hash,
        100,
    )
    .await;
    insert_test_spore_cluster(
        &pool,
        &cluster_id_2,
        "Beta Collection",
        &owner_lock_hash,
        101,
    )
    .await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob&search=alpha")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 1);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["name"], "Alpha Collection");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_assets_all_includes_tokens_and_clusters(pool: sqlx::PgPool) {
    let type_hash = vec![0x01u8; 32];
    let code_hash = vec![0x02u8; 32];
    let first_seen_tx = vec![0x03u8; 32];

    sqlx::query(
        r#"
        INSERT INTO tokens (type_script_hash, type_code_hash, type_hash_type, type_args, standard, decimals, name, symbol, total_supply, holders_count, transfers_count, first_seen_block, first_seen_tx)
        VALUES ($1, $2, 1, $3, 'xudt', 8, 'Test Token', 'TEST', '1000000000000', 10, 100, 1, $4)
        "#,
    )
    .bind(&type_hash)
    .bind(&code_hash)
    .bind(vec![0x04u8; 20])
    .bind(&first_seen_tx)
    .execute(&pool)
    .await
    .unwrap();

    let cluster_id = vec![0x11u8; 32];
    let owner_lock_hash = vec![0x05u8; 32];

    insert_test_spore_cluster(&pool, &cluster_id, "Test Collection", &owner_lock_hash, 100).await;

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/assets")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);

    let asset_types: Vec<&str> = data
        .iter()
        .filter_map(|d| d["assetType"].as_str())
        .collect();
    assert!(asset_types.contains(&"token"));
    assert!(asset_types.contains(&"dob"));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_spore_lookup_by_outpoint(pool: sqlx::PgPool) {
    let spore_id = vec![0x11; 32];
    let owner_lock_hash = vec![0x02; 32];
    let tx_hash = vec![0xaa; 32];

    insert_test_spore_cell(
        &pool,
        &spore_id,
        None,
        "image/png",
        512,
        &owner_lock_hash,
        &tx_hash,
        0,
        100,
        true,
    )
    .await;

    let result: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT spore_id FROM spore_cells WHERE tx_hash = $1 AND output_index = $2")
            .bind(&tx_hash)
            .bind(0i16)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().0, spore_id);

    let not_found: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT spore_id FROM spore_cells WHERE tx_hash = $1 AND output_index = $2")
            .bind(&tx_hash)
            .bind(999i16)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(not_found.is_none());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_address_asset_transfers_empty(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let lock_hash = "0x1111111111111111111111111111111111111111111111111111111111111111";
    let request = Request::builder()
        .uri(format!("/api/v1/addresses/{}/asset-transfers", lock_hash))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 0);
    assert!(json["data"].as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_address_asset_transfers_with_data(pool: sqlx::PgPool) {
    let lock_hash: [u8; 32] = [0x11; 32];
    let tx_hash: [u8; 32] = [0xAA; 32];
    let asset_id: [u8; 32] = [0xBB; 32];
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO address_asset_transfers (
            lock_script_hash, tx_hash, block_number, tx_index, event_index,
            asset_category, asset_type, asset_id, direction, amount, timestamp
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(&lock_hash[..])
    .bind(&tx_hash[..])
    .bind(1000i64)
    .bind(0i32)
    .bind(0i16)
    .bind("token")
    .bind("sudt")
    .bind(&asset_id[..])
    .bind(1i16)
    .bind(Some(1000000i64))
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let lock_hash_hex = format!("0x{}", hex::encode(lock_hash));
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/{}/asset-transfers",
            lock_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 1);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["assetCategory"], "token");
    assert_eq!(data[0]["assetType"], "sudt");
    assert_eq!(data[0]["direction"], "in");
    assert_eq!(data[0]["amount"], "1000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_address_asset_transfers_category_filter(pool: sqlx::PgPool) {
    let lock_hash: [u8; 32] = [0x22; 32];
    let tx_hash1: [u8; 32] = [0xAA; 32];
    let tx_hash2: [u8; 32] = [0xBB; 32];
    let asset_id: [u8; 32] = [0xCC; 32];
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO address_asset_transfers (
            lock_script_hash, tx_hash, block_number, tx_index, event_index,
            asset_category, asset_type, asset_id, direction, amount, timestamp
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(&lock_hash[..])
    .bind(&tx_hash1[..])
    .bind(1000i64)
    .bind(0i32)
    .bind(0i16)
    .bind("token")
    .bind("sudt")
    .bind(&asset_id[..])
    .bind(1i16)
    .bind(Some(1000000i64))
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO address_asset_transfers (
            lock_script_hash, tx_hash, block_number, tx_index, event_index,
            asset_category, asset_type, asset_id, direction, timestamp
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(&lock_hash[..])
    .bind(&tx_hash2[..])
    .bind(1001i64)
    .bind(0i32)
    .bind(0i16)
    .bind("dob")
    .bind("spore")
    .bind(&asset_id[..])
    .bind(1i16)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let lock_hash_hex = format!("0x{}", hex::encode(lock_hash));
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/{}/asset-transfers?category=token",
            lock_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 1);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["assetCategory"], "token");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_transaction_asset_transfers_empty(pool: sqlx::PgPool) {
    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let tx_hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{}/asset-transfers", tx_hash))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json.as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_transaction_asset_transfers_with_data(pool: sqlx::PgPool) {
    let lock_hash: [u8; 32] = [0x11; 32];
    let tx_hash: [u8; 32] = [0xDD; 32];
    let asset_id: [u8; 32] = [0xEE; 32];
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO address_asset_transfers (
            lock_script_hash, tx_hash, block_number, tx_index, event_index,
            asset_category, asset_type, asset_id, direction, amount, timestamp
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(&lock_hash[..])
    .bind(&tx_hash[..])
    .bind(2000i64)
    .bind(1i32)
    .bind(0i16)
    .bind("nft")
    .bind("mnft")
    .bind(&asset_id[..])
    .bind(2i16)
    .bind(Some(1i64))
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let mut config = test_config(pool);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let tx_hash_hex = format!("0x{}", hex::encode(tx_hash));
    let request = Request::builder()
        .uri(format!(
            "/api/v1/transactions/{}/asset-transfers",
            tx_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json.as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["assetCategory"], "nft");
    assert_eq!(data[0]["assetType"], "mnft");
    assert_eq!(data[0]["direction"], "out");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_live_cells_type_code_hash_filter(pool: sqlx::PgPool) {
    let lock_hash =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let dao_code_hash =
        hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e").unwrap();
    let other_code_hash =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let type_hash_dao =
        hex::decode("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd").unwrap();
    let type_hash_other =
        hex::decode("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee").unwrap();
    let lock_code_hash = vec![0u8; 32];

    let tx_hash_1 =
        hex::decode("0001010101010101010101010101010101010101010101010101010101010101").unwrap();
    let tx_hash_2 =
        hex::decode("0002020202020202020202020202020202020202020202020202020202020202").unwrap();
    let tx_hash_3 =
        hex::decode("0003030303030303030303030303030303030303030303030303030303030303").unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, type_code_hash, data_size)
        VALUES 
            ($1, 0, 100, 10000000000, $4, $7, '', $5, $8, 8),
            ($2, 0, 101, 20000000000, $4, $7, '', $5, $8, 8),
            ($3, 0, 102, 30000000000, $4, $7, '', $6, $9, 0)
        "#,
    )
    .bind(&tx_hash_1)
    .bind(&tx_hash_2)
    .bind(&tx_hash_3)
    .bind(&lock_hash)
    .bind(&type_hash_dao)
    .bind(&type_hash_other)
    .bind(&lock_code_hash)
    .bind(&dao_code_hash)
    .bind(&other_code_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let lock_hex = format!("0x{}", hex::encode(&lock_hash));
    let dao_code_hex = format!("0x{}", hex::encode(&dao_code_hash));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/live?lock_script_hash={}&type_code_hash={}",
            lock_hex, dao_code_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert!(data[0]["typeCodeHash"]
        .as_str()
        .unwrap()
        .ends_with("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cell_detail_returns_dao_info_for_deposit(pool: sqlx::PgPool) {
    let tx_hash =
        hex::decode("aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd").unwrap();
    let lock_hash =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let dao_code_hash =
        hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e").unwrap();
    let type_hash =
        hex::decode("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd").unwrap();
    let data_hash = vec![0u8; 32];
    let lock_code_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (tx_hash, output_index, created_at_block, capacity, status, 
                          lock_script_hash, lock_code_hash, lock_hash_type, lock_args,
                          type_script_hash, type_code_hash, type_hash_type, type_args,
                          data_hash, data_size, data)
        VALUES ($1, 0, 5000000, 50000000000, 0, $2, $5, 1, '', $3, $4, 1, '', $6, 8, E'\\x0000000000000000')
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_hash)
    .bind(&type_hash)
    .bind(&dao_code_hash)
    .bind(&lock_code_hash)
    .bind(&data_hash)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO dao_deposits (tx_hash, output_index, lock_script_hash, capacity,
                                 deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, status)
        VALUES ($1, 0, $2, 50000000000, 5000000, $1, '2024-01-15 10:30:00+00', 10000000000000000, 0)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
    let request = Request::builder()
        .uri(format!("/api/v1/cells/{}/0", tx_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["daoInfo"].is_object());
    assert_eq!(json["daoInfo"]["isDaoCell"], true);
    assert_eq!(json["daoInfo"]["daoStatus"], "deposited");
    assert_eq!(json["daoInfo"]["depositBlockNumber"], 5000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cell_detail_dao_info_lookup_by_withdraw_request_tx(pool: sqlx::PgPool) {
    let deposit_tx_hash =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let withdraw_request_tx =
        hex::decode("2222222222222222222222222222222222222222222222222222222222222222").unwrap();
    let lock_hash =
        hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let dao_code_hash =
        hex::decode("82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e").unwrap();
    let type_hash =
        hex::decode("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd").unwrap();
    let data_hash = vec![0u8; 32];
    let lock_code_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (tx_hash, output_index, created_at_block, capacity, status, 
                          lock_script_hash, lock_code_hash, lock_hash_type, lock_args,
                          type_script_hash, type_code_hash, type_hash_type, type_args,
                          data_hash, data_size, data)
        VALUES ($1, 0, 5050000, 50000000000, 0, $2, $5, 1, '', $3, $4, 1, '', $6, 8, E'\\x8040490000000000')
        "#,
    )
    .bind(&withdraw_request_tx)
    .bind(&lock_hash)
    .bind(&type_hash)
    .bind(&dao_code_hash)
    .bind(&lock_code_hash)
    .bind(&data_hash)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO dao_deposits (tx_hash, output_index, lock_script_hash, capacity,
                                 deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, 
                                 status, withdraw_request_block, withdraw_request_tx, withdraw_request_timestamp, withdraw_request_ar)
        VALUES ($1, 0, $3, 50000000000, 5000000, $1, '2024-01-15 10:30:00+00', 10000000000000000, 
                1, 5050000, $2, '2024-02-15 10:30:00+00', 10100000000000000)
        "#,
    )
    .bind(&deposit_tx_hash)
    .bind(&withdraw_request_tx)
    .bind(&lock_hash)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let tx_hash_hex = format!("0x{}", hex::encode(&withdraw_request_tx));
    let request = Request::builder()
        .uri(format!("/api/v1/cells/{}/0", tx_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["daoInfo"].is_object());
    assert_eq!(json["daoInfo"]["isDaoCell"], true);
    assert_eq!(json["daoInfo"]["daoStatus"], "withdrawing");
    assert_eq!(json["daoInfo"]["depositBlockNumber"], 5000000);
    assert_eq!(json["daoInfo"]["withdrawRequestBlock"], 5050000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_assets_dob_returns_real_statistics(pool: sqlx::PgPool) {
    let cluster_id = vec![0x01; 32];
    let owner_lock_hash = vec![0x03; 32];

    insert_test_spore_cluster(
        &pool,
        &cluster_id,
        "Stats Test Collection",
        &owner_lock_hash,
        100,
    )
    .await;

    let owner1 = vec![0x10; 32];
    let owner2 = vec![0x20; 32];
    let owner3 = vec![0x30; 32];

    for (i, owner) in [&owner1, &owner2, &owner3].iter().enumerate() {
        let spore_id = vec![0x50 + i as u8; 32];
        let tx_hash = vec![0x60 + i as u8; 32];
        insert_test_spore_cell(
            &pool,
            &spore_id,
            Some(&cluster_id),
            "dob/0",
            256,
            owner,
            &tx_hash,
            0,
            100 + i as i64,
            true,
        )
        .await;
    }

    sqlx::query("UPDATE spore_clusters SET spores_count = 3 WHERE cluster_id = $1")
        .bind(&cluster_id)
        .execute(&pool)
        .await
        .unwrap();

    for i in 0u8..5 {
        let tx_hash = vec![0x70 + i; 32];
        let from_lock: Option<Vec<u8>> = if i == 0 {
            None
        } else {
            Some(vec![0x80 + i; 32])
        };
        let to_lock = vec![0x90 + i; 32];

        sqlx::query(
            r#"
            INSERT INTO dob_transfers (
                dob_id, cluster_id, dob_type, tx_hash, block_number,
                from_lock_hash, to_lock_hash, event_type, content_type, timestamp
            ) VALUES ($1, $2, 'dob/0', $3, $4, $5, $6, $7, 'dob/0', NOW())
            "#,
        )
        .bind(vec![0x50u8; 32])
        .bind(&cluster_id)
        .bind(&tx_hash)
        .bind(100i64 + i64::from(i))
        .bind(from_lock.as_deref())
        .bind(&to_lock)
        .bind(if i == 0 { "mint" } else { "transfer" })
        .execute(&pool)
        .await
        .unwrap();
    }

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);

    let dob = &data[0];
    assert_eq!(dob["name"], "Stats Test Collection");
    assert_eq!(dob["holdersCount"], 3);
    assert_eq!(dob["transfersCount"], 5);
    assert_eq!(dob["totalSupply"], "3");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_assets_nft_returns_real_statistics(pool: sqlx::PgPool) {
    let class_id = vec![0x01u8; 32];
    let issuer_id = vec![0x02u8; 32];
    let owner_lock_hash = vec![0x03u8; 32];
    let type_script_hash = vec![0x04u8; 32];
    let tx_hash = vec![0x05u8; 32];

    sqlx::query(
        r#"
        INSERT INTO mnft_classes (
            class_id, type_script_hash, issuer_id, name, description,
            total, issued, owner_lock_hash, is_live, created_at_block, created_at_tx
        ) VALUES ($1, $2, $3, 'NFT Stats Collection', 'Test NFT', 100, 10, $4, TRUE, 100, $5)
        "#,
    )
    .bind(&class_id)
    .bind(&type_script_hash)
    .bind(&issuer_id)
    .bind(&owner_lock_hash)
    .bind(&tx_hash)
    .execute(&pool)
    .await
    .unwrap();

    for i in 0u8..7 {
        let tx_hash = vec![0x70 + i; 32];
        let from_lock: Option<Vec<u8>> = if i == 0 {
            None
        } else {
            Some(vec![0x80 + i; 32])
        };
        let to_lock = vec![0x90 + i; 32];
        let token_id = vec![0xA0 + i; 32];

        sqlx::query(
            r#"
            INSERT INTO nft_transfers (
                nft_id, nft_type, issuer_id, class_id, tx_hash, block_number,
                from_lock_hash, to_lock_hash, event_type, name, timestamp
            ) VALUES ($1, 'mnft', $2, $3, $4, $5, $6, $7, $8, 'Test NFT', NOW())
            "#,
        )
        .bind(&token_id)
        .bind(&issuer_id)
        .bind(&class_id)
        .bind(&tx_hash)
        .bind(100i64 + i64::from(i))
        .bind(from_lock.as_deref())
        .bind(&to_lock)
        .bind(if i == 0 { "mint" } else { "transfer" })
        .execute(&pool)
        .await
        .unwrap();
    }

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);

    let nft = &data[0];
    assert_eq!(nft["name"], "NFT Stats Collection");
    assert_eq!(nft["assetType"], "nft");
    assert_eq!(nft["standard"], "m-nft");
    assert_eq!(nft["holdersCount"], 7);
    assert_eq!(nft["transfersCount"], 7);
    assert_eq!(nft["totalSupply"], "100");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_tasks_active_no_running_tasks(pool: sqlx::PgPool) {
    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/tasks/active")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["indexRebuild"].is_null());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_tasks_active_with_running_index_rebuild(pool: sqlx::PgPool) {
    let task_id = uuid::Uuid::new_v4();
    let config = serde_json::json!({
        "type": "index_rebuild",
        "parallelConnections": 10,
        "rebuildConstraints": true
    });
    let result = serde_json::json!({
        "totalIndexes": 26,
        "completedIndexes": 10,
        "currentIndex": "idx_blocks_timestamp",
        "completed": [
            {"name": "idx_1", "durationMs": 1000},
            {"name": "idx_2", "durationMs": 2000}
        ],
        "failed": [{"name": "idx_bad", "error": "some error"}],
        "totalConstraints": 5,
        "completedConstraints": 0
    });

    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, task_type, status, priority, config, 
            progress_total, progress_current, result, started_at
        ) VALUES ($1, 'index_rebuild', 'running', 10, $2, 26, 10, $3, NOW())
        "#,
    )
    .bind(task_id)
    .bind(&config)
    .bind(&result)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/tasks/active")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let rebuild = &json["indexRebuild"];
    assert!(rebuild["isRebuilding"].as_bool().unwrap());
    assert_eq!(rebuild["total"], 26);
    assert_eq!(rebuild["completed"], 10);
    assert_eq!(rebuild["currentIndex"], "idx_blocks_timestamp");
    assert_eq!(rebuild["failed"].as_array().unwrap().len(), 1);
    assert_eq!(rebuild["failed"][0], "idx_bad");
    assert!((rebuild["progress"].as_f64().unwrap() - 38.46).abs() < 0.1);
    assert!(rebuild["startedAt"].is_string());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_tasks_active_ignores_completed_tasks(pool: sqlx::PgPool) {
    let task_id = uuid::Uuid::new_v4();
    let config = serde_json::json!({"type": "index_rebuild"});

    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, task_type, status, priority, config, 
            progress_total, progress_current, started_at, completed_at
        ) VALUES ($1, 'index_rebuild', 'completed', 10, $2, 26, 26, NOW(), NOW())
        "#,
    )
    .bind(task_id)
    .bind(&config)
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(test_config(pool)).await;

    let request = Request::builder()
        .uri("/api/v1/tasks/active")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["indexRebuild"].is_null());
}
