use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub hash: Vec<u8>,
    pub block_number: i64,
    pub block_hash: Vec<u8>,
    pub index: i32,
    pub version: i32,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub witnesses_count: i32,
    pub cell_deps_count: i32,
    pub header_deps_count: i32,
    pub total_input_capacity: i64,
    pub total_output_capacity: i64,
    pub fee: i64,
    pub is_cellbase: bool,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInput {
    pub tx_hash: Vec<u8>,
    pub index: i32,
    pub previous_tx_hash: Vec<u8>,
    pub previous_index: i32,
    pub since: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionOutput {
    pub tx_hash: Vec<u8>,
    pub index: i32,
    pub capacity: i64,
    pub lock_script_hash: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub data_hash: Vec<u8>,
    pub data_size: i32,
}

impl Transaction {
    pub fn hash_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.hash))
    }
}
