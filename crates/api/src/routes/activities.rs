use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::types::{AssetAction, AssetChange};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{address::address_to_lock_script_hash, ensure_derived_ready};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/addresses/{addr}/activities", get(get_address_activities))
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    filter: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub ckb_delta: String,
    pub occupied_delta: String,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AssetChangeResponse {
    #[serde(rename = "token")]
    Token {
        type_script_hash: String,
        delta: String,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    #[serde(rename = "dob")]
    Dob {
        dob_id: String,
        standard: String,
        action: String,
    },
    #[serde(rename = "nft")]
    Nft {
        nft_id: String,
        standard: String,
        action: String,
    },
    #[serde(rename = "daoDeposit")]
    DaoDeposit { capacity: String },
    #[serde(rename = "daoWithdrawRequest")]
    DaoWithdrawRequest {
        capacity: String,
        deposit_block: i64,
    },
    #[serde(rename = "daoWithdrawComplete")]
    DaoWithdrawComplete {
        capacity: String,
        compensation: String,
    },
}

fn action_to_string(action: &AssetAction) -> String {
    match action {
        AssetAction::Mint => "mint".to_string(),
        AssetAction::Transfer => "transfer".to_string(),
        AssetAction::Burn => "burn".to_string(),
        AssetAction::Recycle => "recycle".to_string(),
        AssetAction::Renew => "renew".to_string(),
        AssetAction::Update => "update".to_string(),
    }
}

fn convert_asset_change(change: &AssetChange) -> AssetChangeResponse {
    match change {
        AssetChange::Token {
            type_script_hash,
            delta,
            symbol,
            decimals,
        } => AssetChangeResponse::Token {
            type_script_hash: format!("0x{}", hex::encode(type_script_hash)),
            delta: delta.to_string(),
            symbol: symbol.clone(),
            decimals: *decimals,
        },
        AssetChange::Dob {
            dob_id,
            standard,
            action,
        } => AssetChangeResponse::Dob {
            dob_id: format!("0x{}", hex::encode(dob_id)),
            standard: standard.clone(),
            action: action_to_string(action),
        },
        AssetChange::Nft {
            nft_id,
            standard,
            action,
        } => AssetChangeResponse::Nft {
            nft_id: format!("0x{}", hex::encode(nft_id)),
            standard: standard.clone(),
            action: action_to_string(action),
        },
        AssetChange::DaoDeposit { capacity } => AssetChangeResponse::DaoDeposit {
            capacity: capacity.to_string(),
        },
        AssetChange::DaoWithdrawRequest {
            capacity,
            deposit_block,
        } => AssetChangeResponse::DaoWithdrawRequest {
            capacity: capacity.to_string(),
            deposit_block: *deposit_block,
        },
        AssetChange::DaoWithdrawComplete {
            capacity,
            compensation,
        } => AssetChangeResponse::DaoWithdrawComplete {
            capacity: capacity.to_string(),
            compensation: compensation.to_string(),
        },
    }
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    ensure_derived_ready(&state)?;
    let lock_hash = if addr.starts_with("ckb1") || addr.starts_with("ckt1") {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    // Parse cursor: "block_num:tx_idx"
    let cursor = params.cursor.as_ref().and_then(|c| {
        let parts: Vec<&str> = c.split(':').collect();
        if parts.len() == 2 {
            let block_num = parts[0].parse::<i64>().ok()?;
            let tx_idx = parts[1].parse::<i32>().ok()?;
            Some((block_num, tx_idx))
        } else {
            None
        }
    });

    let results = state
        .derived_store
        .list_activities(&lock_hash, limit + 1, cursor, params.filter.as_deref())
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() > limit;
    let page: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let activities: Vec<ActivityResponse> = page
        .into_iter()
        .map(|(_, _, entry)| ActivityResponse {
            tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
            block_number: entry.block_number,
            tx_index: entry.tx_index,
            timestamp: entry.timestamp.to_string(),
            ckb_delta: entry.ckb_delta.to_string(),
            occupied_delta: entry.occupied_delta.to_string(),
            is_cellbase: entry.is_cellbase,
            asset_changes: entry
                .asset_changes
                .iter()
                .map(convert_asset_change)
                .collect(),
            peers: entry
                .peers
                .iter()
                .map(|h| format!("0x{}", hex::encode(h)))
                .collect(),
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        activities,
        limit as i64,
        next_cursor,
    ))
}
