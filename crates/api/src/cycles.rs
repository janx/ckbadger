use std::sync::Arc;

use serde::Serialize;

use crate::db::DbPool;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CyclesStatus {
    Done,
    Calculating,
    Queued,
    Failed,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CyclesStatusResponse {
    pub status: CyclesStatus,
    pub cycles: Option<i64>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct CyclesCalculator;

impl CyclesCalculator {
    pub fn new(_pool: DbPool, _ckb_rpc_url: String) -> Arc<Self> {
        Arc::new(Self)
    }

    pub async fn request_calculation(&self, _tx_hash: &str) -> CyclesStatus {
        CyclesStatus::Queued
    }

    pub async fn get_status(&self, _tx_hash: &str) -> CyclesStatus {
        CyclesStatus::NotFound
    }
}
