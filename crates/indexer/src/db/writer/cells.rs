use anyhow::Result;
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::LiveCellInfo;

use crate::parser::cell::ParsedCell;

use super::BatchWriter;

impl BatchWriter {
    pub fn insert_cells_batch(
        &self,
        cells: &[(&[u8], i16, &ParsedCell, i64)],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

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
            batch.put_cell(tx_hash, *output_index, &info);
            batch.put_cell_by_lock(
                &cell.lock_script_hash,
                *created_at_block,
                tx_hash,
                *output_index,
            );
            if let Some(ref type_hash) = cell.type_script_hash {
                batch.put_cell_by_type(type_hash, *created_at_block, tx_hash, *output_index);
            }
        }

        Ok(())
    }

    pub fn consume_cells_batch(
        &self,
        consumptions: &[(&[u8], i16, i64, &[u8], i64, i16)],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        for (
            tx_hash,
            output_index,
            _created_at_block,
            _consumed_by_tx,
            _consumed_at_block,
            _consumed_at_index,
        ) in consumptions
        {
            // Get cell info before removing it
            let outpoint_key = keys::encode_outpoint(tx_hash, *output_index);
            if let Ok(Some(value)) = self.store.get_cf(self.store.cf_live_cells(), &outpoint_key) {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    // Move to consumed cells
                    batch.put_consumed_cell(tx_hash, *output_index, &info);
                    // Remove from live cells
                    batch.delete_cell(tx_hash, *output_index);
                    // Remove cell indexes
                    batch.delete_cell_by_lock(
                        &info.lock_script_hash,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                    if let Some(ref type_hash) = info.type_script_hash {
                        batch.delete_cell_by_type(
                            type_hash,
                            info.created_at_block,
                            tx_hash,
                            *output_index,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn get_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<(i64, i64, Vec<u8>)>> {
        let outpoint_key = keys::encode_outpoint(tx_hash, output_index);

        // Check live cells first
        if let Some(value) = self
            .store
            .get_cf(self.store.cf_live_cells(), &outpoint_key)?
        {
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                return Ok(Some((
                    info.capacity,
                    info.created_at_block,
                    info.lock_script_hash,
                )));
            }
        }

        // Check consumed cells
        if let Some(value) = self
            .store
            .get_cf(self.store.cf_consumed_cells(), &outpoint_key)?
        {
            if let Ok(info) =
                bincode::deserialize::<ckbadger_store::types::CompactConsumedCellInfo>(&value)
            {
                return Ok(Some((
                    info.capacity,
                    info.created_at_block,
                    info.lock_script_hash,
                )));
            }
        }

        Ok(None)
    }

    pub fn get_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        _bulk_sync_mode: bool,
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());

        // Batch get from live cells
        let keys: Vec<_> = outpoints
            .iter()
            .map(|(h, i)| {
                let key = keys::encode_outpoint(h, *i);
                (self.store.cf_live_cells(), key)
            })
            .collect();

        let key_refs: Vec<_> = keys.iter().map(|(cf, k)| (*cf, k.as_slice())).collect();
        let results = self.store.multi_get_cf(key_refs);

        for (idx, res) in results.into_iter().enumerate() {
            if let Ok(Some(value)) = res {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let (tx_hash, output_index) = outpoints[idx];
                    result.insert(
                        (tx_hash.to_vec(), output_index),
                        (
                            info.capacity,
                            info.created_at_block,
                            info.lock_script_hash,
                            info.data_size,
                        ),
                    );
                }
            }
        }

        // Check consumed cells for missing entries
        let missing: Vec<_> = outpoints
            .iter()
            .filter(|(h, i)| !result.contains_key(&(h.to_vec(), *i)))
            .collect();

        if !missing.is_empty() {
            let consumed_keys: Vec<_> = missing
                .iter()
                .map(|(h, i)| {
                    let key = keys::encode_outpoint(h, *i);
                    (self.store.cf_consumed_cells(), key)
                })
                .collect();

            let consumed_refs: Vec<_> = consumed_keys
                .iter()
                .map(|(cf, k)| (*cf, k.as_slice()))
                .collect();
            let consumed_results = self.store.multi_get_cf(consumed_refs);

            for (idx, res) in consumed_results.into_iter().enumerate() {
                if let Ok(Some(value)) = res {
                    if let Ok(info) = bincode::deserialize::<
                        ckbadger_store::types::CompactConsumedCellInfo,
                    >(&value)
                    {
                        let (tx_hash, output_index) = missing[idx];
                        result.insert(
                            (tx_hash.to_vec(), *output_index),
                            (
                                info.capacity,
                                info.created_at_block,
                                info.lock_script_hash,
                                info.data_size,
                            ),
                        );
                    }
                }
            }
        }

        Ok(result)
    }

    pub fn get_cells_code_hashes_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        _bulk_sync_mode: bool,
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());

        // Batch get from live cells
        let keys: Vec<_> = outpoints
            .iter()
            .map(|(h, i)| {
                let key = keys::encode_outpoint(h, *i);
                (self.store.cf_live_cells(), key)
            })
            .collect();

        let key_refs: Vec<_> = keys.iter().map(|(cf, k)| (*cf, k.as_slice())).collect();
        let results = self.store.multi_get_cf(key_refs);

        for (idx, res) in results.into_iter().enumerate() {
            if let Ok(Some(value)) = res {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let (tx_hash, output_index) = outpoints[idx];
                    result.insert(
                        (tx_hash.to_vec(), output_index),
                        (info.lock_code_hash, info.type_code_hash),
                    );
                }
            }
        }

        // Check consumed cells for missing entries
        let missing: Vec<_> = outpoints
            .iter()
            .filter(|(h, i)| !result.contains_key(&(h.to_vec(), *i)))
            .collect();

        if !missing.is_empty() {
            let consumed_keys: Vec<_> = missing
                .iter()
                .map(|(h, i)| {
                    let key = keys::encode_outpoint(h, *i);
                    (self.store.cf_consumed_cells(), key)
                })
                .collect();

            let consumed_refs: Vec<_> = consumed_keys
                .iter()
                .map(|(cf, k)| (*cf, k.as_slice()))
                .collect();
            let consumed_results = self.store.multi_get_cf(consumed_refs);

            for (idx, res) in consumed_results.into_iter().enumerate() {
                if let Ok(Some(value)) = res {
                    if let Ok(info) = bincode::deserialize::<
                        ckbadger_store::types::CompactConsumedCellInfo,
                    >(&value)
                    {
                        let (tx_hash, output_index) = missing[idx];
                        result.insert(
                            (tx_hash.to_vec(), *output_index),
                            (info.lock_code_hash, info.type_code_hash),
                        );
                    }
                }
            }
        }

        Ok(result)
    }
}
