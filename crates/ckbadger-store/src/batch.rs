//! WriteBatch builder for atomic multi-CF writes.

use rocksdb::{ColumnFamily, WriteBatch};
use std::collections::HashMap;

use crate::keys;
use crate::store::{CkbadgerStore, StoreWriteIntent};
use crate::types::*;

#[derive(Debug)]
struct AppendBatchOp {
    cf_name: &'static str,
    key: Vec<u8>,
    value: Option<Vec<u8>>,
}

use crate::bytes_to_hex;

/// Accumulates writes across all CFs and commits atomically.
pub struct StoreBatch<'a> {
    store: &'a CkbadgerStore,
    batch: WriteBatch,
    append_ops: Vec<AppendBatchOp>,
    pending_dao_deposits: HashMap<Vec<u8>, DaoDepositCacheEntry>,
}

impl<'a> StoreBatch<'a> {
    pub fn new(store: &'a CkbadgerStore) -> Self {
        Self {
            store,
            batch: WriteBatch::default(),
            append_ops: Vec::new(),
            pending_dao_deposits: HashMap::new(),
        }
    }

    /// Commit all accumulated writes atomically.
    pub fn commit(self) -> anyhow::Result<()> {
        self.commit_inner(false)
    }

    /// Commit with WAL disabled. Use during bulk sync where crash recovery
    /// re-syncs from the last committed block header.
    pub fn commit_no_wal(self) -> anyhow::Result<()> {
        self.commit_inner(true)
    }

    /// Get the number of operations in the batch.
    pub fn len(&self) -> usize {
        self.batch.len() + self.append_ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.batch.is_empty() && self.append_ops.is_empty()
    }

    /// Get the approximate size of the batch in bytes.
    pub fn size_in_bytes(&self) -> usize {
        self.batch.size_in_bytes()
    }

    /// Merge another `StoreBatch` into this one.
    ///
    /// Both batches must reference the same `CkbadgerStore` instance.
    /// After merging, `other` is consumed and all its operations are
    /// appended to `self`.
    ///
    /// The domain `WriteBatch` is merged at the byte level using
    /// RocksDB's serialized representation (`data()` / `from_data()`).
    /// Append-only ops and pending DAO deposits are merged by extending
    /// the corresponding collections.
    // NOTE: This relies on the RocksDB WriteBatch internal wire format:
    // [0..8] sequence number (u64 LE), [8..12] entry count (u32 LE), [12..] operations.
    // Validated against rust-rocksdb 0.22.x / RocksDB 9.x. If upgrading RocksDB,
    // verify this format hasn't changed.
    pub fn merge_from(&mut self, other: StoreBatch<'a>) {
        assert!(
            std::ptr::eq(self.store, other.store),
            "StoreBatch::merge_from: both batches must reference the same CkbadgerStore"
        );

        // Merge domain WriteBatch via raw byte-level concatenation.
        //
        // WriteBatch wire format (little-endian):
        //   [0..8]   sequence number (u64)
        //   [8..12]  operation count (u32)
        //   [12..]   serialized operations
        //
        // We keep self's sequence, sum the counts, and concatenate ops.
        if !other.batch.is_empty() {
            let self_data = self.batch.data();
            let other_data = other.batch.data();

            let self_count = u32::from_le_bytes(
                self_data[8..12]
                    .try_into()
                    .expect("WriteBatch header must be at least 12 bytes"),
            );
            let other_count = u32::from_le_bytes(
                other_data[8..12]
                    .try_into()
                    .expect("WriteBatch header must be at least 12 bytes"),
            );
            let total_count = self_count.checked_add(other_count).expect(
                "StoreBatch::merge_from: operation count overflow merging WriteBatch entries",
            );

            let mut merged = Vec::with_capacity(self_data.len() + other_data.len() - 12);
            merged.extend_from_slice(&self_data[..8]); // sequence from self
            merged.extend_from_slice(&total_count.to_le_bytes()); // combined count
            merged.extend_from_slice(&self_data[12..]); // ops from self
            merged.extend_from_slice(&other_data[12..]); // ops from other

            self.batch = WriteBatch::from_data(&merged);
        }

        // Merge append-only ops.
        self.append_ops.extend(other.append_ops);

        // Merge pending DAO deposits (other's entries win on conflict,
        // preserving last-write-wins within a batch merge sequence).
        self.pending_dao_deposits.extend(other.pending_dao_deposits);
    }

    fn commit_inner(self, no_wal: bool) -> anyhow::Result<()> {
        if self.store.is_append_only_store() {
            let intent = if self.store.is_bulk_sync_mode() {
                StoreWriteIntent::BulkSyncAppendValidated
            } else {
                StoreWriteIntent::AppendValidated
            };
            // Bulk sync is constrained to fresh-db rebuilds; skip per-key existence
            // probes to avoid read-before-write overhead on the hot append path.
            let skip_existing_probe = matches!(intent, StoreWriteIntent::BulkSyncAppendValidated);
            let mut seen_ops: HashMap<(&'static str, Vec<u8>), usize> = HashMap::new();
            let mut filtered_batch = WriteBatch::default();
            for (idx, op) in self.append_ops.iter().enumerate() {
                let dedupe_key = (op.cf_name, op.key.clone());
                if let Some(first_idx) = seen_ops.insert(dedupe_key, idx) {
                    anyhow::bail!(
                        "append-only batch duplicate key blocked: cf={}, key=0x{}, first_op_index={}, second_op_index={}",
                        op.cf_name,
                        bytes_to_hex(&op.key),
                        first_idx,
                        idx
                    );
                }
                let cf = self.store.cf(op.cf_name);
                if let Some(value) = op.value.as_deref() {
                    if !skip_existing_probe {
                        if let Some(existing) = self.store.get_cf(cf, &op.key)? {
                            if existing.as_slice() == value {
                                // Replay-safe idempotency: same key+value in append-only is already committed.
                                continue;
                            }
                            anyhow::bail!(
                                "append-only overwrite blocked: cf={}, key=0x{}, existing_len={}, new_len={}",
                                op.cf_name,
                                bytes_to_hex(&op.key),
                                existing.len(),
                                value.len()
                            );
                        }
                    }
                    filtered_batch.put_cf(cf, &op.key, value);
                } else {
                    self.store
                        .validate_append_delete_by_cf_name(op.cf_name, &op.key, intent)?;
                    filtered_batch.delete_cf(cf, &op.key);
                }
            }
            if no_wal {
                self.store
                    .write_batch_no_wal_with_intent(filtered_batch, intent)
            } else {
                self.store.write_batch_with_intent(filtered_batch, intent)
            }
        } else if no_wal {
            self.store.write_batch_no_wal(self.batch)
        } else {
            self.store.write_batch(self.batch)
        }
    }

    fn put_cf<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, cf: &ColumnFamily, key: K, value: V) {
        let key_ref = key.as_ref();
        let value_ref = value.as_ref();
        if self.store.is_append_only_store() {
            let cf_name = self
                .store
                .append_cf_name_for_handle(cf)
                .unwrap_or_else(|e| {
                    panic!(
                        "failed to resolve append-only CF in StoreBatch::put_cf: {}",
                        e
                    )
                });
            self.append_ops.push(AppendBatchOp {
                cf_name,
                key: key_ref.to_vec(),
                value: Some(value_ref.to_vec()),
            });
        } else {
            self.batch.put_cf(cf, key_ref, value_ref);
        }
    }

    /// Write a raw value into a named column family. Intended for bulk-build
    /// materialization paths that already know the encoded key/value bytes.
    pub fn put_raw_cf_by_name(
        &mut self,
        cf_name: &str,
        key: &[u8],
        value: &[u8],
    ) -> anyhow::Result<()> {
        if !self.store.has_cf(cf_name) {
            anyhow::bail!("CF '{}' is not available in this store batch", cf_name);
        }
        let cf = self.store.cf(cf_name);
        self.put_cf(cf, key, value);
        Ok(())
    }

    fn delete_cf<K: AsRef<[u8]>>(&mut self, cf: &ColumnFamily, key: K) {
        let key_ref = key.as_ref();
        if self.store.is_append_only_store() {
            let cf_name = self
                .store
                .append_cf_name_for_handle(cf)
                .unwrap_or_else(|e| {
                    panic!(
                        "failed to resolve append-only CF in StoreBatch::delete_cf: {}",
                        e
                    )
                });
            self.append_ops.push(AppendBatchOp {
                cf_name,
                key: key_ref.to_vec(),
                value: None,
            });
        } else {
            self.batch.delete_cf(cf, key_ref);
        }
    }

    // ---- Live cells ----

