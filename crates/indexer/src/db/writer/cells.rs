use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::LiveCellInfo;
use ckbadger_store::CkbadgerStore;
use tracing::info;

use crate::parser::cell::ParsedCell;
use crate::parser::UdtParser;

use super::BatchWriter;

fn expected_occupied_capacity_for_cell(
    info: &LiveCellInfo,
    tx_hash: &[u8],
    output_index: i16,
    source: &str,
) -> Result<i64> {
    let data_size = i128::from(info.data_size);
    if data_size < 0 {
        bail!(
            "negative data_size while loading {} cell: outpoint=0x{}:{}, data_size={}",
            source,
            hex::encode(tx_hash),
            output_index,
            info.data_size
        );
    }

    let lock_script_size = 33_i128 + info.lock_args.len() as i128;
    let has_type_script = info.type_script_hash.is_some() || info.type_code_hash.is_some();
    let type_script_size = if has_type_script {
        let type_args = info.type_args.as_ref().ok_or_else(|| {
            anyhow!(
                "missing type_args for typed {} cell: outpoint=0x{}:{}, type_script_hash={}, type_code_hash=0x{}",
                source,
                hex::encode(tx_hash),
                output_index,
                info.type_script_hash
                    .as_ref()
                    .map(|v| format!("0x{}", hex::encode(v)))
                    .unwrap_or_else(|| "none".to_string()),
                info.type_code_hash
                    .as_ref()
                    .map(hex::encode)
                    .unwrap_or_else(|| "none".to_string())
            )
        })?;
        33_i128 + type_args.len() as i128
    } else {
        0
    };

    let occupied = (8_i128 + lock_script_size + type_script_size + data_size)
        .checked_mul(100_000_000_i128)
        .ok_or_else(|| {
            anyhow!(
                "occupied capacity overflow while loading {} cell: outpoint=0x{}:{}",
                source,
                hex::encode(tx_hash),
                output_index
            )
        })?;

    i64::try_from(occupied).map_err(|_| {
        anyhow!(
            "occupied capacity over i64 range while loading {} cell: outpoint=0x{}:{}, occupied={}",
            source,
            hex::encode(tx_hash),
            output_index,
            occupied
        )
    })
}

fn validate_input_cell_occupied_capacity(
    info: &LiveCellInfo,
    tx_hash: &[u8],
    output_index: i16,
    source: &str,
) -> Result<()> {
    let expected = expected_occupied_capacity_for_cell(info, tx_hash, output_index, source)?;
    if info.occupied_capacity <= 0 {
        bail!(
            "invalid occupied capacity while loading {} cell: outpoint=0x{}:{}, occupied={}, expected={}",
            source,
            hex::encode(tx_hash),
            output_index,
            info.occupied_capacity,
            expected
        );
    }
    if info.occupied_capacity != expected {
        bail!(
            "occupied capacity mismatch while loading {} cell: outpoint=0x{}:{}, occupied={}, expected={}",
            source,
            hex::encode(tx_hash),
            output_index,
            info.occupied_capacity,
            expected
        );
    }
    if info.occupied_capacity > info.capacity {
        bail!(
            "invalid occupied capacity while loading {} cell: outpoint=0x{}:{}, occupied={}, capacity={}, expected={}",
            source,
            hex::encode(tx_hash),
            output_index,
            info.occupied_capacity,
            info.capacity,
            expected
        );
    }
    Ok(())
}

