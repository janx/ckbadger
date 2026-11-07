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

/// One stored `cell_by_data_hash` row and the cell it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataHashIndexRow {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    /// The block the row is filed under (the cell's creation block).
    pub created_at_block: i64,
    pub live: bool,
    /// The named cell's stored data size.
    pub data_size: i32,
}

/// A bounded page of [`DataHashIndexRow`]s under one data hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataHashIndexPage {
    pub rows: Vec<DataHashIndexRow>,
    /// No further row exists under the data hash.
    pub complete: bool,
}

/// The three `hash(32) + block(8 BE) + outpoint(34)` cell indexes, and what a
/// row's lifetime is bound to.
#[derive(Debug, Clone, Copy)]
enum CellIndex {
    /// `cell_by_lock`: written and deleted in the batch that writes and
    /// deletes the live marker, so every row names a live cell.
    Lock,
    /// `cell_by_type`: same lifetime as `cell_by_lock`.
    Type,
    /// `cell_by_data_hash`: kept when the cell is consumed (code-cell
    /// resolution reads consumed deployments), so every row names a live or a
    /// consumed cell; only live ones are listed.
    DataHash,
}

impl CellIndex {
    fn cf_name(self) -> &'static str {
        match self {
            Self::Lock => crate::store::CF_CELL_BY_LOCK,
            Self::Type => crate::store::CF_CELL_BY_TYPE,
            Self::DataHash => crate::store::CF_CELL_BY_DATA_HASH,
        }
    }

    fn keeps_consumed_rows(self) -> bool {
        matches!(self, Self::DataHash)
    }

    /// `(block, tx_hash, output_index)` of one row; any width but 74 is
    /// corruption.
    fn decode_row_key(self, key: &[u8]) -> anyhow::Result<(i64, Vec<u8>, i16)> {
        if key.len() != keys::CELL_INDEX_KEY_SIZE {
            anyhow::bail!(
                "{} index row has {} bytes, expected {}: key=0x{}",
                self.cf_name(),
                key.len(),
                keys::CELL_INDEX_KEY_SIZE,
                bytes_to_hex(key)
            );
        }
        let block_number = i64::from_be_bytes(
            key[32..40]
                .try_into()
                .expect("a 74-byte cell index key has an 8-byte block field"),
        );
        let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
        Ok((block_number, tx_hash, output_index))
    }

    fn expected_cell(self) -> &'static str {
        if self.keeps_consumed_rows() {
            "live or consumed"
        } else {
            "live"
        }
    }
}

/// Which script slot a cell-by-code index row refers to. Used to verify the
/// index row against the loaded cell payload (fail fast on index corruption).
#[derive(Clone, Copy)]
enum CodeIndexSide {
    Lock,
    Type,
}

impl CodeIndexSide {
    fn name(self) -> &'static str {
        match self {
            CodeIndexSide::Lock => "lock",
            CodeIndexSide::Type => "type",
        }
    }
}

