mod common;
use common::*;

#[tokio::test]
async fn test_transaction_detail_returns_pending_mempool_transaction() {
    let store = test_store();
    // The tx-output builder reads `baseline.virtual_occupied` once per request
    // (fail-fast if absent), so a synced-chain baseline must be present.
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    // The store is empty: the input can only come from the node, as output 0
    // of the transaction that created it.
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

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
    assert_eq!(json["poolStatus"], "pending");
    assert_eq!(json["interpretation"]["status"], "complete");
    assert!(json["pendingSince"].as_str().is_some());
    assert_eq!(json["blockNumber"], serde_json::Value::Null);
    assert_eq!(json["blockHash"], serde_json::Value::Null);
    assert_eq!(json["index"], serde_json::Value::Null);
    assert_eq!(json["confirmations"], serde_json::Value::Null);
    assert_eq!(json["timestamp"], serde_json::Value::Null);
    assert_eq!(json["inputsCount"], 1);
    assert_eq!(json["outputsCount"], 1);
    assert_eq!(json["fee"], "372");
    // Resolved from the node, so the input side is now reported exactly:
    // 100_000_000_000 output + 372 fee, and 8 + 32 + 1 + 20 occupied bytes.
    assert_eq!(json["inputsCapacity"], "100000000372");
    assert_eq!(
        json["inputsCommonKnowledgeSize"],
        serde_json::Value::from("61")
    );
    assert_eq!(
        json["outputsCommonKnowledgeSize"],
        serde_json::Value::from("61")
    );
    assert_eq!(json["inputs"][0]["capacity"], "100000000372");
    assert_eq!(
        json["inputs"][0]["lock"]["codeHash"],
        TEST_SECP_LOCK_CODE_HASH
    );
    assert!(
        json["inputs"][0]["address"]
            .as_str()
            .unwrap()
            .starts_with("ckb1"),
        "a resolved input must carry its owner's address: {:?}",
        json["inputs"][0]["address"]
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
    // This output is at block 0 but its lock args are not the Satoshi dead
    // address, so it must NOT be tagged as a genesis special burn cell.
    assert_eq!(json["outputs"][0]["cellType"], serde_json::Value::Null);
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        serde_json::Value::Null
    );
}

/// A POOL transaction paying the Satoshi dead-address pubkey hash is not the
/// genesis burn cell: `genesis_special_burn` (and its `virtualUsedCapacity`)
/// describe block 0's cells only. This test used to assert the opposite — the
/// pool branch passed block number 0 for every uncommitted transaction.
#[tokio::test]
async fn pending_tx_paying_the_satoshi_address_is_not_a_genesis_burn() {
    let store = test_store();
    // Seed the synced-chain baseline; `seed_genesis_baseline` uses mainnet's
    // 8.4B burnt * 6/10 == 504e15 shannons, the value asserted below.
    seed_genesis_baseline(&store);

    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let satoshi_args = format!(
        "0x{}",
        hex::encode(ckbadger_common::dao::SATOSHI_PUBKEY_HASH)
    );
    let rpc_response = serde_json::json!({
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
                            "args": satoshi_args
                        },
                        "type": null
                    }
                ],
                "outputs_data": ["0x"],
                "witnesses": ["0x"]
            },
            "cycles": "0x5208",
            "fee": "0x174",
            "time_added_to_pool": pending_tx_pool_timestamp_hex(),
            "min_replace_fee": "0x175",
            "tx_status": {
                "status": "pending",
                "block_hash": null,
                "block_number": null,
                "reason": null
            }
        }
    });
    mount_transaction_rpc(&server, rpc_response).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

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
    assert_eq!(json["outputs"][0]["cellType"], serde_json::Value::Null);
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        serde_json::Value::Null
    );
}

/// The genesis block's cellbase, committed but not yet indexed (the store is
/// empty): its Satoshi output IS the genesis burn cell and carries the
/// network's `baseline.virtual_occupied`; its pseudo-input spends no cell, so
/// it is served (fee 0) rather than reported as unresolved.
#[tokio::test]
async fn committed_unindexed_genesis_cellbase_tags_the_satoshi_output() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let cellbase_input = format!("0x{}", "00".repeat(32));
    let mut response = transaction_rpc_response(
        &hash,
        &[(&cellbase_input, u32::MAX)],
        vec![serde_json::json!({
            "capacity": "0x174876e800",
            "lock": {
                "code_hash": format!("0x{}", "11".repeat(32)),
                "hash_type": "type",
                "args": format!("0x{}", hex::encode(ckbadger_common::dao::SATOSHI_PUBKEY_HASH))
            },
            "type": null
        })],
        vec!["0x".to_string()],
        "committed",
        Some((0, &format!("0x{}", "92".repeat(32)))),
    );
    response["result"]["cycles"] = serde_json::json!("0x0");
    mount_transaction_rpc(&server, response).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["isCellbase"], true);
    assert_eq!(json["blockNumber"], 0);
    assert_eq!(json["fee"], "0");
    assert_eq!(json["interpretation"]["status"], "complete");
    assert_eq!(json["outputs"][0]["cellType"], "genesis_special_burn");
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        "504000000000000000"
    );
}

