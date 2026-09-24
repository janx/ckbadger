//! `entity_capacity_history_matches_chain` — the first chain-derived check.
//!
//! Expected values come from a mock CKB node's own indexer; actual values come
//! from a mock ckbadger typed export. Nothing in this file calls the production
//! writer, parser or protocol registry, which is the whole point: an oracle
//! built from the code under test cannot catch that code being wrong.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ckbadger_indexer::verify::checks::{
    Check, CheckContext, CheckStatus, EntitySelector, ProgressReporter,
};
use ckbadger_indexer::verify::entity_history::EntityCapacityHistoryMatchesChain;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const NODE_VERSION: &str = "0.119.0 (test)";
const GENESIS: &str = "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5";
const SHANNON: i128 = 100_000_000;

/// 2026-09-12 00:00:00 UTC+8 in milliseconds, plus one hour, so the day bucket
/// is unambiguous under the UTC+8 boundary the daily keys use.
fn day_ms(day_index: i64) -> i64 {
    // 2026-09-12T01:00:00+08:00
    1_789_146_000_000 + day_index * 86_400_000
}

fn date_for(day_index: i64) -> u32 {
    let naive = ckbadger_common::block_date_from_ms(day_ms(day_index));
    naive.format("%Y%m%d").to_string().parse().unwrap()
}

fn hex(value: u64) -> String {
    format!("0x{value:x}")
}

fn hash_of(seed: u64) -> String {
    format!("0x{seed:064x}")
}

/// Occupied capacity in shannons: 8 + (33 + lock args) + (33 + type args) + data.
fn occupied(data_len: usize, lock_args_len: usize, type_args_len: usize) -> i128 {
    (8 + 33 + lock_args_len as i128 + 33 + type_args_len as i128 + data_len as i128) * SHANNON
}

/// One token cell's on-chain shape.
#[derive(Clone, Copy)]
struct CellShape {
    capacity_ckb: u64,
    data_len: usize,
    lock_args_len: usize,
}

impl CellShape {
    fn capacity(&self) -> i128 {
        self.capacity_ckb as i128 * SHANNON
    }
    fn occupied(&self) -> i128 {
        occupied(self.data_len, self.lock_args_len, 0)
    }
}

/// A tiny canonical chain: one transaction per block, each either creating the
/// token cell at output 0 or consuming an earlier one at input 0.
struct ChainFixture {
    code_hash: String,
    txs: HashMap<String, Value>,
    headers: HashMap<u64, Value>,
    records: Vec<Value>,
    /// (date, capacity delta, occupied delta) the chain actually implies.
    expected: Vec<(u32, i128, i128)>,
    next_seed: u64,
}

impl ChainFixture {
    fn new(code_hash: &str) -> Self {
        Self {
            code_hash: code_hash.to_string(),
            txs: HashMap::new(),
            headers: HashMap::new(),
            records: Vec::new(),
            expected: Vec::new(),
            next_seed: 1,
        }
    }

    fn type_script(&self) -> Value {
        json!({"code_hash": self.code_hash, "hash_type": "type", "args": "0x"})
    }

    fn header(&mut self, block: u64, day_index: i64) {
        self.headers.entry(block).or_insert_with(|| {
            json!({
                "version": "0x0",
                "compact_target": "0x1a08a97e",
                "timestamp": hex(day_ms(day_index) as u64),
                "number": hex(block),
                "epoch": "0x0",
                "parent_hash": hash_of(0),
                "transactions_root": hash_of(0),
                "proposals_hash": hash_of(0),
                "extra_hash": hash_of(0),
                "dao": format!("0x{}", "00".repeat(32)),
                "nonce": "0x0",
                "hash": format!("0xb{block:063x}"),
            })
        });
    }

    fn record_expected(&mut self, day_index: i64, capacity: i128, occupied: i128) {
        let date = date_for(day_index);
        match self.expected.iter_mut().find(|(d, _, _)| *d == date) {
            Some(entry) => {
                entry.1 += capacity;
                entry.2 += occupied;
            }
            None => self.expected.push((date, capacity, occupied)),
        }
    }

