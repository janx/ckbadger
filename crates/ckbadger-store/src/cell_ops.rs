//! Cell read/write operations.

use std::collections::HashMap;

use crate::batch::StoreBatch;
use crate::keys;
use crate::store::{CkbadgerStore, CF_CELL_INDEX, CF_CELL_PAYLOADS, CF_CELL_STATE};
use crate::types::{
    CellIndexTag, CellState, CellStateKind, ConsumedCellInfo, ConsumedCellMeta, LiveCellInfo,
};

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

/// Aggregated cell statistics for a token.
#[derive(Debug, Clone, Default)]
pub struct TokenCellStats {
    pub cells_count: i64,
    pub total_capacity: i128,
    pub total_occupied_capacity: i128,
}

fn decode_cell_state(
    outpoint_key: &[u8],
    value: &[u8],
    context: &str,
) -> anyhow::Result<CellState> {
    bincode::deserialize::<CellState>(value).map_err(|e| {
        anyhow::anyhow!(
            "failed to deserialize cell_state in {context}: outpoint=0x{}, error={}",
            bytes_to_hex(outpoint_key),
            e
        )
    })
}

fn decode_cell_payload(
    payload_key: &[u8],
    value: &[u8],
    context: &str,
) -> anyhow::Result<LiveCellInfo> {
    bincode::deserialize::<LiveCellInfo>(value).map_err(|e| {
        anyhow::anyhow!(
            "failed to deserialize canonical cell payload in {context}: payload_key=0x{}, error={}",
            bytes_to_hex(payload_key),
            e
        )
    })
}

impl CkbadgerStore {
    fn ensure_unified_cell_payload_layout(&self, context: &str) -> anyhow::Result<()> {
        let has_state = self.has_cf(CF_CELL_STATE);
        let has_payloads = self.has_cf(CF_CELL_PAYLOADS);
        if has_state && has_payloads {
            return Ok(());
        }
        anyhow::bail!(
            "{context} requires explicit payload_store when store lacks unified cell payload layout: has_cell_state={}, has_cell_payloads={}",
            has_state,
            has_payloads
        );
    }

    fn ensure_unified_cell_history_layout(&self, context: &str) -> anyhow::Result<()> {
        let has_state = self.has_cf(CF_CELL_STATE);
        let has_payloads = self.has_cf(CF_CELL_PAYLOADS);
        let has_index = self.has_cf(CF_CELL_INDEX);
        if has_state && has_payloads && has_index {
            return Ok(());
        }
        anyhow::bail!(
            "{context} requires explicit history_store when store lacks unified cell history layout: has_cell_state={}, has_cell_payloads={}, has_cell_index={}",
            has_state,
            has_payloads,
            has_index
        );
    }

    fn ensure_code_hash_backfill_layout(
        &self,
        state_store: &CkbadgerStore,
        context: &str,
    ) -> anyhow::Result<()> {
        let history_has_payloads = self.has_cf(CF_CELL_PAYLOADS);
        let history_has_index = self.has_cf(CF_CELL_INDEX);
        let state_has_cell_state = state_store.has_cf(CF_CELL_STATE);
        if history_has_payloads && history_has_index && state_has_cell_state {
            return Ok(());
        }
        anyhow::bail!(
            "{context} requires history_store with cell_payloads+cell_index and state_store with cell_state: history_has_cell_payloads={}, history_has_cell_index={}, state_has_cell_state={}",
            history_has_payloads,
            history_has_index,
            state_has_cell_state
        );
    }

    fn get_cells_batch_internal(
        &self,
        payload_store: &CkbadgerStore,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let state_cf = self.cf_cell_state();
        let payload_cf = payload_store.cf_cell_payloads();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let state_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (state_cf, k.as_slice())).collect();
        let state_values = self.multi_get_cf(state_cf_keys);

