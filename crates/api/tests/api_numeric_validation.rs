//! Malformed numeric identifier and cursor rejection at the API boundary.
//!
//! Sibling of `api_hash_validation.rs`, covering the identifiers that sit *next
//! to* the hashes that file already pins: block numbers, output indexes and
//! `"<block>:<tx>"` pagination cursors.
//!
//! The failure mode is the same and worse. Store key encoders assert their
//! domain invariants (`encode_block_num`, `encode_desc_block_num`,
//! `encode_desc_tx_idx` and `encode_outpoint` all assert non-negative), the
//! release profile builds with `panic = "abort"`, and the router installs no
//! catch-panic layer — so one unauthenticated request carrying `?cursor=-1:0`
//! or `/blocks/-1` terminates the whole API process. Unlike the hash family
//! these need no valid hash and no seeded data to reach, and `/search?q=-1` is
//! reachable by typing into the public search box.
//!
//! The second family here is silent repair rather than abort: a wrapping
//! `as i16` narrowing served output 0's body for a request for output 65536,
//! and `Option`-returning cursor parsers combined with `.and_then(...)` turned
//! a corrupt cursor into "no cursor" and paginated forever on page 1.

mod common;
use common::*;

/// A well-formed 32-byte hash, so a rejection can only be about the *other*
/// component of the path or query.
fn valid_hash_hex() -> String {
    format!("0x{}", "ab".repeat(32))
}

fn test_app() -> impl std::future::Future<Output = axum::Router> {
    let store = test_store();
    seed_genesis_baseline(&store);
    let config = test_config(store);
    create_router(config)
}

async fn assert_bad_request(app: &axum::Router, path: &str, why: &str) {
    let (status, body) = get_json(app, path).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "GET /api/v1{path} must reject {why} with 400, got {status} body={body}"
    );
}

// ---------------------------------------------------------------------------
// Negative "<block>:<tx>" cursors — the process-abort family.
// ---------------------------------------------------------------------------

/// Reachable with an empty store, no valid hash and no authentication.
#[tokio::test]
async fn test_global_activities_reject_negative_cursor() {
    let app = test_app().await;
    for cursor in ["-1:0", "0:-1", "-1:-1"] {
        assert_bad_request(
            &app,
            &format!("/activities?cursor={cursor}"),
            "a negative activity cursor",
        )
        .await;
    }
}

#[tokio::test]
async fn test_address_activities_reject_negative_cursor() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for cursor in ["-1:0", "0:-1"] {
        assert_bad_request(
            &app,
            &format!("/addresses/{hash}/activities?cursor={cursor}"),
            "a negative activity cursor",
        )
        .await;
    }
}

/// These panic on the axum task itself rather than inside `spawn_blocking`, so
/// before the fix they did not even surface as a 500.
#[tokio::test]
async fn test_asset_and_spore_activities_reject_negative_cursor() {
    let app = test_app().await;
    let id = valid_hash_hex();
    for template in [
        "/assets/objects/{ID}/activities",
        "/assets/identities/dotbit/items/{ID}/activities",
        "/spore/objects/{ID}/activities",
        "/spore/clusters/{ID}/activities",
    ] {
        let path = format!("{}?cursor=-1:0", template.replace("{ID}", &id));
        assert_bad_request(&app, &path, "a negative activity cursor").await;
    }
}

#[tokio::test]
async fn test_token_activity_endpoints_reject_negative_cursor() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for template in ["/tokens/{H}/transfers", "/tokens/{H}/activities"] {
        let path = format!("{}?cursor=-1:0", template.replace("{H}", &hash));
        assert_bad_request(&app, &path, "a negative token cursor").await;
    }
}

#[tokio::test]
async fn test_transaction_listing_endpoints_reject_negative_cursor() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for path in [
        format!("/addresses/{hash}/transactions?cursor=-1:0"),
        format!("/addresses/{hash}/transactions?cursor=0:-1"),
        "/transactions?cursor=-1:0".to_string(),
    ] {
        assert_bad_request(&app, &path, "a negative transaction cursor").await;
    }
}

/// A cursor that is not two colon-separated integers must be an error, not a
/// silent reset to page 1 — a client that resends it would page forever.
#[tokio::test]
async fn test_block_tx_cursors_reject_malformed_shapes() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for cursor in ["zzz", "1", "1:2:3", "abc:def"] {
        assert_bad_request(
            &app,
            &format!("/activities?cursor={cursor}"),
            "a malformed activity cursor",
        )
        .await;
        assert_bad_request(
            &app,
            &format!("/addresses/{hash}/transactions?cursor={cursor}"),
            "a malformed transaction cursor",
        )
        .await;
    }
}