    /// Create the token cell at output 0 of a fresh transaction.
    fn create(&mut self, block: u64, day_index: i64, shape: CellShape) -> String {
        self.header(block, day_index);
        let tx_hash = hash_of(self.next_seed);
        self.next_seed += 1;
        self.txs.insert(
            tx_hash.clone(),
            json!({
                "hash": tx_hash,
                "version": "0x0",
                "cell_deps": [],
                "header_deps": [],
                "inputs": [],
                "outputs": [{
                    "capacity": hex(shape.capacity_ckb * 100_000_000),
                    "lock": {
                        "code_hash": hash_of(0xaa),
                        "hash_type": "type",
                        "args": format!("0x{}", "11".repeat(shape.lock_args_len)),
                    },
                    "type": self.type_script(),
                }],
                "outputs_data": [format!("0x{}", "22".repeat(shape.data_len))],
                "witnesses": [],
            }),
        );
        self.records.push(json!({
            "block_number": hex(block),
            "io_index": "0x0",
            "io_type": "output",
            "tx_hash": tx_hash,
            "tx_index": "0x0",
        }));
        self.record_expected(day_index, shape.capacity(), shape.occupied());
        tx_hash
    }

    /// Consume `prev_tx`'s output 0 at input 0 of a fresh transaction.
    fn consume(&mut self, block: u64, day_index: i64, prev_tx: &str, shape: CellShape) {
        self.header(block, day_index);
        let tx_hash = hash_of(self.next_seed);
        self.next_seed += 1;
        self.txs.insert(
            tx_hash.clone(),
            json!({
                "hash": tx_hash,
                "version": "0x0",
                "cell_deps": [],
                "header_deps": [],
                "inputs": [{
                    "since": "0x0",
                    "previous_output": {"tx_hash": prev_tx, "index": "0x0"},
                }],
                "outputs": [],
                "outputs_data": [],
                "witnesses": [],
            }),
        );
        self.records.push(json!({
            "block_number": hex(block),
            "io_index": "0x0",
            "io_type": "input",
            "tx_hash": tx_hash,
            "tx_index": "0x0",
        }));
        self.record_expected(day_index, -shape.capacity(), -shape.occupied());
    }

    fn daily_rows(&self) -> Vec<(u32, i128, i128)> {
        let mut rows = self.expected.clone();
        rows.sort_by_key(|(date, _, _)| *date);
        rows
    }
}

/// Serve the fixture's `get_transactions` page(s) and point lookups.
struct NodeResponder {
    txs: HashMap<String, Value>,
    headers: HashMap<u64, Value>,
    records: Vec<Value>,
    calls: Arc<Mutex<usize>>,
    /// When set, the block hash at this height changes after the first lookup —
    /// a reorg landing underneath the anchor while the walk is running.
    reorg_at_anchor: Option<u64>,
    anchor_lookups: Arc<Mutex<usize>>,
}

impl Respond for NodeResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let method = body["method"].as_str().unwrap_or_default().to_string();
        let params = &body["params"];
        let result = match method.as_str() {
            "local_node_info" => json!({"version": NODE_VERSION}),
            "get_indexer_tip" => json!({"block_hash": hash_of(0xff), "block_number": "0xffff"}),
            "get_block_hash" => {
                if params[0] == "0x0" {
                    json!(GENESIS)
                } else {
                    let number = u64::from_str_radix(
                        params[0].as_str().unwrap().trim_start_matches("0x"),
                        16,
                    )
                    .unwrap();
                    if self.reorg_at_anchor == Some(number) {
                        let mut seen = self.anchor_lookups.lock().unwrap();
                        *seen += 1;
                        if *seen > 1 {
                            return ResponseTemplate::new(200).set_body_json(json!({
                                "jsonrpc": "2.0", "id": 1,
                                "result": format!("0xdead{number:059x}")
                            }));
                        }
                    }
                    json!(format!("0xb{number:063x}"))
                }
            }
            "get_transactions" => {
                let mut calls = self.calls.lock().unwrap();
                let page = *calls;
                *calls += 1;
                if page == 0 {
                    json!({"objects": self.records, "last_cursor": "0xc1"})
                } else {
                    json!({"objects": [], "last_cursor": "0x"})
                }
            }
            "get_transaction" => {
                let hash = params[0].as_str().unwrap();
                match self.txs.get(hash) {
                    Some(tx) => json!({
                        "transaction": tx,
                        "tx_status": {"status": "committed", "block_hash": hash_of(1), "block_number": "0x1"}
                    }),
                    None => Value::Null,
                }
            }
            "get_header_by_number" => {
                let number =
                    u64::from_str_radix(params[0].as_str().unwrap().trim_start_matches("0x"), 16)
                        .unwrap();
                self.headers.get(&number).cloned().unwrap_or(Value::Null)
            }
            other => panic!("unexpected RPC method {other}"),
        };
        ResponseTemplate::new(200)
            .set_body_json(json!({"jsonrpc": "2.0", "id": 1, "result": result}))
    }
}

