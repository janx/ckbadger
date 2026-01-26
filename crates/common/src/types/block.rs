use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use rust_decimal::Decimal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub number: i64,
    pub hash: Vec<u8>,
    pub parent_hash: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub transactions_count: i32,
    pub proposals_count: i32,
    pub uncles_count: i32,
    pub version: i32,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub difficulty: Decimal,
    pub nonce: Vec<u8>,
    pub total_difficulty: Decimal,
    pub miner_lock_hash: Option<Vec<u8>>,
    pub reward: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub number: i64,
    pub hash: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub transactions_count: i32,
}

impl Block {
    pub fn hash_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.hash))
    }

    pub fn parent_hash_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.parent_hash))
    }
}