/// A pool transaction paying a `data2` lock (any VM2 script) is attributed to
/// that lock's address, and `/tx` renders the lock with the label the node
/// used. The mirror once mapped `data2` to byte 3 — CKB's wire value is 4 — so
/// it hashed the lock into a script hash no address has: the payee's page
/// showed nothing pending and `/tx` 500'd on the unknown byte.
#[tokio::test]
async fn pending_tx_with_data2_lock_is_attributed_to_its_address() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();

    let mut response_json = pending_transaction_rpc_response(&hash, "pending");
    response_json["result"]["transaction"]["outputs"][0]["lock"]["hash_type"] =
        serde_json::json!("data2");
    mount_transaction_rpc(&server, response_json).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;
    mount_tx_pool_rpc(&server, &[(&hash, pending_tx_pool_timestamp_hex())]).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let state = test_app_state(config);
    let outcome = refresh_pool_mirror_once(&state, &server.uri()).await;
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.entry_errors, 0, "{outcome:?}");
    let app = create_router_with_state(state).await;

    let payee = ckbadger_common::script_to_address(&[0x11; 32], 4, &[0x22; 20], "mainnet")
        .expect("data2 is a valid full-address hash_type");
    let (status, json) = get_json(&app, &format!("/addresses/{payee}/activities")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    let rows = json["data"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        1,
        "the data2 payee must see its pending tx: {json:?}"
    );
    assert_eq!(rows[0]["txHash"], hash);
    assert_eq!(rows[0]["poolStatus"], "pending");

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["outputs"][0]["lock"]["hashType"], "data2");
    assert_eq!(json["outputs"][0]["address"], payee);
}

/// A node that cannot answer is an error with context, not an input silently
/// reported as missing.
#[tokio::test]
async fn test_pending_transaction_fails_loudly_when_the_node_cannot_resolve_inputs() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "method": "get_transaction",
            "params": [pending_previous_output_hash_hex()]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32000, "message": "node is busy" }
        })))
        .mount(&server)
        .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/transactions/{hash}/detail"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["message"].as_str().unwrap().contains("node is busy"),
        "the node's own failure must reach the caller: {:?}",
        json["message"]
    );
}

/// The window between node commit and local index: the node has the
/// transaction in a block, this process's store does not have it yet. It is
/// served provisionally rather than 404, so the pool row that links here is not
/// a dead link for those seconds (hours during bulk sync).
///
/// Fixture = what a real node returns: a committed transaction carries no pool
/// `fee`, and its input is SPENT by it — no live-cell answer exists. The input
/// resolves from the parent transaction's body; the fee is Σinputs − Σoutputs.
#[tokio::test]
async fn test_committed_but_unindexed_transaction_is_served_provisionally() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let mut response_json = pending_transaction_rpc_response(&hash, "committed");
    response_json["result"]["tx_status"]["block_number"] = serde_json::json!("0x1092");
    response_json["result"]["tx_status"]["block_hash"] =
        serde_json::json!(format!("0x{}", "33".repeat(32)));
    response_json["result"]["fee"] = serde_json::Value::Null;
    response_json["result"]["time_added_to_pool"] = serde_json::Value::Null;
    response_json["result"]["min_replace_fee"] = serde_json::Value::Null;
    mount_transaction_rpc(&server, response_json).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["status"], "committed");
    assert_eq!(json["poolStatus"], "committed_awaiting_index");
    // The tx page links the committing block while indexing (lane F contract):
    // a committed response never has a null block number.
    assert_eq!(json["blockNumber"], 4242);
    assert_eq!(json["blockHash"], format!("0x{}", "33".repeat(32)));
    assert_eq!(json["timestamp"], serde_json::Value::Null);
    assert_eq!(json["confirmations"], serde_json::Value::Null);
    assert_eq!(json["interpretation"]["status"], "complete");

    let inputs_capacity: u128 = json["inputsCapacity"].as_str().unwrap().parse().unwrap();
    let outputs_capacity: u128 = json["outputsCapacity"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        json["fee"],
        (inputs_capacity - outputs_capacity).to_string()
    );
    assert_eq!(json["fee"], "372");
    let input = &json["inputs"][0];
    assert_eq!(input["capacity"], "100000000372");
    assert_eq!(input["lock"]["codeHash"], TEST_SECP_LOCK_CODE_HASH);
    assert!(
        input["address"].as_str().unwrap().starts_with("ckb1"),
        "a spent input still carries its owner's address: {input:?}"
    );
}

