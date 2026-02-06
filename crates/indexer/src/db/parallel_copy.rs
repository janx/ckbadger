use anyhow::Result;
use futures::future::join_all;
use std::collections::HashMap;
use tokio_postgres::Client;

use crate::db::copy_activities::CopyActivitiesWriter;
use crate::db::copy_blocks::CopyBlocksWriter;
use crate::db::copy_cells::CopyCellsWriter;
use crate::db::copy_inputs::{CopyCellDepsWriter, CopyInputsWriter};
use crate::db::copy_live_cells::CopyLiveCellsWriter;
use crate::db::copy_pool::CopyPoolManager;
use crate::db::copy_proposals::CopyProposalsWriter;
use crate::db::copy_transactions::CopyTransactionsWriter;
use crate::db::copy_udt_cells::CopyUdtCellsWriter;
use crate::db::live_cell_storage::{DynLiveCellStorage, LiveCellInfo};
use crate::parser::activity::ParsedActivity;
use crate::parser::block::ParsedBlock;
use crate::parser::cell::ParsedCell;
use crate::parser::transaction::{ParsedCellDep, ParsedInput};
use crate::parser::ParsedUdtCell;
use chrono::{DateTime, Utc};

const PARTITION_SIZE: i64 = 5_000_000;

/// Type alias for cell data tuple: (tx_hash, output_index, cell, block_number)
type CellData<'a> = (&'a [u8], i16, &'a ParsedCell, i64);

/// Type alias for input data tuple: (tx_hash, block_number, input_index, input)
type InputData<'a> = (&'a [u8], i64, i16, &'a ParsedInput);

/// Type alias for cell dep data tuple: (tx_hash, block_number, dep_index, cell_dep)
type CellDepData<'a> = (&'a [u8], i64, i16, &'a ParsedCellDep);

/// Type alias for transaction data tuple
#[allow(clippy::type_complexity)]
type TxData<'a> = (
    &'a [u8],      // hash
    i64,           // block_number
    i32,           // tx_index
    i32,           // version
    i16,           // inputs_count
    i16,           // outputs_count
    i16,           // witnesses_count
    i16,           // cell_deps_count
    i16,           // header_deps_count
    i64,           // total_input_capacity
    i64,           // total_output_capacity
    i64,           // fee
    Option<i32>,   // tx_size
    Option<i64>,   // cycles
    bool,          // is_cellbase
    DateTime<Utc>, // timestamp
);

/// Type alias for block data tuple: (block, total_difficulty)
type BlockData<'a> = (&'a ParsedBlock, i64);

/// Type alias for activity data tuple: (activity, block_number, timestamp)
type ActivityData<'a> = (&'a ParsedActivity, i64, DateTime<Utc>);

/// Type alias for UDT cell data tuple: (tx_hash, output_index, cell, block_number)
type UdtCellData<'a> = (&'a [u8], i16, &'a ParsedUdtCell, i64);

/// Type alias for proposal data tuple: (block_number, proposal_index, proposal_id)
type ProposalData<'a> = (i64, i16, &'a [u8]);

fn get_partition_index(block_number: i64) -> usize {
    (block_number / PARTITION_SIZE) as usize
}

pub struct ParallelCopyRouter {
    pool_manager: CopyPoolManager,
    live_cell_store: Option<DynLiveCellStorage>,
}

impl ParallelCopyRouter {
    pub fn new(pool_manager: CopyPoolManager) -> Self {
        Self {
            pool_manager,
            live_cell_store: None,
        }
    }

    pub fn with_live_cell_store(pool_manager: CopyPoolManager, store: DynLiveCellStorage) -> Self {
        Self {
            pool_manager,
            live_cell_store: Some(store),
        }
    }

    pub fn pool_status(&self) -> crate::db::copy_pool::PoolStatus {
        self.pool_manager.pool_status()
    }

