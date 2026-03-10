use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;

use super::assets::count_nft_collection_activities_cached;
use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";
const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";

/// Decode an identity collection ID from a URL path segment.
///
/// Accepts human-readable aliases ("dotbit", ".bit", "did:ckb", "did_ckb")
/// and hex-encoded sentinel IDs. Rejects any collection ID that does not
/// resolve to one of the two identity sentinels.
fn decode_identity_collection_id(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    let normalized = raw.to_ascii_lowercase();
    if normalized == "dotbit" || normalized == ".bit" {
        return Ok(DOTBIT_SENTINEL_COLLECTION.to_vec());
    }
    if normalized == "did:ckb" || normalized == "did_ckb" {
        return Ok(DID_CKB_SENTINEL_COLLECTION.to_vec());
    }
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request("Invalid identity collection ID"))?;
    if bytes != DOTBIT_SENTINEL_COLLECTION && bytes != DID_CKB_SENTINEL_COLLECTION {
        return Err(ApiError::bad_request(
            "Collection ID is not an identity collection",
        ));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityCollectionDetailResponse {
    pub collection_id: String,
    pub standard: String,
    pub name: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub holders_count: i64,
    pub activities_count: i64,
}

async fn get_identity_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<IdentityCollectionDetailResponse> {
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;

    let agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    if agg.holders_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid identity collection aggregate holders_count: collection_id=0x{}, holders_count={}",
            hex::encode(&collection_id_bytes),
            agg.holders_count
        )));
    }

    let activities_count = count_nft_collection_activities_cached(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &state.mem_cache,
        &collection_id_bytes,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let standard = agg.standard.asset_standard().to_string();
    let name = agg.name;

    ok(IdentityCollectionDetailResponse {
        collection_id: format!("0x{}", hex::encode(&collection_id_bytes)),
        standard,
        name,
        total_count: agg.total_count,
        live_count: agg.live_count,
        holders_count: agg.holders_count,
        activities_count,
    })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route(
        "/assets/identities/{collection_id}",
        get(get_identity_collection),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_identity_collection_id_aliases() {
        let dotbit = decode_identity_collection_id("dotbit").unwrap();
        assert_eq!(dotbit, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let dotbit_alt = decode_identity_collection_id(".bit").unwrap();
        assert_eq!(dotbit_alt, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let did_ckb = decode_identity_collection_id("did:ckb").unwrap();
        assert_eq!(did_ckb, DID_CKB_SENTINEL_COLLECTION.to_vec());

        let did_ckb_alt = decode_identity_collection_id("did_ckb").unwrap();
        assert_eq!(did_ckb_alt, DID_CKB_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_case_insensitive() {
        let result = decode_identity_collection_id("DotBit").unwrap();
        assert_eq!(result, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let result = decode_identity_collection_id("DID:CKB").unwrap();
        assert_eq!(result, DID_CKB_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_hex() {
        let hex_id = format!("0x{}", hex::encode(DOTBIT_SENTINEL_COLLECTION));
        let result = decode_identity_collection_id(&hex_id).unwrap();
        assert_eq!(result, DOTBIT_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_rejects_non_identity() {
        // Random 32-byte hex that isn't an identity sentinel
        let non_identity = "0x".to_string() + &"aa".repeat(32);
        let result = decode_identity_collection_id(&non_identity);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_identity_collection_id_rejects_invalid_hex() {
        let result = decode_identity_collection_id("not_hex_at_all_zzzz");
        assert!(result.is_err());
    }
}
