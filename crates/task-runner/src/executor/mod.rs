use anyhow::Result;
use std::time::Duration;

use crate::db::{DbPool, TaskDb};

mod activities;
mod address_balances;
mod cells_status;
mod cycles;
mod dotbit;
pub mod index;
mod labels;
mod mnft;
mod secondary_issuance;
mod spore;
pub mod statistics;
mod token;
mod tx_block_map;

#[allow(dead_code)]
pub struct TaskExecutor {
    db: TaskDb,
    pool: DbPool,
    database_url: String,
    runner_id: String,
    ckb_rpc_url: String,
    token_labels_path: String,
    index_rebuild_parallel: usize,
    cycles_batch_size: i64,
    cycles_concurrent: usize,
}

impl TaskExecutor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: DbPool,
        database_url: String,
        runner_id: String,
        ckb_rpc_url: String,
        token_labels_path: String,
        index_rebuild_parallel: usize,
        cycles_batch_size: i64,
        cycles_concurrent: usize,
    ) -> Self {
        Self {
            db: TaskDb::new(pool.clone()),
            pool,
            database_url,
            runner_id,
            ckb_rpc_url,
            token_labels_path,
            index_rebuild_parallel,
            cycles_batch_size,
            cycles_concurrent,
        }
    }

    pub async fn run_continuous(&self, _poll_interval: Duration) -> Result<()> {
        Ok(())
    }

    pub async fn run_once(&self) -> Result<bool> {
        Ok(false)
    }
}