    pub async fn copy_cells_parallel(&self, cells: &[CellData<'_>]) -> Result<u64> {
        if cells.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<CellData<'_>>> = HashMap::new();

        for cell in cells {
            let partition = get_partition_index(cell.3);
            partition_data.entry(partition).or_default().push(*cell);
        }

        let mut futures = Vec::new();

        for (_partition, partition_cells) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyCellsWriter::new();
                for (tx_hash, output_index, cell, block_number) in partition_cells {
                    writer.add_cell(tx_hash, output_index, cell, block_number);
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY cells (tx_hash, output_index, capacity, lock_code_hash, lock_hash_type, lock_args, lock_script_hash, type_code_hash, type_hash_type, type_args, type_script_hash, data_hash, data_size, data, status, created_at_block) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }

    pub async fn copy_live_cells_parallel(&self, cells: &[CellData<'_>]) -> Result<u64> {
        if cells.is_empty() {
            return Ok(0);
        }

        if let Some(ref store) = self.live_cell_store {
            for (tx_hash, output_index, cell, block_number) in cells {
                let info = LiveCellInfo {
                    capacity: cell.capacity,
                    created_at_block: *block_number,
                    lock_script_hash: cell.lock_script_hash.clone(),
                    lock_code_hash: cell.lock_code_hash.clone(),
                    lock_args: cell.lock_args.clone(),
                    type_script_hash: cell.type_script_hash.clone(),
                    type_code_hash: cell.type_code_hash.clone(),
                    data_size: cell.data_size,
                };
                store.insert(tx_hash.to_vec(), *output_index, info);
            }
        }

        let conn = self.pool_manager.get_connection().await?;

        let mut writer = CopyLiveCellsWriter::new();
        for (tx_hash, output_index, cell, block_number) in cells {
            writer.add_live_cell(tx_hash, *output_index, cell, *block_number);
        }

        let data = writer.finish();
        execute_copy(
            conn.as_ref(),
            "COPY live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, type_code_hash, data_size) FROM STDIN WITH (FORMAT BINARY)",
            data,
        ).await
    }

    pub async fn copy_transactions_parallel(&self, txs: &[TxData<'_>]) -> Result<u64> {
        if txs.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<TxData<'_>>> = HashMap::new();

        for tx in txs {
            let partition = get_partition_index(tx.1);
            partition_data.entry(partition).or_default().push(*tx);
        }

        let mut futures = Vec::new();

        for (_partition, partition_txs) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyTransactionsWriter::new();
                for tx in partition_txs {
                    writer.add_transaction(
                        tx.0, tx.1, tx.2, tx.3, tx.4, tx.5, tx.6, tx.7, tx.8, tx.9, tx.10, tx.11,
                        tx.12, tx.13, tx.14, tx.15,
                    );
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY transactions (hash, block_number, tx_index, version, inputs_count, outputs_count, witnesses_count, cell_deps_count, header_deps_count, total_input_capacity, total_output_capacity, fee, tx_size, cycles, is_cellbase, timestamp) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }

    pub async fn copy_inputs_parallel(&self, inputs: &[InputData<'_>]) -> Result<u64> {
        if inputs.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<InputData<'_>>> = HashMap::new();

        for input in inputs {
            let partition = get_partition_index(input.1);
            partition_data.entry(partition).or_default().push(*input);
        }

        let mut futures = Vec::new();

        for (_partition, partition_inputs) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyInputsWriter::new();
                for (tx_hash, block_number, input_index, input) in partition_inputs {
                    writer.add_input(tx_hash, block_number, input_index, input);
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY transaction_inputs (tx_hash, tx_block_number, input_index, previous_tx_hash, previous_output_index, since) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }

    pub async fn copy_cell_deps_parallel(&self, cell_deps: &[CellDepData<'_>]) -> Result<u64> {
        if cell_deps.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<CellDepData<'_>>> = HashMap::new();

        for dep in cell_deps {
            let partition = get_partition_index(dep.1);
            partition_data.entry(partition).or_default().push(*dep);
        }

        let mut futures = Vec::new();

        for (_partition, partition_deps) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyCellDepsWriter::new();
                for (tx_hash, block_number, dep_index, dep) in partition_deps {
                    writer.add_cell_dep(tx_hash, block_number, dep_index, dep);
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY transaction_cell_deps (tx_hash, tx_block_number, dep_index, out_point_tx_hash, out_point_index, dep_type) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }

    pub async fn copy_blocks_parallel(&self, blocks: &[BlockData<'_>]) -> Result<u64> {
        if blocks.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<BlockData<'_>>> = HashMap::new();

        for block in blocks {
            let partition = get_partition_index(block.0.number);
            partition_data.entry(partition).or_default().push(*block);
        }

        let mut futures = Vec::new();

        for (_partition, partition_blocks) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyBlocksWriter::new();
                for (block, total_difficulty) in partition_blocks {
                    writer.add_block(block, total_difficulty);
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY blocks (number, hash, parent_hash, timestamp, version, compact_target, transactions_count, proposals_count, uncles_count, epoch_number, epoch_index, epoch_length, dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash, total_difficulty) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }

    pub async fn copy_activities_parallel(&self, activities: &[ActivityData<'_>]) -> Result<u64> {
        if activities.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<ActivityData<'_>>> = HashMap::new();

        for activity in activities {
            let partition = get_partition_index(activity.1);
            partition_data.entry(partition).or_default().push(*activity);
        }

        let mut futures = Vec::new();

        for (_partition, partition_activities) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyActivitiesWriter::new();
                for (activity, block_number, timestamp) in partition_activities {
                    writer.add_activity(activity, block_number, timestamp);
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY activities (activity_id, activity_type, activity_category, block_number, \
                     tx_hash, tx_index, activity_index, from_lock_hash, to_lock_hash, amount, \
                     asset_id, metadata, timestamp) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }

    pub async fn copy_udt_cells_parallel(&self, udt_cells: &[UdtCellData<'_>]) -> Result<u64> {
        if udt_cells.is_empty() {
            return Ok(0);
        }

        let conn = self.pool_manager.get_connection().await?;

        let mut writer = CopyUdtCellsWriter::new();
        for (tx_hash, output_index, cell, block_number) in udt_cells {
            writer.add_cell(tx_hash, *output_index, cell, *block_number);
        }

        let data = writer.finish();
        execute_copy(
            conn.as_ref(),
            "COPY udt_cells (tx_hash, output_index, type_script_hash, type_code_hash, \
             type_hash_type, type_args, lock_script_hash, amount, standard, is_live, \
             created_at_block) FROM STDIN WITH (FORMAT BINARY)",
            data,
        )
        .await
    }

    pub async fn copy_proposals_parallel(&self, proposals: &[ProposalData<'_>]) -> Result<u64> {
        if proposals.is_empty() {
            return Ok(0);
        }

        let mut partition_data: HashMap<usize, Vec<ProposalData<'_>>> = HashMap::new();

        for proposal in proposals {
            let partition = get_partition_index(proposal.0);
            partition_data.entry(partition).or_default().push(*proposal);
        }

        let mut futures = Vec::new();

        for (_partition, partition_proposals) in partition_data {
            let conn = self.pool_manager.get_connection().await?;

            let future = async move {
                let mut writer = CopyProposalsWriter::new();
                for (block_number, proposal_index, proposal_id) in partition_proposals {
                    writer.add_proposal(block_number, proposal_index, proposal_id);
                }

                let data = writer.finish();
                execute_copy(
                    conn.as_ref(),
                    "COPY block_proposals (block_number, proposal_index, proposal_id) FROM STDIN WITH (FORMAT BINARY)",
                    data,
                ).await
            };

            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut total_rows = 0u64;
        for result in results {
            total_rows += result?;
        }

        Ok(total_rows)
    }
}

async fn execute_copy(client: &Client, query: &str, data: bytes::Bytes) -> Result<u64> {
    use anyhow::Context;
    use futures::SinkExt;
    use std::pin::pin;

    let sink = client
        .copy_in(query)
        .await
        .with_context(|| format!("COPY prepare failed: {}", query))?;
    let mut sink = pin!(sink);
    sink.send(data)
        .await
        .with_context(|| format!("COPY send failed: {}", query))?;
    let rows = sink
        .finish()
        .await
        .with_context(|| format!("COPY finish failed: {}", query))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_index() {
        assert_eq!(get_partition_index(0), 0);
        assert_eq!(get_partition_index(4_999_999), 0);
        assert_eq!(get_partition_index(5_000_000), 1);
        assert_eq!(get_partition_index(9_999_999), 1);
        assert_eq!(get_partition_index(10_000_000), 2);
        assert_eq!(get_partition_index(49_999_999), 9);
    }
}
