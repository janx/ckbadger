//! Producer/consumer coverage for the recorded-calendar verification contract.
//! The real API supplies indexed dates; the mock node supplies independent cells.

use crate::common::*;
use ckbadger_indexer::verify::{
    checks::{Check, CheckContext, EntitySelector, ProgressReporter},
    entity_history::{EntityBudget, EntityCapacityHistoryMatchesChain},
};
use serde_json::{json, Value};
use std::collections::HashMap;
use wiremock::{Request as MockRequest, Respond};

const SHANNON: i128 = 100_000_000;
const VERSION: &str = "0.210.0 (calendar fixture)";

fn hash(byte: u8) -> String {
    format!("0x{}", hex::encode([byte; 32]))
}

struct CellHistoryNode {
    transactions: HashMap<String, Value>,
    records: Vec<Value>,
}

impl Respond for CellHistoryNode {
    fn respond(&self, request: &MockRequest) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let params = &body["params"];
        let result = match body["method"].as_str().unwrap() {
            "local_node_info" => json!({"version": VERSION}),
            "get_indexer_tip" => json!({"block_number":"0x4", "block_hash":hash(4)}),
            "get_block_hash" => match params[0].as_str().unwrap() {
                "0x0" => json!(hash(0)),
                "0x4" => json!(hash(4)),
                other => panic!("unexpected block hash lookup {other}"),
            },
            "get_transactions" => {
                let records = if params.get(3).is_none_or(Value::is_null) {
                    self.records.clone()
                } else {
                    Vec::new()
                };
                json!({"objects":records, "last_cursor":"0x01"})
            }
            "get_transaction" => json!({
                "transaction": self.transactions[params[0].as_str().unwrap()],
                "tx_status": {"status":"committed", "block_hash":hash(4), "block_number":"0x4"}
            }),
            other => panic!("unexpected RPC {other}; calendar verification must not fetch headers"),
        };
        ResponseTemplate::new(200)
            .set_body_json(json!({"jsonrpc":"2.0", "id":body["id"], "result":result}))
    }
}

/// A quiet block advances the date; later token events have backward raw time.
/// Exact amounts must use the API's recorded calendar, and the result must say so.
#[tokio::test]
async fn calendar_contract_real_api_and_verifier_keep_amounts_exact_and_dates_explicit() {
    use ckb_types::prelude::*;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path().join("store")).unwrap());
    let midnight = utc_ms("2026-10-23T16:00:00Z");
    seed_block_headers_at(
        &store,
        &[
            (0, midnight - 2_000),
            (1, midnight - 1_000),
            (2, midnight + 1_000), // No token event here.
            (3, midnight - 3_000),
            (4, midnight - 2_000),
        ],
    );
    let mut sync = store.get_sync_status().unwrap();
    sync.tip_block_number = 4;
    sync.tip_block_hash = vec![4; 32];
    store.set_sync_status(&sync).unwrap();

    let script = ckb_types::packed::Script::new_builder()
        .code_hash([0xa1u8; 32].pack())
        .hash_type(ckb_types::packed::Byte::new(1))
        .args([1u8].pack())
        .build();
    let type_hash: [u8; 32] = script.calc_script_hash().unpack();
    let token_id = format!("0x{}", hex::encode(type_hash));
    let type_script = json!({"code_hash":hash(0xa1), "hash_type":"type", "args":"0x01"});
    let mut batch = StoreBatch::new(&store);
    batch.put_token(
        &type_hash,
        &TokenInfo {
            type_code_hash: vec![0xa1; 32],
            hash_type: 1,
            type_args: vec![1],
            standard: "xudt".into(),
            name: None,
            symbol: None,
            decimals: Some(8),
            max_supply: None,
            first_seen_block: 1,
            icon_url: None,
            description: None,
            transfers_count: 0,
        },
    );
    batch.commit().unwrap();
    // Occupied bytes = capacity 8 + lock 33 + type (33 + 1) + data 16.
    for (date, capacity, knowledge) in [(20261023, 200, 91), (20261024, 100, 0)] {
        store
            .put_token_daily_delta(
                &type_hash,
                date,
                &TokenDailyDelta {
                    owned_capacity_delta: capacity * SHANNON,
                    owned_knowledge_delta: knowledge * SHANNON,
                },
            )
            .unwrap();
    }

    let mut transactions = HashMap::new();
    for (id, capacity) in [(0x11, 200u64), (0x13, 300u64)] {
        transactions.insert(
            hash(id),
            json!({
                "hash":hash(id), "version":"0x0", "cell_deps":[], "header_deps":[],
                "inputs":[], "witnesses":[],
                "outputs":[{
                    "capacity":format!("0x{:x}", capacity * 100_000_000),
                    "lock":{"code_hash":hash(0xaa), "hash_type":"type", "args":"0x"},
                    "type":type_script
                }],
                "outputs_data":[format!("0x{}", "00".repeat(16))]
            }),
        );
    }
    transactions.insert(
        hash(0x14),
        json!({
            "hash":hash(0x14), "version":"0x0", "cell_deps":[], "header_deps":[],
            "inputs":[{"since":"0x0", "previous_output":{"tx_hash":hash(0x11), "index":"0x0"}}],
            "outputs":[], "outputs_data":[], "witnesses":[]
        }),
    );
    let node = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(CellHistoryNode {
            transactions,
            records: [(1, 0x11, "output"), (3, 0x13, "output"), (4, 0x14, "input")]
                .into_iter()
                .map(|(block, id, io)| {
                    json!({
                        "block_number":format!("0x{block:x}"), "io_index":"0x0",
                        "io_type":io, "tx_hash":hash(id), "tx_index":"0x0"
                    })
                })
                .collect(),
        })
        .mount(&node)
        .await;
    let source_path = dir.path().join("verify-source.toml");
    std::fs::write(
        &source_path,
        format!(
            "genesisHash = \"{}\"\nnodeVersion = \"{VERSION}\"\nindexerVersion = \"{VERSION}\"\nbuildStartBlock = 0\ncontinuousFromGenesis = true\nprovenance = \"fixture\"\n",
            hash(0)
        ),
    )
    .unwrap();
    let app = create_router_without_warmup(test_config(store.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let rpc_url = node.uri();
    let evidence_dir = dir.path().join("evidence");
    let evidence = evidence_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        let ctx = CheckContext {
            network: "mainnet",
            api_url: format!("http://{address}/api/v1"),
            rpc_url: Some(rpc_url),
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 1,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: vec![EntitySelector {
                kind: "token".into(),
                id: token_id,
            }],
            verify_source_path: Some(source_path),
            evidence_dir: Some(evidence),
            entity_budget: EntityBudget {
                max_rpc_requests: 12,
                max_records: 10,
                wall_seconds: 30,
            },
            source_profile: std::sync::Mutex::new(None),
        };
        EntityCapacityHistoryMatchesChain.run(&ctx, &ProgressReporter::new(None))
    })
    .await
    .unwrap();
    server.abort();
    let _ = server.await;
    let result = result.unwrap();
    assert!(result.passed(), "{result:?}");
    assert_eq!(result.items_checked, 1);
    let detail = result.detail.unwrap();
    assert!(detail.contains("calendar"), "{detail}");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(evidence_dir.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["calendarIndependentlyVerified"], false);
    assert_eq!(manifest["calendarSource"], "indexed_chain_time");
    assert_eq!(manifest["historyRecords"], 3);
    let requests = node.received_requests().await.unwrap();
    assert_eq!(manifest["rpcRequests"], requests.len());
    assert!(requests.len() <= 12);
    for request in requests {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_ne!(body["method"], "get_header_by_number");
    }
}
