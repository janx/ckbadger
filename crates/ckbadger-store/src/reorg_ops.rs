//! Reorg (rollback) operations.

use rocksdb::{IteratorMode, WriteBatch};
use std::time::{Duration, Instant};
use tracing::info;

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
        // code_hash(32) + kind(1) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_SCRIPT_DAILY => {
            let cutoff_date = std::str::from_utf8(cutoff_yyyymmdd)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if suffix.len() < 37 {
                return false;
            }
            let date = u32::from_be_bytes(suffix[33..37].try_into().unwrap_or([0; 4]));
            date >= cutoff_date
        }
        // type_hash(32) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_TOKEN_DAILY => {
            let cutoff_date = std::str::from_utf8(cutoff_yyyymmdd)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if suffix.len() < 36 {
                return false;
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().unwrap_or([0; 4]));
            date >= cutoff_date
        }
        // cluster_id(32) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_CLUSTER_DAILY => {
            let cutoff_date = std::str::from_utf8(cutoff_yyyymmdd)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if suffix.len() < 36 {
                return false;
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().unwrap_or([0; 4]));
            date >= cutoff_date
        }
        // spore_id(32) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_SPORE_DAILY => {
            let cutoff_date = std::str::from_utf8(cutoff_yyyymmdd)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if suffix.len() < 36 {
                return false;
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().unwrap_or([0; 4]));
            date >= cutoff_date
        }
        // collection_id(32 padded) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_NFT_DAILY => {
            let cutoff_date = std::str::from_utf8(cutoff_yyyymmdd)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if suffix.len() < 36 {
                return false;
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().unwrap_or([0; 4]));
            date >= cutoff_date
        }
        _ => false,
    }
}

const ROLLBACK_PROGRESS_CHECK_EVERY: u64 = 16_384;
const ROLLBACK_PROGRESS_MIN_INTERVAL: Duration = Duration::from_secs(5);

fn should_log_rollback_progress(scanned: u64, since_last_log: Duration) -> bool {
    scanned > 0
        && scanned.is_multiple_of(ROLLBACK_PROGRESS_CHECK_EVERY)
        && since_last_log >= ROLLBACK_PROGRESS_MIN_INTERVAL
}

struct RollbackStageProgress {
    stage: &'static str,
    started_at: Instant,
    last_log_at: Instant,
    scanned: u64,
}

impl RollbackStageProgress {
    fn new(stage: &'static str) -> Self {
        let now = Instant::now();
        Self {
            stage,
            started_at: now,
            last_log_at: now,
            scanned: 0,
        }
    }

    fn tick(&mut self, affected: u64) {
        self.scanned += 1;
        if should_log_rollback_progress(self.scanned, self.last_log_at.elapsed()) {
            info!(
                stage = self.stage,
                scanned = self.scanned,
                affected,
                elapsed_secs = format!("{:.1}", self.started_at.elapsed().as_secs_f64()),
                "Rollback cleanup in progress"
            );
            self.last_log_at = Instant::now();
        }
    }

    fn finish(&self, affected: u64) {
        info!(
            stage = self.stage,
            scanned = self.scanned,
            affected,
            elapsed_secs = format!("{:.1}", self.started_at.elapsed().as_secs_f64()),
            "Rollback cleanup stage complete"
        );
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
        let mut cells_restored = 0u64;
        let rollback_started_at = Instant::now();
        let replay_start = rollback_to + 1;
        let replay_cutoff_date = self
            .get_block_header(replay_start)?
            .and_then(|h| chrono::DateTime::from_timestamp(h.timestamp / 1000, 0))
            .map(|dt| dt.format("%Y%m%d").to_string());

        info!(rollback_to, replay_start, "Rollback cleanup started");

        // 1. Delete block headers > rollback_to
        let mut stage = RollbackStageProgress::new("delete_block_headers");
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
            stage.tick(blocks_removed);
        }
        stage.finish(blocks_removed);

