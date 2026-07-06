#![allow(dead_code, unused_imports)]

pub use axum::body::Body;
pub use axum::http::{Request, StatusCode};
pub use http_body_util::BodyExt;
pub use rocksdb::{ColumnFamilyDescriptor, Options, DB};
pub use std::sync::Arc;
pub use tower::ServiceExt;
pub use uuid::Uuid;
pub use wiremock::matchers::{body_partial_json, method};
pub use wiremock::{Mock, MockServer, ResponseTemplate};

pub use ckbadger_api::cache::{CacheBackend, InMemoryCache};
pub use ckbadger_api::cycles::CyclesClient;
pub use ckbadger_api::routes::api_routes;
pub use ckbadger_api::utils::address::compute_script_hash;
pub use ckbadger_api::ws::WsManager;
pub use ckbadger_api::{
    create_router, dispatch_initial_warmup, AppConfig, AppState, CleanupPathGuard,
};
pub use ckbadger_common::{BackgroundTaskKind, BackgroundTaskState};
pub use ckbadger_indexer::label_import::run_label_import_bundled;
pub use ckbadger_store::batch::StoreBatch;
pub use ckbadger_store::types::{
    AddrTxValue, AssetAction, CachedBlockHeader, ClusterAggregate, ClusterDailyDelta,
    CompositionTier, DailyBlockStats, DailyStats, DaoDailySnapshot, DaoDepositCacheEntry,
    DeepForkInfo, DobDecodedEntry, DobDecodedTrait, EpochStats, HourlyStats,
    IdentityCollectionAggregate, IdentityEntry, IdentityExtra, IdentityStandard, LiveCellInfo,
    MinerStats, MnftCollectionAggregate, MnftDailyDelta, ObjectCollectionActivityEntry,
    ObjectEntry, ObjectExtra, ObjectStandard, ProtocolAction, ReorgEvent, ScriptDailyDelta,
    ScriptFamilyInfo, ScriptInfo, ScriptReferenceInfo, ScriptVersionInfo, SporeDailyDelta,
    SporeMediaProfile, TokenDailyDelta, TokenInfo, TxActions, TxIndexEntry, TypeCallEntry,
};
pub use ckbadger_store::CkbadgerStore;

pub fn test_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

/// Seed a synced-chain genesis economic baseline into a domain store.
///
/// The indexer always derives this at block 0. Read-only handlers that report
/// genesis-derived economics (circulating supply, and the genesis burn-cell
/// tagging in the cell/tx builders) fail-fast if it is absent, so any test
/// whose endpoint touches those paths must seed it first. Values mirror mainnet
/// genesis: 33.6B issued, 8.4B burnt, 6/10 occupied ratio == 504e15 shannons.
pub fn seed_genesis_baseline(store: &Arc<CkbadgerStore>) {
    store
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance: 3_360_000_000_000_000_000,
            burnt: 840_000_000_000_000_000,
            virtual_occupied: 504_000_000_000_000_000,
        })
        .unwrap();
}

pub fn test_append_only_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

pub fn split_test_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
    let store = test_store();
    (store.clone(), store)
}

pub struct TestCkbDb {
    path: String,
    cleanup: Arc<CleanupPathGuard>,
}