        let mut present_indices = Vec::new();
        let mut payload_keys: Vec<Vec<u8>> = Vec::new();
        for (i, state_result) in state_values.into_iter().enumerate() {
            match state_result {
                Ok(Some(value)) => {
                    let state = decode_cell_state(&keys[i], &value, "get_cells_batch")?;
                    if state.is_live() {
                        present_indices.push(i);
                        payload_keys.push(state.payload_key);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading cell_state in get_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let payload_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = payload_keys
            .iter()
            .map(|k| (payload_cf, k.as_slice()))
            .collect();
        let payload_values = payload_store.multi_get_cf(payload_cf_keys);
        for (batch_idx, value_result) in payload_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            let payload_key = &payload_keys[batch_idx];
            match value_result {
                Ok(Some(value)) => {
                    let info = decode_cell_payload(payload_key, &value, "get_cells_batch")?;
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "missing canonical cell payload for live state in get_cells_batch: outpoint=0x{}, payload_key=0x{}",
                        bytes_to_hex(outpoint_key),
                        bytes_to_hex(payload_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading canonical cell payload in get_cells_batch: outpoint=0x{}, payload_key=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        bytes_to_hex(payload_key),
                        e
                    ));
                }
            }
        }
        Ok(result)
    }

    fn get_consumed_cells_batch_internal(
        &self,
        payload_store: &CkbadgerStore,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let state_cf = self.cf_cell_state();
        let payload_cf = payload_store.cf_cell_payloads();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let state_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (state_cf, k.as_slice())).collect();
        let state_values = self.multi_get_cf(state_cf_keys);

        let mut present_indices = Vec::new();
        let mut payload_keys: Vec<Vec<u8>> = Vec::new();
        for (i, value_result) in state_values.into_iter().enumerate() {
            match value_result {
                Ok(Some(value)) => {
                    let state = decode_cell_state(&keys[i], &value, "get_consumed_cells_batch")?;
                    if state.is_consumed() {
                        present_indices.push(i);
                        payload_keys.push(state.payload_key);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading cell_state in get_consumed_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let payload_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = payload_keys
            .iter()
            .map(|k| (payload_cf, k.as_slice()))
            .collect();
        let payload_values = payload_store.multi_get_cf(payload_cf_keys);
        for (batch_idx, value_result) in payload_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            let payload_key = &payload_keys[batch_idx];
            match value_result {
                Ok(Some(value)) => {
                    let info =
                        decode_cell_payload(payload_key, &value, "get_consumed_cells_batch")?;
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "missing canonical cell payload for consumed state in get_consumed_cells_batch: outpoint=0x{}, payload_key=0x{}",
                        bytes_to_hex(outpoint_key),
                        bytes_to_hex(payload_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading canonical cell payload in get_consumed_cells_batch: outpoint=0x{}, payload_key=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        bytes_to_hex(payload_key),
                        e
                    ));
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn get_cell_state_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<CellState>> {
        match self.get_cf(self.cf_cell_state(), outpoint_key)? {
            Some(value) => Ok(Some(decode_cell_state(
                outpoint_key,
                &value,
                "get_cell_state_by_outpoint_key",
            )?)),
            None => Ok(None),
        }
    }

    pub(crate) fn get_cell_payload_by_key(
        &self,
        payload_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        match self.get_cf(self.cf_cell_payloads(), payload_key)? {
            Some(value) => Ok(Some(decode_cell_payload(
                payload_key,
                &value,
                "get_cell_payload_by_key",
            )?)),
            None => Ok(None),
        }
    }

    pub(crate) fn get_cell_payload_for_state_with_store(
        &self,
        payload_store: &CkbadgerStore,
        outpoint_key: &[u8],
        state: &CellState,
        context: &str,
    ) -> anyhow::Result<LiveCellInfo> {
        payload_store
            .get_cell_payload_by_key(&state.payload_key)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "missing canonical cell payload in {context}: outpoint=0x{}, payload_key=0x{}",
                    bytes_to_hex(outpoint_key),
                    bytes_to_hex(&state.payload_key)
                )
            })
    }

    pub fn get_cell_by_outpoint_key_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let Some(state) = self.get_cell_state_by_outpoint_key(outpoint_key)? else {
            return Ok(None);
        };
        Ok(Some(self.get_cell_payload_for_state_with_store(
            payload_store,
            outpoint_key,
            &state,
            "get_cell_by_outpoint_key_with_payload_store",
        )?))
    }

    pub fn get_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        self.ensure_unified_cell_payload_layout("get_cell_by_outpoint_key")?;
        self.get_cell_by_outpoint_key_with_payload_store(self, outpoint_key)
    }

    pub fn get_live_cell_by_outpoint_key_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let Some(state) = self.get_cell_state_by_outpoint_key(outpoint_key)? else {
            return Ok(None);
        };
        if !state.is_live() {
            return Ok(None);
        }
        Ok(Some(self.get_cell_payload_for_state_with_store(
            payload_store,
            outpoint_key,
            &state,
            "get_live_cell_by_outpoint_key_with_payload_store",
        )?))
    }