async fn mock_node(fixture: &ChainFixture) -> MockServer {
    mock_node_with_reorg(fixture, None).await
}

async fn mock_node_with_reorg(fixture: &ChainFixture, reorg_at_anchor: Option<u64>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(NodeResponder {
            txs: fixture.txs.clone(),
            headers: fixture.headers.clone(),
            records: fixture.records.clone(),
            calls: Arc::new(Mutex::new(0)),
            reorg_at_anchor,
            anchor_lookups: Arc::new(Mutex::new(0)),
        })
        .mount(&server)
        .await;
    server
}

/// Build the typed export body for one token.
///
/// `current` is what the *index* reports as live now. It defaults to the
/// accumulation of the rows, which is what the real endpoint publishes; a test
/// can override it to model an index whose current value disagrees with the
/// chain's live cell set.
fn export_body(
    type_hash: &str,
    code_hash: &str,
    rows: &[(u32, i128, i128)],
    anchor: u64,
    complete: bool,
    current: Option<(i128, i128)>,
) -> Value {
    let (current_capacity, current_knowledge) = current.unwrap_or((
        rows.iter().map(|(_, c, _)| c).sum(),
        rows.iter().map(|(_, _, k)| k).sum(),
    ));
    json!({
        "anchor": {"blockNumber": anchor, "blockHash": format!("0xb{anchor:063x}")},
        "state": {
            "bulkSessionInProgress": false,
            "rollbackCleanupInProgress": false,
            "liveCellSummaryInitialized": true,
            "deepForkDetected": false,
            "entityStatsUndoContract": null,
            "hourlyRetention": "unknown"
        },
        "complete": complete,
        "entities": [{
            "kind": "token",
            "id": type_hash,
            "present": true,
            "rowCount": rows.len(),
            "typeScript": {"codeHash": code_hash, "hashType": "type", "args": "0x"},
            "complete": complete,
            "currentCapacity": current_capacity.to_string(),
            "currentKnowledge": current_knowledge.to_string(),
            "daily": rows.iter().map(|(date, capacity, knowledge)| json!({
                "date": date,
                "capacityDelta": capacity.to_string(),
                "knowledgeDelta": knowledge.to_string(),
            })).collect::<Vec<_>>()
        }]
    })
}

async fn mock_api(
    type_hash: &str,
    code_hash: &str,
    rows: &[(u32, i128, i128)],
    anchor: u64,
    complete: bool,
) -> MockServer {
    mock_api_with_current(type_hash, code_hash, rows, anchor, complete, None).await
}

async fn mock_api_with_current(
    type_hash: &str,
    code_hash: &str,
    rows: &[(u32, i128, i128)],
    anchor: u64,
    complete: bool,
    current: Option<(i128, i128)>,
) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/verify/entity-statistics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(export_body(
            type_hash, code_hash, rows, anchor, complete, current,
        )))
        .mount(&server)
        .await;
    // Deliberately NOT mounting /api/v1/tokens/{hash}: the check must build its
    // chain query from the export alone. On the real testnet store that
    // endpoint 500s on this very token, because it accumulates the corrupt rows
    // the check exists to investigate.
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/tokens/{type_hash}")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": "internal_error",
            "message": "owned capacity underflow while accumulating owned capacity",
        })))
        .mount(&server)
        .await;
    server
}