/// With the mirror off there is no snapshot to hold a pool parent, yet a
/// chained pool spend still resolves: the node answers `get_transaction` for
/// pool transactions too.
#[tokio::test]
async fn pending_tx_whose_parent_is_also_pending_resolves_without_the_mirror() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    let mut parent =
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20)));
    parent["result"]["tx_status"] = serde_json::json!({
        "status": "pending",
        "block_hash": null,
        "block_number": null,
        "reason": null
    });
    mount_transaction_rpc(&server, parent).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    config.pool_mirror_enabled = false;
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["poolStatus"], "pending");
    assert_eq!(json["interpretation"]["status"], "complete");
    assert_eq!(json["inputs"][0]["capacity"], "100000000372");
    assert_eq!(json["fee"], "372");
}

/// An input whose parent the node does not know cannot be resolved YET (the
/// parent was evicted, or has not propagated). The response is a retryable
/// 503, never a 200 carrying unresolved inputs and no fee.
#[tokio::test]
async fn pending_tx_with_unknown_parent_returns_503() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    mount_pending_transaction_rpc(&server, &hash, "pending").await;
    mount_transaction_unknown_to_node(&server, &pending_previous_output_hash_hex()).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json:?}");
    assert_eq!(json["error"], "service_unavailable");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains(&pending_previous_output_hash_hex()) && message.contains("retry"),
        "the 503 must name the unknown parent and say to retry: {message}"
    );
}

/// A pending Nervos DAO withdrawal completion is served with its EXACT fee:
/// Σinputs + compensation − Σoutputs, the compensation priced from the deposit
/// and request headers' accumulated rates and the request cell's own occupied
/// capacity (RFC-0023).
///
/// Numbers are the common DAO test vector
/// `compensation_uses_the_cells_actual_occupied_capacity`: a 300 CKB cell
/// occupying 142 CKB (lock args 60 bytes + DAO type + 8-byte data), AR 10_000 →
/// 11_000, compensation 15.8 CKB.
#[tokio::test]
async fn pending_dao_withdrawal_is_served_with_its_exact_fee() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();
    let request_tx = format!("0x{}", "d1".repeat(32));
    let request_block = format!("0x{}", "b1".repeat(32));
    let deposit_block_number: u64 = 1_000;
    let fee: u64 = 1_000;
    let request_capacity: u64 = 300_00000000;
    let compensation: u64 = 15_80000000;

    mount_transaction_rpc(
        &server,
        transaction_rpc_response(
            &request_tx,
            &[(&format!("0x{}", "d0".repeat(32)), 0)],
            vec![serde_json::json!({
                "capacity": format!("0x{request_capacity:x}"),
                "lock": {
                    "code_hash": TEST_SECP_LOCK_CODE_HASH,
                    "hash_type": "type",
                    "args": format!("0x{}", "44".repeat(60))
                },
                "type": {
                    "code_hash": ckbadger_indexer::parser::dao::DAO_CODE_HASH,
                    "hash_type": "type",
                    "args": "0x"
                }
            })],
            vec![format!(
                "0x{}",
                hex::encode(deposit_block_number.to_le_bytes())
            )],
            "committed",
            Some((2_000, &request_block)),
        ),
    )
    .await;
    mount_transaction_rpc(
        &server,
        transaction_rpc_response(
            &hash,
            &[(&request_tx, 0)],
            vec![serde_json::json!({
                "capacity": format!("0x{:x}", request_capacity + compensation - fee),
                "lock": {
                    "code_hash": TEST_SECP_LOCK_CODE_HASH,
                    "hash_type": "type",
                    "args": format!("0x{}", "44".repeat(20))
                },
                "type": null
            })],
            vec!["0x".to_string()],
            "pending",
            None,
        ),
    )
    .await;
    mount_header_rpc(
        &server,
        deposit_block_number,
        &format!("0x{}", "b0".repeat(32)),
        10_000,
    )
    .await;
    mount_header_rpc(&server, 2_000, &request_block, 11_000).await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["interpretation"]["status"], "complete", "{json:?}");
    assert_eq!(json["inputs"][0]["capacity"], request_capacity.to_string());
    assert_eq!(json["fee"], fee.to_string());
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
    // `txSize` is the serialized size in block (molecule 222 + the 4-byte offset
    // slot), the size the node, explorer and wallets report — and the same size
    // `feeRate` divides by, so the two fields reproduce each other:
    // 1234 * 1000 / 226 = 5460.
    assert_eq!(json["txSize"], 226);
    assert_eq!(json["feeRate"], "5460");
    assert_eq!(
        json["feeRate"].as_str().unwrap(),
        (1234u128 * 1000 / json["txSize"].as_u64().unwrap() as u128).to_string(),
        "feeRate must be reproducible from the served txSize"
    );
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

