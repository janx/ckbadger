//! `GET /cells/live` must honour every filter it validates, or say it cannot.
//!
//! The handler validated `type_code_hash` and then dropped it on three of its
//! four dispatch branches, because the branch selection matched only on
//! `(lock_script_hash, type_script_hash)`. A caller therefore got rows that did
//! not match what it asked for, and the `type_code_hash`-only case answered
//! `{"data": [], "hasMore": false}` — indistinguishable from "no such cells
//! exist" — while the store had a cell-by-type-code index available all along.
//!
//! Each test below seeds one real live cell and asserts against it, rather than
//! asserting a status code that an empty store would satisfy either way.

mod common;
use common::*;

use ckbadger_store::{keys, LiveCellInfo, StoreBatch};

const LOCK_HASH: [u8; 32] = [0xAA; 32];
const TYPE_HASH: [u8; 32] = [0xBB; 32];
const TYPE_CODE_HASH: [u8; 32] = [0xCC; 32];
/// A code hash the seeded cell does *not* use.
const OTHER_CODE_HASH: [u8; 32] = [0xDD; 32];
const TX_HASH: [u8; 32] = [0x11; 32];
const BLOCK: i64 = 100;

fn hex32(bytes: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// One live cell carrying both a type script hash and a type code hash, indexed
/// by lock and by type exactly as the indexer write path does.
fn seed_one_live_cell(store: &std::sync::Arc<CkbadgerStore>) {
    let info = LiveCellInfo {
        capacity: 100_00000000,
        lock_script_hash: LOCK_HASH.to_vec(),
        lock_code_hash: vec![0x01; 32],
        lock_hash_type: 1,
        lock_args: vec![],
        type_script_hash: Some(TYPE_HASH.to_vec()),
        type_code_hash: Some(TYPE_CODE_HASH.to_vec()),
        type_hash_type: Some(1),
        type_args: Some(vec![]),
        data_size: 0,
        occupied_capacity: 61_00000000,
        udt_amount: None,
        data_hash: None,
    };

    let mut batch = StoreBatch::new(store);
    batch.put_cell(&TX_HASH, 0, &info, BLOCK);
    batch.put_cell_by_lock(&LOCK_HASH, BLOCK, &TX_HASH, 0);
    batch.put_cell_by_type(&TYPE_HASH, BLOCK, &TX_HASH, 0);
    batch.commit().unwrap();
}

async fn seeded_app() -> axum::Router {
    let store = test_store();
    seed_genesis_baseline(&store);
    seed_one_live_cell(&store);
    create_router(test_config(store)).await
}

fn returned_tx_hashes(body: &serde_json::Value) -> Vec<String> {
    body["data"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| r["txHash"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Baseline: the seeded cell is reachable, so an empty result in the tests below
/// means the filter dropped it rather than the fixture being wrong.
#[tokio::test]
async fn test_seeded_cell_is_listed_by_lock() {
    let app = seeded_app().await;
    let (status, body) = get_json(
        &app,
        &format!("/cells/live?lock_script_hash={}", hex32(&LOCK_HASH)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(
        returned_tx_hashes(&body),
        vec![hex32(&TX_HASH)],
        "body={body}"
    );
}

/// The `(Some, Some)` branch ignored `type_code_hash` entirely: this returned
/// the cell even though its type code hash is not the one requested.
#[tokio::test]
async fn test_lock_and_type_hash_scan_still_applies_type_code_hash() {
    let app = seeded_app().await;

    let matching = format!(
        "/cells/live?lock_script_hash={}&type_script_hash={}&type_code_hash={}",
        hex32(&LOCK_HASH),
        hex32(&TYPE_HASH),
        hex32(&TYPE_CODE_HASH)
    );
    let (status, body) = get_json(&app, &matching).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(
        returned_tx_hashes(&body),
        vec![hex32(&TX_HASH)],
        "a cell matching all three filters must be returned, body={body}"
    );

    let mismatched = format!(
        "/cells/live?lock_script_hash={}&type_script_hash={}&type_code_hash={}",
        hex32(&LOCK_HASH),
        hex32(&TYPE_HASH),
        hex32(&OTHER_CODE_HASH)
    );
    let (status, body) = get_json(&app, &mismatched).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(
        returned_tx_hashes(&body).is_empty(),
        "type_code_hash={} does not match the cell's {}; returning it means the filter was \
         dropped, body={body}",
        hex32(&OTHER_CODE_HASH),
        hex32(&TYPE_CODE_HASH)
    );
}

/// The `(None, Some)` branch ignored `type_code_hash` too.
#[tokio::test]
async fn test_type_hash_scan_still_applies_type_code_hash() {
    let app = seeded_app().await;

    let matching = format!(
        "/cells/live?type_script_hash={}&type_code_hash={}",
        hex32(&TYPE_HASH),
        hex32(&TYPE_CODE_HASH)
    );
    let (status, body) = get_json(&app, &matching).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(
        returned_tx_hashes(&body),
        vec![hex32(&TX_HASH)],
        "body={body}"
    );

    let mismatched = format!(
        "/cells/live?type_script_hash={}&type_code_hash={}",
        hex32(&TYPE_HASH),
        hex32(&OTHER_CODE_HASH)
    );
    let (status, body) = get_json(&app, &mismatched).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(
        returned_tx_hashes(&body).is_empty(),
        "the requested type_code_hash does not match the seeded cell, body={body}"
    );
}

/// The lock scan already honoured `type_code_hash`; pin it so the refactor that
/// unified the three scans into one predicate cannot regress it.
#[tokio::test]
async fn test_lock_scan_applies_type_code_hash() {
    let app = seeded_app().await;
    for (code_hash, expect_hit) in [(TYPE_CODE_HASH, true), (OTHER_CODE_HASH, false)] {
        let path = format!(
            "/cells/live?lock_script_hash={}&type_code_hash={}",
            hex32(&LOCK_HASH),
            hex32(&code_hash)
        );
        let (status, body) = get_json(&app, &path).await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        assert_eq!(
            !returned_tx_hashes(&body).is_empty(),
            expect_hit,
            "type_code_hash={} expect_hit={expect_hit}, body={body}",
            hex32(&code_hash)
        );
    }
}

/// A filter combination that selects no index must say so. Returning an empty
/// page told the caller "no such cells exist" while the cell was sitting in the
/// store, which is the silent-wrong-answer half of the same defect.
#[tokio::test]
async fn test_unindexable_filter_combinations_are_rejected_not_silently_empty() {
    let app = seeded_app().await;

    let (status, body) = get_json(
        &app,
        &format!("/cells/live?type_code_hash={}", hex32(&TYPE_CODE_HASH)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "type_code_hash alone selects no index and must be an explicit error rather than an \
         empty page — the seeded cell has exactly this type code hash, body={body}"
    );
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("by-script"),
        "the error must point at the endpoint that does serve this query, got {message:?}"
    );

    let (status, body) = get_json(&app, "/cells/live").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an unfiltered listing would be a full CF scan and must be refused explicitly, body={body}"
    );
}

/// Pagination must survive the cursor validation added alongside these fixes.
#[tokio::test]
async fn test_cursor_from_this_scan_is_accepted() {
    let app = seeded_app().await;
    let cursor = hex::encode(keys::encode_cell_index_key(&LOCK_HASH, BLOCK, &TX_HASH, 0));
    let path = format!(
        "/cells/live?lock_script_hash={}&cursor=0x{cursor}",
        hex32(&LOCK_HASH)
    );
    let (status, body) = get_json(&app, &path).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a cursor this endpoint produced must be accepted, body={body}"
    );
    assert!(
        returned_tx_hashes(&body).is_empty(),
        "seeking past the only cell must yield an empty page, body={body}"
    );
}