pub fn test_ckb_db_path() -> TestCkbDb {
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

pub fn test_config_with_append_only(
    store: Arc<CkbadgerStore>,
    append_only_store: Arc<CkbadgerStore>,
) -> AppConfig {
    let ckb_db = test_ckb_db_path();
    test_config_with_ckb_db_path(store, append_only_store, ckb_db.path, Some(ckb_db.cleanup))
}

pub fn test_config_with_ckb_db_path(
    store: Arc<CkbadgerStore>,
    append_only_store: Arc<CkbadgerStore>,
    ckb_db_path: String,
    ckb_db_cleanup: Option<Arc<CleanupPathGuard>>,
) -> AppConfig {
    AppConfig {
        append_only_store,
        store,
        network_store: None,
        crawler_enabled: false,
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_network: "mainnet".to_string(),
        rate_limit_per_second: Some(1000),
        rate_limit_burst: Some(2000),
        slow_request_threshold_ms: 100,
        start_background_tasks: false,
        ckb_db_path,
        ckb_db_cleanup,
        dob_decode_dir: std::path::PathBuf::from("/tmp/ckbadger-test-media"),
        cycles_request_dir: None,
    }
}

pub fn test_config(store: Arc<CkbadgerStore>) -> AppConfig {
    test_config_with_append_only(store.clone(), store)
}

/// Build an [`AppConfig`] wired with a seeded network store (for `/network` endpoint tests).
pub fn test_config_with_network(
    store: Arc<CkbadgerStore>,
    network_store: Arc<CkbadgerStore>,
    crawler_enabled: bool,
) -> AppConfig {
    AppConfig {
        network_store: Some(network_store),
        crawler_enabled,
        ..test_config(store)
    }
}

pub fn test_app_state(config: AppConfig) -> Arc<AppState> {
    Arc::new(AppState {
        store: config.store,
        append_only_store: config.append_only_store,
        network_store: config.network_store,
        crawler_enabled: config.crawler_enabled,
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
        dob_decode_dir: config.dob_decode_dir,
        spore_cache: Arc::new(arc_swap::ArcSwap::from_pointee(None)),
        token_cache: Arc::new(arc_swap::ArcSwap::from_pointee(None)),
        object_cache: Arc::new(arc_swap::ArcSwap::from_pointee(None)),
    })
}

pub fn create_router_without_warmup(config: AppConfig) -> axum::Router {
    let state = test_app_state(config);

    axum::Router::new()
        .nest("/api/v1", api_routes())
        .with_state(state)
}

/// Issue a GET against the router and parse the JSON body.
/// `path` is relative to the `/api/v1` mount (e.g. `/network/summary`).
/// Mirrors the inline `oneshot` + `to_bytes` + `from_slice` idiom used across `api_*.rs`.
pub async fn get_json(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .uri(format!("/api/v1{path}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

pub fn compute_blake2b_data_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = ckb_hash::new_blake2b();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    hash.to_vec()
}

pub fn pending_tx_hash_hex() -> String {
    format!("0x{}", "ab".repeat(32))
}

pub fn pending_previous_output_hash_hex() -> String {
    format!("0x{}", "cd".repeat(32))
}

pub fn pending_tx_pool_timestamp_hex() -> &'static str {
    "0x18bcfe5687b"
}

pub fn pending_transaction_rpc_response(hash: &str, status: &str) -> serde_json::Value {
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

pub async fn mount_pending_transaction_rpc(server: &MockServer, hash: &str, status: &str) {
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

pub fn insert_committed_transaction(store: &Arc<CkbadgerStore>, tx_hash: &[u8]) {
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        321,
        &CachedBlockHeader {
            hash: vec![0x44; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_123_456,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
}

/// Create a TxActions with one participant for testing.
pub fn make_test_tx_actions(
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
#[allow(dead_code)]
pub fn make_test_participant(
    lock_byte: u8,
    ckb_delta: i128,
    tags: u16,
) -> ckbadger_store::types::ParticipantDelta {
    ckbadger_store::types::ParticipantDelta {
        lock_hash: vec![lock_byte; 32],
        ckb_delta,
        used_delta: 0,
        item_deltas: vec![],
        tags,
    }
}

/// Open a fresh seeded network store for API tests (a couple of nodes + a status + history).
pub fn test_network_store() -> std::sync::Arc<ckbadger_store::CkbadgerStore> {
    use ckbadger_store::network_keys::{Granularity, Metric};
    use ckbadger_store::{CkbadgerStore, HistoryPoint, LatestStatus, NodeRecord};
    let dir = tempfile::tempdir().expect("tmp");
    let s = std::sync::Arc::new(CkbadgerStore::open_test_network(dir.path()).unwrap());
    std::mem::forget(dir); // keep the temp dir alive for the store's lifetime (mirrors test_store())
    let mut node = NodeRecord {
        own_addrs: vec!["/ip4/1.2.3.4/tcp/8115".into()],
        client_version: "0.119.0".into(),
        flags: 0,
        protocols: vec!["/ckb/discovery".into()],
        first_seen: 100,
        last_seen: 200,
        last_reachable_at: 200,
        reachable: true,
        geo: Some(ckbadger_store::Geo {
            country: "US".into(),
            city: "NYC".into(),
            lat: 0.0,
            lon: 0.0,
        }),
        asn: Some(ckbadger_store::Asn {
            number: 65000,
            org: "Ex".into(),
        }),
        last_rtt_ms: Some(9),
        known_peers: vec![],
    };
    s.put_node(b"peerA", &node).unwrap();
    node.reachable = false;
    node.client_version = "0.118.0".into();
    node.geo = None;
    s.put_node(b"peerB", &node).unwrap();
    s.put_network_status(&LatestStatus {
        round_id: 5,
        reachable: 1,
        unreachable: 1,
        total_known: 2,
        frontier_drained: true,
        ..Default::default()
    })
    .unwrap();
    s.put_history_point(
        Metric::TotalNodes,
        Granularity::Hour,
        10,
        &HistoryPoint {
            scalar: 2,
            buckets: vec![],
        },
    )
    .unwrap();
    s
}

#[test]
fn network_store_helper_seeds_two_nodes() {
    let s = test_network_store();
    assert_eq!(s.scan_nodes().unwrap().len(), 2);
    assert!(s.get_network_status().unwrap().is_some());
}
