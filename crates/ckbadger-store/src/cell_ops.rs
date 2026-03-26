//! Cell read/write operations.

use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    decode_consumed_cell_meta, decode_live_cell_marker, ConsumedCellInfo, ConsumedCellMeta,
    LiveCellInfo, PositionedCellInfo,
};

use crate::bytes_to_hex;
use crate::types::{LockScriptEntry, TokenCellStats};

impl CkbadgerStore {
    /// Look up lock script components by lock_hash.
    pub fn get_lock_script(&self, lock_hash: &[u8]) -> anyhow::Result<Option<LockScriptEntry>> {
        match self.get_cf(self.cf_lock_scripts(), lock_hash)? {
            Some(value) => {
                let entry = postcard::from_bytes::<LockScriptEntry>(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize LockScriptEntry: lock_hash=0x{}, error={}",
                        bytes_to_hex(lock_hash),
                        e
                    )
                })?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    pub fn get_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        match self.get_cf(self.cf_cells(), outpoint_key)? {
            Some(value) => {
                let info = postcard::from_bytes::<LiveCellInfo>(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize canonical cell payload: outpoint=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        e
                    )
                })?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    pub fn get_live_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<PositionedCellInfo>> {
        let Some(marker_bytes) = self.get_cf(self.cf_live_cells(), outpoint_key)? else {
            return Ok(None);
        };
        let created_at_block = decode_live_cell_marker(&marker_bytes).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid live cell marker value: outpoint=0x{}, value_len={}",
                bytes_to_hex(outpoint_key),
                marker_bytes.len()
            )
        })?;
        let info = cells_store
            .get_cell_by_outpoint_key(outpoint_key)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "missing canonical cell for live marker: outpoint=0x{}",
                    bytes_to_hex(outpoint_key)
                )
            })?;
        Ok(Some(PositionedCellInfo::new(info, created_at_block)))
    }

    pub fn get_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<PositionedCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.get_live_cell_by_outpoint_key(&key, cells_store)
    }

    pub fn get_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), PositionedCellInfo>> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let live_cf = self.cf_live_cells();
        let cells_cf = cells_store.cf_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let live_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (live_cf, k.as_slice())).collect();
        let live_values = self.multi_get_cf(live_cf_keys);

        let mut present_indices = Vec::new();
        let mut created_at_blocks = Vec::new();
        let mut cell_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (i, marker_result) in live_values.into_iter().enumerate() {
            match marker_result {
                Ok(Some(marker_bytes)) => {
                    let created_at_block =
                        decode_live_cell_marker(&marker_bytes).ok_or_else(|| {
                            anyhow::anyhow!(
                                "invalid live cell marker value in get_cells_batch: outpoint=0x{}, value_len={}",
                                bytes_to_hex(&keys[i]),
                                marker_bytes.len()
                            )
                        })?;
                    present_indices.push(i);
                    created_at_blocks.push(created_at_block);
                    cell_cf_keys.push((cells_cf, keys[i].as_slice()));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading live marker in get_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let cell_values = cells_store.multi_get_cf(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            match value_result {
                Ok(Some(value)) => {
                    let info = postcard::from_bytes::<LiveCellInfo>(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize canonical cell payload in get_cells_batch: outpoint=0x{}, error={}",
                            bytes_to_hex(outpoint_key),
                            e
                        )
                    })?;
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    result.insert(
                        (tx_hash.to_vec(), idx),
                        PositionedCellInfo::new(info, created_at_blocks[batch_idx]),
                    );
                }
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "missing canonical cell for live marker in get_cells_batch: outpoint=0x{}",
                        bytes_to_hex(outpoint_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading canonical cell in get_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        e
                    ));
                }
            }
        }
        Ok(result)
    }

    pub fn get_consumed_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<PositionedCellInfo>> {
        Ok(self
            .get_consumed_cell_info(tx_hash, output_index, cells_store)?
            .map(|c| c.to_positioned_cell_info()))
    }

    pub fn get_consumed_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<ConsumedCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        let Some(value) = self.get_cf(self.cf_consumed_cells(), &key)? else {
            return Ok(None);
        };

        let meta = decode_consumed_cell_meta(&value).map_err(|e| {
            anyhow::anyhow!(
                "failed to decode consumed cell meta: outpoint=0x{}:{}, error={}",
                bytes_to_hex(tx_hash),
                output_index,
                e
            )
        })?;
        let cell = cells_store.get_cell_by_outpoint_key(&key)?.ok_or_else(|| {
            anyhow::anyhow!(
                "missing canonical cell for consumed outpoint: outpoint=0x{}:{}",
                bytes_to_hex(tx_hash),
                output_index
            )
        })?;
        Ok(Some(ConsumedCellInfo {
            cell,
            consumed_at_block: meta.consumed_at_block,
            consumed_by_tx: meta.consumed_by_tx,
            created_at_block: meta.created_at_block,
        }))
    }

    pub fn get_consumed_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), PositionedCellInfo>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());
        let consumed_cf = self.cf_consumed_cells();
        let cells_cf = cells_store.cf_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let consumed_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (consumed_cf, k.as_slice())).collect();
        let consumed_values = self.multi_get_cf(consumed_cf_keys);

        let mut present_indices = Vec::new();
        let mut metas: Vec<Option<ConsumedCellMeta>> = vec![None; keys.len()];
        let mut cell_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (i, value_result) in consumed_values.into_iter().enumerate() {
            match value_result {
                Ok(Some(value)) => {
                    let meta = decode_consumed_cell_meta(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to decode consumed cell meta in get_consumed_cells_batch: outpoint=0x{}, error={}",
                            bytes_to_hex(&keys[i]),
                            e
                        )
                    })?;
                    metas[i] = Some(meta);
                    present_indices.push(i);
                    cell_cf_keys.push((cells_cf, keys[i].as_slice()));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_consumed_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let cell_values = cells_store.multi_get_cf(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            match value_result {
                Ok(Some(value)) => {
                    let cell = postcard::from_bytes::<LiveCellInfo>(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize canonical cell payload in get_consumed_cells_batch: outpoint=0x{}, error={}",
                            bytes_to_hex(outpoint_key),
                            e
                        )
                    })?;
                    let created_at_block = metas[outpoint_idx]
                        .as_ref()
                        .map(|meta| meta.created_at_block)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "missing decoded consumed cell meta in get_consumed_cells_batch: outpoint=0x{}",
                                bytes_to_hex(outpoint_key)
                            )
                        })?;
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    result.insert(
                        (tx_hash.to_vec(), idx),
                        PositionedCellInfo::new(cell, created_at_block),
                    );
                }
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "missing canonical cell for consumed outpoint in get_consumed_cells_batch: outpoint=0x{}",
                        bytes_to_hex(outpoint_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading canonical cell in get_consumed_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        e
                    ));
                }
            }
        }
        Ok(result)
    }

    pub fn get_consumed_cell_meta_batch(
        &self,
        outpoints: &[(&[u8], i16)],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), ConsumedCellMeta>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(outpoints.len());
        let consumed_cf = self.cf_consumed_cells();
        let cells_cf = cells_store.cf_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let consumed_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (consumed_cf, k.as_slice())).collect();
        let consumed_values = self.multi_get_cf(consumed_cf_keys);

        let mut present_indices = Vec::new();
        let mut cell_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        let mut metas: Vec<Option<ConsumedCellMeta>> = vec![None; keys.len()];
        for (i, value_result) in consumed_values.into_iter().enumerate() {
            match value_result {
                Ok(Some(value)) => {
                    let meta = decode_consumed_cell_meta(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to decode consumed cell meta in get_consumed_cell_meta_batch: outpoint=0x{}, error={}",
                            bytes_to_hex(&keys[i]),
                            e
                        )
                    })?;
                    metas[i] = Some(meta);
                    present_indices.push(i);
                    cell_cf_keys.push((cells_cf, keys[i].as_slice()));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_consumed_cell_meta_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let cell_values = cells_store.multi_get_cf(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
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
                        "missing canonical cell for consumed outpoint in get_consumed_cell_meta_batch: outpoint=0x{}",
                        bytes_to_hex(outpoint_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while checking canonical cell in get_consumed_cell_meta_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        e
                    ));
                }
            }
        }

        Ok(result)
    }

    /// List live cells by lock script hash (prefix scan).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_lock(
        &self,
        lock_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_hash_cf(
            self.cf_cell_by_lock(),
            lock_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    /// List live cells by type script hash (prefix scan).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_type(
        &self,
        type_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_hash_cf(
            self.cf_cell_by_type(),
            type_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    fn list_cells_by_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        let mut results = Vec::new();

        let start_key = after_key
            .map(|k| k.to_vec())
            .unwrap_or_else(|| hash.to_vec());

        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut first = after_key.is_some();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell index in list_cells_by_hash_cf: {}",
                    e
                )
            })?;
            if !key.starts_with(hash) {
                break;
            }
            // Skip the cursor key itself (already returned on the previous page)
            if first {
                first = false;
                if after_key.is_some_and(|ak| key.as_ref() == ak) {
                    continue;
                }
            }
            // Key: hash(32) + block_num(8) + outpoint(34)
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                    results.push((tx_hash, output_index, cell));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// List live cells by cell data hash (prefix scan on cell_by_data_hash).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_data_hash(
        &self,
        data_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_hash_cf(
            self.cf_cell_by_data_hash(),
            data_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    /// Find a cell (live or consumed) by data hash.
    ///
    /// Unlike `list_cells_by_data_hash` which only returns live cells, this method also
    /// checks consumed cells as a fallback. Used for code cell resolution where the
    /// deployment cell may have been consumed.
    /// Prefers live cells over consumed cells.
    pub fn find_any_cell_by_data_hash(
        &self,
        data_hash: &[u8],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Option<(Vec<u8>, i16, PositionedCellInfo)>> {
        let iter = self.iterator_cf(
            self.cf_cell_by_data_hash(),
            rocksdb::IteratorMode::From(data_hash, rocksdb::Direction::Forward),
        );

        let mut consumed_fallback: Option<(Vec<u8>, i16, PositionedCellInfo)> = None;

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell index in find_any_cell_by_data_hash: {}",
                    e
                )
            })?;
            if !key.starts_with(data_hash) {
                break;
            }
            // Key: hash(32) + block_num(8) + outpoint(34)
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                // Prefer live cells
                if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                    return Ok(Some((tx_hash, output_index, cell)));
                }
                // Fall back to consumed cell (take first one found)
                if consumed_fallback.is_none() {
                    if let Some(cell) =
                        self.get_consumed_cell(&tx_hash, output_index, cells_store)?
                    {
                        consumed_fallback = Some((tx_hash, output_index, cell));
                    }
                }
            }
        }

        Ok(consumed_fallback)
    }

    /// List all cells (live and consumed) matching a data hash.
    ///
    /// Returns cells sorted by creation block (ascending, matching index key order).
    /// Each result includes a `bool` indicating whether the cell is live (`true`) or consumed (`false`).
    /// Used by the code-cells endpoint to show all deployment cells for a script.
    #[allow(clippy::type_complexity)]
    pub fn list_all_cells_by_data_hash(
        &self,
        data_hash: &[u8],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo, bool)>> {
        let mut results = Vec::new();

        let iter = self.iterator_cf(
            self.cf_cell_by_data_hash(),
            rocksdb::IteratorMode::From(data_hash, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell index in list_all_cells_by_data_hash: {}",
                    e
                )
            })?;
            if !key.starts_with(data_hash) {
                break;
            }
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                    results.push((tx_hash, output_index, cell, true));
                } else if let Some(cell) =
                    self.get_consumed_cell(&tx_hash, output_index, cells_store)?
                {
                    results.push((tx_hash, output_index, cell, false));
                }
            }
        }

        Ok(results)
    }

    /// List all cells (live and consumed) matching a type script hash.
    ///
    /// For type_id scripts this typically returns 0-1 results.
    /// Each result includes a `bool` indicating whether the cell is live (`true`) or consumed (`false`).
    #[allow(clippy::type_complexity)]
    pub fn list_all_cells_by_type(
        &self,
        type_hash: &[u8],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo, bool)>> {
        let mut results = Vec::new();

        let iter = self.iterator_cf(
            self.cf_cell_by_type(),
            rocksdb::IteratorMode::From(type_hash, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell index in list_all_cells_by_type: {}",
                    e
                )
            })?;
            if !key.starts_with(type_hash) {
                break;
            }
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                    results.push((tx_hash, output_index, cell, true));
                } else if let Some(cell) =
                    self.get_consumed_cell(&tx_hash, output_index, cells_store)?
                {
                    results.push((tx_hash, output_index, cell, false));
                }
            }
        }

        Ok(results)
    }

    /// List live cells by lock code hash (prefix scan on cell_by_lock_code).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_lock_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_lock_code(),
            code_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    /// List live cells by type code hash (prefix scan on cell_by_type_code).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_type_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_type_code(),
            code_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    fn list_cells_by_code_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        let mut results = Vec::new();

        let start_key = after_key
            .map(|k| k.to_vec())
            .unwrap_or_else(|| code_hash.to_vec());

        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut first = after_key.is_some();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate code-hash cell index in list_cells_by_code_hash_cf: {}",
                    e
                )
            })?;
            if !key.starts_with(code_hash) {
                break;
            }
            // Skip the cursor key itself (already returned on the previous page)
            if first {
                first = false;
                if after_key.is_some_and(|ak| key.as_ref() == ak) {
                    continue;
                }
            }
            // Key: code_hash(32) + block_num(8) + outpoint(34) = 74
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                    results.push((tx_hash, output_index, cell));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// Aggregate cell stats for a token (by type script hash).
    /// Prefix-scans `cell_by_type` and multi-gets each cell's capacity/used_capacity.
    pub fn aggregate_token_cell_stats(
        &self,
        type_hash: &[u8],
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<TokenCellStats> {
        let mut stats = TokenCellStats {
            cells_count: 0,
            total_capacity: 0,
            total_used_capacity: 0,
        };

        let cf = self.cf_cell_by_type();
        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(type_hash, rocksdb::Direction::Forward),
        );

        // Collect outpoints in batches for multi-get
        let batch_size = 256;
        let mut outpoints: Vec<(Vec<u8>, i16)> = Vec::with_capacity(batch_size);

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell_by_type in aggregate_token_cell_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(type_hash) {
                break;
            }
            // Key: hash(32) + block_num(8) + outpoint(34) = 74 bytes
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                outpoints.push((tx_hash, output_index));

                if outpoints.len() >= batch_size {
                    Self::accumulate_cell_stats(self, &outpoints, &mut stats, cells_store)?;
                    outpoints.clear();
                }
            }
        }

        // Flush remaining
        if !outpoints.is_empty() {
            Self::accumulate_cell_stats(self, &outpoints, &mut stats, cells_store)?;
        }

        Ok(stats)
    }

    fn accumulate_cell_stats(
        &self,
        outpoints: &[(Vec<u8>, i16)],
        stats: &mut TokenCellStats,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<()> {
        let refs: Vec<(&[u8], i16)> = outpoints.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
        let cells = self.get_cells_batch(&refs, cells_store)?;
        for cell in cells.values() {
            stats.cells_count += 1;
            stats.total_capacity += cell.capacity as i128;
            stats.total_used_capacity += cell.occupied_capacity as i128;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::store::CkbadgerStore;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    fn make_cell(capacity: i64, occupied: i64, type_hash: &[u8]) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0xBB; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.to_vec()),
            type_code_hash: Some(vec![0xCC; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            data_size: 0,
            occupied_capacity: occupied,
            udt_amount: None,
            data_hash: None,
        }
    }

    fn insert_cell(
        store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
        type_hash: &[u8],
        cell: &LiveCellInfo,
    ) {
        // Write canonical payload + live marker
        let outpoint_key = keys::encode_outpoint(tx_hash, output_index);
        let value = postcard::to_allocvec(cell).unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &value)
            .unwrap();
        store
            .put_cf(
                store.cf_live_cells(),
                &outpoint_key,
                &crate::types::encode_live_cell_marker(100),
            )
            .unwrap();

        // Write to cell_by_type index
        let idx_key = keys::encode_cell_index_key(type_hash, 100, tx_hash, output_index);
        store
            .put_cf(store.cf_cell_by_type(), &idx_key, &[])
            .unwrap();
    }

    #[test]
    fn test_aggregate_token_cell_stats_empty() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let stats = store
            .aggregate_token_cell_stats(&type_hash, &store)
            .unwrap();
        assert_eq!(stats.cells_count, 0);
        assert_eq!(stats.total_capacity, 0);
        assert_eq!(stats.total_used_capacity, 0);
    }

    #[test]
    fn test_aggregate_token_cell_stats_single_cell() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let tx_hash = [0x11u8; 32];
        let cell = make_cell(200_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx_hash, 0, &type_hash, &cell);

        let stats = store
            .aggregate_token_cell_stats(&type_hash, &store)
            .unwrap();
        assert_eq!(stats.cells_count, 1);
        assert_eq!(stats.total_capacity, 200_00000000);
        assert_eq!(stats.total_used_capacity, 61_00000000);
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

        let stats = store
            .aggregate_token_cell_stats(&type_hash, &store)
            .unwrap();
        assert_eq!(stats.cells_count, 3);
        assert_eq!(stats.total_capacity, 650_00000000);
        assert_eq!(stats.total_used_capacity, 202_00000000);
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

        let stats_a = store.aggregate_token_cell_stats(&type_a, &store).unwrap();
        assert_eq!(stats_a.cells_count, 1);
        assert_eq!(stats_a.total_capacity, 200_00000000);

        let stats_b = store.aggregate_token_cell_stats(&type_b, &store).unwrap();
        assert_eq!(stats_b.cells_count, 1);
        assert_eq!(stats_b.total_capacity, 500_00000000);
    }

    #[test]
    fn test_get_consumed_cell_info_returns_consumer_metadata() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let consumed_cell = make_cell(200_00000000, 61_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];
        let consumed_by_tx = [0x22u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &consumed_cell, 100);
        batch.put_consumed_cell_with_consumer(
            &tx_hash,
            0,
            &consumed_cell,
            100,
            12345,
            Some(&consumed_by_tx),
        );
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        let info = store
            .get_consumed_cell_info(&tx_hash, 0, &store)
            .unwrap()
            .unwrap();
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
                store.cf_live_cells(),
                &outpoint_key,
                &crate::types::encode_live_cell_marker(100),
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_cells_batch(&refs, &store).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing canonical cell for live marker in get_cells_batch"));
    }

    #[test]
    fn test_get_consumed_cells_batch_fails_on_invalid_consumed_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xCD; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_consumed_cells(),
                &outpoint_key,
                b"invalid-consumed-payload",
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_consumed_cells_batch(&refs, &store).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to decode consumed cell meta"));
    }

    #[test]
    fn test_get_consumed_cells_batch_fails_when_marker_has_no_canonical_cell() {
        let (_dir, store) = test_store();
        let tx_hash = [0xDE; 32];
        let cell = make_cell(123_00000000, 61_00000000, &[0x01; 32]);

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &cell, 100);
        batch.put_consumed_cell_with_consumer(&tx_hash, 0, &cell, 100, 100, None);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();
        store
            .delete_cf(store.cf_cells(), &keys::encode_outpoint(&tx_hash, 0))
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_consumed_cells_batch(&refs, &store).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing canonical cell for consumed outpoint in get_consumed_cells_batch"));
    }

    #[test]
    fn test_get_consumed_cell_meta_batch_returns_consumer_metadata() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let consumed_cell = make_cell(200_00000000, 61_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];
        let consumed_by_tx = [0x22u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &consumed_cell, 100);
        batch.put_consumed_cell_with_consumer(
            &tx_hash,
            0,
            &consumed_cell,
            100,
            12345,
            Some(&consumed_by_tx),
        );
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let metas = store.get_consumed_cell_meta_batch(&refs, &store).unwrap();
        let meta = metas.get(&(tx_hash.to_vec(), 0)).unwrap();
        assert_eq!(meta.consumed_at_block, 12345);
        assert_eq!(meta.consumed_by_tx, Some(consumed_by_tx.to_vec()));
    }

    #[test]
    fn test_get_consumed_cell_meta_batch_fails_on_invalid_consumed_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xCE; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_consumed_cells(),
                &outpoint_key,
                b"invalid-consumed-payload",
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store
            .get_consumed_cell_meta_batch(&refs, &store)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to decode consumed cell meta in get_consumed_cell_meta_batch"));
    }

    #[test]
    fn test_get_consumed_cell_info_rejects_legacy_consumed_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xEF; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        let cell = make_cell(200_00000000, 61_00000000, &[0x01; 32]);
        let legacy = ConsumedCellInfo::from_live_cell_info_with_consumer(&cell, 123, None, 100);
        let legacy_payload = postcard::to_allocvec(&legacy).unwrap();

        store
            .put_cf(
                store.cf_cells(),
                &outpoint_key,
                &postcard::to_allocvec(&cell).unwrap(),
            )
            .unwrap();
        store
            .put_cf(store.cf_consumed_cells(), &outpoint_key, &legacy_payload)
            .unwrap();

        let err = store
            .get_consumed_cell_info(&tx_hash, 0, &store)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to decode consumed cell meta"));
    }

    #[test]
    fn test_list_all_cells_by_data_hash_returns_live_and_consumed() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let data_hash = vec![0xAA; 32];
        let tx1 = vec![0x01; 32];
        let tx2 = vec![0x02; 32];
        let cell = LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 50,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        };

        // Cell 1: live at block 5
        let mut batch = StoreBatch::new(&store);
        batch.put_live_cell_marker_by_outpoint(&tx1, 0, 5);
        batch.put_cell_payload_by_outpoint(&tx1, 0, &cell);
        batch.put_cell_by_data_hash(&data_hash, 5, &tx1, 0);
        batch.commit().unwrap();

        // Cell 2: consumed (created block 10, consumed block 20)
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_payload_by_outpoint(&tx2, 0, &cell);
        batch.put_consumed_cell(&tx2, 0, &cell, 10, 20);
        batch.put_cell_by_data_hash(&data_hash, 10, &tx2, 0);
        batch.commit().unwrap();

        let results = store
            .list_all_cells_by_data_hash(&data_hash, &store)
            .unwrap();
        assert_eq!(results.len(), 2);
        // Sorted by block: block 5 first, block 10 second
        assert_eq!(results[0].0, tx1);
        assert!(results[0].3, "first cell should be live");
        assert_eq!(results[1].0, tx2);
        assert!(!results[1].3, "second cell should be consumed");
    }
}
