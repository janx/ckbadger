//! Reorg (rollback) operations.

use rocksdb::{IteratorMode, WriteBatch};
use std::collections::{HashMap, HashSet};
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
const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";
const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";

fn should_log_rollback_progress(scanned: u64, since_last_log: Duration) -> bool {
    scanned > 0
        && scanned.is_multiple_of(ROLLBACK_PROGRESS_CHECK_EVERY)
        && since_last_log >= ROLLBACK_PROGRESS_MIN_INTERVAL
}

fn delete_cell_index_entries(
    store: &CkbadgerStore,
    batch: &mut WriteBatch,
    cell: &LiveCellInfo,
    tx_hash: &[u8],
    output_index: i16,
) {
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_script_hash,
        cell.created_at_block,
        tx_hash,
        output_index,
    );
    batch.delete_cf(store.cf_cell_by_lock(), &idx_key);
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_code_hash,
        cell.created_at_block,
        tx_hash,
        output_index,
    );
    batch.delete_cf(store.cf_cell_by_lock_code(), &idx_key);
    if let Some(ref type_hash) = cell.type_script_hash {
        let idx_key =
            keys::encode_cell_index_key(type_hash, cell.created_at_block, tx_hash, output_index);
        batch.delete_cf(store.cf_cell_by_type(), &idx_key);
    }
    if let Some(ref type_code_hash) = cell.type_code_hash {
        let idx_key = keys::encode_cell_index_key(
            type_code_hash,
            cell.created_at_block,
            tx_hash,
            output_index,
        );
        batch.delete_cf(store.cf_cell_by_type_code(), &idx_key);
    }
}

fn put_cell_index_entries(
    store: &CkbadgerStore,
    batch: &mut WriteBatch,
    cell: &LiveCellInfo,
    tx_hash: &[u8],
    output_index: i16,
) {
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_script_hash,
        cell.created_at_block,
        tx_hash,
        output_index,
    );
    batch.put_cf(store.cf_cell_by_lock(), &idx_key, []);
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_code_hash,
        cell.created_at_block,
        tx_hash,
        output_index,
    );
    batch.put_cf(store.cf_cell_by_lock_code(), &idx_key, []);
    if let Some(ref type_hash) = cell.type_script_hash {
        let idx_key =
            keys::encode_cell_index_key(type_hash, cell.created_at_block, tx_hash, output_index);
        batch.put_cf(store.cf_cell_by_type(), &idx_key, []);
    }
    if let Some(ref type_code_hash) = cell.type_code_hash {
        let idx_key = keys::encode_cell_index_key(
            type_code_hash,
            cell.created_at_block,
            tx_hash,
            output_index,
        );
        batch.put_cf(store.cf_cell_by_type_code(), &idx_key, []);
    }
}

