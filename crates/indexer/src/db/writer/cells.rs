use anyhow::Result;
use std::collections::HashMap;

use crate::db::LiveCellInfo;
use crate::parser::cell::ParsedCell;

use super::BatchWriter;

/// Dep group format: 4-byte count (u32 LE) + N × 36-byte OutPoints (32 tx_hash + 4 index)
fn looks_like_dep_group(data: &[u8]) -> bool {
    let size = data.len();
    if !(40..=10000).contains(&size) || (size - 4) % 36 != 0 {
        return false;
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
    count > 0 && count <= 256 && count == (size - 4) / 36
}

impl BatchWriter {
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
                let info = LiveCellInfo {
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
                        key,
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
}
