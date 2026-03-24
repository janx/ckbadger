use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ckbadger_api::cache::{CacheBackend, InMemoryCache};
use ckbadger_api::cycles::CyclesClient;
use ckbadger_api::routes::api_routes;
use ckbadger_api::utils::address::compute_script_hash;
use ckbadger_api::ws::WsManager;
use ckbadger_api::{create_router, AppConfig, AppState, CleanupPathGuard};
use ckbadger_common::{BackgroundTaskKind, BackgroundTaskState};
use ckbadger_indexer::label_import::run_label_import_bundled;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AssetAction, CachedBlockHeader, ClusterAggregate, ClusterDailyDelta, CompositionTier,
    DailyBlockStats, DailyStats, DaoDailySnapshot, DaoDepositCacheEntry, DeepForkInfo,
    DobDecodedEntry, DobDecodedTrait, EpochStats, HourlyStats, IdentityCollectionAggregate,
    IdentityEntry, IdentityExtra, IdentityStandard, LiveCellInfo, MinerStats,
    ObjectCollectionActivityEntry, ObjectCollectionAggregate, ObjectDailyDelta, ObjectEntry,
    ObjectExtra, ObjectStandard, ProtocolAction, ReorgEvent, ScriptDailyDelta, ScriptFamilyInfo,
    ScriptInfo, ScriptReferenceInfo, ScriptVersionInfo, SporeDailyDelta, SporeMediaProfile,
    TokenDailyDelta, TokenInfo, TxActions, TxIndexEntry, TypeCallEntry,
};
use ckbadger_store::CkbadgerStore;

fn test_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

fn test_append_only_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

fn split_test_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
    let store = test_store();
    (store.clone(), store)
}

struct TestCkbDb {
    path: String,
    cleanup: Arc<CleanupPathGuard>,
}

fn test_ckb_db_path() -> TestCkbDb {
    let db_path = std::env::temp_dir().join(format!("ckbadger-api-test-ckb-db-{}", Uuid::new_v4()));
    let mut db_opts = Options::default();
    db_opts.create_if_missing(true);
    db_opts.create_missing_column_families(true);
    let cf_descriptors: Vec<ColumnFamilyDescriptor> = (0..=18)
        .map(|index| ColumnFamilyDescriptor::new(index.to_string(), Options::default()))
        .collect();
    let _db = DB::open_cf_descriptors(&db_opts, &db_path, cf_descriptors).unwrap();
    TestCkbDb {
        path: db_path.to_string_lossy().to_string(),
        cleanup: Arc::new(CleanupPathGuard::new(db_path)),
    }
}

fn test_config_with_append_only(
    store: Arc<CkbadgerStore>,
    append_only_store: Arc<CkbadgerStore>,
) -> AppConfig {
    let ckb_db = test_ckb_db_path();
    test_config_with_ckb_db_path(store, append_only_store, ckb_db.path, Some(ckb_db.cleanup))
}

fn test_config_with_ckb_db_path(
    store: Arc<CkbadgerStore>,
    append_only_store: Arc<CkbadgerStore>,
    ckb_db_path: String,
    ckb_db_cleanup: Option<Arc<CleanupPathGuard>>,
) -> AppConfig {
    AppConfig {
        append_only_store,
        store,
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_network: "mainnet".to_string(),
        rate_limit_per_second: Some(1000),
        rate_limit_burst: Some(2000),
        start_background_tasks: false,
        ckb_db_path,
        ckb_db_cleanup,
        media_dir: std::path::PathBuf::from("/tmp/ckbadger-test-media"),
    }
}

fn test_config(store: Arc<CkbadgerStore>) -> AppConfig {
    test_config_with_append_only(store.clone(), store)
}

fn create_router_without_warmup(config: AppConfig) -> axum::Router {
    let state = Arc::new(AppState {
        store: config.store,
        append_only_store: config.append_only_store,
        ws_manager: Arc::new(WsManager::new()),
        cache: CacheBackend::new(),
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        cycles_client: CyclesClient::disabled(),
        ckb_store: None,
        ckb_db_cleanup: config.ckb_db_cleanup,
        mem_cache: InMemoryCache::new(),
        asset_cache_warmup_error: Arc::new(std::sync::RwLock::new(None)),
        background_tasks: Arc::new(std::sync::RwLock::new(Default::default())),
        media_dir: config.media_dir,
    });

    axum::Router::new()
        .nest("/api/v1", api_routes())
        .with_state(state)
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

fn compute_blake2b_data_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = ckb_hash::new_blake2b();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    hash.to_vec()
}

fn pending_tx_hash_hex() -> String {
    format!("0x{}", "ab".repeat(32))
}

fn pending_previous_output_hash_hex() -> String {
    format!("0x{}", "cd".repeat(32))
}

fn pending_tx_pool_timestamp_hex() -> &'static str {
    "0x18bcfe5687b"
}

fn pending_transaction_rpc_response(hash: &str, status: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "transaction": {
                "hash": hash,
                "version": "0x0",
                "cell_deps": [],
                "header_deps": [],
                "inputs": [
                    {
                        "previous_output": {
                            "tx_hash": pending_previous_output_hash_hex(),
                            "index": "0x0"
                        },
                        "since": "0x0"
                    }
                ],
                "outputs": [
                    {
                        "capacity": "0x174876e800",
                        "lock": {
                            "code_hash": format!("0x{}", "11".repeat(32)),
                            "hash_type": "type",
                            "args": format!("0x{}", "22".repeat(20))
                        },
                        "type": null
                    }
                ],
                "outputs_data": ["0x"],
                "witnesses": ["0x5500000010000000550000004100000000000000"]
            },
            "cycles": "0x5208",
            "fee": "0x174",
            "time_added_to_pool": pending_tx_pool_timestamp_hex(),
            "min_replace_fee": "0x175",
            "tx_status": {
                "status": status,
                "block_hash": null,
                "block_number": null,
                "reason": null
            }
        }
    })
}

async fn mount_pending_transaction_rpc(server: &MockServer, hash: &str, status: &str) {
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "method": "get_transaction"
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(pending_transaction_rpc_response(hash, status)),
        )
        .mount(server)
        .await;
}

fn insert_committed_transaction(store: &Arc<CkbadgerStore>, tx_hash: &[u8]) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        321,
        &CachedBlockHeader {
            hash: vec![0x44; 32],
            timestamp: 1_700_000_123_456,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_tx_hash_map(tx_hash, 321, 0);
    batch.put_tx_index(
        321,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_123_456,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1234,
            tx_size: 222,
            cycles: Some(333),
        },
    );
    batch.commit().unwrap();
}

/// Create a TxActions with one participant for testing.
fn make_test_tx_actions(
    lock_hash: &[u8],
    tx_hash: &[u8],
    block_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    ckb_delta: i128,
    tags: u16,
) -> TxActions {
    use ckbadger_store::types::ParticipantDelta;
    TxActions {
        tx_hash: tx_hash.to_vec(),
        block_hash: block_hash.to_vec(),
        block_number: block_num,
        tx_index: tx_idx,
        timestamp: 1_700_000_000 + block_num,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            lock_hash: lock_hash.to_vec(),
            ckb_delta,
            used_delta: 0,
            item_deltas: vec![],
            tags,
        }],
    }
}

/// Create a participant delta for multi-participant TxActions.
fn make_test_participant(lock_byte: u8, ckb_delta: i128, tags: u16) -> ckbadger_store::types::ParticipantDelta {
    ckbadger_store::types::ParticipantDelta {
        lock_hash: vec![lock_byte; 32],
        ckb_delta,
        used_delta: 0,
        item_deltas: vec![],
        tags,
    }
}

#[tokio::test]
async fn test_network_stats_returns_ok() {
    let store = test_store();
    let mut config = test_config(store);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_hardforks_endpoint_returns_default_timeline() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/hardforks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["network"], "mainnet");
    assert_eq!(json["tipEpoch"], 0);
    assert_eq!(json["tipBlock"], 0);
    assert!(json["events"].as_array().unwrap().len() >= 2);
    assert_eq!(json["events"][0]["status"], "upcoming");
    assert_eq!(json["events"][1]["status"], "upcoming");
}