        // 2. Delete tx_index entries > rollback_to
        let mut stage = RollbackStageProgress::new("delete_tx_index");
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
                    stage.tick(txs_removed);
                    continue;
                }
                batch.delete_cf(self.cf_tx_index(), &key);
                txs_removed += 1;
            }
            stage.tick(txs_removed);
        }
        stage.finish(txs_removed);

        // 3. Delete live cells created after rollback_to, restore consumed cells
        let mut stage = RollbackStageProgress::new("delete_live_cells_after_tip");
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
            stage.tick(cells_removed);
        }
        stage.finish(cells_removed);

        // 4. Restore consumed cells that were consumed after rollback_to.
        //
        // New schema stores `consumed_at_block` in consumed_cells so rollback can
        // recover precise state without in-memory history.
        let mut stage = RollbackStageProgress::new("restore_consumed_cells");
        let iter = self.iterator_cf(self.cf_consumed_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                stage.tick(cells_restored);
                continue;
            }
            let Some(consumed) = crate::types::decode_consumed_cell_info(&value) else {
                stage.tick(cells_restored);
                continue;
            };
            if consumed.consumed_at_block <= rollback_to {
                stage.tick(cells_restored);
                continue;
            }

            // Remove stale consumed record from rolled-back blocks.
            batch.delete_cf(self.cf_consumed_cells(), &key);

            // If the cell itself existed before rollback point, restore it to live_cells.
            if consumed.cell.created_at_block <= rollback_to {
                let encoded = bincode::serialize(&consumed.cell)?;
                batch.put_cf(self.cf_live_cells(), &key, &encoded);
                cells_restored += 1;

                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                let idx_key = keys::encode_cell_index_key(
                    &consumed.cell.lock_script_hash,
                    consumed.cell.created_at_block,
                    &tx_hash,
                    output_index,
                );
                batch.put_cf(self.cf_cell_by_lock(), &idx_key, []);
                let idx_key = keys::encode_cell_index_key(
                    &consumed.cell.lock_code_hash,
                    consumed.cell.created_at_block,
                    &tx_hash,
                    output_index,
                );
                batch.put_cf(self.cf_cell_by_lock_code(), &idx_key, []);
                if let Some(ref type_hash) = consumed.cell.type_script_hash {
                    let idx_key = keys::encode_cell_index_key(
                        type_hash,
                        consumed.cell.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    batch.put_cf(self.cf_cell_by_type(), &idx_key, []);
                }
                if let Some(ref type_code_hash) = consumed.cell.type_code_hash {
                    let idx_key = keys::encode_cell_index_key(
                        type_code_hash,
                        consumed.cell.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    batch.put_cf(self.cf_cell_by_type_code(), &idx_key, []);
                }
            }
            stage.tick(cells_restored);
        }
        stage.finish(cells_restored);

        // 5. Delete date-scoped stats entries from replay cutoff date onward.
        // These are additive snapshots and would be double-counted after replay.
        if let Some(cutoff) = replay_cutoff_date.as_deref() {
            let mut stats_removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_stats_from_cutoff");
            let iter = self.iterator_cf(self.cf_stats(), IteratorMode::Start);
            for item in iter.flatten() {
                let (key, _) = item;
                if should_delete_stats_for_replay(&key, cutoff.as_bytes()) {
                    batch.delete_cf(self.cf_stats(), &key);
                    stats_removed += 1;
                }
                stage.tick(stats_removed);
            }
            stage.finish(stats_removed);
        }

        // 7. Delete block issuance > rollback_to
        let mut issuance_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_block_issuance");
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_block_issuance(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter.flatten() {
            let (key, _) = item;
            batch.delete_cf(self.cf_block_issuance(), &key);
            issuance_removed += 1;
            stage.tick(issuance_removed);
        }
        stage.finish(issuance_removed);

        // 8. Delete addr_txs entries > rollback_to
        // Key: lock_hash(32) + block_num(8) + tx_idx(4) = 44
        let mut addr_txs_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_addr_txs");
        let iter = self.iterator_cf(self.cf_addr_txs(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let block_num = keys::decode_block_num(&key[32..40]);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_addr_txs(), &key);
                    addr_txs_removed += 1;
                }
            }
            stage.tick(addr_txs_removed);
        }
        stage.finish(addr_txs_removed);

        // 9a. Delete activities entries > rollback_to
        // Key: lock_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        let mut activities_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_activities");
        let iter = self.iterator_cf(self.cf_activities(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let (_, block_num, _) = keys::decode_activity_key(&key);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_activities(), &key);
                    activities_removed += 1;
                }
            }
            stage.tick(activities_removed);
        }
        stage.finish(activities_removed);

        // 9. Delete token_transfers entries > rollback_to
        // Key: type_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        let mut token_transfers_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_token_transfers");
        let iter = self.iterator_cf(self.cf_token_transfers(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let (block_num, _) = keys::decode_token_transfer_key(&key);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_token_transfers(), &key);
                    token_transfers_removed += 1;
                }
            }
            stage.tick(token_transfers_removed);
        }
        stage.finish(token_transfers_removed);

        // Commit all deletes atomically
        self.write_batch(batch)?;

        info!(
            elapsed_secs = format!("{:.1}", rollback_started_at.elapsed().as_secs_f64()),
            blocks_removed,
            txs_removed,
            cells_removed,
            cells_restored,
            "Rollback cleanup write batch committed"
        );

        // Rebuild addr_balance from live_cells after rollback. Reorg deletes
        // created cells in rolled-back blocks, and historical drift can leave
        // addr_balance inconsistent with live_cells otherwise.
        info!("Rollback cleanup rebuilding addr_balance from live_cells");
        let rebuilt_balances = self.rebuild_addr_balances_from_live_cells()?;
        info!(
            rebuilt_balances,
            elapsed_secs = format!("{:.1}", rollback_started_at.elapsed().as_secs_f64()),
            "Rollback cleanup address balance rebuild complete"
        );

        // Keep sync_status tip aligned with the rolled-back chain head.
        let tip_hash = if rollback_to >= 0 {
            self.get_block_header(rollback_to)?
                .map(|h| h.hash)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let tip_number = rollback_to.max(0);
        self.update_sync_status(|status| {
            status.tip_block_number = tip_number;
            status.tip_block_hash = tip_hash.clone();
            status.last_synced_at = chrono::Utc::now().timestamp();
        })?;

        info!(
            tip_number,
            elapsed_secs = format!("{:.1}", rollback_started_at.elapsed().as_secs_f64()),
            "Rollback cleanup complete"
        );

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
    use crate::batch::StoreBatch;
    use crate::keys;
    use crate::store::CkbadgerStore;
    use crate::types::{AddressBalance, CachedBlockHeader, LiveCellInfo};

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

    #[test]
    fn test_should_delete_stats_for_replay_script_daily_prefix() {
        let cutoff = b"20260210";
        let code_hash = [0xAA; 32];

        let new_key = crate::keys::encode_script_daily_key(&code_hash, false, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff));

        let old_key = crate::keys::encode_script_daily_key(&code_hash, true, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff));
    }

    #[test]
    fn test_should_delete_stats_for_replay_token_daily_prefix() {
        let cutoff = b"20260210";
        let type_hash = [0xBB; 32];

        let new_key = crate::keys::encode_token_daily_key(&type_hash, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff));

        let old_key = crate::keys::encode_token_daily_key(&type_hash, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff));
    }

    #[test]
    fn test_should_delete_stats_for_replay_cluster_daily_prefix() {
        let cutoff = b"20260210";
        let cluster_id = [0xCC; 32];

        let new_key = crate::keys::encode_cluster_daily_key(&cluster_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff));

        let old_key = crate::keys::encode_cluster_daily_key(&cluster_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff));
    }

    #[test]
    fn test_should_delete_stats_for_replay_spore_daily_prefix() {
        let cutoff = b"20260210";
        let spore_id = [0xDD; 32];

        let new_key = crate::keys::encode_spore_daily_key(&spore_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff));

        let old_key = crate::keys::encode_spore_daily_key(&spore_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff));
    }

    #[test]
    fn test_should_delete_stats_for_replay_nft_daily_prefix() {
        let cutoff = b"20260210";
        let collection_id = [0xEE; 24];

        let new_key = crate::keys::encode_nft_daily_key(&collection_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff));

        let old_key = crate::keys::encode_nft_daily_key(&collection_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff));
    }

    #[test]
    fn test_should_log_rollback_progress_requires_count_and_time_threshold() {
        assert!(!should_log_rollback_progress(
            ROLLBACK_PROGRESS_CHECK_EVERY - 1,
            Duration::from_secs(10)
        ));
        assert!(!should_log_rollback_progress(
            ROLLBACK_PROGRESS_CHECK_EVERY,
            Duration::from_secs(1)
        ));
        assert!(should_log_rollback_progress(
            ROLLBACK_PROGRESS_CHECK_EVERY,
            Duration::from_secs(5)
        ));
    }

    #[test]
    fn test_rollback_rebuilds_addr_balance_from_live_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_hash = vec![0xAA; 32];

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let cell_block_1 = LiveCellInfo {
            capacity: 100,
            created_at_block: 1,
            lock_script_hash: lock_hash.clone(),
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 100,
        };
        let cell_block_2 = LiveCellInfo {
            capacity: 300,
            created_at_block: 2,
            lock_script_hash: lock_hash.clone(),
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 300,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x10; 32], 0, &cell_block_1);
        batch.put_cell(&[0x20; 32], 0, &cell_block_2);
        batch.put_addr_balance(
            &lock_hash,
            &AddressBalance {
                balance: 400,
                occupied_capacity: 400,
                live_cells_count: 2,
                total_cells_count: 2,
                txs_count: 0,
                first_seen_block: 1,
                first_seen_tx: vec![0x10; 32],
                last_activity_block: 2,
                last_activity_tx: vec![0x20; 32],
            },
        );
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        let rebuilt = store.get_addr_balance(&lock_hash).unwrap().unwrap();
        assert_eq!(rebuilt.balance, 100);
        assert_eq!(rebuilt.occupied_capacity, 100);
        assert_eq!(rebuilt.live_cells_count, 1);
    }

    #[test]
    fn test_rollback_restores_consumed_cells_after_fork_point() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_hash = vec![0xAB; 32];
        let tx_hash = vec![0x42; 32];

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let cell = LiveCellInfo {
            capacity: 500,
            created_at_block: 1,
            lock_script_hash: lock_hash,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 500,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&tx_hash, 0, &cell);
        batch.commit().unwrap();

        // Simulate consumption in block 2.
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, 0, &cell, 2);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        assert!(store.get_cell(&tx_hash, 0).unwrap().is_none());
        assert!(store.get_consumed_cell(&tx_hash, 0).unwrap().is_some());

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&tx_hash, 0).unwrap().is_some());

        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        let consumed_raw = store
            .get_cf(store.cf_consumed_cells(), &outpoint_key)
            .unwrap();
        assert!(consumed_raw.is_none());
    }
}
