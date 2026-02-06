use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i16)]
pub enum CellStatus {
    #[default]
    Live = 0,
    Dead = 1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub id: i64,
    pub tx_hash: Vec<u8>,
    pub output_index: i32,
    pub capacity: i64,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub lock_script_hash: Vec<u8>,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<i16>,
    pub type_args: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub data_hash: Vec<u8>,
    pub data_size: i32,
    pub status: CellStatus,
    pub created_at_block: i64,
    pub consumed_at_block: Option<i64>,
    pub consumed_by_tx: Option<Vec<u8>>,
    pub consumed_at_index: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutPoint {
    pub tx_hash: Vec<u8>,
    pub index: i32,
}

impl OutPoint {
    pub fn new(tx_hash: Vec<u8>, index: i32) -> Self {
        Self { tx_hash, index }
    }

    pub fn tx_hash_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.tx_hash))
    }
}

impl Cell {
    pub fn out_point(&self) -> OutPoint {
        OutPoint::new(self.tx_hash.clone(), self.output_index)
    }

    pub fn is_live(&self) -> bool {
        self.status == CellStatus::Live
    }
}
