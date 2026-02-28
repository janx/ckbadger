//! Reorg (rollback) operations.

use rocksdb::{IteratorMode, WriteBatch};
use std::time::{Duration, Instant};
use tracing::info;

use crate::keys;
use crate::store::*;
use crate::types::*;

fn parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd: &[u8]) -> anyhow::Result<u32> {
    let cutoff_str = std::str::from_utf8(cutoff_yyyymmdd)
        .map_err(|e| anyhow::anyhow!("invalid cutoff date utf8 {:?}: {}", cutoff_yyyymmdd, e))?;
    cutoff_str
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("invalid cutoff date '{}': {}", cutoff_str, e))
}

fn should_delete_stats_for_replay(key: &[u8], cutoff_yyyymmdd: &[u8]) -> anyhow::Result<bool> {
    if key.is_empty() {
        return Ok(false);
    }
    let prefix = key[0];
    let suffix = &key[1..];

    match prefix {
        // date scoped: YYYYMMDD
        keys::STATS_PREFIX_DAILY
        | keys::STATS_PREFIX_DAILY_BLOCK
        | keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT
        | keys::STATS_PREFIX_HODL_WAVE => Ok(suffix.len() >= 8 && &suffix[..8] >= cutoff_yyyymmdd),
        // hour scoped: YYYYMMDDHH
        keys::STATS_PREFIX_HOURLY => Ok(suffix.len() >= 10 && &suffix[..8] >= cutoff_yyyymmdd),
        // date+miner hash: YYYYMMDD + 32-byte lock hash
        keys::STATS_PREFIX_MINER => Ok(suffix.len() >= 40 && &suffix[..8] >= cutoff_yyyymmdd),
        // code_hash(32) + kind(1) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_SCRIPT_DAILY => {
            let cutoff_date = parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd)?;
            if suffix.len() < 37 {
                return Ok(false);
            }
            let date = u32::from_be_bytes(suffix[33..37].try_into().map_err(|_| {
                anyhow::anyhow!("invalid script_daily suffix length: {}", suffix.len())
            })?);
            Ok(date >= cutoff_date)
        }
        // type_hash(32) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_TOKEN_DAILY => {
            let cutoff_date = parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd)?;
            if suffix.len() < 36 {
                return Ok(false);
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().map_err(|_| {
                anyhow::anyhow!("invalid token_daily suffix length: {}", suffix.len())
            })?);
            Ok(date >= cutoff_date)
        }
        // cluster_id(32) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_CLUSTER_DAILY => {
            let cutoff_date = parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd)?;
            if suffix.len() < 36 {
                return Ok(false);
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().map_err(|_| {
                anyhow::anyhow!("invalid cluster_daily suffix length: {}", suffix.len())
            })?);
            Ok(date >= cutoff_date)
        }
        // spore_id(32) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_SPORE_DAILY => {
            let cutoff_date = parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd)?;
            if suffix.len() < 36 {
                return Ok(false);
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().map_err(|_| {
                anyhow::anyhow!("invalid spore_daily suffix length: {}", suffix.len())
            })?);
            Ok(date >= cutoff_date)
        }
        // collection_id(32 padded) + date(4B u32 YYYYMMDD BE)
        keys::STATS_PREFIX_NFT_DAILY => {
            let cutoff_date = parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd)?;
            if suffix.len() < 36 {
                return Ok(false);
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().map_err(|_| {
                anyhow::anyhow!("invalid nft_daily suffix length: {}", suffix.len())
            })?);
            Ok(date >= cutoff_date)
        }
        _ => Ok(false),
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
            let elapsed_secs = self.started_at.elapsed().as_secs_f64();
            let scanned_per_sec = if elapsed_secs > 0.0 {
                self.scanned as f64 / elapsed_secs
            } else {
                0.0
            };
            info!(
                stage = self.stage,
                scanned = self.scanned,
                affected,
                elapsed_secs = format!("{:.1}", elapsed_secs),
                scanned_per_sec = format!("{:.1}", scanned_per_sec),
                "Rollback cleanup in progress"
            );
            self.last_log_at = Instant::now();
        }
    }

    fn finish(&self, affected: u64) {
        let elapsed_secs = self.started_at.elapsed().as_secs_f64();
        let scanned_per_sec = if elapsed_secs > 0.0 {
            self.scanned as f64 / elapsed_secs
        } else {
            0.0
        };
        info!(
            stage = self.stage,
            scanned = self.scanned,
            affected,
            elapsed_secs = format!("{:.1}", elapsed_secs),
            scanned_per_sec = format!("{:.1}", scanned_per_sec),
            "Rollback cleanup stage complete"
        );
    }
}