fn declaration(dir: &std::path::Path, node_version: &str) -> std::path::PathBuf {
    let path = dir.join("verify-source.toml");
    std::fs::write(
        &path,
        format!(
            r#"genesisHash = "{GENESIS}"
nodeVersion = "{node_version}"
indexerVersion = "{node_version}"
buildStartBlock = 0
continuousFromGenesis = true
provenance = "test fixture"
"#
        ),
    )
    .unwrap();
    path
}

/// Everything the check needs, as plain owned data.
///
/// The `CheckContext` itself is built inside the blocking worker: constructing
/// a `reqwest::blocking::Client` spins up and drops a temporary runtime, which
/// panics if it happens inside an async test body.
#[derive(Clone)]
struct Wiring {
    api_url: String,
    rpc_url: String,
    declaration_path: std::path::PathBuf,
    network: &'static str,
    /// `--entity` selectors; empty is the default (no `--entity`) mode.
    entities: Vec<EntitySelector>,
    /// Where the run writes `manifest.json`; `None` writes nothing.
    evidence_dir: Option<std::path::PathBuf>,
}

fn wiring(
    api: &MockServer,
    node: &MockServer,
    declaration_path: &std::path::Path,
    type_hash: &str,
) -> Wiring {
    Wiring {
        api_url: format!("{}/api/v1", api.uri()),
        rpc_url: node.uri(),
        declaration_path: declaration_path.to_path_buf(),
        network: "mainnet",
        entities: vec![EntitySelector {
            kind: "token".to_string(),
            id: type_hash.to_string(),
        }],
        evidence_dir: None,
    }
}

fn context(wiring: &Wiring) -> CheckContext {
    CheckContext {
        network: wiring.network,
        api_url: wiring.api_url.clone(),
        rpc_url: Some(wiring.rpc_url.clone()),
        explorer_url: None,
        http: reqwest::blocking::Client::new(),
        sample_count: 10,
        seed: 42,
        tolerance: 0.001,
        cache_dir: None,
        entities: wiring.entities.clone(),
        verify_source_path: Some(wiring.declaration_path.clone()),
        evidence_dir: wiring.evidence_dir.clone(),
        entity_budget: Default::default(),
        source_profile: std::sync::Mutex::new(None),
    }
}