// ---------------------------------------------------------------------------
// R4-E bug 3: /transactions/{hash}/lifecycle must honour proposal zones carried
// by *uncles* embedded in a window block, not just the main chain block's own
// `proposals()`. CKB consensus counts uncle proposal zones, so a tx proposed
// only inside an uncle used to report `proposedIn: null`.
// ---------------------------------------------------------------------------

/// One block of the fixture chain: its own proposal zone plus the proposal zones
/// of the uncles it embeds.
struct ProposalZone {
    number: u64,
    proposals: Vec<Vec<u8>>,
    uncles: Vec<(u64, Vec<Vec<u8>>)>,
}

fn build_fixture_block(zone: &ProposalZone) -> ckb_types::core::BlockView {
    use ckb_types::core::{BlockBuilder, EpochNumberWithFraction};
    use ckb_types::packed::ProposalShortId;
    use ckb_types::prelude::*;

    let proposal_id = |raw: &Vec<u8>| ProposalShortId::from_slice(raw).expect("10-byte short id");
    // Non-genesis headers must carry a well-formed epoch (length > index > 0-length).
    let epoch = EpochNumberWithFraction::new(1, 0, 1800);

    let mut uncle_views = Vec::new();
    for (uncle_number, uncle_proposals) in &zone.uncles {
        let mut uncle = BlockBuilder::default()
            .number(uncle_number.pack())
            .epoch(epoch.pack());
        for raw in uncle_proposals {
            uncle = uncle.proposal(proposal_id(raw));
        }
        uncle_views.push(uncle.build().as_uncle());
    }

    let mut builder = BlockBuilder::default()
        .number(zone.number.pack())
        .epoch(epoch.pack());
    for raw in &zone.proposals {
        builder = builder.proposal(proposal_id(raw));
    }
    for uncle in uncle_views {
        builder = builder.uncle(uncle);
    }
    builder.build()
}

/// Seed a committed transaction plus the [commit-10, commit-2] proposal window in
/// both stores: ckbadger block headers (hash + timestamp) and a CKB-node-format
/// RocksDB holding the real blocks.
fn seed_lifecycle_fixture(
    tx_hash: &[u8],
    commit_block: i64,
    zones: &[ProposalZone],
) -> (Arc<CkbadgerStore>, TestCkbChain) {
    use ckb_types::prelude::*;

    let store = test_store();
    let blocks: Vec<ckb_types::core::BlockView> = zones.iter().map(build_fixture_block).collect();

    let mut batch = StoreBatch::new(store.as_ref());
    let header = |hash: Vec<u8>, number: i64| CachedBlockHeader {
        hash,
        parent_hash: vec![0u8; 32],
        timestamp: 1_700_000_000_000 + number,
        epoch_number: 1,
        epoch_index: 0,
        epoch_length: 1800,
        dao: vec![0; 32],
        transactions_count: 1,
        uncles_count: 0,
        proposals_count: 0,
        compact_target: 0,
        miner_lock_hash: None,
        cycles: None,
    };
    for block in &blocks {
        let hash: [u8; 32] = block.hash().unpack();
        batch.put_block_header(
            block.number() as i64,
            &header(hash.to_vec(), block.number() as i64),
        );
    }
    batch.put_block_header(commit_block, &header(vec![0xC0; 32], commit_block));
    batch.put_tx_hash_map(tx_hash, commit_block, 0);
    batch.put_tx_index(
        commit_block,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000 + commit_block,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1000,
            tx_size: 500,
            cycles: Some(1000),
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = commit_block + 100;
        })
        .unwrap();

    let chain = seed_ckb_chain(&blocks);
    (store, chain)
}

