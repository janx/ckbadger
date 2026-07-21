//! Fiber channel query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;

impl CkbadgerStore {
    /// Get a Fiber channel by its channel_id (32-byte blake2b hash).
    pub fn get_fiber_channel(&self, channel_id: &[u8]) -> anyhow::Result<Option<FiberChannel>> {
        match self.get_cf(self.cf_fiber_channels(), channel_id)? {
            Some(value) => {
                let channel: FiberChannel = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize FiberChannel: channel_id=0x{}, error={}",
                        crate::bytes_to_hex(channel_id),
                        e
                    )
                })?;
                Ok(Some(channel))
            }
            None => Ok(None),
        }
    }

    /// Look up a channel_id by commitment lock outpoint hash.
    pub fn get_fiber_channel_id_by_commitment(
        &self,
        commitment_hash: &[u8],
    ) -> anyhow::Result<Option<Vec<u8>>> {
        self.get_cf(self.cf_fiber_channel_by_commitment(), commitment_hash)
    }

    /// List Fiber channels with optional state filter, ordered by channel_id.
    ///
    /// `cursor` is the last channel_id seen (exclusive start for next page).
    pub fn list_fiber_channels(
        &self,
        limit: usize,
        cursor: Option<&[u8]>,
        state_filter: Option<FiberChannelState>,
    ) -> anyhow::Result<Vec<(Vec<u8>, FiberChannel)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let cf = self.cf_fiber_channels();
        let iter = match cursor {
            Some(last_key) => {
                // Start after the cursor key by appending 0xFF
                let mut seek_key = last_key.to_vec();
                seek_key.push(0xFF);
                self.iterator_cf(
                    cf,
                    rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Forward),
                )
            }
            None => self.iterator_cf(cf, rocksdb::IteratorMode::Start),
        };

        let mut results = Vec::new();
        for item in iter {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("failed to iterate fiber_channels: {}", e))?;
            if key.len() != keys::FIBER_CHANNEL_KEY_SIZE {
                continue;
            }

            let channel: FiberChannel = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize FiberChannel in list: key=0x{}, error={}",
                    crate::bytes_to_hex(&key),
                    e
                )
            })?;

            if let Some(filter) = state_filter {
                if channel.state != filter {
                    continue;
                }
            }

            results.push((key.to_vec(), channel));
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// List Fiber channels for a given address (lock_hash).
    ///
    /// Scans CF_ADDR_FIBER_CHANNELS by lock_hash prefix, then fetches
    /// each channel from CF_FIBER_CHANNELS.
    pub fn list_addr_fiber_channels(
        &self,
        lock_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, FiberChannel)>> {
        if limit == 0 || lock_hash.len() < 32 {
            return Ok(Vec::new());
        }

        let prefix = &lock_hash[..32];
        let iter = self.prefix_iterator_cf(self.cf_addr_fiber_channels(), prefix);

        let mut results = Vec::new();
        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate addr_fiber_channels: lock_hash=0x{}, error={}",
                    crate::bytes_to_hex(lock_hash),
                    e
                )
            })?;
            if key.len() < keys::ADDR_FIBER_CHANNEL_KEY_SIZE {
                continue;
            }

            let (_lock, channel_id) = keys::decode_addr_fiber_channel_key(&key);

            match self.get_fiber_channel(channel_id)? {
                Some(channel) => {
                    results.push((channel_id.to_vec(), channel));
                    if results.len() >= limit {
                        break;
                    }
                }
                None => {
                    // Index entry without corresponding channel data — skip.
                    continue;
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn make_channel(
        funding_tx_hash: &[u8],
        output_index: u32,
        state: FiberChannelState,
        capacity: u64,
    ) -> FiberChannel {
        FiberChannel {
            funding_tx_hash: funding_tx_hash.to_vec(),
            funding_output_index: output_index,
            state,
            capacity,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 100,
            open_timestamp: 1_700_000_000,
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![vec![0xAA; 32], vec![0xBB; 32]],
            funding_lock_args: vec![0xCC; 20],
        }
    }

    #[test]
    fn test_get_fiber_channel_not_found() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let result = store.get_fiber_channel(&[0x11; 32]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get_fiber_channel() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let tx_hash = [0x10; 32];
        let channel_id = crate::keys::encode_fiber_channel_id(&tx_hash, 0);
        let channel = make_channel(&tx_hash, 0, FiberChannelState::Open, 500_00000000);

        let mut batch = StoreBatch::new(&store);
        batch.put_fiber_channel(&channel_id, &channel);
        batch.commit().unwrap();

        let loaded = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(loaded.state, FiberChannelState::Open);
        assert_eq!(loaded.capacity, 500_00000000);
        assert_eq!(loaded.funding_tx_hash, tx_hash.to_vec());
    }

    #[test]
    fn test_list_fiber_channels_with_state_filter() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);

        let tx1 = [0x01; 32];
        let id1 = crate::keys::encode_fiber_channel_id(&tx1, 0);
        let ch1 = make_channel(&tx1, 0, FiberChannelState::Open, 100_00000000);
        batch.put_fiber_channel(&id1, &ch1);

        let tx2 = [0x02; 32];
        let id2 = crate::keys::encode_fiber_channel_id(&tx2, 0);
        let ch2 = make_channel(
            &tx2,
            0,
            FiberChannelState::CooperativelyClosed,
            200_00000000,
        );
        batch.put_fiber_channel(&id2, &ch2);

        batch.commit().unwrap();

        // No filter: should return both
        let all = store.list_fiber_channels(10, None, None).unwrap();
        assert_eq!(all.len(), 2);

        // Filter Open
        let open = store
            .list_fiber_channels(10, None, Some(FiberChannelState::Open))
            .unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].1.state, FiberChannelState::Open);

        // Filter CooperativelyClosed
        let closed = store
            .list_fiber_channels(10, None, Some(FiberChannelState::CooperativelyClosed))
            .unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].1.state, FiberChannelState::CooperativelyClosed);
    }

    #[test]
    fn test_list_addr_fiber_channels() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let lock_hash = [0xAA; 32];
        let tx_hash = [0x10; 32];
        let channel_id = crate::keys::encode_fiber_channel_id(&tx_hash, 0);
        let channel = make_channel(&tx_hash, 0, FiberChannelState::Open, 300_00000000);

        let mut batch = StoreBatch::new(&store);
        batch.put_fiber_channel(&channel_id, &channel);
        batch.put_addr_fiber_channel(&lock_hash, &channel_id);
        batch.commit().unwrap();

        let results = store.list_addr_fiber_channels(&lock_hash, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, channel_id);
        assert_eq!(results[0].1.capacity, 300_00000000);

        // Different lock_hash should return empty
        let other = [0xBB; 32];
        let empty = store.list_addr_fiber_channels(&other, 10).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_fiber_channel_by_commitment_lookup() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let commitment_hash = [0xDD; 32];
        let channel_id = [0xEE; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_fiber_channel_by_commitment(&commitment_hash, &channel_id);
        batch.commit().unwrap();

        let result = store
            .get_fiber_channel_id_by_commitment(&commitment_hash)
            .unwrap()
            .unwrap();
        assert_eq!(result, channel_id.to_vec());

        // Non-existent commitment
        let missing = store
            .get_fiber_channel_id_by_commitment(&[0xFF; 32])
            .unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_fiber_channel() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let tx_hash = [0x10; 32];
        let channel_id = crate::keys::encode_fiber_channel_id(&tx_hash, 0);
        let channel = make_channel(&tx_hash, 0, FiberChannelState::Open, 100_00000000);
        let lock_hash = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_fiber_channel(&channel_id, &channel);
        batch.put_addr_fiber_channel(&lock_hash, &channel_id);
        batch.commit().unwrap();

        // Verify exists
        assert!(store.get_fiber_channel(&channel_id).unwrap().is_some());

        // Delete
        let mut batch2 = StoreBatch::new(&store);
        batch2.delete_fiber_channel(&channel_id);
        batch2.delete_addr_fiber_channel(&lock_hash, &channel_id);
        batch2.commit().unwrap();

        assert!(store.get_fiber_channel(&channel_id).unwrap().is_none());
        let results = store.list_addr_fiber_channels(&lock_hash, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_store_batch_fiber_channel_view_reads_own_writes_and_deletes() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let tx_hash = [0x10; 32];
        let channel_id = crate::keys::encode_fiber_channel_id(&tx_hash, 0);
        let mut channel = make_channel(&tx_hash, 0, FiberChannelState::Open, 100_00000000);

        let mut batch = StoreBatch::new(&store);
        assert!(batch.get_fiber_channel(&channel_id).unwrap().is_none());

        batch.put_fiber_channel(&channel_id, &channel);
        assert_eq!(
            batch.get_fiber_channel(&channel_id).unwrap().unwrap().state,
            FiberChannelState::Open
        );

        channel.state = FiberChannelState::CooperativelyClosed;
        batch.put_fiber_channel(&channel_id, &channel);
        assert_eq!(
            batch.get_fiber_channel(&channel_id).unwrap().unwrap().state,
            FiberChannelState::CooperativelyClosed
        );

        batch.delete_fiber_channel(&channel_id);
        assert!(batch.get_fiber_channel(&channel_id).unwrap().is_none());
        batch.commit().unwrap();
        assert!(store.get_fiber_channel(&channel_id).unwrap().is_none());
    }

    #[test]
    fn test_store_batch_commitment_view_reads_own_rotation() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let channel_id = [0xEE; 32];
        let old_hash = [0xD1; 32];
        let new_hash = [0xD2; 32];
        let mut batch = StoreBatch::new(&store);

        batch.put_fiber_channel_by_commitment(&old_hash, &channel_id);
        assert_eq!(
            batch
                .get_fiber_channel_id_by_commitment(&old_hash)
                .unwrap()
                .unwrap(),
            channel_id
        );

        batch.delete_fiber_channel_by_commitment(&old_hash);
        batch.put_fiber_channel_by_commitment(&new_hash, &channel_id);
        assert!(batch
            .get_fiber_channel_id_by_commitment(&old_hash)
            .unwrap()
            .is_none());
        assert_eq!(
            batch
                .get_fiber_channel_id_by_commitment(&new_hash)
                .unwrap()
                .unwrap(),
            channel_id
        );

        batch.commit().unwrap();
        assert!(store
            .get_fiber_channel_id_by_commitment(&old_hash)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_fiber_channel_id_by_commitment(&new_hash)
                .unwrap()
                .unwrap(),
            channel_id
        );
    }

    #[test]
    fn test_list_fiber_channels_limit_zero() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let results = store.list_fiber_channels(0, None, None).unwrap();
        assert!(results.is_empty());
    }
}
