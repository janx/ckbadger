use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{decode_live_cell_marker, LiveCellInfo, PositionedCellInfo};

use crate::parser::cell::ParsedCell;

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
        precomputed_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        domain_batch: &mut StoreBatch,
        cells_batch: &mut StoreBatch,
        skip_cell_indices: bool,
    ) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        for (tx_hash, output_index, cell, _created_at_block) in cells {
            let lookup_key = (tx_hash.to_vec(), *output_index);
            let info = precomputed_infos.get(&lookup_key).cloned().ok_or_else(|| {
                anyhow!(
                    "missing precomputed cell info for insert: outpoint=0x{}:{}, precomputed_size={}",
                    hex::encode(tx_hash),
                    output_index,
                    precomputed_infos.len()
                )
            })?;
            let raw_key = keys::encode_outpoint(tx_hash, *output_index);
            // Cell payload -> append-only batch
            cells_batch.put_cell_payload(&raw_key, &info.cell);
            // Live marker -> domain batch
            domain_batch.put_live_cell_marker(&raw_key, info.created_at_block);
            if !skip_cell_indices {
                domain_batch.put_cell_by_lock(
                    &info.lock_script_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                domain_batch.put_cell_by_lock_code(
                    &info.lock_code_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                if let Some(ref type_hash) = info.type_script_hash {
                    domain_batch.put_cell_by_type(
                        type_hash,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
                if let Some(ref type_code_hash) = info.type_code_hash {
                    domain_batch.put_cell_by_type_code(
                        type_code_hash,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
                if !cell.data_hash.is_empty() {
                    domain_batch.put_cell_by_data_hash(
                        &cell.data_hash,
                        info.created_at_block,
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
        domain_batch: &mut StoreBatch,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        // Collect all outpoint keys and probe live marker set from domain store.
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
        let mut live_markers = Vec::new();
        let mut cell_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (idx, marker) in marker_results.into_iter().enumerate() {
            match marker {
                Ok(Some(marker_bytes)) => {
                    let marker = decode_live_cell_marker(&marker_bytes).ok_or_else(|| {
                        let (tx_hash, output_index, ..) = consumptions[idx];
                        anyhow!(
                            "failed to decode live cell marker during consumption: outpoint=0x{}:{}, marker_len={}",
                            hex::encode(tx_hash),
                            output_index,
                            marker_bytes.len()
                        )
                    })?;
                    present_positions.push(idx);
                    live_markers.push(marker);
                    // Read cell payloads from append-only store
                    cell_refs.push((
                        self.append_only_store.cf_cells(),
                        encoded_keys[idx].as_slice(),
                    ));
                }
                Ok(None) => {
                    let (tx_hash, output_index, ..) = consumptions[idx];
                    bail!(
                        "live cell marker not found in domain store for outpoint 0x{}:{} at input index {} during consumption",
                        hex::encode(tx_hash),
                        output_index,
                        idx
                    );
                }
                Err(e) => {
                    let (tx_hash, output_index, ..) = consumptions[idx];
                    bail!(
                        "failed to read live marker during consumption: outpoint=0x{}:{}, error={}",
                        hex::encode(tx_hash),
                        output_index,
                        e
                    );
                }
            }
        }
        let cell_results = self.append_only_store.multi_get_cf(cell_refs);
        let mut infos_by_pos: HashMap<usize, PositionedCellInfo> = HashMap::new();
        for (batch_idx, res) in cell_results.into_iter().enumerate() {
            let outpoint_idx = present_positions[batch_idx];
            let (tx_hash, output_index, ..) = consumptions[outpoint_idx];
            match res {
                Ok(Some(value)) => {
                    let info = bincode::deserialize::<LiveCellInfo>(&value).map_err(|e| {
                        anyhow!(
                            "failed to deserialize live cell payload during consumption: outpoint=0x{}:{}, error={}",
                            hex::encode(tx_hash),
                            output_index,
                            e
                        )
                    })?;
                    infos_by_pos.insert(
                        outpoint_idx,
                        PositionedCellInfo::new(info, live_markers[batch_idx]),
                    );
                }
                Ok(None) => {
                    bail!(
                        "live cell marker present but canonical cell data missing: outpoint=0x{}:{}",
                        hex::encode(tx_hash),
                        output_index
                    );
                }
                Err(e) => {
                    bail!(
                        "failed to read canonical cell info during consumption: outpoint=0x{}:{}, error={}",
                        hex::encode(tx_hash),
                        output_index,
                        e
                    );
                }
            }
        }

        // Zip results with consumptions and process writes
        for (
            idx,
            (tx_hash, output_index, _created_at_block, consumed_by_tx, consumed_at_block, _),
        ) in consumptions.iter().enumerate()
        {
            let Some(info) = infos_by_pos.get(&idx) else {
                bail!(
                    "missing live cell info during consumption: outpoint=0x{}:{}, consumed_by_tx=0x{}, consumed_at_block={}",
                    hex::encode(tx_hash),
                    output_index,
                    hex::encode(consumed_by_tx),
                    consumed_at_block
                );
            };
            let raw_key = keys::encode_outpoint(tx_hash, *output_index);
            // Write consumed metadata to domain (NOT the cell payload -- already in append-only)
            domain_batch.put_consumed_cell_meta_raw_key(
                &raw_key,
                info.created_at_block,
                *consumed_at_block,
                Some(*consumed_by_tx),
            );
            // Remove from live cells
            domain_batch.delete_cell_raw_key(&raw_key);
            // Remove cell indexes
            domain_batch.delete_cell_by_lock(
                &info.lock_script_hash,
                info.created_at_block,
                tx_hash,
                *output_index,
            );
            domain_batch.delete_cell_by_lock_code(
                &info.lock_code_hash,
                info.created_at_block,
                tx_hash,
                *output_index,
            );
            if let Some(ref type_hash) = info.type_script_hash {
                domain_batch.delete_cell_by_type(
                    type_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
            }
            if let Some(ref type_code_hash) = info.type_code_hash {
                domain_batch.delete_cell_by_type_code(
                    type_code_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
            }
        }

        Ok(())
    }

    pub fn get_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());

        let live_cells = self
            .store
            .get_cells_batch(outpoints, &self.append_only_store)?;
        for ((tx_hash, output_index), info) in live_cells {
            let PositionedCellInfo {
                cell,
                created_at_block,
            } = info;
            result.insert(
                (tx_hash, output_index),
                (
                    cell.capacity,
                    created_at_block,
                    cell.lock_script_hash,
                    cell.data_size,
                ),
            );
        }

        // Check consumed cells for missing entries
        for (tx_hash, output_index) in outpoints {
            let key = (tx_hash.to_vec(), *output_index);
            if result.contains_key(&key) {
                continue;
            }
            if let Some(info) = self.store.get_consumed_cell_info(
                tx_hash,
                *output_index,
                &self.append_only_store,
            )? {
                result.insert(
                    key,
                    (
                        info.cell.capacity,
                        info.created_at_block,
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
    ) -> Result<HashMap<(Vec<u8>, i16), PositionedCellInfo>> {
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
        let mut live_markers = Vec::new();
        let mut cell_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (idx, marker) in marker_results.into_iter().enumerate() {
            match marker {
                Ok(Some(marker_bytes)) => {
                    let marker = decode_live_cell_marker(&marker_bytes).ok_or_else(|| {
                        anyhow!(
                            "failed to decode live cell marker: outpoint=0x{}:{}, marker_len={}",
                            hex::encode(outpoints[idx].0),
                            outpoints[idx].1,
                            marker_bytes.len()
                        )
                    })?;
                    present_positions.push(idx);
                    live_markers.push(marker);
                    // Read cell payloads from append-only store
                    cell_refs.push((
                        self.append_only_store.cf_cells(),
                        encoded_outpoints[idx].as_slice(),
                    ));
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

        let cell_results = self.append_only_store.multi_get_cf(cell_refs);
        for (batch_idx, res) in cell_results.into_iter().enumerate() {
            let outpoint_idx = present_positions[batch_idx];
            let (tx_hash, output_index) = outpoints[outpoint_idx];
            match res {
                Ok(Some(value)) => {
                    let info = bincode::deserialize::<LiveCellInfo>(&value).map_err(|e| {
                        anyhow!(
                            "failed to decode live cell payload: outpoint=0x{}:{}, error={}",
                            hex::encode(tx_hash),
                            output_index,
                            e
                        )
                    })?;
                    validate_input_cell_occupied_capacity(&info, tx_hash, output_index, "live")?;
                    result.insert(
                        (tx_hash.to_vec(), output_index),
                        PositionedCellInfo::new(info, live_markers[batch_idx]),
                    );
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
        let missing: Vec<(&[u8], i16)> = outpoints
            .iter()
            .filter(|(h, i)| !result.contains_key(&(h.to_vec(), *i)))
            .map(|(h, i)| (*h, *i))
            .collect();

        if !missing.is_empty() {
            let consumed = self
                .store
                .get_consumed_cells_batch(&missing, &self.append_only_store)?;
            for ((tx_hash, output_index), live) in consumed {
                validate_input_cell_occupied_capacity(
                    &live,
                    tx_hash.as_slice(),
                    output_index,
                    "consumed",
                )?;
                result.insert((tx_hash, output_index), live);
            }
        }

        Ok(result)
    }

    pub fn consume_cells_batch_preloaded(
        &self,
        consumptions: &[(&[u8], i16, i64, &[u8], i64, i16)],
        preloaded_cells: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        same_batch_cells: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        domain_batch: &mut StoreBatch,
        skip_cell_indices: bool,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        for (tx_hash, output_index, _created_at_block, consumed_by_tx, consumed_at_block, _idx) in
            consumptions
        {
            let lookup_key = (tx_hash.to_vec(), *output_index);
            let info = preloaded_cells
                .get(&lookup_key)
                .or_else(|| same_batch_cells.get(&lookup_key));

            let Some(info) = info else {
                bail!(
                    "missing preloaded cell info during consumption: outpoint=0x{}:{}, consumed_by_tx=0x{}, consumed_at_block={}, preloaded_size={}, same_batch_size={}",
                    hex::encode(tx_hash),
                    output_index,
                    hex::encode(consumed_by_tx),
                    consumed_at_block,
                    preloaded_cells.len(),
                    same_batch_cells.len()
                );
            };

            let raw_key = keys::encode_outpoint(tx_hash, *output_index);
            // Write consumed metadata to domain (NOT the cell payload -- already in append-only)
            domain_batch.put_consumed_cell_meta_raw_key(
                &raw_key,
                info.created_at_block,
                *consumed_at_block,
                Some(*consumed_by_tx),
            );
            domain_batch.delete_cell_raw_key(&raw_key);
            if !skip_cell_indices {
                domain_batch.delete_cell_by_lock(
                    &info.lock_script_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                domain_batch.delete_cell_by_lock_code(
                    &info.lock_code_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                if let Some(ref type_hash) = info.type_script_hash {
                    domain_batch.delete_cell_by_type(
                        type_hash,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
                if let Some(ref type_code_hash) = info.type_code_hash {
                    domain_batch.delete_cell_by_type_code(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::CkbadgerStore;
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

    fn occupied_capacity_from_parsed_cell(cell: &ParsedCell) -> i64 {
        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
        let type_script_size = cell
            .type_args
            .as_ref()
            .map(|args| 32 + 1 + args.len() as i64)
            .unwrap_or(0);
        (8 + lock_script_size + type_script_size + i64::from(cell.data_size)) * 100_000_000
    }

    fn precomputed_info_map(
        tx_hash: &[u8],
        output_index: i16,
        cell: &ParsedCell,
        created_at_block: i64,
        udt_amount: Option<u128>,
    ) -> HashMap<(Vec<u8>, i16), PositionedCellInfo> {
        HashMap::from([(
            (tx_hash.to_vec(), output_index),
            PositionedCellInfo::new(
                LiveCellInfo {
                    capacity: cell.capacity,
                    lock_script_hash: cell.lock_script_hash.clone(),
                    lock_code_hash: cell.lock_code_hash.clone(),
                    lock_hash_type: cell.lock_hash_type,
                    lock_args: cell.lock_args.clone(),
                    type_script_hash: cell.type_script_hash.clone(),
                    type_code_hash: cell.type_code_hash.clone(),
                    type_hash_type: cell.type_hash_type,
                    type_args: cell.type_args.clone(),
                    data_size: cell.data_size,
                    occupied_capacity: occupied_capacity_from_parsed_cell(cell),
                    udt_amount,
                },
                created_at_block,
            ),
        )])
    }

    #[test]
    fn test_insert_cells_batch_persists_precomputed_xudt_without_amount_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        let cell = build_udt_cell(type_code_hash, vec![]);
        let tx_hash = vec![0xAA; 32];
        let all_cells = vec![(tx_hash.as_slice(), 0i16, &cell, 1i64)];
        let precomputed = precomputed_info_map(&tx_hash, 0, &cell, 1, None);

        let mut domain_batch = StoreBatch::new(&store);
        let mut cells_batch = StoreBatch::new(&store);
        writer
            .insert_cells_batch(
                &all_cells,
                &precomputed,
                &mut domain_batch,
                &mut cells_batch,
                false,
            )
            .unwrap();
        cells_batch.commit().unwrap();
        domain_batch.commit().unwrap();

        let stored = store.get_cell(&tx_hash, 0, &store).unwrap().unwrap();
        assert_eq!(stored.udt_amount, None);
    }

    #[test]
    fn test_insert_cells_batch_errors_when_precomputed_info_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        let cell = build_udt_cell(type_code_hash, vec![]);
        let tx_hash = vec![0xAB; 32];
        let all_cells = vec![(tx_hash.as_slice(), 1i16, &cell, 8i64)];
        let precomputed = HashMap::new();

        let mut domain_batch = StoreBatch::new(writer.store());
        let mut cells_batch = StoreBatch::new(writer.store());
        let err = writer
            .insert_cells_batch(
                &all_cells,
                &precomputed,
                &mut domain_batch,
                &mut cells_batch,
                false,
            )
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("missing precomputed cell info for insert"));
    }

    #[test]
    fn test_insert_cells_batch_uses_precomputed_info_without_reparsing_amount() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let cell = build_udt_cell(type_code_hash, vec![]); // invalid sUDT payload
        let tx_hash = vec![0xBC; 32];
        let output_index = 4i16;
        let all_cells = vec![(tx_hash.as_slice(), output_index, &cell, 9i64)];
        let precomputed = precomputed_info_map(&tx_hash, output_index, &cell, 9, Some(7));

        let mut domain_batch = StoreBatch::new(&store);
        let mut cells_batch = StoreBatch::new(&store);
        writer
            .insert_cells_batch(
                &all_cells,
                &precomputed,
                &mut domain_batch,
                &mut cells_batch,
                false,
            )
            .unwrap();
        cells_batch.commit().unwrap();
        domain_batch.commit().unwrap();

        let stored = store
            .get_cell(&tx_hash, output_index, &store)
            .unwrap()
            .unwrap();
        assert_eq!(stored.udt_amount, Some(7));
    }

    #[test]
    fn test_consume_cells_batch_errors_on_corrupt_cell_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xCC; 32];
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, 0);
        let marker = ckbadger_store::types::encode_live_cell_marker(1);

        // Write a live marker and corrupt canonical data
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &marker)
            .unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &[0xFF, 0xAA, 0x10])
            .unwrap();

        let consumed_by = [0xDD_u8; 32];
        let consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> =
            vec![(tx_hash.as_slice(), 0, 1, consumed_by.as_slice(), 2, 0)];

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .consume_cells_batch(&consumptions, &mut batch)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to deserialize live cell payload"),
            "expected deserialization error, got: {}",
            err
        );
    }

    #[test]
    fn test_consume_cells_batch_errors_on_missing_live_cell_info() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xEE; 32];
        let consumed_by = [0xAB_u8; 32];
        let consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> =
            vec![(tx_hash.as_slice(), 1, 10, consumed_by.as_slice(), 11, 0)];

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .consume_cells_batch(&consumptions, &mut batch)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("live cell marker not found in domain store"),
            "expected missing-live-cell error, got: {}",
            err
        );
    }

    #[test]
    fn test_consume_cells_batch_preloaded_errors_on_missing_cell_info() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xFA; 32];
        let consumed_by = [0xBC_u8; 32];
        let consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> =
            vec![(tx_hash.as_slice(), 2, 20, consumed_by.as_slice(), 21, 0)];
        let preloaded_cells: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let same_batch_cells: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .consume_cells_batch_preloaded(
                &consumptions,
                &preloaded_cells,
                &same_batch_cells,
                &mut batch,
                false,
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("missing preloaded cell info during consumption"),
            "expected missing-preloaded-cell error, got: {}",
            err
        );
    }

    #[test]
    fn test_get_full_cells_info_batch_uses_consumed_batch_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xAC; 32];
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, 0);

        // Invalid payload forces decode path; this assertion verifies batch lookup branch.
        store
            .put_cf(store.cf_consumed_cells(), &outpoint_key, &[0xFF])
            .unwrap();

        let outpoints = vec![(tx_hash.as_slice(), 0i16)];
        let err = writer.get_full_cells_info_batch(&outpoints).unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to decode consumed cell meta in get_consumed_cells_batch"),
            "expected consumed batch lookup error context, got: {}",
            err
        );
    }

    #[test]
    fn test_insert_cells_batch_populates_data_hash_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        let cell = build_udt_cell(type_code_hash, vec![0x01, 0x02, 0x03]);
        let tx_hash = vec![0xDA; 32];
        let output_index = 0i16;
        let block_num = 42i64;
        let all_cells = vec![(tx_hash.as_slice(), output_index, &cell, block_num)];
        let precomputed = precomputed_info_map(&tx_hash, output_index, &cell, block_num, None);

        let mut domain_batch = StoreBatch::new(&store);
        let mut cells_batch = StoreBatch::new(&store);
        writer
            .insert_cells_batch(
                &all_cells,
                &precomputed,
                &mut domain_batch,
                &mut cells_batch,
                false,
            )
            .unwrap();
        cells_batch.commit().unwrap();
        domain_batch.commit().unwrap();

        // The data_hash from build_udt_cell is vec![0x66; 32]
        let results = store
            .list_cells_by_data_hash(&cell.data_hash, 10, None, &store)
            .unwrap();
        assert_eq!(results.len(), 1, "expected one cell indexed by data hash");
        assert_eq!(results[0].0, tx_hash);
        assert_eq!(results[0].1, output_index);
    }

    #[test]
    fn test_insert_cells_batch_skips_data_hash_index_when_skip_indices() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        let cell = build_udt_cell(type_code_hash, vec![0x04, 0x05]);
        let tx_hash = vec![0xDB; 32];
        let all_cells = vec![(tx_hash.as_slice(), 0i16, &cell, 10i64)];
        let precomputed = precomputed_info_map(&tx_hash, 0, &cell, 10, None);

        let mut domain_batch = StoreBatch::new(&store);
        let mut cells_batch = StoreBatch::new(&store);
        writer
            .insert_cells_batch(
                &all_cells,
                &precomputed,
                &mut domain_batch,
                &mut cells_batch,
                true, // skip_cell_indices
            )
            .unwrap();
        cells_batch.commit().unwrap();
        domain_batch.commit().unwrap();

        let results = store
            .list_cells_by_data_hash(&cell.data_hash, 10, None, &store)
            .unwrap();
        assert!(
            results.is_empty(),
            "data hash index should be empty when skip_cell_indices=true"
        );
    }
}