#[tokio::test]
async fn test_transaction_lifecycle_honours_uncle_proposal_zone() {
    let tx_hash = vec![0x7b; 32];
    let short_id = tx_hash[..10].to_vec();
    let other_id = vec![0x9f; 10];
    let commit_block = 442i64;

    // The tx's short id appears ONLY in the proposals of an uncle (#438) embedded
    // in main block 440; block 440's own proposal zone holds a different id.
    let zones = vec![
        ProposalZone {
            number: 434,
            proposals: vec![],
            uncles: vec![],
        },
        ProposalZone {
            number: 437,
            proposals: vec![other_id.clone()],
            uncles: vec![],
        },
        ProposalZone {
            number: 440,
            proposals: vec![other_id.clone()],
            uncles: vec![(438, vec![short_id.clone()])],
        },
    ];
    let (store, chain) = seed_lifecycle_fixture(&tx_hash, commit_block, &zones);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["phase"], "committed");
    assert_eq!(
        json["proposedIn"]["blockNumber"], 440,
        "uncle-borne proposals belong to the main-chain block that embeds the uncle, got {}",
        json["proposedIn"]
    );
    assert_eq!(json["commitmentDistance"], commit_block - 440);
    assert_eq!(json["proposedInUncle"]["blockNumber"], 438);
    assert_eq!(json["committedIn"]["blockNumber"], commit_block);
}

#[tokio::test]
async fn test_transaction_lifecycle_reports_earliest_main_proposal() {
    // Control: a tx proposed directly in the main proposal zone still reports the
    // earliest window block, and carries no uncle attribution.
    let tx_hash = vec![0x3c; 32];
    let short_id = tx_hash[..10].to_vec();
    let commit_block = 442i64;

    let zones = vec![
        ProposalZone {
            number: 434,
            proposals: vec![],
            uncles: vec![],
        },
        ProposalZone {
            number: 435,
            proposals: vec![short_id.clone()],
            uncles: vec![],
        },
        ProposalZone {
            number: 437,
            proposals: vec![short_id.clone()],
            uncles: vec![],
        },
    ];
    let (store, chain) = seed_lifecycle_fixture(&tx_hash, commit_block, &zones);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["proposedIn"]["blockNumber"], 435);
    assert_eq!(json["commitmentDistance"], commit_block - 435);
    assert_eq!(json["proposedInUncle"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_transaction_lifecycle_prefers_main_zone_over_uncle_in_same_block() {
    // When one block proposes the tx both directly and through an uncle, the
    // direct main-chain proposal owns the attribution.
    let tx_hash = vec![0x5e; 32];
    let short_id = tx_hash[..10].to_vec();
    let commit_block = 442i64;

    let zones = vec![ProposalZone {
        number: 436,
        proposals: vec![short_id.clone()],
        uncles: vec![(433, vec![short_id.clone()])],
    }];
    let (store, chain) = seed_lifecycle_fixture(&tx_hash, commit_block, &zones);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/lifecycle", hex::encode(&tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["proposedIn"]["blockNumber"], 436);
    assert_eq!(json["proposedInUncle"], serde_json::Value::Null);
}

// ---------------------------------------------------------------------------
// Audited bug (2026-08-01 night, agent E): /transactions/{hash}/cell-deps
// answered `200 []` both when the CKB RocksDB reader was unavailable and when
// the transaction did not exist — a silent-empty shape that made "no deps",
// "no reader", and "no such tx" indistinguishable. Fail-fast contract: reader
// unavailable -> 5xx with context; tx nonexistent -> 404.
// ---------------------------------------------------------------------------

fn unknown_transaction_rpc_response() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "transaction": null,
            "cycles": null,
            "fee": null,
            "min_replace_fee": null,
            "time_added_to_pool": null,
            "tx_status": {
                "status": "unknown",
                "block_hash": null,
                "block_number": null,
                "reason": null
            }
        }
    })
}

