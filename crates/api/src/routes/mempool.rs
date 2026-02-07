use axum::{extract::State, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::response::{ok, ApiError, ApiResult};
use crate::rpc::{parse_hex_u64, CkbRpcClient};
use crate::AppState;

// Cache TTLs
const CACHE_TTL_MEMPOOL_SUMMARY_SECS: u64 = 3;

// Cache key prefixes
const CACHE_KEY_MEMPOOL_SUMMARY: &str = "mempool:summary";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mempool/info", get(get_mempool_info))
        .route("/mempool/transactions", get(get_mempool_transactions))
        .route("/mempool/pending-proposals", get(get_pending_proposals))
        .route("/mempool/summary", get(get_mempool_summary))
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

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryBlock {
    number: u64,
    hash: String,
    timestamp: i64,
    transactions_count: u32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryTransaction {
    hash: String,
    tx_index: u32,
    fee: u64,
    tx_size: u32,
    is_cellbase: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MempoolSummaryResponse {
    pending: Vec<MempoolTransaction>,
    proposals: Vec<PendingProposal>,
    tip_block: Option<SummaryBlock>,
    tip_block_txs: Vec<SummaryTransaction>,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockQueryRow {
    number: u64,
    hash: String,
    timestamp: i64,
    transactions_count: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TxQueryRow {
    hash: String,
    tx_index: u32,
    fee: u64,
    tx_size: u32,
    is_cellbase: u8,
}

async fn get_mempool_summary(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MempoolSummaryResponse> {
    if let Some(cached) = state
        .cache
        .get::<MempoolSummaryResponse>(CACHE_KEY_MEMPOOL_SUMMARY)
        .await
    {
        return ok(cached);
    }

    let rpc = CkbRpcClient::new(&state.ckb_rpc_url);

    let (pool_result, pool_info_result) =
        tokio::join!(rpc.get_raw_tx_pool_verbose(), rpc.get_tx_pool_info());

    let pool = pool_result.map_err(|e| ApiError::internal(e.to_string()))?;
    let pool_info = pool_info_result.map_err(|e| ApiError::internal(e.to_string()))?;

    let tip_number =
        parse_hex_u64(&pool_info.tip_number).map_err(|e| ApiError::internal(e.to_string()))?;

    let mut pending = Vec::new();
    for (tx_hash, entry) in &pool.pending {
        let fee = parse_hex_u64(&entry.fee).unwrap_or(0);
        let size = parse_hex_u64(&entry.size).unwrap_or(0);
        let cycles = parse_hex_u64(&entry.cycles).unwrap_or(0);
        let fee_rate = if size > 0 {
            (fee as f64 / size as f64) * 1000.0
        } else {
            0.0
        };

        pending.push(MempoolTransaction {
            tx_hash: tx_hash.clone(),
            fee,
            size,
            cycles,
            fee_rate,
            ancestors_count: parse_hex_u64(&entry.ancestors_count).unwrap_or(0),
            timestamp: parse_hex_u64(&entry.timestamp).unwrap_or(0),
            status: "pending".to_string(),
        });
    }
    pending.sort_by(|a, b| b.fee_rate.partial_cmp(&a.fee_rate).unwrap());

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

    let block_query = r#"
        SELECT 
            b.number as number,
            hex(b.hash) as hash,
            b.timestamp as timestamp,
            b.transactions_count as transactions_count
        FROM blocks_all b
        INNER JOIN canonical_blocks FINAL c ON b.number = c.number AND b.hash = c.block_hash
        ORDER BY b.number DESC
        LIMIT 1
    "#;

    let block_row: Option<BlockQueryRow> = state
        .pool
        .query_one(block_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query tip block: {}", e)))?;

    let tip_block = block_row.as_ref().map(|b| SummaryBlock {
        number: b.number,
        hash: format!("0x{}", b.hash.to_lowercase()),
        timestamp: b.timestamp,
        transactions_count: b.transactions_count,
    });

    let tip_block_txs = if let Some(ref block) = block_row {
        let tx_query = format!(
            r#"
            SELECT 
                hex(t.hash) as hash,
                t.tx_index as tx_index,
                t.fee as fee,
                t.tx_size as tx_size,
                t.is_cellbase as is_cellbase
            FROM transactions_all t
            INNER JOIN canonical_blocks FINAL c ON t.block_number = c.number AND t.block_hash = c.block_hash
            WHERE t.block_number = {}
            ORDER BY t.tx_index ASC
            LIMIT 200
            "#,
            block.number
        );

        let tx_rows: Vec<TxQueryRow> = state.pool.query_all(&tx_query).await.map_err(|e| {
            ApiError::internal(format!("Failed to query block transactions: {}", e))
        })?;

        tx_rows
            .into_iter()
            .map(|t| SummaryTransaction {
                hash: format!("0x{}", t.hash.to_lowercase()),
                tx_index: t.tx_index,
                fee: t.fee,
                tx_size: t.tx_size,
                is_cellbase: t.is_cellbase == 1,
            })
            .collect()
    } else {
        Vec::new()
    };

    let response = MempoolSummaryResponse {
        pending,
        proposals,
        tip_block,
        tip_block_txs,
    };

    state
        .cache
        .set(
            CACHE_KEY_MEMPOOL_SUMMARY,
            &response,
            Duration::from_secs(CACHE_TTL_MEMPOOL_SUMMARY_SECS),
        )
        .await;

    ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_ttl_mempool_summary_is_3_seconds() {
        assert_eq!(CACHE_TTL_MEMPOOL_SUMMARY_SECS, 3);
    }

    #[test]
    fn test_cache_key_mempool_summary_has_correct_prefix() {
        assert!(CACHE_KEY_MEMPOOL_SUMMARY.starts_with("mempool:"));
    }

    #[test]
    fn test_mempool_transaction_serialization() {
        let tx = MempoolTransaction {
            tx_hash: "0xabc123".to_string(),
            fee: 1000,
            size: 500,
            cycles: 10000,
            fee_rate: 2.0,
            ancestors_count: 0,
            timestamp: 1704067200,
            status: "pending".to_string(),
        };
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("txHash"));
        assert!(json.contains("feeRate"));
        assert!(json.contains("ancestorsCount"));
    }

    #[test]
    fn test_pending_proposal_serialization() {
        let proposal = PendingProposal {
            proposal_id: "0xabc123".to_string(),
            full_tx_hash: Some("0xfull123".to_string()),
            proposed_at_block: 12345,
            proposed_at_index: 0,
            blocks_until_expiry: 10,
            fee: Some(1000),
            size: Some(500),
            cycles: Some(10000),
            fee_rate: Some(2.0),
        };
        let json = serde_json::to_string(&proposal).unwrap();
        assert!(json.contains("proposalId"));
        assert!(json.contains("fullTxHash"));
        assert!(json.contains("proposedAtBlock"));
        assert!(json.contains("blocksUntilExpiry"));
    }

    #[test]
    fn test_summary_block_serialization() {
        let block = SummaryBlock {
            number: 12345,
            hash: "0xabc123".to_string(),
            timestamp: 1704067200000,
            transactions_count: 5,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("number"));
        assert!(json.contains("hash"));
        assert!(json.contains("transactionsCount"));
    }

    #[test]
    fn test_summary_transaction_serialization() {
        let tx = SummaryTransaction {
            hash: "0xabc123".to_string(),
            tx_index: 0,
            fee: 1000,
            tx_size: 500,
            is_cellbase: false,
        };
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("hash"));
        assert!(json.contains("txIndex"));
        assert!(json.contains("txSize"));
        assert!(json.contains("isCellbase"));
    }

    #[test]
    fn test_mempool_summary_response_serialization() {
        let response = MempoolSummaryResponse {
            pending: vec![],
            proposals: vec![],
            tip_block: Some(SummaryBlock {
                number: 12345,
                hash: "0xabc".to_string(),
                timestamp: 1704067200000,
                transactions_count: 5,
            }),
            tip_block_txs: vec![],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("pending"));
        assert!(json.contains("proposals"));
        assert!(json.contains("tipBlock"));
        assert!(json.contains("tipBlockTxs"));
    }

    #[test]
    fn test_mempool_summary_response_with_null_tip_block() {
        let response = MempoolSummaryResponse {
            pending: vec![],
            proposals: vec![],
            tip_block: None,
            tip_block_txs: vec![],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"tipBlock\":null"));
    }
}
