//! Reorg (rollback) operations.

use rocksdb::{IteratorMode, WriteBatch};

use crate::keys;
use crate::store::*;
use crate::types::*;

fn should_delete_stats_for_replay(key: &[u8], cutoff_yyyymmdd: &[u8]) -> bool {
    if key.is_empty() {
        return false;
    }
    let prefix = key[0];
    let suffix = &key[1..];

    match prefix {
        // date scoped: YYYYMMDD
        keys::STATS_PREFIX_DAILY
        | keys::STATS_PREFIX_DAILY_BLOCK
        | keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT
        | keys::STATS_PREFIX_HODL_WAVE => suffix.len() >= 8 && &suffix[..8] >= cutoff_yyyymmdd,
        // hour scoped: YYYYMMDDHH
        keys::STATS_PREFIX_HOURLY => suffix.len() >= 10 && &suffix[..8] >= cutoff_yyyymmdd,
        // date+miner hash: YYYYMMDD + 32-byte lock hash
        keys::STATS_PREFIX_MINER => suffix.len() >= 40 && &suffix[..8] >= cutoff_yyyymmdd,
        _ => false,
    }
}

impl CkbadgerStore {
    /// Atomic rollback across all CFs to a given block number.
    /// Deletes all data for blocks > rollback_to.
    pub fn rollback_to_block(&self, rollback_to: i64) -> anyhow::Result<RollbackResult> {
        let mut batch = WriteBatch::default();
        let mut blocks_removed = 0u64;
        let mut txs_removed = 0u64;
        let mut cells_removed = 0u64;
        let cells_restored = 0u64;
        let replay_start = rollback_to + 1;
        let replay_cutoff_date = self
            .get_block_header(replay_start)?
            .and_then(|h| chrono::DateTime::from_timestamp(h.timestamp / 1000, 0))
            .map(|dt| dt.format("%Y%m%d").to_string());

        // 1. Delete block headers > rollback_to
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_block_headers(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                batch.delete_cf(self.cf_block_headers(), &key);
                batch.delete_cf(self.cf_block_hash_index(), &header.hash);
                blocks_removed += 1;
            }
        }

        // 2. Delete tx_index entries > rollback_to
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_tx_index(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 12 {
                let block_num = keys::decode_block_num(&key[..8]);
                if block_num <= rollback_to {
                    continue;
                }
                batch.delete_cf(self.cf_tx_index(), &key);
                txs_removed += 1;
            }
        }

        // 3. Delete live cells created after rollback_to, restore consumed cells
        let iter = self.iterator_cf(self.cf_live_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > rollback_to {
                    batch.delete_cf(self.cf_live_cells(), &key);
                    cells_removed += 1;

                    // Clean up indexes
                    if key.len() == 34 {
                        let (tx_hash, output_index) = keys::decode_outpoint(&key);
                        let idx_key = keys::encode_cell_index_key(
                            &info.lock_script_hash,
                            info.created_at_block,
                            &tx_hash,
                            output_index,
                        );
                        batch.delete_cf(self.cf_cell_by_lock(), &idx_key);
                        let idx_key = keys::encode_cell_index_key(
                            &info.lock_code_hash,
                            info.created_at_block,
                            &tx_hash,
                            output_index,
                        );
                        batch.delete_cf(self.cf_cell_by_lock_code(), &idx_key);
                        if let Some(ref type_hash) = info.type_script_hash {
                            let idx_key = keys::encode_cell_index_key(
                                type_hash,
                                info.created_at_block,
                                &tx_hash,
                                output_index,
                            );
                            batch.delete_cf(self.cf_cell_by_type(), &idx_key);
                        }
                        if let Some(ref type_code_hash) = info.type_code_hash {
                            let idx_key = keys::encode_cell_index_key(
                                type_code_hash,
                                info.created_at_block,
                                &tx_hash,
                                output_index,
                            );
                            batch.delete_cf(self.cf_cell_by_type_code(), &idx_key);
                        }
                    }
                }
            }
        }

        // 4. Restore consumed cells that were consumed after rollback_to
        // Note: consumed cell restoration is handled by the caller using
        // in-memory consumed_history, since the consumed_cells CF doesn't
        // track consumed_at_block. The caller should restore cells that were
        // consumed after rollback_to back to the live_cells CF.

        // 5. Delete date-scoped stats entries from replay cutoff date onward.
        // These are additive snapshots and would be double-counted after replay.
        if let Some(cutoff) = replay_cutoff_date.as_deref() {
            let iter = self.iterator_cf(self.cf_stats(), IteratorMode::Start);
            for item in iter.flatten() {
                let (key, _) = item;
                if should_delete_stats_for_replay(&key, cutoff.as_bytes()) {
                    batch.delete_cf(self.cf_stats(), &key);
                }
            }
        }

        // 7. Delete block issuance > rollback_to
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_block_issuance(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter.flatten() {
            let (key, _) = item;
            batch.delete_cf(self.cf_block_issuance(), &key);
        }

        // 8. Delete addr_txs entries > rollback_to
        // Key: lock_hash(32) + block_num(8) + tx_idx(4) = 44
        let iter = self.iterator_cf(self.cf_addr_txs(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let block_num = keys::decode_block_num(&key[32..40]);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_addr_txs(), &key);
                }
            }
        }

        // 9a. Delete activities entries > rollback_to
        // Key: lock_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        let iter = self.iterator_cf(self.cf_activities(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let (_, block_num, _) = keys::decode_activity_key(&key);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_activities(), &key);
                }
            }
        }

        // 9. Delete token_transfers entries > rollback_to
        // Key: type_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        let iter = self.iterator_cf(self.cf_token_transfers(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let (block_num, _) = keys::decode_token_transfer_key(&key);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_token_transfers(), &key);
                }
            }
        }

        // Commit all deletes atomically
        self.write_batch(batch)?;

        Ok(RollbackResult {
            blocks_removed,
            txs_removed,
            cells_removed,
            cells_restored,
        })
    }
}

#[derive(Debug, Default)]
pub struct RollbackResult {
    pub blocks_removed: u64,
    pub txs_removed: u64,
    pub cells_removed: u64,
    pub cells_restored: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_delete_stats_for_replay_daily_prefix() {
        let cutoff = b"20260210";
        let key = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260211");
        assert!(should_delete_stats_for_replay(&key, cutoff));

        let key_old = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260209");
        assert!(!should_delete_stats_for_replay(&key_old, cutoff));
    }

    #[test]
    fn test_should_delete_stats_for_replay_hourly_and_miner_prefix() {
        let cutoff = b"20260210";
        let hourly = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_HOURLY, b"2026021001");
        assert!(should_delete_stats_for_replay(&hourly, cutoff));

        let miner_suffix = [b"20260210".as_slice(), &[0xAA; 32]].concat();
        let miner = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_MINER, &miner_suffix);
        assert!(should_delete_stats_for_replay(&miner, cutoff));
    }
}
