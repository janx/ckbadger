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
    // This output is at block 0 but its lock args are not the Satoshi dead
    // address, so it must NOT be tagged as a genesis special burn cell.
    assert_eq!(json["outputs"][0]["cellType"], serde_json::Value::Null);
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        serde_json::Value::Null
    );
}

/// A pending genesis (block 0) output whose lock args equal the Satoshi
/// dead-address pubkey hash is tagged `genesis_special_burn` and reports the
/// network's seeded `baseline.virtual_occupied` as `virtualUsedCapacity`,
/// proving the burn policy + baseline flow through the tx-output builder.
#[tokio::test]
async fn test_pending_transaction_genesis_satoshi_output_tagged() {
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
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "get_transaction" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(rpc_response))
        .mount(&server)
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
    assert_eq!(json["outputs"][0]["cellType"], "genesis_special_burn");
    assert_eq!(
        json["outputs"][0]["virtualCommonKnowledgeSize"],
        "504000000000000000"
    );
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
    // feeRate divides by the serialized size in block (molecule size + 4),
    // matching the node/explorer/wallet convention: 1234 * 1000 / 226 = 5460.
    // The `txSize` field itself stays molecule-sized.
    assert_eq!(json["feeRate"], "5460");
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