    /// Insert cell payload + live marker in one batch. Only valid for TestUnified stores
    /// (domain + append-only CFs coexist). Production code should use separate batches:
    /// `put_cell_payload` on append-only batch + `put_live_cell_marker` on domain batch.
    pub fn put_cell(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        info: &LiveCellInfo,
        created_at_block: i64,
    ) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.put_cell_raw_key(&key, info, created_at_block);
    }

    /// Insert cell payload + live marker using pre-encoded outpoint key. Only valid for
    /// TestUnified stores (domain + append-only CFs coexist). Production code should use
    /// separate batches: `put_cell_payload` on append-only batch +
    /// `put_live_cell_marker` on domain batch.
    pub fn put_cell_raw_key(&mut self, raw_key: &[u8], info: &LiveCellInfo, created_at_block: i64) {
        let value = bincode::serialize(info).expect("serialize LiveCellInfo");
        // Canonical cell payload is append-only in `cells`; live_cells is a marker set.
        self.put_cf(self.store.cf_cells(), raw_key, &value);
        self.put_live_cell_marker(raw_key, created_at_block);
    }

    // ---- Split cell methods for cross-store writes ----

    /// Write cell payload to CF_CELLS. Call on append-only store batch.
    pub fn put_cell_payload(&mut self, raw_key: &[u8], info: &LiveCellInfo) {
        let value = bincode::serialize(info).expect("serialize LiveCellInfo");
        self.put_cf(self.store.cf_cells(), raw_key, &value);
    }

    /// Write cell payload using tx_hash + output_index. Call on append-only store batch.
    pub fn put_cell_payload_by_outpoint(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        info: &LiveCellInfo,
    ) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.put_cell_payload(&key, info);
    }

    /// Write live cell marker to CF_LIVE_CELLS. Call on domain store batch.
    pub fn put_live_cell_marker(&mut self, raw_key: &[u8], created_at_block: i64) {
        self.put_cf(
            self.store.cf_live_cells(),
            raw_key,
            encode_live_cell_marker(created_at_block),
        );
    }

    /// Write live cell marker using tx_hash + output_index. Call on domain store batch.
    pub fn put_live_cell_marker_by_outpoint(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        created_at_block: i64,
    ) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.put_live_cell_marker(&key, created_at_block);
    }

    pub fn delete_cell(&mut self, tx_hash: &[u8], output_index: i16) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.delete_cell_raw_key(&key);
    }

    /// Delete a live cell marker using pre-encoded outpoint key.
    pub fn delete_cell_raw_key(&mut self, raw_key: &[u8]) {
        self.delete_cf(self.store.cf_live_cells(), raw_key);
    }

    /// Write consumed cell (payload + metadata) in one batch. Only valid for TestUnified
    /// stores (domain + append-only CFs coexist). Production code should use separate batches:
    /// `put_cell_payload` on append-only batch + `put_consumed_cell_meta` on domain batch.
    pub fn put_consumed_cell(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        info: &LiveCellInfo,
        created_at_block: i64,
        consumed_at_block: i64,
    ) {
        self.put_consumed_cell_with_consumer(
            tx_hash,
            output_index,
            info,
            created_at_block,
            consumed_at_block,
            None,
        );
    }

    /// Write consumed cell with consumer tx (payload + metadata) in one batch. Only valid for
    /// TestUnified stores (domain + append-only CFs coexist). Production code should use
    /// separate batches: `put_cell_payload` on append-only batch +
    /// `put_consumed_cell_meta` on domain batch.
    pub fn put_consumed_cell_with_consumer(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        info: &LiveCellInfo,
        created_at_block: i64,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
    ) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.put_consumed_cell_with_consumer_raw_key(
            &key,
            info,
            created_at_block,
            consumed_at_block,
            consumed_by_tx,
        );
    }

    /// Mark a cell as consumed using pre-encoded outpoint key. Only valid for TestUnified
    /// stores (domain + append-only CFs coexist). Production code should use separate batches:
    /// `put_cell_payload` on append-only batch + `put_consumed_cell_meta` on domain batch.
    pub fn put_consumed_cell_with_consumer_raw_key(
        &mut self,
        raw_key: &[u8],
        info: &LiveCellInfo,
        created_at_block: i64,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
    ) {
        // Ensure canonical payload exists even when callers only write consumed entries.
        let cell_value = bincode::serialize(info).expect("serialize LiveCellInfo");
        self.put_cf(self.store.cf_cells(), raw_key, &cell_value);
        let consumed = ConsumedCellMeta {
            created_at_block,
            consumed_at_block,
            consumed_by_tx: consumed_by_tx.map(|tx| tx.to_vec()),
        };
        let value = bincode::serialize(&consumed).expect("serialize ConsumedCellMeta");
        self.put_cf(self.store.cf_consumed_cells(), raw_key, &value);
    }

    /// Write consumed cell metadata to CF_CONSUMED_CELLS. Call on domain store batch.
    /// Does NOT write the cell payload — caller must separately write `put_cell_payload`
    /// on the append-only store batch.
    pub fn put_consumed_cell_meta(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        created_at_block: i64,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
    ) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.put_consumed_cell_meta_raw_key(
            &key,
            created_at_block,
            consumed_at_block,
            consumed_by_tx,
        );
    }

    /// Write consumed cell metadata using pre-encoded outpoint key to CF_CONSUMED_CELLS.
    /// Call on domain store batch.
    pub fn put_consumed_cell_meta_raw_key(
        &mut self,
        raw_key: &[u8],
        created_at_block: i64,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
    ) {
        let consumed = ConsumedCellMeta {
            created_at_block,
            consumed_at_block,
            consumed_by_tx: consumed_by_tx.map(|tx| tx.to_vec()),
        };
        let value = bincode::serialize(&consumed).expect("serialize ConsumedCellMeta");
        self.put_cf(self.store.cf_consumed_cells(), raw_key, &value);
    }

    // ---- Lock script mapping ----

    /// Write lock_hash -> script components mapping. Idempotent (same key always maps to same value).
    pub fn put_lock_script(&mut self, lock_hash: &[u8], entry: &crate::types::LockScriptEntry) {
        let value = bincode::serialize(entry).expect("serialize LockScriptEntry");
        self.put_cf(self.store.cf_lock_scripts(), lock_hash, &value);
    }

    // ---- Cell indexes ----

    pub fn put_cell_by_lock(
        &mut self,
        lock_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_hash, block_num, tx_hash, output_index);
        self.put_cf(self.store.cf_cell_by_lock(), key, []);
    }

    pub fn delete_cell_by_lock(
        &mut self,
        lock_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_hash, block_num, tx_hash, output_index);
        self.delete_cf(self.store.cf_cell_by_lock(), &key);
    }

    pub fn put_cell_by_type(
        &mut self,
        type_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_hash, block_num, tx_hash, output_index);
        self.put_cf(self.store.cf_cell_by_type(), key, []);
    }

    pub fn delete_cell_by_type(
        &mut self,
        type_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_hash, block_num, tx_hash, output_index);
        self.delete_cf(self.store.cf_cell_by_type(), &key);
    }

    pub fn put_cell_by_lock_code(
        &mut self,
        lock_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_code_hash, block_num, tx_hash, output_index);
        self.put_cf(self.store.cf_cell_by_lock_code(), key, []);
    }

    pub fn delete_cell_by_lock_code(
        &mut self,
        lock_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_code_hash, block_num, tx_hash, output_index);
        self.delete_cf(self.store.cf_cell_by_lock_code(), &key);
    }

    pub fn put_cell_by_type_code(
        &mut self,
        type_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_code_hash, block_num, tx_hash, output_index);
        self.put_cf(self.store.cf_cell_by_type_code(), key, []);
    }

    pub fn delete_cell_by_type_code(
        &mut self,
        type_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_code_hash, block_num, tx_hash, output_index);
        self.delete_cf(self.store.cf_cell_by_type_code(), &key);
    }

    pub fn put_cell_by_data_hash(
        &mut self,
        data_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(data_hash, block_num, tx_hash, output_index);
        self.put_cf(self.store.cf_cell_by_data_hash(), key, []);
    }

    pub fn delete_cell_by_data_hash(
        &mut self,
        data_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(data_hash, block_num, tx_hash, output_index);
        self.delete_cf(self.store.cf_cell_by_data_hash(), key);
    }

    // ---- Cell index (raw pre-computed key) ----

    pub fn put_cell_by_lock_raw(&mut self, key: &[u8]) {
        self.put_cf(self.store.cf_cell_by_lock(), key, []);
    }

    pub fn delete_cell_by_lock_raw(&mut self, key: &[u8]) {
        self.delete_cf(self.store.cf_cell_by_lock(), key);
    }

    pub fn put_cell_by_type_raw(&mut self, key: &[u8]) {
        self.put_cf(self.store.cf_cell_by_type(), key, []);
    }

    pub fn delete_cell_by_type_raw(&mut self, key: &[u8]) {
        self.delete_cf(self.store.cf_cell_by_type(), key);
    }

    pub fn put_cell_by_lock_code_raw(&mut self, key: &[u8]) {
        self.put_cf(self.store.cf_cell_by_lock_code(), key, []);
    }

    pub fn delete_cell_by_lock_code_raw(&mut self, key: &[u8]) {
        self.delete_cf(self.store.cf_cell_by_lock_code(), key);
    }

    pub fn put_cell_by_type_code_raw(&mut self, key: &[u8]) {
        self.put_cf(self.store.cf_cell_by_type_code(), key, []);
    }

    pub fn delete_cell_by_type_code_raw(&mut self, key: &[u8]) {
        self.delete_cf(self.store.cf_cell_by_type_code(), key);
    }

    // ---- Block headers ----

    pub fn put_block_header(&mut self, block_number: i64, header: &CachedBlockHeader) {
        let key = keys::encode_block_num(block_number);
        let value = bincode::serialize(header).expect("serialize CachedBlockHeader");
        self.put_cf(self.store.cf_block_headers(), key, &value);

        // Also update hash -> number index
        self.put_cf(
            self.store.cf_block_hash_index(),
            &header.hash,
            block_number.to_le_bytes(),
        );
    }

    // ---- Transaction index ----

    pub fn put_tx_index(&mut self, block_num: i64, tx_idx: i32, entry: &TxIndexEntry) {
        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        let value = bincode::serialize(entry).expect("serialize TxIndexEntry");
        self.put_cf(self.store.cf_tx_index(), &key, &value);
    }

    pub fn put_tx_hash_map(&mut self, tx_hash: &[u8], block_num: i64, tx_idx: i32) {
        let value = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        self.put_cf(self.store.cf_tx_hash_map(), tx_hash, &value);
    }

    // ---- Address balance ----

    pub fn put_addr_balance(&mut self, lock_hash: &[u8], balance: &AddressBalance) {
        let value = bincode::serialize(balance).expect("serialize AddressBalance");
        self.put_cf(self.store.cf_addr_balance(), lock_hash, &value);
    }

    pub fn put_addr_tx(&mut self, lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8]) {
        let key = keys::encode_addr_tx_key(lock_hash, block_num, tx_idx, tx_hash);
        self.put_cf(self.store.cf_addr_txs(), &key, []);
    }

    pub fn put_reorg_undo_log_by_block(&mut self, block_num: i64, seq: u64, entry: &UndoLogEntry) {
        let key = keys::encode_reorg_undo_log_key(block_num, seq);
        let value = bincode::serialize(entry).expect("serialize UndoLogEntry");
        self.put_cf(self.store.cf_reorg_undo_log_by_block(), key, &value);
    }

    // ---- DAO ----

    fn delete_dao_secondary_indexes(&mut self, outpoint_key: &[u8], entry: &DaoDepositCacheEntry) {
        let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key);
        let by_lock_key = keys::encode_dao_by_lock_block_key(
            &entry.lock_script_hash,
            entry.deposit_block_number,
            outpoint_key,
        );
        let by_status_key = keys::encode_dao_by_status_block_key(
            entry.status,
            entry.deposit_block_number,
            outpoint_key,
        );

        self.delete_cf(self.store.cf_dao_by_block(), by_block_key);
        self.delete_cf(self.store.cf_dao_by_lock_block(), by_lock_key);
        self.delete_cf(self.store.cf_dao_by_status_block(), by_status_key);
    }

    fn put_dao_secondary_indexes(&mut self, outpoint_key: &[u8], entry: &DaoDepositCacheEntry) {
        let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key);
        let by_lock_key = keys::encode_dao_by_lock_block_key(
            &entry.lock_script_hash,
            entry.deposit_block_number,
            outpoint_key,
        );
        let by_status_key = keys::encode_dao_by_status_block_key(
            entry.status,
            entry.deposit_block_number,
            outpoint_key,
        );

        self.put_cf(self.store.cf_dao_by_block(), by_block_key, []);
        self.put_cf(self.store.cf_dao_by_lock_block(), by_lock_key, []);
        self.put_cf(self.store.cf_dao_by_status_block(), by_status_key, []);
    }

    pub fn put_dao_deposit(&mut self, outpoint_key: &[u8], entry: &DaoDepositCacheEntry) {
        let existing_entry = if let Some(entry) = self.pending_dao_deposits.get(outpoint_key) {
            Some(entry.clone())
        } else {
            self.store
                .get_cf(self.store.cf_dao_deposits(), outpoint_key)
                .unwrap_or_else(|e| {
                    panic!(
                        "failed to read existing dao_deposit before overwrite: outpoint=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        e
                    )
                })
                .map(|existing| {
                    bincode::deserialize(&existing).unwrap_or_else(|e| {
                        panic!(
                            "failed to deserialize existing dao_deposit before overwrite: outpoint=0x{}, error={}",
                            bytes_to_hex(outpoint_key),
                            e
                        )
                    })
                })
        };

        if let Some(existing_entry) = existing_entry {
            self.delete_dao_secondary_indexes(outpoint_key, &existing_entry);
        }

        let value = bincode::serialize(entry).expect("serialize DaoDepositCacheEntry");
        self.put_cf(self.store.cf_dao_deposits(), outpoint_key, &value);
        self.put_dao_secondary_indexes(outpoint_key, entry);
        self.pending_dao_deposits
            .insert(outpoint_key.to_vec(), entry.clone());
    }

    pub fn put_dao_by_withdraw_tx(
        &mut self,
        withdraw_tx_hash: &[u8],
        withdraw_output_index: i16,
        deposit_outpoint_key: &[u8],
    ) {
        let key = keys::encode_outpoint(withdraw_tx_hash, withdraw_output_index);
        self.put_cf(
            self.store.cf_dao_by_withdraw_tx(),
            key,
            deposit_outpoint_key,
        );
    }

    // ---- Tokens ----

    pub fn put_token(&mut self, type_hash: &[u8], info: &TokenInfo) {
        let value = bincode::serialize(info).expect("serialize TokenInfo");
        self.put_cf(self.store.cf_tokens(), type_hash, &value);
    }

    pub fn put_token_holder(&mut self, type_hash: &[u8], lock_hash: &[u8], balance: i128) {
        let key = keys::encode_token_holder_key(type_hash, lock_hash);
        self.put_cf(self.store.cf_token_holders(), key, balance.to_le_bytes());
    }

    pub fn put_token_holder_by_balance(
        &mut self,
        type_hash: &[u8],
        lock_hash: &[u8],
        balance: i128,
    ) {
        assert!(
            balance > 0,
            "put_token_holder_by_balance expects positive balance, got {}",
            balance
        );
        let key = keys::encode_token_holder_balance_key(type_hash, balance, lock_hash);
        self.put_cf(self.store.cf_token_holders_by_balance(), key, []);
    }

    pub fn put_addr_token_by_balance(&mut self, lock_hash: &[u8], type_hash: &[u8], balance: i128) {
        assert!(
            balance > 0,
            "put_addr_token_by_balance expects positive balance, got {}",
            balance
        );
        let key = keys::encode_addr_token_balance_key(lock_hash, balance, type_hash);
        self.put_cf(self.store.cf_addr_tokens_by_balance(), key, []);
    }

    pub fn put_token_transfers_count(&mut self, type_hash: &[u8], count: i64) {
        let key = keys::encode_token_transfers_key(type_hash);
        self.put_cf(self.store.cf_stats_token(), key, count.to_le_bytes());
    }

    pub fn put_token_hourly_transfer(&mut self, type_hash: &[u8], hour_bucket: i64, count: i64) {
        let key = keys::encode_token_hourly_key(type_hash, hour_bucket);
        self.put_cf(self.store.cf_stats_token(), key, count.to_le_bytes());
    }

    pub fn put_spore_hourly_transfer(&mut self, cluster_id: &[u8], hour_bucket: i64, count: i64) {
        let key = keys::encode_spore_hourly_key(cluster_id, hour_bucket);
        self.put_cf(self.store.cf_stats_spore(), key, count.to_le_bytes());
    }

    pub fn put_mnft_hourly_transfer(&mut self, collection_id: &[u8], hour_bucket: i64, count: i64) {
        let key = keys::encode_nft_hourly_key(collection_id, hour_bucket);
        self.put_cf(self.store.cf_stats_mnft(), key, count.to_le_bytes());
    }

    pub fn put_mnft_daily_delta(
        &mut self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
        delta: &MnftDailyDelta,
    ) {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        let value = bincode::serialize(delta).expect("serialize MnftDailyDelta");
        self.put_cf(self.store.cf_stats_mnft(), key, &value);
    }

    pub fn put_mnft_type_index(&mut self, type_script_hash: &[u8], index: &MnftTypeIndex) {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        let value = bincode::serialize(index).expect("serialize MnftTypeIndex");
        self.put_cf(self.store.cf_stats_mnft(), key, &value);
    }

    pub fn put_cluster_daily_delta(
        &mut self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
        delta: &ClusterDailyDelta,
    ) {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        let value = bincode::serialize(delta).expect("serialize ClusterDailyDelta");
        self.put_cf(self.store.cf_stats_spore(), key, &value);
    }

    pub fn put_spore_daily_delta(
        &mut self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
        delta: &SporeDailyDelta,
    ) {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        let value = bincode::serialize(delta).expect("serialize SporeDailyDelta");
        self.put_cf(self.store.cf_stats_spore(), key, &value);
    }

    pub fn put_spore_type_index(&mut self, type_script_hash: &[u8], index: &SporeTypeIndex) {
        let key = keys::encode_spore_type_index_key(type_script_hash);
        let value = bincode::serialize(index).expect("serialize SporeTypeIndex");
        self.put_cf(self.store.cf_stats_spore(), key, &value);
    }

    pub fn put_spore_outpoint(&mut self, tx_hash: &[u8], output_index: i16, spore_id: &[u8]) {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        self.put_cf(self.store.cf_stats_spore(), key, spore_id);
        // Reverse index: spore_id → outpoints
        let rev_key = keys::encode_spore_outpoint_by_id_key(spore_id, tx_hash, output_index);
        self.put_cf(self.store.cf_stats_spore(), rev_key, &[] as &[u8]);
    }

    pub fn put_mnft_class_outpoint(&mut self, tx_hash: &[u8], output_index: i16, class_id: &[u8]) {
        let key = keys::encode_mnft_class_outpoint_key(tx_hash, output_index);
        self.put_cf(self.store.cf_stats_mnft(), key, class_id);
    }

    pub fn put_mnft_token_outpoint(&mut self, tx_hash: &[u8], output_index: i16, token_id: &[u8]) {
        let key = keys::encode_mnft_token_outpoint_key(tx_hash, output_index);
        self.put_cf(self.store.cf_stats_mnft(), key, token_id);
    }

    pub fn put_dotbit_account_outpoint(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        account_id: &[u8],
    ) {
        let key = keys::encode_dotbit_account_outpoint_key(tx_hash, output_index);
        self.put_cf(self.store.cf_stats_mnft(), key, account_id);
    }

    pub fn put_dotbit_outpoint_by_account_id(
        &mut self,
        account_id: &[u8],
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_dotbit_outpoint_by_account_id_key(account_id, tx_hash, output_index);
        self.put_cf(self.store.cf_stats_mnft(), key, []);
    }

    pub fn delete_token_holder(&mut self, type_hash: &[u8], lock_hash: &[u8]) {
        let key = keys::encode_token_holder_key(type_hash, lock_hash);
        self.delete_cf(self.store.cf_token_holders(), key);
    }

    pub fn delete_token_holder_by_balance(
        &mut self,
        type_hash: &[u8],
        lock_hash: &[u8],
        balance: i128,
    ) {
        let key = keys::encode_token_holder_balance_key(type_hash, balance, lock_hash);
        self.delete_cf(self.store.cf_token_holders_by_balance(), key);
    }

    pub fn delete_addr_token_by_balance(
        &mut self,
        lock_hash: &[u8],
        type_hash: &[u8],
        balance: i128,
    ) {
        let key = keys::encode_addr_token_balance_key(lock_hash, balance, type_hash);
        self.delete_cf(self.store.cf_addr_tokens_by_balance(), key);
    }

    pub fn put_token_transfer(
        &mut self,
        type_hash: &[u8],
        block_num: i64,
        tx_idx: i32,
        record: &TokenTransferRecord,
    ) {
        let key = keys::encode_token_transfer_key(type_hash, block_num, tx_idx);
        let value = bincode::serialize(record).expect("serialize TokenTransferRecord");
        self.put_cf(self.store.cf_token_transfers(), key, &value);
    }

    // ---- Spore/Object ----

    pub fn put_spore(&mut self, id: &[u8], entry: &ObjectEntry) {
        let value = bincode::serialize(entry).expect("serialize ObjectEntry");
        self.put_cf(self.store.cf_spore_data(), id, &value);
    }

    pub fn put_spore_by_cluster(&mut self, cluster_id: &[u8], spore_id: &[u8]) {
        let key = keys::encode_spore_by_cluster_key(cluster_id, spore_id);
        self.put_cf(self.store.cf_spore_by_cluster(), key, []);
    }

    pub fn delete_spore_by_cluster(&mut self, cluster_id: &[u8], spore_id: &[u8]) {
        let key = keys::encode_spore_by_cluster_key(cluster_id, spore_id);
        self.delete_cf(self.store.cf_spore_by_cluster(), key);
    }

    pub fn put_mnft(&mut self, id: &[u8], entry: &ObjectEntry) {
        let value = bincode::serialize(entry).expect("serialize ObjectEntry");
        self.put_cf(self.store.cf_mnft_data(), id, &value);
    }

    pub fn put_mnft_by_collection(&mut self, collection_id: &[u8], object_id: &[u8]) {
        let key = keys::encode_nft_by_collection_key(collection_id, object_id);
        self.put_cf(self.store.cf_mnft_by_collection(), key, []);
    }

    // ---- DOB decoded cache ----

    pub fn put_dob_decoded(&mut self, spore_id: &[u8], entry: &crate::types::DobDecodedEntry) {
        let value = bincode::serialize(entry).expect("serialize DobDecodedEntry");
        self.put_cf(self.store.cf_dob_decoded(), spore_id, &value);
    }

    pub fn delete_dob_decoded(&mut self, spore_id: &[u8]) {
        self.delete_cf(self.store.cf_dob_decoded(), spore_id);
    }

    // ---- Identity ----

    pub fn put_identity(&mut self, id: &[u8], entry: &IdentityEntry) {
        let value = bincode::serialize(entry).expect("serialize IdentityEntry");
        self.put_cf(self.store.cf_identity_data(), id, &value);
    }

    pub fn put_identity_by_collection(&mut self, collection_id: &[u8], identity_id: &[u8]) {
        let key = keys::encode_identity_by_collection_key(collection_id, identity_id);
        self.put_cf(self.store.cf_identity_by_collection(), key, []);
    }

    pub fn put_identity_owner_count(&mut self, collection_id: &[u8], lock_hash: &[u8], count: i64) {
        let key = keys::encode_identity_owner_key(collection_id, lock_hash);
        self.put_cf(self.store.cf_stats_identity(), key, count.to_le_bytes());
    }

    pub fn delete_identity_owner(&mut self, collection_id: &[u8], lock_hash: &[u8]) {
        let key = keys::encode_identity_owner_key(collection_id, lock_hash);
        self.delete_cf(self.store.cf_stats_identity(), key);
    }

    // ---- Cluster aggregates ----

    pub fn put_cluster_aggregate(&mut self, cluster_id: &[u8], agg: &ClusterAggregate) {
        let value = bincode::serialize(agg).expect("serialize ClusterAggregate");
        self.put_cf(self.store.cf_cluster_agg(), cluster_id, &value);
    }

    pub fn put_cluster_owner_count(&mut self, cluster_id: &[u8], lock_hash: &[u8], count: i64) {
        let key = keys::encode_cluster_owner_key(cluster_id, lock_hash);
        self.put_cf(self.store.cf_stats_spore(), key, count.to_le_bytes());
    }

    pub fn delete_cluster_owner(&mut self, cluster_id: &[u8], lock_hash: &[u8]) {
        let key = keys::encode_cluster_owner_key(cluster_id, lock_hash);
        self.delete_cf(self.store.cf_stats_spore(), key);
    }

    // ---- mNFT collection aggregates ----

    pub fn put_mnft_collection_aggregate(
        &mut self,
        collection_id: &[u8],
        agg: &MnftCollectionAggregate,
    ) {
        let value = bincode::serialize(agg).expect("serialize MnftCollectionAggregate");
        self.put_cf(self.store.cf_mnft_collection_agg(), collection_id, &value);
    }

    pub fn put_mnft_collection_owner_count(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        count: i64,
    ) {
        let key = keys::encode_nft_collection_owner_key(collection_id, lock_hash);
        self.put_cf(self.store.cf_stats_mnft(), key, count.to_le_bytes());
    }

    pub fn delete_mnft_collection_owner(&mut self, collection_id: &[u8], lock_hash: &[u8]) {
        let key = keys::encode_nft_collection_owner_key(collection_id, lock_hash);
        self.delete_cf(self.store.cf_stats_mnft(), key);
    }

    // ---- Identity collection aggregates ----

    pub fn put_identity_collection_aggregate(
        &mut self,
        collection_id: &[u8],
        agg: &IdentityCollectionAggregate,
    ) {
        let value = bincode::serialize(agg).expect("serialize IdentityCollectionAggregate");
        self.put_cf(self.store.cf_identity_agg(), collection_id, &value);
    }

    // ---- Activities ----

    pub fn put_tx_actions(&mut self, actions: &TxActions) {
        let key =
            keys::encode_tx_actions_key(actions.block_number, actions.tx_index, &actions.tx_hash);
        let value = bincode::serialize(actions).expect("serialize TxActions");
        self.put_cf(self.store.cf_tx_actions(), key, &value);
    }

    // ---- Object collection activities ----

    pub fn put_object_collection_activity(
        &mut self,
        collection_id: &[u8],
        block_num: i64,
        tx_idx: i32,
        entry: &ObjectCollectionActivityEntry,
    ) {
        let key = keys::encode_nft_collection_activity_key(
            collection_id,
            block_num,
            tx_idx,
            &entry.block_hash,
            &entry.tx_hash,
        );
        let value = bincode::serialize(entry).expect("serialize ObjectCollectionActivityEntry");
        self.put_cf(self.store.cf_object_collection_activities(), key, &value);
    }

    // ---- Identity collection activities ----

    pub fn put_identity_collection_activity(
        &mut self,
        collection_id: &[u8],
        block_num: i64,
        tx_idx: i32,
        entry: &ObjectCollectionActivityEntry,
    ) {
        let key = keys::encode_nft_collection_activity_key(
            collection_id,
            block_num,
            tx_idx,
            &entry.block_hash,
            &entry.tx_hash,
        );
        let value = bincode::serialize(entry).expect("serialize identity collection activity");
        self.put_cf(self.store.cf_identity_collection_activities(), key, &value);
    }

    // ---- Statistics ----

    pub fn put_stats(&mut self, key: &[u8], value: &[u8]) {
        let cf = self.store.cf_for_stats_key(key).unwrap_or_else(|e| {
            let prefix = key.first().copied().unwrap_or(0xFF);
            panic!(
                "failed to resolve stats CF: prefix=0x{prefix:02x}, key_len={}, error={}",
                key.len(),
                e
            )
        });
        self.put_cf(cf, key, value);
    }

    pub fn delete_stats(&mut self, key: &[u8]) {
        let cf = self.store.cf_for_stats_key(key).unwrap_or_else(|e| {
            let prefix = key.first().copied().unwrap_or(0xFF);
            panic!(
                "failed to resolve stats CF: prefix=0x{prefix:02x}, key_len={}, error={}",
                key.len(),
                e
            )
        });
        self.delete_cf(cf, key);
    }

    pub fn put_script_info(&mut self, code_hash: &[u8], info: &ScriptInfo) {
        let value = bincode::serialize(info).expect("serialize ScriptInfo");
        self.put_cf(self.store.cf_script_info(), code_hash, &value);
    }

    pub fn put_script_version(&mut self, version_hash: &[u8], info: &ScriptVersionInfo) {
        let value = bincode::serialize(info).expect("serialize ScriptVersionInfo");
        self.put_cf(self.store.cf_script_versions(), version_hash, &value);
    }

    pub fn put_script_version_by_label(&mut self, label_key: &str, version_hash: &[u8]) {
        let key = keys::encode_script_version_by_label_key(label_key, version_hash);
        self.put_cf(self.store.cf_script_versions_by_label(), key, []);
    }

    pub fn delete_script_version_by_label(&mut self, label_key: &str, version_hash: &[u8]) {
        let key = keys::encode_script_version_by_label_key(label_key, version_hash);
        self.delete_cf(self.store.cf_script_versions_by_label(), key);
    }

    pub fn put_script_family(&mut self, family_id: &str, info: &ScriptFamilyInfo) {
        assert!(
            info.family_id == family_id,
            "put_script_family batch family_id mismatch: key={}, value={}",
            family_id,
            info.family_id
        );
        let value = bincode::serialize(info).expect("serialize ScriptFamilyInfo");
        self.put_cf(
            self.store.cf_script_families(),
            family_id.as_bytes(),
            &value,
        );
    }

    pub fn put_script_version_by_family(&mut self, family_id: &str, version_hash: &[u8]) {
        let key = keys::encode_script_version_by_family_key(family_id, version_hash);
        self.put_cf(self.store.cf_script_versions_by_family(), key, []);
    }

    pub fn delete_script_version_by_family(&mut self, family_id: &str, version_hash: &[u8]) {
        let key = keys::encode_script_version_by_family_key(family_id, version_hash);
        self.delete_cf(self.store.cf_script_versions_by_family(), key);
    }

    pub fn put_script_reference_info(
        &mut self,
        hash_type: u8,
        reference_hash: &[u8],
        info: &ScriptReferenceInfo,
    ) {
        assert!(
            info.hash_type == hash_type,
            "put_script_reference_info batch hash_type mismatch: key={}, value={}",
            hash_type,
            info.hash_type
        );
        assert!(
            info.reference_hash.as_slice() == reference_hash,
            "put_script_reference_info batch reference_hash mismatch: key=0x{}, value=0x{}",
            crate::bytes_to_hex(reference_hash),
            crate::bytes_to_hex(&info.reference_hash)
        );
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        let value = bincode::serialize(info).expect("serialize ScriptReferenceInfo");
        self.put_cf(self.store.cf_script_reference_info(), key, &value);
    }

    pub fn delete_script_reference_info(&mut self, hash_type: u8, reference_hash: &[u8]) {
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        self.delete_cf(self.store.cf_script_reference_info(), key);
    }

    pub fn put_script_reference_to_version(
        &mut self,
        hash_type: u8,
        reference_hash: &[u8],
        version_hash: &[u8],
    ) {
        assert!(
            version_hash.len() == 32,
            "put_script_reference_to_version expects 32-byte version_hash, got {}",
            version_hash.len()
        );
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        self.put_cf(
            self.store.cf_script_reference_to_version(),
            key,
            version_hash,
        );
    }

    pub fn delete_script_reference_to_version(&mut self, hash_type: u8, reference_hash: &[u8]) {
        let key = keys::encode_script_reference_key(hash_type, reference_hash);
        self.delete_cf(self.store.cf_script_reference_to_version(), key);
    }

    pub fn put_script_family_by_name(&mut self, family_name: &str, family_id: &str) {
        self.put_cf(
            self.store.cf_script_family_by_name(),
            family_name.as_bytes(),
            family_id.as_bytes(),
        );
    }

    pub fn delete_script_family_by_name(&mut self, family_name: &str) {
        self.delete_cf(
            self.store.cf_script_family_by_name(),
            family_name.as_bytes(),
        );
    }

    // ---- Fiber Channels ----

    pub fn put_fiber_channel(&mut self, channel_id: &[u8], channel: &FiberChannel) {
        let value = bincode::serialize(channel).expect("serialize FiberChannel");
        self.put_cf(self.store.cf_fiber_channels(), channel_id, &value);
    }

    pub fn put_fiber_channel_by_commitment(&mut self, commitment_hash: &[u8], channel_id: &[u8]) {
        self.put_cf(
            self.store.cf_fiber_channel_by_commitment(),
            commitment_hash,
            channel_id,
        );
    }

    pub fn put_addr_fiber_channel(&mut self, lock_hash: &[u8], channel_id: &[u8]) {
        let key = keys::encode_addr_fiber_channel_key(lock_hash, channel_id);
        self.put_cf(self.store.cf_addr_fiber_channels(), key, []);
    }

    pub fn delete_fiber_channel(&mut self, channel_id: &[u8]) {
        self.delete_cf(self.store.cf_fiber_channels(), channel_id);
    }

    pub fn put_fiber_channel_by_funding_args(
        &mut self,
        funding_lock_args: &[u8],
        channel_id: &[u8],
    ) {
        self.put_cf(
            self.store.cf_fiber_channel_by_funding_args(),
            funding_lock_args,
            channel_id,
        );
    }

    pub fn delete_fiber_channel_by_commitment(&mut self, commitment_hash: &[u8]) {
        self.delete_cf(self.store.cf_fiber_channel_by_commitment(), commitment_hash);
    }

    pub fn delete_fiber_channel_by_funding_args(&mut self, funding_lock_args: &[u8]) {
        self.delete_cf(
            self.store.cf_fiber_channel_by_funding_args(),
            funding_lock_args,
        );
    }

    pub fn delete_addr_fiber_channel(&mut self, lock_hash: &[u8], channel_id: &[u8]) {
        let key = keys::encode_addr_fiber_channel_key(lock_hash, channel_id);
        self.delete_cf(self.store.cf_addr_fiber_channels(), &key);
    }

    // ---- Tracker batch methods ----

    pub fn put_hodl_wave(&mut self, date: &str, wave: &DailyHodlWave) {
        let key = keys::encode_stats_key(keys::stats_prefix::HODL_WAVE, date.as_bytes());
        let value = bincode::serialize(wave).expect("failed to serialize hodl wave");
        self.put_cf(self.store.cf_stats_hodl(), &key, &value);
    }

    pub fn put_hodl_tracker_state(&mut self, state: &HodlTrackerState) {
        let value = bincode::serialize(state).expect("failed to serialize hodl tracker state");
        self.put_cf(
            self.store.cf_sync_meta(),
            keys::sync_meta_keys::HODL_TRACKER,
            &value,
        );
    }

    pub fn put_cell_distribution(&mut self, date: &str, snapshot: &DailyCellDistribution) {
        let key = keys::encode_stats_key(keys::stats_prefix::CELL_DISTRIBUTION, date.as_bytes());
        let value = bincode::serialize(snapshot).expect("failed to serialize cell distribution");
        self.put_cf(self.store.cf_stats_hodl(), &key, &value);
    }

    pub fn put_address_cohort(&mut self, date: &str, snapshot: &DailyAddressCohort) {
        let key = keys::encode_stats_key(keys::stats_prefix::ADDR_COHORT, date.as_bytes());
        let value = bincode::serialize(snapshot).expect("failed to serialize address cohort");
        self.put_cf(self.store.cf_stats_hodl(), &key, &value);
    }

    pub fn put_cell_dist_tracker_state(&mut self, state: &CellDistributionTrackerState) {
        let value = bincode::serialize(state).expect("failed to serialize cell dist tracker state");
        self.put_cf(
            self.store.cf_sync_meta(),
            keys::sync_meta_keys::CELL_DIST_TRACKER,
            &value,
        );
    }

    // ---- Sync meta ----

    pub fn put_sync_meta(&mut self, key: &[u8], value: &[u8]) {
        self.put_cf(self.store.cf_sync_meta(), key, value);
    }

    pub fn delete_sync_meta(&mut self, key: &[u8]) {
        self.delete_cf(self.store.cf_sync_meta(), key);
    }
}

