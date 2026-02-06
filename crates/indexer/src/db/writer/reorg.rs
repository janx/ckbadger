#![allow(dead_code)]

use anyhow::Result;

use super::BatchWriter;

#[derive(Debug, Clone, Default)]
pub struct ReorgResult {
    pub event_id: Option<i32>,
    pub reverted_blocks: i64,
    pub reverted_transactions: i64,
    pub reverted_cells: i64,
}

impl BatchWriter {
    pub async fn record_deep_fork(
        &self,
        _fork_point: i64,
        _fork_hash: &[u8],
        _db_tip: i64,
        _db_tip_hash: &[u8],
        _chain_tip: i64,
        _chain_tip_hash: &[u8],
        _depth: i64,
    ) -> Result<i32> {
        Ok(0)
    }

    pub async fn execute_reorg(
        &self,
        _fork_point: i64,
        _fork_hash: &[u8],
        _old_tip: i64,
        _old_tip_hash: &[u8],
        _new_tip: i64,
        _new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        Ok(ReorgResult::default())
    }

    pub async fn resolve_deep_fork(&self) -> Result<()> {
        Ok(())
    }
}
