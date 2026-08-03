//! Malformed-length hash rejection at the API boundary.
//!
//! Every endpoint that accepts a user-supplied 32-byte script/transaction hash
//! must reject wrong-length input with `400 Bad Request` before the value can
//! reach a store key encoder. Those encoders assert a 32-byte invariant, and
//! the release profile builds with `panic = "abort"`, so a leaked short hash
//! terminates the whole API process (remote DoS) while a leaked long hash is
//! silently truncated into a different key (wrong data).
//!
//! Each family below probes 31-byte (short) and 33-byte (long) hex.

mod common;
use common::*;

/// 31-byte hex — one byte short of a script/transaction hash.
fn short_hash_hex() -> String {
    format!("0x{}", "ab".repeat(31))
}

/// 33-byte hex — one byte past a script/transaction hash.
fn long_hash_hex() -> String {
    format!("0x{}", "ab".repeat(33))
}

async fn assert_rejects_malformed_hash(app: &axum::Router, template: &str) {
    for hash in [short_hash_hex(), long_hash_hex()] {
        let path = template.replace("{H}", &hash);
        let (status, body) = get_json(app, &path).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "GET /api/v1{path} must reject a {}-byte hash with 400, got {status} body={body}",
            (hash.len() - 2) / 2
        );
    }
}

fn test_app() -> impl std::future::Future<Output = axum::Router> {
    let store = test_store();
    seed_genesis_baseline(&store);
    let config = test_config(store);
    create_router(config)
}