/// The script hash of the fixture's type script, computed independently.
fn type_hash_of(code_hash: &str) -> String {
    use ckb_types::prelude::*;
    let code: [u8; 32] = hex::decode(code_hash.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let script = ckb_types::packed::Script::new_builder()
        .code_hash(code.pack())
        .hash_type(ckb_types::packed::Byte::new(1))
        .args(Vec::<u8>::new().pack())
        .build();
    let bytes: [u8; 32] = script.calc_script_hash().unpack();
    format!("0x{}", hex::encode(bytes))
}

fn shape(capacity_ckb: u64, data_len: usize, lock_args_len: usize) -> CellShape {
    CellShape {
        capacity_ckb,
        data_len,
        lock_args_len,
    }
}

/// Three creations and one consumption across three UTC+8 days.
fn standard_fixture(code_hash: &str) -> ChainFixture {
    let mut fixture = ChainFixture::new(code_hash);
    let a = shape(1_000, 16, 20);
    let b = shape(2_500, 32, 20);
    let first = fixture.create(10, 0, a);
    fixture.create(20, 1, b);
    fixture.consume(30, 2, &first, a);
    fixture
}

/// Run the check on a blocking worker, building its context there.
async fn run_check(wiring: Wiring) -> ckbadger_indexer::verify::checks::CheckResult {
    tokio::task::spawn_blocking(move || {
        EntityCapacityHistoryMatchesChain
            .run(&context(&wiring), &ProgressReporter::new(None))
            .expect("the check itself must not error on a healthy fixture")
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_index_that_matches_the_chain_passes() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(
        result.status,
        CheckStatus::Pass,
        "findings: {:?}",
        result.findings
    );
    assert_eq!(result.items_checked, 1, "items are entities, not rows");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hundred_shannons_missing_from_one_day_fails_on_the_daily_facet() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let mut rows = fixture.daily_rows();
    rows[0].1 -= 100;

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let expected_date = rows[0].0;
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(result.status, CheckStatus::Fail);
    assert_eq!(result.items_failed, 1, "one entity failed, not one row");
    let details = result.findings[0].details.join("\n");
    assert!(details.contains("daily"), "{details}");
    assert!(details.contains("capacity"), "{details}");
    assert!(details.contains(&expected_date.to_string()), "{details}");
    assert!(details.contains("100"), "{details}");
}

/// The index's current value is its own answer to "what is live now", so it
/// can be wrong while every daily row is right — a broken accumulation, or a
/// current value computed from somewhere else entirely. Only a facet compared
/// against the chain's live cell set can catch that.
#[tokio::test(flavor = "multi_thread")]
async fn a_stored_current_value_that_disagrees_with_the_live_set_fails_on_the_current_facet() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();
    let live_capacity: i128 = rows.iter().map(|(_, c, _)| c).sum();
    let live_knowledge: i128 = rows.iter().map(|(_, _, k)| k).sum();

    let node = mock_node(&fixture).await;
    // Every day agrees; only the reported current total is off.
    let api = mock_api_with_current(
        &type_hash,
        &code_hash,
        &rows,
        100,
        true,
        Some((live_capacity + 7, live_knowledge)),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);

    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;

    assert_eq!(result.status, CheckStatus::Fail);
    let details = result.findings[0].details.join("\n");
    assert!(details.contains("current"), "{details}");
    assert!(details.contains("capacity"), "{details}");
    assert!(
        !details.contains("daily "),
        "no day differs; only the index's current value does: {details}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_occupied_capacity_fails_on_the_knowledge_facet() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let mut rows = fixture.daily_rows();
    rows[1].2 += SHANNON;

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(result.status, CheckStatus::Fail);
    let details = result.findings[0].details.join("\n");
    assert!(details.contains("knowledge"), "{details}");
}

/// The classic silent corruption: two days wrong in opposite directions, so the
/// current total still agrees. Only the per-day facet can catch it.
#[tokio::test(flavor = "multi_thread")]
async fn offsetting_daily_errors_with_a_correct_total_still_fail() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let mut rows = fixture.daily_rows();
    rows[0].1 += 100;
    rows[1].1 -= 100;

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(
        result.status,
        CheckStatus::Fail,
        "a matching current total is not evidence the history is right"
    );
    let details = result.findings[0].details.join("\n");
    assert!(details.contains("daily"), "{details}");
}

/// The iCKB shape: whole days deleted by a shallow-fork rollback. The check must
/// name the missing days, not just report a total that is off.
#[tokio::test(flavor = "multi_thread")]
async fn whole_days_missing_from_the_index_are_named() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let mut fixture = ChainFixture::new(&code_hash);
    let a = shape(1_000, 16, 20);
    for day in 0..5 {
        fixture.create(10 + day as u64, day, a);
    }
    let all_rows = fixture.daily_rows();
    // Days 2, 3 and 4 were wiped by the rollback, exactly as iCKB's were.
    let rows: Vec<(u32, i128, i128)> = all_rows[..2].to_vec();
    let missing: Vec<u32> = all_rows[2..].iter().map(|(date, _, _)| *date).collect();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(result.status, CheckStatus::Fail);
    let details = result.findings[0].details.join("\n");
    for date in &missing {
        assert!(
            details.contains(&date.to_string()),
            "missing day {date} must be named: {details}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_source_that_does_not_qualify_is_inconclusive_not_a_failure() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let mut rows = fixture.daily_rows();
    // Deliberately wrong numbers: an unqualified source must not turn them into
    // a confident Fail.
    rows[0].1 -= 100;

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), "0.118.0-different");
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(result.status, CheckStatus::Inconclusive);
    let detail = result.detail.clone().unwrap_or_default();
    assert!(detail.contains("0.118.0-different"), "{detail}");
}

/// A reorg landing under the anchor while the walk runs makes the node
/// enumerate one chain while the export describes another. Comparing the two
/// would produce a confident `Fail` with exact numbers and exit 1, blaming
/// ckbadger for a chain that moved.
#[tokio::test(flavor = "multi_thread")]
async fn an_anchor_that_moves_during_the_walk_is_inconclusive_not_a_failure() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    // Deliberately wrong rows: a moved anchor must outrank them.
    let mut rows = fixture.daily_rows();
    rows[0].1 -= 100;

    let node = mock_node_with_reorg(&fixture, Some(100)).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);

    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;

    assert_eq!(
        result.status,
        CheckStatus::Inconclusive,
        "the chain moved under the case; its numbers prove nothing"
    );
    assert!(
        result.findings.is_empty(),
        "no confident finding may survive a moved anchor: {:?}",
        result.findings
    );
    let detail = result.detail.clone().unwrap_or_default();
    assert!(detail.contains("anchor"), "{detail}");
    assert!(detail.contains("during the walk"), "{detail}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_incomplete_export_is_inconclusive() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, false).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(
        result.status,
        CheckStatus::Inconclusive,
        "a truncated export cannot be compared against a full history"
    );
}