fn clear_dao_withdraw_request_fields(entry: &mut DaoDepositCacheEntry) {
    entry.withdraw_request_tx = None;
    entry.withdraw_request_output_index = None;
    entry.withdraw_request_block = None;
    entry.withdraw_request_ar = None;
}

fn clear_dao_withdraw_completion_fields(entry: &mut DaoDepositCacheEntry) {
    entry.withdraw_block = None;
    entry.withdraw_tx = None;
    entry.withdraw_to_output_index = None;
    entry.compensation = None;
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

fn normalize_dao_entry_for_rollback(
    entry: &mut DaoDepositCacheEntry,
    rollback_to: i64,
) -> anyhow::Result<bool> {
    let mut changed = false;

    if let Some(request_block) = entry.withdraw_request_block {
        if request_block > rollback_to {
            if entry.status != 0 {
                entry.status = 0;
                changed = true;
            }
            if entry.withdraw_request_tx.is_some()
                || entry.withdraw_request_output_index.is_some()
                || entry.withdraw_request_block.is_some()
                || entry.withdraw_request_ar.is_some()
            {
                clear_dao_withdraw_request_fields(entry);
                changed = true;
            }
            if entry.withdraw_block.is_some()
                || entry.withdraw_tx.is_some()
                || entry.compensation.is_some()
            {
                clear_dao_withdraw_completion_fields(entry);
                changed = true;
            }
        }
    }

    if let Some(withdraw_block) = entry.withdraw_block {
        if withdraw_block > rollback_to {
            if entry.status != 1 {
                entry.status = 1;
                changed = true;
            }
            if entry.withdraw_block.is_some()
                || entry.withdraw_tx.is_some()
                || entry.compensation.is_some()
            {
                clear_dao_withdraw_completion_fields(entry);
                changed = true;
            }
        }
    }

    match entry.status {
        0 => {
            if entry.withdraw_request_tx.is_some()
                || entry.withdraw_request_output_index.is_some()
                || entry.withdraw_request_block.is_some()
                || entry.withdraw_request_ar.is_some()
            {
                clear_dao_withdraw_request_fields(entry);
                changed = true;
            }
            if entry.withdraw_block.is_some()
                || entry.withdraw_tx.is_some()
                || entry.compensation.is_some()
            {
                clear_dao_withdraw_completion_fields(entry);
                changed = true;
            }
        }
        1 => {
            if entry.withdraw_request_tx.is_none()
                || entry.withdraw_request_output_index.is_none()
                || entry.withdraw_request_block.is_none()
            {
                anyhow::bail!(
                    "inconsistent DAO entry after rollback normalization: status=1 missing request fields, deposit_block={}",
                    entry.deposit_block_number
                );
            }
            if entry.withdraw_block.is_some()
                || entry.withdraw_tx.is_some()
                || entry.compensation.is_some()
            {
                clear_dao_withdraw_completion_fields(entry);
                changed = true;
            }
        }
        2 => {
            if entry.withdraw_request_tx.is_none()
                || entry.withdraw_request_output_index.is_none()
                || entry.withdraw_request_block.is_none()
            {
                anyhow::bail!(
                    "inconsistent DAO entry after rollback normalization: status=2 missing request fields, deposit_block={}",
                    entry.deposit_block_number
                );
            }
            if entry.withdraw_block.is_none()
                || entry.withdraw_tx.is_none()
                || entry.compensation.is_none()
            {
                anyhow::bail!(
                    "inconsistent DAO entry after rollback normalization: status=2 missing completion fields, deposit_block={}",
                    entry.deposit_block_number
                );
            }
        }
        other => {
            anyhow::bail!(
                "invalid DAO status in rollback normalization: status={}, deposit_block={}",
                other,
                entry.deposit_block_number
            );
        }
    }

    Ok(changed)
}

impl CkbadgerStore {
    /// Atomic rollback across all CFs to a given block number.
    /// Deletes all data for blocks > rollback_to.
    pub fn rollback_to_block(&self, rollback_to: i64) -> anyhow::Result<RollbackResult> {
        if rollback_to < -1 {
            anyhow::bail!(
                "invalid rollback target: rollback_to={} (expected >= -1)",
                rollback_to
            );
        }
        // Persist a rollback marker so startup can force cleanup if interrupted.
        self.set_rollback_cleanup_in_progress(true)?;
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

        // 1. Delete block index entries > rollback_to (append store data stays)
        let mut stage = RollbackStageProgress::new("delete_block_index");
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_block_index(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter.flatten() {
            let (key, _hash) = item;
            if key.len() == 8 {
                batch.delete_cf(self.cf_block_index(), &key);
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
        // cf_live_cells() stores empty liveness markers; cell data is in cf_cells() (append store).
        let mut stage = RollbackStageProgress::new("delete_live_cells_after_tip");
        let iter = self.iterator_cf(self.cf_live_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _marker) = item;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                stage.tick(cells_removed);
                continue;
            }
            let cell_data = self.append_get_cf(self.cf_cells(), &key)?;
            let info = match cell_data {
                Some(ref bytes) => match bincode::deserialize::<LiveCellInfo>(bytes) {
                    Ok(info) => info,
                    Err(_) => {
                        stage.tick(cells_removed);
                        continue;
                    }
                },
                None => {
                    stage.tick(cells_removed);
                    continue;
                }
            };
            if info.created_at_block > rollback_to {
                batch.delete_cf(self.cf_live_cells(), &key);
                cells_removed += 1;

                // Clean up indexes
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
            stage.tick(cells_removed);
        }
        stage.finish(cells_removed);

        // 4. Restore consumed cells that were consumed after rollback_to.
        //
        // cf_consumed_cells() stores 40-byte metadata (consumed_at_block + consumed_by_tx).
        // Full cell data lives in cf_cells() (append store).
        let mut stage = RollbackStageProgress::new("restore_consumed_cells");
        let iter = self.iterator_cf(self.cf_consumed_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                stage.tick(cells_restored);
                continue;
            }
            if value.len() < keys::CONSUMED_CELL_VALUE_SIZE {
                stage.tick(cells_restored);
                continue;
            }
            let (consumed_at_block, _consumed_by_tx) = keys::decode_consumed_cell_value(&value);
            if consumed_at_block <= rollback_to {
                stage.tick(cells_restored);
                continue;
            }

            // Remove stale consumed record from rolled-back blocks.
            batch.delete_cf(self.cf_consumed_cells(), &key);

            // Get the cell data from append store to check created_at_block and rebuild indexes.
            let cell_data = self.append_get_cf(self.cf_cells(), &key)?;
            let cell = match cell_data {
                Some(ref bytes) => match bincode::deserialize::<LiveCellInfo>(bytes) {
                    Ok(info) => info,
                    Err(_) => {
                        stage.tick(cells_restored);
                        continue;
                    }
                },
                None => {
                    stage.tick(cells_restored);
                    continue;
                }
            };

            // If the cell itself existed before rollback point, restore it to live_cells.
            if cell.created_at_block <= rollback_to {
                // Restore liveness marker (empty value).
                batch.put_cf(self.cf_live_cells(), &key, &[] as &[u8]);
                cells_restored += 1;

                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                let idx_key = keys::encode_cell_index_key(
                    &cell.lock_script_hash,
                    cell.created_at_block,
                    &tx_hash,
                    output_index,
                );
                batch.put_cf(self.cf_cell_by_lock(), &idx_key, []);
                let idx_key = keys::encode_cell_index_key(
                    &cell.lock_code_hash,
                    cell.created_at_block,
                    &tx_hash,
                    output_index,
                );
                batch.put_cf(self.cf_cell_by_lock_code(), &idx_key, []);
                if let Some(ref type_hash) = cell.type_script_hash {
                    let idx_key = keys::encode_cell_index_key(
                        type_hash,
                        cell.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    batch.put_cf(self.cf_cell_by_type(), &idx_key, []);
                }
                if let Some(ref type_code_hash) = cell.type_code_hash {
                    let idx_key = keys::encode_cell_index_key(
                        type_code_hash,
                        cell.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    batch.put_cf(self.cf_cell_by_type_code(), &idx_key, []);
                }
            }
            stage.tick(cells_restored);
        }
        stage.finish(cells_restored);

        // 5. Repair DAO deposits across the rollback boundary.
        let mut dao_deposits_deleted = 0u64;
        let mut dao_deposits_repaired = 0u64;
        let mut stage = RollbackStageProgress::new("repair_dao_deposits");
        let iter = self.iterator_cf(self.cf_dao_deposits(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            let mut entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao_deposit during rollback: outpoint=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;

            if entry.deposit_block_number > rollback_to {
                batch.delete_cf(self.cf_dao_deposits(), &key);
                dao_deposits_deleted += 1;
                stage.tick(dao_deposits_deleted + dao_deposits_repaired);
                continue;
            }

            let changed =
                normalize_dao_entry_for_rollback(&mut entry, rollback_to).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to normalize dao_deposit during rollback: outpoint=0x{}, {}",
                        bytes_to_hex(&key),
                        e
                    )
                })?;
            if changed {
                let encoded = bincode::serialize(&entry).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to serialize repaired dao_deposit during rollback: outpoint=0x{}, error={}",
                        bytes_to_hex(&key),
                        e
                    )
                })?;
                batch.put_cf(self.cf_dao_deposits(), &key, &encoded);
                dao_deposits_repaired += 1;
            }
            stage.tick(dao_deposits_deleted + dao_deposits_repaired);
        }
        stage.finish(dao_deposits_deleted + dao_deposits_repaired);

        // 6. Rebuild dao_by_withdraw_tx index from repaired dao_deposits.
        let mut dao_withdraw_index_deleted = 0u64;
        let mut stage = RollbackStageProgress::new("rebuild_dao_withdraw_index_clear");
        let iter = self.iterator_cf(self.cf_dao_by_withdraw_tx(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            batch.delete_cf(self.cf_dao_by_withdraw_tx(), &key);
            dao_withdraw_index_deleted += 1;
            stage.tick(dao_withdraw_index_deleted);
        }
        stage.finish(dao_withdraw_index_deleted);

        let mut dao_withdraw_index_rebuilt = 0u64;
        let mut stage = RollbackStageProgress::new("rebuild_dao_withdraw_index_fill");
        let iter = self.iterator_cf(self.cf_dao_deposits(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao_deposit while rebuilding dao_by_withdraw_tx: outpoint=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;

            if entry.status >= 1 {
                let request_block = entry.withdraw_request_block.ok_or_else(|| {
                    anyhow::anyhow!(
                        "dao deposit missing withdraw_request_block while rebuilding dao_by_withdraw_tx: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
                let request_tx = entry.withdraw_request_tx.ok_or_else(|| {
                    anyhow::anyhow!(
                        "dao deposit missing withdraw_request_tx while rebuilding dao_by_withdraw_tx: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
                if request_block <= rollback_to {
                    batch.put_cf(self.cf_dao_by_withdraw_tx(), &request_tx, &key);
                    dao_withdraw_index_rebuilt += 1;
                }
            }
            stage.tick(dao_withdraw_index_rebuilt);
        }
        stage.finish(dao_withdraw_index_rebuilt);

        // 7. Delete date-scoped stats entries from replay cutoff date onward.
        // These are additive snapshots and would be double-counted after replay.
        if let Some(cutoff) = replay_cutoff_date.as_deref() {
            let mut stats_removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_stats_from_cutoff");
            let iter = self.iterator_cf(self.cf_stats(), IteratorMode::Start);
            for item in iter.flatten() {
                let (key, _) = item;
                if should_delete_stats_for_replay(&key, cutoff.as_bytes())? {
                    batch.delete_cf(self.cf_stats(), &key);
                    stats_removed += 1;
                }
                stage.tick(stats_removed);
            }
            stage.finish(stats_removed);
        }

        // 8. Delete block issuance > rollback_to
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

        // 9. Delete addr_txs entries > rollback_to
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

        // 10. Delete addr_activities entries > rollback_to
        // Key: lock_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        // Activities data in append store (cf_activities) is immutable; only clean the index.
        let mut activities_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_addr_activities");
        let iter = self.iterator_cf(self.cf_addr_activities(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let (_, block_num, _) = keys::decode_activity_key(&key);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_addr_activities(), &key);
                    activities_removed += 1;
                }
            }
            stage.tick(activities_removed);
        }
        stage.finish(activities_removed);

        // 11. Delete token_transfers entries > rollback_to
        // Key: type_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        let mut token_transfers_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_token_transfers");
        let iter = self.iterator_cf(self.cf_ft_activities(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == 44 {
                let (block_num, _) = keys::decode_token_transfer_key(&key);
                if block_num > rollback_to {
                    batch.delete_cf(self.cf_ft_activities(), &key);
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

        // Rebuild script usage aggregates from live/consumed cells so script_info
        // remains consistent after startup rollback/reorg replay.
        info!("Rollback cleanup rebuilding script_info from cells");
        let rebuilt_script_infos = self.rebuild_script_infos_from_cells()?;
        info!(
            rebuilt_script_infos,
            elapsed_secs = format!("{:.1}", rollback_started_at.elapsed().as_secs_f64()),
            "Rollback cleanup script info rebuild complete"
        );

        // Rebuild token state from transfer history to heal partial UDT writes
        // that can survive crash windows before block-header commit markers.
        info!("Rollback cleanup rebuilding token state from token_transfers");
        let token_rebuild = self.rebuild_token_state_from_transfers()?;
        info!(
            token_holders_cleared = token_rebuild.token_holders_cleared,
            token_transfer_stats_cleared = token_rebuild.token_transfer_stats_cleared,
            token_hourly_stats_cleared = token_rebuild.token_hourly_stats_cleared,
            tokens_written = token_rebuild.tokens_written,
            tokens_deleted = token_rebuild.tokens_deleted,
            token_holders_written = token_rebuild.token_holders_written,
            token_transfer_stats_written = token_rebuild.token_transfer_stats_written,
            token_hourly_stats_written = token_rebuild.token_hourly_stats_written,
            elapsed_secs = format!("{:.1}", rollback_started_at.elapsed().as_secs_f64()),
            "Rollback cleanup token state rebuild complete"
        );

        // Token daily deltas are date-scoped stats and are already truncated from
        // replay cutoff onward in stage 7. Keep rollback cleanup single-pass and
        // avoid full refill/rebuild from cells; replay will write fresh deltas.

        // Keep sync_status tip aligned with the rolled-back chain head.
        let tip_hash = if rollback_to >= 0 {
            self.get_block_header(rollback_to)?
                .map(|h| h.hash)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let tip_number = if rollback_to < 0 { 0 } else { rollback_to };
        self.update_sync_status(|status| {
            status.tip_block_number = tip_number;
            status.tip_block_hash = tip_hash.clone();
            status.last_synced_at = chrono::Utc::now().timestamp();
        })?;

        self.set_rollback_cleanup_in_progress(false)?;

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
    use crate::types::{
        AddressBalance, CachedBlockHeader, DaoDepositCacheEntry, LiveCellInfo, ScriptInfo,
        TokenDailyDelta, TokenInfo, TokenTransferRecord,
    };

    #[test]
    fn test_should_delete_stats_for_replay_daily_prefix() {
        let cutoff = b"20260210";
        let key = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260211");
        assert!(should_delete_stats_for_replay(&key, cutoff).unwrap());

        let key_old = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260209");
        assert!(!should_delete_stats_for_replay(&key_old, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_hourly_and_miner_prefix() {
        let cutoff = b"20260210";
        let hourly = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_HOURLY, b"2026021001");
        assert!(should_delete_stats_for_replay(&hourly, cutoff).unwrap());

        let miner_suffix = [b"20260210".as_slice(), &[0xAA; 32]].concat();
        let miner = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_MINER, &miner_suffix);
        assert!(should_delete_stats_for_replay(&miner, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_script_daily_prefix() {
        let cutoff = b"20260210";
        let code_hash = [0xAA; 32];

        let new_key = crate::keys::encode_script_daily_key(&code_hash, false, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff).unwrap());

        let old_key = crate::keys::encode_script_daily_key(&code_hash, true, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_token_daily_prefix() {
        let cutoff = b"20260210";
        let type_hash = [0xBB; 32];

        let new_key = crate::keys::encode_token_daily_key(&type_hash, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff).unwrap());

        let old_key = crate::keys::encode_token_daily_key(&type_hash, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_cluster_daily_prefix() {
        let cutoff = b"20260210";
        let cluster_id = [0xCC; 32];

        let new_key = crate::keys::encode_cluster_daily_key(&cluster_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff).unwrap());

        let old_key = crate::keys::encode_cluster_daily_key(&cluster_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_spore_daily_prefix() {
        let cutoff = b"20260210";
        let spore_id = [0xDD; 32];

        let new_key = crate::keys::encode_spore_daily_key(&spore_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff).unwrap());

        let old_key = crate::keys::encode_spore_daily_key(&spore_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_nft_daily_prefix() {
        let cutoff = b"20260210";
        let collection_id = [0xEE; 24];

        let new_key = crate::keys::encode_nft_daily_key(&collection_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff).unwrap());

        let old_key = crate::keys::encode_nft_daily_key(&collection_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_errors_on_invalid_cutoff_date() {
        let cutoff = b"invalid-cutoff";
        let code_hash = [0xAA; 32];
        let key = crate::keys::encode_script_daily_key(&code_hash, false, 20260211);
        let err = should_delete_stats_for_replay(&key, cutoff).unwrap_err();
        assert!(err.to_string().contains("invalid cutoff date"));
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
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
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
            udt_amount: None,
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
            udt_amount: None,
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
    fn test_rollback_rebuilds_script_info_from_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_code_hash = vec![0x7A; 32];

        let header1 = CachedBlockHeader {
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
            hash: vec![0x02; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let keep_live = LiveCellInfo {
            capacity: 100,
            created_at_block: 1,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: lock_code_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 60,
            udt_amount: None,
        };
        let rollback_live = LiveCellInfo {
            capacity: 300,
            created_at_block: 2,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: lock_code_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 180,
            udt_amount: None,
        };

        let stale_script_info = ScriptInfo {
            code_hash: lock_code_hash.clone(),
            hash_type: 1,
            name: Some("Rollback Script".to_string()),
            lock_cells_count: 99,
            lock_live_cells_count: 99,
            lock_capacity_sum: 9_999,
            lock_live_capacity_sum: 9_999,
            lock_occupied_capacity_sum: 8_888,
            lock_live_occupied_capacity_sum: 8_888,
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x10; 32], 0, &keep_live);
        batch.put_cell(&[0x20; 32], 0, &rollback_live);
        batch.put_script_info(&lock_code_hash, &stale_script_info);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        let rebuilt = store.get_script_info(&lock_code_hash).unwrap().unwrap();
        assert_eq!(rebuilt.name.as_deref(), Some("Rollback Script"));
        assert_eq!(rebuilt.hash_type, 1);
        assert_eq!(rebuilt.lock_cells_count, 1);
        assert_eq!(rebuilt.lock_live_cells_count, 1);
        assert_eq!(rebuilt.lock_capacity_sum, 100);
        assert_eq!(rebuilt.lock_live_capacity_sum, 100);
        assert_eq!(rebuilt.lock_occupied_capacity_sum, 60);
        assert_eq!(rebuilt.lock_live_occupied_capacity_sum, 60);
    }

    #[test]
    fn test_rollback_restores_consumed_cells_after_fork_point() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_hash = vec![0xAB; 32];
        let tx_hash = vec![0x42; 32];

        let header1 = CachedBlockHeader {
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
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
            udt_amount: None,
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

    #[test]
    fn test_rollback_repairs_dao_deposits_and_withdraw_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
            hash: vec![0x02; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header3 = CachedBlockHeader {
            block_number: 3,
            hash: vec![0x03; 32],
            timestamp: 1_700_000_020_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };

        let tx_a = vec![0xAA; 32];
        let tx_b = vec![0xBB; 32];
        let tx_c = vec![0xCC; 32];
        let request_tx_a = vec![0x11; 32];
        let request_tx_b = vec![0x22; 32];
        let orphan_request_tx = vec![0x33; 32];

        let outpoint_a = keys::encode_outpoint(&tx_a, 0);
        let outpoint_b = keys::encode_outpoint(&tx_b, 0);
        let outpoint_c = keys::encode_outpoint(&tx_c, 0);
        let orphan_outpoint = keys::encode_outpoint(&[0xFF; 32], 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_block_header(3, &header3);

        // request_block > rollback target, should revert to status=0.
        batch.put_dao_deposit(
            &outpoint_a,
            &DaoDepositCacheEntry {
                capacity: 100,
                deposit_block_number: 1,
                lock_script_hash: vec![0xA1; 32],
                deposit_ar: 1,
                status: 1,
                withdraw_request_tx: Some(request_tx_a.clone()),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(2),
                withdraw_request_ar: Some(1),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_by_withdraw_tx(&request_tx_a, &outpoint_a);

        // withdraw_block > rollback target but request_block <= rollback target,
        // should revert to status=1 and keep withdraw_request mapping.
        batch.put_dao_deposit(
            &outpoint_b,
            &DaoDepositCacheEntry {
                capacity: 200,
                deposit_block_number: 1,
                lock_script_hash: vec![0xB1; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(request_tx_b.clone()),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(1),
                withdraw_request_ar: Some(1),
                withdraw_block: Some(3),
                withdraw_tx: Some(vec![0x44; 32]),
                withdraw_to_output_index: Some(0),
                compensation: Some(10),
            },
        );
        batch.put_dao_by_withdraw_tx(&request_tx_b, &outpoint_b);

        // deposit block > rollback target, should be deleted.
        batch.put_dao_deposit(
            &outpoint_c,
            &DaoDepositCacheEntry {
                capacity: 300,
                deposit_block_number: 2,
                lock_script_hash: vec![0xC1; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        // Orphan mapping should be cleared during index rebuild.
        batch.put_dao_by_withdraw_tx(&orphan_request_tx, &orphan_outpoint);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        let repaired_a = store.get_dao_deposit(&outpoint_a).unwrap().unwrap();
        assert_eq!(repaired_a.status, 0);
        assert!(repaired_a.withdraw_request_block.is_none());
        assert!(repaired_a.withdraw_request_tx.is_none());
        assert!(repaired_a.withdraw_block.is_none());
        assert!(repaired_a.compensation.is_none());

        let repaired_b = store.get_dao_deposit(&outpoint_b).unwrap().unwrap();
        assert_eq!(repaired_b.status, 1);
        assert_eq!(repaired_b.withdraw_request_block, Some(1));
        assert_eq!(repaired_b.withdraw_request_tx, Some(request_tx_b.clone()));
        assert!(repaired_b.withdraw_block.is_none());
        assert!(repaired_b.withdraw_tx.is_none());
        assert!(repaired_b.compensation.is_none());

        assert!(store.get_dao_deposit(&outpoint_c).unwrap().is_none());
        assert!(store
            .get_dao_deposit_by_withdraw_tx(&request_tx_a)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_dao_deposit_by_withdraw_tx(&request_tx_b)
                .unwrap()
                .unwrap(),
            outpoint_b
        );
        assert!(store
            .get_dao_deposit_by_withdraw_tx(&orphan_request_tx)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_to_block_errors_when_target_below_minus_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let err = store.rollback_to_block(-2).unwrap_err();
        assert!(err.to_string().contains("expected >= -1"));
    }

    #[test]
    fn test_rollback_rebuilds_token_state_from_transfers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
            hash: vec![0x02; 32],
            timestamp: 1_700_003_600_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };

        let type_hash = vec![0xCD; 32];
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        let stale_token_info = TokenInfo {
            type_code_hash: vec![0x11; 32],
            hash_type: 1,
            type_args: vec![0x22; 32],
            standard: "sudt".to_string(),
            name: Some("Test Token".to_string()),
            symbol: Some("TT".to_string()),
            decimals: Some(8),
            total_supply: Some(100),
            max_supply: Some(1_000),
            holders_count: 2, // stale (contains block2 state)
            first_seen_block: 1,
            icon_url: None,
            description: None,
            transfers_count: 2, // stale (contains block2 state)
        };

        let transfer_block_1 = TokenTransferRecord {
            tx_hash: vec![0x10; 32],
            block_number: 1,
            from_lock_hash: None,
            to_lock_hash: lock_a.clone(),
            amount: 100,
            is_mint: true,
            is_burn: false,
            timestamp: header1.timestamp,
        };
        let transfer_block_2 = TokenTransferRecord {
            tx_hash: vec![0x20; 32],
            block_number: 2,
            from_lock_hash: Some(lock_a.clone()),
            to_lock_hash: lock_b.clone(),
            amount: 60,
            is_mint: false,
            is_burn: false,
            timestamp: header2.timestamp,
        };
        let live_cell_block_1 = LiveCellInfo {
            capacity: 1000,
            created_at_block: 1,
            lock_script_hash: lock_a.clone(),
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 1000,
            udt_amount: Some(100),
        };
        let live_cell_block_2 = LiveCellInfo {
            capacity: 1000,
            created_at_block: 2,
            lock_script_hash: lock_b.clone(),
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 1000,
            udt_amount: Some(60),
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x31; 32], 0, &live_cell_block_1);
        batch.put_cell(&[0x32; 32], 0, &live_cell_block_2);
        batch.put_token(&type_hash, &stale_token_info);
        batch.put_token_holder(&type_hash, &lock_a, 40);
        batch.put_token_holder(&type_hash, &lock_b, 60);
        batch.put_token_transfers_count(&type_hash, 2);
        batch.put_token_hourly_transfer(&type_hash, header1.timestamp / 3_600_000, 1);
        batch.put_token_hourly_transfer(&type_hash, header2.timestamp / 3_600_000, 1);
        batch.put_token_transfer(&type_hash, 1, 0, &transfer_block_1);
        batch.put_token_transfer(&type_hash, 2, 0, &transfer_block_2);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        // Transfer history should be truncated to block 1.
        let transfers = store.list_token_transfers(&type_hash, 10, None).unwrap();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].0, 1);

        // Holder balances should match only block 1 mint.
        assert_eq!(
            store.get_token_holder_balance(&type_hash, &lock_a).unwrap(),
            Some(100)
        );
        assert_eq!(
            store.get_token_holder_balance(&type_hash, &lock_b).unwrap(),
            None
        );

        // Token aggregates should be rebuilt from the truncated transfer history.
        let rebuilt_token = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(rebuilt_token.total_supply, Some(100));
        assert_eq!(rebuilt_token.holders_count, 1);
        assert_eq!(rebuilt_token.transfers_count, 1);
        assert_eq!(rebuilt_token.first_seen_block, 1);
        assert_eq!(rebuilt_token.name.as_deref(), Some("Test Token"));
        assert_eq!(rebuilt_token.max_supply, Some(1_000));

        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 1);
        assert_eq!(
            store
                .get_token_24h_transfers(&type_hash, header2.timestamp)
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_rollback_does_not_rebuild_token_daily_deltas_from_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let day1_ts = 1_704_067_200_000i64; // 2024-01-01T00:00:00Z
        let day2_ts = 1_704_153_600_000i64; // 2024-01-02T00:00:00Z
        let day1 = keys::timestamp_ms_to_date(day1_ts);
        let day2 = keys::timestamp_ms_to_date(day2_ts);
        let type_hash = vec![0x44; 32];

        let header1 = CachedBlockHeader {
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: day1_ts,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
            hash: vec![0x02; 32],
            timestamp: day2_ts,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };

        let keep_cell = LiveCellInfo {
            capacity: 1_000,
            created_at_block: 1,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 600,
            udt_amount: Some(1),
        };
        let rollback_cell = LiveCellInfo {
            capacity: 400,
            created_at_block: 2,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 300,
            udt_amount: Some(1),
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x10; 32], 0, &keep_cell);
        batch.put_cell(&[0x20; 32], 0, &rollback_cell);
        batch.put_token(
            &type_hash,
            &TokenInfo {
                type_code_hash: vec![0x11; 32],
                hash_type: 1,
                type_args: vec![0x22; 32],
                standard: "sudt".to_string(),
                name: None,
                symbol: None,
                decimals: None,
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        );
        batch.commit().unwrap();

        store
            .put_token_daily_delta(
                &type_hash,
                day1,
                &TokenDailyDelta {
                    live_capacity_delta: 100,
                    live_occupied_capacity_delta: 200,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                day2,
                &TokenDailyDelta {
                    live_capacity_delta: 50,
                    live_occupied_capacity_delta: 50,
                },
            )
            .unwrap();
        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_some());

        store.rollback_to_block(1).unwrap();

        let deltas = store.list_token_daily_deltas(&type_hash).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].0, day1);
        assert_eq!(deltas[0].1.live_capacity_delta, 100);
        assert_eq!(deltas[0].1.live_occupied_capacity_delta, 200);
        assert!(store
            .get_token_daily_delta(&type_hash, day2)
            .unwrap()
            .is_none());
        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_rollback_deletes_token_without_remaining_transfers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            block_number: 1,
            hash: vec![0x01; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            block_number: 2,
            hash: vec![0x02; 32],
            timestamp: 1_700_003_600_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };

        let type_hash = vec![0xEF; 32];
        let lock_a = vec![0xAA; 32];

        let token_info = TokenInfo {
            type_code_hash: vec![0x11; 32],
            hash_type: 1,
            type_args: vec![0x22; 32],
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            total_supply: Some(50),
            max_supply: None,
            holders_count: 1,
            first_seen_block: 2,
            icon_url: None,
            description: None,
            transfers_count: 1,
        };
        let transfer_block_2 = TokenTransferRecord {
            tx_hash: vec![0x20; 32],
            block_number: 2,
            from_lock_hash: None,
            to_lock_hash: lock_a.clone(),
            amount: 50,
            is_mint: true,
            is_burn: false,
            timestamp: header2.timestamp,
        };
        let live_cell_block_2 = LiveCellInfo {
            capacity: 1000,
            created_at_block: 2,
            lock_script_hash: lock_a.clone(),
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 1000,
            udt_amount: Some(50),
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x33; 32], 0, &live_cell_block_2);
        batch.put_token(&type_hash, &token_info);
        batch.put_token_holder(&type_hash, &lock_a, 50);
        batch.put_token_transfers_count(&type_hash, 1);
        batch.put_token_hourly_transfer(&type_hash, header2.timestamp / 3_600_000, 1);
        batch.put_token_transfer(&type_hash, 2, 0, &transfer_block_2);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_token(&type_hash).unwrap().is_none());
        assert_eq!(
            store.get_token_holder_balance(&type_hash, &lock_a).unwrap(),
            None
        );
        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 0);
        assert!(store
            .list_token_transfers(&type_hash, 10, None)
            .unwrap()
            .is_empty());
    }
}
