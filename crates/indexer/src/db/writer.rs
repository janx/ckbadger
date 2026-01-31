#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_is_multiple_of)]

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use tracing::{info, warn};

use ckbadger_common::dao::calculate_estimated_apc;

use crate::parser::{
    block::ParsedBlock,
    cell::ParsedCell,
    transaction::{ParsedCellDep, ParsedInput},
    ParsedClusterCell, ParsedDaoDeposit, ParsedDaoWithdrawRequest, ParsedSporeCell,
    ParsedUdtTransfer,
};

const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;

pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)];
    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)];
    fn block_number(&self) -> i64;
    fn consuming_tx_hash(&self) -> &[u8];
    fn timestamp(&self) -> DateTime<Utc>;
}

/// Dep group format: 4-byte count (u32 LE) + N × 36-byte OutPoints (32 tx_hash + 4 index)
fn looks_like_dep_group(data: &[u8]) -> bool {
    let size = data.len();
    if !(40..=10000).contains(&size) || (size - 4) % 36 != 0 {
        return false;
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
    count > 0 && count <= 256 && count == (size - 4) / 36
}

fn extract_ar_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn extract_total_issuance_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 8 {
        return None;
    }
    let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

#[derive(Debug, Clone, Default)]
pub struct SecondaryIssuanceBreakdown {
    pub secondary_issuance: i64,
    pub miner_secondary: i64,
    pub dao_compensation: i64,
    pub burnt: i64,
}

use crate::cache::CacheInvalidator;

#[derive(Clone)]
pub struct BatchWriter {
    pool: PgPool,
    fast_sync_mode: bool,
    live_cell_store: Option<super::DynLiveCellStorage>,
    cache_invalidator: Option<CacheInvalidator>,
}

impl BatchWriter {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            fast_sync_mode: true,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_fast_sync_mode(pool: PgPool, fast_sync_mode: bool) -> Self {
        Self {
            pool,
            fast_sync_mode,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_live_cell_store(
        pool: PgPool,
        fast_sync_mode: bool,
        live_cell_store: super::DynLiveCellStorage,
        cache_invalidator: CacheInvalidator,
    ) -> Self {
        Self {
            pool,
            fast_sync_mode,
            live_cell_store: Some(live_cell_store),
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn live_cell_store(&self) -> Option<&super::DynLiveCellStorage> {
        self.live_cell_store.as_ref()
    }

    pub async fn begin_transaction(&self) -> Result<Transaction<'_, Postgres>> {
        let mut tx = self.pool.begin().await?;
        if self.fast_sync_mode {
            sqlx::query("SET LOCAL synchronous_commit = off")
                .execute(&mut *tx)
                .await?;
        }
        Ok(tx)
    }

    pub async fn migrate_live_cells(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, 
                lock_script_hash, lock_code_hash, lock_args,
                type_script_hash, type_code_hash, data_size)
            SELECT tx_hash, output_index, created_at_block, capacity::bigint,
                lock_script_hash, lock_code_hash, lock_args,
                type_script_hash, type_code_hash, data_size
            FROM cells
            WHERE status = 0
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn insert_block(&self, block: &ParsedBlock, total_difficulty: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO blocks (
                number, hash, parent_hash, timestamp, version, compact_target,
                transactions_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length,
                dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash,
                total_difficulty
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            ON CONFLICT (number) DO UPDATE SET
                hash = EXCLUDED.hash,
                parent_hash = EXCLUDED.parent_hash,
                timestamp = EXCLUDED.timestamp,
                transactions_count = EXCLUDED.transactions_count
            "#,
        )
        .bind(block.number)
        .bind(&block.hash)
        .bind(&block.parent_hash)
        .bind(block.timestamp)
        .bind(block.version)
        .bind(block.compact_target)
        .bind(block.transactions_count)
        .bind(block.proposals_count)
        .bind(block.uncles_count)
        .bind(block.epoch_number)
        .bind(block.epoch_index)
        .bind(block.epoch_length)
        .bind(&block.dao)
        .bind(&block.nonce)
        .bind(&block.extra_hash)
        .bind(&block.proposals_hash)
        .bind(&block.transactions_root)
        .bind(&block.uncles_hash)
        .bind(total_difficulty)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert multiple blocks in a single batch operation
    pub async fn insert_blocks_batch(&self, blocks: &[&ParsedBlock]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let numbers: Vec<i64> = blocks.iter().map(|b| b.number).collect();
        let hashes: Vec<&[u8]> = blocks.iter().map(|b| b.hash.as_slice()).collect();
        let parent_hashes: Vec<&[u8]> = blocks.iter().map(|b| b.parent_hash.as_slice()).collect();
        let timestamps: Vec<DateTime<Utc>> = blocks.iter().map(|b| b.timestamp).collect();
        let versions: Vec<i32> = blocks.iter().map(|b| b.version).collect();
        let compact_targets: Vec<i64> = blocks.iter().map(|b| b.compact_target).collect();
        let transactions_counts: Vec<i32> = blocks.iter().map(|b| b.transactions_count).collect();
        let proposals_counts: Vec<i32> = blocks.iter().map(|b| b.proposals_count).collect();
        let uncles_counts: Vec<i32> = blocks.iter().map(|b| b.uncles_count).collect();
        let epoch_numbers: Vec<i64> = blocks.iter().map(|b| b.epoch_number).collect();
        let epoch_indices: Vec<i32> = blocks.iter().map(|b| b.epoch_index).collect();
        let epoch_lengths: Vec<i32> = blocks.iter().map(|b| b.epoch_length).collect();
        let daos: Vec<&[u8]> = blocks.iter().map(|b| b.dao.as_slice()).collect();
        let nonces: Vec<&[u8]> = blocks.iter().map(|b| b.nonce.as_slice()).collect();
        let extra_hashes: Vec<&[u8]> = blocks.iter().map(|b| b.extra_hash.as_slice()).collect();
        let proposals_hashes: Vec<&[u8]> =
            blocks.iter().map(|b| b.proposals_hash.as_slice()).collect();
        let transactions_roots: Vec<&[u8]> = blocks
            .iter()
            .map(|b| b.transactions_root.as_slice())
            .collect();
        let uncles_hashes: Vec<&[u8]> = blocks.iter().map(|b| b.uncles_hash.as_slice()).collect();
        let total_difficulties: Vec<i64> = vec![0; blocks.len()];

        sqlx::query(
            r#"
            INSERT INTO blocks (
                number, hash, parent_hash, timestamp, version, compact_target,
                transactions_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length,
                dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash,
                total_difficulty
            )
            SELECT * FROM UNNEST(
                $1::bigint[], $2::bytea[], $3::bytea[], $4::timestamptz[], $5::int[], $6::bigint[],
                $7::int[], $8::int[], $9::int[],
                $10::bigint[], $11::int[], $12::int[],
                $13::bytea[], $14::bytea[], $15::bytea[], $16::bytea[], $17::bytea[], $18::bytea[],
                $19::bigint[]
            )
            ON CONFLICT (number) DO UPDATE SET
                hash = EXCLUDED.hash,
                parent_hash = EXCLUDED.parent_hash,
                timestamp = EXCLUDED.timestamp,
                transactions_count = EXCLUDED.transactions_count
            "#,
        )
        .bind(&numbers)
        .bind(&hashes)
        .bind(&parent_hashes)
        .bind(&timestamps)
        .bind(&versions)
        .bind(&compact_targets)
        .bind(&transactions_counts)
        .bind(&proposals_counts)
        .bind(&uncles_counts)
        .bind(&epoch_numbers)
        .bind(&epoch_indices)
        .bind(&epoch_lengths)
        .bind(&daos)
        .bind(&nonces)
        .bind(&extra_hashes)
        .bind(&proposals_hashes)
        .bind(&transactions_roots)
        .bind(&uncles_hashes)
        .bind(&total_difficulties)
        .execute(&self.pool)
        .await?;

        if let Some(store) = &self.live_cell_store {
            for block in blocks {
                store.insert_block_header(
                    block.number,
                    super::CachedBlockHeader {
                        hash: block.hash.clone(),
                        timestamp: block.timestamp.timestamp_millis(),
                        epoch_number: block.epoch_number,
                        epoch_index: block.epoch_index,
                        epoch_length: block.epoch_length,
                        dao: block.dao.clone(),
                        transactions_count: block.transactions_count,
                    },
                );
            }
        }

        Ok(())
    }

    pub async fn insert_transactions_batch(
        &self,
        txs: &[(
            &[u8],
            i64,
            i32,
            i32,
            i16,
            i16,
            i16,
            i16,
            i16,
            i64,
            i64,
            i64,
            Option<i32>,
            Option<i64>,
            bool,
            DateTime<Utc>,
        )],
    ) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        let hashes: Vec<&[u8]> = txs.iter().map(|t| t.0).collect();
        let block_numbers: Vec<i64> = txs.iter().map(|t| t.1).collect();
        let tx_indices: Vec<i32> = txs.iter().map(|t| t.2).collect();
        let versions: Vec<i32> = txs.iter().map(|t| t.3).collect();
        let inputs_counts: Vec<i16> = txs.iter().map(|t| t.4).collect();
        let outputs_counts: Vec<i16> = txs.iter().map(|t| t.5).collect();
        let witnesses_counts: Vec<i16> = txs.iter().map(|t| t.6).collect();
        let cell_deps_counts: Vec<i16> = txs.iter().map(|t| t.7).collect();
        let header_deps_counts: Vec<i16> = txs.iter().map(|t| t.8).collect();
        let total_input_capacities: Vec<i64> = txs.iter().map(|t| t.9).collect();
        let total_output_capacities: Vec<i64> = txs.iter().map(|t| t.10).collect();
        let fees: Vec<i64> = txs.iter().map(|t| t.11).collect();
        let tx_sizes: Vec<Option<i32>> = txs.iter().map(|t| t.12).collect();
        let cycles: Vec<Option<i64>> = txs.iter().map(|t| t.13).collect();
        let is_cellbases: Vec<bool> = txs.iter().map(|t| t.14).collect();
        let timestamps: Vec<DateTime<Utc>> = txs.iter().map(|t| t.15).collect();

        sqlx::query(
            r#"
            INSERT INTO transactions (
                hash, block_number, tx_index, version,
                inputs_count, outputs_count, witnesses_count, cell_deps_count, header_deps_count,
                total_input_capacity, total_output_capacity, fee, tx_size, cycles, is_cellbase, timestamp
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::bigint[], $3::int[], $4::int[],
                $5::smallint[], $6::smallint[], $7::smallint[], $8::smallint[], $9::smallint[],
                $10::numeric[], $11::numeric[], $12::numeric[], $13::int[], $14::bigint[], $15::bool[], $16::timestamptz[]
            )
            ON CONFLICT (block_number, hash) DO NOTHING
            "#,
        )
        .bind(&hashes)
        .bind(&block_numbers)
        .bind(&tx_indices)
        .bind(&versions)
        .bind(&inputs_counts)
        .bind(&outputs_counts)
        .bind(&witnesses_counts)
        .bind(&cell_deps_counts)
        .bind(&header_deps_counts)
        .bind(&total_input_capacities)
        .bind(&total_output_capacities)
        .bind(&fees)
        .bind(&tx_sizes)
        .bind(&cycles)
        .bind(&is_cellbases)
        .bind(&timestamps)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_cells_batch(
        &self,
        cells: &[(&[u8], i16, &ParsedCell, i64)],
        bulk_sync_mode: bool,
    ) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = cells.iter().map(|(h, _, _, _)| *h).collect();
        let output_indices: Vec<i16> = cells.iter().map(|(_, i, _, _)| *i).collect();
        let capacities: Vec<i64> = cells.iter().map(|(_, _, c, _)| c.capacity).collect();
        let lock_code_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.lock_code_hash.as_slice())
            .collect();
        let lock_hash_types: Vec<i16> = cells.iter().map(|(_, _, c, _)| c.lock_hash_type).collect();
        let lock_args: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.lock_args.as_slice())
            .collect();
        let lock_script_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.lock_script_hash.as_slice())
            .collect();
        let type_code_hashes: Vec<Option<&[u8]>> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_code_hash.as_deref())
            .collect();
        let type_hash_types: Vec<Option<i16>> =
            cells.iter().map(|(_, _, c, _)| c.type_hash_type).collect();
        let type_args: Vec<Option<&[u8]>> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_args.as_deref())
            .collect();
        let type_script_hashes: Vec<Option<&[u8]>> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_script_hash.as_deref())
            .collect();
        let data_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.data_hash.as_slice())
            .collect();
        let data_sizes: Vec<i32> = cells.iter().map(|(_, _, c, _)| c.data_size).collect();
        const CELL_DATA_PREVIEW_SIZE: usize = 512;
        let data_values: Vec<Option<Vec<u8>>> = cells
            .iter()
            .map(|(_, _, c, _)| {
                if c.data.is_empty() {
                    None
                } else {
                    Some(c.data[..c.data.len().min(CELL_DATA_PREVIEW_SIZE)].to_vec())
                }
            })
            .collect();
        let created_at_blocks: Vec<i64> = cells.iter().map(|(_, _, _, b)| *b).collect();

        sqlx::query(
            r#"
            INSERT INTO cells (
                tx_hash, output_index, capacity,
                lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
                type_code_hash, type_hash_type, type_args, type_script_hash,
                data_hash, data_size, data, status, created_at_block
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::smallint[], $3::numeric[],
                $4::bytea[], $5::smallint[], $6::bytea[], $7::bytea[],
                $8::bytea[], $9::smallint[], $10::bytea[], $11::bytea[],
                $12::bytea[], $13::int[], $14::bytea[], array_fill(0::smallint, ARRAY[$15]), $16::bigint[]
            )
            ON CONFLICT (created_at_block, tx_hash, output_index) DO NOTHING
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&capacities)
        .bind(&lock_code_hashes)
        .bind(&lock_hash_types)
        .bind(&lock_args)
        .bind(&lock_script_hashes)
        .bind(&type_code_hashes)
        .bind(&type_hash_types)
        .bind(&type_args)
        .bind(&type_script_hashes)
        .bind(&data_hashes)
        .bind(&data_sizes)
        .bind(&data_values)
        .bind(cells.len() as i32)
        .bind(&created_at_blocks)
        .execute(&self.pool)
        .await?;

        let dep_group_cells: Vec<_> = cells
            .iter()
            .filter(|(_, _, c, _)| {
                c.data.len() > CELL_DATA_PREVIEW_SIZE && looks_like_dep_group(&c.data)
            })
            .collect();

        if !dep_group_cells.is_empty() {
            let dg_tx_hashes: Vec<&[u8]> = dep_group_cells.iter().map(|(h, _, _, _)| *h).collect();
            let dg_indices: Vec<i16> = dep_group_cells.iter().map(|(_, i, _, _)| *i).collect();
            let dg_data: Vec<&[u8]> = dep_group_cells
                .iter()
                .map(|(_, _, c, _)| c.data.as_slice())
                .collect();

            sqlx::query(
                r#"
                INSERT INTO cell_data (tx_hash, output_index, data)
                SELECT * FROM UNNEST($1::bytea[], $2::smallint[], $3::bytea[])
                ON CONFLICT (tx_hash, output_index) DO NOTHING
                "#,
            )
            .bind(&dg_tx_hashes)
            .bind(&dg_indices)
            .bind(&dg_data)
            .execute(&self.pool)
            .await?;
        }

        if let Some(store) = &self.live_cell_store {
            for (tx_hash, output_index, cell, created_at_block) in cells {
                let info = super::LiveCellInfo {
                    capacity: cell.capacity,
                    created_at_block: *created_at_block,
                    lock_script_hash: cell.lock_script_hash.clone(),
                    lock_code_hash: cell.lock_code_hash.clone(),
                    lock_args: cell.lock_args.clone(),
                    type_script_hash: cell.type_script_hash.clone(),
                    type_code_hash: cell.type_code_hash.clone(),
                    data_size: cell.data_size,
                };
                store.insert(tx_hash.to_vec(), *output_index, info);
            }

            if bulk_sync_mode {
                return Ok(());
            }
        }

        sqlx::query(
            r#"
            INSERT INTO live_cells (
                tx_hash, output_index, created_at_block, capacity,
                lock_script_hash, lock_code_hash, lock_args,
                type_script_hash, type_code_hash, data_size
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::smallint[], $3::bigint[], $4::bigint[],
                $5::bytea[], $6::bytea[], $7::bytea[],
                $8::bytea[], $9::bytea[], $10::int[]
            )
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&created_at_blocks)
        .bind(&capacities)
        .bind(&lock_script_hashes)
        .bind(&lock_code_hashes)
        .bind(&lock_args)
        .bind(&type_script_hashes)
        .bind(&type_code_hashes)
        .bind(&data_sizes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_transaction_inputs_batch(
        &self,
        inputs: &[(&[u8], i64, i16, &ParsedInput)],
    ) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _, _, _)| *h).collect();
        let tx_block_numbers: Vec<i64> = inputs.iter().map(|(_, b, _, _)| *b).collect();
        let input_indices: Vec<i16> = inputs.iter().map(|(_, _, i, _)| *i).collect();
        let prev_tx_hashes: Vec<&[u8]> = inputs
            .iter()
            .map(|(_, _, _, inp)| inp.previous_tx_hash.as_slice())
            .collect();
        let prev_output_indices: Vec<i16> = inputs
            .iter()
            .map(|(_, _, _, inp)| inp.previous_output_index as i16)
            .collect();
        let sinces: Vec<i64> = inputs.iter().map(|(_, _, _, inp)| inp.since).collect();

        sqlx::query(
            r#"
            INSERT INTO transaction_inputs (
                tx_hash, tx_block_number, input_index, previous_tx_hash, previous_output_index, since
            )
            SELECT * FROM UNNEST($1::bytea[], $2::bigint[], $3::smallint[], $4::bytea[], $5::smallint[], $6::numeric[])
            ON CONFLICT (tx_block_number, tx_hash, input_index) DO NOTHING
            "#,
        )
        .bind(&tx_hashes)
        .bind(&tx_block_numbers)
        .bind(&input_indices)
        .bind(&prev_tx_hashes)
        .bind(&prev_output_indices)
        .bind(&sinces)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_transaction_cell_deps_batch(
        &self,
        cell_deps: &[(&[u8], i64, i16, &ParsedCellDep)],
    ) -> Result<()> {
        if cell_deps.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = cell_deps.iter().map(|(h, _, _, _)| *h).collect();
        let tx_block_numbers: Vec<i64> = cell_deps.iter().map(|(_, b, _, _)| *b).collect();
        let dep_indices: Vec<i16> = cell_deps.iter().map(|(_, _, i, _)| *i).collect();
        let out_point_tx_hashes: Vec<&[u8]> = cell_deps
            .iter()
            .map(|(_, _, _, dep)| dep.out_point_tx_hash.as_slice())
            .collect();
        let out_point_indices: Vec<i16> = cell_deps
            .iter()
            .map(|(_, _, _, dep)| dep.out_point_index)
            .collect();
        let dep_types: Vec<i16> = cell_deps
            .iter()
            .map(|(_, _, _, dep)| dep.dep_type)
            .collect();

        sqlx::query(
            r#"
            INSERT INTO transaction_cell_deps (
                tx_hash, tx_block_number, dep_index, out_point_tx_hash, out_point_index, dep_type
            )
            SELECT * FROM UNNEST($1::bytea[], $2::bigint[], $3::smallint[], $4::bytea[], $5::smallint[], $6::smallint[])
            ON CONFLICT (tx_block_number, tx_hash, dep_index) DO NOTHING
            "#,
        )
        .bind(&tx_hashes)
        .bind(&tx_block_numbers)
        .bind(&dep_indices)
        .bind(&out_point_tx_hashes)
        .bind(&out_point_indices)
        .bind(&dep_types)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_block_proposals_batch(
        &self,
        block_number: i64,
        proposals: &[Vec<u8>],
    ) -> Result<()> {
        if proposals.is_empty() {
            return Ok(());
        }

        let block_numbers: Vec<i64> = vec![block_number; proposals.len()];
        let proposal_indices: Vec<i16> = (0..proposals.len() as i16).collect();
        let proposal_ids: Vec<&[u8]> = proposals.iter().map(|p| p.as_slice()).collect();

        sqlx::query(
            r#"
            INSERT INTO block_proposals (block_number, proposal_index, proposal_id)
            SELECT * FROM UNNEST($1::bigint[], $2::smallint[], $3::bytea[])
            ON CONFLICT (block_number, proposal_index) DO NOTHING
            "#,
        )
        .bind(&block_numbers)
        .bind(&proposal_indices)
        .bind(&proposal_ids)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_cells_batch(
        &self,
        consumptions: &[(&[u8], i16, i64, &[u8], i64, i16)],
        bulk_sync_mode: bool,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        // Update in-memory store if present
        if let Some(store) = &self.live_cell_store {
            for (tx_hash, output_index, _, _, consumed_at_block, _) in consumptions {
                if let Some(info) = store.remove(tx_hash, *output_index) {
                    store.record_consumption(
                        tx_hash.to_vec(),
                        *output_index,
                        info,
                        *consumed_at_block,
                    );
                }
            }
        }

        // Skip DB operations in bulk sync mode
        if bulk_sync_mode {
            return Ok(());
        }

        const PARTITION_SIZE: i64 = 5_000_000;
        let mut by_partition: std::collections::HashMap<i64, Vec<usize>> =
            std::collections::HashMap::new();

        for (idx, (_, _, created_at_block, _, _, _)) in consumptions.iter().enumerate() {
            let partition_key = *created_at_block / PARTITION_SIZE;
            by_partition.entry(partition_key).or_default().push(idx);
        }

        let mut update_futures = Vec::new();

        for (partition_key, indices) in by_partition.iter() {
            let partition_start = partition_key * PARTITION_SIZE;
            let partition_end = partition_start + PARTITION_SIZE;

            let tx_hashes: Vec<&[u8]> = indices.iter().map(|&i| consumptions[i].0).collect();
            let output_indices: Vec<i16> = indices.iter().map(|&i| consumptions[i].1).collect();
            let created_at_blocks: Vec<i64> = indices.iter().map(|&i| consumptions[i].2).collect();
            let consumed_by_txs: Vec<&[u8]> = indices.iter().map(|&i| consumptions[i].3).collect();
            let consumed_at_blocks: Vec<i64> = indices.iter().map(|&i| consumptions[i].4).collect();
            let consumed_at_indices: Vec<i16> =
                indices.iter().map(|&i| consumptions[i].5).collect();

            let fut = sqlx::query(
                r#"
                UPDATE cells SET
                    status = 1,
                    consumed_at_block = u.consumed_at_block,
                    consumed_by_tx = u.consumed_by_tx,
                    consumed_at_index = u.consumed_at_index
                FROM (
                    SELECT * FROM UNNEST($1::bytea[], $2::smallint[], $3::bigint[], $4::bytea[], $5::bigint[], $6::smallint[])
                    AS t(tx_hash, output_index, created_at_block, consumed_by_tx, consumed_at_block, consumed_at_index)
                ) AS u
                WHERE cells.tx_hash = u.tx_hash 
                  AND cells.output_index = u.output_index 
                  AND cells.created_at_block = u.created_at_block
                  AND cells.status = 0
                  AND cells.created_at_block >= $7
                  AND cells.created_at_block < $8
                "#,
            )
            .bind(tx_hashes)
            .bind(output_indices)
            .bind(created_at_blocks)
            .bind(consumed_by_txs)
            .bind(consumed_at_blocks)
            .bind(consumed_at_indices)
            .bind(partition_start)
            .bind(partition_end)
            .execute(&self.pool);

            update_futures.push(fut);
        }

        let all_tx_hashes: Vec<&[u8]> = consumptions.iter().map(|(h, _, _, _, _, _)| *h).collect();
        let all_output_indices: Vec<i16> =
            consumptions.iter().map(|(_, i, _, _, _, _)| *i).collect();

        let delete_live_cells_fut = sqlx::query(
            r#"
            DELETE FROM live_cells
            WHERE (tx_hash, output_index) IN (
                SELECT * FROM UNNEST($1::bytea[], $2::smallint[])
            )
            "#,
        )
        .bind(&all_tx_hashes)
        .bind(&all_output_indices)
        .execute(&self.pool);

        let (update_results, delete_result) = tokio::join!(
            async {
                let mut results = Vec::with_capacity(update_futures.len());
                for fut in update_futures {
                    results.push(fut.await);
                }
                results
            },
            delete_live_cells_fut
        );

        for result in update_results {
            result?;
        }
        delete_result?;

        Ok(())
    }

    pub async fn update_address_balances_batch(
        &self,
        changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let lock_hashes: Vec<&[u8]> = changes.keys().map(|k| k.as_slice()).collect();
        let balance_changes: Vec<i64> = changes.values().map(|(b, _, _, _, _, _)| *b).collect();
        let live_cell_changes: Vec<i32> = changes.values().map(|(_, l, _, _, _, _)| *l).collect();
        let total_cell_changes: Vec<i32> = changes.values().map(|(_, _, t, _, _, _)| *t).collect();
        let tx_counts: Vec<i64> = changes.values().map(|(_, _, _, c, _, _)| *c).collect();
        let block_numbers: Vec<i64> = changes.values().map(|(_, _, _, _, n, _)| *n).collect();
        let tx_hashes: Vec<&[u8]> = changes.values().map(|(_, _, _, _, _, h)| *h).collect();

        sqlx::query(
            r#"
            WITH input AS (
                SELECT lock_hash, balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash
                FROM UNNEST($1::bytea[], $2::bigint[], $3::int[], $4::int[], $5::bigint[], $6::bigint[], $7::bytea[])
                AS t(lock_hash, balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash)
            )
            MERGE INTO address_balances ab
            USING input i ON ab.lock_script_hash = i.lock_hash
            WHEN MATCHED THEN UPDATE SET
                balance = ab.balance + i.balance_delta,
                live_cells_count = GREATEST(0, ab.live_cells_count + i.live_delta),
                total_cells_count = ab.total_cells_count + i.total_delta,
                transactions_count = ab.transactions_count + i.tx_delta,
                last_activity_block = i.block_num,
                last_activity_tx = i.tx_hash,
                updated_at = NOW()
            WHEN NOT MATCHED THEN INSERT (
                lock_script_hash, balance, live_cells_count, total_cells_count,
                transactions_count, first_seen_block, first_seen_tx,
                last_activity_block, last_activity_tx
            ) VALUES (
                i.lock_hash,
                i.balance_delta,
                GREATEST(0, i.live_delta),
                GREATEST(0, i.total_delta),
                i.tx_delta,
                i.block_num,
                i.tx_hash,
                i.block_num,
                i.tx_hash
            )
            "#,
        )
        .bind(&lock_hashes)
        .bind(&balance_changes)
        .bind(&live_cell_changes)
        .bind(&total_cell_changes)
        .bind(&tx_counts)
        .bind(&block_numbers)
        .bind(&tx_hashes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_address_transactions_batch(
        &self,
        records: &[(Vec<u8>, Vec<u8>, i64, i16, i64, DateTime<Utc>)],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let lock_hashes: Vec<&[u8]> = records
            .iter()
            .map(|(l, _, _, _, _, _)| l.as_slice())
            .collect();
        let tx_hashes: Vec<&[u8]> = records
            .iter()
            .map(|(_, t, _, _, _, _)| t.as_slice())
            .collect();
        let block_numbers: Vec<i64> = records.iter().map(|(_, _, b, _, _, _)| *b).collect();
        let tx_types: Vec<i16> = records.iter().map(|(_, _, _, t, _, _)| *t).collect();
        let capacity_changes: Vec<i64> = records.iter().map(|(_, _, _, _, c, _)| *c).collect();
        let timestamps: Vec<DateTime<Utc>> =
            records.iter().map(|(_, _, _, _, _, ts)| *ts).collect();

        sqlx::query(
            r#"
            INSERT INTO address_transactions (
                lock_script_hash, tx_hash, block_number, tx_type, capacity_change, timestamp
            )
            SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::bigint[], $4::smallint[], $5::numeric[], $6::timestamptz[])
            ON CONFLICT (lock_script_hash, block_number, tx_hash) DO NOTHING
            "#,
        )
        .bind(&lock_hashes)
        .bind(&tx_hashes)
        .bind(&block_numbers)
        .bind(&tx_types)
        .bind(&capacity_changes)
        .bind(&timestamps)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_script_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let code_hashes: Vec<&[u8]> = changes.keys().map(|(h, _)| h.as_slice()).collect();
        let script_kinds: Vec<&str> = changes
            .keys()
            .map(|(_, is_type)| if *is_type { "type" } else { "lock" })
            .collect();
        let cells_count_deltas: Vec<i64> = changes.values().map(|(c, _, _, _)| *c).collect();
        let live_cells_deltas: Vec<i64> = changes.values().map(|(_, l, _, _)| *l).collect();
        let capacity_deltas: Vec<i64> = changes.values().map(|(_, _, c, _)| *c).collect();
        let live_capacity_deltas: Vec<i64> = changes.values().map(|(_, _, _, l)| *l).collect();

        sqlx::query(
            r#"
            INSERT INTO script_usage_stats (
                code_hash, script_kind, cells_count, live_cells_count, capacity_sum, live_capacity_sum
            )
            SELECT code_hash, script_kind, cells_delta, live_delta, cap_delta, live_cap_delta
            FROM UNNEST($1::bytea[], $2::text[], $3::bigint[], $4::bigint[], $5::numeric[], $6::numeric[])
            AS t(code_hash, script_kind, cells_delta, live_delta, cap_delta, live_cap_delta)
            ON CONFLICT (code_hash, script_kind) DO UPDATE SET
                cells_count = script_usage_stats.cells_count + EXCLUDED.cells_count,
                live_cells_count = script_usage_stats.live_cells_count + EXCLUDED.live_cells_count,
                capacity_sum = script_usage_stats.capacity_sum + EXCLUDED.capacity_sum,
                live_capacity_sum = script_usage_stats.live_capacity_sum + EXCLUDED.live_capacity_sum,
                updated_at = NOW()
            "#,
        )
        .bind(&code_hashes)
        .bind(&script_kinds)
        .bind(&cells_count_deltas)
        .bind(&live_cells_deltas)
        .bind(&capacity_deltas)
        .bind(&live_capacity_deltas)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_sync_status(
        &self,
        block_number: i64,
        block_hash: &[u8],
        tx_count: i64,
        cells_created: i64,
        cells_consumed: i64,
        new_addresses: i64,
        ema_rate: Option<f64>,
    ) -> Result<()> {
        if let Some(cache) = &self.cache_invalidator {
            let hash_hex = format!("0x{}", hex::encode(block_hash));
            cache
                .update_sync_status(|status| {
                    status.update_batch(
                        block_number,
                        &hash_hex,
                        tx_count,
                        cells_created,
                        cells_consumed,
                        new_addresses,
                        ema_rate,
                    );
                })
                .await;
        }
        Ok(())
    }

    pub async fn find_last_consistent_block(&self) -> Result<Option<i64>> {
        let row: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
            r#"
            SELECT 
                (SELECT MAX(number) FROM blocks) as max_block,
                (SELECT MAX(block_number) FROM transactions) as max_tx_block
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((Some(max_block), Some(max_tx_block))) => {
                if max_block > max_tx_block {
                    warn!(
                        "Data inconsistency detected: blocks up to {} but transactions only up to {}",
                        max_block, max_tx_block
                    );
                    Ok(Some(max_tx_block))
                } else {
                    Ok(Some(max_block))
                }
            }
            Some((Some(max_block), None)) => {
                warn!(
                    "Data inconsistency: blocks exist up to {} but no transactions found",
                    max_block
                );
                Ok(Some(-1))
            }
            Some((None, _)) => Ok(None),
            None => Ok(None),
        }
    }

    pub async fn init_sync_start(&self, start_block: i64) -> Result<()> {
        let next_block = start_block + 1;
        info!(
            "Cleaning up any partial data from block {} onwards before sync start",
            next_block
        );

        sqlx::query("DELETE FROM transaction_inputs WHERE tx_block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM transaction_cell_deps WHERE tx_block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM live_cells WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM cells WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM transactions WHERE block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM block_proposals WHERE block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM blocks WHERE number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        if let Some(cache) = &self.cache_invalidator {
            cache
                .update_sync_status(|status| {
                    status.init_sync_start(start_block);
                })
                .await;
        }

        info!(
            "Partial data cleanup complete, starting sync from block {}",
            next_block
        );
        Ok(())
    }

    pub async fn cleanup_batch_range(&self, start_block: i64, end_block: i64) -> Result<()> {
        info!(
            "Cleaning up partial batch data for blocks {} to {}",
            start_block, end_block
        );

        sqlx::query(
            "DELETE FROM transaction_inputs WHERE tx_block_number >= $1 AND tx_block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM transaction_cell_deps WHERE tx_block_number >= $1 AND tx_block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM live_cells WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query("DELETE FROM cells WHERE created_at_block >= $1 AND created_at_block <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM transactions WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM block_proposals WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "DELETE FROM address_transactions WHERE block_number >= $1 AND block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query("DELETE FROM activities WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "DELETE FROM udt_cells WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query("DELETE FROM token_transfers WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "DELETE FROM dao_deposits WHERE deposit_block_number >= $1 AND deposit_block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM spore_cells WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM spore_clusters WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM mnft_tokens WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM mnft_classes WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM mnft_issuers WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM dotbit_accounts WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        info!(
            "Batch cleanup complete for blocks {} to {}",
            start_block, end_block
        );
        Ok(())
    }

    pub async fn update_hourly_statistics(
        &self,
        hour: DateTime<Utc>,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO hourly_statistics (
                hour, blocks_count, transactions_count, cells_created, cells_consumed, 
                capacity_transferred
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (hour) DO UPDATE SET
                blocks_count = hourly_statistics.blocks_count + EXCLUDED.blocks_count,
                transactions_count = hourly_statistics.transactions_count + EXCLUDED.transactions_count,
                cells_created = hourly_statistics.cells_created + EXCLUDED.cells_created,
                cells_consumed = hourly_statistics.cells_consumed + EXCLUDED.cells_consumed,
                capacity_transferred = hourly_statistics.capacity_transferred + EXCLUDED.capacity_transferred,
                updated_at = NOW()
            "#,
        )
        .bind(hour)
        .bind(blocks_count)
        .bind(transactions_count)
        .bind(cells_created)
        .bind(cells_consumed)
        .bind(capacity_transferred)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_statistics(
        &self,
        date: NaiveDate,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i64,
        data_size_added: i64,
        data_size_consumed: i64,
    ) -> Result<()> {
        let prev_cumulative = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT COALESCE(total_live_cells, 0), COALESCE(total_data_size, 0)
            FROM daily_statistics
            WHERE date < $1
            ORDER BY date DESC
            LIMIT 1
            "#,
        )
        .bind(date)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or((0, 0));

        let net_cells = (cells_created - cells_consumed) as i64;
        let net_data_size = data_size_added - data_size_consumed;

        sqlx::query(
            r#"
            INSERT INTO daily_statistics (
                date, blocks_count, transactions_count, cells_created, cells_consumed, 
                capacity_transferred, total_live_cells, total_data_size
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (date) DO UPDATE SET
                blocks_count = daily_statistics.blocks_count + EXCLUDED.blocks_count,
                transactions_count = daily_statistics.transactions_count + EXCLUDED.transactions_count,
                cells_created = daily_statistics.cells_created + EXCLUDED.cells_created,
                cells_consumed = daily_statistics.cells_consumed + EXCLUDED.cells_consumed,
                capacity_transferred = daily_statistics.capacity_transferred + EXCLUDED.capacity_transferred,
                total_live_cells = daily_statistics.total_live_cells + $4 - $5,
                total_data_size = daily_statistics.total_data_size + $9
            "#,
        )
        .bind(date)
        .bind(blocks_count)
        .bind(transactions_count)
        .bind(cells_created)
        .bind(cells_consumed)
        .bind(capacity_transferred)
        .bind(prev_cumulative.0 + net_cells)
        .bind(prev_cumulative.1 + net_data_size)
        .bind(net_data_size)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_block_stats(
        &self,
        date: NaiveDate,
        compact_target: i64,
        uncles_count: i32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO daily_block_stats (date, avg_compact_target, block_count, total_uncles, avg_uncle_rate)
            VALUES ($1, $2, 1, $3, $3::float / 1.0)
            ON CONFLICT (date) DO UPDATE SET
                avg_compact_target = ((daily_block_stats.avg_compact_target * daily_block_stats.block_count + $2) / (daily_block_stats.block_count + 1))::bigint,
                block_count = daily_block_stats.block_count + 1,
                total_uncles = daily_block_stats.total_uncles + $3,
                avg_uncle_rate = (daily_block_stats.total_uncles + $3)::float / (daily_block_stats.block_count + 1)::float
            "#,
        )
        .bind(date)
        .bind(compact_target)
        .bind(uncles_count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_miner_statistics(
        &self,
        lock_script_hash: &[u8],
        block_number: i64,
        date: NaiveDate,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
            VALUES ($1, $2, 1, $3)
            ON CONFLICT (date, miner_lock_hash) DO UPDATE SET
                blocks_count = miner_statistics.blocks_count + 1,
                last_block_number = $3
            "#,
        )
        .bind(date)
        .bind(lock_script_hash)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<(i64, i64, Vec<u8>)>> {
        let row = sqlx::query_as::<_, (i64, i64, Vec<u8>)>(
            r#"
            SELECT capacity::bigint, created_at_block, lock_script_hash
            FROM cells 
            WHERE tx_hash = $1 AND output_index = $2
            "#,
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn get_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());
        let mut missing = Vec::new();

        if let Some(store) = &self.live_cell_store {
            let cached = store.get_batch(outpoints);
            for (key, info) in cached {
                result.insert(
                    key,
                    (
                        info.capacity,
                        info.created_at_block,
                        info.lock_script_hash,
                        info.data_size,
                    ),
                );
            }
            for op in outpoints {
                if !result.contains_key(&(op.0.to_vec(), op.1)) {
                    missing.push(*op);
                }
            }

            if !missing.is_empty() {
                let consumed = store.get_consumed_cells_batch(&missing);
                for (key, info) in consumed {
                    result.insert(
                        key.clone(),
                        (
                            info.capacity,
                            info.created_at_block,
                            info.lock_script_hash,
                            info.data_size,
                        ),
                    );
                }
                missing.retain(|op| !result.contains_key(&(op.0.to_vec(), op.1)));
            }

            if !missing.is_empty() {
                tracing::debug!(
                    "LiveCellStore cache miss: {}/{} cells",
                    missing.len(),
                    outpoints.len()
                );
            }
        } else {
            missing.extend(outpoints.iter().copied());
        }

        if !missing.is_empty() {
            let tx_hashes: Vec<&[u8]> = missing.iter().map(|(h, _)| *h).collect();
            let indices: Vec<i16> = missing.iter().map(|(_, i)| *i).collect();

            let rows = sqlx::query_as::<_, (Vec<u8>, i16, i64, i64, Vec<u8>, i32)>(
                r#"
                SELECT lc.tx_hash, lc.output_index, lc.capacity, lc.created_at_block, lc.lock_script_hash, lc.data_size
                FROM live_cells lc
                JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
                  ON lc.tx_hash = t.tx_hash AND lc.output_index = t.output_index
                "#,
            )
            .bind(&tx_hashes)
            .bind(&indices)
            .fetch_all(&self.pool)
            .await?;

            for (tx_hash, idx, cap, block, lock_hash, data_size) in rows {
                result.insert((tx_hash, idx), (cap, block, lock_hash, data_size));
            }
        }

        Ok(result)
    }

    pub async fn get_cells_code_hashes_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());
        let mut missing = Vec::new();

        if let Some(store) = &self.live_cell_store {
            let cached = store.get_batch(outpoints);
            for (key, info) in cached {
                result.insert(key, (info.lock_code_hash, info.type_code_hash));
            }
            for op in outpoints {
                if !result.contains_key(&(op.0.to_vec(), op.1)) {
                    missing.push(*op);
                }
            }

            if !missing.is_empty() {
                let consumed = store.get_consumed_cells_batch(&missing);
                for (key, info) in consumed {
                    result.insert(key.clone(), (info.lock_code_hash, info.type_code_hash));
                }
                missing.retain(|op| !result.contains_key(&(op.0.to_vec(), op.1)));
            }

            if !missing.is_empty() {
                tracing::debug!(
                    "LiveCellStore cache miss: {}/{} cells",
                    missing.len(),
                    outpoints.len()
                );
            }
        } else {
            missing.extend(outpoints.iter().copied());
        }

        if !missing.is_empty() {
            let tx_hashes: Vec<&[u8]> = missing.iter().map(|(h, _)| *h).collect();
            let indices: Vec<i16> = missing.iter().map(|(_, i)| *i).collect();

            let rows = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>, Option<Vec<u8>>)>(
                r#"
                SELECT lc.tx_hash, lc.output_index, lc.lock_code_hash, lc.type_code_hash
                FROM live_cells lc
                JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
                  ON lc.tx_hash = t.tx_hash AND lc.output_index = t.output_index
                "#,
            )
            .bind(&tx_hashes)
            .bind(&indices)
            .fetch_all(&self.pool)
            .await?;

            for (tx_hash, idx, lock_code_hash, type_code_hash) in rows {
                result.insert((tx_hash, idx), (lock_code_hash, type_code_hash));
            }
        }

        Ok(result)
    }

    pub async fn get_udt_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String)>>
    {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _)| *h).collect();
        let indices: Vec<i16> = outpoints.iter().map(|(_, i)| *i).collect();

        let rows = sqlx::query_as::<
            _,
            (
                Vec<u8>,
                i16,
                Vec<u8>,
                Vec<u8>,
                i16,
                Vec<u8>,
                Vec<u8>,
                String,
                String,
            ),
        >(
            r#"
            SELECT tx_hash, output_index, type_script_hash, type_code_hash, 
                   type_hash_type, type_args, lock_script_hash, amount::text, standard
            FROM udt_cells
            JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
              USING (tx_hash, output_index)
            WHERE is_live = TRUE
            "#,
        )
        .bind(&tx_hashes)
        .bind(&indices)
        .fetch_all(&self.pool)
        .await?;

        let mut result = HashMap::with_capacity(rows.len());
        for (
            tx_hash,
            idx,
            type_script_hash,
            type_code_hash,
            type_hash_type,
            type_args,
            lock_script_hash,
            amount_str,
            standard,
        ) in rows
        {
            let amount: u128 = amount_str.parse().unwrap_or(0);
            result.insert(
                (tx_hash, idx),
                (
                    type_script_hash,
                    type_code_hash,
                    type_hash_type,
                    type_args,
                    lock_script_hash,
                    amount,
                    standard,
                ),
            );
        }

        Ok(result)
    }

    pub async fn insert_udt_cells_batch(
        &self,
        cells: &[(&[u8], i16, &crate::parser::ParsedUdtCell, i64)],
    ) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = cells.iter().map(|(h, _, _, _)| *h).collect();
        let output_indices: Vec<i16> = cells.iter().map(|(_, i, _, _)| *i).collect();
        let type_script_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_script_hash.as_slice())
            .collect();
        let type_code_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_code_hash.as_slice())
            .collect();
        let type_hash_types: Vec<i16> = cells.iter().map(|(_, _, c, _)| c.type_hash_type).collect();
        let type_args: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.type_args.as_slice())
            .collect();
        let lock_script_hashes: Vec<&[u8]> = cells
            .iter()
            .map(|(_, _, c, _)| c.lock_script_hash.as_slice())
            .collect();
        let amounts: Vec<String> = cells
            .iter()
            .map(|(_, _, c, _)| c.amount.to_string())
            .collect();
        let standards: Vec<&str> = cells
            .iter()
            .map(|(_, _, c, _)| c.standard.as_str())
            .collect();
        let created_at_blocks: Vec<i64> = cells.iter().map(|(_, _, _, b)| *b).collect();

        sqlx::query(
            r#"
            INSERT INTO udt_cells (
                tx_hash, output_index, type_script_hash, type_code_hash, type_hash_type, type_args,
                lock_script_hash, amount, standard, created_at_block
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::smallint[], $3::bytea[], $4::bytea[], $5::smallint[], $6::bytea[],
                $7::bytea[], $8::numeric[], $9::text[], $10::bigint[]
            )
            ON CONFLICT (tx_hash, output_index) DO UPDATE SET
                lock_script_hash = EXCLUDED.lock_script_hash,
                amount = EXCLUDED.amount,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&type_script_hashes)
        .bind(&type_code_hashes)
        .bind(&type_hash_types)
        .bind(&type_args)
        .bind(&lock_script_hashes)
        .bind(&amounts)
        .bind(&standards)
        .bind(&created_at_blocks)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_udt_cells_batch(
        &self,
        outpoints: &[(&[u8], i16, i64, &[u8])],
    ) -> Result<()> {
        if outpoints.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _, _, _)| *h).collect();
        let output_indices: Vec<i16> = outpoints.iter().map(|(_, i, _, _)| *i).collect();
        let consumed_at_blocks: Vec<i64> = outpoints.iter().map(|(_, _, b, _)| *b).collect();
        let consumed_by_txs: Vec<&[u8]> = outpoints.iter().map(|(_, _, _, t)| *t).collect();

        sqlx::query(
            r#"
            UPDATE udt_cells SET
                is_live = FALSE,
                consumed_at_block = u.consumed_at_block,
                consumed_by_tx = u.consumed_by_tx
            FROM (
                SELECT * FROM UNNEST($1::bytea[], $2::smallint[], $3::bigint[], $4::bytea[])
                AS t(tx_hash, output_index, consumed_at_block, consumed_by_tx)
            ) AS u
            WHERE udt_cells.tx_hash = u.tx_hash 
              AND udt_cells.output_index = u.output_index
              AND udt_cells.is_live = TRUE
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&consumed_at_blocks)
        .bind(&consumed_by_txs)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_block_dao_field(&self, block_number: i64) -> Result<Option<Vec<u8>>> {
        if let Some(store) = &self.live_cell_store {
            if let Some(dao) = store.get_dao_field(block_number) {
                return Ok(Some(dao));
            }
        }

        let row = sqlx::query_as::<_, (Vec<u8>,)>("SELECT dao FROM blocks WHERE number = $1")
            .bind(block_number)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|(dao,)| dao))
    }

    pub async fn insert_dao_deposit(
        &self,
        deposit: &ParsedDaoDeposit,
        block_number: i64,
        timestamp: DateTime<Utc>,
        deposit_ar: i64,
    ) -> Result<()> {
        let inserted: Option<(i64,)> = sqlx::query_as(
            r#"
            INSERT INTO dao_deposits (
                tx_hash, output_index, lock_script_hash, capacity,
                deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, status
            ) VALUES ($1, $2, $3, $4, $5, $1, $6, $7, 0)
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(&deposit.tx_hash)
        .bind(deposit.output_index as i16)
        .bind(&deposit.lock_script_hash)
        .bind(deposit.capacity)
        .bind(block_number)
        .bind(timestamp)
        .bind(deposit_ar)
        .fetch_optional(&self.pool)
        .await?;

        if inserted.is_some() {
            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = total_deposited + $1,
                    active_deposits = active_deposits + 1,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(deposit.capacity)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update_dao_withdraw_request(
        &self,
        request: &ParsedDaoWithdrawRequest,
        block_number: i64,
        timestamp: DateTime<Utc>,
        withdraw_ar: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE dao_deposits SET
                status = 1,
                withdraw_request_block = $3,
                withdraw_request_tx = $4,
                withdraw_request_timestamp = $5,
                withdraw_request_ar = $6
            WHERE tx_hash = $1 AND output_index = $2 AND status = 0
            "#,
        )
        .bind(&request.original_tx_hash)
        .bind(request.original_output_index as i16)
        .bind(block_number)
        .bind(&request.tx_hash)
        .bind(timestamp)
        .bind(withdraw_ar)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn complete_dao_withdrawal(
        &self,
        withdraw_request_tx_hash: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let deposit = sqlx::query_as::<_, (i64, i64, i64, Vec<u8>, i16)>(
            r#"
            SELECT capacity::bigint, deposit_block_number, withdraw_request_block, tx_hash, output_index 
            FROM dao_deposits 
            WHERE withdraw_request_tx = $1 AND status = 1
            "#,
        )
        .bind(withdraw_request_tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((
            capacity,
            deposit_block,
            request_block,
            original_tx_hash,
            original_output_index,
        )) = deposit
        {
            let compensation = self
                .calculate_dao_compensation(capacity, deposit_block, request_block)
                .await?
                .unwrap_or(0);

            sqlx::query(
                r#"
                UPDATE dao_deposits SET
                    status = 2,
                    withdraw_block = $3,
                    withdraw_tx = $4,
                    withdraw_timestamp = $5,
                    compensation = $6
                WHERE tx_hash = $1 AND output_index = $2
                "#,
            )
            .bind(&original_tx_hash)
            .bind(original_output_index)
            .bind(block_number)
            .bind(tx_hash)
            .bind(timestamp)
            .bind(compensation)
            .execute(&self.pool)
            .await?;

            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = GREATEST(0, total_deposited - $1),
                    active_deposits = GREATEST(0, active_deposits - 1),
                    total_compensation_paid = total_compensation_paid + $2,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(capacity)
            .bind(compensation)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Find DAO deposits consumed by inputs. Handles both Phase 1 (matches tx_hash)
    /// and Phase 2 (matches withdraw_request_tx for status=1 records).
    pub async fn find_consumed_dao_deposits(
        &self,
        inputs: &[(&[u8], i32)],
    ) -> Result<Vec<(i64, Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| *h).collect();
        let output_indices: Vec<i16> = inputs.iter().map(|(_, i)| *i as i16).collect();
        let query1 = r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status 
            FROM dao_deposits 
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
        "#;
        let rows1: Vec<(i64, Vec<u8>, i16, String, i64, i16)> = sqlx::query_as(query1)
            .bind(&tx_hashes)
            .bind(&output_indices)
            .fetch_all(&self.pool)
            .await?;

        for row in rows1 {
            seen_ids.insert(row.0);
            results.push(row);
        }

        let query2 = r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status 
            FROM dao_deposits 
            WHERE withdraw_request_tx IN (SELECT * FROM UNNEST($1::bytea[])) AND status = 1
        "#;
        let rows2: Vec<(i64, Vec<u8>, i16, String, i64, i16)> = sqlx::query_as(query2)
            .bind(&tx_hashes)
            .fetch_all(&self.pool)
            .await?;

        for row in rows2 {
            if !seen_ids.contains(&row.0) {
                results.push(row);
            }
        }

        Ok(results)
    }

    pub async fn process_dao_withdrawals(
        &self,
        consumed_dao_deposits: &[(i64, Vec<u8>, i16, String, i64, i16)],
        new_dao_outputs: &[(Vec<u8>, i16, Vec<u8>, i64, u64)],
        block_number: i64,
        consuming_tx_hash: &[u8],
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        for (
            deposit_id,
            _original_tx_hash,
            original_output_index,
            capacity_str,
            deposit_block,
            status,
        ) in consumed_dao_deposits
        {
            let capacity: i64 = capacity_str.parse().unwrap_or(0);

            if *status == 0 {
                let matching_output = new_dao_outputs
                    .iter()
                    .find(|(_, _, _, cap, _)| *cap == capacity);

                if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                    sqlx::query(
                        r#"
                        UPDATE dao_deposits SET
                            status = 1,
                            withdraw_request_block = $3,
                            withdraw_request_tx = $4,
                            withdraw_request_timestamp = $5
                        WHERE id = $1 AND status = 0
                        "#,
                    )
                    .bind(deposit_id)
                    .bind(*original_output_index)
                    .bind(block_number)
                    .bind(new_tx_hash.as_slice())
                    .bind(timestamp)
                    .execute(&self.pool)
                    .await?;
                }
            } else if *status == 1 {
                let withdraw_request_block = sqlx::query_as::<_, (Option<i64>,)>(
                    "SELECT withdraw_request_block FROM dao_deposits WHERE id = $1",
                )
                .bind(deposit_id)
                .fetch_one(&self.pool)
                .await?
                .0
                .unwrap_or(block_number);

                let compensation = self
                    .calculate_dao_compensation(capacity, *deposit_block, withdraw_request_block)
                    .await?;

                sqlx::query(
                    r#"
                    UPDATE dao_deposits SET
                        status = 2,
                        withdraw_block = $2,
                        withdraw_tx = $3,
                        withdraw_timestamp = $4,
                        compensation = $5
                    WHERE id = $1
                    "#,
                )
                .bind(deposit_id)
                .bind(block_number)
                .bind(consuming_tx_hash)
                .bind(timestamp)
                .bind(compensation)
                .execute(&self.pool)
                .await?;

                sqlx::query(
                    r#"
                    UPDATE dao_statistics SET
                        total_deposited = GREATEST(0, total_deposited - $1),
                        active_deposits = GREATEST(0, active_deposits - 1),
                        total_compensation_paid = total_compensation_paid + COALESCE($2, 0),
                        updated_at = NOW()
                    WHERE id = 1
                    "#,
                )
                .bind(capacity)
                .bind(compensation)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn calculate_dao_compensation(
        &self,
        capacity: i64,
        deposit_block: i64,
        withdraw_request_block: i64,
    ) -> Result<Option<i64>> {
        let deposit_dao = self.get_block_dao_field(deposit_block).await?;
        let withdraw_dao = self.get_block_dao_field(withdraw_request_block).await?;

        match (deposit_dao, withdraw_dao) {
            (Some(d), Some(w)) => {
                let ar_deposit = extract_ar_from_dao(&d).unwrap_or(1);
                let ar_withdraw = extract_ar_from_dao(&w).unwrap_or(1);

                if ar_deposit == 0 {
                    return Ok(Some(0));
                }

                let capacity_u128 = capacity as u128;
                let free_capacity = capacity_u128.saturating_sub(DAO_OCCUPIED_CAPACITY as u128);
                let compensation = (free_capacity * ar_withdraw as u128 / ar_deposit as u128)
                    .saturating_sub(free_capacity);

                Ok(Some(compensation as i64))
            }
            _ => Ok(None),
        }
    }

    pub async fn insert_dao_deposits_batch(
        &self,
        deposits: &[(ParsedDaoDeposit, i64, DateTime<Utc>, i64)],
    ) -> Result<()> {
        if deposits.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<&[u8]> = deposits
            .iter()
            .map(|(d, _, _, _)| d.tx_hash.as_slice())
            .collect();
        let output_indices: Vec<i16> = deposits
            .iter()
            .map(|(d, _, _, _)| d.output_index as i16)
            .collect();
        let lock_hashes: Vec<&[u8]> = deposits
            .iter()
            .map(|(d, _, _, _)| d.lock_script_hash.as_slice())
            .collect();
        let capacities: Vec<i64> = deposits.iter().map(|(d, _, _, _)| d.capacity).collect();
        let block_numbers: Vec<i64> = deposits.iter().map(|(_, b, _, _)| *b).collect();
        let timestamps: Vec<DateTime<Utc>> = deposits.iter().map(|(_, _, t, _)| *t).collect();
        let ars: Vec<i64> = deposits.iter().map(|(_, _, _, a)| *a).collect();

        let inserted: Vec<(i64, i64)> = sqlx::query_as(
            r#"
            INSERT INTO dao_deposits (
                tx_hash, output_index, lock_script_hash, capacity,
                deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar, status
            )
            SELECT t.tx_hash, t.output_index, t.lock_script_hash, t.capacity,
                   t.block_number, t.tx_hash, t.timestamp, t.ar, 0
            FROM UNNEST($1::bytea[], $2::smallint[], $3::bytea[], $4::bigint[], $5::bigint[], $6::timestamptz[], $7::bigint[])
            AS t(tx_hash, output_index, lock_script_hash, capacity, block_number, timestamp, ar)
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            RETURNING id, capacity::bigint
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&lock_hashes)
        .bind(&capacities)
        .bind(&block_numbers)
        .bind(&timestamps)
        .bind(&ars)
        .fetch_all(&self.pool)
        .await?;

        if !inserted.is_empty() {
            let total_capacity: i64 = inserted.iter().map(|(_, c)| c).sum();
            let count = inserted.len() as i64;

            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = total_deposited + $1,
                    active_deposits = active_deposits + $2,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(total_capacity)
            .bind(count)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn find_consumed_dao_deposits_batch(
        &self,
        inputs: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, Vec<u8>, i16, String, i64, i16)>> {
        if inputs.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result_map: HashMap<(Vec<u8>, i16), (i64, Vec<u8>, i16, String, i64, i16)> =
            HashMap::new();

        let tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| *h).collect();
        let output_indices: Vec<i16> = inputs.iter().map(|(_, i)| *i).collect();

        let rows1: Vec<(i64, Vec<u8>, i16, String, i64, i16)> = sqlx::query_as(
            r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status
            FROM dao_deposits
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
            "#,
        )
        .bind(&tx_hashes)
        .bind(&output_indices)
        .fetch_all(&self.pool)
        .await?;

        for row in rows1 {
            result_map.insert((row.1.clone(), row.2), row);
        }

        let rows2: Vec<(i64, Vec<u8>, i16, String, i64, i16, Vec<u8>)> = sqlx::query_as(
            r#"
            SELECT id, tx_hash, output_index, CAST(capacity AS TEXT), deposit_block_number, status, withdraw_request_tx
            FROM dao_deposits
            WHERE withdraw_request_tx IN (SELECT * FROM UNNEST($1::bytea[])) AND status = 1
            "#,
        )
        .bind(&tx_hashes)
        .fetch_all(&self.pool)
        .await?;

        for row in rows2 {
            let key = (row.6.clone(), 0i16);
            result_map
                .entry(key)
                .or_insert((row.0, row.1, row.2, row.3, row.4, row.5));
        }

        Ok(result_map)
    }

    pub async fn process_dao_withdrawals_batch<T>(&self, contexts: &[T]) -> Result<()>
    where
        T: DaoWithdrawalContextTrait,
    {
        if contexts.is_empty() {
            return Ok(());
        }

        let mut phase1_updates: Vec<(i64, i64, Vec<u8>, DateTime<Utc>)> = Vec::new();
        let mut phase2_updates: Vec<(i64, i64, Vec<u8>, DateTime<Utc>, i64, i64)> = Vec::new();
        let mut total_withdrawn_capacity: i64 = 0;
        let mut total_compensation: i64 = 0;
        let mut completed_count: i64 = 0;

        let mut all_deposit_blocks: HashSet<i64> = HashSet::new();
        let mut all_request_blocks: HashSet<i64> = HashSet::new();

        for ctx in contexts {
            for (_, _, _, _, deposit_block, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    all_deposit_blocks.insert(*deposit_block);
                }
            }
        }

        for ctx in contexts {
            for (deposit_id, _, _, _, _, status) in ctx.consumed_deposits() {
                if *status == 1 {
                    let request_block: Option<i64> = sqlx::query_scalar(
                        "SELECT withdraw_request_block FROM dao_deposits WHERE id = $1",
                    )
                    .bind(deposit_id)
                    .fetch_optional(&self.pool)
                    .await?
                    .flatten();

                    if let Some(rb) = request_block {
                        all_request_blocks.insert(rb);
                    }
                }
            }
        }

        let all_blocks: Vec<i64> = all_deposit_blocks
            .union(&all_request_blocks)
            .copied()
            .collect();
        let dao_fields: HashMap<i64, Vec<u8>> = if !all_blocks.is_empty() {
            let mut result = HashMap::new();
            let mut missing = all_blocks.clone();

            if let Some(store) = &self.live_cell_store {
                let cached = store.get_dao_fields_batch(&all_blocks);
                for (block_num, dao) in cached {
                    result.insert(block_num, dao);
                }
                missing.retain(|n| !result.contains_key(n));
            }

            if !missing.is_empty() {
                let rows: Vec<(i64, Vec<u8>)> =
                    sqlx::query_as("SELECT number, dao FROM blocks WHERE number = ANY($1)")
                        .bind(&missing)
                        .fetch_all(&self.pool)
                        .await?;
                for (block_num, dao) in rows {
                    result.insert(block_num, dao);
                }
            }

            result
        } else {
            HashMap::new()
        };

        for ctx in contexts {
            for (deposit_id, _, _, capacity_str, deposit_block, status) in ctx.consumed_deposits() {
                let capacity: i64 = capacity_str.parse().unwrap_or(0);

                if *status == 0 {
                    let matching_output = ctx
                        .new_dao_outputs()
                        .iter()
                        .find(|(_, _, _, cap, _)| *cap == capacity);

                    if let Some((new_tx_hash, _, _, _, _)) = matching_output {
                        phase1_updates.push((
                            *deposit_id,
                            ctx.block_number(),
                            new_tx_hash.clone(),
                            ctx.timestamp(),
                        ));
                    }
                } else if *status == 1 {
                    let request_block: i64 = sqlx::query_scalar(
                        "SELECT withdraw_request_block FROM dao_deposits WHERE id = $1",
                    )
                    .bind(deposit_id)
                    .fetch_optional(&self.pool)
                    .await?
                    .flatten()
                    .unwrap_or(ctx.block_number());

                    let compensation = if let (Some(dep_dao), Some(req_dao)) = (
                        dao_fields.get(deposit_block),
                        dao_fields.get(&request_block),
                    ) {
                        let ar_deposit = extract_ar_from_dao(dep_dao).unwrap_or(1);
                        let ar_withdraw = extract_ar_from_dao(req_dao).unwrap_or(1);
                        if ar_deposit > 0 {
                            let cap_u128 = capacity as u128;
                            let free = cap_u128.saturating_sub(DAO_OCCUPIED_CAPACITY as u128);
                            Some(
                                ((free * ar_withdraw as u128 / ar_deposit as u128)
                                    .saturating_sub(free)) as i64,
                            )
                        } else {
                            Some(0)
                        }
                    } else {
                        None
                    };

                    phase2_updates.push((
                        *deposit_id,
                        ctx.block_number(),
                        ctx.consuming_tx_hash().to_vec(),
                        ctx.timestamp(),
                        compensation.unwrap_or(0),
                        capacity,
                    ));

                    total_withdrawn_capacity += capacity;
                    total_compensation += compensation.unwrap_or(0);
                    completed_count += 1;
                }
            }
        }

        if !phase1_updates.is_empty() {
            let ids: Vec<i64> = phase1_updates.iter().map(|(id, _, _, _)| *id).collect();
            let blocks: Vec<i64> = phase1_updates.iter().map(|(_, b, _, _)| *b).collect();
            let txs: Vec<&[u8]> = phase1_updates
                .iter()
                .map(|(_, _, tx, _)| tx.as_slice())
                .collect();
            let timestamps: Vec<DateTime<Utc>> =
                phase1_updates.iter().map(|(_, _, _, t)| *t).collect();

            sqlx::query(
                r#"
                UPDATE dao_deposits d SET
                    status = 1,
                    withdraw_request_block = v.block_number,
                    withdraw_request_tx = v.tx_hash,
                    withdraw_request_timestamp = v.timestamp
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::bytea[], $4::timestamptz[])
                      AS t(id, block_number, tx_hash, timestamp)) v
                WHERE d.id = v.id AND d.status = 0
                "#,
            )
            .bind(&ids)
            .bind(&blocks)
            .bind(&txs)
            .bind(&timestamps)
            .execute(&self.pool)
            .await?;
        }

        if !phase2_updates.is_empty() {
            let ids: Vec<i64> = phase2_updates
                .iter()
                .map(|(id, _, _, _, _, _)| *id)
                .collect();
            let blocks: Vec<i64> = phase2_updates.iter().map(|(_, b, _, _, _, _)| *b).collect();
            let txs: Vec<&[u8]> = phase2_updates
                .iter()
                .map(|(_, _, tx, _, _, _)| tx.as_slice())
                .collect();
            let timestamps: Vec<DateTime<Utc>> =
                phase2_updates.iter().map(|(_, _, _, t, _, _)| *t).collect();
            let compensations: Vec<i64> =
                phase2_updates.iter().map(|(_, _, _, _, c, _)| *c).collect();

            sqlx::query(
                r#"
                UPDATE dao_deposits d SET
                    status = 2,
                    withdraw_block = v.block_number,
                    withdraw_tx = v.tx_hash,
                    withdraw_timestamp = v.timestamp,
                    compensation = v.compensation
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::bytea[], $4::timestamptz[], $5::bigint[])
                      AS t(id, block_number, tx_hash, timestamp, compensation)) v
                WHERE d.id = v.id
                "#,
            )
            .bind(&ids)
            .bind(&blocks)
            .bind(&txs)
            .bind(&timestamps)
            .bind(&compensations)
            .execute(&self.pool)
            .await?;

            sqlx::query(
                r#"
                UPDATE dao_statistics SET
                    total_deposited = GREATEST(0, total_deposited - $1),
                    active_deposits = GREATEST(0, active_deposits - $2),
                    total_compensation_paid = total_compensation_paid + $3,
                    updated_at = NOW()
                WHERE id = 1
                "#,
            )
            .bind(total_withdrawn_capacity)
            .bind(completed_count)
            .bind(total_compensation)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn upsert_epoch_statistics(
        &self,
        epoch_number: i64,
        block_number: i64,
        epoch_length: i32,
        timestamp: DateTime<Utc>,
        epoch_index: i32,
        transactions_count: i32,
    ) -> Result<()> {
        if epoch_index == 0 {
            sqlx::query(
                r#"
                INSERT INTO epoch_statistics (
                    epoch_number, start_block, blocks_count, length, 
                    start_timestamp, difficulty, transactions_count
                )
                VALUES ($1, $2, 1, $3, $4, 0, $5)
                ON CONFLICT (epoch_number) DO UPDATE SET
                    blocks_count = epoch_statistics.blocks_count + 1,
                    transactions_count = epoch_statistics.transactions_count + $5,
                    updated_at = NOW()
                "#,
            )
            .bind(epoch_number)
            .bind(block_number)
            .bind(epoch_length)
            .bind(timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE epoch_statistics SET
                    end_block = $2,
                    blocks_count = blocks_count + 1,
                    end_timestamp = $3,
                    transactions_count = transactions_count + $4,
                    updated_at = NOW()
                WHERE epoch_number = $1
                "#,
            )
            .bind(epoch_number)
            .bind(block_number)
            .bind(timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn process_udt_transfer(
        &self,
        transfer: &ParsedUdtTransfer,
        tx_hash: &[u8],
        block_number: i64,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let token_id = self.upsert_token(transfer, block_number, tx_hash).await?;

        if transfer.is_mint {
            sqlx::query(
                "UPDATE tokens SET total_supply = total_supply + $1::numeric WHERE id = $2",
            )
            .bind(transfer.amount.to_string())
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        } else if transfer.is_burn {
            sqlx::query(
                "UPDATE tokens SET total_supply = GREATEST(total_supply - $1::numeric, 0) WHERE id = $2",
            )
            .bind(transfer.amount.to_string())
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        }

        if let Some(ref from_lock) = transfer.from_lock_hash {
            self.update_token_balance(token_id, from_lock, -(transfer.amount as i64), tx_hash)
                .await?;
        }

        if !transfer.to_lock_hash.is_empty() {
            self.update_token_balance(
                token_id,
                &transfer.to_lock_hash,
                transfer.amount as i64,
                tx_hash,
            )
            .await?;
        }

        self.insert_token_transfer(token_id, transfer, tx_hash, block_number, timestamp)
            .await?;

        Ok(())
    }

    async fn upsert_token(
        &self,
        transfer: &ParsedUdtTransfer,
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            INSERT INTO tokens (
                type_script_hash, type_code_hash, type_hash_type, type_args,
                standard, first_seen_block, first_seen_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (type_script_hash) DO UPDATE SET
                transfers_count = tokens.transfers_count + 1,
                updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(&transfer.type_script_hash)
        .bind(&transfer.type_code_hash)
        .bind(transfer.type_hash_type)
        .bind(&transfer.type_args)
        .bind(transfer.standard.as_str())
        .bind(block_number)
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    async fn update_token_balance(
        &self,
        token_id: i64,
        lock_script_hash: &[u8],
        amount_delta: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        if lock_script_hash.is_empty() {
            return Ok(());
        }

        let existing = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, balance::bigint FROM token_balances WHERE token_id = $1 AND lock_script_hash = $2",
        )
        .bind(token_id)
        .bind(lock_script_hash)
        .fetch_optional(&self.pool)
        .await?;

        match existing {
            Some((id, balance)) => {
                let new_balance = (balance + amount_delta).max(0);

                if new_balance == 0 {
                    sqlx::query("DELETE FROM token_balances WHERE id = $1")
                        .bind(id)
                        .execute(&self.pool)
                        .await?;

                    sqlx::query(
                        "UPDATE tokens SET holders_count = holders_count - 1 WHERE id = $1 AND holders_count > 0",
                    )
                    .bind(token_id)
                    .execute(&self.pool)
                    .await?;
                } else {
                    sqlx::query(
                        "UPDATE token_balances SET balance = $1, last_tx = $2, updated_at = NOW() WHERE id = $3",
                    )
                    .bind(new_balance)
                    .bind(tx_hash)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                }
            }
            None => {
                if amount_delta > 0 {
                    sqlx::query(
                        r#"
                        INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
                        VALUES ($1, $2, $3, $4, $4)
                        "#,
                    )
                    .bind(token_id)
                    .bind(lock_script_hash)
                    .bind(amount_delta)
                    .bind(tx_hash)
                    .execute(&self.pool)
                    .await?;

                    sqlx::query(
                        "UPDATE tokens SET holders_count = holders_count + 1 WHERE id = $1",
                    )
                    .bind(token_id)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(())
    }

    async fn insert_token_transfer(
        &self,
        token_id: i64,
        transfer: &ParsedUdtTransfer,
        tx_hash: &[u8],
        block_number: i64,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO token_transfers (
                token_id, tx_hash, block_number, from_lock_hash, to_lock_hash,
                amount, is_mint, is_burn, timestamp
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(token_id)
        .bind(tx_hash)
        .bind(block_number)
        .bind(transfer.from_lock_hash.as_deref())
        .bind(&transfer.to_lock_hash)
        .bind(transfer.amount as i64)
        .bind(transfer.is_mint)
        .bind(transfer.is_burn)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn process_udt_transfers_batch(
        &self,
        transfers: &[(&ParsedUdtTransfer, &[u8], i64, DateTime<Utc>)],
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        // Step 1: Collect unique tokens (first occurrence info for new tokens)
        let mut unique_tokens: HashMap<Vec<u8>, (&ParsedUdtTransfer, i64, Vec<u8>)> =
            HashMap::new();
        for (transfer, tx_hash, block_number, _) in transfers {
            unique_tokens
                .entry(transfer.type_script_hash.clone())
                .or_insert((*transfer, *block_number, tx_hash.to_vec()));
        }

        // Step 2: Batch upsert tokens - get existing + insert new, return all IDs
        let type_script_hashes: Vec<&[u8]> = unique_tokens.keys().map(|k| k.as_slice()).collect();

        // Get existing token IDs
        let existing_tokens: Vec<(Vec<u8>, i64)> = sqlx::query_as(
            "SELECT type_script_hash, id FROM tokens WHERE type_script_hash = ANY($1)",
        )
        .bind(&type_script_hashes)
        .fetch_all(&self.pool)
        .await?;

        let mut token_ids: HashMap<Vec<u8>, i64> = existing_tokens.into_iter().collect();

        // Insert new tokens (ones not in existing)
        let new_tokens: Vec<_> = unique_tokens
            .iter()
            .filter(|(hash, _)| !token_ids.contains_key(*hash))
            .collect();

        if !new_tokens.is_empty() {
            let new_hashes: Vec<&[u8]> = new_tokens.iter().map(|(h, _)| h.as_slice()).collect();
            let new_code_hashes: Vec<&[u8]> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.type_code_hash.as_slice())
                .collect();
            let new_hash_types: Vec<i16> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.type_hash_type)
                .collect();
            let new_args: Vec<&[u8]> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.type_args.as_slice())
                .collect();
            let new_standards: Vec<&str> = new_tokens
                .iter()
                .map(|(_, (t, _, _))| t.standard.as_str())
                .collect();
            let new_blocks: Vec<i64> = new_tokens.iter().map(|(_, (_, b, _))| *b).collect();
            let new_txs: Vec<&[u8]> = new_tokens
                .iter()
                .map(|(_, (_, _, tx))| tx.as_slice())
                .collect();

            let inserted: Vec<(Vec<u8>, i64)> = sqlx::query_as(
                r#"
                INSERT INTO tokens (type_script_hash, type_code_hash, type_hash_type, type_args, standard, first_seen_block, first_seen_tx)
                SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::smallint[], $4::bytea[], $5::text[], $6::bigint[], $7::bytea[])
                ON CONFLICT (type_script_hash) DO NOTHING
                RETURNING type_script_hash, id
                "#,
            )
            .bind(&new_hashes)
            .bind(&new_code_hashes)
            .bind(&new_hash_types)
            .bind(&new_args)
            .bind(&new_standards)
            .bind(&new_blocks)
            .bind(&new_txs)
            .fetch_all(&self.pool)
            .await?;

            for (hash, id) in inserted {
                token_ids.insert(hash, id);
            }

            // Re-fetch any that were already inserted by concurrent process
            let still_missing: Vec<&[u8]> = new_tokens
                .iter()
                .filter(|(h, _)| !token_ids.contains_key(*h))
                .map(|(h, _)| h.as_slice())
                .collect();

            if !still_missing.is_empty() {
                let fetched: Vec<(Vec<u8>, i64)> = sqlx::query_as(
                    "SELECT type_script_hash, id FROM tokens WHERE type_script_hash = ANY($1)",
                )
                .bind(&still_missing)
                .fetch_all(&self.pool)
                .await?;

                for (hash, id) in fetched {
                    token_ids.insert(hash, id);
                }
            }
        }

        // Step 3: Aggregate stats per token (transfer counts, supply changes)
        let mut transfer_counts: HashMap<i64, i64> = HashMap::new();
        let mut supply_changes: HashMap<i64, i128> = HashMap::new();

        for (transfer, _, _, _) in transfers {
            let token_id = token_ids[&transfer.type_script_hash];
            *transfer_counts.entry(token_id).or_default() += 1;

            if transfer.is_mint {
                *supply_changes.entry(token_id).or_default() += transfer.amount as i128;
            } else if transfer.is_burn {
                *supply_changes.entry(token_id).or_default() -= transfer.amount as i128;
            }
        }

        // Step 4: Batch update token stats
        if !transfer_counts.is_empty() {
            let stat_ids: Vec<i64> = transfer_counts.keys().copied().collect();
            let stat_counts: Vec<i64> = stat_ids.iter().map(|id| transfer_counts[id]).collect();
            let stat_supply: Vec<String> = stat_ids
                .iter()
                .map(|id| supply_changes.get(id).copied().unwrap_or(0).to_string())
                .collect();

            sqlx::query(
                r#"
                UPDATE tokens t SET
                    transfers_count = t.transfers_count + v.cnt,
                    total_supply = GREATEST(0, t.total_supply + v.supply::numeric),
                    updated_at = NOW()
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::text[]) AS t(id, cnt, supply)) v
                WHERE t.id = v.id
                "#,
            )
            .bind(&stat_ids)
            .bind(&stat_counts)
            .bind(&stat_supply)
            .execute(&self.pool)
            .await?;
        }

        // Step 5: Aggregate balance changes per (token_id, lock_script_hash)
        // Value: (delta as i128, last_tx)
        let mut balance_changes: HashMap<(i64, Vec<u8>), (i128, Vec<u8>)> = HashMap::new();

        for (transfer, tx_hash, _, _) in transfers {
            let token_id = token_ids[&transfer.type_script_hash];

            if let Some(ref from_lock) = transfer.from_lock_hash {
                if !from_lock.is_empty() {
                    balance_changes
                        .entry((token_id, from_lock.clone()))
                        .and_modify(|(d, t)| {
                            *d -= transfer.amount as i128;
                            *t = tx_hash.to_vec();
                        })
                        .or_insert((-(transfer.amount as i128), tx_hash.to_vec()));
                }
            }

            if !transfer.to_lock_hash.is_empty() {
                balance_changes
                    .entry((token_id, transfer.to_lock_hash.clone()))
                    .and_modify(|(d, t)| {
                        *d += transfer.amount as i128;
                        *t = tx_hash.to_vec();
                    })
                    .or_insert((transfer.amount as i128, tx_hash.to_vec()));
            }
        }

        // Step 6: Apply balance changes in batch
        if !balance_changes.is_empty() {
            self.batch_apply_balance_changes(&balance_changes).await?;
        }

        // Step 7: Batch insert token transfers
        let tr_token_ids: Vec<i64> = transfers
            .iter()
            .map(|(t, _, _, _)| token_ids[&t.type_script_hash])
            .collect();
        let tr_tx_hashes: Vec<&[u8]> = transfers.iter().map(|(_, h, _, _)| *h).collect();
        let tr_blocks: Vec<i64> = transfers.iter().map(|(_, _, b, _)| *b).collect();
        let tr_from: Vec<Option<&[u8]>> = transfers
            .iter()
            .map(|(t, _, _, _)| t.from_lock_hash.as_deref())
            .collect();
        let tr_to: Vec<&[u8]> = transfers
            .iter()
            .map(|(t, _, _, _)| t.to_lock_hash.as_slice())
            .collect();
        let tr_amounts: Vec<i64> = transfers
            .iter()
            .map(|(t, _, _, _)| t.amount as i64)
            .collect();
        let tr_mints: Vec<bool> = transfers.iter().map(|(t, _, _, _)| t.is_mint).collect();
        let tr_burns: Vec<bool> = transfers.iter().map(|(t, _, _, _)| t.is_burn).collect();
        let tr_timestamps: Vec<DateTime<Utc>> = transfers.iter().map(|(_, _, _, ts)| *ts).collect();

        sqlx::query(
            r#"
            INSERT INTO token_transfers (token_id, tx_hash, block_number, from_lock_hash, to_lock_hash, amount, is_mint, is_burn, timestamp)
            SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bigint[], $4::bytea[], $5::bytea[], $6::bigint[], $7::bool[], $8::bool[], $9::timestamptz[])
            "#,
        )
        .bind(&tr_token_ids)
        .bind(&tr_tx_hashes)
        .bind(&tr_blocks)
        .bind(&tr_from)
        .bind(&tr_to)
        .bind(&tr_amounts)
        .bind(&tr_mints)
        .bind(&tr_burns)
        .bind(&tr_timestamps)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batch apply balance changes: fetch existing, compute new values, batch update/insert/delete
    async fn batch_apply_balance_changes(
        &self,
        changes: &HashMap<(i64, Vec<u8>), (i128, Vec<u8>)>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        // Step 1: Get all existing balances in one query
        let keys: Vec<_> = changes.keys().collect();
        let query_tokens: Vec<i64> = keys.iter().map(|(t, _)| *t).collect();
        let query_locks: Vec<&[u8]> = keys.iter().map(|(_, l)| l.as_slice()).collect();

        let existing: Vec<(i64, Vec<u8>, String)> = sqlx::query_as(
            r#"
            SELECT tb.token_id, tb.lock_script_hash, tb.balance::text
            FROM token_balances tb
            INNER JOIN (SELECT * FROM UNNEST($1::bigint[], $2::bytea[]) AS t(token_id, lock_hash)) q
            ON tb.token_id = q.token_id AND tb.lock_script_hash = q.lock_hash
            "#,
        )
        .bind(&query_tokens)
        .bind(&query_locks)
        .fetch_all(&self.pool)
        .await?;

        let existing_map: HashMap<(i64, Vec<u8>), i128> = existing
            .into_iter()
            .map(|(t, l, b)| ((t, l), b.parse::<i128>().unwrap_or(0)))
            .collect();

        // Step 2: Categorize into insert/update/delete
        let mut to_insert: Vec<(i64, Vec<u8>, i128, Vec<u8>)> = Vec::new();
        let mut to_update: Vec<(i64, Vec<u8>, i128, Vec<u8>)> = Vec::new();
        let mut to_delete: Vec<(i64, Vec<u8>)> = Vec::new();
        let mut tokens_with_holder_increase: HashMap<i64, i64> = HashMap::new();
        let mut tokens_with_holder_decrease: HashMap<i64, i64> = HashMap::new();

        for ((token_id, lock_hash), (delta, last_tx)) in changes {
            let key = (*token_id, lock_hash.clone());
            let old_balance = existing_map.get(&key).copied().unwrap_or(0);
            let new_balance = (old_balance + delta).max(0);

            if existing_map.contains_key(&key) {
                // Existing record
                if new_balance == 0 {
                    to_delete.push((*token_id, lock_hash.clone()));
                    *tokens_with_holder_decrease.entry(*token_id).or_default() += 1;
                } else {
                    to_update.push((*token_id, lock_hash.clone(), new_balance, last_tx.clone()));
                }
            } else if new_balance > 0 {
                // New holder
                to_insert.push((*token_id, lock_hash.clone(), new_balance, last_tx.clone()));
                *tokens_with_holder_increase.entry(*token_id).or_default() += 1;
            }
        }

        // Step 3: Batch delete (zero balances)
        if !to_delete.is_empty() {
            let del_tokens: Vec<i64> = to_delete.iter().map(|(t, _)| *t).collect();
            let del_locks: Vec<&[u8]> = to_delete.iter().map(|(_, l)| l.as_slice()).collect();

            sqlx::query(
                r#"
                DELETE FROM token_balances tb
                USING (SELECT * FROM UNNEST($1::bigint[], $2::bytea[]) AS t(token_id, lock_hash)) d
                WHERE tb.token_id = d.token_id AND tb.lock_script_hash = d.lock_hash
                "#,
            )
            .bind(&del_tokens)
            .bind(&del_locks)
            .execute(&self.pool)
            .await?;
        }

        // Step 4: Batch insert (new holders)
        if !to_insert.is_empty() {
            let ins_tokens: Vec<i64> = to_insert.iter().map(|(t, _, _, _)| *t).collect();
            let ins_locks: Vec<&[u8]> = to_insert.iter().map(|(_, l, _, _)| l.as_slice()).collect();
            let ins_balances: Vec<String> =
                to_insert.iter().map(|(_, _, b, _)| b.to_string()).collect();
            let ins_txs: Vec<&[u8]> = to_insert.iter().map(|(_, _, _, t)| t.as_slice()).collect();

            sqlx::query(
                r#"
                INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
                SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::numeric[], $4::bytea[], $4::bytea[])
                ON CONFLICT (token_id, lock_script_hash) DO UPDATE SET
                    balance = EXCLUDED.balance,
                    last_tx = EXCLUDED.last_tx,
                    updated_at = NOW()
                "#,
            )
            .bind(&ins_tokens)
            .bind(&ins_locks)
            .bind(&ins_balances)
            .bind(&ins_txs)
            .execute(&self.pool)
            .await?;
        }

        // Step 5: Batch update (existing holders with changed balance)
        if !to_update.is_empty() {
            let upd_tokens: Vec<i64> = to_update.iter().map(|(t, _, _, _)| *t).collect();
            let upd_locks: Vec<&[u8]> = to_update.iter().map(|(_, l, _, _)| l.as_slice()).collect();
            let upd_balances: Vec<String> =
                to_update.iter().map(|(_, _, b, _)| b.to_string()).collect();
            let upd_txs: Vec<&[u8]> = to_update.iter().map(|(_, _, _, t)| t.as_slice()).collect();

            sqlx::query(
                r#"
                UPDATE token_balances tb SET
                    balance = v.balance::numeric,
                    last_tx = v.last_tx,
                    updated_at = NOW()
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::text[], $4::bytea[]) AS t(token_id, lock_hash, balance, last_tx)) v
                WHERE tb.token_id = v.token_id AND tb.lock_script_hash = v.lock_hash
                "#,
            )
            .bind(&upd_tokens)
            .bind(&upd_locks)
            .bind(&upd_balances)
            .bind(&upd_txs)
            .execute(&self.pool)
            .await?;
        }

        // Step 6: Update holders_count for affected tokens
        let mut holder_changes: HashMap<i64, i64> = HashMap::new();
        for (token_id, inc) in tokens_with_holder_increase {
            *holder_changes.entry(token_id).or_default() += inc;
        }
        for (token_id, dec) in tokens_with_holder_decrease {
            *holder_changes.entry(token_id).or_default() -= dec;
        }

        if !holder_changes.is_empty() {
            let hc_tokens: Vec<i64> = holder_changes.keys().copied().collect();
            let hc_deltas: Vec<i64> = hc_tokens.iter().map(|t| holder_changes[t]).collect();

            sqlx::query(
                r#"
                UPDATE tokens t SET
                    holders_count = GREATEST(0, t.holders_count + v.delta::int),
                    updated_at = NOW()
                FROM (SELECT * FROM UNNEST($1::bigint[], $2::bigint[]) AS t(id, delta)) v
                WHERE t.id = v.id
                "#,
            )
            .bind(&hc_tokens)
            .bind(&hc_deltas)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn insert_spore_cluster(
        &self,
        cluster: &ParsedClusterCell,
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO spore_clusters (
                cluster_id, type_script_hash, name, description, owner_lock_hash,
                created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (cluster_id) DO UPDATE SET
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                updated_at = NOW()
            "#,
        )
        .bind(&cluster.cluster_id)
        .bind(&cluster.type_script_hash)
        .bind(&cluster.name)
        .bind(&cluster.description)
        .bind(&cluster.owner_lock_hash)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_spore_cell(
        &self,
        spore: &ParsedSporeCell,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO spore_cells (
                spore_id, type_script_hash, tx_hash, output_index, cluster_id,
                content_type, content_size, owner_lock_hash, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $3)
            ON CONFLICT (spore_id) DO UPDATE SET
                tx_hash = EXCLUDED.tx_hash,
                output_index = EXCLUDED.output_index,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&spore.spore_id)
        .bind(&spore.type_script_hash)
        .bind(tx_hash)
        .bind(output_index)
        .bind(&spore.cluster_id)
        .bind(&spore.content_type)
        .bind(spore.content.len() as i32)
        .bind(&spore.owner_lock_hash)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        if let Some(ref cluster_id) = spore.cluster_id {
            sqlx::query(
                "UPDATE spore_clusters SET spores_count = spores_count + 1, updated_at = NOW() WHERE cluster_id = $1",
            )
            .bind(cluster_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn insert_spore_content(&self, spore_id: &[u8], content: &[u8]) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO spore_content (spore_id, content)
            VALUES ($1, $2)
            ON CONFLICT (spore_id) DO NOTHING
            "#,
        )
        .bind(spore_id)
        .bind(content)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_spore(
        &self,
        spore_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        let spore = sqlx::query_as::<_, (Option<Vec<u8>>,)>(
            "SELECT cluster_id FROM spore_cells WHERE spore_id = $1",
        )
        .bind(spore_id)
        .fetch_optional(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE spore_cells SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE spore_id = $1
            "#,
        )
        .bind(spore_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        if let Some((Some(cluster_id),)) = spore {
            sqlx::query(
                "UPDATE spore_clusters SET spores_count = GREATEST(0, spores_count - 1), updated_at = NOW() WHERE cluster_id = $1",
            )
            .bind(&cluster_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT spore_id FROM spore_cells WHERE tx_hash = $1 AND output_index = $2",
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id))
    }

    pub async fn update_dao_daily_snapshot(&self, date: NaiveDate) -> Result<()> {
        let stats = sqlx::query_as::<_, (String, i64, String, i64)>(
            r#"
            SELECT 
                COALESCE(SUM(capacity::numeric), 0)::text as total_deposit,
                COUNT(DISTINCT lock_script_hash) as depositors_count,
                COALESCE(SUM(CASE WHEN deposit_timestamp::date = $1 THEN capacity::numeric ELSE 0 END), 0)::text as daily_deposit,
                COUNT(CASE WHEN deposit_timestamp::date = $1 THEN 1 END) as daily_deposit_count
            FROM dao_deposits
            WHERE deposit_timestamp::date <= $1
              AND (withdraw_timestamp IS NULL OR withdraw_timestamp::date > $1)
            "#,
        )
        .bind(date)
        .fetch_one(&self.pool)
        .await?;

        let dao_data = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT dao FROM blocks WHERE timestamp::date = $1 ORDER BY number DESC LIMIT 1",
        )
        .bind(date)
        .fetch_optional(&self.pool)
        .await?;

        let total_issuance = dao_data
            .as_ref()
            .and_then(|(dao,)| {
                if dao.len() >= 8 {
                    let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
                    Some(u64::from_le_bytes(bytes).to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "0".to_string());

        let secondary_issuance = sqlx::query_as::<_, (String, String, String)>(
            r#"
            SELECT 
                COALESCE(SUM(burnt), 0)::text,
                COALESCE(SUM(miner_secondary), 0)::text,
                COALESCE(SUM(dao_compensation), 0)::text
            FROM block_secondary_issuance
            WHERE block_timestamp::date <= $1
            "#,
        )
        .bind(date)
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|_| ("0".to_string(), "0".to_string(), "0".to_string()));

        sqlx::query(
            r#"
            INSERT INTO dao_daily_snapshots (
                date, total_deposit, depositors_count, daily_deposit, daily_deposit_count, 
                total_issuance, cumulative_burnt, cumulative_mining_reward, cumulative_deposit_compensation,
                dao_data
            )
            VALUES ($1, $2::numeric, $3, $4::numeric, $5, $6::numeric, $7, $8, $9, $10)
            ON CONFLICT (date) DO UPDATE SET
                total_deposit = EXCLUDED.total_deposit,
                depositors_count = EXCLUDED.depositors_count,
                daily_deposit = EXCLUDED.daily_deposit,
                daily_deposit_count = EXCLUDED.daily_deposit_count,
                total_issuance = EXCLUDED.total_issuance,
                cumulative_burnt = EXCLUDED.cumulative_burnt,
                cumulative_mining_reward = EXCLUDED.cumulative_mining_reward,
                cumulative_deposit_compensation = EXCLUDED.cumulative_deposit_compensation,
                dao_data = EXCLUDED.dao_data
            "#,
        )
        .bind(date)
        .bind(&stats.0)
        .bind(stats.1 as i32)
        .bind(&stats.2)
        .bind(stats.3 as i32)
        .bind(&total_issuance)
        .bind(&secondary_issuance.0)
        .bind(&secondary_issuance.1)
        .bind(&secondary_issuance.2)
        .bind(dao_data.map(|(d,)| d))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_previous_block_timestamp(
        &self,
        block_number: i64,
    ) -> Result<Option<DateTime<Utc>>> {
        if block_number <= 0 {
            return Ok(None);
        }

        let row =
            sqlx::query_as::<_, (DateTime<Utc>,)>("SELECT timestamp FROM blocks WHERE number = $1")
                .bind(block_number - 1)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(|(ts,)| ts))
    }

    pub async fn get_dao_deposits_at_block(&self, block_number: i64) -> Result<u128> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT COALESCE(SUM(capacity::numeric), 0)::text
            FROM dao_deposits
            WHERE deposit_block_number < $1
              AND (withdraw_block IS NULL OR withdraw_block >= $1)
            "#,
        )
        .bind(block_number)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0.parse().unwrap_or(0))
    }

    pub async fn get_previous_epoch_duration_minutes(
        &self,
        epoch_number: i64,
    ) -> Result<Option<f64>> {
        let row = sqlx::query_as::<_, (f64,)>(
            r#"
            SELECT (EXTRACT(EPOCH FROM (MAX(timestamp) - MIN(timestamp))) / 60.0)::float8
            FROM blocks
            WHERE epoch_number = $1
            "#,
        )
        .bind(epoch_number)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(d,)| d))
    }

    pub async fn get_last_epoch_start(
        &self,
        before_block: i64,
    ) -> Result<Option<(i64, DateTime<Utc>)>> {
        let row = sqlx::query_as::<_, (i64, DateTime<Utc>)>(
            r#"
            SELECT epoch_number, timestamp
            FROM blocks
            WHERE number < $1 AND epoch_index = 0
            ORDER BY number DESC
            LIMIT 1
            "#,
        )
        .bind(before_block)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn update_block_time_distribution(&self, block_time_seconds: i64) -> Result<()> {
        if block_time_seconds < 0 {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO block_time_distribution (bucket_seconds, block_count)
            VALUES ($1, 1)
            ON CONFLICT (bucket_seconds) DO UPDATE SET
                block_count = block_time_distribution.block_count + 1,
                updated_at = NOW()
            "#,
        )
        .bind(block_time_seconds as i32)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_avg_block_time(
        &self,
        date: NaiveDate,
        block_time_ms: i64,
    ) -> Result<()> {
        if block_time_ms < 0 {
            return Ok(());
        }

        // Use incremental average: new_avg = (old_avg * count + new_value) / (count + 1)
        sqlx::query(
            r#"
            UPDATE daily_statistics
            SET avg_block_time_ms = CASE
                WHEN avg_block_time_ms IS NULL THEN $2
                ELSE ((avg_block_time_ms * (blocks_count - 1) + $2) / blocks_count)::integer
            END,
            updated_at = NOW()
            WHERE date = $1
            "#,
        )
        .bind(date)
        .bind(block_time_ms as i32)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_avg_block_time_batch(
        &self,
        date: NaiveDate,
        avg_block_time_ms: i64,
        block_count: i32,
    ) -> Result<()> {
        if block_count <= 0 {
            return Ok(());
        }

        // Batch update: merge new batch avg with existing avg using weighted average
        sqlx::query(
            r#"
            UPDATE daily_statistics
            SET avg_block_time_ms = CASE
                WHEN avg_block_time_ms IS NULL THEN $2
                ELSE ((avg_block_time_ms * (blocks_count - $3) + $2 * $3) / blocks_count)::integer
            END,
            updated_at = NOW()
            WHERE date = $1
            "#,
        )
        .bind(date)
        .bind(avg_block_time_ms as i32)
        .bind(block_count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_epoch_time_distribution(
        &self,
        epoch_number: i64,
        epoch_duration_minutes: f64,
    ) -> Result<()> {
        if epoch_number <= 0 || epoch_duration_minutes < 0.0 {
            return Ok(());
        }

        let bucket_minutes = ((epoch_duration_minutes / 2.0).floor() as i32) * 2;

        sqlx::query(
            r#"
            INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
            VALUES ($1, 1)
            ON CONFLICT (bucket_minutes) DO UPDATE SET
                epoch_count = epoch_time_distribution.epoch_count + 1,
                updated_at = NOW()
            "#,
        )
        .bind(bucket_minutes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_daily_block_stats_batch(
        &self,
        date: NaiveDate,
        avg_compact_target: i64,
        block_count: i32,
        total_uncles: i32,
    ) -> Result<()> {
        let avg_uncle_rate = if block_count > 0 {
            total_uncles as f64 / block_count as f64
        } else {
            0.0
        };

        sqlx::query(
            r#"
            INSERT INTO daily_block_stats (date, avg_compact_target, block_count, total_uncles, avg_uncle_rate)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (date) DO UPDATE SET
                avg_compact_target = ((daily_block_stats.avg_compact_target * daily_block_stats.block_count + $2 * $3) / (daily_block_stats.block_count + $3))::bigint,
                block_count = daily_block_stats.block_count + $3,
                total_uncles = daily_block_stats.total_uncles + $4,
                avg_uncle_rate = (daily_block_stats.total_uncles + $4)::float / (daily_block_stats.block_count + $3)::float
            "#,
        )
        .bind(date)
        .bind(avg_compact_target)
        .bind(block_count)
        .bind(total_uncles)
        .bind(avg_uncle_rate)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_miner_statistics_batch(
        &self,
        lock_script_hash: &[u8],
        last_block_number: i64,
        date: NaiveDate,
        blocks_count: i32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (date, miner_lock_hash) DO UPDATE SET
                blocks_count = miner_statistics.blocks_count + $3,
                last_block_number = GREATEST(miner_statistics.last_block_number, $4)
            "#,
        )
        .bind(date)
        .bind(lock_script_hash)
        .bind(blocks_count)
        .bind(last_block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn upsert_epoch_statistics_batch(
        &self,
        epoch_number: i64,
        start_block: i64,
        end_block: i64,
        epoch_length: i32,
        start_timestamp: DateTime<Utc>,
        end_timestamp: DateTime<Utc>,
        transactions_count: i32,
        is_new: bool,
    ) -> Result<()> {
        if is_new {
            sqlx::query(
                r#"
                INSERT INTO epoch_statistics (
                    epoch_number, start_block, end_block, blocks_count, length, 
                    start_timestamp, end_timestamp, difficulty, transactions_count
                )
                VALUES ($1, $2, $3, $3 - $2 + 1, $4, $5, $6, 0, $7)
                ON CONFLICT (epoch_number) DO UPDATE SET
                    end_block = GREATEST(epoch_statistics.end_block, EXCLUDED.end_block),
                    blocks_count = GREATEST(epoch_statistics.end_block, EXCLUDED.end_block) - epoch_statistics.start_block + 1,
                    end_timestamp = EXCLUDED.end_timestamp,
                    transactions_count = epoch_statistics.transactions_count + EXCLUDED.transactions_count,
                    updated_at = NOW()
                "#,
            )
            .bind(epoch_number)
            .bind(start_block)
            .bind(end_block)
            .bind(epoch_length)
            .bind(start_timestamp)
            .bind(end_timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE epoch_statistics SET
                    end_block = GREATEST(end_block, $2),
                    blocks_count = GREATEST(end_block, $2) - start_block + 1,
                    end_timestamp = $3,
                    transactions_count = transactions_count + $4,
                    updated_at = NOW()
                WHERE epoch_number = $1
                "#,
            )
            .bind(epoch_number)
            .bind(end_block)
            .bind(end_timestamp)
            .bind(transactions_count)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update_block_time_distribution_batch(
        &self,
        bucket_seconds: i32,
        count: i32,
    ) -> Result<()> {
        if bucket_seconds < 0 {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO block_time_distribution (bucket_seconds, block_count)
            VALUES ($1, $2)
            ON CONFLICT (bucket_seconds) DO UPDATE SET
                block_count = block_time_distribution.block_count + $2,
                updated_at = NOW()
            "#,
        )
        .bind(bucket_seconds)
        .bind(count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_epoch_time_distribution_batch(
        &self,
        bucket_minutes: i32,
        count: i32,
    ) -> Result<()> {
        if bucket_minutes < 0 {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
            VALUES ($1, $2)
            ON CONFLICT (bucket_minutes) DO UPDATE SET
                epoch_count = epoch_time_distribution.epoch_count + $2,
                updated_at = NOW()
            "#,
        )
        .bind(bucket_minutes)
        .bind(count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn refresh_token_24h_transfers(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            WITH block_24h_ago AS (
                SELECT COALESCE(
                    (SELECT number FROM blocks 
                     WHERE timestamp >= (SELECT MAX(timestamp) - INTERVAL '24 hours' FROM blocks)
                     ORDER BY number ASC LIMIT 1),
                    0
                ) as block_num
            )
            UPDATE tokens t SET 
                transfers_24h = (
                    SELECT COUNT(*) 
                    FROM cells c
                    WHERE c.type_script_hash = t.type_script_hash
                    AND c.created_at_block >= (SELECT block_num FROM block_24h_ago)
                ),
                updated_at = NOW()
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn get_secondary_issuance_state(&self) -> Result<(u128, u128, u128, u128, i64)> {
        let row = sqlx::query_as::<_, (String, String, String, String, i64)>(
            r#"SELECT 
                COALESCE(cumulative_secondary_issuance, '0'),
                COALESCE(cumulative_miner_secondary, '0'),
                COALESCE(cumulative_dao_compensation, '0'),
                COALESCE(cumulative_burnt, '0'),
                COALESCE(last_processed_block, 0)
            FROM dao_statistics WHERE id = 1"#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((sec, miner, dao, burnt, block)) => Ok((
                sec.parse().unwrap_or(0),
                miner.parse().unwrap_or(0),
                dao.parse().unwrap_or(0),
                burnt.parse().unwrap_or(0),
                block,
            )),
            None => Ok((0, 0, 0, 0, 0)),
        }
    }

    pub async fn accumulate_secondary_issuance(
        &self,
        breakdown: &SecondaryIssuanceBreakdown,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO block_secondary_issuance (
                block_number, block_timestamp, secondary_issuance, miner_secondary, dao_compensation, burnt
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (block_number) DO NOTHING
            "#,
        )
        .bind(block_number)
        .bind(block_timestamp)
        .bind(breakdown.secondary_issuance)
        .bind(breakdown.miner_secondary)
        .bind(breakdown.dao_compensation)
        .bind(breakdown.burnt)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE dao_statistics SET
                cumulative_secondary_issuance = (COALESCE(cumulative_secondary_issuance, '0')::numeric + $1)::text,
                cumulative_miner_secondary = (COALESCE(cumulative_miner_secondary, '0')::numeric + $2)::text,
                cumulative_dao_compensation = (COALESCE(cumulative_dao_compensation, '0')::numeric + $3)::text,
                cumulative_burnt = (COALESCE(cumulative_burnt, '0')::numeric + $4)::text,
                mining_reward = (COALESCE(cumulative_miner_secondary, '0')::numeric + $2)::text,
                deposit_compensation = (COALESCE(cumulative_dao_compensation, '0')::numeric + $3)::text,
                burnt = (COALESCE(cumulative_burnt, '0')::numeric + $4)::text,
                last_processed_block = $5,
                updated_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(breakdown.secondary_issuance)
        .bind(breakdown.miner_secondary)
        .bind(breakdown.dao_compensation)
        .bind(breakdown.burnt)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn recalculate_dao_extended_statistics(&self, _current_block: i64) -> Result<()> {
        let latest = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT number, dao FROM blocks WHERE dao IS NOT NULL ORDER BY number DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        let (latest_block, latest_dao) = match latest {
            Some((num, dao)) => (num, dao),
            None => {
                warn!("DAO stats: no blocks with dao field found");
                return Ok(());
            }
        };

        let latest_ar = match extract_ar_from_dao(&latest_dao) {
            Some(ar) => ar,
            None => {
                warn!(
                    "DAO stats: failed to extract AR from block {}, dao len={}",
                    latest_block,
                    latest_dao.len()
                );
                return Ok(());
            }
        };
        let total_issuance = extract_total_issuance_from_dao(&latest_dao).unwrap_or(0);

        let base_stats = sqlx::query_as::<_, (String, i64, i64)>(
            r#"SELECT 
                CAST(COALESCE(SUM(capacity), 0) AS TEXT),
                COUNT(DISTINCT lock_script_hash),
                COUNT(*)
            FROM dao_deposits WHERE status = 0"#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(("0".to_string(), 0, 0));

        let compensation_paid = sqlx::query_as::<_, (String,)>(
            "SELECT CAST(COALESCE(SUM(compensation), 0) AS TEXT) FROM dao_deposits WHERE status = 2 AND compensation IS NOT NULL"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(("0".to_string(),));

        let avg_epochs: (Option<f64>,) = sqlx::query_as(
            r#"SELECT AVG(($1 - deposit_block_number)::float8 / 1800.0) 
            FROM dao_deposits 
            WHERE status = 0 AND deposit_block_number <= $1"#,
        )
        .bind(latest_block)
        .fetch_one(&self.pool)
        .await
        .unwrap_or((None,));

        let deposits_with_ar = sqlx::query_as::<_, (String, Vec<u8>)>(
            r#"SELECT 
                CAST(d.capacity AS TEXT),
                b.dao
            FROM dao_deposits d
            JOIN blocks b ON d.deposit_block_number = b.number
            WHERE d.status = 0 AND b.dao IS NOT NULL AND d.deposit_block_number <= $1"#,
        )
        .bind(latest_block)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut total_unclaimed: u128 = 0;
        let dao_occupied_capacity_u128 = DAO_OCCUPIED_CAPACITY as u128;

        for (capacity_str, deposit_dao) in &deposits_with_ar {
            let capacity: u128 = capacity_str.parse().unwrap_or(0);
            let free_capacity = capacity.saturating_sub(dao_occupied_capacity_u128);

            if let Some(ar_deposit) = extract_ar_from_dao(deposit_dao) {
                if ar_deposit > 0 {
                    let compensation = (free_capacity * latest_ar as u128 / ar_deposit as u128)
                        .saturating_sub(free_capacity);
                    total_unclaimed += compensation;
                }
            }
        }

        let secondary_burnt: u128 = sqlx::query_as::<_, (String,)>(
            "SELECT COALESCE(cumulative_burnt, '0') FROM dao_statistics WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|(s,)| s.parse().unwrap_or(0))
        .unwrap_or(0);

        let estimated_apc = calculate_estimated_apc(total_issuance, secondary_burnt);

        let avg_epochs_val = avg_epochs.0.unwrap_or(0.0);

        info!(
            "DAO stats update: block={}, ar={}, issuance={}, deposits_matched={}, unclaimed={}, apc={:.2}%, avg_epochs={:.1}",
            latest_block,
            latest_ar,
            total_issuance,
            deposits_with_ar.len(),
            total_unclaimed,
            estimated_apc,
            avg_epochs_val
        );

        sqlx::query(
            r#"
            UPDATE dao_statistics SET
                total_deposited = $1::numeric,
                total_depositors = $2,
                active_deposits = $3,
                total_compensation_paid = $4::numeric,
                unclaimed_compensation = $5::numeric,
                average_deposit_epochs = $6,
                estimated_apc = $7,
                updated_at = NOW()
            WHERE id = 1
            "#,
        )
        .bind(&base_stats.0)
        .bind(base_stats.1 as i32)
        .bind(base_stats.2 as i32)
        .bind(&compensation_paid.0)
        .bind(total_unclaimed.to_string())
        .bind(avg_epochs_val as i32)
        .bind(format!("{:.2}", estimated_apc))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn record_deep_fork(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        db_tip: i64,
        db_tip_hash: &[u8],
        chain_tip: i64,
        chain_tip_hash: &[u8],
        depth: i64,
    ) -> Result<i32> {
        let event_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO reorg_events (
                fork_point_number, fork_point_hash,
                old_tip_number, old_tip_hash,
                new_tip_number, new_tip_hash,
                depth, event_type
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'deep')
            RETURNING id
            "#,
        )
        .bind(fork_point)
        .bind(fork_hash)
        .bind(db_tip)
        .bind(db_tip_hash)
        .bind(chain_tip)
        .bind(chain_tip_hash)
        .bind(depth as i32)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE sync_status SET
                deep_fork_detected = TRUE,
                deep_fork_at = NOW(),
                deep_fork_db_tip = $1,
                deep_fork_db_tip_hash = $2,
                deep_fork_chain_tip = $3,
                deep_fork_chain_tip_hash = $4,
                deep_fork_depth = $5,
                deep_fork_fork_point = $6
            WHERE id = 1
            "#,
        )
        .bind(db_tip)
        .bind(db_tip_hash)
        .bind(chain_tip)
        .bind(chain_tip_hash)
        .bind(depth as i32)
        .bind(fork_point)
        .execute(&self.pool)
        .await?;

        Ok(event_id)
    }

    pub async fn execute_reorg(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        old_tip: i64,
        old_tip_hash: &[u8],
        new_tip: i64,
        new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        let mut tx = self.pool.begin().await?;
        let rollback_from = fork_point + 1;
        let depth = (old_tip - fork_point) as i32;

        let event_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO reorg_events (
                fork_point_number, fork_point_hash,
                old_tip_number, old_tip_hash,
                new_tip_number, new_tip_hash,
                depth, event_type
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'auto')
            RETURNING id
            "#,
        )
        .bind(fork_point)
        .bind(fork_hash)
        .bind(old_tip)
        .bind(old_tip_hash)
        .bind(new_tip)
        .bind(new_tip_hash)
        .bind(depth)
        .fetch_one(&mut *tx)
        .await?;

        let orphaned_blocks: i64 = sqlx::query_scalar(
            r#"
            WITH archived AS (
                INSERT INTO orphaned_blocks (
                    reorg_event_id, number, hash, parent_hash,
                    timestamp, transactions_count, miner_lock_hash
                )
                SELECT $1, number, hash, parent_hash,
                       timestamp, transactions_count, miner_lock_hash
                FROM blocks
                WHERE number >= $2
                RETURNING 1
            )
            SELECT COUNT(*) FROM archived
            "#,
        )
        .bind(event_id)
        .bind(rollback_from)
        .fetch_one(&mut *tx)
        .await?;

        let orphaned_txs: i64 = sqlx::query_scalar(
            r#"
            WITH archived AS (
                INSERT INTO orphaned_transactions (
                    reorg_event_id, hash, block_number, block_hash,
                    tx_index, inputs_count, outputs_count, total_capacity
                )
                SELECT $1, t.hash, t.block_number, b.hash,
                       t.tx_index, t.inputs_count, t.outputs_count, t.total_output_capacity
                FROM transactions t
                JOIN blocks b ON t.block_number = b.number
                WHERE t.block_number >= $2
                RETURNING 1
            )
            SELECT COUNT(*) FROM archived
            "#,
        )
        .bind(event_id)
        .bind(rollback_from)
        .fetch_one(&mut *tx)
        .await?;

        // Rollback statistics before deleting blocks/cells (need the data for calculation)
        self.rollback_statistics(&mut tx, rollback_from).await?;

        sqlx::query(
            r#"
            INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, 
                lock_script_hash, lock_code_hash, lock_args,
                type_script_hash, type_code_hash, data_size)
            SELECT tx_hash, output_index, created_at_block, capacity::bigint,
                lock_script_hash, lock_code_hash, lock_args,
                type_script_hash, type_code_hash, data_size
            FROM cells
            WHERE consumed_at_block >= $1
            ON CONFLICT (tx_hash, output_index) DO NOTHING
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE cells SET
                status = 0,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                consumed_at_index = NULL
            WHERE consumed_at_block >= $1
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM live_cells WHERE created_at_block >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM cells WHERE created_at_block >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM address_transactions WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM address_asset_transfers WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM dob_transfers WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM nft_transfers WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM activities WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM transaction_inputs WHERE tx_block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM transaction_cell_deps WHERE tx_block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM transactions WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM block_proposals WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM blocks WHERE number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            UPDATE dao_deposits SET
                withdraw_request_tx = NULL,
                withdraw_request_block = NULL,
                withdraw_request_timestamp = NULL,
                withdraw_request_ar = NULL,
                status = 0
            WHERE withdraw_request_block >= $1 AND status = 1
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE dao_deposits SET
                withdraw_tx = NULL,
                withdraw_block = NULL,
                withdraw_timestamp = NULL,
                compensation = NULL,
                status = CASE 
                    WHEN withdraw_request_tx IS NOT NULL THEN 1 
                    ELSE 0 
                END
            WHERE withdraw_block >= $1
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM dao_deposits WHERE deposit_block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        self.rollback_token_statistics(&mut tx, rollback_from)
            .await?;

        sqlx::query(
            r#"
            UPDATE sync_status SET
                last_reorg_at = NOW(),
                last_reorg_depth = $1,
                deep_fork_detected = FALSE,
                deep_fork_at = NULL,
                deep_fork_db_tip = NULL,
                deep_fork_db_tip_hash = NULL,
                deep_fork_chain_tip = NULL,
                deep_fork_chain_tip_hash = NULL,
                deep_fork_depth = NULL,
                deep_fork_fork_point = NULL
            WHERE id = 1
            "#,
        )
        .bind(depth)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE reorg_events SET
                orphaned_blocks_count = $2,
                orphaned_txs_count = $3
            WHERE id = $1
            "#,
        )
        .bind(event_id)
        .bind(orphaned_blocks as i32)
        .bind(orphaned_txs as i32)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        if let Some(cache) = &self.cache_invalidator {
            let hash_hex = format!("0x{}", hex::encode(fork_hash));
            cache
                .update_sync_status(|status| {
                    status.tip_block_number = fork_point;
                    status.tip_block_hash = hash_hex;
                    status.last_synced_at = chrono::Utc::now().timestamp();
                })
                .await;
        }

        info!(
            "Reorg completed: fork_point={}, depth={}, orphaned_blocks={}, orphaned_txs={}",
            fork_point, depth, orphaned_blocks, orphaned_txs
        );

        Ok(ReorgResult {
            event_id,
            depth,
            orphaned_blocks: orphaned_blocks as i32,
            orphaned_txs: orphaned_txs as i32,
        })
    }

    async fn rollback_token_statistics(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rollback_from: i64,
    ) -> Result<()> {
        let affected_tokens: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT token_id FROM token_transfers WHERE block_number >= $1",
        )
        .bind(rollback_from)
        .fetch_all(&mut **tx)
        .await?;

        if affected_tokens.is_empty() {
            sqlx::query("DELETE FROM token_transfers WHERE block_number >= $1")
                .bind(rollback_from)
                .execute(&mut **tx)
                .await?;
            return Ok(());
        }

        let token_ids: Vec<i64> = affected_tokens.into_iter().map(|(id,)| id).collect();

        for &token_id in &token_ids {
            let supply_delta: (Option<String>, Option<String>) = sqlx::query_as(
                r#"
                SELECT 
                    SUM(CASE WHEN is_mint THEN amount ELSE 0 END)::text,
                    SUM(CASE WHEN is_burn THEN amount ELSE 0 END)::text
                FROM token_transfers 
                WHERE token_id = $1 AND block_number >= $2
                "#,
            )
            .bind(token_id)
            .bind(rollback_from)
            .fetch_one(&mut **tx)
            .await?;

            let minted: i128 = supply_delta.0.unwrap_or_default().parse().unwrap_or(0);
            let burned: i128 = supply_delta.1.unwrap_or_default().parse().unwrap_or(0);
            let net_supply_change = minted - burned;

            if net_supply_change != 0 {
                sqlx::query(
                    r#"
                    UPDATE tokens SET 
                        total_supply = GREATEST(total_supply - $1::numeric, 0)
                    WHERE id = $2
                    "#,
                )
                .bind(net_supply_change.to_string())
                .bind(token_id)
                .execute(&mut **tx)
                .await?;
            }

            let transfers_to_remove: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM token_transfers WHERE token_id = $1 AND block_number >= $2",
            )
            .bind(token_id)
            .bind(rollback_from)
            .fetch_one(&mut **tx)
            .await?;

            sqlx::query(
                r#"
                UPDATE tokens SET 
                    transfers_count = GREATEST(transfers_count - $1, 0)
                WHERE id = $2
                "#,
            )
            .bind(transfers_to_remove)
            .bind(token_id)
            .execute(&mut **tx)
            .await?;
        }

        sqlx::query("DELETE FROM token_transfers WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut **tx)
            .await?;

        for &token_id in &token_ids {
            sqlx::query("DELETE FROM token_balances WHERE token_id = $1")
                .bind(token_id)
                .execute(&mut **tx)
                .await?;

            sqlx::query(
                r#"
                INSERT INTO token_balances (token_id, lock_script_hash, balance, first_tx, last_tx)
                SELECT 
                    b.token_id,
                    b.lock_hash,
                    b.balance,
                    first_t.tx_hash as first_tx,
                    last_t.tx_hash as last_tx
                FROM (
                    SELECT 
                        token_id,
                        lock_hash,
                        SUM(amount) as balance,
                        MIN(block_number) as first_block,
                        MAX(block_number) as last_block
                    FROM (
                        SELECT token_id, to_lock_hash as lock_hash, amount, block_number
                        FROM token_transfers WHERE token_id = $1
                        UNION ALL
                        SELECT token_id, from_lock_hash as lock_hash, -amount, block_number
                        FROM token_transfers WHERE token_id = $1 AND from_lock_hash IS NOT NULL
                    ) movements
                    GROUP BY token_id, lock_hash
                    HAVING SUM(amount) > 0
                ) b
                JOIN LATERAL (
                    SELECT tx_hash FROM token_transfers 
                    WHERE token_id = b.token_id 
                      AND (to_lock_hash = b.lock_hash OR from_lock_hash = b.lock_hash)
                    ORDER BY block_number ASC, id ASC LIMIT 1
                ) first_t ON true
                JOIN LATERAL (
                    SELECT tx_hash FROM token_transfers 
                    WHERE token_id = b.token_id 
                      AND (to_lock_hash = b.lock_hash OR from_lock_hash = b.lock_hash)
                    ORDER BY block_number DESC, id DESC LIMIT 1
                ) last_t ON true
                "#,
            )
            .bind(token_id)
            .execute(&mut **tx)
            .await?;

            let holders: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM token_balances WHERE token_id = $1")
                    .bind(token_id)
                    .fetch_one(&mut **tx)
                    .await?;

            sqlx::query("UPDATE tokens SET holders_count = $1 WHERE id = $2")
                .bind(holders as i32)
                .bind(token_id)
                .execute(&mut **tx)
                .await?;
        }

        Ok(())
    }

    async fn rollback_statistics(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rollback_from: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            WITH rollback_hourly AS (
                SELECT 
                    date_trunc('hour', timestamp) AS hour,
                    COUNT(*)::int AS blocks_count,
                    SUM(transactions_count)::int AS transactions_count
                FROM blocks 
                WHERE number >= $1
                GROUP BY date_trunc('hour', timestamp)
            )
            UPDATE hourly_statistics h SET 
                blocks_count = GREATEST(h.blocks_count - r.blocks_count, 0),
                transactions_count = GREATEST(h.transactions_count - r.transactions_count, 0),
                updated_at = NOW()
            FROM rollback_hourly r 
            WHERE h.hour = r.hour
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_hourly_cells AS (
                SELECT 
                    date_trunc('hour', b.timestamp) AS hour,
                    COUNT(*) FILTER (WHERE c.created_at_block >= $1)::int AS cells_created,
                    COUNT(*) FILTER (WHERE c.consumed_at_block >= $1)::int AS cells_consumed
                FROM blocks b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                WHERE b.number >= $1
                GROUP BY date_trunc('hour', b.timestamp)
            )
            UPDATE hourly_statistics h SET 
                cells_created = GREATEST(h.cells_created - COALESCE(r.cells_created, 0), 0),
                cells_consumed = GREATEST(h.cells_consumed - COALESCE(r.cells_consumed, 0), 0)
            FROM rollback_hourly_cells r 
            WHERE h.hour = r.hour
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_daily AS (
                SELECT 
                    timestamp::date AS date,
                    COUNT(*)::int AS blocks_count,
                    SUM(transactions_count)::int AS transactions_count
                FROM blocks 
                WHERE number >= $1
                GROUP BY timestamp::date
            )
            UPDATE daily_statistics d SET 
                blocks_count = GREATEST(d.blocks_count - r.blocks_count, 0),
                transactions_count = GREATEST(d.transactions_count - r.transactions_count, 0),
                updated_at = NOW()
            FROM rollback_daily r 
            WHERE d.date = r.date
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_daily_cells AS (
                SELECT 
                    b.timestamp::date AS date,
                    COUNT(*) FILTER (WHERE c.created_at_block >= $1)::int AS cells_created,
                    COUNT(*) FILTER (WHERE c.consumed_at_block >= $1)::int AS cells_consumed,
                    COALESCE(SUM(c.data_size) FILTER (WHERE c.created_at_block >= $1), 0)::bigint AS data_created,
                    COALESCE(SUM(c.data_size) FILTER (WHERE c.consumed_at_block >= $1), 0)::bigint AS data_consumed
                FROM blocks b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                WHERE b.number >= $1
                GROUP BY b.timestamp::date
            )
            UPDATE daily_statistics d SET 
                cells_created = GREATEST(d.cells_created - COALESCE(r.cells_created, 0), 0),
                cells_consumed = GREATEST(d.cells_consumed - COALESCE(r.cells_consumed, 0), 0),
                total_live_cells = d.total_live_cells - COALESCE(r.cells_created, 0) + COALESCE(r.cells_consumed, 0),
                total_data_size = d.total_data_size - COALESCE(r.data_created, 0) + COALESCE(r.data_consumed, 0)
            FROM rollback_daily_cells r 
            WHERE d.date = r.date
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_miner AS (
                SELECT 
                    timestamp::date AS date,
                    miner_lock_hash,
                    COUNT(*)::int AS blocks_count
                FROM blocks 
                WHERE number >= $1 AND miner_lock_hash IS NOT NULL
                GROUP BY timestamp::date, miner_lock_hash
            )
            UPDATE miner_statistics m SET 
                blocks_count = GREATEST(m.blocks_count - r.blocks_count, 0)
            FROM rollback_miner r 
            WHERE m.date = r.date AND m.miner_lock_hash = r.miner_lock_hash
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        info!(
            "Statistics rollback completed for blocks >= {}",
            rollback_from
        );
        Ok(())
    }

    pub async fn clear_deep_fork_flag(&self) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_status SET
                deep_fork_detected = FALSE,
                deep_fork_at = NULL,
                deep_fork_db_tip = NULL,
                deep_fork_db_tip_hash = NULL,
                deep_fork_chain_tip = NULL,
                deep_fork_chain_tip_hash = NULL,
                deep_fork_depth = NULL,
                deep_fork_fork_point = NULL
            WHERE id = 1
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn resolve_deep_fork(
        &self,
        action: &str,
        resolved_by: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE reorg_events SET
                event_type = 'resolved',
                resolved_at = NOW(),
                resolved_by = $2,
                resolution_action = $1,
                resolution_notes = $3
            WHERE event_type = 'deep' AND resolved_at IS NULL
            "#,
        )
        .bind(action)
        .bind(resolved_by)
        .bind(notes)
        .execute(&self.pool)
        .await?;

        self.clear_deep_fork_flag().await?;

        Ok(())
    }

    // ===========================================
    // M-NFT Functions
    // ===========================================

    pub async fn insert_mnft_issuer(
        &self,
        issuer: &crate::parser::mnft::ParsedMnftIssuer,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO mnft_issuers (
                issuer_id, type_script_hash, name, info, owner_lock_hash,
                classes_count, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (issuer_id) DO UPDATE SET
                name = COALESCE(EXCLUDED.name, mnft_issuers.name),
                info = COALESCE(EXCLUDED.info, mnft_issuers.info),
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                classes_count = EXCLUDED.classes_count,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&issuer.issuer_id)
        .bind(&issuer.type_script_hash)
        .bind(&issuer.name)
        .bind(&issuer.info)
        .bind(&issuer.owner_lock_hash)
        .bind(issuer.class_count as i32)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_mnft_issuer(
        &self,
        issuer_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE mnft_issuers SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE issuer_id = $1
            "#,
        )
        .bind(issuer_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_mnft_class(
        &self,
        class: &crate::parser::mnft::ParsedMnftClass,
        tx_hash: &[u8],
        _output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO mnft_classes (
                class_id, type_script_hash, issuer_id, name, description, renderer,
                total, issued, owner_lock_hash, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (class_id) DO UPDATE SET
                name = COALESCE(EXCLUDED.name, mnft_classes.name),
                description = COALESCE(EXCLUDED.description, mnft_classes.description),
                renderer = COALESCE(EXCLUDED.renderer, mnft_classes.renderer),
                total = EXCLUDED.total,
                issued = EXCLUDED.issued,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&class.class_id)
        .bind(&class.type_script_hash)
        .bind(&class.issuer_id)
        .bind(&class.name)
        .bind(&class.description)
        .bind(&class.renderer)
        .bind(class.total as i32)
        .bind(class.issued as i32)
        .bind(&class.owner_lock_hash)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        // Update issuer's class count
        sqlx::query(
            "UPDATE mnft_issuers SET classes_count = classes_count + 1, updated_at = NOW() WHERE issuer_id = $1",
        )
        .bind(&class.issuer_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_mnft_class(
        &self,
        class_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        let class = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT issuer_id FROM mnft_classes WHERE class_id = $1",
        )
        .bind(class_id)
        .fetch_optional(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE mnft_classes SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE class_id = $1
            "#,
        )
        .bind(class_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        if let Some((issuer_id,)) = class {
            sqlx::query(
                "UPDATE mnft_issuers SET classes_count = GREATEST(0, classes_count - 1), updated_at = NOW() WHERE issuer_id = $1",
            )
            .bind(&issuer_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_mnft_class_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        // Classes don't store outpoint in schema, so we query by type_script_hash
        // This is a limitation - we may need to add tx_hash/output_index to schema
        // For now, return None as classes are identified by class_id
        Ok(None)
    }

    pub async fn insert_mnft_token(
        &self,
        token: &crate::parser::mnft::ParsedMnftToken,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO mnft_tokens (
                token_id, type_script_hash, tx_hash, output_index, class_id, token_index,
                characteristic, configure, state, owner_lock_hash, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $3)
            ON CONFLICT (token_id) DO UPDATE SET
                tx_hash = EXCLUDED.tx_hash,
                output_index = EXCLUDED.output_index,
                characteristic = EXCLUDED.characteristic,
                configure = EXCLUDED.configure,
                state = EXCLUDED.state,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&token.token_id)
        .bind(&token.type_script_hash)
        .bind(tx_hash)
        .bind(output_index)
        .bind(&token.class_id)
        .bind(token.token_index as i32)
        .bind(&token.characteristic)
        .bind(token.configure as i16)
        .bind(token.state as i16)
        .bind(&token.owner_lock_hash)
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        // Update class issued count
        sqlx::query(
            "UPDATE mnft_classes SET issued = issued + 1, updated_at = NOW() WHERE class_id = $1",
        )
        .bind(&token.class_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_mnft_token(
        &self,
        token_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        let token =
            sqlx::query_as::<_, (Vec<u8>,)>("SELECT class_id FROM mnft_tokens WHERE token_id = $1")
                .bind(token_id)
                .fetch_optional(&self.pool)
                .await?;

        sqlx::query(
            r#"
            UPDATE mnft_tokens SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE token_id = $1
            "#,
        )
        .bind(token_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        if let Some((class_id,)) = token {
            sqlx::query(
                "UPDATE mnft_classes SET issued = GREATEST(0, issued - 1), updated_at = NOW() WHERE class_id = $1",
            )
            .bind(&class_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_mnft_token_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT token_id FROM mnft_tokens WHERE tx_hash = $1 AND output_index = $2",
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id))
    }

    // ===========================================
    // .bit (DAS) Functions
    // ===========================================

    pub async fn insert_dotbit_account(
        &self,
        account: &crate::parser::dotbit::ParsedDotbitAccount,
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
    ) -> Result<()> {
        // For account_name, we use hex-encoded account_id since the parser doesn't extract the human-readable name
        // In a full implementation, this would parse the account name from witness data
        let account_name = format!("0x{}", hex::encode(&account.account_id));

        sqlx::query(
            r#"
            INSERT INTO dotbit_accounts (
                account_id, type_script_hash, tx_hash, output_index, account_name,
                owner_lock_hash, expired_at, created_at_block, created_at_tx
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $3)
            ON CONFLICT (account_id) DO UPDATE SET
                tx_hash = EXCLUDED.tx_hash,
                output_index = EXCLUDED.output_index,
                owner_lock_hash = EXCLUDED.owner_lock_hash,
                expired_at = EXCLUDED.expired_at,
                is_live = TRUE,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                updated_at = NOW()
            "#,
        )
        .bind(&account.account_id)
        .bind(&account.type_script_hash)
        .bind(tx_hash)
        .bind(output_index)
        .bind(&account_name)
        .bind(&account.owner_lock_hash)
        .bind(account.expired_at.map(|e| e as i64))
        .bind(block_number)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn consume_dotbit_account(
        &self,
        account_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE dotbit_accounts SET
                is_live = FALSE,
                consumed_at_block = $2,
                consumed_by_tx = $3,
                updated_at = NOW()
            WHERE account_id = $1
            "#,
        )
        .bind(account_id)
        .bind(block_number)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_dotbit_account_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT account_id FROM dotbit_accounts WHERE tx_hash = $1 AND output_index = $2",
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(id,)| id))
    }

    pub async fn insert_dob_transfer(
        &self,
        dob_id: &[u8],
        cluster_id: Option<&[u8]>,
        dob_type: &str,
        tx_hash: &[u8],
        block_number: i64,
        from_lock_hash: Option<&[u8]>,
        to_lock_hash: &[u8],
        event_type: &str,
        content_type: Option<&str>,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO dob_transfers (
                dob_id, cluster_id, dob_type, tx_hash, block_number,
                from_lock_hash, to_lock_hash, event_type, content_type, timestamp
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(dob_id)
        .bind(cluster_id)
        .bind(dob_type)
        .bind(tx_hash)
        .bind(block_number)
        .bind(from_lock_hash)
        .bind(to_lock_hash)
        .bind(event_type)
        .bind(content_type)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_nft_transfer(
        &self,
        nft_id: &[u8],
        nft_type: &str,
        issuer_id: Option<&[u8]>,
        class_id: Option<&[u8]>,
        tx_hash: &[u8],
        block_number: i64,
        from_lock_hash: Option<&[u8]>,
        to_lock_hash: &[u8],
        event_type: &str,
        name: Option<&str>,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO nft_transfers (
                nft_id, nft_type, issuer_id, class_id, tx_hash, block_number,
                from_lock_hash, to_lock_hash, event_type, name, timestamp
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(nft_id)
        .bind(nft_type)
        .bind(issuer_id)
        .bind(class_id)
        .bind(tx_hash)
        .bind(block_number)
        .bind(from_lock_hash)
        .bind(to_lock_hash)
        .bind(event_type)
        .bind(name)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_address_asset_transfers_batch(
        &self,
        records: &[(
            Vec<u8>,         // lock_script_hash
            Vec<u8>,         // tx_hash
            i64,             // block_number
            i32,             // tx_index
            i16,             // event_index
            String,          // asset_category
            String,          // asset_type
            Option<Vec<u8>>, // asset_id
            i16,             // direction (1=in, 2=out)
            Option<Vec<u8>>, // peer_lock_hash
            Option<String>,  // amount (as string for NUMERIC)
            Option<String>,  // event_type
            DateTime<Utc>,   // timestamp
        )],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let lock_hashes: Vec<&[u8]> = records.iter().map(|r| r.0.as_slice()).collect();
        let tx_hashes: Vec<&[u8]> = records.iter().map(|r| r.1.as_slice()).collect();
        let block_numbers: Vec<i64> = records.iter().map(|r| r.2).collect();
        let tx_indexes: Vec<i32> = records.iter().map(|r| r.3).collect();
        let event_indexes: Vec<i16> = records.iter().map(|r| r.4).collect();
        let asset_categories: Vec<&str> = records.iter().map(|r| r.5.as_str()).collect();
        let asset_types: Vec<&str> = records.iter().map(|r| r.6.as_str()).collect();
        let asset_ids: Vec<Option<&[u8]>> = records.iter().map(|r| r.7.as_deref()).collect();
        let directions: Vec<i16> = records.iter().map(|r| r.8).collect();
        let peer_lock_hashes: Vec<Option<&[u8]>> = records.iter().map(|r| r.9.as_deref()).collect();
        let amounts: Vec<Option<&str>> = records.iter().map(|r| r.10.as_deref()).collect();
        let event_types: Vec<Option<&str>> = records.iter().map(|r| r.11.as_deref()).collect();
        let timestamps: Vec<DateTime<Utc>> = records.iter().map(|r| r.12).collect();

        sqlx::query(
            r#"
            INSERT INTO address_asset_transfers (
                lock_script_hash, tx_hash, block_number, tx_index, event_index,
                asset_category, asset_type, asset_id, direction, peer_lock_hash,
                amount, event_type, timestamp
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::bytea[], $3::bigint[], $4::int[], $5::smallint[],
                $6::text[], $7::text[], $8::bytea[], $9::smallint[], $10::bytea[],
                $11::numeric[], $12::text[], $13::timestamptz[]
            )
            "#,
        )
        .bind(&lock_hashes)
        .bind(&tx_hashes)
        .bind(&block_numbers)
        .bind(&tx_indexes)
        .bind(&event_indexes)
        .bind(&asset_categories)
        .bind(&asset_types)
        .bind(&asset_ids)
        .bind(&directions)
        .bind(&peer_lock_hashes)
        .bind(&amounts)
        .bind(&event_types)
        .bind(&timestamps)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_spore_owner_by_id(&self, spore_id: &[u8]) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT owner_lock_hash FROM spore_cells WHERE spore_id = $1",
        )
        .bind(spore_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(hash,)| hash))
    }

    pub async fn get_mnft_token_owner_by_id(&self, token_id: &[u8]) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT owner_lock_hash FROM mnft_tokens WHERE token_id = $1",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(hash,)| hash))
    }

    pub async fn get_dotbit_owner_by_id(&self, account_id: &[u8]) -> Result<Option<Vec<u8>>> {
        let result = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT owner_lock_hash FROM dotbit_accounts WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(hash,)| hash))
    }

    pub async fn rebuild_all_statistics(&self) -> Result<()> {
        info!("Rebuilding all statistics after bulk sync completion...");

        self.rebuild_daily_statistics().await?;
        self.rebuild_daily_block_stats().await?;
        self.rebuild_hourly_statistics().await?;
        self.rebuild_miner_statistics().await?;
        self.rebuild_block_time_distribution().await?;
        self.rebuild_epoch_time_distribution().await?;
        self.rebuild_dao_daily_snapshots().await?;

        info!("All statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_daily_statistics(&self) -> Result<()> {
        info!("Rebuilding daily_statistics...");
        sqlx::query("TRUNCATE TABLE daily_statistics")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO daily_statistics (
                date, blocks_count, transactions_count, cells_created, cells_consumed,
                capacity_transferred, total_live_cells, total_data_size
            )
            WITH daily_blocks AS (
                SELECT 
                    timestamp::date as date,
                    COUNT(*) as blocks_count,
                    SUM(transactions_count) as transactions_count
                FROM blocks
                GROUP BY timestamp::date
            ),
            daily_cells AS (
                SELECT 
                    b.timestamp::date as date,
                    SUM(CASE WHEN c.created_at_block = b.number THEN 1 ELSE 0 END) as cells_created,
                    SUM(CASE WHEN c.consumed_at_block = b.number THEN 1 ELSE 0 END) as cells_consumed,
                    SUM(CASE WHEN c.created_at_block = b.number THEN c.capacity ELSE 0 END) as capacity_transferred,
                    SUM(CASE WHEN c.created_at_block = b.number THEN c.data_size ELSE 0 END) as data_size_added,
                    SUM(CASE WHEN c.consumed_at_block = b.number THEN c.data_size ELSE 0 END) as data_size_consumed
                FROM blocks b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                GROUP BY b.timestamp::date
            )
            SELECT 
                db.date,
                db.blocks_count::int,
                db.transactions_count::int,
                COALESCE(dc.cells_created, 0)::int,
                COALESCE(dc.cells_consumed, 0)::int,
                COALESCE(dc.capacity_transferred, 0),
                SUM(COALESCE(dc.cells_created, 0) - COALESCE(dc.cells_consumed, 0)) 
                    OVER (ORDER BY db.date) as total_live_cells,
                SUM(COALESCE(dc.data_size_added, 0) - COALESCE(dc.data_size_consumed, 0)) 
                    OVER (ORDER BY db.date) as total_data_size
            FROM daily_blocks db
            LEFT JOIN daily_cells dc ON db.date = dc.date
            ORDER BY db.date
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("daily_statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_daily_block_stats(&self) -> Result<()> {
        info!("Rebuilding daily_block_stats...");
        sqlx::query("TRUNCATE TABLE daily_block_stats")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO daily_block_stats (
                date, avg_compact_target, block_count, total_uncles, avg_uncle_rate, avg_block_time_ms
            )
            WITH block_times AS (
                SELECT 
                    number,
                    timestamp,
                    timestamp::date as date,
                    compact_target,
                    uncles_count,
                    EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
                FROM blocks
            )
            SELECT 
                date,
                AVG(compact_target)::bigint as avg_compact_target,
                COUNT(*)::int as block_count,
                SUM(uncles_count)::int as total_uncles,
                SUM(uncles_count)::float / NULLIF(COUNT(*), 0)::float as avg_uncle_rate,
                AVG(block_time_ms)::int as avg_block_time_ms
            FROM block_times
            GROUP BY date
            ORDER BY date
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("daily_block_stats rebuild completed");
        Ok(())
    }

    async fn rebuild_hourly_statistics(&self) -> Result<()> {
        info!("Rebuilding hourly_statistics...");
        sqlx::query("TRUNCATE TABLE hourly_statistics")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO hourly_statistics (
                hour, blocks_count, transactions_count, cells_created, cells_consumed, capacity_transferred
            )
            WITH hourly_blocks AS (
                SELECT 
                    date_trunc('hour', timestamp) as hour,
                    COUNT(*) as blocks_count,
                    SUM(transactions_count) as transactions_count
                FROM blocks
                GROUP BY date_trunc('hour', timestamp)
            ),
            hourly_cells AS (
                SELECT 
                    date_trunc('hour', b.timestamp) as hour,
                    SUM(CASE WHEN c.created_at_block = b.number THEN 1 ELSE 0 END) as cells_created,
                    SUM(CASE WHEN c.consumed_at_block = b.number THEN 1 ELSE 0 END) as cells_consumed,
                    SUM(CASE WHEN c.created_at_block = b.number THEN c.capacity ELSE 0 END) as capacity_transferred
                FROM blocks b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                GROUP BY date_trunc('hour', b.timestamp)
            )
            SELECT 
                hb.hour,
                hb.blocks_count::int,
                hb.transactions_count::int,
                COALESCE(hc.cells_created, 0)::int,
                COALESCE(hc.cells_consumed, 0)::int,
                COALESCE(hc.capacity_transferred, 0)
            FROM hourly_blocks hb
            LEFT JOIN hourly_cells hc ON hb.hour = hc.hour
            ORDER BY hb.hour
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("hourly_statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_miner_statistics(&self) -> Result<()> {
        info!("Rebuilding miner_statistics...");
        sqlx::query("TRUNCATE TABLE miner_statistics")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
            SELECT 
                b.timestamp::date as date,
                c.lock_script_hash as miner_lock_hash,
                COUNT(*)::int as blocks_count,
                MAX(b.number) as last_block_number
            FROM blocks b
            JOIN transactions t ON t.block_number = b.number AND t.tx_index = 0
            JOIN cells c ON c.tx_hash = t.hash AND c.output_index = 0
            GROUP BY b.timestamp::date, c.lock_script_hash
            ORDER BY date, blocks_count DESC
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("miner_statistics rebuild completed");
        Ok(())
    }

    async fn rebuild_block_time_distribution(&self) -> Result<()> {
        info!("Rebuilding block_time_distribution...");
        sqlx::query("TRUNCATE TABLE block_time_distribution")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO block_time_distribution (bucket_seconds, block_count)
            SELECT 
                CASE 
                    WHEN block_time_sec < 1 THEN 0
                    WHEN block_time_sec < 30 THEN FLOOR(block_time_sec)::int
                    ELSE 30
                END as bucket_seconds,
                COUNT(*) as block_count
            FROM (
                SELECT 
                    EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) as block_time_sec
                FROM blocks
                WHERE number > 0
            ) block_times
            WHERE block_time_sec IS NOT NULL AND block_time_sec >= 0
            GROUP BY bucket_seconds
            ORDER BY bucket_seconds
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("block_time_distribution rebuild completed");
        Ok(())
    }

    async fn rebuild_epoch_time_distribution(&self) -> Result<()> {
        info!("Rebuilding epoch_time_distribution...");
        sqlx::query("TRUNCATE TABLE epoch_time_distribution")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
            SELECT 
                CASE 
                    WHEN epoch_minutes < 180 THEN 180
                    WHEN epoch_minutes > 300 THEN 300
                    ELSE (FLOOR(epoch_minutes / 5) * 5)::int
                END as bucket_minutes,
                COUNT(*) as epoch_count
            FROM (
                SELECT 
                    EXTRACT(EPOCH FROM (end_timestamp - start_timestamp)) / 60 as epoch_minutes
                FROM epoch_statistics
                WHERE end_timestamp IS NOT NULL
            ) epoch_times
            GROUP BY bucket_minutes
            ORDER BY bucket_minutes
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("epoch_time_distribution rebuild completed");
        Ok(())
    }

    async fn rebuild_dao_daily_snapshots(&self) -> Result<()> {
        info!("Rebuilding dao_daily_snapshots...");
        sqlx::query("TRUNCATE TABLE dao_daily_snapshots")
            .execute(&self.pool)
            .await?;

        let dates: Vec<(NaiveDate,)> =
            sqlx::query_as("SELECT DISTINCT timestamp::date as date FROM blocks ORDER BY date")
                .fetch_all(&self.pool)
                .await?;

        for (date,) in dates {
            self.update_dao_daily_snapshot(date).await?;
        }

        info!("dao_daily_snapshots rebuild completed");
        Ok(())
    }
}

pub struct ReorgResult {
    pub event_id: i32,
    pub depth: i32,
    pub orphaned_blocks: i32,
    pub orphaned_txs: i32,
}