/// Independence from the production protocol registry: the expectation is built
/// from chain records and the selector alone, so a code hash the registry has
/// never heard of produces exactly the same numbers.
#[tokio::test(flavor = "multi_thread")]
async fn expected_values_do_not_depend_on_the_protocol_registry() {
    let unregistered = hash_of(0x9999_9999);
    let type_hash = type_hash_of(&unregistered);
    let fixture = standard_fixture(&unregistered);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &unregistered, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let result = run_check(wiring(&api, &node, &declaration_path, &type_hash)).await;
    assert_eq!(
        result.status,
        CheckStatus::Pass,
        "the oracle must not consult PROTOCOL_REGISTRY: {:?}",
        result.findings
    );
}

/// Review #7: the default (no `--entity`) mode reads the head of the API's
/// token directory, which `GET /tokens` serves as a `CursorPaginatedResponse`
/// envelope. Deserializing it as a bare array failed on every run, so the
/// default mode could never end anything but Inconclusive.
#[tokio::test(flavor = "multi_thread")]
async fn default_run_reads_the_token_directory_envelope() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tokens"))
        .and(query_param("limit", "8"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"typeScriptHash": type_hash, "name": "Fixture Token"}],
            "limit": 8,
            "hasMore": false,
            "nextCursor": null,
        })))
        .mount(&api)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);

    // A network with no incident selectors: the directory is the only source
    // of candidates, so a directory that cannot be read leaves nothing.
    let wiring = Wiring {
        network: "devnet",
        entities: vec![],
        ..wiring(&api, &node, &declaration_path, &type_hash)
    };
    let result = run_check(wiring).await;

    assert_eq!(
        result.status,
        CheckStatus::Pass,
        "detail: {:?}, findings: {:?}",
        result.detail,
        result.findings
    );
    assert_eq!(result.items_checked, 1);
    let detail = result.detail.clone().unwrap_or_default();
    assert!(
        !detail.contains("token directory could not be listed"),
        "no candidate gap may be reported: {detail}"
    );
}