#[tokio::test]
async fn test_hardforks_endpoint_marks_activated_and_fills_activation_block() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .put_epoch_stats(
            5414,
            &EpochStats {
                epoch_number: 5414,
                start_block: 8_775_638,
                end_block: None,
                blocks_count: 1800,
                length: 1800,
                start_timestamp: chrono::Utc::now(),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_block_header(
        19_000_000,
        &CachedBlockHeader {
            hash: vec![0xaa; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 13_000,
            epoch_index: 100,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/hardforks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["tipEpoch"], 13_000);
    assert_eq!(json["tipBlock"], 19_000_000);
    assert_eq!(json["events"][0]["id"], "mirana-2021");
    assert_eq!(json["events"][0]["status"], "activated");
    assert_eq!(json["events"][0]["activationBlock"], 8_775_638);
    assert_eq!(json["events"][1]["id"], "meepo-2024");
    assert_eq!(json["events"][1]["status"], "activated");
}

#[tokio::test]
async fn test_hardforks_endpoint_rejects_unknown_network() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/hardforks?network=devnet")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_forks_recent_fails_on_deep_fork_invariant_violation() {
    let store = test_store();
    store
        .update_sync_status(|s| {
            s.deep_fork_detected = true;
            s.deep_fork_info = None;
        })
        .unwrap();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/forks/recent")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("deep_fork_detected=true but deep_fork_info is missing"));
}

#[tokio::test]
async fn test_forks_uses_persisted_reorg_detected_at_timestamp() {
    let store = test_store();
    store
        .set_deep_fork(DeepForkInfo {
            db_tip: 100,
            db_tip_hash: vec![0x11; 32],
            chain_tip: 160,
            chain_tip_hash: vec![0x22; 32],
            depth: 60,
            fork_point: 100,
        })
        .unwrap();

    let detected_at = 1_700_000_123i64;
    let event = ReorgEvent {
        detected_at,
        rollback_from: 101,
        rollback_to: 100,
        depth: 60,
    };
    store
        .put_cf(
            store.cf_sync_meta(),
            ckbadger_store::keys::sync_meta_keys::REORG_LATEST_EVENT,
            &bincode::serialize(&event).unwrap(),
        )
        .unwrap();

    let expected_detected_at = chrono::DateTime::<chrono::Utc>::from_timestamp(detected_at, 0)
        .unwrap()
        .to_rfc3339();

    let config = test_config(store);
    let app = create_router(config).await;

    let request_recent = Request::builder()
        .uri("/api/v1/forks/recent")
        .body(Body::empty())
        .unwrap();
    let response_recent = app.clone().oneshot(request_recent).await.unwrap();
    assert_eq!(response_recent.status(), StatusCode::OK);
    let body_recent = response_recent
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json_recent: serde_json::Value = serde_json::from_slice(&body_recent).unwrap();
    assert_eq!(json_recent["reorg"]["detectedAt"], expected_detected_at);

    let request_list = Request::builder()
        .uri("/api/v1/forks")
        .body(Body::empty())
        .unwrap();
    let response_list = app.oneshot(request_list).await.unwrap();
    assert_eq!(response_list.status(), StatusCode::OK);
    let body_list = response_list
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json_list: serde_json::Value = serde_json::from_slice(&body_list).unwrap();
    assert_eq!(json_list["data"][0]["detectedAt"], expected_detected_at);
}

#[tokio::test]
async fn test_recent_blocks_endpoint_empty_db() {
    let store = test_store();
    let mut config = test_config(store);
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/recent-blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["blocks"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_tx_stats_reads_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let this_hour = now.timestamp() - 60;
    let date = ckbadger_common::block_date(now);
    let date_str = date.format("%Y%m%d").to_string();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0x10; 32],
            timestamp: now_ms,
            epoch_number: 1,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.commit().unwrap();

    core_store
        .put_hourly_stats(
            &this_hour.to_string(),
            &HourlyStats {
                hour: this_hour,
                blocks_count: 1,
                transactions_count: 77,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &date_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 456,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                avg_block_time_ms: None,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/tx-stats")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["currentHour"], 77);
    assert!(!json["dailyData"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_epoch_time_charts_read_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let start = chrono::Utc::now() - chrono::Duration::hours(4);
    let end = chrono::Utc::now();

    core_store.put_epoch_time_dist(240, 3).unwrap();
    core_store
        .put_epoch_stats(
            12,
            &EpochStats {
                epoch_number: 12,
                start_block: 1,
                end_block: Some(100),
                blocks_count: 100,
                length: 1800,
                start_timestamp: start,
                end_timestamp: Some(end),
                transactions_count: 200,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let dist_request = Request::builder()
        .uri("/api/v1/charts/epoch-time-distribution")
        .body(Body::empty())
        .unwrap();
    let dist_response = app.clone().oneshot(dist_request).await.unwrap();
    assert_eq!(dist_response.status(), StatusCode::OK);
    let dist_body = dist_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let dist_json: serde_json::Value = serde_json::from_slice(&dist_body).unwrap();
    let dist_data = dist_json["data"].as_array().unwrap();
    assert!(dist_data
        .iter()
        .any(|point| point["date"] == "4.00" && point["value"] == "3"));

    let length_request = Request::builder()
        .uri("/api/v1/charts/epoch-time-length")
        .body(Body::empty())
        .unwrap();
    let length_response = app.clone().oneshot(length_request).await.unwrap();
    assert_eq!(length_response.status(), StatusCode::OK);
    let length_body = length_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let length_json: serde_json::Value = serde_json::from_slice(&length_body).unwrap();
    let length_data = length_json["data"].as_array().unwrap();
    assert_eq!(length_data.len(), 1);
    assert_eq!(length_data[0]["value2"], "100");
}

#[tokio::test]
async fn test_network_stats_reads_derived_statistics() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let today = ckbadger_common::block_date(now);
    let yesterday = today - chrono::Duration::days(1);
    let today_str = today.format("%Y%m%d").to_string();
    let yesterday_str = yesterday.format("%Y%m%d").to_string();

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            timestamp: now_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.commit().unwrap();

    core_store
        .put_epoch_stats(
            42,
            &EpochStats {
                epoch_number: 42,
                start_block: 1,
                end_block: None,
                blocks_count: 11,
                length: 1800,
                start_timestamp: now - chrono::Duration::seconds(110),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &today_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 120,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                avg_block_time_ms: None,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &yesterday_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 80,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                avg_block_time_ms: None,
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            &today_str,
            &DailyBlockStats {
                avg_compact_target: 1_000_000.0,
                block_count: 100,
                total_uncles: 5,
                avg_block_time_ms: Some(10_000),
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["transactionsPerDay"], "200");
}

#[tokio::test]
async fn test_network_stats_includes_hero_metrics_from_dao_snapshot() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let now = chrono::Utc::now();
    let now_ms = now.timestamp_millis();
    let today = ckbadger_common::block_date(now);
    let yesterday = today - chrono::Duration::days(1);
    let today_str = today.format("%Y%m%d").to_string();
    let yesterday_str = yesterday.format("%Y%m%d").to_string();

    // Minimal block header so fetch_network_stats_from_db succeeds
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            timestamp: now_ms,
            epoch_number: 42,
            epoch_index: 10,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.commit().unwrap();

    core_store
        .put_epoch_stats(
            42,
            &EpochStats {
                epoch_number: 42,
                start_block: 1,
                end_block: None,
                blocks_count: 11,
                length: 1800,
                start_timestamp: now - chrono::Duration::seconds(110),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &today_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 10,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                avg_block_time_ms: None,
            },
        )
        .unwrap();
    core_store
        .put_daily_stats(
            &yesterday_str,
            &DailyStats {
                blocks_count: 1,
                transactions_count: 5,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
                used_capacity_created: 0,
                used_capacity_consumed: 0,
                total_live_cells: 0,
                total_dead_cells: 0,
                total_all_cells: 0,
                total_data_size: 0,
                knowledge_size: None,
                avg_block_time_ms: None,
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            &today_str,
            &DailyBlockStats {
                avg_compact_target: 1_000_000.0,
                block_count: 100,
                total_uncles: 5,
                avg_block_time_ms: Some(10_000),
            },
        )
        .unwrap();

    // Write a DAO daily snapshot with known hero metric values
    let snapshot = DaoDailySnapshot {
        date: "2026-03-10".to_string(),
        total_deposited: 50_000_000_000_000_000,
        depositors_count: 100,
        new_deposits: 5,
        withdrawals: 2,
        compensation: 100_000_000_000_000,
        cumulative_deposit_amount: 60_000_000_000_000_000,
        total_issuance: 3_500_000_000_000_000_000,
        secondary_pool: 10_000_000_000_000_000,
        occupied_capacity: 1_000_000_000_000_000_000,
        cum_miner_secondary: 5_000_000_000_000_000,
        cum_dao_compensation: 3_000_000_000_000_000,
        cum_treasury: 2_000_000_000_000_000,
        unclaimed_compensation: 0,
    };
    let snapshot_key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20260310",
    );
    let snapshot_value = bincode::serialize(&snapshot).unwrap();
    core_store
        .put_cf(core_store.cf_stats_dao(), &snapshot_key, &snapshot_value)
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/network")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // knowledge_size = occupied_capacity
    assert_eq!(json["knowledgeSize"], "1000000000000000000");
    // circulating_supply = total_issuance - (GENESIS_BURNT + cum_treasury) - total_deposited
    let expected_circulating: i128 = 3_500_000_000_000_000_000
        - (840_000_000_000_000_000 + 2_000_000_000_000_000)
        - 50_000_000_000_000_000;
    assert_eq!(json["circulatingSupply"], expected_circulating.to_string());
    // dao_locked = total_deposited
    assert_eq!(json["daoLocked"], "50000000000000000");
}

#[tokio::test]
async fn test_daily_block_charts_read_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    core_store
        .put_daily_block_stats(
            "20260101",
            &DailyBlockStats {
                avg_compact_target: 1_000_000.0,
                block_count: 100,
                total_uncles: 2,
                avg_block_time_ms: Some(10_000),
            },
        )
        .unwrap();
    core_store
        .put_daily_block_stats(
            "20260102",
            &DailyBlockStats {
                avg_compact_target: 2_000_000.0,
                block_count: 120,
                total_uncles: 3,
                avg_block_time_ms: Some(10_000),
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    for path in [
        "/api/v1/charts/hash-rate",
        "/api/v1/charts/difficulty",
        "/api/v1/charts/uncle-rate",
    ] {
        let request = Request::builder().uri(path).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!json["data"].as_array().unwrap().is_empty(), "path={path}");
    }
}

#[tokio::test]
async fn test_miner_distribution_reads_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let miner_hash = vec![0x66; 32];
    core_store
        .put_miner_stats(
            "20260101",
            &miner_hash,
            &MinerStats {
                miner_lock_hash: miner_hash.clone(),
                blocks_count: 10,
                last_block_number: 99,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/miner-address-distribution")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalBlocks"], 10);
    assert_eq!(json["data"][0]["blocksMined"], 10);
}

#[tokio::test]
async fn test_blocks_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_block_includes_hardfork_activation() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .put_epoch_stats(
            5414,
            &EpochStats {
                epoch_number: 5414,
                start_block: 8_775_638,
                end_block: None,
                blocks_count: 1800,
                length: 1800,
                start_timestamp: chrono::Utc::now(),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_block_header(
        8_775_638,
        &CachedBlockHeader {
            hash: vec![0x11; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 5414,
            epoch_index: 7,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/blocks/8775638")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["number"], 8_775_638);
    assert_eq!(json["hardforkActivation"]["id"], "mirana-2021");
    assert_eq!(json["hardforkActivation"]["shortName"], "Mirana");
    assert_eq!(json["hardforkActivation"]["activationEpoch"], 5414);
    assert_eq!(
        json["hardforkActivation"]["resources"][0]["label"],
        "CKB2021"
    );
    assert_eq!(
        json["hardforkActivation"]["resources"][0]["url"],
        "https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0037-ckb2021/0037-ckb2021.md"
    );
}

#[tokio::test]
async fn test_blocks_list_includes_hardfork_activation() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .put_epoch_stats(
            5414,
            &EpochStats {
                epoch_number: 5414,
                start_block: 8_775_638,
                end_block: None,
                blocks_count: 1800,
                length: 1800,
                start_timestamp: chrono::Utc::now(),
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_block_header(
        8_775_639,
        &CachedBlockHeader {
            hash: vec![0x22; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 5414,
            epoch_index: 8,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 2,
        },
    );
    batch.put_block_header(
        8_775_638,
        &CachedBlockHeader {
            hash: vec![0x11; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 5414,
            epoch_index: 7,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/blocks?limit=2")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().expect("block rows");
    assert_eq!(rows.len(), 2);

    let activation_row = rows
        .iter()
        .find(|row| row["number"].as_i64() == Some(8_775_638))
        .expect("activation block row");
    assert_eq!(activation_row["hardforkActivation"]["id"], "mirana-2021");
    assert_eq!(
        activation_row["hardforkActivation"]["shortName"],
        serde_json::Value::from("Mirana")
    );
    assert_eq!(
        activation_row["hardforkActivation"]["resources"][0]["label"],
        serde_json::Value::from("CKB2021")
    );

    let normal_row = rows
        .iter()
        .find(|row| row["number"].as_i64() == Some(8_775_639))
        .expect("non-activation block row");
    assert_eq!(normal_row["hardforkActivation"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_get_cell_returns_occupied_capacity_breakdown() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];
    let output_index = 1i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: Some(vec![0x44; 32]),
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0xaa, 0xbb]),
            data_size: 42,
            occupied_capacity: 138_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/0x{}/{}",
            hex::encode(&tx_hash),
            output_index
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json["commonKnowledgeSize"],
        serde_json::Value::from(138_00000000i64)
    );
    assert_eq!(json["type"]["args"], serde_json::Value::from("0xaabb"));
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["capacityFieldBytes"],
        serde_json::Value::from(8)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["lockScriptBytes"],
        serde_json::Value::from(53)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["typeScriptBytes"],
        serde_json::Value::from(35)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["dataBytes"],
        serde_json::Value::from(42)
    );
    assert_eq!(
        json["commonKnowledgeSizeBreakdown"]["totalBytes"],
        serde_json::Value::from(138)
    );
}

#[tokio::test]
async fn test_dead_cell_exposes_consumer_metadata_in_cell_and_graph() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];
    let output_index = 0i16;
    let consumed_by_tx = vec![0xcd; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_consumed_cell_with_consumer(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
        456,
        Some(&consumed_by_tx),
    );
    // graph route requires creating tx location to exist
    batch.put_tx_hash_map(&tx_hash, 123, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
    let consumed_by_tx_hex = format!("0x{}", hex::encode(&consumed_by_tx));

    let cell_request = Request::builder()
        .uri(format!("/api/v1/cells/{}/{}", tx_hash_hex, output_index))
        .body(Body::empty())
        .unwrap();
    let cell_response = app.clone().oneshot(cell_request).await.unwrap();
    assert_eq!(cell_response.status(), StatusCode::OK);
    let cell_body = cell_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let cell_json: serde_json::Value = serde_json::from_slice(&cell_body).unwrap();
    assert_eq!(cell_json["status"], "dead");
    assert_eq!(cell_json["consumedAtBlock"], serde_json::Value::from(456));
    assert_eq!(
        cell_json["consumedByTx"],
        serde_json::Value::from(consumed_by_tx_hex.clone())
    );

    let graph_request = Request::builder()
        .uri(format!(
            "/api/v1/graph/cell/{}/{}?depth=1",
            tx_hash_hex, output_index
        ))
        .body(Body::empty())
        .unwrap();
    let graph_response = app.oneshot(graph_request).await.unwrap();
    assert_eq!(graph_response.status(), StatusCode::OK);
    let graph_body = graph_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let graph_json: serde_json::Value = serde_json::from_slice(&graph_body).unwrap();

    let links = graph_json["links"].as_array().unwrap();
    assert!(links.iter().any(|link| {
        link["linkType"] == "consumed_by" && link["source"] == format!("cell-{}-{}", tx_hash_hex, 0)
    }));

    let nodes = graph_json["nodes"].as_array().unwrap();
    assert!(nodes
        .iter()
        .any(|node| node["data"]["hash"] == consumed_by_tx_hex));
}

#[tokio::test]
async fn test_search_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/search?q=0x1234")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_search_hash_without_0x_returns_ambiguous_block_and_transaction() {
    let store = test_store();
    let hash = vec![0xaa; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        123,
        &CachedBlockHeader {
            hash: hash.clone(),
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_tx_hash_map(&hash, 123, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/search?q={}", hex::encode(&hash)))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["normalizedQuery"],
        serde_json::Value::from(format!("0x{}", hex::encode(&hash)))
    );
    assert_eq!(json["ambiguous"], serde_json::Value::from(true));
    let result_types: Vec<_> = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["resultType"].as_str())
        .collect();
    assert!(result_types.contains(&"block"));
    assert!(result_types.contains(&"transaction"));
}

#[tokio::test]
async fn test_search_pending_transaction_hash_returns_transaction_result() {
    let store = test_store();
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/search?q={hash}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["resultType"], "transaction");
    assert_eq!(results[0]["id"], hash);
    assert_eq!(results[0]["url"], format!("/tx/{hash}"));
    assert_eq!(results[0]["label"], "Pending Transaction");
    assert_eq!(results[0]["matchKind"], "exact_hash");
    assert_eq!(json["ambiguous"], false);
}

#[tokio::test]
async fn test_transaction_detail_returns_pending_mempool_transaction() {
    let store = test_store();
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/detail"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["hash"], hash);
    assert_eq!(json["status"], "pending");
    assert!(json["pendingSince"].as_str().is_some());
    assert_eq!(json["blockNumber"], serde_json::Value::Null);
    assert_eq!(json["blockHash"], serde_json::Value::Null);
    assert_eq!(json["index"], serde_json::Value::Null);
    assert_eq!(json["confirmations"], serde_json::Value::Null);
    assert_eq!(json["timestamp"], serde_json::Value::Null);
    assert_eq!(json["inputsCount"], 1);
    assert_eq!(json["outputsCount"], 1);
    assert_eq!(json["fee"], "372");
    assert_eq!(json["inputsCommonKnowledgeSize"], serde_json::Value::Null);
    assert_eq!(
        json["outputsCommonKnowledgeSize"],
        serde_json::Value::from("61")
    );
    assert!(json["txSize"].as_i64().unwrap() > 0);
    assert_eq!(json["cycles"], 21000);
    assert_eq!(json["witnessesAvailable"], true);
    assert_eq!(
        json["witnesses"][0],
        "0x5500000010000000550000004100000000000000"
    );
    assert_eq!(
        json["inputs"][0]["previousOutput"]["txHash"],
        pending_previous_output_hash_hex()
    );
    assert_eq!(json["outputs"][0]["capacity"], "100000000000");
}

#[tokio::test]
async fn test_transaction_detail_prefers_committed_store_over_mempool() {
    let store = test_store();
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap()).unwrap();
    insert_committed_transaction(&store, &hash_bytes);
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/detail"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["hash"], hash);
    assert_eq!(json["status"], "committed");
    assert_eq!(json["blockNumber"], 321);
    assert_eq!(json["fee"], "1234");
    assert_eq!(json["txSize"], 222);
    assert_eq!(json["cycles"], 333);
}

#[tokio::test]
async fn test_pending_transaction_committed_only_routes_return_explicit_error() {
    let store = test_store();
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let cell_deps_request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/cell-deps"))
        .body(Body::empty())
        .unwrap();
    let cell_deps_response = app.clone().oneshot(cell_deps_request).await.unwrap();
    assert_eq!(cell_deps_response.status(), StatusCode::BAD_REQUEST);
    let cell_deps_body = cell_deps_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let cell_deps_json: serde_json::Value = serde_json::from_slice(&cell_deps_body).unwrap();
    assert!(cell_deps_json["message"]
        .as_str()
        .unwrap()
        .contains("pending"));

    let lifecycle_request = Request::builder()
        .uri(format!("/api/v1/transactions/{hash}/lifecycle"))
        .body(Body::empty())
        .unwrap();
    let lifecycle_response = app.clone().oneshot(lifecycle_request).await.unwrap();
    assert_eq!(lifecycle_response.status(), StatusCode::BAD_REQUEST);
    let lifecycle_body = lifecycle_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let lifecycle_json: serde_json::Value = serde_json::from_slice(&lifecycle_body).unwrap();
    assert!(lifecycle_json["message"]
        .as_str()
        .unwrap()
        .contains("pending"));

    let graph_request = Request::builder()
        .uri(format!("/api/v1/graph/transaction/{hash}"))
        .body(Body::empty())
        .unwrap();
    let graph_response = app.oneshot(graph_request).await.unwrap();
    assert_eq!(graph_response.status(), StatusCode::BAD_REQUEST);
    let graph_body = graph_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let graph_json: serde_json::Value = serde_json::from_slice(&graph_body).unwrap();
    assert!(graph_json["message"].as_str().unwrap().contains("pending"));
}

#[tokio::test]
async fn test_search_name_matches_script_token_and_cluster_assets() {
    let store = test_store();

    let script_hash = vec![0x31; 32];
    let token_hash = vec![0x32; 32];
    let cluster_id = vec![0x33; 32];

    store
        .put_script_version(
            &script_hash,
            &ScriptVersionInfo {
                version_hash: script_hash.clone(),
                name: Some("Alpha Lock".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .put_token_direct(
            &token_hash,
            &TokenInfo {
                type_code_hash: vec![0x44; 32],
                hash_type: 1,
                type_args: vec![0x55; 32],
                standard: "xudt".to_string(),
                name: Some("Alpha Token".to_string()),
                symbol: Some("ALPHA".to_string()),
                decimals: Some(8),
                total_supply: Some(1_000_000),
                max_supply: None,
                holders_count: 10,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: Some("Alpha Cluster".to_string()),
            description: None,
            total_count: 10,
            live_count: 8,
            owner_count: 2,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/search?q=alpha")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["results"].as_array().unwrap();
    let expected_script_url = "/scripts/Alpha%20Lock".to_string();
    let expected_token_url = format!("/tokens/0x{}", hex::encode(&token_hash));
    let expected_cluster_url = format!("/clusters/0x{}", hex::encode(&cluster_id));

    assert!(rows.iter().any(|row| {
        row["resultType"].as_str() == Some("script")
            && row["url"].as_str() == Some(expected_script_url.as_str())
    }));
    assert!(rows.iter().any(|row| {
        row["resultType"].as_str() == Some("token")
            && row["url"].as_str() == Some(expected_token_url.as_str())
    }));
    assert!(rows.iter().any(|row| {
        row["resultType"].as_str() == Some("cluster")
            && row["url"].as_str() == Some(expected_cluster_url.as_str())
    }));
}

#[tokio::test]
async fn test_search_cell_prefix_supports_colon_and_hex_output_index() {
    let store = test_store();
    let tx_hash = vec![0xab; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/search?q=cell:{}:0x1",
            hex::encode(&tx_hash)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["normalizedQuery"],
        serde_json::Value::from(format!("0x{}-1", hex::encode(&tx_hash)))
    );
    assert_eq!(json["results"][0]["resultType"], "cell");
    assert_eq!(
        json["results"][0]["id"],
        serde_json::Value::from(format!("0x{}-1", hex::encode(&tx_hash)))
    );
}

#[tokio::test]
async fn test_dao_stats_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing sync tip block while computing DAO statistics"));
}

#[tokio::test]
async fn test_dao_stats_uses_precomputed_latest_stats_when_tip_matches() {
    let store = test_store();

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xAA; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let latest = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 10,
        total_deposited: 123_00000000,
        total_depositors: 7,
        active_deposits: 9,
        total_compensation_paid: 11_00000000,
        unclaimed_compensation: 13_00000000,
        average_deposit_days: "950 days".to_string(),
        estimated_apc: "2.74".to_string(),
        mining_reward: 17_00000000,
        deposit_compensation: 19_00000000,
        burnt: 23_00000000,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let value = bincode::serialize(&latest).unwrap();
    store.put_stats_key(&key, &value).unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalDeposited"], "12300000000");
    assert_eq!(json["totalDepositors"], 7);
    assert_eq!(json["activeDeposits"], 9);
    assert_eq!(json["estimatedApc"], "2.74");
}

#[tokio::test]
async fn test_dao_stats_ignores_stale_precomputed_latest_stats() {
    let store = test_store();

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xBB; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let stale = ckbadger_store::DaoLatestStatistics {
        tip_block_number: 9,
        total_deposited: 999_00000000,
        total_depositors: 999,
        active_deposits: 999,
        total_compensation_paid: 0,
        unclaimed_compensation: 0,
        average_deposit_days: "999 days".to_string(),
        estimated_apc: "9.99".to_string(),
        mining_reward: 0,
        deposit_compensation: 0,
        burnt: 0,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_LATEST_STATS,
        b"latest",
    );
    let value = bincode::serialize(&stale).unwrap();
    store.put_stats_key(&key, &value).unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // No DAO deposits in DB, so fallback computation should be zero rather than stale 999.
    assert_eq!(json["totalDeposited"], "0");
    assert_eq!(json["totalDepositors"], 0);
}

#[tokio::test]
async fn test_dao_stats_cached_response_is_stable_within_ttl() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let mut dao = vec![0u8; 32];
    dao[8..16].copy_from_slice(&1u64.to_le_bytes());
    batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: vec![0xAA; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
        },
    );
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x11; 32], 0),
        &DaoDepositCacheEntry {
            capacity: 200_00000000,
            deposit_block_number: 10,
            lock_script_hash: vec![0x01; 32],
            deposit_ar: 1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store.clone())).await;
    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(first_json["totalDeposited"], "20000000000");

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x22; 32], 0),
        &DaoDepositCacheEntry {
            capacity: 300_00000000,
            deposit_block_number: 10,
            lock_script_hash: vec![0x02; 32],
            deposit_ar: 1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        },
    );
    batch.commit().unwrap();

    let request = Request::builder()
        .uri("/api/v1/dao/statistics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Expect cached response within TTL; without cache this would become 50000000000.
    assert_eq!(second_json["totalDeposited"], "20000000000");
}

#[tokio::test]
async fn test_tokens_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/tokens")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_tokens_list_includes_maximum_supply_status() {
    let store = test_store();

    let mut xudt_plain_args = vec![0x11; 32];
    xudt_plain_args.extend_from_slice(&0u32.to_le_bytes());
    let mut xudt_ext_args = vec![0x22; 32];
    xudt_ext_args.extend_from_slice(&1u32.to_le_bytes());

    let fixtures = vec![
        (
            vec![0x01; 32],
            TokenInfo {
                type_code_hash: vec![0xA1; 32],
                hash_type: 1,
                type_args: xudt_plain_args.clone(),
                standard: "xudt".to_string(),
                name: Some("Limited XUDT".to_string()),
                symbol: Some("CAP".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: Some(1000),
                holders_count: 50,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
        (
            vec![0x02; 32],
            TokenInfo {
                type_code_hash: vec![0xA2; 32],
                hash_type: 1,
                type_args: xudt_plain_args,
                standard: "xudt".to_string(),
                name: Some("Plain XUDT".to_string()),
                symbol: Some("PX".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 40,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
        (
            vec![0x03; 32],
            TokenInfo {
                type_code_hash: vec![0xA3; 32],
                hash_type: 1,
                type_args: xudt_ext_args,
                standard: "xudt".to_string(),
                name: Some("Extended XUDT".to_string()),
                symbol: Some("EX".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 30,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
        (
            vec![0x04; 32],
            TokenInfo {
                type_code_hash: vec![0xA4; 32],
                hash_type: 1,
                type_args: vec![0x44; 20],
                standard: "sudt".to_string(),
                name: Some("Plain SUDT".to_string()),
                symbol: Some("SD".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 20,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        ),
    ];

    for (type_hash, info) in fixtures {
        store.put_token_direct(&type_hash, &info).unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/tokens?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();

    let cap = rows
        .iter()
        .find(|row| row["symbol"] == "CAP")
        .expect("CAP token should exist");
    assert_eq!(cap["maximumSupply"], "1000");
    assert_eq!(cap["maximumSupplyStatus"], "limited");

    let px = rows
        .iter()
        .find(|row| row["symbol"] == "PX")
        .expect("PX token should exist");
    assert_eq!(px["maximumSupply"], serde_json::Value::Null);
    assert_eq!(px["maximumSupplyStatus"], "unlimited");

    let ex = rows
        .iter()
        .find(|row| row["symbol"] == "EX")
        .expect("EX token should exist");
    assert_eq!(ex["maximumSupply"], serde_json::Value::Null);
    assert_eq!(ex["maximumSupplyStatus"], "unknown");

    let sd = rows
        .iter()
        .find(|row| row["symbol"] == "SD")
        .expect("SD token should exist");
    assert_eq!(sd["maximumSupply"], serde_json::Value::Null);
    assert_eq!(sd["maximumSupplyStatus"], "unlimited");
}

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
                avg_block_time_ms: Some(12_000),
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
    assert_eq!(second_data[0]["date"], "20240115");
    assert_eq!(second_data[0]["value"], "12.00");
}

#[tokio::test]
async fn test_total_deposit_chart_recomputes_after_initial_empty_response() {
    let store = test_store();
    let config = test_config(store.clone());
    let app = create_router(config).await;

    let first_request = Request::builder()
        .uri("/api/v1/dao/charts/total-deposit")
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

    let snapshot = DaoDailySnapshot {
        date: "2024-01-15".to_string(),
        total_deposited: 123_00000000,
        depositors_count: 7,
        new_deposits: 0,
        withdrawals: 0,
        compensation: 0,
        cumulative_deposit_amount: 123_00000000,
        total_issuance: 0,
        secondary_pool: 0,
        occupied_capacity: 0,
        cum_miner_secondary: 0,
        cum_dao_compensation: 0,
        cum_treasury: 0,
        unclaimed_compensation: 0,
    };
    let key = ckbadger_store::keys::encode_stats_key(
        ckbadger_store::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        b"20240115",
    );
    let value = bincode::serialize(&snapshot).unwrap();
    store.put_cf(store.cf_stats_dao(), &key, &value).unwrap();

    let second_request = Request::builder()
        .uri("/api/v1/dao/charts/total-deposit")
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
    assert_eq!(second_data[0]["date"], "2024-01-15");
    assert_eq!(second_data[0]["value"], "123");
    assert_eq!(second_data[0]["value2"], "7");
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

    store
        .put_script_info_direct(
            &code_hash_a1,
            &ScriptInfo {
                code_hash: code_hash_a1.clone(),
                name: Some("Script A".to_string()),
                lock_cells_count: 10,
                lock_live_cells_count: 8,
                lock_owned_capacity_sum: 500,
                lock_owned_knowledge_sum: 300,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_a2,
            &ScriptInfo {
                code_hash: code_hash_a2.clone(),
                name: Some("Script A".to_string()),
                type_cells_count: 6,
                type_live_cells_count: 5,
                type_owned_capacity_sum: 700,
                type_owned_knowledge_sum: 500,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_b,
            &ScriptInfo {
                code_hash: code_hash_b.clone(),
                name: Some("Script B".to_string()),
                lock_cells_count: 9,
                lock_live_cells_count: 7,
                lock_owned_capacity_sum: 800,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_unknown,
            &ScriptInfo {
                code_hash: code_hash_unknown.clone(),
                name: None,
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
    let used_share = &json["usedShare"];
    let used_series = used_share["series"].as_array().unwrap();
    assert_eq!(used_series.len(), 4);
    assert_eq!(used_series[0]["label"], "Script A");
    assert_eq!(
        used_series[1]["label"],
        format!("0x{}", hex::encode(&code_hash_unknown))
    );
    assert_eq!(used_series[2]["label"], "Script B");
    assert_eq!(used_series[3]["label"], "Others");

    let used_data = used_share["data"].as_array().unwrap();
    assert_eq!(used_data.len(), 1);
    assert_eq!(used_data[0]["date"], "2024-01-01");
    assert_eq!(used_data[0]["values"]["top0"], "800");
    assert_eq!(used_data[0]["values"]["top1"], "550");
    assert_eq!(used_data[0]["values"]["top2"], "200");
    assert_eq!(used_data[0]["values"]["others"], "0");

    let capacity_share = &json["capacityShare"];
    let capacity_series = capacity_share["series"].as_array().unwrap();
    assert_eq!(capacity_series[0]["label"], "Script A");
    assert_eq!(capacity_series[1]["label"], "Script B");
    assert_eq!(
        capacity_series[2]["label"],
        format!("0x{}", hex::encode(&code_hash_unknown))
    );
    assert_eq!(capacity_series[3]["label"], "Others");

    let capacity_data = capacity_share["data"].as_array().unwrap();
    assert_eq!(capacity_data[0]["values"]["top0"], "1200");
    assert_eq!(capacity_data[0]["values"]["top1"], "800");
    assert_eq!(capacity_data[0]["values"]["top2"], "600");
    assert_eq!(capacity_data[0]["values"]["others"], "0");
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
                total_supply: Some(1000),
                max_supply: None,
                holders_count: 10,
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
                total_supply: Some(1000),
                max_supply: None,
                holders_count: 11,
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
    batch.put_object_collection_aggregate(
        &nft_collection_id,
        &ObjectCollectionAggregate {
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
        .put_object_daily_delta(
            &nft_collection_id,
            20240101,
            &ObjectDailyDelta {
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
    assert_eq!(used_series[0]["label"], "NFT Collection (nft)");
    assert_eq!(used_series[1]["label"], "DOB Cluster (nft)");
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
    assert_eq!(capacity_series[1]["label"], "NFT Collection (nft)");
    assert_eq!(capacity_series[2]["label"], "DOB Cluster (nft)");
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

    let mut batch = StoreBatch::new(store.as_ref());
    for (number, ts_ms) in [(0i64, 0i64), (1, 1_000), (2, 3_000)] {
        batch.put_block_header(
            number,
            &CachedBlockHeader {
                hash: vec![number as u8; 32],
                timestamp: ts_ms,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
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

    let point_1s = data.iter().find(|point| point["date"] == "1.0").unwrap();
    let point_2s = data.iter().find(|point| point["date"] == "2.0").unwrap();
    assert_eq!(point_1s["value"], "50.000");
    assert_eq!(point_2s["value"], "50.000");
}

#[tokio::test]
async fn test_block_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/blocks/999999999")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_transaction_not_found() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let hash = "0x".to_string() + &"ab".repeat(32);
    let request = Request::builder()
        .uri(format!("/api/v1/transactions/{}", hash))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_scripts_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_scripts_list_returns_warmup_pending_when_script_cache_missing() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router_without_warmup(config);

    let request = Request::builder()
        .uri("/api/v1/scripts")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "warmup_pending");
    assert_eq!(
        json["message"],
        "script cache unavailable; warmup in progress"
    );
}

#[tokio::test]
async fn test_scripts_list_returns_default_lock_family_for_data1_reference() {
    let store = test_store();

    let family_id = "default-lock";
    let version_hash =
        hex::decode("709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649").unwrap();
    let canonical_type_reference =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let observed_data1_reference = version_hash.clone();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            description: Some("Default lock family".to_string()),
            versions_count: 1,
            live_cells_count: 10,
            cells_count: 14,
            owned_capacity_sum: 1_500,
            owned_knowledge_sum: 900,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            description: Some("Default lock family".to_string()),
            canonical_reference_hash: Some(canonical_type_reference.clone()),
            canonical_hash_type: Some(1),
            lock_live_cells_count: 10,
            lock_cells_count: 14,
            lock_owned_capacity_sum: 1_500,
            lock_owned_knowledge_sum: 900,
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &canonical_type_reference,
        &ScriptReferenceInfo {
            reference_hash: canonical_type_reference.clone(),
            hash_type: 1,
            lock_live_cells_count: 4,
            lock_cells_count: 6,
            lock_owned_capacity_sum: 700,
            lock_owned_knowledge_sum: 400,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(1, &canonical_type_reference, &version_hash);
    batch.put_script_reference_info(
        2,
        &observed_data1_reference,
        &ScriptReferenceInfo {
            reference_hash: observed_data1_reference.clone(),
            hash_type: 2,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.put_script_reference_to_version(2, &observed_data1_reference, &version_hash);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=20")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();

    assert_eq!(json["total"], 1);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["familyId"], "default-lock");
    assert_eq!(data[0]["name"], "Default Lock");
    assert_eq!(data[0]["liveCellsCount"], 10);
    assert_eq!(data[0]["cellsCount"], 14);
    assert_eq!(data[0]["ownedCapacitySum"], "1500");
    assert_eq!(data[0]["ownedKnowledgeSum"], "900");
    assert_eq!(data[0]["versionsCount"], 1);
}

#[tokio::test]
async fn test_scripts_list_supports_cursor_pagination() {
    let store = test_store();

    let mut batch = StoreBatch::new(store.as_ref());
    for (family_id, name) in [
        ("a-script", "A_SCRIPT"),
        ("b-script", "B_SCRIPT"),
        ("c-script", "C_SCRIPT"),
    ] {
        batch.put_script_family(
            family_id,
            &ScriptFamilyInfo {
                family_id: family_id.to_string(),
                name: name.to_string(),
                versions_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_family_by_name(name, family_id);
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page1 = json["data"].as_array().unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0]["name"], "A_SCRIPT");
    assert_eq!(page1[1]["name"], "B_SCRIPT");
    assert_eq!(page1[0]["ownedCapacitySum"], "0");
    assert_eq!(page1[0]["ownedKnowledgeSum"], "0");
    assert_eq!(json["total"], 3);
    assert_eq!(json["limit"], 2);
    assert_eq!(json["hasMore"], true);
    assert_eq!(json["nextCursor"], "2");

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&cursor=2")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page2 = json["data"].as_array().unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0]["name"], "C_SCRIPT");
    assert_eq!(json["total"], 3);
    assert_eq!(json["limit"], 2);
    assert_eq!(json["hasMore"], false);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_scripts_list_sorts_before_cursor_pagination() {
    let store = test_store();

    let mut batch = StoreBatch::new(store.as_ref());
    for (family_id, name, owned_capacity_sum) in [
        ("a-script", "A_SCRIPT", 10i128),
        ("b-script", "B_SCRIPT", 30i128),
        ("c-script", "C_SCRIPT", 20i128),
    ] {
        batch.put_script_family(
            family_id,
            &ScriptFamilyInfo {
                family_id: family_id.to_string(),
                name: name.to_string(),
                owned_capacity_sum,
                versions_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_family_by_name(name, family_id);
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&sort_key=capacity&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page1 = json["data"].as_array().unwrap();
    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0]["name"], "B_SCRIPT");
    assert_eq!(page1[1]["name"], "C_SCRIPT");
    assert_eq!(page1[0]["ownedCapacitySum"], "30");
    assert_eq!(page1[1]["ownedCapacitySum"], "20");
    assert_eq!(json["nextCursor"], "2");
    assert_eq!(json["hasMore"], true);

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=2&cursor=2&sort_key=capacity&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let page2 = json["data"].as_array().unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0]["name"], "A_SCRIPT");
    assert_eq!(page2[0]["ownedCapacitySum"], "10");
    assert_eq!(json["hasMore"], false);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_scripts_list_ignores_unlabeled_references_without_family_metadata() {
    let store = test_store();

    for code_byte in [0x11u8, 0x22u8] {
        let code_hash = vec![code_byte; 32];
        store
            .put_script_info_direct(
                &code_hash,
                &ScriptInfo {
                    code_hash: code_hash.clone(),
                    hash_type: 1,
                    name: None,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["total"], 0);
    assert_eq!(data.len(), 0);
}

#[tokio::test]
async fn test_script_lookup_and_code_cell_resolve_data_reference() {
    let store = test_store();

    let version_hash = vec![0x70; 32];
    let code_cell_tx_hash = vec![0xe2; 32];
    let code_cell_output_index = 1i16;

    store
        .put_script_info_direct(
            &version_hash,
            &ScriptInfo {
                code_hash: version_hash.clone(),
                hash_type: 0,
                name: Some("Default Lock".to_string()),
                lock_cells_count: 10,
                lock_live_cells_count: 10,
                lock_capacity_sum: 1_000_000_000,
                lock_owned_capacity_sum: 1_000_000_000,
                lock_used_capacity_sum: 600_000_000,
                lock_owned_knowledge_sum: 600_000_000,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            name: Some("Default Lock".to_string()),
            category: Some("lock".to_string()),
            lock_cells_count: 10,
            lock_live_cells_count: 10,
            lock_capacity_sum: 1_000_000_000,
            lock_owned_capacity_sum: 1_000_000_000,
            lock_used_capacity_sum: 600_000_000,
            lock_owned_knowledge_sum: 600_000_000,
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_data_hash(
        &version_hash,
        123,
        &code_cell_tx_hash,
        code_cell_output_index,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            version_hash_hex
        )))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&version_hash_hex]["name"], "Default Lock");
    assert_eq!(json[&version_hash_hex]["codeHash"], version_hash_hex);
    assert_eq!(json[&version_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&version_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&version_hash_hex]["codeCellOutputIndex"], 1);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=data",
            version_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["outputIndex"], 1);
}

#[tokio::test]
async fn test_script_code_cells_resolve_unique_type_reference() {
    let store = test_store();

    let version_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let code_cell_tx_hash = vec![0xe2; 32];
    let code_cell_output_index = 1i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            name: Some("Default Lock".to_string()),
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_type(&type_hash, 123, &code_cell_tx_hash, code_cell_output_index);
    batch.put_cell_by_data_hash(
        &version_hash,
        123,
        &code_cell_tx_hash,
        code_cell_output_index,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=type",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["resolvedVersionHash"], version_hash_hex);
    assert_eq!(json["liveCount"], 1);
    assert_eq!(json["totalCount"], 1);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["codeCells"][0]["outputIndex"], 1);
    assert_eq!(json["codeCells"][0]["status"], "live");
    assert_eq!(json["codeCells"][0]["createdAtBlock"], 123);
}

#[tokio::test]
async fn test_script_lookup_and_code_cells_allow_unlabeled_resolved_type_reference() {
    let store = test_store();

    let version_hash = vec![0x51; 32];
    let type_hash = vec![0x61; 32];
    let code_cell_tx_hash = vec![0x71; 32];
    let code_cell_output_index = 2i16;

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(version_hash.clone()),
                lock_cells_count: 4,
                lock_live_cells_count: 2,
                lock_capacity_sum: 900,
                lock_owned_capacity_sum: 500,
                lock_used_capacity_sum: 700,
                lock_owned_knowledge_sum: 350,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash.clone()),
        },
        234,
    );
    batch.put_cell_by_type(&type_hash, 234, &code_cell_tx_hash, code_cell_output_index);
    batch.put_cell_by_data_hash(
        &version_hash,
        234,
        &code_cell_tx_hash,
        code_cell_output_index,
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let version_hash_hex = format!("0x{}", hex::encode(&version_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            type_hash_hex
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&type_hash_hex]["resolutionState"], "resolved");
    assert_eq!(json[&type_hash_hex]["name"], "Unknown");
    assert_eq!(json[&type_hash_hex]["codeHash"], version_hash_hex);
    assert_eq!(json[&type_hash_hex]["hashType"], "type");
    assert_eq!(json[&type_hash_hex]["deploymentTypeHash"], type_hash_hex);
    assert_eq!(json[&type_hash_hex]["deploymentDataHash"], version_hash_hex);
    assert_eq!(json[&type_hash_hex]["scriptKind"], "lock");
    assert_eq!(json[&type_hash_hex]["liveCellsCount"], 2);
    assert_eq!(json[&type_hash_hex]["ownedCapacitySum"], "500");
    assert_eq!(json[&type_hash_hex]["ownedKnowledgeSum"], "350");
    assert_eq!(
        json[&type_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&type_hash_hex]["codeCellOutputIndex"], 2);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}&hash_type=type",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["resolvedVersionHash"], version_hash_hex);
    assert_eq!(json["liveCount"], 1);
    assert_eq!(json["totalCount"], 1);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["codeCells"][0]["outputIndex"], 2);
    assert_eq!(json["codeCells"][0]["status"], "live");
}

#[tokio::test]
async fn test_scripts_list_merges_unknown_reference_into_known_deployment() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let family_id = "default-lock";

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.commit().unwrap();

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(data_hash.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                name: None,
                ..Default::default()
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();

    assert_eq!(json["total"], 1);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["familyId"], family_id);
    assert_eq!(data[0]["name"], "Default Lock");
}

#[tokio::test]
async fn test_unknown_data_hash_script_resolves_code_cell_via_data_hash_index() {
    let store = test_store();

    let code_bytes = b"unknown-script-code-cell";
    let data_hash = compute_blake2b_data_hash(code_bytes);
    let code_cell_tx_hash = vec![0xcd; 32];
    let code_cell_output_index = 2i16;

    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                lock_live_cells_count: 3,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &data_hash,
        &ScriptVersionInfo {
            version_hash: data_hash.clone(),
            lock_live_cells_count: 3,
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        code_cell_output_index,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: code_bytes.len() as i32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(data_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_data_hash(&data_hash, 123, &code_cell_tx_hash, code_cell_output_index);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let data_hash_hex = format!("0x{}", hex::encode(&data_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            data_hash_hex
        )))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[&data_hash_hex]["name"], "Unknown");
    assert_eq!(json[&data_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&data_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );
    assert_eq!(json[&data_hash_hex]["codeCellOutputIndex"], 2);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cell?code_hash={}&hash_type=data",
            data_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["txHash"], code_cell_tx_hash_hex);
    assert_eq!(json["outputIndex"], 2);
}

#[tokio::test]
async fn test_script_lookup_and_code_cells_resolve_unique_reference_without_hash_type() {
    let store = test_store();
    let reference_hash = vec![0x77; 32];
    let code_cell_tx_hash = vec![0xce; 32];

    store
        .put_script_info_direct(
            &reference_hash,
            &ScriptInfo {
                code_hash: reference_hash.clone(),
                hash_type: 0,
                lock_cells_count: 3,
                lock_live_cells_count: 1,
                lock_capacity_sum: 500,
                lock_owned_capacity_sum: 200,
                lock_used_capacity_sum: 500,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &reference_hash,
        &ScriptVersionInfo {
            version_hash: reference_hash.clone(),
            name: Some("UniqueScript".to_string()),
            category: Some("lock".to_string()),
            lock_cells_count: 3,
            lock_live_cells_count: 1,
            lock_capacity_sum: 500,
            lock_owned_capacity_sum: 200,
            lock_used_capacity_sum: 500,
            lock_owned_knowledge_sum: 200,
            ..Default::default()
        },
    );
    batch.put_cell(
        &code_cell_tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(reference_hash.clone()),
        },
        123,
    );
    batch.put_cell_by_data_hash(&reference_hash, 123, &code_cell_tx_hash, 1);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let code_cell_tx_hash_hex = format!("0x{}", hex::encode(&code_cell_tx_hash));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            reference_hash_hex
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json[&reference_hash_hex]["resolutionState"], "resolved");
    assert_eq!(json[&reference_hash_hex]["name"], "UniqueScript");
    assert_eq!(json[&reference_hash_hex]["codeHash"], reference_hash_hex);
    assert_eq!(json[&reference_hash_hex]["hashType"], "data");
    assert_eq!(
        json[&reference_hash_hex]["codeCellTxHash"],
        code_cell_tx_hash_hex
    );

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}",
            reference_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["resolvedVersionHash"], reference_hash_hex);
    assert_eq!(json["liveCount"], 1);
    assert_eq!(json["totalCount"], 1);
    assert_eq!(json["codeCells"][0]["txHash"], code_cell_tx_hash_hex);
}

#[tokio::test]
async fn test_script_lookup_and_code_cells_surface_type_reference_ambiguity() {
    let store = test_store();
    let reference_hash = vec![0x88; 32];
    let version_hash_a = vec![0xa1; 32];
    let version_hash_b = vec![0xb2; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &[0xd1; 32],
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference_hash.clone()),
            type_code_hash: Some(vec![0x33; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash_a.clone()),
        },
        100,
    );
    batch.put_cell_by_type(&reference_hash, 100, &[0xd1; 32], 0);
    batch.put_cell(
        &[0xd2; 32],
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x44; 32],
            lock_code_hash: vec![0x55; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(reference_hash.clone()),
            type_code_hash: Some(vec![0x66; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(version_hash_b.clone()),
        },
        101,
    );
    batch.put_cell_by_type(&reference_hash, 101, &[0xd2; 32], 0);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let version_hash_a_hex = format!("0x{}", hex::encode(&version_hash_a));
    let version_hash_b_hex = format!("0x{}", hex::encode(&version_hash_b));

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            reference_hash_hex
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json[&reference_hash_hex]["resolutionState"], "ambiguous");
    assert_eq!(
        json[&reference_hash_hex]["ambiguity"]["versionHashes"],
        serde_json::json!([version_hash_a_hex, version_hash_b_hex])
    );

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/code-cells?code_hash={}",
            reference_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["codeCells"], serde_json::json!([]));
    assert_eq!(
        json["ambiguity"]["versionHashes"],
        serde_json::json!([version_hash_a_hex, version_hash_b_hex])
    );
}

#[tokio::test]
async fn test_search_exact_script_hash_uses_reference_version_resolution() {
    let store = test_store();
    let reference_hash = vec![0x93; 32];

    store
        .put_script_info_direct(
            &reference_hash,
            &ScriptInfo {
                code_hash: reference_hash.clone(),
                hash_type: 0,
                lock_cells_count: 1,
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_version(
        &reference_hash,
        &ScriptVersionInfo {
            version_hash: reference_hash.clone(),
            name: Some("SearchableScript".to_string()),
            lock_cells_count: 1,
            lock_live_cells_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let reference_hash_hex = format!("0x{}", hex::encode(&reference_hash));
    let request = Request::builder()
        .uri(format!("/api/v1/search?q={}", reference_hash_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let script_result = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["resultType"] == "script")
        .expect("script result");
    assert_eq!(
        script_result["url"],
        format!("/script/{}", reference_hash_hex)
    );
    assert_eq!(script_result["label"], "Script SearchableScript");
}

#[tokio::test]
async fn test_cells_by_script_resolves_reference_hash_type_alias() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let type_hash = vec![0x9b; 32];
    let tx_hash = vec![0xab; 32];

    store
        .put_script_info_direct(
            &type_hash,
            &ScriptInfo {
                code_hash: type_hash.clone(),
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                dep_type_hash: Some(type_hash.clone()),
                dep_data_hash: Some(data_hash.clone()),
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                name: None,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: type_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
    );
    batch.put_cell_by_lock_code(&type_hash, 123, &tx_hash, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash=0x{}&hash_type=type&script_kind=lock&limit=20",
            hex::encode(&data_hash)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();

    assert_eq!(json["total"], 1);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

#[tokio::test]
async fn test_cells_by_script_type_request_returns_empty_for_data_only_deployment() {
    let store = test_store();

    let data_hash = vec![0x70; 32];
    let tx_hash = vec![0xab; 32];

    store
        .put_script_info_direct(
            &data_hash,
            &ScriptInfo {
                code_hash: data_hash.clone(),
                hash_type: 0,
                lock_live_cells_count: 1,
                ..Default::default()
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: data_hash.clone(),
            lock_hash_type: 0,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        123,
    );
    batch.put_cell_by_lock_code(&data_hash, 123, &tx_hash, 0);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let data_hash_hex = format!("0x{}", hex::encode(&data_hash));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash={}&hash_type=data&script_kind=lock&limit=20",
            data_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/cells/by-script?code_hash={}&hash_type=type&script_kind=lock&limit=20",
            data_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 0);
    assert_eq!(json["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_script_returns_versions_sorted_by_deployed_at() {
    let store = test_store();
    let name = "SECP256K1_BLAKE160".to_string();
    let family_id = "secp256k1-blake160";

    let older_type_hash = vec![0x11; 32];
    let newer_type_hash = vec![0x22; 32];
    let older_version_hash = vec![0x33; 32];
    let newer_version_hash = vec![0x44; 32];
    let older_tx_hash = vec![0xaa; 32];
    let newer_earliest_tx_hash = vec![0xab; 32];
    let newer_tx_hash = vec![0xbb; 32];

    let older_block = 100i64;
    let newer_earliest_block = 150i64;
    let newer_block = 200i64;
    let older_timestamp = 1_700_000_000_000i64;
    let newer_earliest_timestamp = 1_700_050_000_000i64;
    let newer_timestamp = 1_700_100_000_000i64;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        older_block,
        &CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: older_timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_block_header(
        newer_earliest_block,
        &CachedBlockHeader {
            hash: vec![0x03; 32],
            timestamp: newer_earliest_timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_block_header(
        newer_block,
        &CachedBlockHeader {
            hash: vec![0x04; 32],
            timestamp: newer_timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: name.clone(),
            versions_count: 2,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name(&name, family_id);
    batch.put_script_version_by_family(family_id, &older_version_hash);
    batch.put_script_version_by_family(family_id, &newer_version_hash);
    batch.put_script_version(
        &older_version_hash,
        &ScriptVersionInfo {
            version_hash: older_version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some(name.clone()),
            canonical_reference_hash: Some(older_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_version(
        &newer_version_hash,
        &ScriptVersionInfo {
            version_hash: newer_version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some(name.clone()),
            canonical_reference_hash: Some(newer_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cell(
        &older_tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x10; 32],
            lock_code_hash: vec![0x20; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(older_type_hash.clone()),
            type_code_hash: Some(vec![0x30; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(older_version_hash.clone()),
        },
        older_block,
    );
    batch.put_cell_by_type(&older_type_hash, older_block, &older_tx_hash, 0);
    batch.put_cell_by_data_hash(&older_version_hash, older_block, &older_tx_hash, 0);
    batch.put_cell(
        &newer_earliest_tx_hash,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x12; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(newer_type_hash.clone()),
            type_code_hash: Some(vec![0x32; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(newer_version_hash.clone()),
        },
        newer_earliest_block,
    );
    batch.put_cell_by_type(
        &newer_type_hash,
        newer_earliest_block,
        &newer_earliest_tx_hash,
        0,
    );
    batch.put_cell_by_data_hash(
        &newer_version_hash,
        newer_earliest_block,
        &newer_earliest_tx_hash,
        0,
    );
    batch.put_cell(
        &newer_tx_hash,
        1,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x21; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(newer_type_hash.clone()),
            type_code_hash: Some(vec![0x31; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 32,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: Some(newer_version_hash.clone()),
        },
        newer_block,
    );
    batch.put_cell_by_type(&newer_type_hash, newer_block, &newer_tx_hash, 1);
    batch.put_cell_by_data_hash(&newer_version_hash, newer_block, &newer_tx_hash, 1);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json["versions"].as_array().unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(json["familyId"], family_id);
    assert_eq!(json["name"], name);
    assert_eq!(
        items[0]["versionHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&newer_version_hash)))
    );
    assert_eq!(
        items[0]["canonicalReferenceHash"],
        format!("0x{}", hex::encode(&newer_type_hash))
    );
    assert_eq!(items[0]["canonicalHashType"], "type");
    assert_eq!(items[0]["deployedAt"], newer_earliest_timestamp);
    let newer_deployments = items[0]["deployments"].as_array().unwrap();
    assert_eq!(newer_deployments.len(), 2);
    assert_eq!(
        newer_deployments[0]["codeCellTxHash"],
        format!("0x{}", hex::encode(&newer_earliest_tx_hash))
    );
    assert_eq!(newer_deployments[0]["codeCellOutputIndex"], 0);
    assert_eq!(newer_deployments[0]["deployedAt"], newer_earliest_timestamp);
    assert_eq!(
        newer_deployments[0]["typeReferenceHash"],
        format!("0x{}", hex::encode(&newer_type_hash))
    );
    assert_eq!(
        newer_deployments[0]["dataReferenceHash"],
        format!("0x{}", hex::encode(&newer_version_hash))
    );
    assert_eq!(
        items[1]["versionHash"],
        serde_json::Value::String(format!("0x{}", hex::encode(&older_version_hash)))
    );
    assert_eq!(
        items[1]["canonicalReferenceHash"],
        format!("0x{}", hex::encode(&older_type_hash))
    );
    assert_eq!(items[1]["canonicalHashType"], "type");
    assert_eq!(items[1]["deployedAt"], older_timestamp);
}

#[tokio::test]
async fn test_get_script_includes_direct_version_hash_reference_without_mapping() {
    let store = test_store();
    let family_id = "default-lock";
    let version_hash = vec![0x70; 32];
    let canonical_type_hash = vec![0x9b; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            canonical_reference_hash: Some(canonical_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        2,
        &version_hash,
        &ScriptReferenceInfo {
            reference_hash: version_hash.clone(),
            hash_type: 2,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let references = json["versions"][0]["references"].as_array().unwrap();

    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0]["referenceHash"],
        format!("0x{}", hex::encode(&version_hash))
    );
    assert_eq!(references[0]["hashType"], "data1");
}

#[tokio::test]
async fn test_get_script_fails_when_relevant_canonical_reference_mapping_missing() {
    let store = test_store();
    let family_id = "default-lock";
    let version_hash = vec![0x70; 32];
    let canonical_type_hash = vec![0x9b; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            canonical_reference_hash: Some(canonical_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &canonical_type_hash,
        &ScriptReferenceInfo {
            reference_hash: canonical_type_hash.clone(),
            hash_type: 1,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("missing reference->version mapping"));
}

#[tokio::test]
async fn test_get_script_ignores_unrelated_unresolved_reference_info() {
    let store = test_store();
    let family_id = "default-lock";
    let version_hash = vec![0x70; 32];
    let canonical_type_hash = vec![0x9b; 32];
    let unrelated_reference = vec![0xaa; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_hash);
    batch.put_script_version(
        &version_hash,
        &ScriptVersionInfo {
            version_hash: version_hash.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            canonical_reference_hash: Some(canonical_type_hash.clone()),
            canonical_hash_type: Some(1),
            ..Default::default()
        },
    );
    batch.put_script_reference_info(
        1,
        &unrelated_reference,
        &ScriptReferenceInfo {
            reference_hash: unrelated_reference.clone(),
            hash_type: 1,
            lock_live_cells_count: 6,
            lock_cells_count: 8,
            lock_owned_capacity_sum: 800,
            lock_owned_knowledge_sum: 500,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let references = json["versions"][0]["references"].as_array().unwrap();
    assert_eq!(references.len(), 0);
}

#[tokio::test]
async fn test_get_script_usage_aggregates_family_versions() {
    let store = test_store();
    let family_id = "default-lock";
    let version_a = vec![0x11; 32];
    let version_b = vec![0x22; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "Default Lock".to_string(),
            versions_count: 2,
            ..Default::default()
        },
    );
    batch.put_script_family_by_name("Default Lock", family_id);
    batch.put_script_version_by_family(family_id, &version_a);
    batch.put_script_version_by_family(family_id, &version_b);
    batch.put_script_version(
        &version_a,
        &ScriptVersionInfo {
            version_hash: version_a.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            category: Some("lock".to_string()),
            lock_cells_count: 4,
            lock_live_cells_count: 3,
            lock_capacity_sum: 500,
            lock_owned_capacity_sum: 300,
            lock_used_capacity_sum: 260,
            lock_owned_knowledge_sum: 180,
            ..Default::default()
        },
    );
    batch.put_script_version(
        &version_b,
        &ScriptVersionInfo {
            version_hash: version_b.clone(),
            family_id: Some(family_id.to_string()),
            name: Some("Default Lock".to_string()),
            category: Some("type".to_string()),
            type_cells_count: 5,
            type_live_cells_count: 2,
            type_capacity_sum: 700,
            type_owned_capacity_sum: 400,
            type_used_capacity_sum: 500,
            type_owned_knowledge_sum: 220,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    store
        .put_script_info_direct(
            &[0xaa; 32],
            &ScriptInfo {
                code_hash: vec![0xaa; 32],
                hash_type: 1,
                name: Some("Default Lock".to_string()),
                lock_cells_count: 999,
                lock_live_cells_count: 999,
                lock_capacity_sum: 999_999,
                lock_owned_capacity_sum: 999_999,
                lock_used_capacity_sum: 999_999,
                lock_owned_knowledge_sum: 999_999,
                ..Default::default()
            },
        )
        .unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/scripts/Default%20Lock/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["name"], "Default Lock");
    assert_eq!(json["cellsCount"], 9);
    assert_eq!(json["liveCellsCount"], 5);
    assert_eq!(json["capacitySum"], "1200");
    assert_eq!(json["ownedCapacitySum"], "700");
    assert_eq!(json["commonKnowledgeSizeSum"], "760");
    assert_eq!(json["ownedKnowledgeSum"], "400");
    assert_eq!(
        json["byDeployment"][0]["codeHash"],
        format!("0x{}", hex::encode(&version_a))
    );
    assert_eq!(
        json["byDeployment"][1]["codeHash"],
        format!("0x{}", hex::encode(&version_b))
    );
}

#[tokio::test]
async fn test_get_script_usage_returns_not_found_for_unknown_family() {
    let store = test_store();
    let app = create_router(test_config(store)).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/Unknown%20Family/usage")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_script_capacity_chart_aggregates_deployments() {
    let store = test_store();

    let code_hash_a = vec![0x11; 32];
    let code_hash_b = vec![0x22; 32];
    let name = "SECP256K1_BLAKE160".to_string();

    store
        .put_script_info_direct(
            &code_hash_a,
            &ScriptInfo {
                code_hash: code_hash_a.clone(),
                name: Some(name.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .put_script_info_direct(
            &code_hash_b,
            &ScriptInfo {
                code_hash: code_hash_b.clone(),
                name: Some(name.clone()),
                ..Default::default()
            },
        )
        .unwrap();

    store
        .put_script_daily_delta(
            &code_hash_a,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_a,
            false,
            20240117,
            &ScriptDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 50,
                owned_knowledge_delta: 30,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash_b,
            false,
            20240117,
            &ScriptDailyDelta {
                owned_capacity_delta: 10,
                owned_knowledge_delta: 5,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        300,
        &CachedBlockHeader {
            hash: vec![0x03; 32],
            timestamp: 1_705_536_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160/charts/capacity-history")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["title"], "SECP256K1_BLAKE160 Capacity History");
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "90");
    assert_eq!(data[0]["values"]["unused"], "60");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["used"], "90");
    assert_eq!(data[1]["values"]["unused"], "60");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["used"], "85");
    assert_eq!(data[2]["values"]["unused"], "55");

    let request = Request::builder()
        .uri("/api/v1/scripts/SECP256K1_BLAKE160/charts/capacity-history?from=2024-01-16&to=2024-01-16")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["used"], "90");
    assert_eq!(data[0]["values"]["unused"], "60");
}

#[tokio::test]
async fn test_script_capacity_chart_by_code_hash_with_kind_filter() {
    let store = test_store();
    let code_hash = vec![0x33; 32];
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    store
        .put_script_daily_delta(
            &code_hash,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 40,
            },
        )
        .unwrap();
    store
        .put_script_daily_delta(
            &code_hash,
            true,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 80,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0x04; 32],
            timestamp: 1_705_363_200_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/charts/capacity-history?code_hash={}&script_kind=lock",
            code_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["values"]["used"], "40");
    assert_eq!(data[0]["values"]["unused"], "60");
}

#[tokio::test]
async fn test_script_capacity_chart_by_code_hash_extends_to_latest_complete_ckb_day() {
    let store = test_store();
    let code_hash = vec![0x44; 32];
    let code_hash_hex = format!("0x{}", hex::encode(&code_hash));

    store
        .put_script_daily_delta(
            &code_hash,
            false,
            20240115,
            &ScriptDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 40,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: 1_705_536_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/scripts/charts/capacity-history?code_hash={}&script_kind=lock",
            code_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "40");
    assert_eq!(data[0]["values"]["unused"], "60");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["used"], "40");
    assert_eq!(data[1]["values"]["unused"], "60");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["used"], "40");
    assert_eq!(data[2]["values"]["unused"], "60");
}

#[tokio::test]
async fn test_get_token_includes_maximum_supply() {
    let store = test_store();
    let type_hash = vec![0x77; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let holder_lock = [0x11; 32];

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Cap Token".to_string()),
                symbol: Some("CAP".to_string()),
                decimals: Some(8),
                total_supply: Some(500_00000000),
                max_supply: Some(100_000_000_000),
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &holder_lock, 500_00000000);
    batch.put_token_holder_by_balance(&type_hash, &holder_lock, 500_00000000);
    batch.put_addr_token_by_balance(&holder_lock, &type_hash, 500_00000000);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/tokens/{}", type_hash_hex))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalSupply"], "50000000000");
    assert_eq!(json["totalCommonKnowledgeSize"], serde_json::Value::Null);
    assert_eq!(json["maximumSupply"], "100000000000");
    assert_eq!(json["maximumSupplyStatus"], "limited");
}

#[tokio::test]
async fn test_get_token_returns_store_backed_detail_when_filtered_from_warmup_cache() {
    let store = test_store();
    let type_hash = vec![0x78; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));
    let holder_lock = [0x12; 32];

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: None,
                symbol: None,
                decimals: Some(8),
                total_supply: Some(500_00000000),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: Some("Store-backed token detail".to_string()),
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &holder_lock, 500_00000000);
    batch.put_token_holder_by_balance(&type_hash, &holder_lock, 500_00000000);
    batch.put_addr_token_by_balance(&holder_lock, &type_hash, 500_00000000);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/tokens/{}", type_hash_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["typeScriptHash"], type_hash_hex);
    assert!(json["name"].is_null());
    assert!(json["symbol"].is_null());
    assert_eq!(json["description"], "Store-backed token detail");
    assert_eq!(json["totalSupply"], "50000000000");
}

#[tokio::test]
async fn test_get_token_derives_stats_from_holder_and_stats_cfs_when_token_row_is_placeholder() {
    let store = test_store();
    let type_hash = vec![0x7b; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 2,
                type_args: vec![0x66; 32],
                standard: "xudt".to_string(),
                name: Some("Placeholder Label".to_string()),
                symbol: Some("PLH".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: Some("logo.png".to_string()),
                description: Some("label metadata only".to_string()),
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &[0x01; 32], 200);
    batch.put_token_holder(&type_hash, &[0x02; 32], 100);
    batch.put_token_holder_by_balance(&type_hash, &[0x01; 32], 200);
    batch.put_token_holder_by_balance(&type_hash, &[0x02; 32], 100);
    batch.put_addr_token_by_balance(&[0x01; 32], &type_hash, 200);
    batch.put_addr_token_by_balance(&[0x02; 32], &type_hash, 100);
    batch.put_token_transfers_count(&type_hash, 7);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let detail_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tokens/{type_hash_hex}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_body = detail_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let detail_json: serde_json::Value = serde_json::from_slice(&detail_body).unwrap();
    assert_eq!(detail_json["name"], "Placeholder Label");
    assert_eq!(detail_json["totalSupply"], "300");
    assert_eq!(detail_json["holdersCount"], 2);
    assert_eq!(detail_json["transfersCount"], 7);

    let holders_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tokens/{type_hash_hex}/holders?limit=10"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(holders_response.status(), StatusCode::OK);
    let holders_body = holders_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let holders_json: serde_json::Value = serde_json::from_slice(&holders_body).unwrap();
    assert_eq!(holders_json["total"], 2);
    assert_eq!(holders_json["data"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_get_token_holders_preserves_equal_balance_pagination() {
    let store = test_store();
    let type_hash = vec![0x79; 32];
    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Paged Holders".to_string()),
                symbol: Some("PH".to_string()),
                decimals: Some(8),
                total_supply: Some(300),
                max_supply: None,
                holders_count: 3,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&type_hash, &[0x01; 32], 100);
    batch.put_token_holder(&type_hash, &[0x02; 32], 100);
    batch.put_token_holder(&type_hash, &[0x03; 32], 50);
    batch.put_token_holder_by_balance(&type_hash, &[0x01; 32], 100);
    batch.put_token_holder_by_balance(&type_hash, &[0x02; 32], 100);
    batch.put_token_holder_by_balance(&type_hash, &[0x03; 32], 50);
    batch.put_addr_token_by_balance(&[0x01; 32], &type_hash, 100);
    batch.put_addr_token_by_balance(&[0x02; 32], &type_hash, 100);
    batch.put_addr_token_by_balance(&[0x03; 32], &type_hash, 50);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    let first_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/tokens/{type_hash_hex}/holders?limit=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    let next_cursor = first_json["nextCursor"]
        .as_str()
        .expect("first page should have next cursor")
        .to_string();
    assert_eq!(
        first_json["data"][0]["lockScriptHash"],
        format!("0x{}", "01".repeat(32))
    );

    let second_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/tokens/{type_hash_hex}/holders?limit=1&cursor={next_cursor}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(
        second_json["data"][0]["lockScriptHash"],
        format!("0x{}", "02".repeat(32))
    );
}

#[tokio::test]
async fn test_get_address_tokens_uses_store_backed_pagination_without_warmup_cache() {
    let store = test_store();
    let lock_hash = vec![0x88; 32];
    let token_a = vec![0x81; 32];
    let token_b = vec![0x82; 32];

    store
        .put_token_direct(
            &token_a,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Alpha".to_string()),
                symbol: Some("ALP".to_string()),
                decimals: Some(8),
                total_supply: Some(500),
                max_supply: None,
                holders_count: 1,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_direct(
            &token_b,
            &TokenInfo {
                type_code_hash: vec![0x56; 32],
                hash_type: 1,
                type_args: vec![0x67; 20],
                standard: "sudt".to_string(),
                name: Some("Beta".to_string()),
                symbol: Some("BET".to_string()),
                decimals: Some(4),
                total_supply: Some(300),
                max_supply: None,
                holders_count: 1,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(&store);
    batch.put_token_holder(&token_a, &lock_hash, 200);
    batch.put_token_holder(&token_b, &lock_hash, 100);
    batch.put_token_holder_by_balance(&token_a, &lock_hash, 200);
    batch.put_token_holder_by_balance(&token_b, &lock_hash, 100);
    batch.put_addr_token_by_balance(&lock_hash, &token_a, 200);
    batch.put_addr_token_by_balance(&lock_hash, &token_b, 100);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let lock_hash_hex = format!("0x{}", hex::encode(&lock_hash));

    let first_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/addresses/{lock_hash_hex}/tokens?limit=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body = first_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let first_json: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(
        first_json["data"][0]["typeScriptHash"],
        format!("0x{}", hex::encode(&token_a))
    );
    let next_cursor = first_json["nextCursor"]
        .as_str()
        .expect("first page should have next cursor")
        .to_string();

    let second_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/addresses/{lock_hash_hex}/tokens?limit=1&cursor={next_cursor}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let second_json: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(
        second_json["data"][0]["typeScriptHash"],
        format!("0x{}", hex::encode(&token_b))
    );
}

#[tokio::test]
async fn test_get_token_maximum_supply_status_without_cap() {
    let store = test_store();

    let sudt_hash = vec![0x71; 32];
    store
        .put_token_direct(
            &sudt_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "sudt".to_string(),
                name: Some("Plain sUDT".to_string()),
                symbol: Some("SUDT".to_string()),
                decimals: Some(8),
                total_supply: Some(123),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let xudt_hash = vec![0x72; 32];
    let mut xudt_type_args_with_extension = vec![0xAA; 32];
    xudt_type_args_with_extension.extend_from_slice(&1u32.to_le_bytes());
    store
        .put_token_direct(
            &xudt_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: xudt_type_args_with_extension,
                standard: "xudt".to_string(),
                name: Some("Extensible Token".to_string()),
                symbol: Some("XUDT".to_string()),
                decimals: Some(8),
                total_supply: Some(456),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let xudt_plain_hash = vec![0x73; 32];
    let mut xudt_plain_type_args = vec![0xBB; 32];
    xudt_plain_type_args.extend_from_slice(&0u32.to_le_bytes());
    store
        .put_token_direct(
            &xudt_plain_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: xudt_plain_type_args,
                standard: "xudt".to_string(),
                name: Some("Plain XUDT".to_string()),
                symbol: Some("PXUDT".to_string()),
                decimals: Some(8),
                total_supply: Some(789),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let sudt_request = Request::builder()
        .uri(format!("/api/v1/tokens/0x{}", hex::encode(&sudt_hash)))
        .body(Body::empty())
        .unwrap();
    let sudt_response = app.clone().oneshot(sudt_request).await.unwrap();
    assert_eq!(sudt_response.status(), StatusCode::OK);
    let sudt_body = sudt_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let sudt_json: serde_json::Value = serde_json::from_slice(&sudt_body).unwrap();
    assert_eq!(sudt_json["maximumSupply"], serde_json::Value::Null);
    assert_eq!(sudt_json["maximumSupplyStatus"], "unlimited");

    let xudt_request = Request::builder()
        .uri(format!("/api/v1/tokens/0x{}", hex::encode(&xudt_hash)))
        .body(Body::empty())
        .unwrap();
    let xudt_response = app.clone().oneshot(xudt_request).await.unwrap();
    assert_eq!(xudt_response.status(), StatusCode::OK);
    let xudt_body = xudt_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let xudt_json: serde_json::Value = serde_json::from_slice(&xudt_body).unwrap();
    assert_eq!(xudt_json["maximumSupply"], serde_json::Value::Null);
    assert_eq!(xudt_json["maximumSupplyStatus"], "unknown");

    let xudt_plain_request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/0x{}",
            hex::encode(&xudt_plain_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let xudt_plain_response = app.oneshot(xudt_plain_request).await.unwrap();
    assert_eq!(xudt_plain_response.status(), StatusCode::OK);
    let xudt_plain_body = xudt_plain_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let xudt_plain_json: serde_json::Value = serde_json::from_slice(&xudt_plain_body).unwrap();
    assert_eq!(xudt_plain_json["maximumSupply"], serde_json::Value::Null);
    assert_eq!(xudt_plain_json["maximumSupplyStatus"], "unlimited");
}

#[tokio::test]
async fn test_token_capacity_chart_returns_cumulative_series() {
    let store = test_store();
    let type_hash = vec![0x44; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Test Token".to_string()),
                symbol: Some("TEST".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &type_hash,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &type_hash,
            20240117,
            &TokenDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(json["title"], "TEST Capacity History");
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
    assert_eq!(data[1]["date"], "2024-01-16");
    assert_eq!(data[1]["values"]["used"], "60");
    assert_eq!(data[1]["values"]["unused"], "40");
    assert_eq!(data[2]["date"], "2024-01-17");
    assert_eq!(data[2]["values"]["used"], "50");
    assert_eq!(data[2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
}

#[tokio::test]
async fn test_token_capacity_chart_reads_daily_deltas_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let type_hash = vec![0x64; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    core_store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Derived Delta Token".to_string()),
                symbol: Some("DDT".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    core_store
        .put_token_daily_delta(
            &type_hash,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-15");
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
}

#[tokio::test]
async fn test_token_capacity_chart_rejects_invalid_date_range() {
    let store = test_store();
    let type_hash = vec![0x45; 32];
    let type_hash_hex = format!("0x{}", hex::encode(&type_hash));

    store
        .put_token_direct(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x55; 32],
                hash_type: 1,
                type_args: vec![0x66; 20],
                standard: "xudt".to_string(),
                name: Some("Test Token".to_string()),
                symbol: Some("TEST".to_string()),
                decimals: Some(8),
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/tokens/{}/charts/capacity-history?from=2024-01-31&to=2024-01-01",
            type_hash_hex
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_spore_list_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/spore/objects")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_cluster_capacity_chart_and_cluster_capacity_fields() {
    let store = test_store();
    let cluster_id = [0x42u8; 32];
    let cluster_id_hex = format!("0x{}", hex::encode(cluster_id));

    let cluster_entry = ObjectEntry {
        standard: ObjectStandard::SporeCluster,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("Test Cluster".to_string()),
        description: None,
        is_live: true,
        created_at_block: 123,
        created_at_tx: vec![0x22; 32],
        extra: ObjectExtra::SporeCluster,
    };
    store.put_spore_direct(&cluster_id, &cluster_entry).unwrap();
    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240115,
            &ClusterDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_cluster_daily_delta(
            &cluster_id,
            20240117,
            &ClusterDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/{}/charts/capacity-history",
            cluster_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Test Cluster Capacity History");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");
    assert_eq!(data[1]["values"]["used"], "60");
    assert_eq!(data[1]["values"]["unused"], "40");
    assert_eq!(data[2]["values"]["used"], "50");
    assert_eq!(data[2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
            cluster_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["used"], "60");
    assert_eq!(data[0]["values"]["unused"], "40");

    let request = Request::builder()
        .uri(format!("/api/v1/spore/clusters/{}", cluster_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ownedCapacity"], "80");
    assert_eq!(json["ownedKnowledge"], "50");
    assert_eq!(json["composition"]["tier"], "unknown");
}

#[tokio::test]
async fn test_spore_cluster_holders_supports_pagination() {
    let store = test_store();
    let cluster_id = [0x52u8; 32];
    let owner_a = [0x11u8; 32];
    let owner_b = [0x22u8; 32];

    store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(owner_a.to_vec()),
                name: Some("Holders Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 88,
                created_at_tx: vec![0x33; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_owner_count(&cluster_id, &owner_a, 3);
    batch.put_cluster_owner_count(&cluster_id, &owner_b, 1);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/holders?limit=1",
            hex::encode(cluster_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_a))
    );
    assert_eq!(json["data"][0]["itemCount"], 3);
    let next_cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/holders?limit=1&cursor={}",
            hex::encode(cluster_id),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_b))
    );
    assert_eq!(json["data"][0]["itemCount"], 1);
}

#[tokio::test]
async fn test_spore_cluster_activities_supports_action_filter() {
    let (core_store, append_only_store) = split_test_stores();
    let cluster_id = [0x62u8; 32];
    let mint_tx = vec![0x91; 32];
    let transfer_tx = vec![0x92; 32];
    let burn_tx = vec![0x93; 32];

    // Register cluster so existence check passes
    core_store
        .put_spore_direct(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x21; 32]),
                name: Some("Activities Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 80,
                created_at_tx: vec![0x31; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    // Write pre-computed collection activities (the index the handler now reads)
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&mint_tx, 100, 0);
    core_batch.put_tx_hash_map(&transfer_tx, 200, 0);
    core_batch.put_tx_hash_map(&burn_tx, 300, 0);
    core_batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
        },
    );
    core_batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
        },
    );
    core_batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
        },
    );
    core_batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0xA1; 32],
            timestamp: 1_700_000_100,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0xA2; 32],
            timestamp: 1_700_000_200,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.put_block_header(
        300,
        &CachedBlockHeader {
            hash: vec![0xA3; 32],
            timestamp: 1_700_000_300,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.commit().unwrap();

    let mut append_batch = StoreBatch::new(append_only_store.as_ref());
    append_batch.put_object_collection_activity(
        &cluster_id,
        100,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: mint_tx.clone(),
            block_hash: vec![0xA1; 32],
            timestamp_ms: 1_700_000_100,
            actions: vec![AssetAction::Mint],
        },
    );
    append_batch.put_object_collection_activity(
        &cluster_id,
        200,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: transfer_tx.clone(),
            block_hash: vec![0xA2; 32],
            timestamp_ms: 1_700_000_200,
            actions: vec![AssetAction::Transfer],
        },
    );
    append_batch.put_object_collection_activity(
        &cluster_id,
        300,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: burn_tx.clone(),
            block_hash: vec![0xA3; 32],
            timestamp_ms: 1_700_000_300,
            actions: vec![AssetAction::Burn],
        },
    );
    append_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;

    // All activities — newest first
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/activities?limit=20",
            hex::encode(cluster_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["data"][0]["actions"][0], "burn");
    assert_eq!(json["data"][1]["blockNumber"], 200);
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["blockNumber"], 100);
    assert_eq!(json["data"][2]["actions"][0], "mint");

    // Action filter
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/activities?limit=20&action=transfer",
            hex::encode(cluster_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");

    // Invalid action filter
    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/clusters/0x{}/activities?action=invalid",
            hex::encode(cluster_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_spore_capacity_chart_and_spore_capacity_fields() {
    let store = test_store();
    let spore_id = [0x77u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 1024,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();
    store
        .put_spore_daily_delta(
            &spore_id,
            20240115,
            &SporeDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 61,
            },
        )
        .unwrap();
    store
        .put_spore_daily_delta(
            &spore_id,
            20240117,
            &SporeDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -11,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/objects/{}/charts/capacity-history",
            spore_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Spore Capacity History");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[1]["values"]["used"], "61");
    assert_eq!(data[1]["values"]["unused"], "39");
    assert_eq!(data[2]["values"]["used"], "50");
    assert_eq!(data[2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/spore/objects/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
            spore_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-16");
    assert_eq!(data[0]["values"]["used"], "61");
    assert_eq!(data[0]["values"]["unused"], "39");

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ownedCapacity"], "80");
    assert_eq!(json["ownedKnowledge"], "50");
}

#[tokio::test]
async fn test_spore_decode_endpoint_returns_pending_without_cached_result() {
    let store = test_store();
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 128,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "pending");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "dob/0");
    assert_eq!(json["traits"], serde_json::json!([]));
    assert!(json["issues"].as_array().unwrap().iter().any(|issue| issue
        .as_str()
        .is_some_and(|s| s.contains("background worker"))));
}

#[tokio::test]
async fn test_spore_decode_endpoint_returns_decoded_from_cache() {
    let store = test_store();
    let spore_id = [0x55u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 321,
        created_at_tx: vec![0xBB; 32],
        extra: ObjectExtra::Spore {
            content_type: "dob/0".to_string(),
            content_length: 128,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let decoded_entry = DobDecodedEntry {
        traits: vec![
            DobDecodedTrait {
                name: "Background".to_string(),
                value: "red".to_string(),
            },
            DobDecodedTrait {
                name: "Level".to_string(),
                value: "11".to_string(),
            },
        ],
        media: vec![ckbadger_store::DecodedMedia {
            media_type: "image/svg+xml".to_string(),
            role: Some("render".to_string()),
            size: 29,
            hash: "abc123".to_string(),
            step: None,
        }],
        media_sources: vec![],
        decoded_at: 1700000000,
    };
    store
        .put_dob_decoded_direct(&spore_id, &decoded_entry)
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "decoded");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "dob/0");
    let traits = json["traits"].as_array().unwrap();
    assert_eq!(traits.len(), 2);
    assert_eq!(traits[0]["name"], "Background");
    assert_eq!(traits[0]["value"], "red");
    assert_eq!(traits[1]["name"], "Level");
    assert_eq!(traits[1]["value"], "11");
    let media = json["media"].as_array().unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(media[0]["mediaType"], "image/svg+xml");
    assert_eq!(media[0]["role"], "render");
    assert_eq!(media[0]["size"], 29);
    assert_eq!(media[0]["hash"], "abc123");
    assert!(json["issues"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_spore_decode_endpoint_non_dob_returns_pending_with_content_type() {
    let store = test_store();
    let spore_id = [0x66u8; 32];
    let spore_id_hex = format!("0x{}", hex::encode(spore_id));

    let spore_entry = ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0xAA; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 500,
        created_at_tx: vec![0xCC; 32],
        extra: ObjectExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 4096,
            media_profile: SporeMediaProfile::default(),
        },
    };
    store.put_spore_direct(&spore_id, &spore_entry).unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", spore_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "pending");
    assert_eq!(json["sporeId"], spore_id_hex);
    assert_eq!(json["contentType"], "image/png");
    assert_eq!(json["traits"], serde_json::json!([]));
}

#[tokio::test]
async fn test_spore_decode_endpoint_returns_not_found_for_missing_spore() {
    let store = test_store();
    let missing_id_hex = format!("0x{}", hex::encode([0x99u8; 32]));

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!("/api/v1/spore/objects/{}/decode", missing_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_assets_nft_includes_spore_cluster_name_when_aggregate_name_missing() {
    let store = test_store();

    let cluster_id = [0x42u8; 32];
    let cluster_entry = ObjectEntry {
        standard: ObjectStandard::SporeCluster,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some("Recovered Cluster Name".to_string()),
        description: Some("desc".to_string()),
        is_live: true,
        created_at_block: 123,
        created_at_tx: vec![0x22; 32],
        extra: ObjectExtra::SporeCluster,
    };
    store.put_spore_direct(&cluster_id, &cluster_entry).unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_id,
        &ClusterAggregate {
            name: None,
            description: None,
            total_count: 3,
            live_count: 3,
            owner_count: 1,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["name"], "Recovered Cluster Name");
    assert_eq!(json["data"][0]["assetType"], "object");
    assert_eq!(json["data"][0]["standard"], "spore");
}

#[tokio::test]
async fn test_assets_rejects_legacy_dob_type_filter() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=dob")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_assets_list_supports_standard_filter_for_tokens_and_nfts() {
    let store = test_store();
    let token_xudt = [0x61u8; 32];
    let token_sudt = [0x62u8; 32];
    let spore_cluster_id = [0x71u8; 32];
    let dotbit_collection_id = b"dotbit_collection_______________".to_vec();

    for (type_hash, standard, symbol) in
        [(token_xudt, "xudt", "XUDT"), (token_sudt, "sudt", "SUDT")]
    {
        store
            .put_token_direct(
                &type_hash,
                &TokenInfo {
                    type_code_hash: vec![0xAA; 32],
                    hash_type: 1,
                    type_args: vec![0x01; 20],
                    standard: standard.to_string(),
                    name: Some(format!("{symbol} Token")),
                    symbol: Some(symbol.to_string()),
                    decimals: Some(8),
                    total_supply: Some(1000),
                    max_supply: None,
                    holders_count: 10,
                    first_seen_block: 1,
                    icon_url: None,
                    description: None,
                    transfers_count: 1,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                20240115,
                &TokenDailyDelta {
                    owned_capacity_delta: 100,
                    owned_knowledge_delta: 50,
                },
            )
            .unwrap();
    }

    store
        .put_spore_direct(
            &spore_cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0x11; 32]),
                name: Some("Spore Filter Cluster".to_string()),
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0x22; 32],
                extra: ObjectExtra::SporeCluster,
            },
        )
        .unwrap();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &spore_cluster_id,
        &ClusterAggregate {
            name: Some("Spore Filter Cluster".to_string()),
            description: None,
            total_count: 1,
            live_count: 1,
            owner_count: 1,
            ..Default::default()
        },
    );
    batch.put_object_collection_aggregate(
        &dotbit_collection_id,
        &ObjectCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: ObjectStandard::default(),
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let token_request = Request::builder()
        .uri("/api/v1/assets?type=token&standard=xudt")
        .body(Body::empty())
        .unwrap();
    let token_response = app.clone().oneshot(token_request).await.unwrap();
    assert_eq!(token_response.status(), StatusCode::OK);
    let token_body = token_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let token_json: serde_json::Value = serde_json::from_slice(&token_body).unwrap();
    assert_eq!(token_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(token_json["data"][0]["standard"], "xudt");
    assert_eq!(token_json["data"][0]["assetType"], "token");

    let nft_request = Request::builder()
        .uri("/api/v1/assets?type=nft&standard=spore")
        .body(Body::empty())
        .unwrap();
    let nft_response = app.oneshot(nft_request).await.unwrap();
    assert_eq!(nft_response.status(), StatusCode::OK);
    let nft_body = nft_response.into_body().collect().await.unwrap().to_bytes();
    let nft_json: serde_json::Value = serde_json::from_slice(&nft_body).unwrap();
    assert_eq!(nft_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(nft_json["data"][0]["standard"], "spore");
    assert_eq!(nft_json["data"][0]["assetType"], "object");
}

#[tokio::test]
async fn test_assets_list_supports_composition_tier_filter_and_onchain_ratio_sort() {
    let store = test_store();
    let cluster_onchain = [0x81u8; 32];
    let cluster_centralized = [0x82u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_cluster_aggregate(
        &cluster_onchain,
        &ClusterAggregate {
            name: Some("Onchain Cluster".to_string()),
            description: None,
            total_count: 5,
            live_count: 5,
            owner_count: 2,
            btc_ckb_count: 0,
            pure_ckb_count: 5,
            decentralized_mixture_count: 0,
            centralized_mixture_count: 0,
            unknown_count: 0,
        },
    );
    batch.put_cluster_aggregate(
        &cluster_centralized,
        &ClusterAggregate {
            name: Some("Centralized Cluster".to_string()),
            description: None,
            total_count: 4,
            live_count: 4,
            owner_count: 2,
            btc_ckb_count: 0,
            pure_ckb_count: 0,
            decentralized_mixture_count: 0,
            centralized_mixture_count: 4,
            unknown_count: 0,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft&composition_tier=pure_ckb")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Onchain Cluster");
    assert_eq!(rows[0]["compositionTier"], "pure_ckb");

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft&composition_tier=centralized_mixture")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Centralized Cluster");
    assert_eq!(rows[0]["compositionTier"], "centralized_mixture");

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft&sort_key=onchain_ratio&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "Onchain Cluster");
    assert_eq!(rows[1]["name"], "Centralized Cluster");
}

#[tokio::test]
async fn test_assets_list_includes_did_ckb_collection_under_nft_type() {
    let store = test_store();
    let did_collection_id = *b"did_ckb_collection______________";

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_object_collection_aggregate(
        &did_collection_id,
        &ObjectCollectionAggregate {
            name: Some("did:ckb".to_string()),
            standard: ObjectStandard::default(),
            total_count: 2,
            live_count: 2,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft&standard=did:ckb")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["assetType"], "identity");
    assert_eq!(rows[0]["standard"], "did_ckb");
    assert_eq!(rows[0]["name"], "did:ckb");
}

#[tokio::test]
async fn test_nft_collection_items_supports_did_ckb_collection_from_spore_data() {
    let store = test_store();
    let did_collection_id = *b"did_ckb_collection______________";
    let did_id = [0xD3u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &did_id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("did:alice.ckb".to_string()),
            is_live: true,
            created_at_block: 321,
            created_at_tx: vec![0x22; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.put_identity_collection_aggregate(
        &did_collection_id,
        &IdentityCollectionAggregate {
            name: Some("did:ckb".to_string()),
            standard: IdentityStandard::DidCkb,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity_by_collection(&did_collection_id, &did_id);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/0x{}/items",
            hex::encode(did_collection_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["standard"], "did_ckb");
    assert_eq!(rows[0]["name"], "did:alice.ckb");
    assert_eq!(rows[0]["isLive"], true);
}

#[tokio::test]
async fn test_assets_list_defaults_to_capacity_sort_and_supports_cursor_pagination() {
    let store = test_store();
    let token_a = [0x11u8; 32];
    let token_b = [0x22u8; 32];

    store
        .put_token_direct(
            &token_a,
            &TokenInfo {
                type_code_hash: vec![0xAA; 32],
                hash_type: 1,
                type_args: vec![0x01; 20],
                standard: "xudt".to_string(),
                name: Some("Alpha Token".to_string()),
                symbol: Some("ALPHA".to_string()),
                decimals: Some(8),
                total_supply: Some(1000),
                max_supply: None,
                holders_count: 10,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 1,
            },
        )
        .unwrap();
    store
        .put_token_direct(
            &token_b,
            &TokenInfo {
                type_code_hash: vec![0xBB; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("Beta Token".to_string()),
                symbol: Some("BETA".to_string()),
                decimals: Some(8),
                total_supply: Some(2000),
                max_supply: None,
                holders_count: 20,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 2,
            },
        )
        .unwrap();

    store
        .put_token_daily_delta(
            &token_a,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &token_b,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 300,
                owned_knowledge_delta: 120,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=token&limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["id"], format!("0x{}", hex::encode(token_b)));
    assert_eq!(json["data"][0]["ownedCapacity"], "300");
    assert_eq!(json["data"][0]["ownedKnowledge"], "120");

    let next_cursor = json["nextCursor"].as_str().unwrap();
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets?type=token&limit=1&sort_key=capacity&sort_direction=desc&cursor={next_cursor}"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["id"], format!("0x{}", hex::encode(token_a)));
    assert!(json["nextCursor"].is_null());

    let request = Request::builder()
        .uri("/api/v1/assets?type=token&sort_key=capacity&sort_direction=asc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"][0]["id"], format!("0x{}", hex::encode(token_a)));
}

#[tokio::test]
async fn test_assets_list_token_errors_when_daily_deltas_invalid() {
    let store = test_store();
    let healthy_token = [0x31u8; 32];
    let broken_token = [0x32u8; 32];

    for (hash, name, symbol) in [
        (healthy_token, "Healthy Token", "HLT"),
        (broken_token, "Broken Token", "BKT"),
    ] {
        store
            .put_token_direct(
                &hash,
                &TokenInfo {
                    type_code_hash: vec![0xAA; 32],
                    hash_type: 1,
                    type_args: vec![0x01; 20],
                    standard: "xudt".to_string(),
                    name: Some(name.to_string()),
                    symbol: Some(symbol.to_string()),
                    decimals: Some(8),
                    total_supply: Some(1000),
                    max_supply: None,
                    holders_count: 10,
                    first_seen_block: 1,
                    icon_url: None,
                    description: None,
                    transfers_count: 1,
                },
            )
            .unwrap();
    }

    store
        .put_token_daily_delta(
            &healthy_token,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 200,
                owned_knowledge_delta: 100,
            },
        )
        .unwrap();

    // Broken history: used exceeds capacity; API must fail fast instead of masking.
    store
        .put_token_daily_delta(
            &broken_token,
            20240115,
            &TokenDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 120,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=token&sort_key=capacity&sort_direction=desc")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    let message = json["message"].as_str().unwrap();
    assert!(message.contains("asset cache warmup failed"));
    assert!(message.contains("invalid token daily deltas during warmup"));
    assert!(message.contains(&format!("type_hash=0x{}", hex::encode(broken_token))));
}

#[tokio::test]
async fn test_assets_nft_collection_capacity_chart_and_capacity_fields() {
    let store = test_store();
    let collection_id = [0x24u8; 24];
    let collection_id_hex = format!("0x{}", hex::encode(collection_id));

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_object_collection_aggregate(
        &collection_id,
        &ObjectCollectionAggregate {
            name: Some("Test NFT Collection".to_string()),
            standard: ObjectStandard::MnftToken,
            total_count: 100,
            live_count: 60,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    store
        .put_object_daily_delta(
            &collection_id,
            20240115,
            &ObjectDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();
    store
        .put_object_daily_delta(
            &collection_id,
            20240117,
            &ObjectDailyDelta {
                owned_capacity_delta: -20,
                owned_knowledge_delta: -10,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/{}/charts/capacity-history",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "Test NFT Collection Capacity History");
    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][1]["values"]["used"], "60");
    assert_eq!(json["data"][1]["values"]["unused"], "40");
    assert_eq!(json["data"][2]["values"]["used"], "50");
    assert_eq!(json["data"][2]["values"]["unused"], "30");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/{}/charts/capacity-history?from=2024-01-16&to=2024-01-16",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["date"], "2024-01-16");
    assert_eq!(json["data"][0]["values"]["used"], "60");
    assert_eq!(json["data"][0]["values"]["unused"], "40");

    let request = Request::builder()
        .uri(format!("/api/v1/assets/objects/{}", collection_id_hex))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["ownedCapacity"], "80");
    assert_eq!(json["ownedKnowledge"], "50");
}

#[tokio::test]
async fn test_assets_nft_collection_accepts_dotbit_alias() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: None,
            standard: IdentityStandard::DotBit,
            total_count: 200,
            live_count: 120,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.commit().unwrap();

    store
        .put_object_daily_delta(
            &collection_id,
            20240115,
            &ObjectDailyDelta {
                owned_capacity_delta: 100,
                owned_knowledge_delta: 60,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], ".bit Capacity History");
    assert_eq!(json["data"][0]["values"]["used"], "60");
    assert_eq!(json["data"][0]["values"]["unused"], "40");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "dotbit");
    assert_eq!(json["name"], ".bit");
    assert_eq!(json["ownedCapacity"], "100");
    assert_eq!(json["ownedKnowledge"], "60");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/DOTBIT/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let request = Request::builder()
        .uri("/api/v1/assets/objects/%2Ebit")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "dotbit");
    assert_eq!(json["name"], ".bit");
}

#[tokio::test]
async fn test_assets_nft_collection_detail_uses_preaggregated_counts() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 200,
            live_count: 120,
            holders_count: 77,
            activities_count: 6_543,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["holdersCount"], 77);
    assert_eq!(json["activitiesCount"], 6543);
}

#[tokio::test]
async fn test_assets_nft_collection_detail_enriches_mnft_class_metadata() {
    let store = test_store();
    let issuer_id = [0x21u8; 20];
    let class_id = [0x31u8; 24];

    let mut batch = StoreBatch::new(store.as_ref());

    // Insert issuer ObjectEntry with MnftIssuer extra
    batch.put_object(
        &issuer_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x01; 32]),
            name: Some("Issuer-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 90,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftIssuer {
                class_count: 2,
                set_count: 3,
                info: Some(br#"{"name":"Issuer-A"}"#.to_vec()),
            },
        },
    );

    // Insert class ObjectEntry with MnftClass extra
    batch.put_object(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x02; 32]),
            name: Some("Class-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 95,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 500,
                issued: 128,
                configure: 9,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );

    // Insert ObjectCollectionAggregate (required for get_object_collection to find it)
    batch.put_object_collection_aggregate(
        &class_id,
        &ObjectCollectionAggregate {
            name: Some("Class-A".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 50,
            live_count: 40,
            holders_count: 12,
            activities_count: 30,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    // Hit the endpoint
    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/0x{}",
            hex::encode(class_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify base collection fields
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["name"], "Class-A");
    assert_eq!(json["totalCount"], 50);
    assert_eq!(json["liveCount"], 40);
    assert_eq!(json["holdersCount"], 12);
    assert_eq!(json["activitiesCount"], 30);

    // Verify enriched class metadata
    assert_eq!(json["classDetail"]["name"], "Class-A");
    assert_eq!(json["classDetail"]["description"], "Class description");
    assert_eq!(json["classDetail"]["renderer"], "renderer:v1");
    assert_eq!(json["classDetail"]["total"], 500);
    assert_eq!(json["classDetail"]["issued"], 128);
    assert_eq!(json["classDetail"]["configure"], 9);
    assert_eq!(
        json["classDetail"]["classId"],
        format!("0x{}", hex::encode(class_id))
    );
    assert_eq!(
        json["classDetail"]["issuerId"],
        format!("0x{}", hex::encode(issuer_id))
    );

    // Verify enriched issuer metadata
    assert_eq!(json["issuerDetail"]["name"], "Issuer-A");
    assert_eq!(json["issuerDetail"]["classCount"], 2);
    assert_eq!(json["issuerDetail"]["setCount"], 3);
    assert_eq!(
        json["issuerDetail"]["issuerId"],
        format!("0x{}", hex::encode(issuer_id))
    );

    // Verify created_at_block and owner_lock_hash
    assert_eq!(json["createdAtBlock"], 95);
    let owner_hash = json["ownerLockHash"].as_str().unwrap();
    assert!(owner_hash.starts_with("0x"));
    assert_eq!(owner_hash, format!("0x{}", hex::encode(vec![0x02u8; 32])));
}

#[tokio::test]
async fn test_assets_nft_collection_accepts_did_ckb_aliases() {
    let store = test_store();
    let collection_id = b"did_ckb_collection______________".to_vec();
    let did_id = [0xA5u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &did_id,
        &IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![0x21; 32]),
            name: Some("did:alice.ckb".to_string()),
            is_live: true,
            created_at_block: 888,
            created_at_tx: vec![0x33; 32],
            extra: IdentityExtra::DidCkb,
        },
    );
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: None,
            standard: IdentityStandard::DidCkb,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity_by_collection(&collection_id, &did_id);
    batch.commit().unwrap();

    store
        .put_object_daily_delta(
            &collection_id,
            20240115,
            &ObjectDailyDelta {
                owned_capacity_delta: 120,
                owned_knowledge_delta: 70,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/did:ckb")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["standard"], "did_ckb");
    assert_eq!(json["name"], "did:ckb");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/did_ckb/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "did:alice.ckb");
    assert_eq!(json["data"][0]["standard"], "did_ckb");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/did%3Ackb/charts/capacity-history")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "did:ckb Capacity History");
    assert_eq!(json["data"][0]["values"]["used"], "70");
    assert_eq!(json["data"][0]["values"]["unused"], "50");
}

#[tokio::test]
async fn test_assets_did_ckb_item_detail_and_activities() {
    let store = test_store();
    let did_id = [0xB7u8; 32];
    let mint_tx = vec![0x91; 32];
    let transfer_tx = vec![0x92; 32];

    {
        let mut batch = StoreBatch::new(store.as_ref());
        batch.put_identity(
            &did_id,
            &IdentityEntry {
                standard: IdentityStandard::DidCkb,
                owner_lock_hash: Some(vec![0x31; 32]),
                name: Some("did:alice.ckb".to_string()),
                is_live: true,
                created_at_block: 100,
                created_at_tx: mint_tx.clone(),
                extra: IdentityExtra::DidCkb,
            },
        );
        batch.commit().unwrap();
    }

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_spore_outpoint(&mint_tx, 0, &did_id);
    batch.put_spore_outpoint(&transfer_tx, 0, &did_id);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![0x61; 20],
            type_script_hash: Some(vec![0x71; 32]),
            type_code_hash: Some(vec![0x81; 32]),
            type_hash_type: Some(1),
            type_args: Some(did_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 200,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}",
            hex::encode(did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["name"], "did:alice.ckb");
    assert_eq!(json["standard"], "did_ckb");
    assert_eq!(json["isLive"], true);
    assert_eq!(json["txHash"], serde_json::Value::Null);
    assert_eq!(json["outputIndex"], serde_json::Value::Null);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}/activities?limit=20",
            hex::encode(did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["blockNumber"], 200);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 100);
    assert_eq!(json["data"][1]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/did/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(did_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
}

#[tokio::test]
async fn test_assets_nft_list_uses_dotbit_display_name_when_aggregate_name_missing() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_object_collection_aggregate(
        &collection_id,
        &ObjectCollectionAggregate {
            name: None,
            standard: ObjectStandard::default(),
            total_count: 20,
            live_count: 12,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets?type=nft")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"][0]["name"], ".bit");
    assert_eq!(json["data"][0]["standard"], "dotbit");
}

#[tokio::test]
async fn test_assets_nft_collection_items_dotbit_human_readable_and_pagination() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let dotbit_code_hash =
        hex::decode("4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918").unwrap();
    let nft_a = [0x11u8; 20];
    let nft_b = [0x22u8; 20];
    let nft_a_type_hash = compute_script_hash(&dotbit_code_hash, 1, &nft_a);
    let nft_a_tx_hash = vec![0x9au8; 32];
    let nft_a_output_index = 2i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 2,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_a,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_b,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: Some("bob.bit".to_string()),
            is_live: false,
            created_at_block: 101,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_900_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_a);
    batch.put_identity_by_collection(&collection_id, &nft_b);
    batch.put_cell(
        &nft_a_tx_hash,
        nft_a_output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(nft_a_type_hash.clone()),
            type_code_hash: Some(dotbit_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(nft_a.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
    );
    batch.put_dotbit_account_outpoint(&nft_a_tx_hash, nft_a_output_index, &nft_a);
    batch.put_dotbit_outpoint_by_account_id(&nft_a, &nft_a_tx_hash, nft_a_output_index);
    batch.put_cell_by_type(&nft_a_type_hash, 100, &nft_a_tx_hash, nft_a_output_index);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "alice.bit");
    assert_eq!(json["data"][0]["isLive"], true);
    assert_eq!(json["data"][0]["expiredAt"], 1_800_000_000u64);
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&nft_a_tx_hash))
    );
    assert_eq!(json["data"][0]["outputIndex"], nft_a_output_index);
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/dotbit/items?limit=1&cursor={cursor}"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "bob.bit");
    assert_eq!(json["data"][0]["isLive"], false);
    assert_eq!(json["data"][0]["txHash"], serde_json::Value::Null);
    assert_eq!(json["data"][0]["outputIndex"], serde_json::Value::Null);
    assert_eq!(json["nextCursor"], serde_json::Value::Null);

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20&search=alice")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "alice.bit");
    assert!(json.get("total").is_none());

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20&status=live")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "alice.bit");
    assert_eq!(json["data"][0]["isLive"], true);

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20&status=recycled")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["name"], "bob.bit");
    assert_eq!(json["data"][0]["isLive"], false);
    assert_eq!(json["data"][0]["txHash"], serde_json::Value::Null);
    assert_eq!(json["data"][0]["outputIndex"], serde_json::Value::Null);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}",
            hex::encode(nft_a)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["name"], "alice.bit");
    assert_eq!(json["isLive"], true);
    assert_eq!(json["txHash"], format!("0x{}", hex::encode(&nft_a_tx_hash)));
    assert_eq!(json["outputIndex"], nft_a_output_index);
}

#[tokio::test]
async fn test_assets_nft_collection_items_dotbit_requires_outpoint_index_even_with_live_cell() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let dotbit_code_hash =
        hex::decode("4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918").unwrap();
    let nft_id = [0x66u8; 20];
    let nft_type_hash = compute_script_hash(&dotbit_code_hash, 1, &nft_id);
    let tx_hash = vec![0xabu8; 32];
    let output_index = 3i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("indexed.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(nft_type_hash.clone()),
            type_code_hash: Some(dotbit_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(nft_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
    );
    batch.put_cell_by_type(&nft_type_hash, 100, &tx_hash, output_index);
    // Intentionally no put_dotbit_account_outpoint(...): live cell exists but index is required.
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("live dotbit account missing outpoint index"));
}

#[tokio::test]
async fn test_assets_nft_collection_items_dotbit_live_missing_outpoint_fails_fast() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let nft_id = [0x67u8; 20];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x31; 32]),
            name: Some("broken.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_id);
    // Intentionally no outpoint index and no fallback-resolvable live cell.
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/items?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "internal_error");
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("live dotbit account missing outpoint index"));
}

#[tokio::test]
async fn test_assets_nft_collection_items_mnft_live_outpoint() {
    let store = test_store();
    let class_id = [0x24u8; 24];
    let issuer_id = [0x13u8; 20];
    let token_id = [0x42u8; 28];
    let tx_hash = vec![0x55u8; 32];
    let output_index = 6i16;
    let collection_id_hex = format!("0x{}", hex::encode(class_id));

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_object_collection_aggregate(
        &class_id,
        &ObjectCollectionAggregate {
            name: Some("Genesis Class".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 1,
            live_count: 1,
            holders_count: 0,
            activities_count: 0,
            ..Default::default()
        },
    );
    batch.put_object(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("Genesis Class".to_string()),
            description: None,
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 1000,
                issued: 1,
                configure: 7,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );
    batch.put_object(
        &token_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(vec![0x22; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 101,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftToken {
                token_index: 1,
                characteristic: vec![1, 2, 3, 4, 5, 6, 7, 8],
                configure: 3,
                state: 1,
            },
        },
    );
    batch.put_object_by_collection(&class_id, &token_id);
    batch.put_mnft_token_outpoint(&tx_hash, output_index, &token_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x41; 32],
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x62; 32]),
            type_hash_type: Some(1),
            type_args: Some(token_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        101,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/{}/items?limit=20",
            collection_id_hex
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["txHash"],
        format!("0x{}", hex::encode(&tx_hash))
    );
    assert_eq!(json["data"][0]["outputIndex"], output_index);
}

#[tokio::test]
async fn test_assets_nft_collection_holders_supports_pagination() {
    let store = test_store();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let nft_a = [0x81u8; 20];
    let nft_b = [0x82u8; 20];
    let nft_c = [0x83u8; 20];
    let nft_d = [0x84u8; 20];
    let owner_a = vec![0x11u8; 32];
    let owner_b = vec![0x22u8; 32];
    let owner_c = vec![0x33u8; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 4,
            live_count: 3,
            holders_count: 2,
            activities_count: 0,
        },
    );
    batch.put_identity(
        &nft_a,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_a.clone()),
            name: Some("alpha.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_b,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_a.clone()),
            name: Some("beta.bit".to_string()),
            is_live: true,
            created_at_block: 101,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_001),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_c,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_b.clone()),
            name: Some("gamma.bit".to_string()),
            is_live: true,
            created_at_block: 102,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_002),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity(
        &nft_d,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_c),
            name: Some("dead.bit".to_string()),
            is_live: false,
            created_at_block: 103,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_003),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_identity_by_collection(&collection_id, &nft_a);
    batch.put_identity_by_collection(&collection_id, &nft_b);
    batch.put_identity_by_collection(&collection_id, &nft_c);
    batch.put_identity_by_collection(&collection_id, &nft_d);
    batch.put_identity_owner_count(&collection_id, &owner_a, 2);
    batch.put_identity_owner_count(&collection_id, &owner_b, 1);
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/holders?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["total"], 2);
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_a))
    );
    assert_eq!(json["data"][0]["itemCount"], 2);
    let next_cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/dotbit/holders?limit=1&cursor={next_cursor}"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_b))
    );
    assert_eq!(json["data"][0]["itemCount"], 1);
}

#[tokio::test]
async fn test_assets_nft_collection_activities_supports_action_filter() {
    let (core_store, append_only_store) = split_test_stores();
    let collection_id = b"dotbit_collection_______________".to_vec();
    let account_id = [0x91u8; 20];
    let mint_tx = vec![0xa1; 32];
    let transfer_tx = vec![0xa2; 32];
    let burn_tx = vec![0xa3; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_identity_collection_aggregate(
        &collection_id,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 0,
            holders_count: 0,
            activities_count: 0,
        },
    );
    core_batch.put_identity(
        &account_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: Some("burned.bit".to_string()),
            is_live: false,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    core_batch.put_identity_by_collection(&collection_id, &account_id);
    core_batch.put_dotbit_account_outpoint(&mint_tx, 0, &account_id);
    core_batch.put_dotbit_outpoint_by_account_id(&account_id, &mint_tx, 0);
    core_batch.put_dotbit_account_outpoint(&transfer_tx, 0, &account_id);
    core_batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx, 0);
    core_batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x41; 32],
            lock_hash_type: 1,
            lock_args: vec![0x51; 20],
            type_script_hash: Some(vec![0x61; 32]),
            type_code_hash: Some(vec![0x62; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    core_batch.put_consumed_cell_with_consumer(
        &transfer_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x32; 32],
            lock_code_hash: vec![0x42; 32],
            lock_hash_type: 1,
            lock_args: vec![0x52; 20],
            type_script_hash: Some(vec![0x63; 32]),
            type_code_hash: Some(vec![0x64; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        200,
        300,
        Some(&burn_tx),
    );
    core_batch.put_tx_hash_map(&mint_tx, 100, 0);
    core_batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
        },
    );
    core_batch.put_tx_hash_map(&transfer_tx, 200, 0);
    core_batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
        },
    );
    core_batch.put_tx_hash_map(&burn_tx, 300, 0);
    core_batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 1,
            outputs_count: 0,
            fee: 0,
            tx_size: 160,
            cycles: None,
        },
    );
    core_batch.put_block_header(
        100,
        &CachedBlockHeader {
            hash: vec![0xB1; 32],
            timestamp: 1_700_000_100,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: vec![0xB2; 32],
            timestamp: 1_700_000_200,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.put_block_header(
        300,
        &CachedBlockHeader {
            hash: vec![0xB3; 32],
            timestamp: 1_700_000_300,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.commit().unwrap();

    let mut append_batch = StoreBatch::new(append_only_store.as_ref());
    append_batch.put_identity_collection_activity(
        &collection_id,
        100,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: mint_tx.clone(),
            block_hash: vec![0xB1; 32],
            timestamp_ms: 1_700_000_100,
            actions: vec![AssetAction::Mint],
        },
    );
    append_batch.put_identity_collection_activity(
        &collection_id,
        200,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: transfer_tx.clone(),
            block_hash: vec![0xB2; 32],
            timestamp_ms: 1_700_000_200,
            actions: vec![AssetAction::Transfer],
        },
    );
    append_batch.put_identity_collection_activity(
        &collection_id,
        300,
        0,
        &ObjectCollectionActivityEntry {
            tx_hash: burn_tx.clone(),
            block_hash: vec![0xB3; 32],
            timestamp_ms: 1_700_000_300,
            actions: vec![AssetAction::Burn],
        },
    );
    append_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/activities?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["data"][0]["actions"][0], "burn");
    assert_eq!(json["data"][1]["blockNumber"], 200);
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["blockNumber"], 100);
    assert_eq!(json["data"][2]["actions"][0], "mint");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/activities?limit=20&action=burn")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "burn");

    let request = Request::builder()
        .uri("/api/v1/assets/objects/dotbit/activities?action=invalid")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_assets_nft_item_detail_mnft() {
    let store = test_store();
    let issuer_id = [0x21u8; 20];
    let class_id = [0x31u8; 24];
    let token_id = [0x41u8; 28];
    let tx_hash = vec![0x91u8; 32];
    let output_index = 4i16;

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_object(
        &issuer_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0x01; 32]),
            name: Some("Issuer-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 90,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftIssuer {
                class_count: 2,
                set_count: 3,
                info: Some(br#"{"name":"Issuer-A"}"#.to_vec()),
            },
        },
    );
    batch.put_object(
        &class_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(issuer_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x02; 32]),
            name: Some("Class-A".to_string()),
            description: None,
            is_live: true,
            created_at_block: 95,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftClass {
                description: Some("Class description".to_string()),
                renderer: Some("renderer:v1".to_string()),
                total: 500,
                issued: 128,
                configure: 9,
                composition_tier: CompositionTier::PureCkb,
            },
        },
    );
    batch.put_object(
        &token_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(vec![0x03; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 120,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftToken {
                token_index: 128,
                characteristic: vec![0xaa; 8],
                configure: 5,
                state: 2,
            },
        },
    );
    batch.put_mnft_token_outpoint(&tx_hash, output_index, &token_id);
    batch.put_cell(
        &tx_hash,
        output_index,
        &LiveCellInfo {
            capacity: 300_00000000,
            lock_script_hash: vec![0x31; 32],
            lock_code_hash: vec![0x32; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x33; 32]),
            type_code_hash: Some(vec![0x34; 32]),
            type_hash_type: Some(1),
            type_args: Some(token_id.to_vec()),
            data_size: 64,
            occupied_capacity: 62_00000000,
            udt_amount: None,
            data_hash: None,
        },
        120,
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["nftId"], format!("0x{}", hex::encode(token_id)));
    assert_eq!(json["standard"], "m-nft");
    assert_eq!(json["tokenIndex"], 128);
    assert_eq!(json["state"], 2);
    assert_eq!(json["class"]["name"], "Class-A");
    assert_eq!(json["issuer"]["name"], "Issuer-A");
    assert_eq!(json["txHash"], format!("0x{}", hex::encode(&tx_hash)));
    assert_eq!(json["outputIndex"], output_index);
    assert_eq!(json["lifecycle"][0]["event"], "mint");
    assert_eq!(json["lifecycle"][1]["event"], "live");
}

#[tokio::test]
async fn test_assets_nft_item_activities_mnft() {
    let store = test_store();
    let class_id = [0x31u8; 24];
    let token_id = [0x41u8; 28];
    let owner_lock_hash = vec![0x77u8; 32];
    let previous_owner_lock_hash = vec![0x66u8; 32];
    let mint_tx = vec![0x93; 32];
    let transfer_tx = vec![0x91; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_object(
        &token_id,
        &ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(class_id.to_vec()),
            token_id: Some(token_id.to_vec()),
            owner_lock_hash: Some(owner_lock_hash.clone()),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 120,
            created_at_tx: vec![],
            extra: ObjectExtra::MnftToken {
                token_index: 128,
                characteristic: vec![0xaa; 8],
                configure: 5,
                state: 2,
            },
        },
    );
    batch.put_mnft_token_outpoint(&mint_tx, 0, &token_id);
    batch.put_mnft_token_outpoint(&transfer_tx, 0, &token_id);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: previous_owner_lock_hash,
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: Some(vec![0x44; 32]),
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(token_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        300,
        Some(&transfer_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 300, 0);
    batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}/activities?limit=20",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 100);
    assert_eq!(json["data"][1]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}/activities?limit=1",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 300);
    assert_eq!(json["hasMore"], true);
    let next_cursor = json["nextCursor"]
        .as_str()
        .expect("next cursor for mnft activities");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}/activities?limit=1&cursor={}",
            hex::encode(token_id),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 100);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["actions"][0], "transfer");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/objects/items/0x{}/activities?action=invalid",
            hex::encode(token_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_assets_nft_item_activities_dotbit() {
    let store = test_store();
    let account_id = [0x11u8; 20];
    let owner_a = vec![0x88u8; 32];
    let owner_b = vec![0x77u8; 32];
    let owner_c = vec![0x66u8; 32];
    let mint_tx = vec![0xa2; 32];
    let transfer_tx_1 = vec![0xa1; 32];
    let transfer_tx_2 = vec![0xa4; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &account_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(owner_c.clone()),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 120,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_dotbit_account_outpoint(&mint_tx, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &mint_tx, 0);
    batch.put_dotbit_account_outpoint(&transfer_tx_1, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx_1, 0);
    batch.put_dotbit_account_outpoint(&transfer_tx_2, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx_2, 0);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_a,
            lock_code_hash: vec![0x31; 32],
            lock_hash_type: 1,
            lock_args: vec![0x32; 20],
            type_script_hash: Some(vec![0x33; 32]),
            type_code_hash: Some(vec![0x34; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        300,
        320,
        Some(&transfer_tx_1),
    );
    batch.put_consumed_cell_with_consumer(
        &transfer_tx_1,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_b,
            lock_code_hash: vec![0x41; 32],
            lock_hash_type: 1,
            lock_args: vec![0x42; 20],
            type_script_hash: Some(vec![0x43; 32]),
            type_code_hash: Some(vec![0x44; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        320,
        340,
        Some(&transfer_tx_2),
    );
    batch.put_tx_hash_map(&mint_tx, 300, 0);
    batch.put_tx_index(
        300,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_300,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&transfer_tx_1, 320, 0);
    batch.put_tx_index(
        320,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_320,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&transfer_tx_2, 340, 0);
    batch.put_tx_index(
        340,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_340,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=20",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 340);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
    assert_eq!(json["data"][1]["blockNumber"], 320);
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["blockNumber"], 300);
    assert_eq!(json["data"][2]["actions"][0], "mint");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=1",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 340);
    assert_eq!(json["hasMore"], true);
    let next_cursor = json["nextCursor"]
        .as_str()
        .expect("next cursor for dotbit activities");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=1&cursor={}",
            hex::encode(account_id),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["blockNumber"], 320);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=20&action=transfer",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"][0]["actions"][0], "transfer");
}

#[tokio::test]
async fn test_assets_nft_item_activities_dotbit_recycled_has_burn_history() {
    let store = test_store();
    let account_id = [0x31u8; 20];
    let owner_a = vec![0x21u8; 32];
    let owner_b = vec![0x22u8; 32];
    let mint_tx = vec![0xb1; 32];
    let transfer_tx = vec![0xb2; 32];
    let burn_tx = vec![0xb3; 32];

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_identity(
        &account_id,
        &IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: Some("recycled.bit".to_string()),
            is_live: false,
            created_at_block: 100,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_800_000_000),
                registered_at: None,
                status: None,
            },
        },
    );
    batch.put_dotbit_account_outpoint(&mint_tx, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &mint_tx, 0);
    batch.put_dotbit_account_outpoint(&transfer_tx, 0, &account_id);
    batch.put_dotbit_outpoint_by_account_id(&account_id, &transfer_tx, 0);
    batch.put_consumed_cell_with_consumer(
        &mint_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_a,
            lock_code_hash: vec![0x51; 32],
            lock_hash_type: 1,
            lock_args: vec![0x52; 20],
            type_script_hash: Some(vec![0x53; 32]),
            type_code_hash: Some(vec![0x54; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        100,
        200,
        Some(&transfer_tx),
    );
    batch.put_consumed_cell_with_consumer(
        &transfer_tx,
        0,
        &LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_b,
            lock_code_hash: vec![0x61; 32],
            lock_hash_type: 1,
            lock_args: vec![0x62; 20],
            type_script_hash: Some(vec![0x63; 32]),
            type_code_hash: Some(vec![0x64; 32]),
            type_hash_type: Some(1),
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        },
        200,
        260,
        Some(&burn_tx),
    );
    batch.put_tx_hash_map(&mint_tx, 100, 0);
    batch.put_tx_index(
        100,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_100,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 180,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&transfer_tx, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 220,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&burn_tx, 260, 0);
    batch.put_tx_index(
        260,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_260,
            inputs_count: 1,
            outputs_count: 0,
            fee: 0,
            tx_size: 200,
            cycles: None,
        },
    );
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/assets/identities/dotbit/items/0x{}/activities?limit=20",
            hex::encode(account_id)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["data"].as_array().unwrap().len(), 3);
    assert_eq!(json["data"][0]["blockNumber"], 260);
    assert_eq!(json["data"][0]["actions"][0], "burn");
    assert_eq!(json["data"][1]["actions"][0], "transfer");
    assert_eq!(json["data"][2]["actions"][0], "mint");
}

#[tokio::test]
async fn test_hodl_wave_chart_empty_db() {
    let store = test_store();
    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hodl-wave")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "CKB HODL Wave");
    assert!(json["data"].as_array().unwrap().is_empty());
    assert_eq!(json["series"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn test_hodl_wave_chart_with_data() {
    let store = test_store();

    // Insert test HODL wave data
    store
        .put_hodl_wave(
            "20240115",
            &ckbadger_store::types::DailyHodlWave {
                band_24h: 100_00000000,
                band_1d_1w: 200_00000000,
                band_1w_1m: 300_00000000,
                band_1m_3m: 400_00000000,
                band_3m_6m: 500_00000000,
                band_6m_1y: 600_00000000,
                band_1y_3y: 700_00000000,
                band_gt_3y: 800_00000000,
                holder_count: 42_000,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/charts/hodl-wave")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["title"], "CKB HODL Wave");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["date"], "2024-01-15");
    // Verify holderCount is present
    assert_eq!(data[0]["values"]["holderCount"], "42000");
    // Verify percentage values are present (all should be > 0)
    let v24h: f64 = data[0]["values"]["24h"].as_str().unwrap().parse().unwrap();
    assert!(v24h > 0.0 && v24h < 100.0);
    // Series should have 8 entries
    assert_eq!(json["series"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn test_address_activities_reads_from_store() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x22; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xba; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&tx_hash, 10, 0);
    core_batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
        },
    );
    core_batch.put_block_header(
        10,
        &CachedBlockHeader {
            hash: block_hash.clone(),
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    let actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 10, 0, 100, 0);
    core_batch.put_tx_actions(&actions);
    core_batch.put_addr_tx(&lock_hash, 10, 0, &tx_hash);
    core_batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_address_activities_returns_protocol_metadata() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x24; 32];
    let tx_hash = vec![0xaa; 32];
    let block_hash = vec![0xbb; 32];

    let mut actions = make_test_tx_actions(&lock_hash, &tx_hash, &block_hash, 88, 1, 100, 0);
    actions.protocol_actions = vec![ProtocolAction::new(
        "stablepp",
        "deposit",
        serde_json::json!({
            "hasIntent": true,
            "vaultCount": 2,
        }),
    )];

    let mut batch = StoreBatch::new(core_store.as_ref());
    batch.put_tx_hash_map(&tx_hash, 88, 1);
    batch.put_tx_index(
        88,
        1,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_123,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
        },
    );
    batch.put_block_header(
        88,
        &CachedBlockHeader {
            hash: block_hash,
            timestamp: 1_700_000_123,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_tx_actions(&actions);
    batch.put_addr_tx(&lock_hash, 88, 1, &tx_hash);
    batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 88;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["data"][0]["protocolActions"][0]["protocol"],
        "stablepp"
    );
    assert_eq!(
        json["data"][0]["protocolActions"][0]["metadata"]["hasIntent"],
        true
    );
    assert_eq!(
        json["data"][0]["protocolActions"][0]["metadata"]["vaultCount"],
        2
    );
}

#[tokio::test]
async fn test_address_activities_rejects_unknown_filter() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities?filter=tok",
            "11".repeat(32)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("invalid activity filter"));
}

#[tokio::test]
async fn test_address_activities_return_type_calls_and_support_type_call_filter() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let lock_hash = vec![0x12; 32];
    let tx_hash = vec![0x34; 32];
    let block_hash = vec![0x56; 32];
    let type_code_hash = vec![0x78; 32];
    let type_args = vec![0x9A; 20];
    let expected_script_hash = format!(
        "0x{}",
        hex::encode(compute_script_hash(&type_code_hash, 1, &type_args))
    );

    use ckbadger_store::types::{ParticipantDelta, TAG_TYPE_CALL};
    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 88,
        tx_index: 0,
        timestamp: 1_700_000_888,
        is_cellbase: false,
        protocol_actions: vec![],
        type_calls: vec![TypeCallEntry {
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
        }],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            lock_hash: lock_hash.clone(),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL,
        }],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_actions(&actions);
    core_batch.put_addr_tx(&lock_hash, 88, 0, &tx_hash);
    core_batch.put_tx_hash_map(&tx_hash, 88, 0);
    core_batch.put_tx_index(
        88,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_888_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 120,
            cycles: None,
        },
    );
    core_batch.put_block_header(
        88,
        &CachedBlockHeader {
            hash: block_hash,
            timestamp: 1_700_000_888_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    core_batch.put_script_info(
        &type_code_hash,
        &ScriptInfo {
            code_hash: type_code_hash.clone(),
            hash_type: 1,
            name: Some("RGB++ Lock".to_string()),
            ..Default::default()
        },
    );
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/activities?filter=type_call",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["itemDeltas"].as_array().unwrap().len(), 0);
    let type_calls = data[0]["typeCalls"].as_array().unwrap();
    assert_eq!(type_calls.len(), 1);
    assert_eq!(
        type_calls[0]["typeCodeHash"],
        format!("0x{}", hex::encode(&type_code_hash))
    );
    assert_eq!(type_calls[0]["typeHashType"], "type");
    assert_eq!(
        type_calls[0]["typeArgs"],
        format!("0x{}", hex::encode(&type_args))
    );
    assert_eq!(type_calls[0]["scriptHash"], expected_script_hash);
    assert_eq!(type_calls[0]["scriptName"], "RGB++ Lock");
    let lock_calls = data[0]["lockCalls"].as_array().unwrap();
    assert_eq!(lock_calls.len(), 0);
}

#[tokio::test]
async fn test_latest_activities_return_type_calls() {
    use ckbadger_store::types::{ParticipantDelta, TAG_TYPE_CALL, TAG_DAO};
    let core_store = test_store();
    let append_only_store = test_append_only_store();
    let tx_hash = vec![0x68; 32];
    let block_hash = vec![0x79; 32];
    let type_code_hash = vec![0x46; 32];
    let type_args = vec![0x57; 20];
    let expected_script_hash = format!(
        "0x{}",
        hex::encode(compute_script_hash(&type_code_hash, 1, &type_args))
    );

    let actions = TxActions {
        tx_hash: tx_hash.clone(),
        block_hash: block_hash.clone(),
        block_number: 99,
        tx_index: 1,
        timestamp: 1_700_000_999,
        is_cellbase: false,
        protocol_actions: vec![ProtocolAction::new("dao", "deposit", serde_json::json!({"capacity": 102_00000000i64}))],
        type_calls: vec![TypeCallEntry {
            type_code_hash: type_code_hash.clone(),
            type_hash_type: 1,
            type_args: type_args.clone(),
        }],
        lock_calls: vec![],
        participants: vec![ParticipantDelta {
            lock_hash: vec![0x13; 32],
            ckb_delta: -30000,
            used_delta: 0,
            item_deltas: vec![],
            tags: TAG_TYPE_CALL | TAG_DAO,
        }],
    };

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_script_info(
        &type_code_hash,
        &ScriptInfo {
            code_hash: type_code_hash.clone(),
            hash_type: 1,
            name: Some("RGB++ Lock".to_string()),
            ..Default::default()
        },
    );
    core_batch.put_block_header(
        99,
        &CachedBlockHeader {
            hash: block_hash,
            timestamp: 1_700_000_999,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 2,
        },
    );
    core_batch.put_tx_hash_map(&tx_hash, 99, 1);
    core_batch.put_tx_index(
        99,
        1,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_999,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        },
    );
    core_batch.put_tx_actions(&actions);
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/activities/latest?limit=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let type_calls = items[0]["typeCalls"].as_array().unwrap();
    assert_eq!(type_calls.len(), 1);
    assert_eq!(type_calls[0]["typeHashType"], "type");
    assert_eq!(type_calls[0]["scriptHash"], expected_script_hash);
    assert_eq!(type_calls[0]["scriptName"], "RGB++ Lock");
    let lock_calls = items[0]["lockCalls"].as_array().unwrap();
    assert_eq!(lock_calls.len(), 0);
}

// Global activities cursor pagination test removed: owner-level pagination replaced with TX-level
// in the TxActions model. The /api/v1/activities endpoint now returns TX-level items.

#[tokio::test]
async fn test_global_activities_basic() {
    let store = test_store();
    let tx_hash = vec![0x91; 32];
    let block_hash = vec![0xA1; 32];

    let actions = make_test_tx_actions(&vec![0x11; 32], &tx_hash, &block_hash, 200, 0, 111, 0);

    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        200,
        &CachedBlockHeader {
            hash: block_hash,
            timestamp: 1_700_000_200,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        },
    );
    batch.put_tx_hash_map(&tx_hash, 200, 0);
    batch.put_tx_index(
        200,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_200,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
        },
    );
    batch.put_tx_actions(&actions);
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;

    let request = Request::builder()
        .uri("/api/v1/activities?limit=10")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().expect("data array");
    assert!(!data.is_empty());
    assert_eq!(
        data[0]["txHash"],
        format!("0x{}", hex::encode(&tx_hash))
    );
}

#[tokio::test]
async fn test_dao_deposits_cursor_pagination_descending() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xA1; 32], 0i16, 30i64),
        (vec![0xA2; 32], 0i16, 20i64),
        (vec![0xA3; 32], 0i16, 10i64),
    ];
    for (tx_hash, output_index, block_number) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, output_index),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: block_number,
                lock_script_hash: vec![0x22; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
    let next_cursor = json["nextCursor"].as_str().unwrap();
    assert!(next_cursor.starts_with("0x"));
    assert_eq!(json["hasMore"], true);

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=2&cursor={}",
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 10);
    assert!(json["nextCursor"].is_null());
    assert_eq!(json["hasMore"], false);
}

#[tokio::test]
async fn test_dao_deposits_cursor_pagination_keeps_same_block_rows() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xD1; 32], 0i16, 30i64),
        (vec![0xD2; 32], 1i16, 30i64),
        (vec![0xD3; 32], 2i16, 30i64),
        (vec![0xD4; 32], 0i16, 20i64),
    ];
    for (tx_hash, output_index, block_number) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, output_index),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: block_number,
                lock_script_hash: vec![0x33; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=2")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 30);
    let next_cursor = json["nextCursor"].as_str().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=2&cursor={}",
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
}

#[tokio::test]
async fn test_dao_deposits_status_filter_uses_descending_order() {
    let store = test_store();
    let lock_a = vec![0x11; 32];
    let lock_b = vec![0x22; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xB1; 32], 30i64, 1i16, lock_a.clone()),
        (vec![0xB2; 32], 20i64, 1i16, lock_b.clone()),
        (vec![0xB3; 32], 10i64, 0i16, lock_a.clone()),
    ];
    for (tx_hash, block_number, status, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: block_number,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=10&status=1")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    assert_eq!(rows[1]["depositBlockNumber"], 20);
}

#[tokio::test]
async fn test_dao_deposits_status_cursor_mismatch_returns_bad_request() {
    let store = test_store();
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xE1; 32], 30i64, 0i16, vec![0x11; 32]),
        (vec![0xE2; 32], 20i64, 0i16, vec![0x22; 32]),
        (vec![0xE3; 32], 10i64, 1i16, vec![0x33; 32]),
    ];
    for (tx_hash, block_number, status, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: block_number,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri("/api/v1/dao/deposits?limit=1&status=0")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits?limit=1&status=1&cursor={}",
            cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Invalid dao deposits cursor"));
}

#[tokio::test]
async fn test_dao_deposits_by_lock_hash_cursor_pagination() {
    let store = test_store();
    let lock_a = vec![0x33; 32];
    let lock_b = vec![0x44; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xC1; 32], 30i64, lock_a.clone()),
        (vec![0xC2; 32], 20i64, lock_a.clone()),
        (vec![0xC3; 32], 10i64, lock_b),
    ];
    for (tx_hash, block_number, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: block_number,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1",
            hex::encode(&lock_a)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 30);
    let next_cursor = json["nextCursor"].as_str().unwrap();
    assert!(next_cursor.starts_with("0x"));

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1&cursor={}",
            hex::encode(&lock_a),
            next_cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let rows = json["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["depositBlockNumber"], 20);
    assert!(json["nextCursor"].is_null());
}

#[tokio::test]
async fn test_dao_deposits_by_lock_hash_cursor_mismatch_returns_bad_request() {
    let store = test_store();
    let lock_a = vec![0x55; 32];
    let lock_b = vec![0x66; 32];
    let mut batch = StoreBatch::new(store.as_ref());

    let entries = [
        (vec![0xF1; 32], 30i64, lock_a.clone()),
        (vec![0xF2; 32], 20i64, lock_b.clone()),
        (vec![0xF3; 32], 10i64, lock_b.clone()),
    ];
    for (tx_hash, block_number, lock_hash) in entries {
        batch.put_dao_deposit(
            &ckbadger_store::keys::encode_outpoint(&tx_hash, 0),
            &DaoDepositCacheEntry {
                capacity: 100_00000000,
                deposit_block_number: block_number,
                lock_script_hash: lock_hash,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
    }
    batch.commit().unwrap();

    let app = create_router(test_config(store)).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1",
            hex::encode(&lock_b)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let cursor = json["nextCursor"].as_str().expect("next cursor");

    let request = Request::builder()
        .uri(format!(
            "/api/v1/dao/deposits/0x{}?limit=1&cursor={}",
            hex::encode(&lock_a),
            cursor
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "bad_request");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("Invalid dao deposits by address cursor"));
}

#[tokio::test]
async fn test_address_transactions_reads_from_derived_store() {
    let (core_store, append_only_store) = split_test_stores();
    let lock_hash = vec![0x33; 32];
    let tx_hash = vec![0xab; 32];

    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_tx_hash_map(&tx_hash, 10, 0);
    core_batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 2,
            fee: 1000,
            tx_size: 120,
            cycles: Some(10_000),
        },
    );
    core_batch.commit().unwrap();
    core_store
        .update_sync_status(|s| {
            s.tip_block_number = 10;
        })
        .unwrap();

    let config = test_config_with_append_only(core_store.clone(), append_only_store.clone());
    let app = create_router(config).await;
    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 0);

    let mut derived_batch = StoreBatch::new(append_only_store.as_ref());
    derived_batch.put_addr_tx(&lock_hash, 10, 0, &tx_hash);
    derived_batch.commit().unwrap();

    let request = Request::builder()
        .uri(format!(
            "/api/v1/addresses/0x{}/transactions",
            hex::encode(&lock_hash)
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["txHash"], format!("0x{}", hex::encode(&tx_hash)));
}

#[tokio::test]
async fn test_scripts_list_reads_from_derived_store() {
    let core_store = test_store();
    let append_only_store = test_append_only_store();

    let family_id = "core-only-script";
    let mut core_batch = StoreBatch::new(core_store.as_ref());
    core_batch.put_script_family(
        family_id,
        &ScriptFamilyInfo {
            family_id: family_id.to_string(),
            name: "CoreOnlyScript".to_string(),
            versions_count: 1,
            ..Default::default()
        },
    );
    core_batch.put_script_family_by_name("CoreOnlyScript", family_id);
    core_batch.commit().unwrap();

    let config = test_config_with_append_only(core_store, append_only_store);
    let app = create_router(config).await;
    let request = Request::builder()
        .uri("/api/v1/scripts?limit=20")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["familyId"], family_id);
    assert_eq!(data[0]["name"], "CoreOnlyScript");
}

#[tokio::test]
async fn test_deprecated_script_labels_resolve_by_name_and_api_flag() {
    let store = test_store();
    run_label_import_bundled(store.as_ref(), "mainnet").unwrap();

    let app = create_router(test_config(store)).await;
    let pw_lock_data_hash = "0xd6a5a0edb152e88e8bbc702e164441cb3890fae35da672b408d28ca9a1bde3ee";

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/scripts/lookup")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"codeHashes":["{}"]}}"#,
            pw_lock_data_hash
        )))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json[pw_lock_data_hash]["name"], "PW Lock");
    assert_eq!(json[pw_lock_data_hash]["deprecated"], true);
    assert_eq!(json[pw_lock_data_hash]["resolutionState"], "resolved");

    let request = Request::builder()
        .uri("/api/v1/scripts/PW%20Lock")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = json["versions"].as_array().unwrap();

    assert!(!items.is_empty());
    assert_eq!(json["name"], "PW Lock");
    assert_eq!(items[0]["name"], "PW Lock");
    assert_eq!(items[0]["deprecated"], true);
}

#[tokio::test]
async fn test_asset_ecosystem_returns_expected_structure() {
    let store = test_store();

    // Seed one token so the warmup cache populates CACHE_KEY_ASSETS_TOKEN.
    store
        .put_token_direct(
            &[0xAA; 32],
            &TokenInfo {
                type_code_hash: vec![0x01; 32],
                hash_type: 1,
                type_args: vec![0x02; 20],
                standard: "xudt".to_string(),
                name: Some("TestToken".to_string()),
                symbol: Some("TT".to_string()),
                decimals: Some(8),
                total_supply: Some(1_000_000),
                max_supply: None,
                holders_count: 42,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 10,
            },
        )
        .unwrap();
    store
        .put_token_daily_delta(
            &[0xAA; 32],
            20240101,
            &TokenDailyDelta {
                owned_capacity_delta: 500_00000000,
                owned_knowledge_delta: 300_00000000,
            },
        )
        .unwrap();

    let config = test_config(store);
    let app = create_router(config).await;

    let request = Request::builder()
        .uri("/api/v1/statistics/asset-ecosystem")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify top-level structure (response is the struct directly, not wrapped in "data")
    assert!(json["topTokens"].is_array(), "topTokens should be an array");
    assert!(
        json["capacityBreakdown"].is_array(),
        "capacityBreakdown should be an array"
    );
    assert!(
        json["totalKnowledgeSizeCkb"].is_string(),
        "totalKnowledgeSizeCkb should be a string"
    );

    // Verify seeded token appears in topTokens
    let top_tokens = json["topTokens"].as_array().unwrap();
    assert_eq!(top_tokens.len(), 1);
    assert_eq!(top_tokens[0]["name"], "TestToken");
    assert_eq!(top_tokens[0]["symbol"], "TT");
    assert_eq!(top_tokens[0]["holdersCount"], 42);

    // Verify capacity breakdown has the expected categories
    let breakdown = json["capacityBreakdown"].as_array().unwrap();
    let categories: Vec<&str> = breakdown
        .iter()
        .map(|c| c["category"].as_str().unwrap())
        .collect();
    assert_eq!(categories, vec!["dao", "tokens", "objects", "other"]);
}

#[tokio::test]
async fn test_network_stats_includes_api_background_tasks() {
    let store = test_store();
    let config = test_config(store);

    // Build AppState manually so we can hold a reference to it.
    let state = Arc::new(AppState {
        store: config.store,
        append_only_store: config.append_only_store,
        ws_manager: Arc::new(WsManager::new()),
        cache: CacheBackend::new(),
        ckb_rpc_url: config.ckb_rpc_url,
        ckb_network: config.ckb_network,
        cycles_client: CyclesClient::disabled(),
        ckb_store: None,
        ckb_db_cleanup: config.ckb_db_cleanup,
        mem_cache: InMemoryCache::new(),
        asset_cache_warmup_error: Arc::new(std::sync::RwLock::new(None)),
        background_tasks: Arc::new(std::sync::RwLock::new(Default::default())),
        media_dir: config.media_dir,
    });

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