/// Merge `source` WriteBatch into `target` using byte-level concatenation.
///
/// Uses the RocksDB WriteBatch wire format:
///   [0..8]  sequence number (u64 LE) — target's preserved
///   [8..12] operation count (u32 LE) — summed
///   [12..]  serialized operations  — concatenated
///
/// This is the same technique as `StoreBatch::merge_from` but operates on
/// raw `WriteBatch` without requiring a store reference.
pub fn merge_write_batches(target: &mut WriteBatch, source: WriteBatch) {
    if source.is_empty() {
        return;
    }
    if target.is_empty() {
        *target = source;
        return;
    }
    let target_data = target.data();
    let source_data = source.data();

    let target_count = u32::from_le_bytes(
        target_data[8..12]
            .try_into()
            .expect("WriteBatch header must be at least 12 bytes"),
    );
    let source_count = u32::from_le_bytes(
        source_data[8..12]
            .try_into()
            .expect("WriteBatch header must be at least 12 bytes"),
    );
    let total_count = target_count
        .checked_add(source_count)
        .expect("WriteBatch merge: operation count overflow");

    let mut merged = Vec::with_capacity(target_data.len() + source_data.len() - 12);
    merged.extend_from_slice(&target_data[..8]);
    merged.extend_from_slice(&total_count.to_le_bytes());
    merged.extend_from_slice(&target_data[12..]);
    merged.extend_from_slice(&source_data[12..]);

    *target = WriteBatch::from_data(&merged);
}