/// Review #12: a requested selector of a family this delivery does not cover
/// used to be dropped silently whenever a token selector was also present, so
/// the run could end Pass (exit 0) with a manifest that never mentioned it.
#[tokio::test(flavor = "multi_thread")]
async fn token_plus_spore_selectors_end_inconclusive_and_name_the_spore_one() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let evidence = dir.path().join("evidence");
    let spore = EntitySelector {
        kind: "spore".to_string(),
        id: hash_of(0xbb),
    };

    let mut wiring = wiring(&api, &node, &declaration_path, &type_hash);
    wiring.entities.push(spore.clone());
    wiring.evidence_dir = Some(evidence.clone());
    let result = run_check(wiring).await;

    assert_eq!(
        result.status,
        CheckStatus::Inconclusive,
        "a dropped selector can never leave the run green: {:?}",
        result.detail
    );
    let detail = result.detail.clone().unwrap_or_default();
    assert!(detail.contains(&spore.to_string()), "{detail}");
    assert!(detail.contains("family not covered"), "{detail}");

    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(evidence.join("manifest.json")).unwrap())
            .unwrap();
    let entities = manifest["entities"].as_array().unwrap();
    let spore_entry = entities
        .iter()
        .find(|entry| entry["selector"]["kind"] == "spore")
        .unwrap_or_else(|| panic!("the manifest must list the uncovered selector: {manifest}"));
    assert_eq!(spore_entry["selector"]["id"], spore.id);
    assert_eq!(spore_entry["complete"], false);
    assert!(spore_entry["uncoveredReason"]
        .as_str()
        .unwrap()
        .contains("family not covered"));
    let token_entry = entities
        .iter()
        .find(|entry| entry["selector"]["kind"] == "token")
        .expect("the token entity is still verified");
    assert_eq!(token_entry["complete"], true);
}

/// The manifest's `rpcRequests` is what the run spent against its RPC budget.
/// Every node call — source qualification and the post-walk anchor
/// re-verification included — must be charged, or the budget under-reports
/// the spend it exists to bound.
#[tokio::test(flavor = "multi_thread")]
async fn rpc_budget_spend_equals_the_requests_the_node_received() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);
    let evidence = dir.path().join("evidence");

    let mut wiring = wiring(&api, &node, &declaration_path, &type_hash);
    wiring.evidence_dir = Some(evidence.clone());
    let result = run_check(wiring).await;
    assert_eq!(result.status, CheckStatus::Pass, "{:?}", result.detail);

    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(evidence.join("manifest.json")).unwrap())
            .unwrap();
    let received = node.received_requests().await.unwrap().len();
    assert!(received > 0);
    assert_eq!(
        manifest["rpcRequests"].as_u64().unwrap() as usize,
        received,
        "the budget must charge exactly one request per RPC the node received"
    );
}

/// `--entity` ids are hex; a bare or upper-case id names the same entity as its
/// canonical `0x` + lowercase form, and the export is keyed by the latter.
#[tokio::test(flavor = "multi_thread")]
async fn a_bare_uppercase_entity_id_selects_the_same_entity() {
    let code_hash = hash_of(0xc0de);
    let type_hash = type_hash_of(&code_hash);
    let fixture = standard_fixture(&code_hash);
    let rows = fixture.daily_rows();

    let node = mock_node(&fixture).await;
    let api = mock_api(&type_hash, &code_hash, &rows, 100, true).await;
    let dir = tempfile::tempdir().unwrap();
    let declaration_path = declaration(dir.path(), NODE_VERSION);

    let bare = format!(
        "token:{}",
        type_hash.trim_start_matches("0x").to_uppercase()
    );
    let selector = EntitySelector::parse(&bare).unwrap();
    assert_eq!(
        selector,
        EntitySelector::parse(&format!("token:{type_hash}")).unwrap()
    );

    let mut wiring = wiring(&api, &node, &declaration_path, &type_hash);
    wiring.entities = vec![selector];
    let result = run_check(wiring).await;
    assert_eq!(
        result.status,
        CheckStatus::Pass,
        "{:?} {:?}",
        result.detail,
        result.findings
    );
}

#[test]
fn an_entity_selector_names_its_family_and_id() {
    let id = hash_of(7);
    let selector = EntitySelector::parse(&format!("token:{id}")).unwrap();
    assert_eq!(selector.kind, "token");
    assert_eq!(selector.id, id);

    assert!(
        EntitySelector::parse(&id).is_err(),
        "an id with no family is ambiguous"
    );
    assert!(EntitySelector::parse("token:").is_err());
}