#[tokio::test]
async fn test_transaction_endpoints_reject_malformed_hash() {
    let app = test_app().await;
    for template in [
        "/transactions/{H}",
        "/transactions/{H}/detail",
        "/transactions/{H}/cell-deps",
        "/transactions/{H}/cycles",
        "/transactions/{H}/lifecycle",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_cell_outpoint_endpoints_reject_malformed_tx_hash() {
    let app = test_app().await;
    for template in ["/cells/{H}/0", "/graph/cell/{H}/0"] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_graph_transaction_rejects_malformed_hash() {
    let app = test_app().await;
    assert_rejects_malformed_hash(&app, "/graph/transaction/{H}").await;
}

#[tokio::test]
async fn test_block_endpoints_reject_malformed_hash() {
    let app = test_app().await;
    for template in [
        "/blocks/{H}",
        "/blocks/{H}/fee-stats",
        "/blocks/{H}/proposals",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_address_endpoints_reject_malformed_lock_hash() {
    let app = test_app().await;
    for template in [
        "/addresses/{H}",
        "/addresses/{H}/transactions",
        "/addresses/{H}/tokens",
        "/addresses/{H}/activities",
        "/addresses/{H}/fiber/channels",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_dao_endpoints_reject_malformed_lock_hash() {
    let app = test_app().await;
    for template in ["/dao/deposits/{H}", "/dao/summary/{H}"] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_token_endpoints_reject_malformed_type_hash() {
    let app = test_app().await;
    for template in [
        "/tokens/{H}",
        "/tokens/{H}/holders",
        "/tokens/{H}/transfers",
        "/tokens/{H}/activities",
        "/tokens/{H}/charts/capacity-history",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_live_cells_query_rejects_malformed_hashes() {
    let app = test_app().await;
    for template in [
        "/cells/live?lock_script_hash={H}",
        "/cells/live?type_script_hash={H}",
        "/cells/live?type_code_hash={H}",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_cells_by_script_rejects_malformed_code_hash() {
    let app = test_app().await;
    for template in [
        "/cells/by-script?code_hash={H}&hash_type=type",
        "/cells/by-script?code_hash={H}&hash_type=data",
        "/cells/by-script?code_hash={H}&hash_type=type&script_kind=type",
        "/cells/by-script?code_hash={H}&hash_type=type&script_kind=both",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_script_code_cell_endpoints_reject_malformed_code_hash() {
    let app = test_app().await;
    for template in [
        "/scripts/code-cell?code_hash={H}",
        "/scripts/code-cell?code_hash={H}&hash_type=type",
        "/scripts/code-cells?code_hash={H}",
        "/scripts/code-cells?code_hash={H}&hash_type=data",
        "/scripts/charts/capacity-history?code_hash={H}",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

#[tokio::test]
async fn test_scripts_lookup_rejects_malformed_code_hash_in_batch() {
    let app = test_app().await;
    let valid = format!("0x{}", "11".repeat(32));

    for malformed in [short_hash_hex(), long_hash_hex()] {
        // Malformed entry alone, and mixed into an otherwise valid batch:
        // one bad item must fail the whole request, never be silently
        // truncated into a lookup for a different script.
        for body in [
            serde_json::json!({ "codeHashes": [malformed] }),
            serde_json::json!({ "codeHashes": [valid, malformed] }),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri("/api/v1/scripts/lookup")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "POST /scripts/lookup must reject body {body} with 400"
            );
        }
    }
}

#[tokio::test]
async fn test_scripts_lookup_rejects_malformed_tx_hash_hint() {
    let app = test_app().await;
    let valid = format!("0x{}", "11".repeat(32));

    for malformed in [short_hash_hex(), long_hash_hex()] {
        let body = serde_json::json!({ "codeHashes": [valid], "txHash": malformed });
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/scripts/lookup")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "POST /scripts/lookup must reject body {body} with 400"
        );
    }
}

#[tokio::test]
async fn test_spore_endpoints_reject_malformed_ids() {
    let app = test_app().await;
    for template in [
        "/spore/objects/{H}",
        "/spore/objects/{H}/activities",
        "/spore/objects/{H}/decode",
        "/spore/objects/{H}/render",
        "/spore/objects/{H}/charts/capacity-history",
        "/spore/clusters/{H}",
        "/spore/clusters/{H}/holders",
        "/spore/clusters/{H}/activities",
        "/spore/clusters/{H}/spores",
        "/spore/clusters/{H}/charts/capacity-history",
        "/spore/owner/{H}",
    ] {
        assert_rejects_malformed_hash(&app, template).await;
    }
}

/// Asset IDs are *not* 32-byte hashes: an mNFT class ID is 24 bytes, a token ID
/// 28, a `.bit` account ID 20. The store right-pads them into a 32-byte key
/// window, so the boundary invariant is "at most 32 bytes" — anything wider hits
/// the `pad_id_32` assert. Short IDs must stay usable; oversized ones must 400.
async fn assert_rejects_oversized_asset_id(app: &axum::Router, template: &str) {
    let oversized = template.replace("{H}", &long_hash_hex());
    let (status, body) = get_json(app, &oversized).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "GET /api/v1{oversized} must reject a 33-byte ID with 400, got {status} body={body}"
    );

    // A legitimately narrow ID must still be served (not found here, but never
    // a panic and never a 5xx).
    let narrow = template.replace("{H}", &format!("0x{}", "ab".repeat(24)));
    let (status, body) = get_json(app, &narrow).await;
    assert!(
        status.is_success() || status == StatusCode::NOT_FOUND,
        "GET /api/v1{narrow} must still accept a 24-byte asset ID, got {status} body={body}"
    );
}

#[tokio::test]
async fn test_object_asset_endpoints_reject_oversized_ids() {
    let app = test_app().await;
    for template in [
        "/assets/objects/{H}",
        "/assets/objects/{H}/items",
        "/assets/objects/{H}/holders",
        "/assets/objects/{H}/activities",
        "/assets/objects/{H}/charts/capacity-history",
        "/assets/objects/items/{H}",
        "/assets/objects/items/{H}/activities",
    ] {
        assert_rejects_oversized_asset_id(&app, template).await;
    }
}

#[tokio::test]
async fn test_object_collection_items_cursor_rejects_oversized_id() {
    let app = test_app().await;
    let collection = format!("0x{}", "ab".repeat(24));
    let (status, body) = get_json(
        &app,
        &format!(
            "/assets/objects/{collection}/items?cursor={}",
            long_hash_hex()
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an oversized item cursor must be rejected, got {status} body={body}"
    );
}

#[tokio::test]
async fn test_identity_item_endpoints_reject_oversized_ids() {
    let app = test_app().await;
    for template in [
        "/assets/identities/dotbit/items/{H}",
        "/assets/identities/dotbit/items/{H}/activities",
        "/assets/identities/did/items/{H}",
        "/assets/identities/bit-cell/items/{H}",
    ] {
        assert_rejects_oversized_asset_id(&app, template).await;
    }
}

/// Identity *collections* are the three fixed 32-byte sentinels, so every other
/// ID — oversized included — is already a 400 via the sentinel comparison. That
/// makes a status-only assertion here vacuous: it would stay green with the
/// boundary check removed entirely.
///
/// So this asserts the *reason*. An oversized ID must be rejected for its
/// length, by `parse_asset_id_max32`, before it can reach `pad_id_32` — not
/// incidentally, for failing to equal a sentinel.
#[tokio::test]
async fn test_identity_collection_endpoints_reject_oversized_ids_for_their_length() {
    let app = test_app().await;
    for template in [
        "/assets/identities/{H}",
        "/assets/identities/{H}/holders",
        "/assets/identities/{H}/activities",
        "/assets/identities/{H}/items",
    ] {
        let path = template.replace("{H}", &long_hash_hex());
        let (status, body) = get_json(&app, &path).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "GET /api/v1{path} must reject a 33-byte collection ID with 400, got {status} body={body}"
        );
        let message = body["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("33 bytes"),
            "GET /api/v1{path} must reject the 33-byte ID for its length, not for failing the \
             sentinel comparison — got {message:?}"
        );
    }
}

#[tokio::test]
async fn test_fiber_channel_rejects_malformed_channel_id() {
    let app = test_app().await;
    assert_rejects_malformed_hash(&app, "/fiber/channels/{H}").await;
}
