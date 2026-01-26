use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest<T> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'static str,
    pub params: T,
}

impl<T> JsonRpcRequest<T> {
    pub fn new(id: u64, method: &'static str, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcBatchRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

impl JsonRpcBatchRequest {
    pub fn new(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse<T> {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<T>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcBatchResponseItem {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockView {
    pub header: HeaderView,
    pub uncles: Vec<UncleBlockView>,
    pub transactions: Vec<TransactionView>,
    pub proposals: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockResponseWithCycles {
    pub block: BlockView,
    #[serde(default)]
    pub cycles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeaderView {
    pub version: String,
    pub compact_target: String,
    pub timestamp: String,
    pub number: String,
    pub epoch: String,
    pub parent_hash: String,
    pub transactions_root: String,
    pub proposals_hash: String,
    pub extra_hash: String,
    pub dao: String,
    pub nonce: String,
    pub hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UncleBlockView {
    pub header: HeaderView,
    pub proposals: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionView {
    pub hash: String,
    pub version: String,
    pub cell_deps: Vec<CellDep>,
    pub header_deps: Vec<String>,
    pub inputs: Vec<CellInput>,
    pub outputs: Vec<CellOutput>,
    pub outputs_data: Vec<String>,
    pub witnesses: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CellDep {
    pub out_point: OutPoint,
    pub dep_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutPoint {
    pub tx_hash: String,
    pub index: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CellInput {
    pub since: String,
    pub previous_output: OutPoint,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CellOutput {
    pub capacity: String,
    pub lock: Script,
    #[serde(rename = "type")]
    pub type_: Option<Script>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Script {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TipHeader {
    pub version: String,
    pub compact_target: String,
    pub timestamp: String,
    pub number: String,
    pub epoch: String,
    pub parent_hash: String,
    pub transactions_root: String,
    pub proposals_hash: String,
    pub extra_hash: String,
    pub dao: String,
    pub nonce: String,
    pub hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockEconomicState {
    pub issuance: BlockIssuance,
    pub miner_reward: MinerReward,
    pub txs_fee: String,
    pub finalized_at: String,
}

/// Parsed DAO header field (32 bytes) containing economic state
#[derive(Debug, Clone, Default)]
pub struct DaoField {
    /// C: Total issuance (cumulative primary + secondary) in shannons
    pub total_issuance: u64,
    /// AR: Accumulated Rate for DAO interest calculation (scaled by 10^16)
    pub accumulated_rate: u64,
    /// S: Secondary issuance pool (cumulative secondary - miner rewards) in shannons
    pub secondary_pool: u64,
    /// U: Total occupied capacity in shannons
    pub occupied_capacity: u64,
}

impl DaoField {
    /// Parse DAO field from 32-byte hex string (0x-prefixed)
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let bytes = hex::decode(hex).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        Some(Self {
            total_issuance: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            accumulated_rate: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            secondary_pool: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
            occupied_capacity: u64::from_le_bytes(bytes[24..32].try_into().ok()?),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockIssuance {
    pub primary: String,
    pub secondary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinerReward {
    pub primary: String,
    pub secondary: String,
    pub committed: String,
    pub proposal: String,
}

// ============ TxPool Types ============

/// Response from `tx_pool_info` RPC
#[derive(Debug, Clone, Deserialize)]
pub struct TxPoolInfo {
    /// Tip block hash
    pub tip_hash: String,
    /// Tip block number (hex)
    pub tip_number: String,
    /// Count of transactions in the pending state
    pub pending: String,
    /// Count of transactions in the proposed state
    pub proposed: String,
    /// Count of orphan transactions
    pub orphan: String,
    /// Total consumed cycles of all transactions in the pool (hex)
    pub total_tx_cycles: String,
    /// Total serialized size in bytes of all transactions in the pool (hex)
    pub total_tx_size: String,
    /// Min fee rate (shannon per KB) for accepting transactions
    pub min_fee_rate: String,
    /// Min RBF rate (shannon per KB) for replacing transactions
    pub min_rbf_rate: String,
    /// Last updated timestamp (Unix milliseconds, hex)
    pub last_txs_updated_at: String,
    /// Max cycles limit per transaction
    pub tx_size_limit: String,
    /// Max size limit per transaction
    pub max_tx_pool_size: String,
}

/// Response from `get_raw_tx_pool` RPC with verbose=true
#[derive(Debug, Clone, Deserialize)]
pub struct RawTxPoolVerbose {
    /// Transactions in pending state
    pub pending: std::collections::HashMap<String, TxPoolEntry>,
    /// Transactions in proposed state
    pub proposed: std::collections::HashMap<String, TxPoolEntry>,
}

/// Response from `get_raw_tx_pool` RPC with verbose=false (default)
#[derive(Debug, Clone, Deserialize)]
pub struct RawTxPool {
    /// Transaction hashes in pending state
    pub pending: Vec<String>,
    /// Transaction hashes in proposed state
    pub proposed: Vec<String>,
}

/// Detailed entry for a transaction in the txpool
#[derive(Debug, Clone, Deserialize)]
pub struct TxPoolEntry {
    /// Consumed cycles (hex)
    pub cycles: String,
    /// Serialized size in bytes (hex)
    pub size: String,
    /// Transaction fee in shannons (hex)
    pub fee: String,
    /// Ancestors count (hex)
    pub ancestors_count: String,
    /// Ancestors cycles (hex)
    pub ancestors_cycles: String,
    /// Ancestors size (hex)
    pub ancestors_size: String,
    /// Unix timestamp when this tx entered the pool (hex)
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dao_field_from_hex() {
        let hex = "0xc8a7b2766ca2a12e344d9ad50a87230095fed7f93f1b000000b73b6aa83aff06";
        let dao = DaoField::from_hex(hex).unwrap();

        assert_eq!(dao.total_issuance, 3_360_145_383_726_688_200);
        assert_eq!(dao.accumulated_rate, 10_000_104_787_954_996);
        assert_eq!(dao.secondary_pool, 29_961_588_571_797);
        assert_eq!(dao.occupied_capacity, 504_186_178_300_000_000);
    }

    #[test]
    fn test_dao_field_secondary_pool_diff() {
        let prev_hex = "0xeadbf9dd8c36da2ed38b703c79912300dd868544bdc40b000047300381250007";
        let curr_hex = "0x08ceba9cad36da2efb497742799123004f790f05c4c40b0000e4c66e82250007";

        let prev = DaoField::from_hex(prev_hex).unwrap();
        let curr = DaoField::from_hex(curr_hex).unwrap();

        let dao_compensation = curr.secondary_pool - prev.secondary_pool;
        assert_eq!(dao_compensation, 29_000_069_746);
    }

    #[test]
    fn test_dao_field_invalid_length() {
        assert!(DaoField::from_hex("0x1234").is_none());
        assert!(DaoField::from_hex("").is_none());
    }
}