impl BatchWriter {
    pub fn insert_cells_batch(
        &self,
        cells: &[(&[u8], i16, &ParsedCell, i64)],
        batch: &mut StoreBatch,
        skip_cell_indices: bool,
    ) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        for (tx_hash, output_index, cell, created_at_block) in cells {
            // Compute occupied capacity:
            // occupied = (8 + lock_script_size + type_script_size + data_size) * 100_000_000
            // lock_script_size = 32 (code_hash) + 1 (hash_type) + lock_args.len()
            // type_script_size = if type_script { 32 + 1 + type_args.len() } else { 0 }
            let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
            let type_script_size = cell
                .type_args
                .as_ref()
                .map(|args| 32 + 1 + args.len() as i64)
                .unwrap_or(0);
            let occupied_capacity =
                (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;

            let udt_amount = match (cell.type_code_hash.as_deref(), cell.type_hash_type) {
                (Some(type_code_hash), Some(hash_type)) => {
                    match UdtParser::is_udt_code_hash_bytes(type_code_hash, hash_type) {
                        Some(crate::parser::UdtStandard::Xudt) => {
                            // xUDT-compatible cells can be non-fungible metadata/owner cells.
                            // Those cells intentionally carry no 16-byte token amount.
                            UdtParser::parse_amount(&cell.data)
                        }
                        Some(crate::parser::UdtStandard::Sudt) => {
                            let amount = UdtParser::parse_amount(&cell.data).ok_or_else(|| {
                                anyhow!(
                                    "failed to parse UDT amount from output cell data: outpoint=0x{}:{}, type_code_hash=0x{}",
                                    hex::encode(tx_hash),
                                    output_index,
                                    hex::encode(type_code_hash)
                                )
                            })?;
                            Some(amount)
                        }
                        None => None,
                    }
                }
                _ => None,
            };

            let info = LiveCellInfo {
                capacity: cell.capacity,
                created_at_block: *created_at_block,
                lock_script_hash: cell.lock_script_hash.clone(),
                lock_code_hash: cell.lock_code_hash.clone(),
                lock_hash_type: cell.lock_hash_type,
                lock_args: cell.lock_args.clone(),
                type_script_hash: cell.type_script_hash.clone(),
                type_code_hash: cell.type_code_hash.clone(),
                type_args: cell.type_args.clone(),
                data_size: cell.data_size,
                occupied_capacity,
                udt_amount,
            };
            batch.put_cell(tx_hash, *output_index, &info);
            if !skip_cell_indices {
                batch.put_cell_by_lock(
                    &cell.lock_script_hash,
                    *created_at_block,
                    tx_hash,
                    *output_index,
                );
                batch.put_cell_by_lock_code(
                    &cell.lock_code_hash,
                    *created_at_block,
                    tx_hash,
                    *output_index,
                );
                if let Some(ref type_hash) = cell.type_script_hash {
                    batch.put_cell_by_type(type_hash, *created_at_block, tx_hash, *output_index);
                }
                if let Some(ref type_code_hash) = cell.type_code_hash {
                    batch.put_cell_by_type_code(
                        type_code_hash,
                        *created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
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

        // Collect all outpoint keys and probe live marker set.
        let encoded_keys: Vec<_> = consumptions
            .iter()
            .map(|(tx_hash, output_index, ..)| keys::encode_outpoint(tx_hash, *output_index))
            .collect();

        let marker_refs: Vec<_> = encoded_keys
            .iter()
            .map(|k| (self.store.cf_live_cells(), k.as_slice()))
            .collect();
        let marker_results = self.store.multi_get_cf(marker_refs);

        let mut present_positions = Vec::new();
        let mut cell_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (idx, marker) in marker_results.into_iter().enumerate() {
            if matches!(marker, Ok(Some(_))) {
                present_positions.push(idx);
                cell_refs.push((self.store.cf_cells(), encoded_keys[idx].as_slice()));
            }
        }
        let cell_results = self.store.multi_get_cf(cell_refs);
        let mut infos_by_pos: HashMap<usize, LiveCellInfo> = HashMap::new();
        for (batch_idx, res) in cell_results.into_iter().enumerate() {
            if let Ok(Some(value)) = res {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    infos_by_pos.insert(present_positions[batch_idx], info);
                }
            }
        }

        // Zip results with consumptions and process writes
        for (
            idx,
            (tx_hash, output_index, _created_at_block, consumed_by_tx, consumed_at_block, _),
        ) in consumptions.iter().enumerate()
        {
            if let Some(info) = infos_by_pos.get(&idx) {
                // Move to consumed cells
                batch.put_consumed_cell_with_consumer(
                    tx_hash,
                    *output_index,
                    info,
                    *consumed_at_block,
                    Some(*consumed_by_tx),
                );
                // Remove from live cells
                batch.delete_cell(tx_hash, *output_index);
                // Remove cell indexes
                batch.delete_cell_by_lock(
                    &info.lock_script_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                batch.delete_cell_by_lock_code(
                    &info.lock_code_hash,
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
                if let Some(ref type_code_hash) = info.type_code_hash {
                    batch.delete_cell_by_type_code(
                        type_code_hash,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
            }
        }

        Ok(())
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

        let live_cells = self.store.get_cells_batch(outpoints);
        for ((tx_hash, output_index), info) in live_cells {
            result.insert(
                (tx_hash, output_index),
                (
                    info.capacity,
                    info.created_at_block,
                    info.lock_script_hash,
                    info.data_size,
                ),
            );
        }

        // Check consumed cells for missing entries
        for (tx_hash, output_index) in outpoints {
            let key = (tx_hash.to_vec(), *output_index);
            if result.contains_key(&key) {
                continue;
            }
            if let Some(info) = self.store.get_consumed_cell_info(tx_hash, *output_index)? {
                result.insert(
                    key,
                    (
                        info.cell.capacity,
                        info.cell.created_at_block,
                        info.cell.lock_script_hash,
                        info.cell.data_size,
                    ),
                );
            }
        }

        Ok(result)
    }

    pub fn get_full_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        _bulk_sync_mode: bool,
    ) -> Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());

        // Batch read live markers, then load canonical cell payloads for present outpoints.
        let encoded_outpoints: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(h, i)| keys::encode_outpoint(h, *i))
            .collect();
        let marker_refs: Vec<_> = encoded_outpoints
            .iter()
            .map(|k| (self.store.cf_live_cells(), k.as_slice()))
            .collect();
        let marker_results = self.store.multi_get_cf(marker_refs);

        let mut present_positions = Vec::new();
        let mut cell_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (idx, marker) in marker_results.into_iter().enumerate() {
            match marker {
                Ok(Some(_)) => {
                    present_positions.push(idx);
                    cell_refs.push((self.store.cf_cells(), encoded_outpoints[idx].as_slice()));
                }
                Ok(None) => {}
                Err(e) => {
                    let (tx_hash, output_index) = outpoints[idx];
                    bail!(
                        "failed to read live marker: outpoint=0x{}:{}, error={}",
                        hex::encode(tx_hash),
                        output_index,
                        e
                    );
                }
            }
        }

        let cell_results = self.store.multi_get_cf(cell_refs);
        for (batch_idx, res) in cell_results.into_iter().enumerate() {
            let outpoint_idx = present_positions[batch_idx];
            let (tx_hash, output_index) = outpoints[outpoint_idx];
            match res {
                Ok(Some(value)) => {
                    let info = bincode::deserialize::<LiveCellInfo>(&value).map_err(|e| {
                        anyhow!(
                            "failed to decode live cell info: outpoint=0x{}:{}, error={}",
                            hex::encode(tx_hash),
                            output_index,
                            e
                        )
                    })?;
                    validate_input_cell_occupied_capacity(&info, tx_hash, output_index, "live")?;
                    result.insert((tx_hash.to_vec(), output_index), info);
                }
                Ok(None) => {}
                Err(e) => {
                    bail!(
                        "failed to read canonical cell info: outpoint=0x{}:{}, error={}",
                        hex::encode(tx_hash),
                        output_index,
                        e
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
            for (tx_hash, output_index) in missing {
                if let Some(info) = self.store.get_consumed_cell_info(tx_hash, *output_index)? {
                    let live = info.to_live_cell_info();
                    validate_input_cell_occupied_capacity(
                        &live,
                        tx_hash,
                        *output_index,
                        "consumed",
                    )?;
                    result.insert((tx_hash.to_vec(), *output_index), live);
                }
            }
        }

        Ok(result)
    }

    pub fn consume_cells_batch_preloaded(
        &self,
        consumptions: &[(&[u8], i16, i64, &[u8], i64, i16)],
        preloaded_cells: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
        same_batch_cells: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
        batch: &mut StoreBatch,
        skip_cell_indices: bool,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        for (tx_hash, output_index, _created_at_block, consumed_by_tx, consumed_at_block, _idx) in
            consumptions
        {
            let key = (tx_hash.to_vec(), *output_index);
            let info = preloaded_cells
                .get(&key)
                .or_else(|| same_batch_cells.get(&key));

            if let Some(info) = info {
                batch.put_consumed_cell_with_consumer(
                    tx_hash,
                    *output_index,
                    info,
                    *consumed_at_block,
                    Some(*consumed_by_tx),
                );
                batch.delete_cell(tx_hash, *output_index);
                if !skip_cell_indices {
                    batch.delete_cell_by_lock(
                        &info.lock_script_hash,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                    batch.delete_cell_by_lock_code(
                        &info.lock_code_hash,
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
                    if let Some(ref type_code_hash) = info.type_code_hash {
                        batch.delete_cell_by_type_code(
                            type_code_hash,
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
}

/// Rebuild the 4 cell secondary index CFs by scanning all live cells.
///
/// This is called after bulk sync completes, when cell indices were skipped
/// during sync to reduce write volume. Iterates CF_LIVE_CELLS and writes the
/// corresponding entries into CELL_BY_LOCK, CELL_BY_TYPE, CELL_BY_LOCK_CODE,
/// and CELL_BY_TYPE_CODE.
pub fn rebuild_cell_indices(store: &CkbadgerStore) {
    const BATCH_SIZE: usize = 50_000;
    const LOG_INTERVAL: usize = 1_000_000;

    info!("Cell index rebuild: scanning LIVE_CELLS");

    let cf = store.cf_live_cells();
    let mut batch = StoreBatch::new(store);
    let mut count: usize = 0;
    let mut last_log: usize = 0;
    let start = std::time::Instant::now();

    for item in store.iterator_cf(cf, rocksdb::IteratorMode::Start) {
        let (key, _) = match item {
            Ok(kv) => kv,
            Err(e) => {
                tracing::warn!("Cell index rebuild: iterator error: {}", e);
                continue;
            }
        };

        let (tx_hash, output_index) = keys::decode_outpoint(&key);
        let info: LiveCellInfo = match store.get_cell_by_outpoint_key(&key) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("Cell index rebuild: failed to load canonical cell: {}", e);
                continue;
            }
        };

        batch.put_cell_by_lock(
            &info.lock_script_hash,
            info.created_at_block,
            &tx_hash,
            output_index,
        );
        batch.put_cell_by_lock_code(
            &info.lock_code_hash,
            info.created_at_block,
            &tx_hash,
            output_index,
        );
        if let Some(ref type_hash) = info.type_script_hash {
            batch.put_cell_by_type(type_hash, info.created_at_block, &tx_hash, output_index);
        }
        if let Some(ref type_code_hash) = info.type_code_hash {
            batch.put_cell_by_type_code(
                type_code_hash,
                info.created_at_block,
                &tx_hash,
                output_index,
            );
        }

        count += 1;
        if count % BATCH_SIZE == 0 {
            if let Err(e) = batch.commit() {
                tracing::error!("Cell index rebuild: batch commit error: {}", e);
                return;
            }
            batch = StoreBatch::new(store);
        }
        if count - last_log >= LOG_INTERVAL {
            let elapsed = start.elapsed().as_secs();
            let rate = if elapsed > 0 {
                count as f64 / elapsed as f64
            } else {
                0.0
            };
            info!(
                "Cell index rebuild: {} cells processed ({:.0} cells/s)",
                count, rate
            );
            last_log = count;
        }
    }

    // Commit remaining
    if count % BATCH_SIZE != 0 {
        if let Err(e) = batch.commit() {
            tracing::error!("Cell index rebuild: final commit error: {}", e);
            return;
        }
    }

    let elapsed_secs = start.elapsed().as_secs();
    info!(
        "Cell index rebuild complete: {} cells indexed in {}s",
        count, elapsed_secs
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn build_udt_cell(type_code_hash: Vec<u8>, data: Vec<u8>) -> ParsedCell {
        ParsedCell {
            capacity: 100_000_000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: vec![0x66; 32],
            data_size: data.len() as i32,
            data,
        }
    }

    #[test]
    fn test_insert_cells_batch_allows_xudt_cells_without_amount_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        let cell = build_udt_cell(type_code_hash, vec![]);
        let tx_hash = vec![0xAA; 32];
        let all_cells = vec![(tx_hash.as_slice(), 0i16, &cell, 1i64)];

        let mut batch = StoreBatch::new(&store);
        writer
            .insert_cells_batch(&all_cells, &mut batch, false)
            .unwrap();
        batch.commit().unwrap();

        let stored = store.get_cell(&tx_hash, 0).unwrap().unwrap();
        assert_eq!(stored.udt_amount, None);
    }

    #[test]
    fn test_insert_cells_batch_rejects_invalid_sudt_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store);

        let type_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let cell = build_udt_cell(type_code_hash, vec![]);
        let tx_hash = vec![0xBB; 32];
        let all_cells = vec![(tx_hash.as_slice(), 3i16, &cell, 1i64)];

        let mut batch = StoreBatch::new(writer.store());
        let err = writer
            .insert_cells_batch(&all_cells, &mut batch, false)
            .unwrap_err();
        assert!(err.to_string().contains("failed to parse UDT amount"));
    }
}