    pub fn get_live_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        self.ensure_unified_cell_payload_layout("get_live_cell_by_outpoint_key")?;
        self.get_live_cell_by_outpoint_key_with_payload_store(self, outpoint_key)
    }

    pub fn get_cell_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.get_live_cell_by_outpoint_key_with_payload_store(payload_store, &key)
    }

    pub fn get_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        self.ensure_unified_cell_payload_layout("get_cell")?;
        self.get_cell_with_payload_store(self, tx_hash, output_index)
    }

    pub fn get_consumed_cell_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        Ok(self
            .get_consumed_cell_info_with_payload_store(payload_store, tx_hash, output_index)?
            .map(|c| c.to_live_cell_info()))
    }

    pub fn get_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        self.ensure_unified_cell_payload_layout("get_cells_batch")?;
        self.get_cells_batch_internal(self, outpoints)
    }

    pub fn get_cells_batch_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        self.get_cells_batch_internal(payload_store, outpoints)
    }

    pub fn get_consumed_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        self.ensure_unified_cell_payload_layout("get_consumed_cell")?;
        self.get_consumed_cell_with_payload_store(self, tx_hash, output_index)
    }

    pub fn get_consumed_cell_info_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<ConsumedCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        let Some(state) = self.get_cell_state_by_outpoint_key(&key)? else {
            return Ok(None);
        };
        let CellStateKind::Consumed {
            consumed_at_block,
            consumed_by_tx,
        } = &state.state
        else {
            return Ok(None);
        };
        let cell = self.get_cell_payload_for_state_with_store(
            payload_store,
            &key,
            &state,
            "get_consumed_cell_info_with_payload_store",
        )?;
        Ok(Some(ConsumedCellInfo {
            cell,
            consumed_at_block: *consumed_at_block,
            consumed_by_tx: (!consumed_by_tx.is_empty()).then(|| consumed_by_tx.clone()),
        }))
    }

    pub fn get_consumed_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<ConsumedCellInfo>> {
        self.ensure_unified_cell_payload_layout("get_consumed_cell_info")?;
        self.get_consumed_cell_info_with_payload_store(self, tx_hash, output_index)
    }

    pub fn get_consumed_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }
        self.ensure_unified_cell_payload_layout("get_consumed_cells_batch")?;
        self.get_consumed_cells_batch_internal(self, outpoints)
    }

    pub fn get_consumed_cells_batch_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }
        self.get_consumed_cells_batch_internal(payload_store, outpoints)
    }

    pub fn get_consumed_cell_meta_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), ConsumedCellMeta>> {
        self.ensure_unified_cell_payload_layout("get_consumed_cell_meta_batch")?;
        self.get_consumed_cell_meta_batch_with_payload_store(self, outpoints)
    }

    pub fn get_consumed_cell_meta_batch_with_payload_store(
        &self,
        payload_store: &CkbadgerStore,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), ConsumedCellMeta>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());
        let state_cf = self.cf_cell_state();
        let payload_cf = payload_store.cf_cell_payloads();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let state_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (state_cf, k.as_slice())).collect();
        let state_values = self.multi_get_cf(state_cf_keys);

        let mut present_indices = Vec::new();
        let mut payload_keys: Vec<Vec<u8>> = Vec::new();
        let mut metas: Vec<Option<ConsumedCellMeta>> = vec![None; keys.len()];
        for (i, value_result) in state_values.into_iter().enumerate() {
            match value_result {
                Ok(Some(value)) => {
                    let state =
                        decode_cell_state(&keys[i], &value, "get_consumed_cell_meta_batch")?;
                    if let CellStateKind::Consumed {
                        consumed_at_block,
                        consumed_by_tx,
                    } = state.state
                    {
                        metas[i] = Some(ConsumedCellMeta {
                            consumed_at_block,
                            consumed_by_tx: (!consumed_by_tx.is_empty()).then_some(consumed_by_tx),
                        });
                        present_indices.push(i);
                        payload_keys.push(state.payload_key);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading cell_state in get_consumed_cell_meta_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let payload_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = payload_keys
            .iter()
            .map(|k| (payload_cf, k.as_slice()))
            .collect();
        let payload_values = payload_store.multi_get_cf(payload_cf_keys);
        for (batch_idx, value_result) in payload_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            let payload_key = &payload_keys[batch_idx];
            match value_result {
                Ok(Some(_)) => {
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    let meta = metas[outpoint_idx].clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "missing decoded consumed cell meta in get_consumed_cell_meta_batch: outpoint=0x{}",
                            bytes_to_hex(outpoint_key)
                        )
                    })?;
                    result.insert((tx_hash.to_vec(), idx), meta);
                }
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "missing canonical cell payload for consumed state in get_consumed_cell_meta_batch: outpoint=0x{}, payload_key=0x{}",
                        bytes_to_hex(outpoint_key),
                        bytes_to_hex(payload_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while checking canonical cell payload in get_consumed_cell_meta_batch: outpoint=0x{}, payload_key=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        bytes_to_hex(payload_key),
                        e
                    ));
                }
            }
        }

        Ok(result)
    }

    fn live_cell_for_index_key(
        &self,
        history_store: &CkbadgerStore,
        key: &[u8],
        expected_tag: CellIndexTag,
        context: &str,
    ) -> anyhow::Result<Option<(Vec<u8>, i16, LiveCellInfo)>> {
        let decoded = keys::decode_cell_index_entry_key(key).ok_or_else(|| {
            anyhow::anyhow!(
                "failed to decode unified cell index key in {context}: key=0x{}",
                bytes_to_hex(key)
            )
        })?;
        if decoded.tag != expected_tag {
            anyhow::bail!(
                "unexpected unified cell index tag in {context}: key=0x{}, expected_tag={}, actual_tag={}",
                bytes_to_hex(key),
                expected_tag.as_byte(),
                decoded.tag.as_byte()
            );
        }

        let outpoint_key = keys::encode_outpoint(&decoded.tx_hash, decoded.output_index);
        let Some(state) = self.get_cell_state_by_outpoint_key(&outpoint_key)? else {
            return Ok(None);
        };
        if !state.is_live() || state.created_at_block != decoded.block_number {
            return Ok(None);
        }

        let cell = self.get_cell_payload_for_state_with_store(
            history_store,
            &outpoint_key,
            &state,
            context,
        )?;
        Ok(Some((decoded.tx_hash, decoded.output_index, cell)))
    }

    /// List live cells by lock script hash (prefix scan).
    /// `after_key` is the full 75-byte unified cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_lock(
        &self,
        lock_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.ensure_unified_cell_history_layout("list_cells_by_lock")?;
        self.list_cells_by_lock_with_history_store(self, lock_hash, limit, after_key)
    }

    pub fn list_cells_by_lock_with_history_store(
        &self,
        history_store: &CkbadgerStore,
        lock_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_tag_with_history_store(
            history_store,
            CellIndexTag::Lock,
            lock_hash,
            limit,
            after_key,
        )
    }

    /// List live cells by type script hash (prefix scan).
    /// `after_key` is the full 75-byte unified cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_type(
        &self,
        type_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.ensure_unified_cell_history_layout("list_cells_by_type")?;
        self.list_cells_by_type_with_history_store(self, type_hash, limit, after_key)
    }

    pub fn list_cells_by_type_with_history_store(
        &self,
        history_store: &CkbadgerStore,
        type_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_tag_with_history_store(
            history_store,
            CellIndexTag::Type,
            type_hash,
            limit,
            after_key,
        )
    }

    fn list_cells_by_tag_with_history_store(
        &self,
        history_store: &CkbadgerStore,
        tag: CellIndexTag,
        hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        let mut results = Vec::new();
        let prefix = keys::encode_cell_index_prefix(tag, hash);

        let start_key = after_key
            .map(|k| k.to_vec())
            .unwrap_or_else(|| prefix.to_vec());

        let iter = history_store.iterator_cf(
            history_store.cf_cell_index(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut first = after_key.is_some();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate unified cell index in list_cells_by_tag: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            // Skip the cursor key itself (already returned on the previous page)
            if first {
                first = false;
                if after_key.is_some_and(|ak| key.as_ref() == ak) {
                    continue;
                }
            }
            if let Some(cell_row) = self.live_cell_for_index_key(
                history_store,
                &key,
                tag,
                "list_cells_by_tag_with_history_store",
            )? {
                results.push(cell_row);
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List live cells by lock code hash (prefix scan on unified cell_index).
    /// `after_key` is the full 75-byte unified cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_lock_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.ensure_unified_cell_history_layout("list_cells_by_lock_code_hash")?;
        self.list_cells_by_lock_code_hash_with_history_store(self, code_hash, limit, after_key)
    }

    pub fn list_cells_by_lock_code_hash_with_history_store(
        &self,
        history_store: &CkbadgerStore,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_tag_with_history_store(
            history_store,
            CellIndexTag::LockCode,
            code_hash,
            limit,
            after_key,
        )
    }

    /// List live cells by type code hash (prefix scan on unified cell_index).
    /// `after_key` is the full 75-byte unified cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_type_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.ensure_unified_cell_history_layout("list_cells_by_type_code_hash")?;
        self.list_cells_by_type_code_hash_with_history_store(self, code_hash, limit, after_key)
    }

    pub fn list_cells_by_type_code_hash_with_history_store(
        &self,
        history_store: &CkbadgerStore,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_tag_with_history_store(
            history_store,
            CellIndexTag::TypeCode,
            code_hash,
            limit,
            after_key,
        )
    }

    pub fn live_cells_count(&self) -> usize {
        let mut count = 0;
        let iter = self.iterator_cf(self.cf_cell_state(), rocksdb::IteratorMode::Start);
        for item in iter {
            match item {
                Ok((key, value)) => {
                    let state = decode_cell_state(&key, &value, "live_cells_count")
                        .unwrap_or_else(|e| panic!("{e}"));
                    if state.is_live() {
                        count += 1;
                    }
                }
                Err(e) => panic!("failed to iterate cell_state in live_cells_count: {}", e),
            }
        }
        count
    }

    /// Backfill lock/type-code entries into the history store's cell_index from canonical live
    /// state stored in `state_store`. Returns the number of live cells scanned/written.
    pub fn backfill_code_hash_indexes_from_state_store(
        &self,
        state_store: &CkbadgerStore,
    ) -> anyhow::Result<u64> {
        self.ensure_code_hash_backfill_layout(
            state_store,
            "backfill_code_hash_indexes_from_state_store",
        )?;
        let mut count = 0u64;
        let mut batch = StoreBatch::new(self);
        let batch_size = 10_000;

        let iter =
            state_store.iterator_cf(state_store.cf_cell_state(), rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell_state in backfill_code_hash_indexes_from_state_store: {}",
                    e
                )
            })?;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                let state =
                    decode_cell_state(&key, &value, "backfill_code_hash_indexes_from_state_store")?;
                if !state.is_live() {
                    continue;
                }
                let info = state_store.get_cell_payload_for_state_with_store(
                    self,
                    &key,
                    &state,
                    "backfill_code_hash_indexes_from_state_store",
                )?;
                let (tx_hash, output_index) = keys::decode_outpoint(&key);

                batch.put_cell_by_lock_code(
                    &info.lock_code_hash,
                    info.created_at_block,
                    &tx_hash,
                    output_index,
                );

                if let Some(ref type_code_hash) = info.type_code_hash {
                    batch.put_cell_by_type_code(
                        type_code_hash,
                        info.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                }

                count += 1;
                #[allow(clippy::manual_is_multiple_of)]
                if count % batch_size as u64 == 0 {
                    batch.commit()?;
                    batch = StoreBatch::new(self);
                }
            }
        }

        if !batch.is_empty() {
            batch.commit()?;
        }

        Ok(count)
    }

    /// Backfill lock/type-code entries into the unified cell_index from canonical live state.
    /// Returns the number of live cells scanned/written.
    pub fn backfill_code_hash_indexes(&self) -> anyhow::Result<u64> {
        self.backfill_code_hash_indexes_from_state_store(self)
    }

    /// Check if the code_hash indexes have been populated.
    pub fn code_hash_indexes_populated(&self) -> bool {
        let tag = [CellIndexTag::LockCode.as_byte()];
        let mut iter = self.iterator_cf(
            self.cf_cell_index(),
            rocksdb::IteratorMode::From(&tag, rocksdb::Direction::Forward),
        );
        match iter.next() {
            Some(Ok((key, _))) => key.first() == Some(&CellIndexTag::LockCode.as_byte()),
            Some(Err(e)) => panic!(
                "failed to iterate unified cell_index in code_hash_indexes_populated: {}",
                e
            ),
            None => false,
        }
    }

    /// Aggregate cell stats for a token (by type script hash).
    /// Prefix-scans unified cell_index and loads only canonical live cells.
    pub fn aggregate_token_cell_stats(&self, type_hash: &[u8]) -> anyhow::Result<TokenCellStats> {
        self.ensure_unified_cell_history_layout("aggregate_token_cell_stats")?;
        self.aggregate_token_cell_stats_with_history_store(self, type_hash)
    }

    pub fn aggregate_token_cell_stats_with_history_store(
        &self,
        history_store: &CkbadgerStore,
        type_hash: &[u8],
    ) -> anyhow::Result<TokenCellStats> {
        let mut stats = TokenCellStats {
            cells_count: 0,
            total_capacity: 0,
            total_occupied_capacity: 0,
        };

        let prefix = keys::encode_cell_index_prefix(CellIndexTag::Type, type_hash);
        let iter = history_store.iterator_cf(
            history_store.cf_cell_index(),
            rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate unified cell_index in aggregate_token_cell_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Some((_, _, cell)) = self.live_cell_for_index_key(
                history_store,
                &key,
                CellIndexTag::Type,
                "aggregate_token_cell_stats_with_history_store",
            )? {
                stats.cells_count += 1;
                stats.total_capacity += cell.capacity as i128;
                stats.total_occupied_capacity += cell.occupied_capacity as i128;
            }
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::CkbadgerStore;
    use crate::types::{CellIndexTag, CellState};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    fn make_cell(capacity: i64, occupied: i64, type_hash: &[u8]) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            created_at_block: 100,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0xBB; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.to_vec()),
            type_code_hash: Some(vec![0xCC; 32]),
            type_args: Some(vec![]),
            data_size: 0,
            occupied_capacity: occupied,
            udt_amount: None,
        }
    }

    fn insert_cell(
        store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
        type_hash: &[u8],
        cell: &LiveCellInfo,
    ) {
        insert_live_cell_new_schema(
            store,
            tx_hash,
            output_index,
            &cell.lock_script_hash,
            type_hash,
            cell,
        );
    }

    #[test]
    fn test_aggregate_token_cell_stats_empty() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let stats = store.aggregate_token_cell_stats(&type_hash).unwrap();
        assert_eq!(stats.cells_count, 0);
        assert_eq!(stats.total_capacity, 0);
        assert_eq!(stats.total_occupied_capacity, 0);
    }

    #[test]
    fn test_aggregate_token_cell_stats_single_cell() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let tx_hash = [0x11u8; 32];
        let cell = make_cell(200_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx_hash, 0, &type_hash, &cell);

        let stats = store.aggregate_token_cell_stats(&type_hash).unwrap();
        assert_eq!(stats.cells_count, 1);
        assert_eq!(stats.total_capacity, 200_00000000);
        assert_eq!(stats.total_occupied_capacity, 61_00000000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_multiple_cells() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];

        let tx1 = [0x11u8; 32];
        let cell1 = make_cell(200_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx1, 0, &type_hash, &cell1);

        let tx2 = [0x22u8; 32];
        let cell2 = make_cell(300_00000000, 80_00000000, &type_hash);
        insert_cell(&store, &tx2, 0, &type_hash, &cell2);

        let tx3 = [0x33u8; 32];
        let cell3 = make_cell(150_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx3, 1, &type_hash, &cell3);

        let stats = store.aggregate_token_cell_stats(&type_hash).unwrap();
        assert_eq!(stats.cells_count, 3);
        assert_eq!(stats.total_capacity, 650_00000000);
        assert_eq!(stats.total_occupied_capacity, 202_00000000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_different_types_isolated() {
        let (_dir, store) = test_store();
        let type_a = [0x01u8; 32];
        let type_b = [0x02u8; 32];

        let tx1 = [0x11u8; 32];
        let cell1 = make_cell(200_00000000, 61_00000000, &type_a);
        insert_cell(&store, &tx1, 0, &type_a, &cell1);

        let tx2 = [0x22u8; 32];
        let cell2 = make_cell(500_00000000, 100_00000000, &type_b);
        insert_cell(&store, &tx2, 0, &type_b, &cell2);

        let stats_a = store.aggregate_token_cell_stats(&type_a).unwrap();
        assert_eq!(stats_a.cells_count, 1);
        assert_eq!(stats_a.total_capacity, 200_00000000);

        let stats_b = store.aggregate_token_cell_stats(&type_b).unwrap();
        assert_eq!(stats_b.cells_count, 1);
        assert_eq!(stats_b.total_capacity, 500_00000000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_with_history_store_reads_split_layout() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let type_hash = [0x05u8; 32];
        let tx_hash = [0x77u8; 32];
        let cell = make_cell(250_00000000, 72_00000000, &type_hash);

        let mut domain_batch = crate::batch::StoreBatch::new(&domain);
        domain_batch.put_cell(&tx_hash, 0, &cell);
        domain_batch.commit().unwrap();

        let mut append_batch = crate::batch::StoreBatch::new(&append);
        append_batch.put_cell(&tx_hash, 0, &cell);
        append_batch.put_cell_by_type(&type_hash, cell.created_at_block, &tx_hash, 0);
        append_batch.commit().unwrap();

        let stats = domain
            .aggregate_token_cell_stats_with_history_store(&append, &type_hash)
            .unwrap();
        assert_eq!(stats.cells_count, 1);
        assert_eq!(stats.total_capacity, 250_00000000);
        assert_eq!(stats.total_occupied_capacity, 72_00000000);
    }

    #[test]
    fn test_get_consumed_cell_info_returns_consumer_metadata() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let consumed_cell = make_cell(200_00000000, 61_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];
        let consumed_by_tx = [0x22u8; 32];
        insert_consumed_cell_new_schema(
            &store,
            &tx_hash,
            0,
            (&consumed_cell.lock_script_hash, &type_hash),
            &consumed_cell,
            12345,
            &consumed_by_tx,
        );

        let info = store.get_consumed_cell_info(&tx_hash, 0).unwrap().unwrap();
        assert_eq!(info.consumed_at_block, 12345);
        assert_eq!(info.consumed_by_tx, Some(consumed_by_tx.to_vec()));
        assert_eq!(info.cell.capacity, consumed_cell.capacity);
    }

    #[test]
    fn test_get_cells_batch_fails_when_live_marker_has_no_canonical_cell() {
        let (_dir, store) = test_store();
        let tx_hash = [0xAB; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(&CellState::live(100, b"missing-payload".to_vec())).unwrap(),
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_cells_batch(&refs).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing canonical cell payload for live state in get_cells_batch"));
    }

    #[test]
    fn test_get_consumed_cells_batch_fails_when_consumed_state_has_no_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xCD; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(
                    &CellState::live(100, b"missing-payload".to_vec())
                        .into_consumed(200, vec![0x22; 32]),
                )
                .unwrap(),
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_consumed_cells_batch(&refs).unwrap_err();
        assert!(err.to_string().contains(
            "missing canonical cell payload for consumed state in get_consumed_cells_batch"
        ));
    }

    #[test]
    fn test_get_consumed_cells_batch_fails_when_consumed_state_payload_missing() {
        let (_dir, store) = test_store();
        let tx_hash = [0xDE; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(
                    &CellState::live(100, b"missing-payload-2".to_vec())
                        .into_consumed(300, vec![0x33; 32]),
                )
                .unwrap(),
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_consumed_cells_batch(&refs).unwrap_err();
        assert!(err.to_string().contains(
            "missing canonical cell payload for consumed state in get_consumed_cells_batch"
        ));
    }

    #[test]
    fn test_get_consumed_cell_meta_batch_returns_consumer_metadata() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let consumed_cell = make_cell(200_00000000, 61_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];
        let consumed_by_tx = [0x22u8; 32];

        insert_consumed_cell_new_schema(
            &store,
            &tx_hash,
            0,
            (&consumed_cell.lock_script_hash, &type_hash),
            &consumed_cell,
            12345,
            &consumed_by_tx,
        );

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let metas = store.get_consumed_cell_meta_batch(&refs).unwrap();
        let meta = metas.get(&(tx_hash.to_vec(), 0)).unwrap();
        assert_eq!(meta.consumed_at_block, 12345);
        assert_eq!(meta.consumed_by_tx, Some(consumed_by_tx.to_vec()));
    }

    #[test]
    fn test_get_consumed_cell_meta_batch_fails_when_consumed_state_has_no_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xCE; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(
                    &CellState::live(100, b"missing-meta-payload".to_vec())
                        .into_consumed(400, vec![0x44; 32]),
                )
                .unwrap(),
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_consumed_cell_meta_batch(&refs).unwrap_err();
        assert!(err.to_string().contains(
            "missing canonical cell payload for consumed state in get_consumed_cell_meta_batch"
        ));
    }

    #[test]
    fn test_get_consumed_cell_meta_batch_with_payload_store_reads_split_domain_append_layout() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let tx_hash = [0x58; 32];
        let type_hash = [0x68; 32];
        let consumed_by_tx = [0x78; 32];
        let cell = make_cell(200_00000000, 61_00000000, &type_hash);

        let mut domain_batch = crate::batch::StoreBatch::new(&domain);
        domain_batch.put_consumed_cell_with_consumer(
            &tx_hash,
            3,
            &cell,
            456,
            Some(&consumed_by_tx),
        );
        domain_batch.commit().unwrap();

        let mut append_batch = crate::batch::StoreBatch::new(&append);
        append_batch.put_cell(&tx_hash, 3, &cell);
        append_batch.commit().unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 3)];
        let metas = domain
            .get_consumed_cell_meta_batch_with_payload_store(&append, &refs)
            .unwrap();
        let meta = metas.get(&(tx_hash.to_vec(), 3)).unwrap();
        assert_eq!(meta.consumed_at_block, 456);
        assert_eq!(meta.consumed_by_tx, Some(consumed_by_tx.to_vec()));
    }

    #[test]
    fn test_get_consumed_cell_meta_batch_requires_explicit_payload_store_for_split_domain() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let tx_hash = [0x88; 32];
        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];

        let err = domain.get_consumed_cell_meta_batch(&refs).unwrap_err();
        assert!(err.to_string().contains("requires explicit payload_store"));
    }

    #[test]
    fn test_get_consumed_cell_info_fails_when_consumed_state_has_no_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xEF; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(
                    &CellState::live(100, b"missing-consumed-info".to_vec())
                        .into_consumed(500, vec![0x55; 32]),
                )
                .unwrap(),
            )
            .unwrap();

        let err = store.get_consumed_cell_info(&tx_hash, 0).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing canonical cell payload in get_consumed_cell_info"));
    }

    fn insert_live_cell_new_schema(
        store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
        lock_hash: &[u8],
        type_hash: &[u8],
        cell: &LiveCellInfo,
    ) {
        let payload_key =
            keys::encode_cell_payload_key(cell.created_at_block, tx_hash, output_index);
        let outpoint_key = keys::encode_outpoint(tx_hash, output_index);
        store
            .put_cf(
                store.cf_cell_payloads(),
                &payload_key,
                &bincode::serialize(cell).unwrap(),
            )
            .unwrap();
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(&CellState::live(
                    cell.created_at_block,
                    payload_key.to_vec(),
                ))
                .unwrap(),
            )
            .unwrap();
        store
            .put_cf(
                store.cf_cell_index(),
                &keys::encode_cell_index_entry_key(
                    CellIndexTag::Lock,
                    lock_hash,
                    cell.created_at_block,
                    tx_hash,
                    output_index,
                ),
                &[],
            )
            .unwrap();
        store
            .put_cf(
                store.cf_cell_index(),
                &keys::encode_cell_index_entry_key(
                    CellIndexTag::Type,
                    type_hash,
                    cell.created_at_block,
                    tx_hash,
                    output_index,
                ),
                &[],
            )
            .unwrap();
    }

    fn insert_consumed_cell_new_schema(
        store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
        script_hashes: (&[u8], &[u8]),
        cell: &LiveCellInfo,
        consumed_at_block: i64,
        consumed_by_tx: &[u8],
    ) {
        let (lock_hash, type_hash) = script_hashes;
        insert_live_cell_new_schema(store, tx_hash, output_index, lock_hash, type_hash, cell);
        let outpoint_key = keys::encode_outpoint(tx_hash, output_index);
        let payload_key =
            keys::encode_cell_payload_key(cell.created_at_block, tx_hash, output_index);
        store
            .put_cf(
                store.cf_cell_state(),
                &outpoint_key,
                &bincode::serialize(
                    &CellState::live(cell.created_at_block, payload_key.to_vec())
                        .into_consumed(consumed_at_block, consumed_by_tx.to_vec()),
                )
                .unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn test_get_cell_reads_live_state_then_payload() {
        let (_dir, store) = test_store();
        let lock_hash = [0xAA; 32];
        let type_hash = [0x01; 32];
        let tx_hash = [0x11; 32];
        let cell = make_cell(200_00000000, 61_00000000, &type_hash);
        insert_live_cell_new_schema(&store, &tx_hash, 0, &lock_hash, &type_hash, &cell);

        let loaded = store.get_cell(&tx_hash, 0).unwrap().unwrap();
        assert_eq!(loaded.created_at_block, 100);
        assert_eq!(loaded.capacity, cell.capacity);
    }

    #[test]
    fn test_get_cell_with_payload_store_reads_split_domain_append_layout() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let tx_hash = [0x19; 32];
        let type_hash = [0x02; 32];
        let cell = make_cell(210_00000000, 61_00000000, &type_hash);

        let mut domain_batch = crate::batch::StoreBatch::new(&domain);
        domain_batch.put_cell(&tx_hash, 0, &cell);
        domain_batch.commit().unwrap();

        let mut append_batch = crate::batch::StoreBatch::new(&append);
        append_batch.put_cell(&tx_hash, 0, &cell);
        append_batch.put_cell_by_lock(&cell.lock_script_hash, cell.created_at_block, &tx_hash, 0);
        append_batch.put_cell_by_type(&type_hash, cell.created_at_block, &tx_hash, 0);
        append_batch.commit().unwrap();

        let loaded = domain
            .get_cell_with_payload_store(&append, &tx_hash, 0)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.created_at_block, cell.created_at_block);
        assert_eq!(loaded.capacity, cell.capacity);
    }

    #[test]
    fn test_get_consumed_cell_info_reads_state_then_payload() {
        let (_dir, store) = test_store();
        let lock_hash = [0xAA; 32];
        let type_hash = [0x01; 32];
        let tx_hash = [0x22; 32];
        let consumed_by_tx = [0x44; 32];
        let cell = make_cell(300_00000000, 80_00000000, &type_hash);
        insert_consumed_cell_new_schema(
            &store,
            &tx_hash,
            1,
            (&lock_hash, &type_hash),
            &cell,
            200,
            &consumed_by_tx,
        );

        let info = store.get_consumed_cell_info(&tx_hash, 1).unwrap().unwrap();
        assert_eq!(info.consumed_at_block, 200);
        assert_eq!(info.consumed_by_tx, Some(consumed_by_tx.to_vec()));
        assert_eq!(info.cell.capacity, cell.capacity);
    }

    #[test]
    fn test_get_consumed_cell_info_with_payload_store_reads_split_domain_append_layout() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let tx_hash = [0x29; 32];
        let type_hash = [0x03; 32];
        let consumed_by_tx = [0x39; 32];
        let cell = make_cell(310_00000000, 72_00000000, &type_hash);

        let mut domain_batch = crate::batch::StoreBatch::new(&domain);
        domain_batch.put_consumed_cell_with_consumer(
            &tx_hash,
            1,
            &cell,
            345,
            Some(&consumed_by_tx),
        );
        domain_batch.commit().unwrap();

        let mut append_batch = crate::batch::StoreBatch::new(&append);
        append_batch.put_cell(&tx_hash, 1, &cell);
        append_batch.commit().unwrap();

        let info = domain
            .get_consumed_cell_info_with_payload_store(&append, &tx_hash, 1)
            .unwrap()
            .unwrap();
        assert_eq!(info.consumed_at_block, 345);
        assert_eq!(info.consumed_by_tx, Some(consumed_by_tx.to_vec()));
        assert_eq!(info.cell.capacity, cell.capacity);
    }

    #[test]
    fn test_list_cells_by_lock_skips_stale_historical_index_entries() {
        let (_dir, store) = test_store();
        let lock_hash = [0xAA; 32];
        let type_hash = [0x01; 32];
        let tx_hash = [0x33; 32];
        let cell = make_cell(150_00000000, 61_00000000, &type_hash);
        insert_live_cell_new_schema(&store, &tx_hash, 0, &lock_hash, &type_hash, &cell);

        store
            .put_cf(
                store.cf_cell_index(),
                &keys::encode_cell_index_entry_key(CellIndexTag::Lock, &lock_hash, 50, &tx_hash, 0),
                &[],
            )
            .unwrap();

        let rows = store.list_cells_by_lock(&lock_hash, 10, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, 0);
    }

    #[test]
    fn test_list_cells_by_lock_with_history_store_reads_split_domain_append_layout() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let tx_hash = [0x49; 32];
        let type_hash = [0x04; 32];
        let cell = make_cell(410_00000000, 81_00000000, &type_hash);

        let mut domain_batch = crate::batch::StoreBatch::new(&domain);
        domain_batch.put_cell(&tx_hash, 0, &cell);
        domain_batch.commit().unwrap();

        let mut append_batch = crate::batch::StoreBatch::new(&append);
        append_batch.put_cell(&tx_hash, 0, &cell);
        append_batch.put_cell_by_lock(&cell.lock_script_hash, cell.created_at_block, &tx_hash, 0);
        append_batch.put_cell_by_type(&type_hash, cell.created_at_block, &tx_hash, 0);
        append_batch.commit().unwrap();

        let rows = domain
            .list_cells_by_lock_with_history_store(&append, &cell.lock_script_hash, 10, None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, tx_hash.to_vec());
        assert_eq!(rows[0].1, 0);
        assert_eq!(rows[0].2.capacity, cell.capacity);
    }

    #[test]
    fn test_list_cells_by_lock_requires_explicit_history_store_for_split_domain() {
        let root = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let lock_hash = [0x99; 32];

        let err = domain.list_cells_by_lock(&lock_hash, 10, None).unwrap_err();
        assert!(err.to_string().contains("requires explicit history_store"));
    }
}
