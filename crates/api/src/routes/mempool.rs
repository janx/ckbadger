use axum::{extract::State, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::rpc::{parse_hex_u64, CkbRpcClient};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mempool/info", get(get_mempool_info))
        .route("/mempool/transactions", get(get_mempool_transactions))
        .route("/mempool/pending-proposals", get(get_pending_proposals))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MempoolInfoResponse {
    pending_count: u64,
    proposed_count: u64,
    orphan_count: u64,
    total_size: u64,
    total_cycles: u64,
    min_fee_rate: u64,
    tip_number: u64,
    tip_hash: String,
    last_updated_at: u64,
}

async fn get_mempool_info(State(state): State<Arc<AppState>>) -> ApiResult<MempoolInfoResponse> {
    let rpc = CkbRpcClient::new(&state.ckb_rpc_url);

    let pool_info = rpc
        .get_tx_pool_info()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(MempoolInfoResponse {
        pending_count: parse_hex_u64(&pool_info.pending)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        proposed_count: parse_hex_u64(&pool_info.proposed)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        orphan_count: parse_hex_u64(&pool_info.orphan)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        total_size: parse_hex_u64(&pool_info.total_tx_size)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        total_cycles: parse_hex_u64(&pool_info.total_tx_cycles)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        min_fee_rate: parse_hex_u64(&pool_info.min_fee_rate)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        tip_number: parse_hex_u64(&pool_info.tip_number)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        tip_hash: pool_info.tip_hash,
        last_updated_at: parse_hex_u64(&pool_info.last_txs_updated_at)
            .map_err(|e| ApiError::internal(e.to_string()))?,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MempoolTransaction {
    tx_hash: String,
    fee: u64,
    size: u64,
    cycles: u64,
    fee_rate: f64,
    ancestors_count: u64,
    timestamp: u64,
    status: String,
}

async fn get_mempool_transactions(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Vec<MempoolTransaction>> {
    let rpc = CkbRpcClient::new(&state.ckb_rpc_url);

    let pool = rpc
        .get_raw_tx_pool_verbose()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut transactions = Vec::new();

    for (tx_hash, entry) in pool.pending {
        let fee = parse_hex_u64(&entry.fee).unwrap_or(0);
        let size = parse_hex_u64(&entry.size).unwrap_or(0);
        let cycles = parse_hex_u64(&entry.cycles).unwrap_or(0);
        let fee_rate = if size > 0 {
            (fee as f64 / size as f64) * 1000.0
        } else {
            0.0
        };

        transactions.push(MempoolTransaction {
            tx_hash,
            fee,
            size,
            cycles,
            fee_rate,
            ancestors_count: parse_hex_u64(&entry.ancestors_count).unwrap_or(0),
            timestamp: parse_hex_u64(&entry.timestamp).unwrap_or(0),
            status: "pending".to_string(),
        });
    }

    for (tx_hash, entry) in pool.proposed {
        let fee = parse_hex_u64(&entry.fee).unwrap_or(0);
        let size = parse_hex_u64(&entry.size).unwrap_or(0);
        let cycles = parse_hex_u64(&entry.cycles).unwrap_or(0);
        let fee_rate = if size > 0 {
            (fee as f64 / size as f64) * 1000.0
        } else {
            0.0
        };

        transactions.push(MempoolTransaction {
            tx_hash,
            fee,
            size,
            cycles,
            fee_rate,
            ancestors_count: parse_hex_u64(&entry.ancestors_count).unwrap_or(0),
            timestamp: parse_hex_u64(&entry.timestamp).unwrap_or(0),
            status: "proposed".to_string(),
        });
    }

    transactions.sort_by(|a, b| b.fee_rate.partial_cmp(&a.fee_rate).unwrap());

    ok(transactions)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingProposal {
    proposal_id: String,
    full_tx_hash: Option<String>,
    proposed_at_block: u64,
    proposed_at_index: u64,
    blocks_until_expiry: i64,
    fee: Option<u64>,
    size: Option<u64>,
    cycles: Option<u64>,
    fee_rate: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingProposalsResponse {
    proposals: Vec<PendingProposal>,
    tip_block_number: u64,
    total_count: usize,
}

async fn get_pending_proposals(
    State(state): State<Arc<AppState>>,
) -> ApiResult<PendingProposalsResponse> {
    let rpc = CkbRpcClient::new(&state.ckb_rpc_url);

    let pool_info = rpc
        .get_tx_pool_info()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let tip_number =
        parse_hex_u64(&pool_info.tip_number).map_err(|e| ApiError::internal(e.to_string()))?;

    let pool = rpc
        .get_raw_tx_pool_verbose()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut proposals: Vec<PendingProposal> = pool
        .proposed
        .into_iter()
        .map(|(tx_hash, entry)| {
            let fee = parse_hex_u64(&entry.fee).ok();
            let size = parse_hex_u64(&entry.size).ok();
            let cycles = parse_hex_u64(&entry.cycles).ok();
            let fee_rate = match (fee, size) {
                (Some(f), Some(s)) if s > 0 => Some((f as f64 / s as f64) * 1000.0),
                _ => None,
            };

            let proposal_id = if tx_hash.len() >= 10 {
                tx_hash[..10].to_string()
            } else {
                tx_hash.clone()
            };

            PendingProposal {
                proposal_id,
                full_tx_hash: Some(tx_hash),
                proposed_at_block: tip_number,
                proposed_at_index: 0,
                blocks_until_expiry: 10,
                fee,
                size,
                cycles,
                fee_rate,
            }
        })
        .collect();

    proposals.sort_by(|a, b| {
        b.fee_rate
            .partial_cmp(&a.fee_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_count = proposals.len();

    ok(PendingProposalsResponse {
        proposals,
        tip_block_number: tip_number,
        total_count,
    })
}