async fn mount_unknown_transaction_rpc(server: &MockServer) {
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "get_transaction" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(unknown_transaction_rpc_response()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_cell_deps_nonexistent_tx_returns_404() {
    let store = test_store();
    let server = MockServer::start().await;
    mount_unknown_transaction_rpc(&server).await;
    let hash = format!("0x{}", "ee".repeat(32));

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/cell-deps")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a transaction unknown to both the CKB store and the node must 404, not answer a silent `200 []`, got {json}"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Transaction not found"),
        "got {json}"
    );
}

#[tokio::test]
async fn test_cell_deps_without_ckb_store_is_5xx_not_silent_empty() {
    let store = test_store();
    // The RPC mock reports the tx as unknown so the OLD code path (RPC lookup
    // then `ok(vec![])`) observably produced `200 []` here rather than failing
    // on an unreachable RPC endpoint.
    let server = MockServer::start().await;
    mount_unknown_transaction_rpc(&server).await;

    // A CKB DB path that does not exist: the reader cannot open, so the
    // endpoint has no data source at all.
    let missing_path =
        std::env::temp_dir().join(format!("ckbadger-missing-ckb-db-{}", Uuid::new_v4()));
    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        missing_path.to_string_lossy().to_string(),
        None,
    );
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/cell-deps", "ee".repeat(32)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "no CKB RocksDB reader means the endpoint has no data source: loud 5xx, never a silent `200 []`, got {json}"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("CKB RocksDB reader"),
        "error must name the unavailable reader, got {json}"
    );
}

#[tokio::test]
async fn test_cell_deps_committed_tx_returns_deps_from_ckb_store() {
    use ckb_types::prelude::*;

    let store = test_store();

    let dep_tx_hash = [0xAB; 32];
    let tx = ckb_types::core::TransactionBuilder::default()
        .cell_dep(
            ckb_types::packed::CellDep::new_builder()
                .out_point(
                    ckb_types::packed::OutPoint::new_builder()
                        .tx_hash(ckb_types::packed::Byte32::new(dep_tx_hash))
                        .index(1u32.pack())
                        .build(),
                )
                .dep_type(ckb_types::core::DepType::DepGroup.into())
                .build(),
        )
        .build();
    let tx_hash: [u8; 32] = tx.hash().unpack();

    let epoch = ckb_types::core::EpochNumberWithFraction::new(1, 0, 1800);
    let block = ckb_types::core::BlockBuilder::default()
        .number(77u64.pack())
        .epoch(epoch.pack())
        .transaction(tx)
        .build();
    let chain = seed_ckb_chain(&[block]);

    let config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/cell-deps", hex::encode(tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let deps = json.as_array().expect("cell deps array");
    assert_eq!(deps.len(), 1);
    assert_eq!(
        deps[0]["outPointTxHash"],
        format!("0x{}", hex::encode(dep_tx_hash))
    );
    assert_eq!(deps[0]["outPointIndex"], 1);
    assert_eq!(deps[0]["depType"], "dep_group");
}

// ── `.cell` records decoded from the creating transaction's witness ──────
// Chain data copied from `crates/indexer/src/parser/dotcell_fixtures.rs`
// `T2_REGISTER_JOAOM`: testnet tx 0x89191ea4…386b at block 22471181, which
// registers `joaom.cell` (output 1, no records) and re-creates its ring
// predecessor `maria.cell` (output 0, six records in witness 0). Node-verified
// 2026-09-24. The fixture module is `#[cfg(test)]` in the indexer crate.

const T2_ACCOUNT_LOCK: &str = "ede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd";
const T2_ACCOUNT_TYPE: &str = "e0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9";
const T2_NAMESPACE_ARGS: &str = "2510c78057479c9b023fe6e98ce43979e92a1353";
/// `T2_OUT0_DATA` (`maria.cell`).
const T2_OUT0_DATA: &str = "033b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb18241e3586a41eb75dd6d68bf555acea74d6649ed529da856c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b1416d61726961";
/// `T2_OUT1_DATA` (`joaom.cell`).
const T2_OUT1_DATA: &str = "0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e22cf2cdac7ab0e7b97f4b50475fb5ce1b32dfe711fbe68f6c0069e8165efb4cb3b2cd62300e7d16f41a1c65ceab69e8165efb4cb3b2cd62300e7d16f41a1c65ceab6a6f616f6d";
/// `T2_WITNESS_0`: maria's six records in `output_type`.
const T2_WITNESS_0: &str = "c901000010000000100000001c000000080000007265676973746572a901000006000b616464726573732e333039006400636b7431717266727763646e76737373776477706e337339763866703837656d617433303663746a77736d336e6d6c6b6a673871797a61326371677171397837357a75346c37676c64363036723665796430306d346c7a79337a6b786b71346e79777a752c01000009616464726573732e30003e00746231703237767464306a776d3235746478336d36746763757168346c6578756c3076736c7572683079637a633766393272667030736b737264737936742c0100000a616464726573732e3630002a003078656646324634436132444536656444363364416665343833383536463134453639346437333144442c0100000d70726f66696c652e656d61696c0011006d61726961406578616d706c652e636f6d2c0100000d70726f66696c652e70686f6e650010002b3335312039313220333435203637382c0100000a647765622e636b626673004800636b6266733a2f2f346266306362646261633066386538656231616664646534333963336263306234333731623366336165636538633730343866656630366566366563376231302c010000";
/// `T2_WITNESS_1`: a secp witness whose `output_type` is joaom's empty records payload.
const T2_WITNESS_1: &str = "7f00000010000000550000007900000041000000e329de7a51feb20409c370832f99a08b76236b2c6cdbb7cc5bbc2858d1bc69b753a8b932eaa752fa46c794eaa36adee5cd063a25760cbd0fa151034f6b0e66010020000000b749b13ab9026ab71f3cb0cc971d0dbf1030bae1f8b3a006a45055d5eec73f0a020000000000";