/// `?cursor=` is the query-string spelling of "absent" and must stay page 1 —
/// the strictness above is about values that cannot mean anything, not about
/// breaking clients that interpolate `cursor ?? ''`.
#[tokio::test]
async fn test_empty_cursor_is_page_one_everywhere() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for path in [
        "/activities?cursor=".to_string(),
        format!("/addresses/{hash}/activities?cursor="),
        format!("/addresses/{hash}/transactions?cursor="),
        "/transactions?cursor=".to_string(),
    ] {
        let (status, body) = get_json(&app, &path).await;
        assert_ne!(
            status,
            StatusCode::BAD_REQUEST,
            "GET /api/v1{path} must treat an empty cursor as page 1, got {status} body={body}"
        );
    }
}

// ---------------------------------------------------------------------------
// Negative block numbers — the process-abort family.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_block_endpoints_reject_negative_block_number() {
    let app = test_app().await;
    for path in [
        "/blocks/-1",
        "/blocks/-1/fee-stats",
        "/blocks/-1/proposals",
        "/graph/proposals/-1",
        "/transactions?block_number=-1",
    ] {
        assert_bad_request(&app, path, "a negative block number").await;
    }
}

/// Free-text search forwards whatever the user typed. A negative number is
/// simply not a block number, so the block lookup must not run — and must not
/// abort the process on the way.
#[tokio::test]
async fn test_search_survives_a_negative_number_query() {
    let app = test_app().await;
    for query in ["-1", "-9223372036854775808"] {
        let (status, body) = get_json(&app, &format!("/search?q={query}")).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET /api/v1/search?q={query} must answer, got {status} body={body}"
        );
        assert_eq!(
            body["results"].as_array().map(|r| r.len()),
            Some(0),
            "a negative number matches no block, got body={body}"
        );
    }
}

/// Genesis must stay reachable — the fix rejects `< 0`, not `<= 0`.
#[tokio::test]
async fn test_block_zero_is_still_a_valid_lookup() {
    let app = test_app().await;
    let (status, _) = get_json(&app, "/blocks/0").await;
    assert_ne!(
        status,
        StatusCode::BAD_REQUEST,
        "block 0 is genesis and must not be rejected as malformed"
    );
}

// ---------------------------------------------------------------------------
// Output indexes — the silent-aliasing family.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_outpoint_endpoints_reject_negative_output_index() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for template in ["/cells/{H}/-1", "/graph/cell/{H}/-1"] {
        let path = template.replace("{H}", &hash);
        assert_bad_request(&app, &path, "a negative output index").await;
    }
}

/// `65536 as i16` is `0`. Before the fix these served output 0's body — and the
/// graph route echoed `"outputIndex": 65536` over it, labelling another cell's
/// capacity and block as the requested outpoint.
#[tokio::test]
async fn test_outpoint_endpoints_reject_indexes_that_would_alias_another_cell() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for index in ["32768", "65536", "65537"] {
        for template in ["/cells/{H}/{I}", "/graph/cell/{H}/{I}"] {
            let path = template.replace("{H}", &hash).replace("{I}", index);
            assert_bad_request(&app, &path, "an out-of-range output index").await;
        }
    }
}

#[tokio::test]
async fn test_outpoint_endpoints_accept_the_storable_range() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for index in ["0", "32767"] {
        let path = format!("/cells/{hash}/{index}");
        let (status, body) = get_json(&app, &path).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "GET /api/v1{path} is a well-formed outpoint and must reach the store \
             (404 on an empty store), got {status} body={body}"
        );
    }
}

// ---------------------------------------------------------------------------
// Cell index cursors — the silent-truncation family.
// ---------------------------------------------------------------------------

/// `decode_cell_cursor` validated nothing: non-hex silently became "no cursor"
/// (page 1 forever) and a wrong-length hex cursor was used verbatim as the
/// RocksDB seek key, landing outside the range and returning an empty page that
/// reads as "end of results".
#[tokio::test]
async fn test_live_cells_reject_malformed_cursor() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    for cursor in [
        "zzz",                             // not hex
        "0xab",                            // hex, far too short
        &format!("0x{}", "ab".repeat(73)), // hex, one byte short of a cell index key
        &format!("0x{}", "ab".repeat(75)), // hex, one byte long
    ] {
        assert_bad_request(
            &app,
            &format!("/cells/live?lock_script_hash={hash}&cursor={cursor}"),
            "a malformed cell cursor",
        )
        .await;
    }
}

/// A correctly-sized cursor for a *different* script than the one being scanned
/// would seek outside the requested range and silently truncate the page.
#[tokio::test]
async fn test_live_cells_reject_cursor_from_another_script() {
    let app = test_app().await;
    let hash = valid_hash_hex();
    // 74 bytes = script_hash(32) + block_num(8) + tx_hash(32) + output_index(2),
    // but prefixed with a script hash that is not the one being scanned.
    let foreign = format!("0x{}{}", "cd".repeat(32), "00".repeat(42));
    assert_bad_request(
        &app,
        &format!("/cells/live?lock_script_hash={hash}&cursor={foreign}"),
        "a cell cursor belonging to a different script",
    )
    .await;
}