impl CkbadgerStore {
    /// Look up lock script components by lock_hash.
    pub fn get_lock_script(&self, lock_hash: &[u8]) -> anyhow::Result<Option<LockScriptEntry>> {
        match self.get_cf(self.cf_lock_scripts(), lock_hash)? {
            Some(value) => {
                let entry = bincode::deserialize::<LockScriptEntry>(&value).map_err(|e| {
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
                let info = LiveCellInfo::decode(&value).map_err(|e| {
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

    /// Every output payload of transaction `tx_hash` (`outputs_count`
    /// outputs) in one batched read, in output order. A missing payload is
    /// `None` at its index; the caller decides what that means.
    pub fn get_tx_output_payloads(
        &self,
        tx_hash: &[u8],
        outputs_count: i16,
    ) -> anyhow::Result<Vec<Option<LiveCellInfo>>> {
        anyhow::ensure!(
            outputs_count >= 0,
            "negative outputs count {outputs_count} for tx 0x{}",
            bytes_to_hex(tx_hash)
        );
        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = (0..outputs_count)
            .map(|index| keys::encode_outpoint(tx_hash, index))
            .collect();
        let cf = self.cf_cells();
        let values =
            self.multi_get_cf_sorted(keys.iter().map(|key| (cf, key.as_slice())).collect());
        values
            .into_iter()
            .zip(&keys)
            .map(|(value, key)| match value {
                Ok(Some(value)) => LiveCellInfo::decode(&value).map(Some).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize canonical cell payload: outpoint=0x{}, error={}",
                        bytes_to_hex(key),
                        e
                    )
                }),
                Ok(None) => Ok(None),
                Err(e) => Err(anyhow::anyhow!(
                    "rocksdb multi_get failed for cell payload: outpoint=0x{}, error={}",
                    bytes_to_hex(key),
                    e
                )),
            })
            .collect()
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
        let live_values = self.multi_get_cf_sorted(live_cf_keys);

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

        let cell_values = cells_store.multi_get_cf_sorted(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            match value_result {
                Ok(Some(value)) => {
                    let info = LiveCellInfo::decode(&value).map_err(|e| {
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
        Ok(Some(ConsumedCellInfo::new(&cell, meta)))
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
        let consumed_values = self.multi_get_cf_sorted(consumed_cf_keys);

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

        let cell_values = cells_store.multi_get_cf_sorted(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            match value_result {
                Ok(Some(value)) => {
                    let cell = LiveCellInfo::decode(&value).map_err(|e| {
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
        let consumed_values = self.multi_get_cf_sorted(consumed_cf_keys);

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

        let cell_values = cells_store.multi_get_cf_sorted(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            match value_result {
                Ok(Some(_)) => {
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    let meta = metas[outpoint_idx].ok_or_else(|| {
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
            CellIndex::Lock,
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
            CellIndex::Type,
            type_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    /// The live cells of one `hash(32) + block(8 BE) + outpoint(34)` index.
    ///
    /// Fails fast (J8) instead of returning a short page: a key of any other
    /// width, and a row whose cell is not live (or, for `cell_by_data_hash`,
    /// neither live nor consumed), is an indexer bug. The error names the CF,
    /// the key, the outpoint and the block.
    fn list_cells_by_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        index: CellIndex,
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
            let (block_number, tx_hash, output_index) = index.decode_row_key(&key)?;
            match self.get_cell(&tx_hash, output_index, cells_store)? {
                Some(cell) => {
                    results.push((tx_hash, output_index, cell));
                    if results.len() >= limit {
                        break;
                    }
                }
                None => {
                    let consumed = index.keeps_consumed_rows()
                        && self
                            .get_cf(self.cf_consumed_cells(), &key[40..74])?
                            .is_some();
                    if !consumed {
                        anyhow::bail!(
                            "{} index row names no {} cell: key=0x{}, outpoint=0x{}:{}, block={}",
                            index.cf_name(),
                            index.expected_cell(),
                            bytes_to_hex(&key),
                            bytes_to_hex(&tx_hash),
                            output_index,
                            block_number
                        );
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
            CellIndex::DataHash,
            data_hash,
            limit,
            after_key,
            cells_store,
        )
    }

    /// The stored `cell_by_data_hash` rows under one data hash, live and
    /// consumed alike, for the verifier (at most `limit`; `complete` says no
    /// further row exists). Each row must name a live or a consumed cell — a
    /// dangling or malformed row is an error naming it, as in
    /// [`Self::list_cells_by_data_hash`].
    pub fn list_data_hash_index_rows(
        &self,
        data_hash: &[u8],
        limit: usize,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<DataHashIndexPage> {
        let mut rows = Vec::new();
        let mut complete = true;
        let iter = self.iterator_cf(
            self.cf_cell_by_data_hash(),
            rocksdb::IteratorMode::From(data_hash, rocksdb::Direction::Forward),
        );
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate cell_by_data_hash for the verify export: {e}")
            })?;
            if !key.starts_with(data_hash) {
                break;
            }
            if rows.len() == limit {
                complete = false;
                break;
            }
            let (block_number, tx_hash, output_index) = CellIndex::DataHash.decode_row_key(&key)?;
            let (cell, live) = match self.get_cell(&tx_hash, output_index, cells_store)? {
                Some(cell) => (cell, true),
                None => match self.get_consumed_cell(&tx_hash, output_index, cells_store)? {
                    Some(cell) => (cell, false),
                    None => anyhow::bail!(
                        "{} index row names no {} cell: key=0x{}, outpoint=0x{}:{}, block={}",
                        CellIndex::DataHash.cf_name(),
                        CellIndex::DataHash.expected_cell(),
                        bytes_to_hex(&key),
                        bytes_to_hex(&tx_hash),
                        output_index,
                        block_number
                    ),
                },
            };
            rows.push(DataHashIndexRow {
                tx_hash,
                output_index,
                created_at_block: block_number,
                live,
                data_size: cell.data_size,
            });
        }
        Ok(DataHashIndexPage { rows, complete })
    }

    /// Whether the data under `data_hash` is code: `cell_by_data_hash` holds a
    /// row exactly for each cell whose data is code (`keys::is_code_cell`),
    /// and cells with the same data hash carry the same bytes, so one seek
    /// answers for every cell with that data. A row of another width is
    /// corruption.
    pub fn is_code_data_hash(&self, data_hash: &[u8]) -> anyhow::Result<bool> {
        let mut iter = self.iterator_cf(
            self.cf_cell_by_data_hash(),
            rocksdb::IteratorMode::From(data_hash, rocksdb::Direction::Forward),
        );
        match iter.next() {
            Some(item) => {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to seek cell_by_data_hash for data hash 0x{}: {e}",
                        bytes_to_hex(data_hash)
                    )
                })?;
                if !key.starts_with(data_hash) {
                    return Ok(false);
                }
                CellIndex::DataHash.decode_row_key(&key)?;
                Ok(true)
            }
            None => Ok(false),
        }
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

    /// List live cells of one script reference form `(code_hash, hash_type)`
    /// via a prefix scan on `cell_by_lock_code`.
    /// `after_key` is the full 75-byte cell-by-code index key of the last
    /// returned entry (for pagination).
    pub fn list_cells_by_lock_code_hash(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_lock_code(),
            CodeIndexSide::Lock,
            rocksdb::Direction::Forward,
            code_hash,
            hash_type,
            limit,
            after_key,
            cells_store,
        )
    }

    /// [`Self::list_cells_by_lock_code_hash`] newest block first: the same
    /// `cell_by_lock_code` rows read backwards (block desc, then tx hash and
    /// output index desc — the exact reverse of the key order). `after_key`
    /// is the 75-byte key of the last returned entry; the next page starts
    /// strictly below it.
    pub fn list_cells_by_lock_code_hash_desc(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_lock_code(),
            CodeIndexSide::Lock,
            rocksdb::Direction::Reverse,
            code_hash,
            hash_type,
            limit,
            after_key,
            cells_store,
        )
    }

    /// List live cells of one script reference form `(code_hash, hash_type)`
    /// via a prefix scan on `cell_by_type_code`.
    /// `after_key` is the full 75-byte cell-by-code index key of the last
    /// returned entry (for pagination).
    pub fn list_cells_by_type_code_hash(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_type_code(),
            CodeIndexSide::Type,
            rocksdb::Direction::Forward,
            code_hash,
            hash_type,
            limit,
            after_key,
            cells_store,
        )
    }

    /// [`Self::list_cells_by_type_code_hash`] newest block first (the
    /// `cell_by_type_code` rows read backwards; see
    /// [`Self::list_cells_by_lock_code_hash_desc`]).
    pub fn list_cells_by_type_code_hash_desc(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_type_code(),
            CodeIndexSide::Type,
            rocksdb::Direction::Reverse,
            code_hash,
            hash_type,
            limit,
            after_key,
            cells_store,
        )
    }

    /// One scan of one reference form's contiguous key range, in either
    /// direction. Forward starts at the form's prefix; Reverse seeks to the
    /// last key at or below the form's upper bound (`prefix ‖ 0xFF…`, which
    /// every 75-byte key of the form sorts at or below). With a cursor both
    /// seek to the cursor key and skip it if it is still there.
    #[allow(clippy::too_many_arguments)]
    fn list_cells_by_code_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        side: CodeIndexSide,
        direction: rocksdb::Direction,
        code_hash: &[u8],
        hash_type: u8,
        limit: usize,
        after_key: Option<&[u8]>,
        cells_store: &CkbadgerStore,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo)>> {
        let mut results = Vec::new();

        let prefix = keys::encode_cell_code_index_prefix(code_hash, hash_type);
        let start_key = match (after_key, direction) {
            (Some(k), _) => k.to_vec(),
            (None, rocksdb::Direction::Forward) => prefix.clone(),
            (None, rocksdb::Direction::Reverse) => {
                let mut upper = prefix.clone();
                upper.resize(keys::CELL_CODE_INDEX_KEY_SIZE, 0xFF);
                upper
            }
        };

        let iter = self.iterator_cf(cf, rocksdb::IteratorMode::From(&start_key, direction));

        let mut first = after_key.is_some();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate code-hash cell index in list_cells_by_code_hash_cf: {}",
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
            // Key: code_hash(32) + hash_type(1) + block_num(8) + outpoint(34) = 75
            if key.len() != keys::CELL_CODE_INDEX_KEY_SIZE {
                anyhow::bail!(
                    "malformed cell-by-{}-code index key: key=0x{}, len={}, expected={}",
                    side.name(),
                    hex::encode(&key),
                    key.len(),
                    keys::CELL_CODE_INDEX_KEY_SIZE
                );
            }
            let (tx_hash, output_index) = keys::decode_outpoint(&key[41..75]);
            if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                let payload_matches = match side {
                    CodeIndexSide::Lock => {
                        cell.lock_code_hash.as_slice() == &code_hash[..32]
                            && cell.lock_hash_type == i16::from(hash_type)
                    }
                    CodeIndexSide::Type => {
                        cell.type_code_hash.as_ref().map(|hash| hash.as_slice())
                            == Some(&code_hash[..32])
                            && cell.type_hash_type == Some(i16::from(hash_type))
                    }
                };
                if !payload_matches {
                    anyhow::bail!(
                        "cell-by-{}-code index row disagrees with cell payload: code_hash=0x{}, hash_type={}, outpoint=0x{}:{}",
                        side.name(),
                        hex::encode(&code_hash[..32]),
                        hash_type,
                        hex::encode(&tx_hash),
                        output_index
                    );
                }
                results.push((tx_hash, output_index, cell));
                if results.len() >= limit {
                    break;
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
            stats.total_used_capacity += cell.occupied_capacity() as i128;
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

    /// The type script's args tell fixtures apart: its hash is derived.
    fn make_cell(capacity: i64, type_args: &[u8]) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            lock_code_hash: [0xBB; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_code_hash: Some([0xCC; 32]),
            type_hash_type: Some(1),
            type_args: Some(type_args.to_vec()),
            data_size: 0,
            udt_amount: None,
            data_hash: None,
        }
    }

    #[test]
    fn tx_output_payloads_are_one_batched_read_in_output_order() {
        let (_dir, store) = test_store();
        let tx = [0x42; 32];
        insert_cell(&store, &tx, 0, &make_cell(100, &[0x01; 32]));
        insert_cell(&store, &tx, 2, &make_cell(300, &[0x01; 32]));
        store.reset_read_call_counters();
        let payloads = store.get_tx_output_payloads(&tx, 3).unwrap();
        assert_eq!(store.read_call_counts(), (0, 1));
        let capacities: Vec<_> = payloads
            .iter()
            .map(|p| p.as_ref().map(|c| c.capacity))
            .collect();
        assert_eq!(capacities, [Some(100), None, Some(300)]);
        assert!(store.get_tx_output_payloads(&tx, -1).is_err());
    }

    /// A live typed cell and its `cell_by_type` row, keyed by the cell's own
    /// derived type script hash, as the indexer writes them.
    fn insert_cell(store: &CkbadgerStore, tx_hash: &[u8], output_index: i16, cell: &LiveCellInfo) {
        // Write canonical payload + live marker
        let outpoint_key = keys::encode_outpoint(tx_hash, output_index);
        let value = cell.encode().unwrap();
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
        let type_hash = cell
            .type_script_hash()
            .expect("insert_cell takes a typed cell");
        let idx_key = keys::encode_cell_index_key(&type_hash, 100, tx_hash, output_index);
        store
            .put_cf(store.cf_cell_by_type(), &idx_key, &[])
            .unwrap();
    }

    /// Insert a live cell plus its cell-by-lock-code / cell-by-type-code index
    /// rows, exactly as the indexer write path does.
    fn insert_code_indexed_cell(
        store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
        created_at_block: i64,
        cell: &LiveCellInfo,
    ) {
        let mut batch = StoreBatch::new(store);
        batch.put_cell(tx_hash, output_index, cell, created_at_block);
        batch.put_cell_by_lock_code(
            &cell.lock_code_hash,
            cell.lock_hash_type as u8,
            created_at_block,
            tx_hash,
            output_index,
        );
        if let (Some(type_code_hash), Some(type_hash_type)) =
            (&cell.type_code_hash, cell.type_hash_type)
        {
            batch.put_cell_by_type_code(
                type_code_hash,
                type_hash_type as u8,
                created_at_block,
                tx_hash,
                output_index,
            );
        }
        batch.commit().unwrap();
    }

    fn code_form_cell(lock_code_hash: &[u8; 32], lock_hash_type: i16) -> LiveCellInfo {
        LiveCellInfo {
            capacity: 100_00000000,
            lock_code_hash: *lock_code_hash,
            lock_hash_type,
            lock_args: vec![],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            udt_amount: None,
            data_hash: None,
        }
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_never_scans_a_sibling_form() {
        // Same code_hash bytes, two hash_type forms: a dense `type` form and a
        // sparse `data` form. Each query must see only its own form's rows.
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];

        for i in 0..8u8 {
            let tx_hash = [0x10 + i; 32];
            insert_code_indexed_cell(
                &store,
                &tx_hash,
                0,
                100 + i as i64,
                &code_form_cell(&code_hash, 1),
            );
        }
        let data_form_tx = [0xd1; 32];
        insert_code_indexed_cell(
            &store,
            &data_form_tx,
            0,
            200,
            &code_form_cell(&code_hash, 0),
        );

        let data_rows = store
            .list_cells_by_lock_code_hash(&code_hash, 0, 100, None, &store)
            .unwrap();
        assert_eq!(data_rows.len(), 1, "data form has exactly one live cell");
        assert_eq!(data_rows[0].0, data_form_tx.to_vec());
        assert!(data_rows
            .iter()
            .all(|(_, _, info)| info.cell.lock_hash_type == 0));

        let type_rows = store
            .list_cells_by_lock_code_hash(&code_hash, 1, 100, None, &store)
            .unwrap();
        assert_eq!(type_rows.len(), 8, "type form has exactly eight live cells");
        assert!(type_rows
            .iter()
            .all(|(_, _, info)| info.cell.lock_hash_type == 1));

        // An unobserved form is empty without touching the sibling forms.
        let data1_rows = store
            .list_cells_by_lock_code_hash(&code_hash, 2, 100, None, &store)
            .unwrap();
        assert!(data1_rows.is_empty());
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_paginates_within_one_form() {
        // Paging a form must enumerate its rows exactly once, and must not
        // spill into the sibling form when the form is exhausted.
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];

        let mut expected = Vec::new();
        for i in 0..5u8 {
            let tx_hash = [0x20 + i; 32];
            insert_code_indexed_cell(
                &store,
                &tx_hash,
                0,
                300 + i as i64,
                &code_form_cell(&code_hash, 1),
            );
            expected.push(tx_hash.to_vec());
        }
        // Sibling data-form rows sort before the type-form rows and must never
        // appear in a type-form page.
        for i in 0..3u8 {
            insert_code_indexed_cell(
                &store,
                &[0xe0 + i; 32],
                0,
                301 + i as i64,
                &code_form_cell(&code_hash, 0),
            );
        }

        let mut seen: Vec<Vec<u8>> = Vec::new();
        let mut cursor: Option<Vec<u8>> = None;
        loop {
            let page = store
                .list_cells_by_lock_code_hash(&code_hash, 1, 2, cursor.as_deref(), &store)
                .unwrap();
            if page.is_empty() {
                break;
            }
            for (tx_hash, output_index, info) in &page {
                assert_eq!(info.cell.lock_hash_type, 1);
                seen.push(tx_hash.clone());
                cursor = Some(keys::encode_cell_code_index_key(
                    &code_hash,
                    1,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                ));
            }
            if page.len() < 2 {
                break;
            }
        }

        assert_eq!(seen, expected, "each type-form row appears exactly once");
    }

    /// Seed one `type` lock form (hash_type 1) between two sibling forms of the
    /// same code hash: `data` (0, sorts before it) and `data1` (2, sorts after
    /// it). Several rows share a block and a tx, so the key's block, tx hash
    /// and output index all decide the order.
    fn seed_three_lock_forms(store: &CkbadgerStore, code_hash: &[u8; 32]) {
        let rows: [(u8, i16, i64); 7] = [
            (0x20, 0, 300),
            (0x21, 0, 301),
            (0x21, 1, 301),
            (0x22, 0, 301),
            (0x23, 4, 302),
            (0x10, 0, 305),
            (0x30, 2, 305),
        ];
        for (tx_byte, output_index, block) in rows {
            insert_code_indexed_cell(
                store,
                &[tx_byte; 32],
                output_index,
                block,
                &code_form_cell(code_hash, 1),
            );
        }
        insert_code_indexed_cell(store, &[0xe0; 32], 0, 900, &code_form_cell(code_hash, 0));
        insert_code_indexed_cell(store, &[0x01; 32], 0, 1, &code_form_cell(code_hash, 2));
    }

    fn outpoints(rows: &[(Vec<u8>, i16, PositionedCellInfo)]) -> Vec<(Vec<u8>, i16, i64)> {
        rows.iter()
            .map(|(tx_hash, output_index, info)| {
                (tx_hash.clone(), *output_index, info.created_at_block)
            })
            .collect()
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_desc_is_the_exact_reverse_of_asc() {
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];
        seed_three_lock_forms(&store, &code_hash);

        let asc = store
            .list_cells_by_lock_code_hash(&code_hash, 1, 100, None, &store)
            .unwrap();
        let desc = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 1, 100, None, &store)
            .unwrap();
        assert_eq!(asc.len(), 7, "the type form has seven live cells");
        let mut reversed = outpoints(&asc);
        reversed.reverse();
        assert_eq!(
            outpoints(&desc),
            reversed,
            "desc = asc reversed, key by key"
        );
        assert_eq!(
            desc[0].2.created_at_block, 305,
            "the newest block comes first"
        );
        assert!(
            desc.iter()
                .all(|(_, _, info)| info.cell.lock_hash_type == 1),
            "neither sibling form (data before, data1 after) leaks into the page"
        );

        // The sibling forms read on their own, newest first too.
        let data = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 0, 100, None, &store)
            .unwrap();
        assert_eq!(outpoints(&data), vec![(vec![0xe0; 32], 0, 900)]);
        let data1 = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 2, 100, None, &store)
            .unwrap();
        assert_eq!(outpoints(&data1), vec![(vec![0x01; 32], 0, 1)]);
        let data2 = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 3, 100, None, &store)
            .unwrap();
        assert!(data2.is_empty(), "an unobserved form is empty");
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_desc_cursor_continues_below_the_last_key() {
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];
        seed_three_lock_forms(&store, &code_hash);

        let mut expected = outpoints(
            &store
                .list_cells_by_lock_code_hash(&code_hash, 1, 100, None, &store)
                .unwrap(),
        );
        expected.reverse();

        let mut seen = Vec::new();
        let mut cursor: Option<Vec<u8>> = None;
        let mut pages = 0;
        loop {
            let page = store
                .list_cells_by_lock_code_hash_desc(&code_hash, 1, 2, cursor.as_deref(), &store)
                .unwrap();
            pages += 1;
            assert!(page.len() <= 2);
            if let Some((tx_hash, output_index, info)) = page.last() {
                cursor = Some(keys::encode_cell_code_index_key(
                    &code_hash,
                    1,
                    info.created_at_block,
                    tx_hash,
                    *output_index,
                ));
            }
            seen.extend(outpoints(&page));
            if page.len() < 2 {
                break;
            }
        }
        assert_eq!(seen, expected, "every row once, newest first, across pages");
        assert_eq!(pages, 4, "7 rows at 2 a page: 2 + 2 + 2 + 1");
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_desc_cursor_survives_its_row_being_spent() {
        // The cursor's own row is consumed between two pages: the reverse seek
        // lands on the next lower key, which must be returned, not skipped.
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];
        seed_three_lock_forms(&store, &code_hash);
        let first = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 1, 2, None, &store)
            .unwrap();
        let (tx_hash, output_index, info) = first.last().unwrap();
        let cursor = keys::encode_cell_code_index_key(
            &code_hash,
            1,
            info.created_at_block,
            tx_hash,
            *output_index,
        );
        let mut batch = StoreBatch::new(&store);
        batch.delete_cell(tx_hash, *output_index);
        batch.delete_cell_by_lock_code(
            &code_hash,
            1,
            info.created_at_block,
            tx_hash,
            *output_index,
        );
        batch.commit().unwrap();

        let next = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 1, 2, Some(&cursor), &store)
            .unwrap();
        // Seeded type-form keys, newest first: 305/0x30:2, 305/0x10:0, 302/0x23:4, 301/0x22:0, …
        assert_eq!(
            outpoints(&next),
            vec![(vec![0x23; 32], 4, 302), (vec![0x22; 32], 0, 301)]
        );
    }

    #[test]
    fn test_list_cells_by_type_code_hash_desc_is_the_exact_reverse_of_asc() {
        let (_dir, store) = test_store();
        let type_code_hash = [0x77; 32];
        for (tx_byte, block) in [(0x31u8, 400i64), (0x32, 402), (0x33, 401)] {
            let mut typed = code_form_cell(&[0xBB; 32], 1);
            typed.type_code_hash = Some(type_code_hash);
            typed.type_hash_type = Some(1);
            typed.type_args = Some(vec![]);
            insert_code_indexed_cell(&store, &[tx_byte; 32], 0, block, &typed);
        }
        let asc = store
            .list_cells_by_type_code_hash(&type_code_hash, 1, 100, None, &store)
            .unwrap();
        let desc = store
            .list_cells_by_type_code_hash_desc(&type_code_hash, 1, 100, None, &store)
            .unwrap();
        let mut reversed = outpoints(&asc);
        reversed.reverse();
        assert_eq!(outpoints(&desc), reversed);
        assert_eq!(
            desc.iter()
                .map(|(_, _, info)| info.created_at_block)
                .collect::<Vec<_>>(),
            vec![402, 401, 400]
        );
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_desc_rejects_index_payload_disagreement() {
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];
        let tx_hash = [0x41; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &code_form_cell(&code_hash, 1), 500);
        batch.commit().unwrap();
        store
            .put_cf(
                store.cf_cell_by_lock_code(),
                &keys::encode_cell_code_index_key(&code_hash, 0, 500, &tx_hash, 0),
                &[],
            )
            .unwrap();
        let err = store
            .list_cells_by_lock_code_hash_desc(&code_hash, 0, 10, None, &store)
            .expect_err("index/payload disagreement must fail fast in either direction");
        assert!(err
            .to_string()
            .contains("index row disagrees with cell payload"));
    }

    #[test]
    fn test_list_cells_by_type_code_hash_isolates_forms() {
        let (_dir, store) = test_store();
        let type_code_hash = [0x77; 32];

        let mut typed = code_form_cell(&[0xBB; 32], 1);
        typed.type_code_hash = Some(type_code_hash);
        typed.type_hash_type = Some(1);
        typed.type_args = Some(vec![]);
        insert_code_indexed_cell(&store, &[0x31; 32], 0, 400, &typed);

        let mut data_typed = typed.clone();
        data_typed.type_hash_type = Some(0);
        insert_code_indexed_cell(&store, &[0x32; 32], 0, 401, &data_typed);

        let type_rows = store
            .list_cells_by_type_code_hash(&type_code_hash, 1, 100, None, &store)
            .unwrap();
        assert_eq!(type_rows.len(), 1);
        assert_eq!(type_rows[0].0, vec![0x31; 32]);

        let data_rows = store
            .list_cells_by_type_code_hash(&type_code_hash, 0, 100, None, &store)
            .unwrap();
        assert_eq!(data_rows.len(), 1);
        assert_eq!(data_rows[0].0, vec![0x32; 32]);
    }

    #[test]
    fn test_list_cells_by_lock_code_hash_rejects_index_payload_disagreement() {
        // A cell-by-code row filed under a hash_type its payload does not use
        // is index corruption; the read path must fail fast, not return the row.
        let (_dir, store) = test_store();
        let code_hash = [0x9b; 32];
        let tx_hash = [0x41; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &code_form_cell(&code_hash, 1), 500);
        batch.commit().unwrap();
        // Hand-written row under the wrong form.
        store
            .put_cf(
                store.cf_cell_by_lock_code(),
                &keys::encode_cell_code_index_key(&code_hash, 0, 500, &tx_hash, 0),
                &[],
            )
            .unwrap();

        let err = store
            .list_cells_by_lock_code_hash(&code_hash, 0, 10, None, &store)
            .expect_err("index/payload disagreement must fail fast");
        let message = err.to_string();
        assert!(
            message.contains("index row disagrees with cell payload"),
            "unexpected error: {message}"
        );
        assert!(message.contains(&hex::encode(tx_hash)));
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
        let tx_hash = [0x11u8; 32];
        let cell = make_cell(200_00000000, &[0x01; 32]);
        let type_hash = cell.type_script_hash().unwrap();
        insert_cell(&store, &tx_hash, 0, &cell);

        let stats = store
            .aggregate_token_cell_stats(&type_hash, &store)
            .unwrap();
        assert_eq!(stats.cells_count, 1);
        assert_eq!(stats.total_capacity, 200_00000000);
        // Derived: 8 + 33 (empty lock args) + 33 + 32 (type args), no data.
        assert_eq!(stats.total_used_capacity, 106_00000000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_multiple_cells() {
        let (_dir, store) = test_store();
        let type_args = [0x01u8; 32];

        let tx1 = [0x11u8; 32];
        let cell1 = make_cell(200_00000000, &type_args);
        insert_cell(&store, &tx1, 0, &cell1);

        let tx2 = [0x22u8; 32];
        let mut cell2 = make_cell(300_00000000, &type_args);
        cell2.data_size = 19;
        insert_cell(&store, &tx2, 0, &cell2);

        let tx3 = [0x33u8; 32];
        let cell3 = make_cell(150_00000000, &type_args);
        insert_cell(&store, &tx3, 1, &cell3);
        let type_hash = cell1.type_script_hash().unwrap();

        let stats = store
            .aggregate_token_cell_stats(&type_hash, &store)
            .unwrap();
        assert_eq!(stats.cells_count, 3);
        assert_eq!(stats.total_capacity, 650_00000000);
        // 106 CKB per cell, plus 19 data bytes on the second.
        assert_eq!(stats.total_used_capacity, (3 * 106 + 19) * 100_000_000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_different_types_isolated() {
        let (_dir, store) = test_store();
        let tx1 = [0x11u8; 32];
        let cell1 = make_cell(200_00000000, &[0x01; 32]);
        insert_cell(&store, &tx1, 0, &cell1);

        let tx2 = [0x22u8; 32];
        let cell2 = make_cell(500_00000000, &[0x02; 32]);
        insert_cell(&store, &tx2, 0, &cell2);
        let type_a = cell1.type_script_hash().unwrap();
        let type_b = cell2.type_script_hash().unwrap();

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
        let consumed_cell = make_cell(200_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &consumed_cell, 100);
        batch.put_consumed_cell(&tx_hash, 0, &consumed_cell, 100, 12345, 2);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        let info = store
            .get_consumed_cell_info(&tx_hash, 0, &store)
            .unwrap()
            .unwrap();
        assert_eq!(info.consumed_at_block, 12345);
        assert_eq!(info.consumer_position(), (12345, 2));
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
        let cell = make_cell(123_00000000, &[0x01; 32]);

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &cell, 100);
        batch.put_consumed_cell(&tx_hash, 0, &cell, 100, 100, 1);
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
        let consumed_cell = make_cell(200_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &consumed_cell, 100);
        batch.put_consumed_cell(&tx_hash, 0, &consumed_cell, 100, 12345, 5);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let metas = store.get_consumed_cell_meta_batch(&refs, &store).unwrap();
        let meta = metas.get(&(tx_hash.to_vec(), 0)).unwrap();
        assert_eq!(meta.consumed_at_block, 12345);
        assert_eq!(meta.consumer_position(), (12345, 5));
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
        let cell = make_cell(200_00000000, &[0x01; 32]);
        let legacy = crate::types::v1_layout::ConsumedCellInfoV1 {
            cell: crate::types::v1_layout::LiveCellInfoV1::from_v2(&cell),
            consumed_at_block: 123,
            consumed_by_tx: None,
            created_at_block: 100,
        };
        let legacy_payload = bincode::serialize(&legacy).unwrap();

        store
            .put_cf(store.cf_cells(), &outpoint_key, &cell.encode().unwrap())
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

    /// Finding 8: whether a data hash names code is the code-cell index's
    /// answer; a row of the wrong width is corruption.
    #[test]
    fn is_code_data_hash_answers_from_the_code_cell_index() {
        let (_dir, store) = test_store();
        let code = [0x5au8; 32];
        let other = [0x5bu8; 32];
        store
            .put_cf(
                store.cf_cell_by_data_hash(),
                &keys::encode_cell_index_key(&code, 5, &[0x21; 32], 0),
                &[],
            )
            .unwrap();
        assert!(store.is_code_data_hash(&code).unwrap());
        assert!(!store.is_code_data_hash(&other).unwrap());
        assert!(!store.is_code_data_hash(&[0x00; 32]).unwrap());
        let mut bad = other.to_vec();
        bad.extend_from_slice(&[1, 2, 3]);
        store
            .put_cf(store.cf_cell_by_data_hash(), &bad, &[])
            .unwrap();
        assert!(store.is_code_data_hash(&other).is_err());
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
            lock_code_hash: [0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 50,
            udt_amount: None,
            data_hash: None,
        };

        // Cell 1: live at block 5
        let mut batch = StoreBatch::new(&store);
        batch.put_live_cell_marker_by_outpoint(&tx1, 0, 5);
        batch.put_cell_payload_by_outpoint(&tx1, 0, &cell).unwrap();
        batch.put_cell_by_data_hash(&data_hash, 5, &tx1, 0);
        batch.commit().unwrap();

        // Cell 2: consumed (created block 10, consumed block 20)
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_payload_by_outpoint(&tx2, 0, &cell).unwrap();
        batch.put_consumed_cell(&tx2, 0, &cell, 10, 20, 1);
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

    // ── J8: `list_cells_by_hash_cf` fails fast on a dangling or malformed row ──

    /// A plain cell, typed by code `[0x44; 32]` with these args when given.
    fn plain_cell(type_args: Option<&[u8]>) -> LiveCellInfo {
        LiveCellInfo {
            capacity: 100_00000000,
            lock_code_hash: [0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_code_hash: type_args.map(|_| [0x44; 32]),
            type_hash_type: type_args.map(|_| 1),
            type_args: type_args.map(<[u8]>::to_vec),
            data_size: 50,
            udt_amount: None,
            data_hash: None,
        }
    }

    /// The writers delete a `cell_by_type` row in the same batch as the live
    /// marker, so a row without a live cell is an indexer bug: the listing
    /// must name it instead of returning a shorter page.
    #[test]
    fn test_list_cells_by_type_fails_on_a_row_without_a_live_cell() {
        let (_dir, store) = test_store();
        let live_cell = plain_cell(Some(&[0x7a; 32]));
        let type_hash = live_cell.type_script_hash().unwrap();
        let live_tx = [0x01; 32];
        let dangling_tx = [0x02; 32];
        insert_cell(&store, &live_tx, 0, &live_cell);
        let dangling_key = keys::encode_cell_index_key(&type_hash, 123, &dangling_tx, 3);
        store
            .put_cf(store.cf_cell_by_type(), &dangling_key, &[])
            .unwrap();

        let err = store
            .list_cells_by_type(&type_hash, 10, None, &store)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("cell_by_type")
                && err.contains(&hex::encode(&dangling_key))
                && err.contains(&format!("outpoint=0x{}:3", hex::encode(dangling_tx)))
                && err.contains("block=123"),
            "{err}"
        );
    }

    #[test]
    fn test_list_cells_by_lock_fails_on_a_row_without_a_live_cell() {
        let (_dir, store) = test_store();
        let lock_hash = [0x11; 32];
        let dangling_tx = [0x05; 32];
        let dangling_key = keys::encode_cell_index_key(&lock_hash, 77, &dangling_tx, 0);
        store
            .put_cf(store.cf_cell_by_lock(), &dangling_key, &[])
            .unwrap();

        let err = store
            .list_cells_by_lock(&lock_hash, 10, None, &store)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("cell_by_lock")
                && err.contains(&hex::encode(&dangling_key))
                && err.contains(&format!("outpoint=0x{}:0", hex::encode(dangling_tx)))
                && err.contains("block=77"),
            "{err}"
        );
    }

    /// Every row is 74 bytes (`encode_cell_index_key`); a shorter one under
    /// the prefix is corruption, not a row to step over.
    #[test]
    fn test_list_cells_by_type_fails_on_a_short_key() {
        let (_dir, store) = test_store();
        let type_hash = [0x7b; 32];
        let mut short_key = type_hash.to_vec();
        short_key.extend_from_slice(&[0x00; 8]);
        store
            .put_cf(store.cf_cell_by_type(), &short_key, &[])
            .unwrap();

        let err = store
            .list_cells_by_type(&type_hash, 10, None, &store)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("cell_by_type")
                && err.contains(&hex::encode(&short_key))
                && err.contains("40 bytes, expected 74"),
            "{err}"
        );
    }

    /// `cell_by_data_hash` rows outlive consumption by design (code-cell
    /// resolution reads consumed deployments): a consumed cell is passed over,
    /// a live one is listed — but a row naming neither is dangling.
    #[test]
    fn test_list_cells_by_data_hash_passes_over_consumed_and_fails_on_dangling() {
        let (_dir, store) = test_store();
        let data_hash = [0xda; 32];
        let live_tx = [0x01; 32];
        let consumed_tx = [0x02; 32];
        let cell = plain_cell(None);
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&live_tx, 0, &cell, 5);
        batch.put_cell_by_data_hash(&data_hash, 5, &live_tx, 0);
        batch
            .put_cell_payload_by_outpoint(&consumed_tx, 0, &cell)
            .unwrap();
        batch.put_consumed_cell(&consumed_tx, 0, &cell, 10, 20, 1);
        batch.put_cell_by_data_hash(&data_hash, 10, &consumed_tx, 0);
        batch.commit().unwrap();

        let listed = store
            .list_cells_by_data_hash(&data_hash, 10, None, &store)
            .unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|(tx, index, _)| (tx.clone(), *index))
                .collect::<Vec<_>>(),
            vec![(live_tx.to_vec(), 0)],
            "the live cell is listed, the consumed one passed over"
        );

        let dangling_tx = [0x03; 32];
        let dangling_key = keys::encode_cell_index_key(&data_hash, 30, &dangling_tx, 1);
        store
            .put_cf(store.cf_cell_by_data_hash(), &dangling_key, &[])
            .unwrap();
        let err = store
            .list_cells_by_data_hash(&data_hash, 10, None, &store)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cell_by_data_hash")
                && err.contains(&hex::encode(&dangling_key))
                && err.contains(&format!("outpoint=0x{}:1", hex::encode(dangling_tx)))
                && err.contains("block=30"),
            "{err}"
        );
    }
}
