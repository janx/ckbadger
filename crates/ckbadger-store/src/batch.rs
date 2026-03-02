//! WriteBatch builder for atomic multi-CF writes.

use rocksdb::WriteBatch;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::*;

/// Accumulates writes across all CFs and commits atomically.
pub struct StoreBatch<'a> {
    store: &'a CkbadgerStore,
    batch: WriteBatch,
}

impl<'a> StoreBatch<'a> {
    pub fn new(store: &'a CkbadgerStore) -> Self {
        Self {
            store,
            batch: WriteBatch::default(),
        }
    }

    /// Commit all accumulated writes atomically.
    pub fn commit(self) -> anyhow::Result<()> {
        self.store.write_batch(self.batch)
    }

    /// Commit with WAL disabled. Use during bulk sync where crash recovery
    /// re-syncs from the last committed block header.
    pub fn commit_no_wal(self) -> anyhow::Result<()> {
        self.store.write_batch_no_wal(self.batch)
    }

    /// Get the number of operations in the batch.
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Get the approximate size of the batch in bytes.
    pub fn size_in_bytes(&self) -> usize {
        self.batch.size_in_bytes()
    }

    // ---- Live cells ----

    pub fn put_cell(&mut self, tx_hash: &[u8], output_index: i16, info: &LiveCellInfo) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        let value = bincode::serialize(info).expect("serialize LiveCellInfo");
        // Canonical cell payload is append-only in `cells`; live_cells is a marker set.
        self.batch.put_cf(self.store.cf_cells(), key, &value);
        self.batch.put_cf(self.store.cf_live_cells(), key, []);
    }

    pub fn delete_cell(&mut self, tx_hash: &[u8], output_index: i16) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.batch.delete_cf(self.store.cf_live_cells(), key);
    }

    pub fn put_consumed_cell(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        info: &LiveCellInfo,
        consumed_at_block: i64,
    ) {
        self.put_consumed_cell_with_consumer(tx_hash, output_index, info, consumed_at_block, None);
    }

    pub fn put_consumed_cell_with_consumer(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        info: &LiveCellInfo,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
    ) {
        let key = keys::encode_outpoint(tx_hash, output_index);
        // Ensure canonical payload exists even when callers only write consumed entries.
        let cell_value = bincode::serialize(info).expect("serialize LiveCellInfo");
        self.batch.put_cf(self.store.cf_cells(), key, &cell_value);
        let consumed = ConsumedCellMeta {
            consumed_at_block,
            consumed_by_tx: consumed_by_tx.map(|tx| tx.to_vec()),
        };
        let value = bincode::serialize(&consumed).expect("serialize ConsumedCellMeta");
        self.batch
            .put_cf(self.store.cf_consumed_cells(), key, &value);
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
        self.batch.put_cf(self.store.cf_cell_by_lock(), key, []);
    }

    pub fn delete_cell_by_lock(
        &mut self,
        lock_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_hash, block_num, tx_hash, output_index);
        self.batch.delete_cf(self.store.cf_cell_by_lock(), &key);
    }

    pub fn put_cell_by_type(
        &mut self,
        type_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_hash, block_num, tx_hash, output_index);
        self.batch.put_cf(self.store.cf_cell_by_type(), key, []);
    }

    pub fn delete_cell_by_type(
        &mut self,
        type_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_hash, block_num, tx_hash, output_index);
        self.batch.delete_cf(self.store.cf_cell_by_type(), &key);
    }

    pub fn put_cell_by_lock_code(
        &mut self,
        lock_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_code_hash, block_num, tx_hash, output_index);
        self.batch
            .put_cf(self.store.cf_cell_by_lock_code(), key, []);
    }

    pub fn delete_cell_by_lock_code(
        &mut self,
        lock_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(lock_code_hash, block_num, tx_hash, output_index);
        self.batch
            .delete_cf(self.store.cf_cell_by_lock_code(), &key);
    }

    pub fn put_cell_by_type_code(
        &mut self,
        type_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_code_hash, block_num, tx_hash, output_index);
        self.batch
            .put_cf(self.store.cf_cell_by_type_code(), key, []);
    }

    pub fn delete_cell_by_type_code(
        &mut self,
        type_code_hash: &[u8],
        block_num: i64,
        tx_hash: &[u8],
        output_index: i16,
    ) {
        let key = keys::encode_cell_index_key(type_code_hash, block_num, tx_hash, output_index);
        self.batch
            .delete_cf(self.store.cf_cell_by_type_code(), &key);
    }

    // ---- Cell index (raw pre-computed key) ----

    pub fn put_cell_by_lock_raw(&mut self, key: &[u8]) {
        self.batch.put_cf(self.store.cf_cell_by_lock(), key, []);
    }

    pub fn delete_cell_by_lock_raw(&mut self, key: &[u8]) {
        self.batch.delete_cf(self.store.cf_cell_by_lock(), key);
    }

    pub fn put_cell_by_type_raw(&mut self, key: &[u8]) {
        self.batch.put_cf(self.store.cf_cell_by_type(), key, []);
    }

    pub fn delete_cell_by_type_raw(&mut self, key: &[u8]) {
        self.batch.delete_cf(self.store.cf_cell_by_type(), key);
    }

    pub fn put_cell_by_lock_code_raw(&mut self, key: &[u8]) {
        self.batch
            .put_cf(self.store.cf_cell_by_lock_code(), key, []);
    }

    pub fn delete_cell_by_lock_code_raw(&mut self, key: &[u8]) {
        self.batch.delete_cf(self.store.cf_cell_by_lock_code(), key);
    }

    pub fn put_cell_by_type_code_raw(&mut self, key: &[u8]) {
        self.batch
            .put_cf(self.store.cf_cell_by_type_code(), key, []);
    }

    pub fn delete_cell_by_type_code_raw(&mut self, key: &[u8]) {
        self.batch.delete_cf(self.store.cf_cell_by_type_code(), key);
    }

    // ---- Block headers ----

    pub fn put_block_header(&mut self, block_number: i64, header: &CachedBlockHeader) {
        let key = keys::encode_block_num(block_number);
        let value = bincode::serialize(header).expect("serialize CachedBlockHeader");
        self.batch
            .put_cf(self.store.cf_block_headers(), key, &value);

        // Also update hash -> number index
        self.batch.put_cf(
            self.store.cf_block_hash_index(),
            &header.hash,
            block_number.to_le_bytes(),
        );
    }

    pub fn delete_block_header(&mut self, block_number: i64, hash: &[u8]) {
        let key = keys::encode_block_num(block_number);
        self.batch.delete_cf(self.store.cf_block_headers(), key);
        self.batch.delete_cf(self.store.cf_block_hash_index(), hash);
    }

    // ---- Transaction index ----

    pub fn put_tx_index(&mut self, block_num: i64, tx_idx: i32, entry: &TxIndexEntry) {
        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        let value = bincode::serialize(entry).expect("serialize TxIndexEntry");
        self.batch.put_cf(self.store.cf_tx_index(), &key, &value);
    }

    pub fn put_tx_hash_map(&mut self, tx_hash: &[u8], block_num: i64, tx_idx: i32) {
        let value = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        self.batch
            .put_cf(self.store.cf_tx_hash_map(), tx_hash, &value);
    }

    pub fn delete_tx_index(&mut self, block_num: i64, tx_idx: i32) {
        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        self.batch.delete_cf(self.store.cf_tx_index(), &key);
    }

    pub fn delete_tx_hash_map(&mut self, tx_hash: &[u8]) {
        self.batch.delete_cf(self.store.cf_tx_hash_map(), tx_hash);
    }

    // ---- Address balance ----

    pub fn put_addr_balance(&mut self, lock_hash: &[u8], balance: &AddressBalance) {
        let value = bincode::serialize(balance).expect("serialize AddressBalance");
        self.batch
            .put_cf(self.store.cf_addr_balance(), lock_hash, &value);
    }

    pub fn put_addr_tx(&mut self, lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8]) {
        let key = keys::encode_addr_tx_key(lock_hash, block_num, tx_idx);
        self.batch.put_cf(self.store.cf_addr_txs(), &key, tx_hash);
    }

    // ---- DAO ----

    pub fn put_dao_deposit(&mut self, outpoint_key: &[u8], entry: &DaoDepositCacheEntry) {
        let value = bincode::serialize(entry).expect("serialize DaoDepositCacheEntry");
        self.batch
            .put_cf(self.store.cf_dao_deposits(), outpoint_key, &value);
    }

    pub fn put_dao_by_withdraw_tx(&mut self, tx_hash: &[u8], outpoint_key: &[u8]) {
        self.batch
            .put_cf(self.store.cf_dao_by_withdraw_tx(), tx_hash, outpoint_key);
    }

    pub fn put_block_issuance(&mut self, block_num: i64, issuance: &SecondaryIssuance) {
        let key = keys::encode_block_num(block_num);
        let value = bincode::serialize(issuance).expect("serialize SecondaryIssuance");
        self.batch
            .put_cf(self.store.cf_block_issuance(), key, &value);
    }

    // ---- Tokens ----

    pub fn put_token(&mut self, type_hash: &[u8], info: &TokenInfo) {
        let value = bincode::serialize(info).expect("serialize TokenInfo");
        self.batch.put_cf(self.store.cf_tokens(), type_hash, &value);
    }

    pub fn put_token_holder(&mut self, type_hash: &[u8], lock_hash: &[u8], balance: i128) {
        let key = keys::encode_token_holder_key(type_hash, lock_hash);
        self.batch
            .put_cf(self.store.cf_token_holders(), key, balance.to_le_bytes());
    }

    pub fn put_token_transfers_count(&mut self, type_hash: &[u8], count: i64) {
        let key = keys::encode_token_transfers_key(type_hash);
        self.batch
            .put_cf(self.store.cf_stats_token(), key, count.to_le_bytes());
    }

    pub fn put_token_hourly_transfer(&mut self, type_hash: &[u8], hour_bucket: i64, count: i64) {
        let key = keys::encode_token_hourly_key(type_hash, hour_bucket);
        self.batch
            .put_cf(self.store.cf_stats_token(), key, count.to_le_bytes());
    }

    pub fn put_spore_hourly_transfer(&mut self, cluster_id: &[u8], hour_bucket: i64, count: i64) {
        let key = keys::encode_spore_hourly_key(cluster_id, hour_bucket);
        self.batch
            .put_cf(self.store.cf_stats_spore(), key, count.to_le_bytes());
    }

    pub fn put_nft_hourly_transfer(&mut self, collection_id: &[u8], hour_bucket: i64, count: i64) {
        let key = keys::encode_nft_hourly_key(collection_id, hour_bucket);
        self.batch
            .put_cf(self.store.cf_stats_nft(), key, count.to_le_bytes());
    }

    pub fn put_nft_daily_delta(
        &mut self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
        delta: &NftDailyDelta,
    ) {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        let value = bincode::serialize(delta).expect("serialize NftDailyDelta");
        self.batch.put_cf(self.store.cf_stats_nft(), key, &value);
    }

    pub fn put_nft_type_index(&mut self, type_script_hash: &[u8], index: &NftTypeIndex) {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        let value = bincode::serialize(index).expect("serialize NftTypeIndex");
        self.batch.put_cf(self.store.cf_stats_nft(), key, &value);
    }

    pub fn put_cluster_daily_delta(
        &mut self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
        delta: &ClusterDailyDelta,
    ) {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        let value = bincode::serialize(delta).expect("serialize ClusterDailyDelta");
        self.batch.put_cf(self.store.cf_stats_spore(), key, &value);
    }

    pub fn put_spore_daily_delta(
        &mut self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
        delta: &SporeDailyDelta,
    ) {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        let value = bincode::serialize(delta).expect("serialize SporeDailyDelta");
        self.batch.put_cf(self.store.cf_stats_spore(), key, &value);
    }

    pub fn put_spore_type_index(&mut self, type_script_hash: &[u8], index: &SporeTypeIndex) {
        let key = keys::encode_spore_type_index_key(type_script_hash);
        let value = bincode::serialize(index).expect("serialize SporeTypeIndex");
        self.batch.put_cf(self.store.cf_stats_spore(), key, &value);
    }

    pub fn put_spore_outpoint(&mut self, tx_hash: &[u8], output_index: i16, spore_id: &[u8]) {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        self.batch
            .put_cf(self.store.cf_stats_spore(), key, spore_id);
        // Reverse index: spore_id → outpoints
        let rev_key = keys::encode_spore_outpoint_by_id_key(spore_id, tx_hash, output_index);
        self.batch
            .put_cf(self.store.cf_stats_spore(), rev_key, &[] as &[u8]);
    }

    pub fn delete_spore_outpoint(&mut self, tx_hash: &[u8], output_index: i16, spore_id: &[u8]) {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        self.batch.delete_cf(self.store.cf_stats_spore(), key);
        let rev_key = keys::encode_spore_outpoint_by_id_key(spore_id, tx_hash, output_index);
        self.batch.delete_cf(self.store.cf_stats_spore(), rev_key);
    }

    pub fn put_mnft_class_outpoint(&mut self, tx_hash: &[u8], output_index: i16, class_id: &[u8]) {
        let key = keys::encode_mnft_class_outpoint_key(tx_hash, output_index);
        self.batch.put_cf(self.store.cf_stats_nft(), key, class_id);
    }

    pub fn put_mnft_token_outpoint(&mut self, tx_hash: &[u8], output_index: i16, token_id: &[u8]) {
        let key = keys::encode_mnft_token_outpoint_key(tx_hash, output_index);
        self.batch.put_cf(self.store.cf_stats_nft(), key, token_id);
    }

    pub fn put_dotbit_account_outpoint(
        &mut self,
        tx_hash: &[u8],
        output_index: i16,
        account_id: &[u8],
    ) {
        let key = keys::encode_dotbit_account_outpoint_key(tx_hash, output_index);
        self.batch
            .put_cf(self.store.cf_stats_nft(), key, account_id);
    }

    pub fn delete_token_holder(&mut self, type_hash: &[u8], lock_hash: &[u8]) {
        let key = keys::encode_token_holder_key(type_hash, lock_hash);
        self.batch.delete_cf(self.store.cf_token_holders(), key);
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
        self.batch
            .put_cf(self.store.cf_token_transfers(), key, &value);
    }

    // ---- Spore/NFT ----

    pub fn put_spore(&mut self, id: &[u8], entry: &SporeEntry) {
        let value = bincode::serialize(entry).expect("serialize SporeEntry");
        self.batch.put_cf(self.store.cf_spore_data(), id, &value);
    }

    pub fn put_spore_by_cluster(&mut self, cluster_id: &[u8], spore_id: &[u8]) {
        let key = keys::encode_spore_by_cluster_key(cluster_id, spore_id);
        self.batch.put_cf(self.store.cf_spore_by_cluster(), key, []);
    }

    pub fn delete_spore_by_cluster(&mut self, cluster_id: &[u8], spore_id: &[u8]) {
        let key = keys::encode_spore_by_cluster_key(cluster_id, spore_id);
        self.batch.delete_cf(self.store.cf_spore_by_cluster(), key);
    }

    pub fn put_nft(&mut self, id: &[u8], entry: &NftEntry) {
        let value = bincode::serialize(entry).expect("serialize NftEntry");
        self.batch.put_cf(self.store.cf_nft_data(), id, &value);
    }

    pub fn put_nft_by_collection(&mut self, collection_id: &[u8], nft_id: &[u8]) {
        let key = keys::encode_nft_by_collection_key(collection_id, nft_id);
        self.batch
            .put_cf(self.store.cf_nft_by_collection(), key, []);
    }

    pub fn delete_nft_by_collection(&mut self, collection_id: &[u8], nft_id: &[u8]) {
        let key = keys::encode_nft_by_collection_key(collection_id, nft_id);
        self.batch.delete_cf(self.store.cf_nft_by_collection(), key);
    }

    // ---- Cluster aggregates ----

    pub fn put_cluster_aggregate(&mut self, cluster_id: &[u8], agg: &ClusterAggregate) {
        let value = bincode::serialize(agg).expect("serialize ClusterAggregate");
        self.batch
            .put_cf(self.store.cf_cluster_agg(), cluster_id, &value);
    }

    pub fn put_cluster_owner_count(&mut self, cluster_id: &[u8], lock_hash: &[u8], count: i64) {
        let key = keys::encode_cluster_owner_key(cluster_id, lock_hash);
        self.batch
            .put_cf(self.store.cf_stats_spore(), key, count.to_le_bytes());
    }

    pub fn delete_cluster_owner(&mut self, cluster_id: &[u8], lock_hash: &[u8]) {
        let key = keys::encode_cluster_owner_key(cluster_id, lock_hash);
        self.batch.delete_cf(self.store.cf_stats_spore(), key);
    }

    // ---- NFT collection aggregates ----

    pub fn put_nft_collection_aggregate(
        &mut self,
        collection_id: &[u8],
        agg: &NftCollectionAggregate,
    ) {
        let value = bincode::serialize(agg).expect("serialize NftCollectionAggregate");
        self.batch
            .put_cf(self.store.cf_nft_collection_agg(), collection_id, &value);
    }

    // ---- Activities ----

    pub fn put_activity(
        &mut self,
        lock_hash: &[u8],
        block_num: i64,
        tx_idx: i32,
        entry: &ActivityEntry,
    ) {
        let key = keys::encode_activity_key(lock_hash, block_num, tx_idx);
        let value = bincode::serialize(entry).expect("serialize ActivityEntry");
        self.batch.put_cf(self.store.cf_activities(), key, &value);
    }

    // ---- NFT collection activities ----

    pub fn put_nft_collection_activity(
        &mut self,
        collection_id: &[u8],
        block_num: i64,
        tx_idx: i32,
        entry: &NftCollectionActivityEntry,
    ) {
        let key = keys::encode_nft_collection_activity_key(collection_id, block_num, tx_idx);
        let value = bincode::serialize(entry).expect("serialize NftCollectionActivityEntry");
        self.batch
            .put_cf(self.store.cf_nft_collection_activities(), key, &value);
    }

    // ---- Address daily stats ----

    pub fn put_addr_daily_stats(
        &mut self,
        lock_hash: &[u8],
        date_yyyymmdd: u32,
        stats: &AddressDailyStats,
    ) {
        let key = keys::encode_addr_daily_stats_key(lock_hash, date_yyyymmdd);
        let value = bincode::serialize(stats).expect("serialize AddressDailyStats");
        self.batch
            .put_cf(self.store.cf_addr_daily_stats(), key, &value);
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
        self.batch.put_cf(cf, key, value);
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
        self.batch.delete_cf(cf, key);
    }

    pub fn put_script_info(&mut self, code_hash: &[u8], info: &ScriptInfo) {
        let value = bincode::serialize(info).expect("serialize ScriptInfo");
        self.batch
            .put_cf(self.store.cf_script_info(), code_hash, &value);
    }

    // ---- Sync meta ----

    pub fn put_sync_meta(&mut self, key: &[u8], value: &[u8]) {
        self.batch.put_cf(self.store.cf_sync_meta(), key, value);
    }

    /// Get mutable access to the underlying WriteBatch for direct operations.
    pub fn raw_batch(&mut self) -> &mut WriteBatch {
        &mut self.batch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_batch_commit() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        let header = CachedBlockHeader {
            hash: vec![1u8; 32],
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
        let store = CkbadgerStore::open(dir.path()).unwrap();

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
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let tx_hash = [42u8; 32];
        let info = LiveCellInfo {
            capacity: 10000,
            created_at_block: 1,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
            udt_amount: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &info);
        batch.commit().unwrap();

        let key = keys::encode_outpoint(&tx_hash, 0);
        assert!(store.get_cf(store.cf_live_cells(), &key).unwrap().is_some());

        let mut batch = StoreBatch::new(&store);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        assert!(store.get_cf(store.cf_live_cells(), &key).unwrap().is_none());
    }

    #[test]
    fn test_token_transfers_count_batch_write() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
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
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let type_hash = [0xBBu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&type_hash, 500_000, 7);
        batch.commit().unwrap();

        let key = keys::encode_token_hourly_key(&type_hash, 500_000);
        let val = store.get_cf(store.cf_stats_token(), &key).unwrap().unwrap();
        assert_eq!(i64::from_le_bytes(val[..8].try_into().unwrap()), 7);
    }

    fn make_activity(block_num: i64, tx_idx: i32, delta: i128) -> ActivityEntry {
        ActivityEntry {
            tx_hash: vec![block_num as u8; 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            ckb_delta: delta,
            occupied_delta: 0,
            is_cellbase: tx_idx == 0,
            asset_changes: vec![],
            peers: vec![],
        }
    }

    #[test]
    fn test_put_and_list_activities() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock = [0xAAu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_activity(&lock, 100, 0, &make_activity(100, 0, 500));
        batch.put_activity(&lock, 200, 0, &make_activity(200, 0, -300));
        batch.put_activity(&lock, 300, 1, &make_activity(300, 1, 1000));
        batch.commit().unwrap();

        let results = store.list_activities(&lock, 100, None, None).unwrap();
        assert_eq!(results.len(), 3);
        // Descending block order: 300, 200, 100
        assert_eq!(results[0].0, 300);
        assert_eq!(results[0].2.ckb_delta, 1000);
        assert_eq!(results[1].0, 200);
        assert_eq!(results[1].2.ckb_delta, -300);
        assert_eq!(results[2].0, 100);
        assert_eq!(results[2].2.ckb_delta, 500);
    }

    #[test]
    fn test_list_activities_with_limit() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock = [0xBBu8; 32];

        let mut batch = StoreBatch::new(&store);
        for i in 1..=5 {
            batch.put_activity(&lock, i * 100, 0, &make_activity(i * 100, 0, i as i128));
        }
        batch.commit().unwrap();

        let results = store.list_activities(&lock, 2, None, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 500); // newest first
        assert_eq!(results[1].0, 400);
    }

    #[test]
    fn test_list_activities_cursor_pagination() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock = [0xCCu8; 32];

        let mut batch = StoreBatch::new(&store);
        for i in 1..=5 {
            batch.put_activity(&lock, i * 100, 0, &make_activity(i * 100, 0, i as i128));
        }
        batch.commit().unwrap();

        // Page 1: first 2
        let page1 = store.list_activities(&lock, 2, None, None).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].0, 500);
        assert_eq!(page1[1].0, 400);

        // Page 2: cursor from last item of page1
        let cursor = (page1[1].0, page1[1].1);
        let page2 = store.list_activities(&lock, 2, Some(cursor), None).unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].0, 300);
        assert_eq!(page2[1].0, 200);

        // Page 3: last page
        let cursor = (page2[1].0, page2[1].1);
        let page3 = store.list_activities(&lock, 2, Some(cursor), None).unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].0, 100);
    }

    #[test]
    fn test_list_activities_different_locks_isolated() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = [0x01u8; 32];
        let lock_b = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_activity(&lock_a, 100, 0, &make_activity(100, 0, 10));
        batch.put_activity(&lock_a, 200, 0, &make_activity(200, 0, 20));
        batch.put_activity(&lock_b, 100, 0, &make_activity(100, 0, 30));
        batch.commit().unwrap();

        let a = store.list_activities(&lock_a, 100, None, None).unwrap();
        assert_eq!(a.len(), 2);

        let b = store.list_activities(&lock_b, 100, None, None).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].2.ckb_delta, 30);
    }

    #[test]
    fn test_list_activities_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock = [0xFFu8; 32];

        let results = store.list_activities(&lock, 100, None, None).unwrap();
        assert!(results.is_empty());
    }
}
