use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    decode_live_cell_marker, LiveCellInfo, LockScriptEntry, PositionedCellInfo,
};

use crate::parser::cell::ParsedCell;

use super::BatchWriter;

/// A live-cell marker is present in the domain store but its canonical payload
/// is missing from the append-only store.
///
/// This is a hard cross-store invariant violation. A marker is only ever written
/// *after* its payload commits (append-only commits first, synced, before the
/// domain batch — see the live finalize path), so a visible marker implies a
/// committed payload. When it does not, the store was left inconsistent by an
/// unclean shutdown that lost unsynced append-only writes. The payload will
/// never appear on its own, so this must fail fast — never be retried as a
/// transient "writer behind" condition.
#[derive(Debug, thiserror::Error)]
#[error("missing canonical cell for live marker in {context}: outpoint=0x{outpoint}:{index}")]
pub struct MissingCanonicalCellError {
    pub outpoint: String,
    pub index: i16,
    pub context: &'static str,
}

/// True when `err` is a [`MissingCanonicalCellError`]. The parser uses this to
/// decide fail-fast (true) vs retry (false) when a cell lookup fails.
pub fn is_cross_store_inconsistency(err: &anyhow::Error) -> bool {
    err.downcast_ref::<MissingCanonicalCellError>().is_some()
}

/// Compute CKB occupied capacity in shannons for a cell.
///
/// `data_size`: length of cell data in bytes
/// `lock_args_len`: length of lock script args in bytes
/// `type_args_len`: Some(len) if type script present, None if no type script
pub fn compute_occupied_capacity_shannons(
    data_size: usize,
    lock_args_len: usize,
    type_args_len: Option<usize>,
) -> Result<i64> {
    ckbadger_common::dao::occupied_capacity_shannons(data_size, lock_args_len, type_args_len)
}

/// A stored cell must hold at least the capacity its own fields occupy (CKB
/// consensus). The occupied capacity is derived from those fields, never
/// stored, so this compares two different things: the stored capacity and
/// the size of the stored scripts and data.
fn validate_input_cell_occupied_capacity(
    info: &LiveCellInfo,
    tx_hash: &[u8],
    output_index: i16,
    source: &str,
) -> Result<()> {
    let occupied = info.occupied_capacity();
    if occupied > info.capacity {
        bail!(
            "invalid occupied capacity while loading {} cell: outpoint=0x{}:{}, occupied={}, capacity={}",
            source,
            hex::encode(tx_hash),
            output_index,
            occupied,
            info.capacity
        );
    }
    Ok(())
}

/// Resolve the lock hash_type key byte for a cell's cell-by-lock-code index
/// row, failing fast with outpoint context on corrupt stored values.
fn lock_code_index_hash_type(info: &LiveCellInfo, tx_hash: &[u8], output_index: i16) -> Result<u8> {
    keys::cell_code_index_hash_type_byte(info.lock_hash_type).with_context(|| {
        format!(
            "invalid lock hash_type for cell-by-lock-code index: outpoint=0x{}:{}, hash_type={}",
            hex::encode(tx_hash),
            output_index,
            info.lock_hash_type
        )
    })
}

/// Resolve the type hash_type key byte for a cell's cell-by-type-code index
/// row. The caller has already established `type_code_hash` is present, so a
/// missing `type_hash_type` is a payload invariant violation.
fn type_code_index_hash_type(info: &LiveCellInfo, tx_hash: &[u8], output_index: i16) -> Result<u8> {
    let type_hash_type = info.type_hash_type.ok_or_else(|| {
        anyhow!(
            "cell has type_code_hash but no type_hash_type: outpoint=0x{}:{}",
            hex::encode(tx_hash),
            output_index
        )
    })?;
    keys::cell_code_index_hash_type_byte(type_hash_type).with_context(|| {
        format!(
            "invalid type hash_type for cell-by-type-code index: outpoint=0x{}:{}, hash_type={}",
            hex::encode(tx_hash),
            output_index,
            type_hash_type
        )
    })
}