/// The T2 transaction rebuilt from its chain bytes. `cell_deps` are not part of
/// the fixture, so the hash differs from the chain's; nothing here reads it
/// beyond looking the transaction up by it.
fn t2_register_transaction() -> ckb_types::core::TransactionView {
    use ckb_types::bytes::Bytes;
    use ckb_types::core::{Capacity, ScriptHashType, TransactionBuilder};
    use ckb_types::packed;
    use ckb_types::prelude::*;

    let script = |code_hash: &str, args: &str| {
        let code_hash: [u8; 32] = hex::decode(code_hash).unwrap().try_into().unwrap();
        packed::Script::new_builder()
            .code_hash(packed::Byte32::new(code_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(hex::decode(args).unwrap()).pack())
            .build()
    };
    let output = |capacity: u64, lock: packed::Script, type_: Option<packed::Script>| {
        packed::CellOutput::new_builder()
            .capacity(Capacity::shannons(capacity).pack())
            .lock(lock)
            .type_(type_.pack())
            .build()
    };
    let input = |tx_hash: &str, index: u32| {
        let tx_hash: [u8; 32] = hex::decode(tx_hash).unwrap().try_into().unwrap();
        packed::CellInput::new(
            packed::OutPoint::new_builder()
                .tx_hash(packed::Byte32::new(tx_hash))
                .index(index.pack())
                .build(),
            0,
        )
    };
    let bytes = |hex_str: &str| Bytes::from(hex::decode(hex_str).unwrap()).pack();
    let name_lock = || script(T2_ACCOUNT_LOCK, "");
    let name_type = || Some(script(T2_ACCOUNT_TYPE, T2_NAMESPACE_ARGS));

    TransactionBuilder::default()
        .input(input(
            "974fc983a62a6f7b977c6e2170695cc4aafc5a7544c1aa2513574f1563fa0fad",
            0,
        ))
        .input(input(
            "b1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae",
            0,
        ))
        .input(input(
            "b1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae",
            1,
        ))
        .output(output(0x59682f000, name_lock(), name_type()))
        .output_data(bytes(T2_OUT0_DATA))
        .output(output(0x59682f000, name_lock(), name_type()))
        .output_data(bytes(T2_OUT1_DATA))
        .output(output(
            0x5c89ddb680,
            script(
                "d23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
                "000140911fa94eaef8c1d0eca81b23e1972ecb0548dc",
            ),
            None,
        ))
        .output_data(bytes(""))
        .output(output(
            0x19f6a3c11688,
            script(
                "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                "23870b08ec5f6260c50a63646170d61e26d155c7",
            ),
            None,
        ))
        .output_data(bytes(""))
        .witness(bytes(T2_WITNESS_0))
        .witness(bytes(T2_WITNESS_1))
        .build()
}

/// The tx page decodes each `.cell` name's records where they live — the
/// witness at the name cell's own output index — through the same parser the
/// indexer runs, and resolves the `address.309` record to a testnet address.
#[tokio::test]
async fn test_transaction_detail_decodes_dotcell_records_from_witness() {
    use ckb_types::prelude::*;

    let block_number: i64 = 22_471_181;
    let tx = t2_register_transaction();
    let tx_hash: [u8; 32] = tx.hash().unpack();
    let block = ckb_types::core::BlockBuilder::default()
        .number((block_number as u64).pack())
        .epoch(ckb_types::core::EpochNumberWithFraction::new(1, 0, 1800).pack())
        .transaction(tx)
        .build();
    let block_hash: [u8; 32] = block.hash().unpack();
    let chain = seed_ckb_chain(&[block]);

    let store = test_store();
    seed_genesis_baseline(&store);
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_block_header(
        block_number,
        &CachedBlockHeader {
            hash: block_hash.to_vec(),
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        },
    );
    batch.put_tx_hash_map(&tx_hash, block_number, 0);
    batch.put_tx_index(
        block_number,
        0,
        &TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 3,
            outputs_count: 4,
            fee: 1000,
            tx_size: 1500,
            cycles: Some(1000),
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();
    store
        .update_sync_status(|s| {
            s.tip_block_number = block_number + 10;
        })
        .unwrap();

    let mut config = test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    );
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let (status, json) = get_json(
        &app,
        &format!("/transactions/0x{}/detail", hex::encode(tx_hash)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    let names = json["dotcellNames"]
        .as_array()
        .unwrap_or_else(|| panic!("dotcellNames missing: {json}"));
    assert_eq!(names.len(), 2, "both name outputs: {json}");

    let maria = &names[0];
    assert_eq!(maria["outputIndex"], 0);
    assert_eq!(maria["name"], "maria.cell");
    assert_eq!(
        maria["identityId"],
        "0x2224948f63975a7a0741139cd5d2a45b9fb02c03"
    );
    let records = maria["records"].as_array().expect("records");
    assert_eq!(records.len(), 6);
    assert_eq!(records[0]["key"], "address.309");
    assert!(
        records[0]["decodedAddress"]["address"]
            .as_str()
            .is_some_and(|address| address.starts_with("ckt1")),
        "got {}",
        records[0]
    );

    let joaom = &names[1];
    assert_eq!(joaom["outputIndex"], 1);
    assert_eq!(joaom["name"], "joaom.cell");
    assert_eq!(
        joaom["identityId"],
        "0x241e3586a41eb75dd6d68bf555acea74d6649ed5"
    );
    assert_eq!(joaom["records"].as_array().map(Vec::len), Some(0));
}

/// The pending (pool) builder decodes `.cell` records through the same call:
/// a name registration waiting in the pool already shows its records.
#[tokio::test]
async fn test_pending_transaction_detail_decodes_dotcell_records_from_witness() {
    let store = test_store();
    seed_genesis_baseline(&store);
    let server = MockServer::start().await;
    let hash = pending_tx_hash_hex();

    // The standard pending fixture, its one output replaced by T2's
    // `maria.cell` with the witness carrying maria's records.
    let mut response = pending_transaction_rpc_response(&hash, "pending");
    let tx = &mut response["result"]["transaction"];
    tx["outputs"] = serde_json::json!([{
        "capacity": "0x59682f000",
        "lock": {
            "code_hash": format!("0x{T2_ACCOUNT_LOCK}"),
            "hash_type": "type",
            "args": "0x"
        },
        "type": {
            "code_hash": format!("0x{T2_ACCOUNT_TYPE}"),
            "hash_type": "type",
            "args": format!("0x{T2_NAMESPACE_ARGS}")
        }
    }]);
    tx["outputs_data"] = serde_json::json!([format!("0x{T2_OUT0_DATA}")]);
    tx["witnesses"] = serde_json::json!([format!("0x{T2_WITNESS_0}")]);
    mount_transaction_rpc(&server, response).await;
    mount_transaction_rpc(
        &server,
        funding_transaction_rpc_response("0x174876e974", &format!("0x{}", "33".repeat(20))),
    )
    .await;

    let mut config = test_config(store);
    config.ckb_rpc_url = server.uri();
    config.ckb_network = "testnet".to_string();
    let app = create_router(config).await;

    let (status, json) = get_json(&app, &format!("/transactions/{hash}/detail")).await;
    assert_eq!(status, StatusCode::OK, "got {json}");
    assert_eq!(json["status"], "pending");
    let names = json["dotcellNames"]
        .as_array()
        .unwrap_or_else(|| panic!("dotcellNames missing: {json}"));
    assert_eq!(names.len(), 1);
    assert_eq!(names[0]["outputIndex"], 0);
    assert_eq!(names[0]["name"], "maria.cell");
    let records = names[0]["records"].as_array().expect("records");
    assert_eq!(records.len(), 6);
    assert!(
        records[0]["decodedAddress"]["address"]
            .as_str()
            .is_some_and(|address| address.starts_with("ckt1")),
        "got {}",
        records[0]
    );
}