/// Pack multiple WriteBatch fragments into one batch with a single linear pass.
///
/// Empty batches are ignored. Operation order is preserved exactly as provided.
/// The resulting batch preserves the header sequence bytes from the first
/// non-empty batch and recomputes the total entry count once.
pub fn pack_write_batches_in_order(batches: Vec<WriteBatch>) -> WriteBatch {
    let mut non_empty_batches = batches
        .into_iter()
        .filter(|batch| !batch.is_empty())
        .collect::<Vec<_>>();

    if non_empty_batches.is_empty() {
        return WriteBatch::default();
    }

    if non_empty_batches.len() == 1 {
        return non_empty_batches
            .pop()
            .expect("non-empty batch collection must contain one batch");
    }

    let first_data = non_empty_batches[0].data();
    let mut total_count = u32::from_le_bytes(
        first_data[8..12]
            .try_into()
            .expect("WriteBatch header must be at least 12 bytes"),
    );
    let mut total_len = first_data.len();

    for batch in non_empty_batches.iter().skip(1) {
        let data = batch.data();
        let batch_count = u32::from_le_bytes(
            data[8..12]
                .try_into()
                .expect("WriteBatch header must be at least 12 bytes"),
        );
        total_count = total_count
            .checked_add(batch_count)
            .expect("WriteBatch pack: operation count overflow");
        total_len = total_len
            .checked_add(data.len() - 12)
            .expect("WriteBatch pack: serialized byte length overflow");
    }

    let mut packed = Vec::with_capacity(total_len);
    packed.extend_from_slice(&first_data[..8]);
    packed.extend_from_slice(&total_count.to_le_bytes());
    packed.extend_from_slice(&first_data[12..]);

    for batch in non_empty_batches.iter().skip(1) {
        packed.extend_from_slice(&batch.data()[12..]);
    }

    WriteBatch::from_data(&packed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CF_CELLS;
    use tempfile::TempDir;

    #[test]
    fn test_batch_commit() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        let header = CachedBlockHeader {
            hash: vec![1u8; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1000,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0u8; 32],
            transactions_count: 5,
        };
        batch.put_block_header(0, &header);
        assert!(!batch.is_empty());
        batch.commit().unwrap();

        let cf = store.cf_block_headers();
        let key = keys::encode_block_num(0);
        let val = store.get_cf(cf, &key).unwrap();
        assert!(val.is_some());

        let decoded: CachedBlockHeader = bincode::deserialize(&val.unwrap()).unwrap();
        assert_eq!(decoded.timestamp, 1000);
        assert_eq!(decoded.transactions_count, 5);
    }

    #[test]
    fn test_batch_atomicity() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Write some data
        let mut batch = StoreBatch::new(&store);
        batch.put_sync_meta(b"key1", b"val1");
        batch.put_sync_meta(b"key2", b"val2");
        batch.commit().unwrap();

        // Verify both written
        let cf = store.cf_sync_meta();
        assert!(store.get_cf(cf, b"key1").unwrap().is_some());
        assert!(store.get_cf(cf, b"key2").unwrap().is_some());
    }

    #[test]
    fn test_cell_write_and_delete() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [42u8; 32];
        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &info, 1);
        batch.commit().unwrap();

        let key = keys::encode_outpoint(&tx_hash, 0);
        assert!(store.get_cf(store.cf_live_cells(), &key).unwrap().is_some());

        let mut batch = StoreBatch::new(&store);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        assert!(store.get_cf(store.cf_live_cells(), &key).unwrap().is_none());
    }

    #[test]
    fn test_consumed_cell_writes_metadata() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [0x33u8; 32];
        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell_with_consumer(&tx_hash, 0, &info, 1, 22, Some(&[0x44; 32]));
        batch.commit().unwrap();

        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        let meta = store
            .get_cf(store.cf_consumed_cells(), &outpoint_key)
            .unwrap()
            .unwrap();
        let decoded: ConsumedCellMeta = bincode::deserialize(&meta).unwrap();
        assert_eq!(decoded.created_at_block, 1);
        assert_eq!(decoded.consumed_at_block, 22);
        assert_eq!(decoded.consumed_by_tx, Some(vec![0x44; 32]));
    }

    #[test]
    fn test_reorg_undo_log_by_block_batch_write() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let entry = UndoLogEntry::KeyMutation {
            target_store: UndoLogStoreTarget::Domain,
            cf_name: crate::store::CF_SYNC_META.to_string(),
            key: b"k".to_vec(),
            previous_value: Some(b"v1".to_vec()),
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_reorg_undo_log_by_block(88, 3, &entry);
        batch.commit().unwrap();

        let key = keys::encode_reorg_undo_log_key(88, 3);
        let value = store
            .get_cf(store.cf_reorg_undo_log_by_block(), &key)
            .unwrap()
            .unwrap();
        let decoded: UndoLogEntry = bincode::deserialize(&value).unwrap();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn test_token_transfers_count_batch_write() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let type_hash = [0xAAu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfers_count(&type_hash, 123);
        batch.commit().unwrap();

        let key = keys::encode_token_transfers_key(&type_hash);
        let val = store.get_cf(store.cf_stats_token(), &key).unwrap().unwrap();
        assert_eq!(i64::from_le_bytes(val[..8].try_into().unwrap()), 123);
    }

    #[test]
    fn test_token_hourly_transfer_batch_write() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let type_hash = [0xBBu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&type_hash, 500_000, 7);
        batch.commit().unwrap();

        let key = keys::encode_token_hourly_key(&type_hash, 500_000);
        let val = store.get_cf(store.cf_stats_token(), &key).unwrap().unwrap();
        assert_eq!(i64::from_le_bytes(val[..8].try_into().unwrap()), 7);
    }

    #[test]
    fn test_put_addr_tx_stores_empty_value() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAD; 32];
        let tx_hash = [0xBE; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 0, &tx_hash);
        batch.commit().unwrap();

        let key = keys::encode_addr_tx_key(&lock, 100, 0, &tx_hash);
        let value = store.get_cf(store.cf_addr_txs(), &key).unwrap().unwrap();
        assert!(value.is_empty());
    }

    // Activity list tests moved to activity_ops.rs tests module.

    #[test]
    fn test_append_only_batch_rejects_duplicate_key_in_same_commit() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell_payload_by_outpoint(&[0xA1; 32], 0, &info);
        batch.put_cell_payload_by_outpoint(&[0xA1; 32], 0, &info);
        let err = batch.commit().unwrap_err();
        assert!(err
            .to_string()
            .contains("append-only batch duplicate key blocked"));
    }

    #[test]
    fn test_script_family_and_reference_delete_helpers_batch_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let family = ScriptFamilyInfo {
            family_id: "family/default-lock".to_string(),
            name: "Default Lock".to_string(),
            ..Default::default()
        };
        let reference = ScriptReferenceInfo {
            reference_hash: vec![0x33; 32],
            hash_type: 1,
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_script_family(&family.family_id, &family);
        batch.put_script_version_by_family(&family.family_id, &[0x11; 32]);
        batch.put_script_family_by_name(&family.name, &family.family_id);
        batch.put_script_reference_info(reference.hash_type, &reference.reference_hash, &reference);
        batch.put_script_reference_to_version(
            reference.hash_type,
            &reference.reference_hash,
            &[0x55; 32],
        );
        batch.commit().unwrap();

        let mut delete_batch = StoreBatch::new(&store);
        delete_batch.delete_script_version_by_family(&family.family_id, &[0x11; 32]);
        delete_batch.delete_script_family_by_name(&family.name);
        delete_batch.delete_script_reference_info(reference.hash_type, &reference.reference_hash);
        delete_batch
            .delete_script_reference_to_version(reference.hash_type, &reference.reference_hash);
        delete_batch.commit().unwrap();

        assert!(store
            .list_script_version_hashes_by_family(&family.family_id)
            .unwrap()
            .is_empty());
        assert!(store
            .get_script_family_id_by_name(&family.name)
            .unwrap()
            .is_none());
        assert!(store
            .get_script_reference_info(reference.hash_type, &reference.reference_hash)
            .unwrap()
            .is_none());
        assert!(store
            .get_script_reference_version_hash(reference.hash_type, &reference.reference_hash)
            .unwrap()
            .is_none());
    }

    #[test]
    #[should_panic(expected = "put_script_family batch family_id mismatch")]
    fn test_put_script_family_batch_rejects_mismatched_family_id() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let family = ScriptFamilyInfo {
            family_id: "family/actual".to_string(),
            name: "Default Lock".to_string(),
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_script_family("family/requested", &family);
    }

    #[test]
    #[should_panic(expected = "put_script_reference_info batch hash_type mismatch")]
    fn test_put_script_reference_info_batch_rejects_hash_type_mismatch() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference = ScriptReferenceInfo {
            reference_hash: vec![0x33; 32],
            hash_type: 2,
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_script_reference_info(1, &reference.reference_hash, &reference);
    }

    #[test]
    #[should_panic(expected = "put_script_reference_info batch reference_hash mismatch")]
    fn test_put_script_reference_info_batch_rejects_reference_hash_mismatch() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let reference = ScriptReferenceInfo {
            reference_hash: vec![0x33; 32],
            hash_type: 1,
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_script_reference_info(1, &[0x44; 32], &reference);
    }

    #[test]
    fn test_append_only_cell_rejects_overwrite() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell_payload_by_outpoint(&[0x11; 32], 0, &info);
        batch.commit().unwrap();

        // Overwrite with a DIFFERENT value should be rejected
        let mut different_info = info.clone();
        different_info.capacity = 99999;
        let mut overwrite_batch = StoreBatch::new(&store);
        overwrite_batch.put_cell_payload_by_outpoint(&[0x11; 32], 0, &different_info);
        let err = overwrite_batch.commit().unwrap_err();
        assert!(err.to_string().contains("append-only overwrite blocked"));
    }

    #[test]
    fn test_append_only_nft_collection_activity_preserves_competing_block_hash_history() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let collection_id = [0x12u8; 32];
        let tx_hash = vec![0x44; 32];

        let first = ObjectCollectionActivityEntry {
            tx_hash: tx_hash.clone(),
            block_hash: vec![0x31; 32],
            timestamp_ms: 1_700_000_000_100,
            actions: vec![AssetAction::Mint],
        };
        let second = ObjectCollectionActivityEntry {
            tx_hash: tx_hash.clone(),
            block_hash: vec![0x32; 32],
            timestamp_ms: 1_700_000_000_200,
            actions: vec![AssetAction::Transfer],
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_object_collection_activity(&collection_id, 100, 0, &first);
        batch.put_object_collection_activity(&collection_id, 100, 0, &second);
        batch.commit().unwrap();

        let rows = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|(_, _, entry)| entry.block_hash == vec![0x31; 32]));
        assert!(rows
            .iter()
            .any(|(_, _, entry)| entry.block_hash == vec![0x32; 32]));
    }

    #[test]
    fn test_append_only_batch_allows_idempotent_replay_existing_value() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut first_batch = StoreBatch::new(&store);
        first_batch.put_cell_payload_by_outpoint(&[0xA1; 32], 0, &info);
        first_batch.commit().unwrap();

        // Replay with identical value should succeed (idempotent)
        let mut replay_batch = StoreBatch::new(&store);
        replay_batch.put_cell_payload_by_outpoint(&[0xA1; 32], 0, &info);
        replay_batch.commit().unwrap();

        // Cell should still be readable
        let key = keys::encode_outpoint(&[0xA1; 32], 0);
        let val = store.get_cf(store.cf_cells(), &key).unwrap();
        assert!(val.is_some());
    }

    #[test]
    fn test_append_only_cell_payload_ignores_created_at_block_differences() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut first_batch = StoreBatch::new(&store);
        first_batch.put_cell_payload_by_outpoint(&[0xB1; 32], 0, &info);
        first_batch.commit().unwrap();

        let replay = info.clone();
        let mut replay_batch = StoreBatch::new(&store);
        replay_batch.put_cell_payload_by_outpoint(&[0xB1; 32], 0, &replay);
        replay_batch.commit().unwrap();

        let key = keys::encode_outpoint(&[0xB1; 32], 0);
        let stored = store.get_cf(store.cf_cells(), &key).unwrap().unwrap();
        let payload: LiveCellInfo = bincode::deserialize(&stored).unwrap();
        assert_eq!(payload, info);
    }

    #[test]
    fn test_put_cell_raw_key_produces_same_result() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [0x33u8; 32];
        let output_index: i16 = 2;
        let raw_key = keys::encode_outpoint(&tx_hash, output_index);

        let info = LiveCellInfo {
            capacity: 50000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: Some(vec![4u8; 32]),
            type_code_hash: Some(vec![5u8; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![6u8; 32]),
            data_size: 64,
            occupied_capacity: 10200000000,
            udt_amount: Some(999),
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell_raw_key(&raw_key, &info, 7);
        batch.commit().unwrap();

        // Verify readable via standard get_cell (which encodes the key internally)
        let cell = store.get_cell(&tx_hash, output_index, &store).unwrap();
        assert!(cell.is_some());
        let cell = cell.unwrap();
        assert_eq!(cell.capacity, 50000);
        assert_eq!(cell.created_at_block, 7);
        assert_eq!(cell.udt_amount, Some(999));
        assert_eq!(cell.data_size, 64);

        // Verify live marker exists
        assert!(store
            .get_cf(store.cf_live_cells(), &raw_key)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_put_cell_raw_key_matches_put_cell() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash_a = [0xAAu8; 32];
        let tx_hash_b = [0xBBu8; 32];
        let output_index: i16 = 1;

        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        // Write cell A via put_cell (original method)
        let mut batch_a = StoreBatch::new(&store);
        batch_a.put_cell(&tx_hash_a, output_index, &info, 3);
        batch_a.commit().unwrap();

        // Write cell B via put_cell_raw_key
        let raw_key_b = keys::encode_outpoint(&tx_hash_b, output_index);
        let mut batch_b = StoreBatch::new(&store);
        batch_b.put_cell_raw_key(&raw_key_b, &info, 3);
        batch_b.commit().unwrap();

        // Both should be readable and produce identical cell info
        let cell_a = store
            .get_cell(&tx_hash_a, output_index, &store)
            .unwrap()
            .unwrap();
        let cell_b = store
            .get_cell(&tx_hash_b, output_index, &store)
            .unwrap()
            .unwrap();
        assert_eq!(cell_a.capacity, cell_b.capacity);
        assert_eq!(cell_a.created_at_block, cell_b.created_at_block);
        assert_eq!(cell_a.lock_script_hash, cell_b.lock_script_hash);

        // Both should have live markers
        let key_a = keys::encode_outpoint(&tx_hash_a, output_index);
        assert!(store
            .get_cf(store.cf_live_cells(), &key_a)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_live_cells(), &raw_key_b)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_consume_cell_raw_key_matches_consume_cell() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [0xCCu8; 32];
        let output_index: i16 = 0;
        let consumed_by = [0xDDu8; 32];

        let info = LiveCellInfo {
            capacity: 20000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        // First, insert the cell as live
        let raw_key = keys::encode_outpoint(&tx_hash, output_index);
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_raw_key(&raw_key, &info, 5);
        batch.commit().unwrap();

        // Verify live marker exists
        assert!(store
            .get_cf(store.cf_live_cells(), &raw_key)
            .unwrap()
            .is_some());

        // Consume via raw-key methods
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell_with_consumer_raw_key(&raw_key, &info, 5, 10, Some(&consumed_by));
        batch.delete_cell_raw_key(&raw_key);
        batch.commit().unwrap();

        // Verify live marker is gone
        assert!(store
            .get_cf(store.cf_live_cells(), &raw_key)
            .unwrap()
            .is_none());

        // Verify consumed cell metadata is written correctly
        let meta_bytes = store
            .get_cf(store.cf_consumed_cells(), &raw_key)
            .unwrap()
            .unwrap();
        let meta: ConsumedCellMeta = bincode::deserialize(&meta_bytes).unwrap();
        assert_eq!(meta.created_at_block, 5);
        assert_eq!(meta.consumed_at_block, 10);
        assert_eq!(meta.consumed_by_tx, Some(consumed_by.to_vec()));

        // Verify canonical cell payload is still present in CF_CELLS
        let cell_bytes = store.get_cf(store.cf_cells(), &raw_key).unwrap().unwrap();
        let cell: LiveCellInfo = bincode::deserialize(&cell_bytes).unwrap();
        assert_eq!(cell.capacity, 20000);
        assert_eq!(cell.lock_script_hash, info.lock_script_hash);
    }

    #[test]
    fn test_delete_cell_raw_key_removes_live_marker() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [0xEEu8; 32];
        let output_index: i16 = 3;
        let raw_key = keys::encode_outpoint(&tx_hash, output_index);

        let info = LiveCellInfo {
            capacity: 30000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell_raw_key(&raw_key, &info, 1);
        batch.commit().unwrap();

        assert!(store
            .get_cf(store.cf_live_cells(), &raw_key)
            .unwrap()
            .is_some());

        let mut batch = StoreBatch::new(&store);
        batch.delete_cell_raw_key(&raw_key);
        batch.commit().unwrap();

        assert!(store
            .get_cf(store.cf_live_cells(), &raw_key)
            .unwrap()
            .is_none());
    }

    // ---- merge_from tests ----

    #[test]
    fn test_merge_domain_batches() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut a = StoreBatch::new(&store);
        let mut b = StoreBatch::new(&store);

        a.put_tx_hash_map(&[0x11; 32], 1, 0);
        b.put_tx_hash_map(&[0x22; 32], 2, 0);

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);

        a.merge_from(b);
        assert_eq!(a.len(), 2);

        // Commit and verify both writes landed
        a.commit().unwrap();
        assert!(store.get_tx_location(&[0x11; 32]).unwrap().is_some());
        assert!(store.get_tx_location(&[0x22; 32]).unwrap().is_some());

        let (block1, idx1) = store.get_tx_location(&[0x11; 32]).unwrap().unwrap();
        assert_eq!(block1, 1);
        assert_eq!(idx1, 0);

        let (block2, idx2) = store.get_tx_location(&[0x22; 32]).unwrap().unwrap();
        assert_eq!(block2, 2);
        assert_eq!(idx2, 0);
    }

    #[test]
    fn test_merge_into_empty_batch() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut a = StoreBatch::new(&store);
        let mut b = StoreBatch::new(&store);

        b.put_tx_hash_map(&[0x33; 32], 3, 1);
        assert_eq!(a.len(), 0);
        assert_eq!(b.len(), 1);

        a.merge_from(b);
        assert_eq!(a.len(), 1);

        a.commit().unwrap();
        let (block, idx) = store.get_tx_location(&[0x33; 32]).unwrap().unwrap();
        assert_eq!(block, 3);
        assert_eq!(idx, 1);
    }

    #[test]
    fn test_merge_empty_into_nonempty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut a = StoreBatch::new(&store);
        let b = StoreBatch::new(&store);

        a.put_tx_hash_map(&[0x44; 32], 4, 2);
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 0);

        a.merge_from(b);
        assert_eq!(a.len(), 1);

        a.commit().unwrap();
        let (block, idx) = store.get_tx_location(&[0x44; 32]).unwrap().unwrap();
        assert_eq!(block, 4);
        assert_eq!(idx, 2);
    }

    #[test]
    fn test_merge_multiple_batches() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut a = StoreBatch::new(&store);
        let mut b = StoreBatch::new(&store);
        let mut c = StoreBatch::new(&store);

        a.put_tx_hash_map(&[0x01; 32], 10, 0);
        b.put_tx_hash_map(&[0x02; 32], 20, 0);
        b.put_tx_hash_map(&[0x03; 32], 30, 0);
        c.put_tx_hash_map(&[0x04; 32], 40, 0);

        a.merge_from(b);
        a.merge_from(c);
        assert_eq!(a.len(), 4);

        a.commit().unwrap();
        for (hash_byte, expected_block) in [(0x01, 10), (0x02, 20), (0x03, 30), (0x04, 40)] {
            let (block, _) = store
                .get_tx_location(&[hash_byte; 32])
                .unwrap()
                .unwrap_or_else(|| panic!("tx 0x{hash_byte:02x} not found"));
            assert_eq!(block, expected_block);
        }
    }

    #[test]
    fn test_merge_cross_cf_operations() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut a = StoreBatch::new(&store);
        let mut b = StoreBatch::new(&store);

        // Batch a: block header
        let header = CachedBlockHeader {
            hash: vec![0xAA; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 9999,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0u8; 32],
            transactions_count: 2,
        };
        a.put_block_header(42, &header);

        // Batch b: tx hash map + sync meta
        b.put_tx_hash_map(&[0xBB; 32], 42, 0);
        b.put_sync_meta(b"tip", b"42");

        a.merge_from(b);
        a.commit().unwrap();

        // Verify block header
        let key = keys::encode_block_num(42);
        let val = store
            .get_cf(store.cf_block_headers(), &key)
            .unwrap()
            .unwrap();
        let decoded: CachedBlockHeader = bincode::deserialize(&val).unwrap();
        assert_eq!(decoded.timestamp, 9999);

        // Verify tx hash map
        let (block, idx) = store.get_tx_location(&[0xBB; 32]).unwrap().unwrap();
        assert_eq!(block, 42);
        assert_eq!(idx, 0);

        // Verify sync meta
        let val = store.get_cf(store.cf_sync_meta(), b"tip").unwrap().unwrap();
        assert_eq!(&val[..], b"42");
    }

    #[test]
    #[should_panic(expected = "both batches must reference the same CkbadgerStore")]
    fn test_merge_different_stores_panics() {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let store_a = CkbadgerStore::open_test_unified(dir_a.path()).unwrap();
        let store_b = CkbadgerStore::open_test_unified(dir_b.path()).unwrap();

        let mut a = StoreBatch::new(&store_a);
        let b = StoreBatch::new(&store_b);
        a.merge_from(b);
    }

    #[test]
    fn test_append_only_batch_len_no_double_counting() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let info = LiveCellInfo {
            capacity: 10000,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        assert_eq!(batch.len(), 0);
        assert!(batch.is_empty());

        batch.put_cell_payload_by_outpoint(&[0xA1; 32], 0, &info);
        assert_eq!(batch.len(), 1);
        assert!(!batch.is_empty());

        batch.put_cell_payload_by_outpoint(&[0xA2; 32], 0, &info);
        assert_eq!(batch.len(), 2);

        batch.put_cell_payload_by_outpoint(&[0xA3; 32], 0, &info);
        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn test_append_only_raw_cf_name_put_rejects_overwrite() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let mut first_batch = StoreBatch::new(&store);
        first_batch
            .put_raw_cf_by_name(CF_CELLS, b"k1", b"v1")
            .expect("first append-only put");
        first_batch.commit().unwrap();

        let mut overwrite_batch = StoreBatch::new(&store);
        overwrite_batch
            .put_raw_cf_by_name(CF_CELLS, b"k1", b"v2")
            .expect("overwrite append-only put");
        let err = overwrite_batch.commit().unwrap_err();
        assert!(err.to_string().contains("append-only overwrite blocked"));
    }

    #[test]
    fn merge_write_batches_concatenates_entries() {
        let mut a = rocksdb::WriteBatch::default();
        a.put(b"k1", b"v1");
        a.put(b"k2", b"v2");
        let mut b = rocksdb::WriteBatch::default();
        b.put(b"k3", b"v3");

        merge_write_batches(&mut a, b);
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn merge_write_batches_into_empty() {
        let mut a = rocksdb::WriteBatch::default();
        let mut b = rocksdb::WriteBatch::default();
        b.put(b"k1", b"v1");

        merge_write_batches(&mut a, b);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn merge_write_batches_from_empty_noop() {
        let mut a = rocksdb::WriteBatch::default();
        a.put(b"k1", b"v1");
        let b = rocksdb::WriteBatch::default();

        merge_write_batches(&mut a, b);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn pack_write_batches_in_order_preserves_entry_order() {
        let mut first = rocksdb::WriteBatch::default();
        first.put(b"k1", b"v1");

        let mut second = rocksdb::WriteBatch::default();
        second.delete(b"k2");

        let mut third = rocksdb::WriteBatch::default();
        third.put(b"k3", b"v3");

        let packed = pack_write_batches_in_order(vec![first, second, third]);

        let mut expected = rocksdb::WriteBatch::default();
        expected.put(b"k1", b"v1");
        expected.delete(b"k2");
        expected.put(b"k3", b"v3");

        assert_eq!(packed.len(), 3);
        assert_eq!(packed.data(), expected.data());
    }

    #[test]
    fn pack_write_batches_in_order_skips_empty_batches() {
        let empty = rocksdb::WriteBatch::default();

        let mut middle = rocksdb::WriteBatch::default();
        middle.put(b"k1", b"v1");

        let packed =
            pack_write_batches_in_order(vec![empty, middle, rocksdb::WriteBatch::default()]);

        let mut expected = rocksdb::WriteBatch::default();
        expected.put(b"k1", b"v1");

        assert_eq!(packed.len(), 1);
        assert_eq!(packed.data(), expected.data());
    }

    #[test]
    fn pack_write_batches_in_order_matches_left_fold_for_small_inputs() {
        let mut first = rocksdb::WriteBatch::default();
        first.put(b"k1", b"v1");

        let mut second = rocksdb::WriteBatch::default();
        second.put(b"k2", b"v2");

        let mut third = rocksdb::WriteBatch::default();
        third.delete(b"k3");

        let packed =
            pack_write_batches_in_order(vec![first, rocksdb::WriteBatch::default(), second, third]);

        let mut left_fold = rocksdb::WriteBatch::default();
        let mut batch_a = rocksdb::WriteBatch::default();
        batch_a.put(b"k1", b"v1");
        merge_write_batches(&mut left_fold, batch_a);
        merge_write_batches(&mut left_fold, rocksdb::WriteBatch::default());
        let mut batch_b = rocksdb::WriteBatch::default();
        batch_b.put(b"k2", b"v2");
        merge_write_batches(&mut left_fold, batch_b);
        let mut batch_c = rocksdb::WriteBatch::default();
        batch_c.delete(b"k3");
        merge_write_batches(&mut left_fold, batch_c);

        assert_eq!(packed.len(), left_fold.len());
        assert_eq!(packed.data(), left_fold.data());
    }
}
