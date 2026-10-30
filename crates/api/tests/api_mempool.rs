use crate::common::*;

async fn mount_pool(server: &MockServer, pending: serde_json::Value) {
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({ "method": "get_raw_tx_pool" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "pending": pending, "proposed": {} }
        })))
        .mount(server)
        .await;
}

fn pool_entry(fee: u64, size: u64) -> serde_json::Value {
    serde_json::json!({
        "cycles": "0x1", "size": format!("0x{size:x}"),
        "fee": format!("0x{fee:x}"),
        "ancestors_count": "0x0", "timestamp": "0x1"
    })
}

#[tokio::test]
async fn mempool_rates_above_ieee_safe_integer_are_exact_strings_and_sorted() {
    let server = MockServer::start().await;
    let lower = format!("0x{}", "11".repeat(32));
    let higher = format!("0x{}", "22".repeat(32));
    mount_pool(
        &server,
        serde_json::json!({
            lower.clone(): pool_entry(9_007_199_254_740_992, 1),
            higher.clone(): pool_entry(9_007_199_254_740_993, 1),
        }),
    )
    .await;
    let mut config = test_config(test_store());
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/mempool/transactions").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["transactions"][0]["txHash"], higher);
    assert_eq!(json["transactions"][0]["fee"], "9007199254740993");
    assert_eq!(json["transactions"][0]["feeRate"], "9007199254740993.00");
    assert_eq!(json["transactions"][1]["feeRate"], "9007199254740992.00");
}

#[tokio::test]
async fn mempool_zero_size_is_invalid_not_zero_fee_rate() {
    let server = MockServer::start().await;
    let hash = format!("0x{}", "11".repeat(32));
    mount_pool(
        &server,
        serde_json::json!({
            hash: pool_entry(10, 0),
        }),
    )
    .await;
    let mut config = test_config(test_store());
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/mempool/transactions").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(json["message"].as_str().unwrap().contains("size"));
}

#[tokio::test]
async fn mempool_block_median_and_empty_fee_range_are_exact() {
    let server = MockServer::start().await;
    let lower = format!("0x{}", "33".repeat(32));
    let higher = format!("0x{}", "44".repeat(32));
    mount_pool(
        &server,
        serde_json::json!({
            lower: pool_entry(9_007_199_254_740_992, 1),
            higher: pool_entry(9_007_199_254_740_993, 1),
        }),
    )
    .await;
    let mut config = test_config(test_store());
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/mempool/blocks").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["pendingBlocks"][0]["medianFeeRate"],
        "9007199254740992.50"
    );
    assert_eq!(
        json["pendingBlocks"][0]["feeRateRange"]["max"],
        "9007199254740993.00"
    );
    assert_eq!(json["pendingBlocks"][0]["totalFee"], "18014398509481985");
    assert!(json["pendingBlocks"][1]["medianFeeRate"].is_null());
    assert!(json["pendingBlocks"][1]["feeRateRange"]["min"].is_null());
}

#[tokio::test]
async fn pending_proposals_reject_corrupt_sync_status_instead_of_tip_zero() {
    let store = test_store();
    store
        .put_cf(store.cf_sync_meta(), b"sync_status", b"invalid-bincode")
        .unwrap();
    let app = create_router(test_config(store)).await;
    let (status, json) = get_json(&app, "/mempool/pending-proposals").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
}

#[tokio::test]
async fn mempool_block_rejects_transaction_larger_than_consensus_limit() {
    let server = MockServer::start().await;
    let hash = format!("0x{}", "55".repeat(32));
    mount_pool(
        &server,
        serde_json::json!({
            hash: pool_entry(10, u64::MAX),
        }),
    )
    .await;
    let mut config = test_config(test_store());
    config.ckb_rpc_url = server.uri();
    let app = create_router(config).await;
    let (status, json) = get_json(&app, "/mempool/blocks").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("exceeds block limits"));
}