impl BatchWriter {
    /// Stage the batch's created cells: each payload into `cells_batch`
    /// (append-only) and its live marker, lock script and index rows into
    /// `domain_batch` in the scope of the block that created the cell.
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

        for (tx_hash, output_index, cell, created_at_block) in cells {
            let lookup_key = (tx_hash.to_vec(), *output_index);
            let info = precomputed_infos.get(&lookup_key).cloned().ok_or_else(|| {
                anyhow!(
                    "missing precomputed cell info for insert: outpoint=0x{}:{}, precomputed_size={}",
                    hex::encode(tx_hash),
                    output_index,
                    precomputed_infos.len()
                )
            })?;
            if info.created_at_block != *created_at_block {
                bail!(
                    "precomputed cell info names another creation block: outpoint=0x{}:{}, created_at_block={}, info_created_at_block={}",
                    hex::encode(tx_hash),
                    output_index,
                    created_at_block,
                    info.created_at_block
                );
            }
            // Every domain row of the cell belongs to the block that created it.
            let mut domain_batch = domain_batch.at(*created_at_block);
            let raw_key = keys::encode_outpoint(tx_hash, *output_index);
            // Cell payload -> append-only batch
            cells_batch.put_cell_payload(&raw_key, &info.cell)?;
            // Live marker -> domain batch
            domain_batch.put_live_cell_marker(&raw_key, info.created_at_block);
            // Index keys take the script hashes the parser computed for this
            // output; the payload stores none.
            // Lock script mapping (idempotent, survives cell consumption)
            domain_batch.put_lock_script(
                &cell.lock_script_hash,
                &LockScriptEntry {
                    code_hash: info.lock_code_hash.to_vec(),
                    hash_type: info.lock_hash_type,
                    args: info.lock_args.clone(),
                },
            );
            if !skip_cell_indices {
                domain_batch.put_cell_by_lock(
                    &cell.lock_script_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                domain_batch.put_cell_by_lock_code(
                    &info.lock_code_hash,
                    lock_code_index_hash_type(&info, tx_hash, *output_index)?,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                if let Some(type_hash) = &cell.type_script_hash {
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
                        type_code_index_hash_type(&info, tx_hash, *output_index)?,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
                if cell.is_code_cell() {
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

    pub fn get_full_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), PositionedCellInfo>> {
        self.get_full_cells_info_batch_chunk(outpoints)
    }

    /// Read a single chunk of outpoints. Callers issuing very large batches
    /// (thousands of outpoints) should split the input themselves and accumulate
    /// the results so they can observe per-chunk latency and yield between
    /// chunks. This function performs the actual two-stage RocksDB lookup
    /// (live marker → cell payload, with consumed-cell fallback).
    pub fn get_full_cells_info_batch_chunk(
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
        let marker_results = self.store.multi_get_cf_sorted(marker_refs);

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

        let cell_results = self.append_only_store.multi_get_cf_sorted(cell_refs);
        for (batch_idx, res) in cell_results.into_iter().enumerate() {
            let outpoint_idx = present_positions[batch_idx];
            let (tx_hash, output_index) = outpoints[outpoint_idx];
            match res {
                Ok(Some(value)) => {
                    let info = LiveCellInfo::decode(&value).map_err(|e| {
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
                Ok(None) => {
                    return Err(MissingCanonicalCellError {
                        outpoint: hex::encode(tx_hash),
                        index: output_index,
                        context: "get_full_cells_info_batch",
                    }
                    .into());
                }
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

    /// Stage the batch's consumed cells into `domain_batch`, each consumed
    /// marker, live-marker delete and index-row delete in the scope of the
    /// block whose transaction consumed the cell.
    pub fn consume_cells_batch_preloaded(
        &self,
        consumptions: &[(&[u8], i16, i64, &[u8], i64, i32)],
        preloaded_cells: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        same_batch_cells: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        script_hashes: &crate::sync::types::InputScriptHashes,
        domain_batch: &mut StoreBatch,
        skip_cell_indices: bool,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        for (
            tx_hash,
            output_index,
            _created_at_block,
            consumed_by_tx,
            consumed_at_block,
            consumed_tx_idx,
        ) in consumptions
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
            // Every domain row the consumption moves belongs to the consuming block.
            let mut domain_batch = domain_batch.at(*consumed_at_block);
            // Write consumed metadata to domain (NOT the cell payload -- already in append-only)
            domain_batch.put_consumed_cell_meta_raw_key(
                &raw_key,
                info.created_at_block,
                *consumed_at_block,
                *consumed_tx_idx,
            );
            domain_batch.delete_cell_raw_key(&raw_key);
            if !skip_cell_indices {
                // The input's script hashes, derived once by the parser.
                let (lock_script_hash, type_script_hash) =
                    script_hashes.get(&lookup_key).ok_or_else(|| {
                        anyhow!(
                            "consumed cell has no derived script hashes: outpoint=0x{}:{}, consumed_by_tx=0x{}",
                            hex::encode(tx_hash),
                            output_index,
                            hex::encode(consumed_by_tx)
                        )
                    })?;
                domain_batch.delete_cell_by_lock(
                    lock_script_hash,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                domain_batch.delete_cell_by_lock_code(
                    &info.lock_code_hash,
                    lock_code_index_hash_type(info, tx_hash, *output_index)?,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                );
                if let Some(type_hash) = type_script_hash {
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
                        type_code_index_hash_type(info, tx_hash, *output_index)?,
                        info.created_at_block,
                        tx_hash,
                        *output_index,
                    );
                }
                // NOTE: CF_CELL_BY_DATA_HASH entries are intentionally preserved on consumption.
                // See comment in consume_cells() for rationale.
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

    /// A parsed UDT output whose script hashes are the parser's, computed from
    /// its script fields.
    fn build_udt_cell(type_code_hash: Vec<u8>, data: Vec<u8>) -> ParsedCell {
        let script_hash = crate::parser::ScriptParser::compute_script_hash_raw;
        ParsedCell {
            capacity: 100_000_000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: script_hash(&[0x11; 32], 1, &[0x22; 20]),
            type_script_hash: Some(script_hash(&type_code_hash, 1, &[0x44; 32])),
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            data_hash: [0x66; 32],
            data_size: data.len() as i32,
            data,
        }
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
                {
                    let mut info = cell.to_live_cell_info(udt_amount).unwrap();
                    if cell.data_size == 0 {
                        info.data_hash = None;
                    }
                    info
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

    /// The writer reads each consumed input's script hashes from what the
    /// parser shipped (it derives none itself): the index rows go under those
    /// keys, and an input missing from the shipped map is an error.
    #[test]
    fn test_consume_cells_batch_preloaded_uses_the_shipped_script_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xFB; 32];
        let consumed_by = [0xBD_u8; 32];
        let consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i32)> =
            vec![(tx_hash.as_slice(), 0, 20, consumed_by.as_slice(), 21, 1)];
        let info = PositionedCellInfo::new(
            build_udt_cell(vec![0x77; 32], vec![0u8; 16])
                .to_live_cell_info(None)
                .unwrap(),
            20,
        );
        let lock_hash = info.lock_script_hash();
        let type_hash = info.type_script_hash().unwrap();
        let preloaded_cells = HashMap::from([((tx_hash.clone(), 0i16), info)]);
        let same_batch_cells = HashMap::new();

        let mut setup = StoreBatch::new(&store);
        setup.put_cell_by_lock(&lock_hash, 20, &tx_hash, 0);
        setup.put_cell_by_type(&type_hash, 20, &tx_hash, 0);
        setup.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .consume_cells_batch_preloaded(
                &consumptions,
                &preloaded_cells,
                &same_batch_cells,
                &crate::sync::types::InputScriptHashes::new(),
                &mut batch,
                false,
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("consumed cell has no derived script hashes")
                && err.to_string().contains(&hex::encode(&tx_hash)),
            "{err}"
        );

        let shipped = crate::sync::types::InputScriptHashes::from([(
            (tx_hash.clone(), 0i16),
            (lock_hash, Some(type_hash)),
        )]);
        let mut batch = StoreBatch::new(&store);
        writer
            .consume_cells_batch_preloaded(
                &consumptions,
                &preloaded_cells,
                &same_batch_cells,
                &shipped,
                &mut batch,
                false,
            )
            .unwrap();
        batch.commit().unwrap();
        assert!(store
            .list_cells_by_lock(&lock_hash, 10, None, &store)
            .unwrap()
            .is_empty());
        assert!(store
            .list_cells_by_type(&type_hash, 10, None, &store)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_consume_cells_batch_preloaded_errors_on_missing_cell_info() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xFA; 32];
        let consumed_by = [0xBC_u8; 32];
        let consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i32)> =
            vec![(tx_hash.as_slice(), 2, 20, consumed_by.as_slice(), 21, 0)];
        let preloaded_cells: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let same_batch_cells: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .consume_cells_batch_preloaded(
                &consumptions,
                &preloaded_cells,
                &same_batch_cells,
                &crate::sync::types::InputScriptHashes::new(),
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
    fn test_get_full_cells_info_batch_errors_on_missing_append_only_payload() {
        // Regression: if a live marker exists in domain store but the append-only
        // cell payload is missing, this is an invariant violation that must fail-fast.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xEE; 32];
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, 0);

        // Write only a live marker (domain) — no cell payload in append-only.
        let marker = ckbadger_store::types::encode_live_cell_marker(100);
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &marker)
            .unwrap();

        let outpoints = vec![(tx_hash.as_slice(), 0i16)];
        let err = writer.get_full_cells_info_batch(&outpoints).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing canonical cell for live marker"),
            "expected invariant violation error, got: {}",
            err
        );
    }

    #[test]
    fn test_missing_append_only_payload_is_classified_cross_store_inconsistency() {
        // Regression for the crash-loop root cause: a live marker present in the
        // domain store with a missing append-only payload must be classifiable as
        // a *cross-store inconsistency* so the parser fails fast, rather than being
        // retried as a transient "writer behind" condition (which livelocked the
        // indexer for ~9h in production, ~250 restarts).
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let tx_hash = vec![0xEE; 32];
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, 0);
        let marker = ckbadger_store::types::encode_live_cell_marker(100);
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &marker)
            .unwrap();

        let outpoints = vec![(tx_hash.as_slice(), 0i16)];
        let err = writer.get_full_cells_info_batch(&outpoints).unwrap_err();

        // The parser uses this classifier to decide fail-fast (true) vs retry.
        assert!(
            is_cross_store_inconsistency(&err),
            "marker-without-payload must be a cross-store inconsistency (fail-fast), got: {}",
            err
        );

        // A genuinely transient error (e.g. lookup timeout) must NOT be
        // classified as a cross-store inconsistency — it stays retryable.
        let transient =
            anyhow!("parser cell lookup exceeded hard cap 1500ms (chunk 0, total_keys=10)");
        assert!(
            !is_cross_store_inconsistency(&transient),
            "transient lookup error must remain retryable"
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

    /// A cell with `data` under an optional type script `(code_hash,
    /// hash_type)`, with its real data hash.
    fn build_data_cell(data: Vec<u8>, type_script: Option<([u8; 32], i16)>) -> ParsedCell {
        let script_hash = crate::parser::ScriptParser::compute_script_hash_raw;
        ParsedCell {
            capacity: 100_000_000_000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: script_hash(&[0x11; 32], 1, &[0x22; 20]),
            type_script_hash: type_script.map(|(code_hash, hash_type)| {
                script_hash(&code_hash, hash_type as u8, &[0x33; 32])
            }),
            type_code_hash: type_script.map(|(code_hash, _)| code_hash.to_vec()),
            type_hash_type: type_script.map(|(_, hash_type)| hash_type),
            type_args: type_script.map(|_| vec![0x33; 32]),
            data_hash: crate::parser::ScriptParser::compute_data_hash(&data),
            data_size: data.len() as i32,
            data,
        }
    }

    /// C-2: `cell_by_data_hash` files exactly the code cells — a RISC-V
    /// binary, or a Type ID cell with data — and no other cell with data.
    #[test]
    fn test_insert_cells_batch_indexes_only_code_cells_by_data_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut type_id = [0u8; 32];
        type_id[25..].copy_from_slice(b"TYPE_ID");
        let xudt: [u8; 32] =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE)
                .try_into()
                .unwrap();
        let elf = build_data_cell(b"\x7fELF\x02\x01\x01binary".to_vec(), None);
        let deployment = build_data_cell(b"upgradable state".to_vec(), Some((type_id, 1)));
        let udt = build_data_cell(1_000u128.to_le_bytes().to_vec(), Some((xudt, 1)));
        let plain_data = build_data_cell(b"not a binary".to_vec(), None);
        let tx_hash = vec![0xDA; 32];
        let block_num = 42i64;
        let cells = [&elf, &deployment, &udt, &plain_data];
        let all_cells: Vec<_> = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| (tx_hash.as_slice(), index as i16, *cell, block_num))
            .collect();
        let mut precomputed = HashMap::new();
        for (index, cell) in cells.iter().enumerate() {
            precomputed.extend(precomputed_info_map(
                &tx_hash,
                index as i16,
                cell,
                block_num,
                None,
            ));
        }

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

        let rows: Vec<Vec<u8>> = store
            .iterator_cf(store.cf_cell_by_data_hash(), rocksdb::IteratorMode::Start)
            .map(|item| item.unwrap().0.to_vec())
            .collect();
        let mut expected = vec![
            keys::encode_cell_index_key(&elf.data_hash, block_num, &tx_hash, 0),
            keys::encode_cell_index_key(&deployment.data_hash, block_num, &tx_hash, 1),
        ];
        expected.sort();
        assert_eq!(
            rows, expected,
            "only the ELF binary and the Type ID deployment are code cells"
        );
    }

    #[test]
    fn test_insert_cells_batch_skips_data_hash_index_when_skip_indices() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        // A code cell, so the skip flag is the only reason it has no row.
        let cell = build_data_cell(b"\x7fELF\x02\x01\x01skipped".to_vec(), None);
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

    #[test]
    fn test_compute_occupied_capacity_no_type_script() {
        // 8 (capacity) + 33 (lock) + 20 (lock_args) + 0 (data) = 61 bytes
        // 61 * 100_000_000 = 6_100_000_000
        let result = compute_occupied_capacity_shannons(0, 20, None).unwrap();
        assert_eq!(result, 6_100_000_000);
    }

    #[test]
    fn test_compute_occupied_capacity_with_type_script() {
        // 8 + 33 + 20 + 33 + 32 + 100 = 226 bytes
        // 226 * 100_000_000 = 22_600_000_000
        let result = compute_occupied_capacity_shannons(100, 20, Some(32)).unwrap();
        assert_eq!(result, 22_600_000_000);
    }

    #[test]
    fn test_compute_occupied_matches_old_inline_formula() {
        // Verify the new function matches the old inline formula from activities.rs
        let data_size = 50usize;
        let lock_args_len = 20usize;
        let type_args_len = 32usize;

        // Old formula (from activities.rs): (32 + 1) for script size
        let lock_script_size = 32 + 1 + lock_args_len as i64;
        let type_script_size = 32 + 1 + type_args_len as i64;
        let old_result = (8 + lock_script_size + type_script_size + data_size as i64) * 100_000_000;

        // New function uses 33 for script size (same as 32+1)
        let new_result =
            compute_occupied_capacity_shannons(data_size, lock_args_len, Some(type_args_len))
                .unwrap();

        assert_eq!(old_result, new_result);
    }
}