fn load_tx_contexts_from_undo_log(
    store: &CkbadgerStore,
    rollback_to: i64,
) -> anyhow::Result<Vec<UndoTxContext>> {
    let start_key = keys::encode_block_num(rollback_to + 1);
    let iter = store.iterator_cf(
        store.cf_reorg_undo_log_by_block(),
        IteratorMode::From(&start_key, rocksdb::Direction::Forward),
    );
    let mut contexts = Vec::new();
    for item in iter {
        let (key, value) = item.map_err(|e| {
            anyhow::anyhow!(
                "failed to iterate reorg_undo_log_by_block while loading tx contexts: {}",
                e
            )
        })?;
        if key.len() != keys::REORG_UNDO_LOG_KEY_SIZE {
            anyhow::bail!(
                "invalid reorg_undo_log_by_block key length while loading tx contexts: expected={}, got={}",
                keys::REORG_UNDO_LOG_KEY_SIZE,
                key.len()
            );
        }
        let (block_num, _) = keys::decode_reorg_undo_log_key(&key);
        if block_num <= rollback_to {
            continue;
        }
        let entry: UndoLogEntry = bincode::deserialize(&value).map_err(|e| {
            anyhow::anyhow!(
                "failed to decode undo log entry while loading tx contexts: key=0x{}, error={}",
                bytes_to_hex(&key),
                e
            )
        })?;
        if let UndoLogEntry::TxContext(ctx) = entry {
            contexts.push(ctx);
        }
    }
    Ok(contexts)
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

fn truncate_hodl_tracker_state_for_rollback(
    state: &mut HodlTrackerState,
    rollback_to: i64,
) -> anyhow::Result<bool> {
    if rollback_to < 0 {
        return Ok(true);
    }

    let mut changed = false;
    let original_transitions = state.date_transitions.len();
    state
        .date_transitions
        .retain(|(block_num, _)| *block_num <= rollback_to);
    if state.date_transitions.len() != original_transitions {
        changed = true;
    }

    if state.date_transitions.is_empty() {
        anyhow::bail!(
            "invalid HODL tracker state after rollback truncate: rollback_to={}, no remaining date transitions",
            rollback_to
        );
    }

    let max_date = state
        .date_transitions
        .last()
        .map(|(_, date)| date.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing last date transition while truncating HODL tracker state: rollback_to={}",
                rollback_to
            )
        })?;

    let original_capacity_dates = state.capacity_by_date.len();
    state
        .capacity_by_date
        .retain(|(date, _)| date.as_str() <= max_date.as_str());
    if state.capacity_by_date.len() != original_capacity_dates {
        changed = true;
    }

    if let Some(last_snapshot_date) = state.last_snapshot_date.as_ref() {
        if last_snapshot_date.as_str() > max_date.as_str() {
            state.last_snapshot_date = Some(max_date);
            changed = true;
        }
    }

    Ok(changed)
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
        self.rollback_to_block_with_tx_index_store(rollback_to, None)
    }

    pub fn rollback_to_block_with_tx_index_store(
        &self,
        rollback_to: i64,
        tx_index_store: Option<&CkbadgerStore>,
    ) -> anyhow::Result<RollbackResult> {
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

        // 1. Delete block headers > rollback_to
        let mut stage = RollbackStageProgress::new("delete_block_headers");
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_block_headers(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate block_headers in rollback_to_block cleanup: {}",
                    e
                )
            })?;
            if key.len() != 8 {
                anyhow::bail!(
                    "invalid block header key length during rollback cleanup: key_len={}, key=0x{}",
                    key.len(),
                    bytes_to_hex(&key)
                );
            }
            let block_num = keys::decode_block_num(&key);
            let header: CachedBlockHeader = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize block header during rollback cleanup: block_num={}, key=0x{}, error={}",
                    block_num,
                    bytes_to_hex(&key),
                    e
                )
            })?;
            batch.delete_cf(self.cf_block_headers(), &key);
            batch.delete_cf(self.cf_block_hash_index(), &header.hash);
            blocks_removed += 1;
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
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate tx_index in rollback_to_block cleanup: {}",
                    e
                )
            })?;
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

        // 3. Delete tx_hash_map entries for rolled-back transactions.
        // Prefer tx-context hashes from undo log to avoid full-CF scans.
        let tx_contexts = load_tx_contexts_from_undo_log(self, rollback_to)?;
        let tx_context_count = tx_contexts.len() as u64;
        let use_tx_context = tx_context_count > 0 && tx_context_count == txs_removed;
        let mut tx_hash_map_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_tx_hash_map");
        if use_tx_context {
            let mut seen_tx_hashes: HashSet<&[u8]> = HashSet::new();
            for ctx in &tx_contexts {
                if ctx.tx_hash.len() != 32 {
                    anyhow::bail!(
                        "invalid tx-context hash length while deleting tx_hash_map: expected=32, got={}",
                        ctx.tx_hash.len()
                    );
                }
                if !seen_tx_hashes.insert(ctx.tx_hash.as_slice()) {
                    continue;
                }
                batch.delete_cf(self.cf_tx_hash_map(), &ctx.tx_hash);
                tx_hash_map_removed += 1;
                stage.tick(tx_hash_map_removed);
            }
        } else {
            let iter = self.iterator_cf(self.cf_tx_hash_map(), IteratorMode::Start);
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate tx_hash_map in rollback_to_block cleanup: {}",
                        e
                    )
                })?;
                if value.len() != 12 {
                    anyhow::bail!(
                        "invalid tx_hash_map value length during rollback cleanup: key=0x{}, value_len={}, expected=12",
                        bytes_to_hex(&key),
                        value.len()
                    );
                }
                let mapped_block = keys::decode_block_num(&value[..8]);
                if mapped_block > rollback_to {
                    batch.delete_cf(self.cf_tx_hash_map(), &key);
                    tx_hash_map_removed += 1;
                }
                stage.tick(tx_hash_map_removed);
            }
        }
        stage.finish(tx_hash_map_removed);

        // 4-5. Roll back cell/live/consumed/index state.
        // Prefer tx-context entries from reorg_undo_log_by_block to derive touched outpoints.
        // Fallback to full scans when tx-context coverage is missing or partial.
        if !use_tx_context {
            if txs_removed > 0 {
                let reason = if tx_context_count == 0 {
                    "no tx-context undo entries found"
                } else {
                    "tx-context undo entries are partial"
                };
                info!(
                    rollback_to,
                    txs_removed,
                    tx_context_count,
                    replay_start,
                    reason,
                    "Falling back to full cell scans for rollback cell cleanup"
                );
            }

            // Fallback A: drop live cells created after rollback point.
            let mut stage = RollbackStageProgress::new("delete_live_cells_after_tip_fallback");
            let iter = self.iterator_cf(self.cf_live_cells(), IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate live_cells in rollback_to_block fallback cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::OUTPOINT_KEY_SIZE {
                    anyhow::bail!(
                        "invalid live_cells key length during rollback fallback: key_len={}, expected={}",
                        key.len(),
                        keys::OUTPOINT_KEY_SIZE
                    );
                }
                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                let info = self.get_cell_by_outpoint_key(&key)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing canonical cell for live outpoint during rollback fallback: outpoint=0x{}:{}",
                        bytes_to_hex(&tx_hash),
                        output_index
                    )
                })?;
                if info.created_at_block > rollback_to {
                    batch.delete_cf(self.cf_live_cells(), &key);
                    delete_cell_index_entries(self, &mut batch, &info, &tx_hash, output_index);
                    cells_removed += 1;
                }
                stage.tick(cells_removed);
            }
            stage.finish(cells_removed);

            // Fallback B: restore cells consumed after rollback point.
            let mut stage = RollbackStageProgress::new("restore_consumed_cells_fallback");
            let iter = self.iterator_cf(self.cf_consumed_cells(), IteratorMode::Start);
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate consumed_cells in rollback_to_block fallback cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::OUTPOINT_KEY_SIZE {
                    anyhow::bail!(
                        "invalid consumed_cells key length during rollback fallback: key_len={}, expected={}",
                        key.len(),
                        keys::OUTPOINT_KEY_SIZE
                    );
                }
                let meta = decode_consumed_cell_meta(&value).ok_or_else(|| {
                    anyhow::anyhow!(
                        "failed to decode consumed cell metadata during rollback fallback: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
                if meta.consumed_at_block <= rollback_to {
                    stage.tick(cells_restored);
                    continue;
                }

                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                batch.delete_cf(self.cf_consumed_cells(), &key);
                let info = self.get_cell_by_outpoint_key(&key)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing canonical cell for consumed outpoint during rollback fallback: outpoint=0x{}:{}",
                        bytes_to_hex(&tx_hash),
                        output_index
                    )
                })?;
                if info.created_at_block <= rollback_to {
                    batch.put_cf(self.cf_live_cells(), &key, []);
                    put_cell_index_entries(self, &mut batch, &info, &tx_hash, output_index);
                    cells_restored += 1;
                }
                stage.tick(cells_restored);
            }
            stage.finish(cells_restored);
        } else {
            let mut stage = RollbackStageProgress::new("rollback_cells_from_tx_context");
            for ctx in tx_contexts.into_iter().rev() {
                if ctx.tx_hash.len() != 32 {
                    anyhow::bail!(
                        "invalid tx-context hash length during rollback: expected=32, got={}",
                        ctx.tx_hash.len()
                    );
                }
                if ctx.outputs_count < 0 {
                    anyhow::bail!(
                        "invalid tx-context outputs_count during rollback: tx_hash=0x{}, outputs_count={}",
                        bytes_to_hex(&ctx.tx_hash),
                        ctx.outputs_count
                    );
                }

                for output_index in 0..(ctx.outputs_count as i32) {
                    let output_index = i16::try_from(output_index).map_err(|_| {
                        anyhow::anyhow!(
                            "tx-context output index exceeds i16 during rollback: tx_hash=0x{}, output_index={}",
                            bytes_to_hex(&ctx.tx_hash),
                            output_index
                        )
                    })?;
                    let outpoint_key = keys::encode_outpoint(&ctx.tx_hash, output_index);
                    if self.get_cf(self.cf_live_cells(), &outpoint_key)?.is_some() {
                        let info = self.get_cell_by_outpoint_key(&outpoint_key)?.ok_or_else(|| {
                            anyhow::anyhow!(
                                "missing canonical cell for live tx output during rollback: outpoint=0x{}:{}",
                                bytes_to_hex(&ctx.tx_hash),
                                output_index
                            )
                        })?;
                        batch.delete_cf(self.cf_live_cells(), outpoint_key);
                        delete_cell_index_entries(
                            self,
                            &mut batch,
                            &info,
                            &ctx.tx_hash,
                            output_index,
                        );
                        cells_removed += 1;
                    }
                    // Remove consumed marker for outputs created in rolled-back blocks.
                    batch.delete_cf(self.cf_consumed_cells(), outpoint_key);
                }

                for input in &ctx.inputs {
                    if input.tx_hash.len() != 32 {
                        anyhow::bail!(
                            "invalid tx-context input hash length during rollback: expected=32, got={}, consuming_tx=0x{}",
                            input.tx_hash.len(),
                            bytes_to_hex(&ctx.tx_hash)
                        );
                    }
                    if input.output_index < 0 {
                        let is_cellbase_sentinel =
                            input.output_index == -1 && input.tx_hash.iter().all(|b| *b == 0);
                        if is_cellbase_sentinel {
                            // Cellbase sentinel has no referenced previous cell.
                            continue;
                        }
                        anyhow::bail!(
                            "invalid negative tx-context input output_index during rollback: consuming_tx=0x{}, outpoint=0x{}:{}",
                            bytes_to_hex(&ctx.tx_hash),
                            bytes_to_hex(&input.tx_hash),
                            input.output_index
                        );
                    }
                    let outpoint_key = keys::encode_outpoint(&input.tx_hash, input.output_index);
                    match self.get_consumed_cell_info(&input.tx_hash, input.output_index)? {
                        Some(consumed) => {
                            if consumed.consumed_at_block <= rollback_to {
                                anyhow::bail!(
                                    "invalid tx-context consumed block during rollback: consuming_tx=0x{}, outpoint=0x{}:{}, consumed_at_block={}, rollback_to={}",
                                    bytes_to_hex(&ctx.tx_hash),
                                    bytes_to_hex(&input.tx_hash),
                                    input.output_index,
                                    consumed.consumed_at_block,
                                    rollback_to
                                );
                            }
                            if let Some(ref consumed_by_tx) = consumed.consumed_by_tx {
                                if consumed_by_tx.as_slice() != ctx.tx_hash.as_slice() {
                                    anyhow::bail!(
                                        "tx-context consumed_by_tx mismatch during rollback: expected=0x{}, actual=0x{}, outpoint=0x{}:{}",
                                        bytes_to_hex(&ctx.tx_hash),
                                        bytes_to_hex(consumed_by_tx),
                                        bytes_to_hex(&input.tx_hash),
                                        input.output_index
                                    );
                                }
                            }
                            batch.delete_cf(self.cf_consumed_cells(), outpoint_key);
                            if consumed.cell.created_at_block <= rollback_to {
                                batch.put_cf(self.cf_live_cells(), outpoint_key, []);
                                put_cell_index_entries(
                                    self,
                                    &mut batch,
                                    &consumed.cell,
                                    &input.tx_hash,
                                    input.output_index,
                                );
                                cells_restored += 1;
                            }
                        }
                        None => {
                            if self.get_cf(self.cf_live_cells(), &outpoint_key)?.is_none() {
                                anyhow::bail!(
                                    "missing consumed/live input during tx-context rollback: consuming_tx=0x{}, outpoint=0x{}:{}",
                                    bytes_to_hex(&ctx.tx_hash),
                                    bytes_to_hex(&input.tx_hash),
                                    input.output_index
                                );
                            }
                        }
                    }
                }
                stage.tick(cells_removed + cells_restored);
            }
            stage.finish(cells_removed + cells_restored);
        }

        // 5. Clear DAO secondary indexes before rebuilding from repaired deposits.
        let mut dao_indexes_deleted = 0u64;
        let mut stage = RollbackStageProgress::new("rebuild_dao_indexes_clear");
        let dao_index_cfs = [
            self.cf_dao_by_withdraw_tx(),
            self.cf_dao_by_block(),
            self.cf_dao_by_lock_block(),
            self.cf_dao_by_status_block(),
        ];
        for cf in dao_index_cfs {
            let iter = self.iterator_cf(cf, IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate DAO index in rollback_to_block clear stage: {}",
                        e
                    )
                })?;
                batch.delete_cf(cf, &key);
                dao_indexes_deleted += 1;
                stage.tick(dao_indexes_deleted);
            }
        }
        stage.finish(dao_indexes_deleted);

        // 6. Repair DAO deposits and stream rebuilt secondary indexes into the same batch.
        let mut dao_deposits_deleted = 0u64;
        let mut dao_deposits_repaired = 0u64;
        let mut dao_indexes_rebuilt = 0u64;
        let mut stage = RollbackStageProgress::new("repair_and_rebuild_dao_indexes");
        let iter = self.iterator_cf(self.cf_dao_deposits(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_deposits in rollback_to_block repair stage: {}",
                    e
                )
            })?;
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
                stage.tick(dao_deposits_deleted + dao_deposits_repaired + dao_indexes_rebuilt);
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

            let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, &key);
            let by_lock_key = keys::encode_dao_by_lock_block_key(
                &entry.lock_script_hash,
                entry.deposit_block_number,
                &key,
            );
            let by_status_key = keys::encode_dao_by_status_block_key(
                entry.status,
                entry.deposit_block_number,
                &key,
            );
            batch.put_cf(self.cf_dao_by_block(), by_block_key, []);
            batch.put_cf(self.cf_dao_by_lock_block(), by_lock_key, []);
            batch.put_cf(self.cf_dao_by_status_block(), by_status_key, []);
            dao_indexes_rebuilt += 3;

            if entry.status >= 1 {
                let request_block = entry.withdraw_request_block.ok_or_else(|| {
                    anyhow::anyhow!(
                        "dao deposit missing withdraw_request_block while preparing rollback rebuild: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
                let request_tx = entry.withdraw_request_tx.ok_or_else(|| {
                    anyhow::anyhow!(
                        "dao deposit missing withdraw_request_tx while preparing rollback rebuild: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
                let request_output_index = entry.withdraw_request_output_index.ok_or_else(|| {
                    anyhow::anyhow!(
                        "dao deposit missing withdraw_request_output_index while preparing rollback rebuild: outpoint=0x{}",
                        bytes_to_hex(&key)
                    )
                })?;
                if request_block <= rollback_to {
                    let withdraw_outpoint_key =
                        keys::encode_outpoint(&request_tx, request_output_index);
                    batch.put_cf(self.cf_dao_by_withdraw_tx(), withdraw_outpoint_key, &key);
                    dao_indexes_rebuilt += 1;
                }
            }
            stage.tick(dao_deposits_deleted + dao_deposits_repaired + dao_indexes_rebuilt);
        }
        stage.finish(dao_deposits_deleted + dao_deposits_repaired + dao_indexes_rebuilt);

        // 7. Delete date-scoped stats entries from replay cutoff date onward.
        // These are additive snapshots and would be double-counted after replay.
        // Scan all split stats CFs that may contain date-scoped prefixes.
        if let Some(cutoff) = replay_cutoff_date.as_deref() {
            let mut stats_removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_stats_from_cutoff");
            let stats_cfs = [
                self.cf_stats_chain(),
                self.cf_stats_dao(),
                self.cf_stats_hodl(),
                self.cf_stats_script(),
                self.cf_stats_token(),
                self.cf_stats_spore(),
                self.cf_stats_nft(),
            ];
            for cf in stats_cfs {
                let iter = self.iterator_cf(cf, IteratorMode::Start);
                for item in iter {
                    let (key, _) = item.map_err(|e| {
                        anyhow::anyhow!(
                            "failed to iterate stats CF in rollback_to_block cleanup: {}",
                            e
                        )
                    })?;
                    if should_delete_stats_for_replay(&key, cutoff.as_bytes())? {
                        batch.delete_cf(cf, &key);
                        stats_removed += 1;
                    }
                    stage.tick(stats_removed);
                }
            }
            stage.finish(stats_removed);
        }

        // 8. Delete token_transfers entries > rollback_to
        // Key: type_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        let mut token_transfers_removed = 0u64;
        let mut stage = RollbackStageProgress::new("delete_token_transfers");
        let iter = self.iterator_cf(self.cf_token_transfers(), IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_transfers in rollback_to_block cleanup: {}",
                    e
                )
            })?;
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

        // 10. Repair Spore/NFT domain state for orphaned blocks and rebuild secondary indexes.
        let mut stage = RollbackStageProgress::new("repair_spore_nft_domain");
        let mut spore_deleted = 0u64;
        let mut nft_deleted = 0u64;
        let mut secondary_keys_deleted = 0u64;
        let mut secondary_keys_written = 0u64;
        let mut aggregate_rows_written = 0u64;
        let mut cluster_owner_rows_written = 0u64;

        let secondary_cfs = [
            self.cf_spore_by_cluster(),
            self.cf_cluster_agg(),
            self.cf_nft_by_collection(),
            self.cf_nft_collection_agg(),
        ];
        for cf in secondary_cfs {
            let iter = self.iterator_cf(cf, IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate spore/nft secondary CF during rollback cleanup: {}",
                        e
                    )
                })?;
                batch.delete_cf(cf, &key);
                secondary_keys_deleted += 1;
                stage.tick(
                    spore_deleted
                        + nft_deleted
                        + secondary_keys_deleted
                        + secondary_keys_written
                        + aggregate_rows_written
                        + cluster_owner_rows_written,
                );
            }
        }

        // Cluster-owner counters are stored in stats_spore CF under a dedicated prefix.
        let iter = self.iterator_cf(self.cf_stats_spore(), IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore while clearing cluster owner counters in rollback cleanup: {}",
                    e
                )
            })?;
            if key.first() == Some(&keys::STATS_PREFIX_CLUSTER_OWNER) {
                batch.delete_cf(self.cf_stats_spore(), &key);
                secondary_keys_deleted += 1;
            }
            stage.tick(
                spore_deleted
                    + nft_deleted
                    + secondary_keys_deleted
                    + secondary_keys_written
                    + aggregate_rows_written
                    + cluster_owner_rows_written,
            );
        }

        let mut cluster_aggs: HashMap<Vec<u8>, ClusterAggregate> = HashMap::new();
        let mut cluster_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64> = HashMap::new();
        let mut nft_collection_aggs: HashMap<Vec<u8>, NftCollectionAggregate> = HashMap::new();

        let iter = self.iterator_cf(self.cf_spore_data(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_data while repairing rollback state: {}",
                    e
                )
            })?;
            let spore_id = key.to_vec();
            let entry: DobEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize spore_data entry during rollback repair: spore_id=0x{}, error={}",
                    bytes_to_hex(&spore_id),
                    e
                )
            })?;

            if entry.created_at_block > rollback_to {
                batch.delete_cf(self.cf_spore_data(), &key);
                spore_deleted += 1;
                stage.tick(
                    spore_deleted
                        + nft_deleted
                        + secondary_keys_deleted
                        + secondary_keys_written
                        + aggregate_rows_written
                        + cluster_owner_rows_written,
                );
                continue;
            }

            match entry.standard {
                DobStandard::SporeCluster => {
                    let agg = cluster_aggs.entry(spore_id).or_default();
                    agg.name = entry.name.clone();
                    agg.description = entry.description.clone();
                }
                DobStandard::Spore => {
                    if let Some(cluster_id) = entry.collection_id.as_ref() {
                        let idx_key = keys::encode_spore_by_cluster_key(cluster_id, &spore_id);
                        batch.put_cf(self.cf_spore_by_cluster(), idx_key, []);
                        secondary_keys_written += 1;

                        let agg = cluster_aggs.entry(cluster_id.clone()).or_default();
                        agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "cluster total_count overflow while repairing rollback state: cluster_id=0x{}",
                                bytes_to_hex(cluster_id)
                            )
                        })?;
                        if entry.is_live {
                            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "cluster live_count overflow while repairing rollback state: cluster_id=0x{}",
                                    bytes_to_hex(cluster_id)
                                )
                            })?;

                            if let Some(owner_lock_hash) = entry.owner_lock_hash.as_ref() {
                                let owner_key = (cluster_id.clone(), owner_lock_hash.clone());
                                let owner_count =
                                    cluster_owner_counts.entry(owner_key).or_insert(0);
                                *owner_count = owner_count.checked_add(1).ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "cluster owner count overflow while repairing rollback state: cluster_id=0x{}",
                                        bytes_to_hex(cluster_id)
                                    )
                                })?;
                            }

                            let tier = match &entry.extra {
                                DobExtra::Spore { media_profile, .. } => media_profile.tier,
                                _ => StorageDependencyTier::Unknown,
                            };
                            let tier_slot = match tier {
                                StorageDependencyTier::FullyOnchain => &mut agg.fully_onchain_count,
                                StorageDependencyTier::DecentralizedExternal => {
                                    &mut agg.decentralized_external_count
                                }
                                StorageDependencyTier::CentralizedDependent => {
                                    &mut agg.centralized_dependent_count
                                }
                                StorageDependencyTier::Unknown => &mut agg.unknown_count,
                            };
                            *tier_slot = tier_slot.checked_add(1).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "cluster media tier count overflow while repairing rollback state: cluster_id=0x{}, tier={}",
                                    bytes_to_hex(cluster_id),
                                    tier.as_str()
                                )
                            })?;
                        }
                    }
                }
                DobStandard::DidCkb => {
                    let idx_key =
                        keys::encode_nft_by_collection_key(&DID_CKB_SENTINEL_COLLECTION, &spore_id);
                    batch.put_cf(self.cf_nft_by_collection(), idx_key, []);
                    secondary_keys_written += 1;

                    let agg = nft_collection_aggs
                        .entry(DID_CKB_SENTINEL_COLLECTION.to_vec())
                        .or_insert_with(|| NftCollectionAggregate {
                            name: Some("did:ckb".to_string()),
                            standard: NftStandard::DidCkb,
                            ..Default::default()
                        });
                    agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "did:ckb total_count overflow while repairing rollback state"
                        )
                    })?;
                    if entry.is_live {
                        agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "did:ckb live_count overflow while repairing rollback state"
                            )
                        })?;
                    }
                }
            }
            stage.tick(
                spore_deleted
                    + nft_deleted
                    + secondary_keys_deleted
                    + secondary_keys_written
                    + aggregate_rows_written
                    + cluster_owner_rows_written,
            );
        }

        let iter = self.iterator_cf(self.cf_nft_data(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate nft_data while repairing rollback state: {}",
                    e
                )
            })?;
            let nft_id = key.to_vec();
            let entry: NftEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize nft_data entry during rollback repair: nft_id=0x{}, error={}",
                    bytes_to_hex(&nft_id),
                    e
                )
            })?;

            if entry.created_at_block > rollback_to {
                batch.delete_cf(self.cf_nft_data(), &key);
                nft_deleted += 1;
                stage.tick(
                    spore_deleted
                        + nft_deleted
                        + secondary_keys_deleted
                        + secondary_keys_written
                        + aggregate_rows_written
                        + cluster_owner_rows_written,
                );
                continue;
            }

            match entry.standard {
                NftStandard::DotBit => {
                    let idx_key =
                        keys::encode_nft_by_collection_key(&DOTBIT_SENTINEL_COLLECTION, &nft_id);
                    batch.put_cf(self.cf_nft_by_collection(), idx_key, []);
                    secondary_keys_written += 1;
                    let agg = nft_collection_aggs
                        .entry(DOTBIT_SENTINEL_COLLECTION.to_vec())
                        .or_insert_with(|| NftCollectionAggregate {
                            name: Some(".bit".to_string()),
                            standard: NftStandard::DotBit,
                            ..Default::default()
                        });
                    agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "dotbit total_count overflow while repairing rollback state"
                        )
                    })?;
                    if entry.is_live {
                        agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "dotbit live_count overflow while repairing rollback state"
                            )
                        })?;
                    }
                }
                NftStandard::MnftClass => {
                    let agg = nft_collection_aggs.entry(nft_id).or_insert_with(|| {
                        NftCollectionAggregate {
                            standard: NftStandard::MnftClass,
                            ..Default::default()
                        }
                    });
                    agg.standard = NftStandard::MnftClass;
                    if entry.name.is_some() {
                        agg.name = entry.name.clone();
                    }
                }
                NftStandard::MnftToken => {
                    if let Some(collection_id) = entry.collection_id.as_ref() {
                        let idx_key = keys::encode_nft_by_collection_key(collection_id, &nft_id);
                        batch.put_cf(self.cf_nft_by_collection(), idx_key, []);
                        secondary_keys_written += 1;
                        let agg = nft_collection_aggs
                            .entry(collection_id.clone())
                            .or_insert_with(|| NftCollectionAggregate {
                                standard: NftStandard::MnftClass,
                                ..Default::default()
                            });
                        agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "mNFT total_count overflow while repairing rollback state: collection_id=0x{}",
                                bytes_to_hex(collection_id)
                            )
                        })?;
                        if entry.is_live {
                            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "mNFT live_count overflow while repairing rollback state: collection_id=0x{}",
                                    bytes_to_hex(collection_id)
                                )
                            })?;
                        }
                    }
                }
                NftStandard::DidCkb => {
                    let idx_key =
                        keys::encode_nft_by_collection_key(&DID_CKB_SENTINEL_COLLECTION, &nft_id);
                    batch.put_cf(self.cf_nft_by_collection(), idx_key, []);
                    secondary_keys_written += 1;
                    let agg = nft_collection_aggs
                        .entry(DID_CKB_SENTINEL_COLLECTION.to_vec())
                        .or_insert_with(|| NftCollectionAggregate {
                            name: Some("did:ckb".to_string()),
                            standard: NftStandard::DidCkb,
                            ..Default::default()
                        });
                    agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "did:ckb total_count overflow while repairing rollback state"
                        )
                    })?;
                    if entry.is_live {
                        agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "did:ckb live_count overflow while repairing rollback state"
                            )
                        })?;
                    }
                }
                NftStandard::MnftIssuer => {}
            }
            stage.tick(
                spore_deleted
                    + nft_deleted
                    + secondary_keys_deleted
                    + secondary_keys_written
                    + aggregate_rows_written
                    + cluster_owner_rows_written,
            );
        }

        let mut cluster_owner_totals: HashMap<Vec<u8>, i64> = HashMap::new();
        for ((cluster_id, lock_hash), count) in &cluster_owner_counts {
            let owner_key = keys::encode_cluster_owner_key(cluster_id, lock_hash);
            batch.put_cf(
                self.cf_stats_spore(),
                owner_key,
                count.to_le_bytes().as_slice(),
            );
            cluster_owner_rows_written += 1;
            let owner_total = cluster_owner_totals.entry(cluster_id.clone()).or_insert(0);
            *owner_total = owner_total.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "cluster owner_count overflow while repairing rollback state: cluster_id=0x{}",
                    bytes_to_hex(cluster_id)
                )
            })?;
        }
        for (cluster_id, agg) in &mut cluster_aggs {
            agg.owner_count = cluster_owner_totals.get(cluster_id).copied().unwrap_or(0);
            let encoded = bincode::serialize(agg).map_err(|e| {
                anyhow::anyhow!(
                    "failed to serialize cluster aggregate during rollback repair: cluster_id=0x{}, error={}",
                    bytes_to_hex(cluster_id),
                    e
                )
            })?;
            batch.put_cf(self.cf_cluster_agg(), cluster_id, &encoded);
            aggregate_rows_written += 1;
        }

        for (collection_id, agg) in &nft_collection_aggs {
            let encoded = bincode::serialize(agg).map_err(|e| {
                anyhow::anyhow!(
                    "failed to serialize nft collection aggregate during rollback repair: collection_id=0x{}, error={}",
                    bytes_to_hex(collection_id),
                    e
                )
            })?;
            batch.put_cf(self.cf_nft_collection_agg(), collection_id, &encoded);
            aggregate_rows_written += 1;
        }
        stage.finish(
            spore_deleted
                + nft_deleted
                + secondary_keys_deleted
                + secondary_keys_written
                + aggregate_rows_written
                + cluster_owner_rows_written,
        );

        // 11. Keep HODL tracker state aligned with rollback tip in the same write batch.
        let mut stage = RollbackStageProgress::new("repair_hodl_tracker_state");
        let mut hodl_tracker_repaired = 0u64;
        if rollback_to < 0 {
            if self
                .get_cf(self.cf_sync_meta(), keys::sync_meta_keys::HODL_TRACKER)?
                .is_some()
            {
                batch.delete_cf(self.cf_sync_meta(), keys::sync_meta_keys::HODL_TRACKER);
                hodl_tracker_repaired += 1;
            }
        } else if let Some(mut state) = self.get_hodl_tracker_state()? {
            if truncate_hodl_tracker_state_for_rollback(&mut state, rollback_to)? {
                let encoded = bincode::serialize(&state).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to serialize repaired HODL tracker state during rollback cleanup: {}",
                        e
                    )
                })?;
                batch.put_cf(
                    self.cf_sync_meta(),
                    keys::sync_meta_keys::HODL_TRACKER,
                    &encoded,
                );
                hodl_tracker_repaired += 1;
            }
        }
        stage.tick(hodl_tracker_repaired);
        stage.finish(hodl_tracker_repaired);

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
        let rebuilt_balances =
            self.rebuild_addr_balances_from_live_cells_with_tx_index_store(tx_index_store)?;
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
            let header = self.get_block_header(rollback_to)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing rollback target block header while updating sync status tip: rollback_to={}",
                    rollback_to
                )
            })?;
            header.hash
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
        AddressBalance, CachedBlockHeader, DaoDepositCacheEntry, DobEntry, DobExtra, DobStandard,
        HodlTrackerState, LiveCellInfo, NftCollectionAggregate, NftEntry, NftExtra, NftStandard,
        ScriptInfo, SporeMediaProfile, StorageDependencyTier, TokenDailyDelta, TokenInfo,
        TokenTransferRecord, TxIndexEntry, UndoInputOutPoint, UndoLogEntry, UndoTxContext,
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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock_code_hash = vec![0x7A; 32];

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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
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
    fn test_rollback_uses_tx_context_undo_entries_for_cell_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

        let input_tx = vec![0x31; 32];
        let consuming_tx = vec![0x32; 32];
        let input_cell = LiveCellInfo {
            capacity: 400,
            created_at_block: 1,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 400,
            udt_amount: None,
        };
        let rollback_output_cell = LiveCellInfo {
            capacity: 200,
            created_at_block: 2,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
        };

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_cell(&input_tx, 0, &input_cell);
        batch.put_cell(&consuming_tx, 0, &rollback_output_cell);
        batch.put_consumed_cell_with_consumer(&input_tx, 0, &input_cell, 2, Some(&consuming_tx));
        batch.delete_cell(&input_tx, 0);
        batch.put_reorg_undo_log_by_block(
            2,
            0,
            &UndoLogEntry::TxContext(UndoTxContext {
                tx_hash: consuming_tx.clone(),
                outputs_count: 1,
                inputs: vec![UndoInputOutPoint {
                    tx_hash: input_tx.clone(),
                    output_index: 0,
                }],
            }),
        );
        batch.commit().unwrap();

        assert!(store.get_cell(&input_tx, 0).unwrap().is_none());
        assert!(store.get_cell(&consuming_tx, 0).unwrap().is_some());
        assert!(store.get_consumed_cell(&input_tx, 0).unwrap().is_some());

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&input_tx, 0).unwrap().is_some());
        assert!(store.get_cell(&consuming_tx, 0).unwrap().is_none());
        assert!(store.get_consumed_cell(&input_tx, 0).unwrap().is_none());
    }

    #[test]
    fn test_rollback_falls_back_to_full_scan_when_tx_contexts_are_partial() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
            transactions_count: 2,
        };

        let input_tx = vec![0x41; 32];
        let consuming_tx_a = vec![0x42; 32];
        let consuming_tx_b = vec![0x43; 32];
        let input_cell = LiveCellInfo {
            capacity: 400,
            created_at_block: 1,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 400,
            udt_amount: None,
        };
        let rollback_output_cell_a = LiveCellInfo {
            capacity: 200,
            created_at_block: 2,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
        };
        let rollback_output_cell_b = LiveCellInfo {
            capacity: 180,
            created_at_block: 2,
            lock_script_hash: vec![0xCC; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 180,
            udt_amount: None,
        };

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_tx_index(2, 1, &tx_index);
        batch.put_cell(&input_tx, 0, &input_cell);
        batch.put_cell(&consuming_tx_a, 0, &rollback_output_cell_a);
        batch.put_cell(&consuming_tx_b, 0, &rollback_output_cell_b);
        batch.put_consumed_cell_with_consumer(&input_tx, 0, &input_cell, 2, Some(&consuming_tx_a));
        batch.delete_cell(&input_tx, 0);
        // Seed only one tx-context entry while tx_index indicates two txs in block 2.
        // rollback_to_block should detect partial coverage and use full-scan fallback.
        batch.put_reorg_undo_log_by_block(
            2,
            0,
            &UndoLogEntry::TxContext(UndoTxContext {
                tx_hash: consuming_tx_a.clone(),
                outputs_count: 1,
                inputs: vec![UndoInputOutPoint {
                    tx_hash: input_tx.clone(),
                    output_index: 0,
                }],
            }),
        );
        batch.commit().unwrap();

        assert!(store.get_cell(&input_tx, 0).unwrap().is_none());
        assert!(store.get_cell(&consuming_tx_a, 0).unwrap().is_some());
        assert!(store.get_cell(&consuming_tx_b, 0).unwrap().is_some());
        assert!(store.get_consumed_cell(&input_tx, 0).unwrap().is_some());

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&input_tx, 0).unwrap().is_some());
        assert!(store.get_consumed_cell(&input_tx, 0).unwrap().is_none());
        assert!(store.get_cell(&consuming_tx_a, 0).unwrap().is_none());
        assert!(store.get_cell(&consuming_tx_b, 0).unwrap().is_none());
    }

    #[test]
    fn test_rollback_removes_tx_hash_map_entries_above_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

        let keep_tx = vec![0x11; 32];
        let drop_tx = vec![0x22; 32];
        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_hash_map(&keep_tx, 1, 0);
        batch.put_tx_hash_map(&drop_tx, 2, 0);
        batch.put_tx_index(2, 0, &tx_index);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert_eq!(store.get_tx_location(&keep_tx).unwrap(), Some((1, 0)));
        assert_eq!(store.get_tx_location(&drop_tx).unwrap(), None);
        assert!(store.get_tx_index(2, 0).unwrap().is_none());
    }

    #[test]
    fn test_rollback_uses_tx_context_for_tx_hash_map_without_scanning() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

        let keep_tx = vec![0x11; 32];
        let drop_tx = vec![0x22; 32];
        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_hash_map(&keep_tx, 1, 0);
        batch.put_tx_hash_map(&drop_tx, 2, 0);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_reorg_undo_log_by_block(
            2,
            0,
            &UndoLogEntry::TxContext(UndoTxContext {
                tx_hash: drop_tx.clone(),
                outputs_count: 1,
                inputs: vec![],
            }),
        );
        batch.commit().unwrap();

        // Corrupt unrelated tx_hash_map row to prove rollback does not full-scan tx_hash_map
        // when tx-context coverage is complete.
        store
            .put_cf(store.cf_tx_hash_map(), &[0xFF; 32], &[0xAA; 8])
            .unwrap();

        store.rollback_to_block(1).unwrap();

        assert_eq!(store.get_tx_location(&keep_tx).unwrap(), Some((1, 0)));
        assert_eq!(store.get_tx_location(&drop_tx).unwrap(), None);
    }

    #[test]
    fn test_rollback_tx_context_ignores_cellbase_sentinel_input() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

        let cellbase_tx = vec![0x22; 32];
        let tx_index = TxIndexEntry {
            is_cellbase: true,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };
        let cell = LiveCellInfo {
            capacity: 100,
            created_at_block: 2,
            lock_script_hash: vec![0xAA; 32],
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

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_hash_map(&cellbase_tx, 2, 0);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_cell(&cellbase_tx, 0, &cell);
        batch.put_reorg_undo_log_by_block(
            2,
            0,
            &UndoLogEntry::TxContext(UndoTxContext {
                tx_hash: cellbase_tx.clone(),
                outputs_count: 1,
                inputs: vec![UndoInputOutPoint {
                    tx_hash: vec![0u8; 32],
                    output_index: -1,
                }],
            }),
        );
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&cellbase_tx, 0).unwrap().is_none());
        assert!(store.get_tx_index(2, 0).unwrap().is_none());
        assert_eq!(store.get_tx_location(&cellbase_tx).unwrap(), None);
    }

    #[test]
    fn test_rollback_cleans_rolled_back_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
        let keep_cell = LiveCellInfo {
            capacity: 100,
            created_at_block: 1,
            lock_script_hash: vec![0xAA; 32],
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
        let drop_live_cell = LiveCellInfo {
            capacity: 200,
            created_at_block: 2,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
        };
        let drop_consumed_cell = LiveCellInfo {
            capacity: 300,
            created_at_block: 2,
            lock_script_hash: vec![0xCC; 32],
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

        let keep_tx = vec![0x10; 32];
        let drop_live_tx = vec![0x20; 32];
        let drop_consumed_tx = vec![0x30; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&keep_tx, 0, &keep_cell);
        batch.put_cell(&drop_live_tx, 0, &drop_live_cell);
        batch.put_cell(&drop_consumed_tx, 0, &drop_consumed_cell);
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&drop_consumed_tx, 0, &drop_consumed_cell, 2);
        batch.delete_cell(&drop_consumed_tx, 0);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&keep_tx, 0).unwrap().is_some());
        assert!(store.get_cell(&drop_live_tx, 0).unwrap().is_none());
        assert!(store.get_cell(&drop_consumed_tx, 0).unwrap().is_none());
        assert!(store
            .get_consumed_cell(&drop_consumed_tx, 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_repairs_dao_deposits_and_withdraw_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
        let header3 = CachedBlockHeader {
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
        batch.put_dao_by_withdraw_tx(&request_tx_a, 0, &outpoint_a);

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
        batch.put_dao_by_withdraw_tx(&request_tx_b, 0, &outpoint_b);

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
        batch.put_dao_by_withdraw_tx(&orphan_request_tx, 0, &orphan_outpoint);
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
            .get_dao_deposit_by_withdraw_tx(&request_tx_a, 0)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_dao_deposit_by_withdraw_tx(&request_tx_b, 0)
                .unwrap()
                .unwrap(),
            outpoint_b
        );
        assert!(store
            .get_dao_deposit_by_withdraw_tx(&orphan_request_tx, 0)
            .unwrap()
            .is_none());

        let status0 = store
            .list_dao_deposits_by_status_paginated(0, 10, None)
            .unwrap();
        assert_eq!(status0.len(), 1);
        assert_eq!(status0[0].0, outpoint_a);

        let status1 = store
            .list_dao_deposits_by_status_paginated(1, 10, None)
            .unwrap();
        assert_eq!(status1.len(), 1);
        assert_eq!(status1[0].0, outpoint_b);

        let mut lock_rows = Vec::new();
        store
            .scan_dao_deposits_by_lock(&[0xB1; 32], |outpoint, _| {
                lock_rows.push(outpoint.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(lock_rows, vec![outpoint_b]);
    }

    #[test]
    fn test_rollback_does_not_reindex_deleted_dao_deposit() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

        let deposit_tx = vec![0xAB; 32];
        let request_tx = vec![0xCD; 32];
        let deposit_outpoint = keys::encode_outpoint(&deposit_tx, 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_dao_deposit(
            &deposit_outpoint,
            &DaoDepositCacheEntry {
                capacity: 123,
                deposit_block_number: 2,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 1,
                withdraw_request_tx: Some(request_tx.clone()),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(1),
                withdraw_request_ar: Some(1),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_by_withdraw_tx(&request_tx, 0, &deposit_outpoint);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_dao_deposit(&deposit_outpoint).unwrap().is_none());
        assert!(store
            .get_dao_deposit_by_withdraw_tx(&request_tx, 0)
            .unwrap()
            .is_none());
        assert!(store
            .list_dao_deposits_by_status_paginated(1, 10, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_rollback_to_block_errors_when_target_below_minus_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store.rollback_to_block(-2).unwrap_err();
        assert!(err.to_string().contains("expected >= -1"));
    }

    #[test]
    fn test_rollback_to_block_fails_on_invalid_block_header_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let key = keys::encode_block_num(1);
        store
            .put_cf(
                store.cf_block_headers(),
                &key,
                b"invalid-block-header-payload",
            )
            .unwrap();

        let err = store.rollback_to_block(-1).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize block header during rollback cleanup"));
    }

    #[test]
    fn test_rollback_to_block_fails_when_target_header_missing_for_tip_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(2, &header2);
        batch.commit().unwrap();

        let err = store.rollback_to_block(1).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing rollback target block header while updating sync status tip"));
        assert!(err.to_string().contains("rollback_to=1"));
    }

    #[test]
    fn test_rollback_rebuilds_token_state_from_transfers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
    fn test_rollback_keeps_token_daily_deltas_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let day1_ts = 1_704_067_200_000i64; // 2024-01-01T00:00:00Z
        let day2_ts = 1_704_153_600_000i64; // 2024-01-02T00:00:00Z
        let day1 = keys::timestamp_ms_to_date(day1_ts);
        let day2 = keys::timestamp_ms_to_date(day2_ts);
        let type_hash = vec![0x44; 32];

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: day1_ts,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

    #[test]
    fn test_rollback_repairs_spore_nft_domain_indexes_and_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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

        let cluster_id = vec![0xAA; 32];
        let owner = vec![0xCC; 32];
        let spore_keep_id = vec![0x11; 32];
        let spore_drop_id = vec![0x22; 32];
        let nft_keep_id = vec![0x33; 32];
        let nft_drop_id = vec![0x44; 32];
        let class_id = vec![0x55; 24];

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_spore(
            &cluster_id,
            &DobEntry {
                standard: DobStandard::SporeCluster,
                collection_id: None,
                owner_lock_hash: None,
                name: Some("cluster-a".to_string()),
                description: Some("desc".to_string()),
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x01; 32],
                extra: DobExtra::SporeCluster,
            },
        );
        batch.put_spore(
            &spore_keep_id,
            &DobEntry {
                standard: DobStandard::Spore,
                collection_id: Some(cluster_id.clone()),
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x02; 32],
                extra: DobExtra::Spore {
                    content_type: "image/png".to_string(),
                    content_length: 8,
                    media_profile: SporeMediaProfile {
                        tier: StorageDependencyTier::FullyOnchain,
                        ..Default::default()
                    },
                },
            },
        );
        batch.put_spore(
            &spore_drop_id,
            &DobEntry {
                standard: DobStandard::Spore,
                collection_id: Some(cluster_id.clone()),
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 2,
                created_at_tx: vec![0x03; 32],
                extra: DobExtra::Spore {
                    content_type: "image/png".to_string(),
                    content_length: 8,
                    media_profile: SporeMediaProfile {
                        tier: StorageDependencyTier::FullyOnchain,
                        ..Default::default()
                    },
                },
            },
        );
        batch.put_nft(
            &nft_keep_id,
            &NftEntry {
                standard: NftStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(nft_keep_id.clone()),
                owner_lock_hash: Some(owner.clone()),
                name: None,
                is_live: true,
                created_at_block: 1,
                extra: NftExtra::MnftToken {
                    token_index: 1,
                    characteristic: vec![],
                    configure: 0,
                    state: 0,
                },
            },
        );
        batch.put_nft(
            &nft_drop_id,
            &NftEntry {
                standard: NftStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(nft_drop_id.clone()),
                owner_lock_hash: Some(owner.clone()),
                name: None,
                is_live: true,
                created_at_block: 2,
                extra: NftExtra::MnftToken {
                    token_index: 2,
                    characteristic: vec![],
                    configure: 0,
                    state: 0,
                },
            },
        );
        // Seed stale rows that should be replaced by rollback repair.
        batch.put_spore_by_cluster(&cluster_id, &[0xFF; 32]);
        batch.put_cluster_aggregate(
            &cluster_id,
            &ClusterAggregate {
                total_count: 99,
                live_count: 99,
                owner_count: 99,
                ..Default::default()
            },
        );
        batch.put_nft_by_collection(&class_id, &[0xEE; 32]);
        batch.put_nft_collection_aggregate(
            &class_id,
            &NftCollectionAggregate {
                name: Some("stale".to_string()),
                standard: NftStandard::MnftClass,
                total_count: 99,
                live_count: 99,
            },
        );
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_spore(&spore_keep_id).unwrap().is_some());
        assert!(store.get_spore(&spore_drop_id).unwrap().is_none());
        assert!(store.get_nft(&nft_keep_id).unwrap().is_some());
        assert!(store.get_nft(&nft_drop_id).unwrap().is_none());

        let spores_in_cluster = store.list_spores_by_cluster(&cluster_id, 10).unwrap();
        assert_eq!(spores_in_cluster.len(), 1);
        assert_eq!(spores_in_cluster[0].0, spore_keep_id);

        let class_tokens = store
            .list_nft_ids_by_collection(&class_id, None, 10)
            .unwrap();
        assert_eq!(class_tokens.len(), 1);
        assert_eq!(class_tokens[0], nft_keep_id);

        let cluster_agg = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(cluster_agg.total_count, 1);
        assert_eq!(cluster_agg.live_count, 1);
        assert_eq!(cluster_agg.owner_count, 1);
        assert_eq!(cluster_agg.fully_onchain_count, 1);

        let class_agg = store
            .get_nft_collection_aggregate(&class_id)
            .unwrap()
            .unwrap();
        assert_eq!(class_agg.total_count, 1);
        assert_eq!(class_agg.live_count, 1);
    }

    #[test]
    fn test_rollback_truncates_hodl_tracker_state_to_tip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
            timestamp: 1_700_086_400_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.commit().unwrap();

        store
            .put_hodl_tracker_state(&HodlTrackerState {
                capacity_by_date: vec![
                    ("20231114".to_string(), 100),
                    ("20231115".to_string(), 200),
                ],
                date_transitions: vec![(1, "20231114".to_string()), (2, "20231115".to_string())],
                holder_count: 9,
                last_snapshot_date: Some("20231115".to_string()),
            })
            .unwrap();

        store.rollback_to_block(1).unwrap();

        let repaired = store.get_hodl_tracker_state().unwrap().unwrap();
        assert_eq!(repaired.date_transitions, vec![(1, "20231114".to_string())]);
        assert_eq!(
            repaired.capacity_by_date,
            vec![("20231114".to_string(), 100)]
        );
        assert_eq!(repaired.last_snapshot_date, Some("20231114".to_string()));
    }
}
