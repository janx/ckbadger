//! Reorg (rollback) operations.

use rocksdb::{IteratorMode, WriteBatch};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::info;

use crate::keys;
use crate::store::*;
use crate::sync_ops::checked_rollback_total;
use crate::types::*;

/// Bundled rollback delta maps for cutoff-date stats repair.
struct RollbackStatsDeltas {
    /// Per-date: (blocks, txs, cells_created, cells_consumed)
    date: HashMap<String, (i32, i32, i32, i32)>,
    /// Per-date rolled-back uncle count (from CachedBlockHeader.uncles_count).
    /// Used to repair DailyBlockStats.total_uncles on the cutoff date.
    date_uncles: HashMap<String, i32>,
    /// Per-hour: (blocks, txs, cells_created, cells_consumed)
    hour: HashMap<String, (i32, i32, i32, i32)>,
    /// Per-date: (cap_transferred, used_cap_created, used_cap_consumed, data_created, data_consumed)
    date_capacity: HashMap<String, (i128, i128, i128, i64, i64)>,
    /// Per-date activity stats from rolled-back TxActions
    activity_date: HashMap<String, DailyActivityStats>,
    /// Per-hour activity stats from rolled-back TxActions
    activity_hour: HashMap<String, DailyActivityStats>,
    /// Per-(date, miner_lock_hash) rolled-back block count
    miner: HashMap<(String, Vec<u8>), i32>,
}

/// Cell distribution size bucket — must match the logic in
/// `crates/indexer/src/db/writer/cell_distribution.rs::size_bucket`.
fn cell_dist_size_bucket(occupied_capacity: i64) -> usize {
    const CKB: i64 = 100_000_000;
    let ckb = occupied_capacity / CKB;
    match ckb {
        0..=99 => 0,
        100..=999 => 1,
        1_000..=9_999 => 2,
        10_000..=99_999 => 3,
        100_000..=999_999 => 4,
        _ => 5,
    }
}

/// Attempt to repair a daily or hourly stats entry on the cutoff date by
/// subtracting the rolled-back block/tx/cell deltas instead of deleting.
/// Returns `true` if the entry was repaired (caller should NOT delete it),
/// `false` if it should be deleted as before (e.g. not a daily/hourly prefix,
/// or the entire day's blocks were rolled back).
fn repair_cutoff_date_stats(
    key: &[u8],
    value: &[u8],
    cutoff_date: &str,
    cutoff_yyyymmddhh: &str,
    deltas: &RollbackStatsDeltas,
    store: &CkbadgerStore,
    batch: &mut WriteBatch,
) -> anyhow::Result<bool> {
    if key.is_empty() {
        return Ok(false);
    }
    let prefix = key[0];
    let suffix = &key[1..];

    match prefix {
        keys::STATS_PREFIX_DAILY => {
            if suffix.len() < 8 {
                return Ok(false);
            }
            let date_str = std::str::from_utf8(&suffix[..8])
                .map_err(|e| anyhow::anyhow!("invalid daily stats date: {}", e))?;
            if date_str != cutoff_date {
                return Ok(false); // strictly after — delete as before
            }
            let delta = deltas.date.get(date_str);
            let cap_delta = deltas.date_capacity.get(date_str);
            if delta.is_none() && cap_delta.is_none() {
                return Ok(false);
            }
            let (rb_blocks, rb_txs, rb_created, rb_consumed) = delta.copied().unwrap_or_default();
            let (rb_cap, rb_used_created, rb_used_consumed, rb_data_created, rb_data_consumed) =
                cap_delta.copied().unwrap_or_default();

            let mut s: DailyStats = bincode::deserialize(value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily stats for rollback repair: date={}, {}",
                    date_str,
                    e
                )
            })?;
            s.blocks_count -= rb_blocks;
            s.transactions_count -= rb_txs;
            s.cells_created -= rb_created;
            s.cells_consumed -= rb_consumed;
            s.capacity_transferred -= rb_cap;
            s.used_capacity_created -= rb_used_created;
            s.used_capacity_consumed -= rb_used_consumed;
            // Recompute cumulative fields from previous day + corrected per-day values.
            let prev_date_str = {
                let d = chrono::NaiveDate::parse_from_str(date_str, "%Y%m%d")
                    .map_err(|e| anyhow::anyhow!("bad date in stats repair: {}", e))?;
                (d - chrono::Duration::days(1)).format("%Y%m%d").to_string()
            };
            let prev_key =
                keys::encode_stats_key(keys::STATS_PREFIX_DAILY, prev_date_str.as_bytes());
            let (prev_live, prev_dead, prev_all, _prev_data) = store
                .get_stats_key(&prev_key)?
                .map(|v| bincode::deserialize::<DailyStats>(&v))
                .transpose()
                .map_err(|e| anyhow::anyhow!("bad prev-day stats in repair: {}", e))?
                .map(|p| {
                    (
                        p.total_live_cells,
                        p.total_dead_cells,
                        p.total_all_cells,
                        p.total_data_size,
                    )
                })
                .unwrap_or((0, 0, 0, 0));
            s.total_live_cells = prev_live + (s.cells_created - s.cells_consumed) as i64;
            s.total_dead_cells = prev_dead + s.cells_consumed as i64;
            s.total_all_cells = prev_all + s.cells_created as i64;
            // total_data_size = prev_data + (data_added - data_consumed) for the day.
            // Subtract the rolled-back portion's net data contribution.
            s.total_data_size -= rb_data_created - rb_data_consumed;

            let cf = store.stats_cf_by_prefix(prefix)?;
            let encoded = bincode::serialize(&s)
                .map_err(|e| anyhow::anyhow!("serialize daily stats repair: {}", e))?;
            batch.put_cf(cf, key, &encoded);
            Ok(true)
        }
        keys::STATS_PREFIX_HOURLY => {
            if suffix.len() < 10 {
                return Ok(false);
            }
            let hour_str = std::str::from_utf8(&suffix[..10])
                .map_err(|e| anyhow::anyhow!("invalid hourly stats hour: {}", e))?;
            let date_part = &hour_str[..8];
            if date_part != cutoff_date {
                return Ok(false);
            }
            // Only repair the cutoff hour itself; later hours are fully
            // rolled back and should be deleted.
            if hour_str != cutoff_yyyymmddhh {
                return Ok(false);
            }
            let delta = match deltas.hour.get(hour_str) {
                Some(d) => *d,
                None => return Ok(false),
            };
            let (rb_blocks, rb_txs, rb_created, rb_consumed) = delta;
            let mut s: HourlyStats = bincode::deserialize(value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize hourly stats for rollback repair: hour={}, {}",
                    hour_str,
                    e
                )
            })?;
            s.blocks_count -= rb_blocks;
            s.transactions_count -= rb_txs;
            s.cells_created -= rb_created;
            s.cells_consumed -= rb_consumed;
            // capacity_transferred for hourly — subtract from date-level
            // capacity deltas (not tracked per-hour; leave unchanged for hourly)

            let cf = store.stats_cf_by_prefix(prefix)?;
            let encoded = bincode::serialize(&s)
                .map_err(|e| anyhow::anyhow!("serialize hourly stats repair: {}", e))?;
            batch.put_cf(cf, key, &encoded);
            Ok(true)
        }
        keys::STATS_PREFIX_ACTIVITY_DAILY => {
            if suffix.len() < 8 {
                return Ok(false);
            }
            let date_str = std::str::from_utf8(&suffix[..8])
                .map_err(|e| anyhow::anyhow!("invalid activity daily date: {}", e))?;
            if date_str != cutoff_date {
                return Ok(false);
            }
            let delta = match deltas.activity_date.get(date_str) {
                Some(d) => d,
                None => return Ok(false),
            };
            let mut s: DailyActivityStats = bincode::deserialize(value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize activity daily stats for repair: date={}, {}",
                    date_str,
                    e
                )
            })?;
            s.transfer_count = s.transfer_count.saturating_sub(delta.transfer_count);
            s.dao_deposit_count = s.dao_deposit_count.saturating_sub(delta.dao_deposit_count);
            s.dao_withdraw_request_count = s
                .dao_withdraw_request_count
                .saturating_sub(delta.dao_withdraw_request_count);
            s.dao_withdraw_complete_count = s
                .dao_withdraw_complete_count
                .saturating_sub(delta.dao_withdraw_complete_count);
            s.token_count = s.token_count.saturating_sub(delta.token_count);
            s.object_count = s.object_count.saturating_sub(delta.object_count);
            s.identity_count = s.identity_count.saturating_sub(delta.identity_count);
            s.script_call_count = s.script_call_count.saturating_sub(delta.script_call_count);
            s.unknown_count = s.unknown_count.saturating_sub(delta.unknown_count);
            s.coinbase_count = s.coinbase_count.saturating_sub(delta.coinbase_count);
            s.total_ckb_moved = s.total_ckb_moved.saturating_sub(delta.total_ckb_moved);
            // unique_address_count: keep existing value — cutoff-date addr set
            // is preserved (not deleted) so live sync dedup remains correct.
            // Subtract script_counts
            for (k, v) in &delta.script_counts {
                if let Some(existing) = s.script_counts.get_mut(k) {
                    *existing = existing.saturating_sub(*v);
                }
            }
            s.script_counts.retain(|_, v| *v > 0);
            // Subtract protocol_action_counts
            for (k, v) in &delta.protocol_action_counts {
                if let Some(existing) = s.protocol_action_counts.get_mut(k) {
                    *existing = existing.saturating_sub(*v);
                }
            }
            s.protocol_action_counts.retain(|_, v| *v > 0);
            let cf = store.stats_cf_by_prefix(prefix)?;
            let encoded = bincode::serialize(&s)
                .map_err(|e| anyhow::anyhow!("serialize activity daily stats repair: {}", e))?;
            batch.put_cf(cf, key, &encoded);
            Ok(true)
        }
        keys::STATS_PREFIX_ACTIVITY_HOURLY => {
            if suffix.len() < 10 {
                return Ok(false);
            }
            let hour_str = std::str::from_utf8(&suffix[..10])
                .map_err(|e| anyhow::anyhow!("invalid activity hourly hour: {}", e))?;
            let date_part = &hour_str[..8];
            if date_part != cutoff_date {
                return Ok(false);
            }
            // Only repair the cutoff hour; later hours are fully rolled back.
            if hour_str != cutoff_yyyymmddhh {
                return Ok(false);
            }
            let delta = match deltas.activity_hour.get(hour_str) {
                Some(d) => d,
                None => return Ok(false),
            };
            let mut s: DailyActivityStats = bincode::deserialize(value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize activity hourly stats for repair: hour={}, {}",
                    hour_str,
                    e
                )
            })?;
            s.transfer_count = s.transfer_count.saturating_sub(delta.transfer_count);
            s.dao_deposit_count = s.dao_deposit_count.saturating_sub(delta.dao_deposit_count);
            s.dao_withdraw_request_count = s
                .dao_withdraw_request_count
                .saturating_sub(delta.dao_withdraw_request_count);
            s.dao_withdraw_complete_count = s
                .dao_withdraw_complete_count
                .saturating_sub(delta.dao_withdraw_complete_count);
            s.token_count = s.token_count.saturating_sub(delta.token_count);
            s.object_count = s.object_count.saturating_sub(delta.object_count);
            s.identity_count = s.identity_count.saturating_sub(delta.identity_count);
            s.script_call_count = s.script_call_count.saturating_sub(delta.script_call_count);
            s.unknown_count = s.unknown_count.saturating_sub(delta.unknown_count);
            s.coinbase_count = s.coinbase_count.saturating_sub(delta.coinbase_count);
            s.total_ckb_moved = s.total_ckb_moved.saturating_sub(delta.total_ckb_moved);
            for (k, v) in &delta.script_counts {
                if let Some(existing) = s.script_counts.get_mut(k) {
                    *existing = existing.saturating_sub(*v);
                }
            }
            s.script_counts.retain(|_, v| *v > 0);
            for (k, v) in &delta.protocol_action_counts {
                if let Some(existing) = s.protocol_action_counts.get_mut(k) {
                    *existing = existing.saturating_sub(*v);
                }
            }
            s.protocol_action_counts.retain(|_, v| *v > 0);
            let cf = store.stats_cf_by_prefix(prefix)?;
            let encoded = bincode::serialize(&s)
                .map_err(|e| anyhow::anyhow!("serialize activity hourly stats repair: {}", e))?;
            batch.put_cf(cf, key, &encoded);
            Ok(true)
        }
        keys::STATS_PREFIX_DAILY_BLOCK => {
            if suffix.len() < 8 {
                return Ok(false);
            }
            let date_str = std::str::from_utf8(&suffix[..8])
                .map_err(|e| anyhow::anyhow!("invalid daily_block date: {}", e))?;
            if date_str != cutoff_date {
                return Ok(false);
            }
            let rb_blocks = deltas.date.get(date_str).map_or(0, |d| d.0);
            // Absence in date_uncles means no rolled-back block on this date
            // carried an uncle — a legitimate default of 0, not a masked bad
            // state. (Matches the pre-existing pattern for rb_blocks above.)
            let rb_uncles = deltas.date_uncles.get(date_str).copied().unwrap_or(0);
            if rb_blocks == 0 && rb_uncles == 0 {
                return Ok(false);
            }
            let mut s: DailyBlockStats = bincode::deserialize(value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily_block for repair: date={}, {}",
                    date_str,
                    e
                )
            })?;
            s.block_count -= rb_blocks;
            if s.block_count <= 0 {
                return Ok(false); // all blocks rolled back — delete
            }
            // Subtract rolled-back uncles. Fail-fast if this would go negative,
            // which indicates the delta collection is out of sync with persisted
            // state (e.g., historical rows missing uncles_count after a schema
            // upgrade without re-sync).
            s.total_uncles = s.total_uncles.checked_sub(rb_uncles).ok_or_else(|| {
                anyhow::anyhow!(
                    "daily_block total_uncles underflow on rollback repair: date={}, stored_uncles={}, rolled_back_uncles={}",
                    date_str,
                    s.total_uncles,
                    rb_uncles
                )
            })?;
            // checked_sub only catches arithmetic overflow (result below i32::MIN),
            // not simply "went negative" — `0i32.checked_sub(5) == Some(-5)`. This
            // explicit guard catches the case where stored total_uncles is smaller
            // than the rolled-back count, indicating delta collection is out of
            // sync with persisted state.
            if s.total_uncles < 0 {
                anyhow::bail!(
                    "daily_block total_uncles went negative on rollback repair: date={}, result={}",
                    date_str,
                    s.total_uncles
                );
            }
            // avg_difficulty is kept unchanged: per-block difficulty is not stored
            // in CachedBlockHeader, and shallow reorgs don't meaningfully change
            // the daily average.
            let cf = store.stats_cf_by_prefix(prefix)?;
            let encoded = bincode::serialize(&s)
                .map_err(|e| anyhow::anyhow!("serialize daily_block repair: {}", e))?;
            batch.put_cf(cf, key, &encoded);
            Ok(true)
        }
        keys::STATS_PREFIX_MINER => {
            if suffix.len() < 40 {
                return Ok(false);
            }
            let date_str = std::str::from_utf8(&suffix[..8])
                .map_err(|e| anyhow::anyhow!("invalid miner date: {}", e))?;
            if date_str != cutoff_date {
                return Ok(false);
            }
            let miner_hash = suffix[8..40].to_vec();
            let rb_blocks = deltas
                .miner
                .get(&(date_str.to_string(), miner_hash))
                .copied()
                .unwrap_or(0);
            if rb_blocks == 0 {
                // This miner had no blocks in the rolled-back range — preserve as-is.
                return Ok(true);
            }
            let mut s: MinerStats = bincode::deserialize(value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize miner stats for repair: date={}, {}",
                    date_str,
                    e
                )
            })?;
            s.blocks_count -= rb_blocks;
            if s.blocks_count <= 0 {
                return Ok(false); // all blocks by this miner rolled back — delete
            }
            let cf = store.stats_cf_by_prefix(prefix)?;
            let encoded = bincode::serialize(&s)
                .map_err(|e| anyhow::anyhow!("serialize miner stats repair: {}", e))?;
            batch.put_cf(cf, key, &encoded);
            Ok(true)
        }
        // Remaining prefixes (per-entity daily stats, distribution, etc.)
        // are either cumulative or per-entity-per-date and not worth repairing
        // for shallow reorgs. Fall through to deletion.
        _ => Ok(false),
    }
}

fn parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd: &[u8]) -> anyhow::Result<u32> {
    let cutoff_str = std::str::from_utf8(cutoff_yyyymmdd)
        .map_err(|e| anyhow::anyhow!("invalid cutoff date utf8 {:?}: {}", cutoff_yyyymmdd, e))?;
    cutoff_str
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("invalid cutoff date '{}': {}", cutoff_str, e))
}

fn should_delete_stats_for_replay(
    key: &[u8],
    cutoff_yyyymmdd: &[u8],
    cutoff_yyyymmddhh: &[u8],
    cutoff_hour: i64,
    cutoff_epoch: i64,
) -> anyhow::Result<bool> {
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
        | keys::STATS_PREFIX_HODL_WAVE
        | keys::STATS_PREFIX_CELL_DISTRIBUTION
        | keys::STATS_PREFIX_ADDR_COHORT => {
            Ok(suffix.len() >= 8 && &suffix[..8] >= cutoff_yyyymmdd)
        }
        // hour scoped: YYYYMMDDHH
        keys::STATS_PREFIX_HOURLY => Ok(suffix.len() >= 10 && &suffix[..10] >= cutoff_yyyymmddhh),
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
        keys::STATS_PREFIX_OBJECT_DAILY => {
            let cutoff_date = parse_cutoff_date_yyyymmdd(cutoff_yyyymmdd)?;
            if suffix.len() < 36 {
                return Ok(false);
            }
            let date = u32::from_be_bytes(suffix[32..36].try_into().map_err(|_| {
                anyhow::anyhow!("invalid object_daily suffix length: {}", suffix.len())
            })?);
            Ok(date >= cutoff_date)
        }
        // activity daily: YYYYMMDD
        keys::STATS_PREFIX_ACTIVITY_DAILY => {
            Ok(suffix.len() >= 8 && &suffix[..8] >= cutoff_yyyymmdd)
        }
        // Strict > : preserve cutoff-date addr set for dedup continuity
        keys::STATS_PREFIX_ACTIVITY_DAILY_ADDR_SET => {
            Ok(suffix.len() >= 8 && &suffix[..8] > cutoff_yyyymmdd)
        }
        // activity hourly: YYYYMMDDHH
        keys::STATS_PREFIX_ACTIVITY_HOURLY => {
            Ok(suffix.len() >= 10 && &suffix[..10] >= cutoff_yyyymmddhh)
        }
        // Strict > : preserve cutoff-hour addr set for dedup continuity
        keys::STATS_PREFIX_ACTIVITY_HOURLY_ADDR_SET => {
            Ok(suffix.len() >= 10 && &suffix[..10] > cutoff_yyyymmddhh)
        }
        // per-asset hourly transfer counters: entity_hash(32B) + hour_bucket(8B BE i64)
        keys::STATS_PREFIX_TOKEN_HOURLY
        | keys::STATS_PREFIX_SPORE_HOURLY
        | keys::STATS_PREFIX_OBJECT_HOURLY => {
            if suffix.len() < 40 {
                return Ok(false);
            }
            let hour_bucket = i64::from_be_bytes(suffix[32..40].try_into().map_err(|_| {
                anyhow::anyhow!(
                    "invalid per-asset hourly key suffix length: {}",
                    suffix.len()
                )
            })?);
            Ok(hour_bucket >= cutoff_hour)
        }
        // epoch-scoped: prefix(1B) + epoch_number(8B BE i64)
        keys::STATS_PREFIX_EPOCH => {
            if suffix.len() < 8 {
                return Ok(false);
            }
            let epoch = i64::from_be_bytes(suffix[..8].try_into().map_err(|_| {
                anyhow::anyhow!("invalid epoch stats suffix length: {}", suffix.len())
            })?);
            Ok(epoch >= cutoff_epoch)
        }
        // BLOCK_TIME_DIST and EPOCH_TIME_DIST are cumulative histograms
        // spanning the entire chain. A shallow reorg (≤36 blocks) has
        // negligible impact on distributions built over millions of blocks.
        // Deleting them would lose all pre-rollback counts since replay
        // only re-processes blocks after the fork point.
        //
        // DAO singleton aggregates: always delete so they are recomputed after replay
        keys::STATS_PREFIX_DAO_LATEST_STATS | keys::STATS_PREFIX_DAO_TOP_DEPOSITORS => Ok(true),
        // Outpoint/index entries are NOT deleted here. They are append-only
        // historical indexes that cannot be rebuilt from ObjectEntry alone
        // (ObjectEntry lacks the current outpoint). Blanket deletion would
        // destroy outpoint lookups for entities created before rollback_to,
        // since only blocks > rollback_to are replayed. Instead, stage 10
        // selectively cleans up outpoint entries for deleted entities.
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

fn delete_cell_index_entries(
    store: &CkbadgerStore,
    batch: &mut WriteBatch,
    cell: &LiveCellInfo,
    created_at_block: i64,
    tx_hash: &[u8],
    output_index: i16,
) {
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_script_hash,
        created_at_block,
        tx_hash,
        output_index,
    );
    batch.delete_cf(store.cf_cell_by_lock(), &idx_key);
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_code_hash,
        created_at_block,
        tx_hash,
        output_index,
    );
    batch.delete_cf(store.cf_cell_by_lock_code(), &idx_key);
    if let Some(ref type_hash) = cell.type_script_hash {
        let idx_key =
            keys::encode_cell_index_key(type_hash, created_at_block, tx_hash, output_index);
        batch.delete_cf(store.cf_cell_by_type(), &idx_key);
    }
    if let Some(ref type_code_hash) = cell.type_code_hash {
        let idx_key =
            keys::encode_cell_index_key(type_code_hash, created_at_block, tx_hash, output_index);
        batch.delete_cf(store.cf_cell_by_type_code(), &idx_key);
    }
    if let Some(ref data_hash) = cell.data_hash {
        let idx_key =
            keys::encode_cell_index_key(data_hash, created_at_block, tx_hash, output_index);
        batch.delete_cf(store.cf_cell_by_data_hash(), &idx_key);
    }
}

fn put_cell_index_entries(
    store: &CkbadgerStore,
    batch: &mut WriteBatch,
    cell: &LiveCellInfo,
    created_at_block: i64,
    tx_hash: &[u8],
    output_index: i16,
) {
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_script_hash,
        created_at_block,
        tx_hash,
        output_index,
    );
    batch.put_cf(store.cf_cell_by_lock(), &idx_key, []);
    let idx_key = keys::encode_cell_index_key(
        &cell.lock_code_hash,
        created_at_block,
        tx_hash,
        output_index,
    );
    batch.put_cf(store.cf_cell_by_lock_code(), &idx_key, []);
    if let Some(ref type_hash) = cell.type_script_hash {
        let idx_key =
            keys::encode_cell_index_key(type_hash, created_at_block, tx_hash, output_index);
        batch.put_cf(store.cf_cell_by_type(), &idx_key, []);
    }
    if let Some(ref type_code_hash) = cell.type_code_hash {
        let idx_key =
            keys::encode_cell_index_key(type_code_hash, created_at_block, tx_hash, output_index);
        batch.put_cf(store.cf_cell_by_type_code(), &idx_key, []);
    }
    if let Some(ref data_hash) = cell.data_hash {
        let idx_key =
            keys::encode_cell_index_key(data_hash, created_at_block, tx_hash, output_index);
        batch.put_cf(store.cf_cell_by_data_hash(), &idx_key, []);
    }
}

/// Accumulate derived-CF deltas for a cell changing live state during rollback.
/// `sign` is -1 when removing from live (cell created after rollback_to),
/// +1 when restoring to live (cell consumed after rollback_to).
/// Script reference delta tuple:
/// (cells_delta, live_delta, capacity_delta, owned_cap_delta, used_delta, owned_knowledge_delta)
type ScriptReferenceDelta = (i64, i64, i128, i128, i128, i128);

#[allow(clippy::too_many_arguments)]
fn accumulate_cell_deltas(
    cell: &LiveCellInfo,
    sign: i128,
    addr_deltas: &mut HashMap<Vec<u8>, (i128, i128, i32, i64)>,
    script_deltas: &mut HashMap<(Vec<u8>, bool), (i64, i128, i128)>,
    token_holder_deltas: &mut HashMap<(Vec<u8>, Vec<u8>), i128>,
    script_reference_deltas: &mut HashMap<(Vec<u8>, u8, bool), ScriptReferenceDelta>,
    cell_dist_count_deltas: &mut [i64; 6],
    cell_dist_capacity_deltas: &mut [i128; 6],
) {
    let cap = cell.capacity as i128 * sign;
    let occ = cell.occupied_capacity as i128 * sign;
    let live_d = sign as i32;

    // cell distribution bucket: live cell count and capacity per size bucket.
    // sign=+1 when restoring a consumed cell (add back to bucket),
    // sign=-1 when removing a created cell (subtract from bucket).
    let bucket = cell_dist_size_bucket(cell.occupied_capacity);
    cell_dist_count_deltas[bucket] += live_d as i64;
    cell_dist_capacity_deltas[bucket] += occ;

    // addr_balance: (balance_delta, used_delta, live_cells_delta, total_cells_delta)
    // total_cells_delta only counts cells being removed (sign == -1 means cell was created
    // after rollback_to and should be un-counted)
    let total_d = if sign < 0 { -1i64 } else { 0 };
    let e = addr_deltas
        .entry(cell.lock_script_hash.clone())
        .or_insert((0, 0, 0, 0));
    e.0 += cap;
    e.1 += occ;
    e.2 += live_d;
    e.3 += total_d;

    // script_info — lock side: (live_cells_delta, live_cap_delta, live_used_delta)
    let e = script_deltas
        .entry((cell.lock_code_hash.clone(), false))
        .or_insert((0, 0, 0));
    e.0 += live_d as i64;
    e.1 += cap;
    e.2 += occ;

    // script_reference_info — lock side
    let hash_type_u8 = cell.lock_hash_type as u8;
    let e = script_reference_deltas
        .entry((cell.lock_code_hash.clone(), hash_type_u8, false))
        .or_insert((0, 0, 0, 0, 0, 0));
    e.0 += total_d; // cells_delta (total cells created/removed)
    e.1 += live_d as i64; // live_delta
    e.2 += cap; // capacity_delta (owned)
    e.3 += cap; // owned_cap_delta
    e.4 += occ; // used_delta
    e.5 += occ; // owned_knowledge_delta

    // script_info — type side (if present)
    if let Some(ref type_code_hash) = cell.type_code_hash {
        let e = script_deltas
            .entry((type_code_hash.clone(), true))
            .or_insert((0, 0, 0));
        e.0 += live_d as i64;
        e.1 += cap;
        e.2 += occ;

        // script_reference_info — type side
        if let Some(type_hash_type) = cell.type_hash_type {
            let type_ht_u8 = type_hash_type as u8;
            let e = script_reference_deltas
                .entry((type_code_hash.clone(), type_ht_u8, true))
                .or_insert((0, 0, 0, 0, 0, 0));
            e.0 += total_d;
            e.1 += live_d as i64;
            e.2 += cap;
            e.3 += cap;
            e.4 += occ;
            e.5 += occ;
        }
    }

    // token_holder (UDT cells with type_script)
    if let (Some(ref type_script_hash), Some(udt_amount)) =
        (&cell.type_script_hash, cell.udt_amount)
    {
        if udt_amount > 0 {
            *token_holder_deltas
                .entry((type_script_hash.clone(), cell.lock_script_hash.clone()))
                .or_insert(0) += udt_amount as i128 * sign;
        }
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

use crate::bytes_to_hex;

fn truncate_hodl_tracker_state_for_rollback(
    state: &mut HodlTrackerState,
    rollback_to: i64,
    holder_count_delta: i64,
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

    // Apply holder_count correction from addr_balance rollback deltas.
    if holder_count_delta != 0 {
        let new_holder_count = state.holder_count + holder_count_delta;
        if new_holder_count < 0 {
            anyhow::bail!(
                "holder_count underflow during rollback repair: current={}, delta={}, result={}",
                state.holder_count,
                holder_count_delta,
                new_holder_count
            );
        }
        state.holder_count = new_holder_count;
        changed = true;
    }

    // Update last_processed_block to match rollback target.
    if state.last_processed_block.is_some_and(|b| b > rollback_to) {
        state.last_processed_block = Some(rollback_to);
        changed = true;
    }

    Ok(changed)
}

fn truncate_cell_dist_tracker_state_for_rollback(
    state: &mut CellDistributionTrackerState,
    rollback_to: i64,
    count_deltas: &[i64; 6],
    capacity_deltas: &[i128; 6],
) -> anyhow::Result<bool> {
    if rollback_to < 0 {
        return Ok(true);
    }

    let mut changed = false;

    // 1. Truncate date_transitions to blocks <= rollback_to.
    let original_transitions = state.date_transitions.len();
    state
        .date_transitions
        .retain(|(block_num, _)| *block_num <= rollback_to);
    if state.date_transitions.len() != original_transitions {
        changed = true;
    }

    if state.date_transitions.is_empty() {
        anyhow::bail!(
            "invalid cell_dist tracker state after rollback truncate: rollback_to={}, no remaining date transitions",
            rollback_to
        );
    }

    // 2. Get max surviving date.
    let max_date = state
        .date_transitions
        .last()
        .map(|(_, date)| date.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing last date transition while truncating cell_dist tracker state: rollback_to={}",
                rollback_to
            )
        })?;

    // 3. Clamp last_snapshot_date.
    if let Some(last_snapshot_date) = state.last_snapshot_date.as_ref() {
        if last_snapshot_date.as_str() > max_date.as_str() {
            state.last_snapshot_date = Some(max_date);
            changed = true;
        }
    }

    // Note: cohort_accum (address cohort accumulator) cannot be precisely
    // adjusted without per-address tracking. For shallow reorgs the drift is
    // negligible; the next forward sync corrects incrementally.

    // 4. Apply cell distribution bucket deltas from cell rollback.
    // count_deltas/capacity_deltas were accumulated during stages 4-5:
    //   cells removed  → bucket -= 1 / capacity -= occ
    //   cells restored → bucket += 1 / capacity += occ
    for i in 0..6 {
        if count_deltas[i] != 0 || capacity_deltas[i] != 0 {
            let new_count = state.count_by_bucket[i] + count_deltas[i];
            let new_cap = state.total_capacity_by_bucket[i] + capacity_deltas[i];
            if new_count < 0 {
                anyhow::bail!(
                    "cell_dist count_by_bucket underflow during rollback: bucket={}, current={}, delta={}, result={}",
                    i,
                    state.count_by_bucket[i],
                    count_deltas[i],
                    new_count
                );
            }
            if new_cap < 0 {
                anyhow::bail!(
                    "cell_dist total_capacity_by_bucket underflow during rollback: bucket={}, current={}, delta={}, result={}",
                    i,
                    state.total_capacity_by_bucket[i],
                    capacity_deltas[i],
                    new_cap
                );
            }
            state.count_by_bucket[i] = new_count;
            state.total_capacity_by_bucket[i] = new_cap;
            changed = true;
        }
    }

    // Update last_processed_block to match rollback target.
    if state.last_processed_block.is_some_and(|b| b > rollback_to) {
        state.last_processed_block = Some(rollback_to);
        changed = true;
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
    ///
    /// This convenience method is only valid on unified/test stores that also
    /// expose append-only history CFs. Split-store callers must use
    /// `rollback_to_block_with_append_only_store` and pass the append-only store.
    pub fn rollback_to_block(&self, rollback_to: i64) -> anyhow::Result<RollbackResult> {
        if !self.has_cf(CF_ADDR_TXS) || !self.has_cf(CF_OBJECT_COLLECTION_ACTIVITIES) {
            anyhow::bail!(
                "rollback_to_block requires append_only_store when store lacks append-only history CFs; \
                 use rollback_to_block_with_append_only_store(..., append_only_store)"
            );
        }
        self.rollback_to_block_with_append_only_store(rollback_to, None)
    }

    pub fn rollback_to_block_with_append_only_store(
        &self,
        rollback_to: i64,
        append_only_store: Option<&CkbadgerStore>,
    ) -> anyhow::Result<RollbackResult> {
        self.rollback_to_block_impl(rollback_to, append_only_store, None)
    }

    /// Like `rollback_to_block_with_append_only_store` but accepts pre-loaded
    /// tx-context entries extracted by a prior `rollback_via_undo_log` call.
    /// This avoids re-reading from `CF_REORG_UNDO_LOG_BY_BLOCK` after the
    /// undo-log replay has already deleted those entries.
    pub fn rollback_to_block_with_tx_contexts(
        &self,
        rollback_to: i64,
        append_only_store: Option<&CkbadgerStore>,
        tx_contexts: Vec<UndoTxContext>,
    ) -> anyhow::Result<RollbackResult> {
        self.rollback_to_block_impl(rollback_to, append_only_store, Some(tx_contexts))
    }

    fn rollback_to_block_impl(
        &self,
        rollback_to: i64,
        append_only_store: Option<&CkbadgerStore>,
        preloaded_tx_contexts: Option<Vec<UndoTxContext>>,
    ) -> anyhow::Result<RollbackResult> {
        if rollback_to < -1 {
            anyhow::bail!(
                "invalid rollback target: rollback_to={} (expected >= -1)",
                rollback_to
            );
        }
        let cells_store = append_only_store.unwrap_or(self);
        // Persist a rollback marker so startup can force cleanup if interrupted.
        self.set_rollback_cleanup_in_progress(true)?;
        let sync_status_before = self.get_sync_status()?;
        // Only blocks up to the last persisted sync tip were counted into sync_status totals.
        // Startup cleanup may delete partial tail data beyond that point without subtracting it.
        let rollback_accounted_tip = if sync_status_before.tip_block_number == 0
            && sync_status_before.tip_block_hash.is_empty()
        {
            -1
        } else {
            sync_status_before.tip_block_number
        };
        let mut batch = WriteBatch::default();
        let mut blocks_removed = 0u64;
        let mut txs_removed = 0u64;
        let mut cells_removed = 0u64;
        let mut cells_restored = 0u64;
        let mut rollback_total_transactions = 0i64;
        let mut rollback_total_cells_created = 0i64;
        let mut rollback_total_cells_consumed = 0i64;
        let rollback_started_at = Instant::now();
        let replay_start = rollback_to + 1;
        let replay_start_header = self.get_block_header(replay_start)?;
        let replay_cutoff_date = replay_start_header.as_ref().map(|h| {
            ckbadger_common::block_date_from_ms(h.timestamp)
                .format("%Y%m%d")
                .to_string()
        });
        let replay_cutoff_hour = replay_start_header
            .as_ref()
            .map(|h| h.timestamp / 3_600_000);
        let replay_cutoff_epoch = replay_start_header.as_ref().map(|h| h.epoch_number);
        let replay_cutoff_hour_str = replay_start_header.as_ref().map(|h| {
            ckbadger_common::block_datetime_from_ms(h.timestamp)
                .format("%Y%m%d%H")
                .to_string()
        });

        // Determine the date of the fork_point itself so we can detect
        // partial-day rollbacks (fork_point and first rolled-back block on
        // the same calendar day).
        let fork_point_date = if rollback_to >= 0 {
            self.get_block_header(rollback_to)?.map(|h| {
                ckbadger_common::block_date_from_ms(h.timestamp)
                    .format("%Y%m%d")
                    .to_string()
            })
        } else {
            None
        };

        info!(rollback_to, replay_start, "Rollback cleanup started");

        // block_date_map: block_num → (date_yyyymmdd, hour_yyyymmddhh) for
        // rolled-back blocks, used for per-date stats delta subtraction.
        let mut block_date_map: HashMap<i64, (String, String)> = HashMap::new();
        // Per-date rolled-back uncle count, populated during block header deletion loop.
        let mut stats_date_uncles: HashMap<String, i32> = HashMap::new();

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
            // Collect block→date/hour mapping for stats delta subtraction.
            let block_dt = ckbadger_common::block_datetime_from_ms(header.timestamp);
            let date_str = block_dt.format("%Y%m%d").to_string();
            let hour_str = block_dt.format("%Y%m%d%H").to_string();
            // Accumulate per-date uncle count for repair_cutoff_date_stats.
            let uncles_entry = stats_date_uncles.entry(date_str.clone()).or_insert(0);
            *uncles_entry = uncles_entry
                .checked_add(header.uncles_count)
                .ok_or_else(|| anyhow::anyhow!(
                    "uncles_count overflow during rollback delta accumulation: block_num={}",
                    block_num
                ))?;
            block_date_map.insert(block_num, (date_str, hour_str));

            batch.delete_cf(self.cf_block_headers(), &key);
            batch.delete_cf(self.cf_block_hash_index(), &header.hash);
            blocks_removed += 1;
            stage.tick(blocks_removed);
        }
        stage.finish(blocks_removed);

        // Per-date stats rollback deltas: for the cutoff date, we subtract
        // these from the existing daily/hourly stats instead of deleting.
        // Keyed by date_yyyymmdd.  Fields: (blocks, txs, cells_created, cells_consumed).
        let mut stats_date_deltas: HashMap<String, (i32, i32, i32, i32)> = HashMap::new();
        // Per-hour stats rollback deltas, keyed by hour_yyyymmddhh.
        let mut stats_hour_deltas: HashMap<String, (i32, i32, i32, i32)> = HashMap::new();
        // Per-date capacity/used_capacity/data_size deltas, populated during cell rollback.
        // Keyed by date_yyyymmdd: (capacity_transferred, used_capacity_created,
        //                          used_capacity_consumed, data_size_created, data_size_consumed)
        let mut stats_date_capacity_deltas: HashMap<String, (i128, i128, i128, i64, i64)> =
            HashMap::new();
        // Cellbase tx hashes for rolled-back blocks (used to exclude cellbase
        // outputs from capacity_transferred in stage 4).
        let mut cellbase_tx_hashes: HashSet<Vec<u8>> = HashSet::new();

        // Count rolled-back blocks per date for the blocks_count delta.
        for (date_str, hour_str) in block_date_map.values() {
            let e = stats_date_deltas.entry(date_str.clone()).or_default();
            e.0 += 1;
            let e = stats_hour_deltas.entry(hour_str.clone()).or_default();
            e.0 += 1;
        }

        // 2. Delete tx_index entries > rollback_to
        let mut stage = RollbackStageProgress::new("delete_tx_index");
        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_tx_index(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter {
            let (key, value) = item.map_err(|e| {
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
                let tx_idx = keys::decode_tx_idx(&key[8..12]);
                let tx_index: TxIndexEntry = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize tx_index during rollback cleanup: block_num={}, tx_idx={}, key=0x{}, error={}",
                        block_num,
                        tx_idx,
                        bytes_to_hex(&key),
                        e
                    )
                })?;
                let cells_created_removed = i64::from(tx_index.outputs_count);
                let inputs_count = i64::from(tx_index.inputs_count);
                if cells_created_removed < 0 || inputs_count < 0 {
                    anyhow::bail!(
                        "invalid negative tx_index counts during rollback cleanup: block_num={}, tx_idx={}, inputs_count={}, outputs_count={}",
                        block_num,
                        tx_idx,
                        tx_index.inputs_count,
                        tx_index.outputs_count
                    );
                }
                let cells_consumed_removed = if tx_index.is_cellbase {
                    0
                } else {
                    inputs_count
                };
                batch.delete_cf(self.cf_tx_index(), &key);
                txs_removed += 1;

                // Accumulate per-date and per-hour tx/cell deltas.
                if let Some((date_str, hour_str)) = block_date_map.get(&block_num) {
                    let de = stats_date_deltas.entry(date_str.clone()).or_default();
                    de.1 += 1; // txs
                    de.2 += tx_index.outputs_count as i32; // cells_created
                    if !tx_index.is_cellbase {
                        de.3 += tx_index.inputs_count as i32; // cells_consumed
                    }
                    let he = stats_hour_deltas.entry(hour_str.clone()).or_default();
                    he.1 += 1;
                    he.2 += tx_index.outputs_count as i32;
                    if !tx_index.is_cellbase {
                        he.3 += tx_index.inputs_count as i32;
                    }
                }

                if block_num <= rollback_accounted_tip {
                    rollback_total_transactions = rollback_total_transactions
                        .checked_add(1)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "rollback total_transactions delta overflow: block_num={}, tx_idx={}",
                                block_num,
                                tx_idx
                            )
                        })?;
                    rollback_total_cells_created = rollback_total_cells_created
                        .checked_add(cells_created_removed)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "rollback total_cells_created delta overflow: block_num={}, tx_idx={}, outputs_count={}",
                                block_num,
                                tx_idx,
                                tx_index.outputs_count
                            )
                        })?;
                    rollback_total_cells_consumed = rollback_total_cells_consumed
                        .checked_add(cells_consumed_removed)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "rollback total_cells_consumed delta overflow: block_num={}, tx_idx={}, inputs_count={}",
                                block_num,
                                tx_idx,
                                tx_index.inputs_count
                            )
                        })?;
                }
            }
            stage.tick(txs_removed);
        }
        stage.finish(txs_removed);

        // 3. Delete tx_hash_map entries for rolled-back transactions.
        // Use pre-loaded tx-context entries when available (from prior
        // rollback_via_undo_log), otherwise read from the undo log CF.
        let tx_contexts = match preloaded_tx_contexts {
            Some(ctx) => ctx,
            None => load_tx_contexts_from_undo_log(self, rollback_to)?,
        };
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
                // Collect cellbase tx hashes before deleting the mapping.
                if let Some(val) = self.get_cf(self.cf_tx_hash_map(), &ctx.tx_hash)? {
                    if val.len() == 12 {
                        let tx_idx = keys::decode_tx_idx(&val[8..12]);
                        if tx_idx == 0 {
                            cellbase_tx_hashes.insert(ctx.tx_hash.clone());
                        }
                    }
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
                    let tx_idx = keys::decode_tx_idx(&value[8..12]);
                    if tx_idx == 0 {
                        cellbase_tx_hashes.insert(key.to_vec());
                    }
                    batch.delete_cf(self.cf_tx_hash_map(), &key);
                    tx_hash_map_removed += 1;
                }
                stage.tick(tx_hash_map_removed);
            }
        }
        stage.finish(tx_hash_map_removed);

        // Helper: accumulate per-date capacity deltas for stats repair.
        // `tx_hash`: first 32 bytes of outpoint key
        // `created_at_block`: block where cell was created
        // `is_removal`: true if cell is being removed (created after fork), false if restored
        let accumulate_stats_capacity_delta =
            |cell: &LiveCellInfo,
             tx_hash: &[u8],
             created_at_block: i64,
             consumed_at_block: Option<i64>,
             is_removal: bool,
             block_date_map: &HashMap<i64, (String, String)>,
             cellbase_tx_hashes: &HashSet<Vec<u8>>,
             date_cap_deltas: &mut HashMap<String, (i128, i128, i128, i64, i64)>| {
                if is_removal {
                    // Cell was created after fork_point — subtract its contribution.
                    if let Some((date_str, _)) = block_date_map.get(&created_at_block) {
                        let e = date_cap_deltas.entry(date_str.clone()).or_default();
                        // capacity_transferred: only non-cellbase tx outputs
                        if !cellbase_tx_hashes.contains(tx_hash) {
                            e.0 += cell.capacity as i128;
                        }
                        e.1 += cell.occupied_capacity as i128; // used_capacity_created
                        e.3 += cell.data_size as i64; // data_size_created
                    }
                } else {
                    // Cell was consumed after fork_point — subtract its consumption.
                    if let Some(consumed_block) = consumed_at_block {
                        if let Some((date_str, _)) = block_date_map.get(&consumed_block) {
                            let e = date_cap_deltas.entry(date_str.clone()).or_default();
                            e.2 += cell.occupied_capacity as i128; // used_capacity_consumed
                            e.4 += cell.data_size as i64; // data_size_consumed
                        }
                    }
                }
            };

        // 4-5. Roll back cell/live/consumed/index state.
        // Prefer tx-context entries from reorg_undo_log_by_block to derive touched outpoints.
        // Fallback to full scans when tx-context coverage is missing or partial.

        // Delta accumulators for derived CFs, populated during cell rollback.
        // addr_deltas: lock_hash -> (balance_delta, used_delta, live_cells_delta)
        let mut addr_balance_deltas: HashMap<Vec<u8>, (i128, i128, i32, i64)> = HashMap::new();
        // script_deltas: (code_hash, is_type) -> (live_cells_delta, live_cap_delta, live_occ_delta)
        let mut script_info_deltas: HashMap<(Vec<u8>, bool), (i64, i128, i128)> = HashMap::new();
        // token_holder_deltas: (type_hash, lock_hash) -> balance_delta
        let mut token_holder_deltas: HashMap<(Vec<u8>, Vec<u8>), i128> = HashMap::new();
        // script_reference_deltas: (code_hash, hash_type, is_type) -> ScriptReferenceDelta
        let mut script_reference_deltas: HashMap<(Vec<u8>, u8, bool), ScriptReferenceDelta> =
            HashMap::new();
        // cell_dist_bucket_deltas: per-bucket (count_delta, capacity_delta) for cell
        // distribution tracker repair.  Cells removed (created after fork_point)
        // subtract from buckets; cells restored (consumed after fork_point) add back.
        let mut cell_dist_count_deltas: [i64; 6] = [0; 6];
        let mut cell_dist_capacity_deltas: [i128; 6] = [0; 6];

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
                let positioned = self
                    .get_live_cell_by_outpoint_key(&key, cells_store)?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "missing canonical cell for live outpoint during rollback fallback: outpoint=0x{}:{}",
                            bytes_to_hex(&tx_hash),
                            output_index
                        )
                    })?;
                if positioned.created_at_block > rollback_to {
                    batch.delete_cf(self.cf_live_cells(), &key);
                    delete_cell_index_entries(
                        self,
                        &mut batch,
                        &positioned.cell,
                        positioned.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    cells_removed += 1;
                    accumulate_cell_deltas(
                        &positioned.cell,
                        -1,
                        &mut addr_balance_deltas,
                        &mut script_info_deltas,
                        &mut token_holder_deltas,
                        &mut script_reference_deltas,
                        &mut cell_dist_count_deltas,
                        &mut cell_dist_capacity_deltas,
                    );
                    accumulate_stats_capacity_delta(
                        &positioned.cell,
                        &tx_hash,
                        positioned.created_at_block,
                        None,
                        true,
                        &block_date_map,
                        &cellbase_tx_hashes,
                        &mut stats_date_capacity_deltas,
                    );
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
                let meta = decode_consumed_cell_meta(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to decode consumed cell metadata during rollback fallback: outpoint=0x{}, error={}",
                        bytes_to_hex(&key),
                        e
                    )
                })?;
                if meta.consumed_at_block <= rollback_to {
                    stage.tick(cells_restored);
                    continue;
                }

                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                batch.delete_cf(self.cf_consumed_cells(), &key);
                let info = cells_store
                    .get_cell_by_outpoint_key(&key)?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "missing canonical cell for consumed outpoint during rollback fallback: outpoint=0x{}:{}",
                            bytes_to_hex(&tx_hash),
                            output_index
                        )
                    })?;
                if meta.created_at_block <= rollback_to {
                    batch.put_cf(
                        self.cf_live_cells(),
                        &key,
                        encode_live_cell_marker(meta.created_at_block),
                    );
                    put_cell_index_entries(
                        self,
                        &mut batch,
                        &info,
                        meta.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    cells_restored += 1;
                    accumulate_cell_deltas(
                        &info,
                        1,
                        &mut addr_balance_deltas,
                        &mut script_info_deltas,
                        &mut token_holder_deltas,
                        &mut script_reference_deltas,
                        &mut cell_dist_count_deltas,
                        &mut cell_dist_capacity_deltas,
                    );
                    accumulate_stats_capacity_delta(
                        &info,
                        &tx_hash,
                        meta.created_at_block,
                        Some(meta.consumed_at_block),
                        false,
                        &block_date_map,
                        &cellbase_tx_hashes,
                        &mut stats_date_capacity_deltas,
                    );
                }
                stage.tick(cells_restored);
            }
            stage.finish(cells_restored);
        } else {
            let mut stage = RollbackStageProgress::new("rollback_cells_from_tx_context");
            // tx_contexts is already in newest-first (LIFO) order from
            // rollback_via_undo_log — do NOT reverse again.
            for ctx in tx_contexts.into_iter() {
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
                        let positioned = self
                            .get_live_cell_by_outpoint_key(&outpoint_key, cells_store)?
                            .ok_or_else(|| {
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
                            &positioned.cell,
                            positioned.created_at_block,
                            &ctx.tx_hash,
                            output_index,
                        );
                        cells_removed += 1;
                        accumulate_cell_deltas(
                            &positioned.cell,
                            -1,
                            &mut addr_balance_deltas,
                            &mut script_info_deltas,
                            &mut token_holder_deltas,
                            &mut script_reference_deltas,
                            &mut cell_dist_count_deltas,
                            &mut cell_dist_capacity_deltas,
                        );
                        accumulate_stats_capacity_delta(
                            &positioned.cell,
                            &ctx.tx_hash,
                            positioned.created_at_block,
                            None,
                            true,
                            &block_date_map,
                            &cellbase_tx_hashes,
                            &mut stats_date_capacity_deltas,
                        );
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
                    match self.get_consumed_cell_info(
                        &input.tx_hash,
                        input.output_index,
                        cells_store,
                    )? {
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
                            if consumed.created_at_block <= rollback_to {
                                batch.put_cf(
                                    self.cf_live_cells(),
                                    outpoint_key,
                                    encode_live_cell_marker(consumed.created_at_block),
                                );
                                put_cell_index_entries(
                                    self,
                                    &mut batch,
                                    &consumed.cell,
                                    consumed.created_at_block,
                                    &input.tx_hash,
                                    input.output_index,
                                );
                                cells_restored += 1;
                                accumulate_cell_deltas(
                                    &consumed.cell,
                                    1,
                                    &mut addr_balance_deltas,
                                    &mut script_info_deltas,
                                    &mut token_holder_deltas,
                                    &mut script_reference_deltas,
                                    &mut cell_dist_count_deltas,
                                    &mut cell_dist_capacity_deltas,
                                );
                                accumulate_stats_capacity_delta(
                                    &consumed.cell,
                                    &input.tx_hash,
                                    consumed.created_at_block,
                                    Some(consumed.consumed_at_block),
                                    false,
                                    &block_date_map,
                                    &cellbase_tx_hashes,
                                    &mut stats_date_capacity_deltas,
                                );
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
        // Use delete_range_cf to wipe each CF in O(1) instead of per-key iteration.
        let dao_index_cfs = [
            self.cf_dao_by_withdraw_tx(),
            self.cf_dao_by_block(),
            self.cf_dao_by_lock_block(),
            self.cf_dao_by_status_block(),
        ];
        for cf in dao_index_cfs {
            batch.delete_range_cf::<&[u8]>(cf, &[], &[0xFF; 128]);
        }
        info!("rollback: cleared 4 DAO secondary index CFs via delete_range");

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
        // For the cutoff date itself (when fork_point is on the same day),
        // subtract per-date rollback deltas instead of deleting so that the
        // retained portion of that day is preserved.
        if let Some(cutoff) = replay_cutoff_date.as_deref() {
            let cutoff_hour =
                replay_cutoff_hour.expect("cutoff_hour must be set when cutoff_date is set");
            let cutoff_epoch =
                replay_cutoff_epoch.expect("cutoff_epoch must be set when cutoff_date is set");
            let cutoff_hour_str = replay_cutoff_hour_str
                .as_deref()
                .expect("cutoff_hour_str must be set when cutoff_date is set");
            // Detect partial-day rollback: fork_point and first rolled-back block
            // share the same calendar date.
            let is_partial_day = fork_point_date.as_deref().is_some_and(|fpd| fpd == cutoff);

            // Pre-scan tx_actions for activity + miner rollback deltas (partial-day only).
            let mut activity_date_deltas: HashMap<String, DailyActivityStats> = HashMap::new();
            let mut activity_hour_deltas: HashMap<String, DailyActivityStats> = HashMap::new();
            let mut miner_deltas: HashMap<(String, Vec<u8>), i32> = HashMap::new();
            if is_partial_day {
                let mut stage = RollbackStageProgress::new("prescan_tx_actions_for_deltas");
                let mut scanned = 0u64;
                let iter = self.iterator_cf(self.cf_tx_actions(), IteratorMode::Start);
                for item in iter {
                    let (key, value) = item.map_err(|e| {
                        anyhow::anyhow!(
                            "failed to iterate tx_actions for rollback delta prescan: {}",
                            e
                        )
                    })?;
                    if key.len() != keys::TX_ACTIONS_KEY_SIZE {
                        continue;
                    }
                    let (block_num, _tx_idx, _tx_hash) = keys::decode_tx_actions_key(&key);
                    if block_num <= rollback_to {
                        break;
                    }
                    let tx_actions: TxActions =
                        bincode::deserialize(&value).map_err(|e| {
                            anyhow::anyhow!(
                                "failed to deserialize TxActions for rollback delta prescan: block_num={}, {}",
                                block_num,
                                e
                            )
                        })?;
                    let dt = ckbadger_common::block_datetime_from_ms(tx_actions.timestamp);
                    let date_str = dt.format("%Y%m%d").to_string();
                    let hour_str = dt.format("%Y%m%d%H").to_string();

                    activity_date_deltas
                        .entry(date_str.clone())
                        .or_default()
                        .accumulate_from_tx_actions(&tx_actions);
                    activity_hour_deltas
                        .entry(hour_str)
                        .or_default()
                        .accumulate_from_tx_actions(&tx_actions);

                    // Miner identification: cellbase first participant's lock hash
                    if tx_actions.is_cellbase {
                        if let Some(p) = tx_actions.participants.first() {
                            if p.lock_hash.len() == 32 {
                                *miner_deltas
                                    .entry((date_str, p.lock_hash.clone()))
                                    .or_insert(0) += 1;
                            }
                        }
                    }

                    scanned += 1;
                    stage.tick(scanned);
                }
                stage.finish(scanned);
            }

            let rollback_deltas = RollbackStatsDeltas {
                date: stats_date_deltas,
                date_uncles: stats_date_uncles,
                hour: stats_hour_deltas,
                date_capacity: stats_date_capacity_deltas,
                activity_date: activity_date_deltas,
                activity_hour: activity_hour_deltas,
                miner: miner_deltas,
            };

            let mut stats_removed = 0u64;
            let mut stats_repaired = 0u64;
            let mut stage = RollbackStageProgress::new("delete_stats_from_cutoff");
            let stats_cfs = [
                self.cf_stats_chain(),
                self.cf_stats_dao(),
                self.cf_stats_hodl(),
                self.cf_stats_script(),
                self.cf_stats_token(),
                self.cf_stats_spore(),
                self.cf_stats_mnft(),
            ];
            for cf in stats_cfs {
                let iter = self.iterator_cf(cf, IteratorMode::Start);
                for item in iter {
                    let (key, value) = item.map_err(|e| {
                        anyhow::anyhow!(
                            "failed to iterate stats CF in rollback_to_block cleanup: {}",
                            e
                        )
                    })?;
                    if !should_delete_stats_for_replay(
                        &key,
                        cutoff.as_bytes(),
                        cutoff_hour_str.as_bytes(),
                        cutoff_hour,
                        cutoff_epoch,
                    )? {
                        stage.tick(stats_removed + stats_repaired);
                        continue;
                    }
                    // For daily/hourly main stats on the cutoff date in a partial-day
                    // rollback, subtract the rolled-back deltas instead of deleting.
                    if is_partial_day && !key.is_empty() {
                        let repaired = repair_cutoff_date_stats(
                            &key,
                            &value,
                            cutoff,
                            cutoff_hour_str,
                            &rollback_deltas,
                            self,
                            &mut batch,
                        )?;
                        if repaired {
                            stats_repaired += 1;
                            stage.tick(stats_removed + stats_repaired);
                            continue;
                        }
                    }
                    batch.delete_cf(cf, &key);
                    stats_removed += 1;
                    stage.tick(stats_removed + stats_repaired);
                }
            }
            stage.finish(stats_removed + stats_repaired);
            if stats_repaired > 0 {
                info!(
                    stats_repaired,
                    stats_removed, "rollback: cutoff-day stats repaired via delta subtraction"
                );
            }

            // 7b. Recompute DAO daily snapshots for all dates affected by
            // the rollback. Runs AFTER the dao_deposits repair stage and
            // AFTER the date-scoped stats deletion. This reads the now-correct
            // dao_deposits CF and walks block_headers forward from the start
            // of each affected date to produce fully correct snapshots —
            // including cum_miner_secondary / cum_dao_compensation / cum_treasury
            // and secondary_pool / total_issuance / occupied_capacity re-read
            // from the last surviving block's DAO header.
            //
            // Runs for both partial-day and cross-day rollbacks:
            // - Partial-day: recomputes just the cutoff date up to rollback_to
            // - Cross-day: recomputes every date from fork_point_date through
            //   cutoff_date (inclusive), each bounded by its end-of-day block.
            let cutoff_naive = chrono::NaiveDate::parse_from_str(cutoff, "%Y%m%d")
                .map_err(|e| anyhow::anyhow!("invalid cutoff_date {}: {}", cutoff, e))?;
            let recompute_start = if let Some(fpd) = fork_point_date.as_deref() {
                chrono::NaiveDate::parse_from_str(fpd, "%Y%m%d")
                    .map_err(|e| anyhow::anyhow!("invalid fork_point_date {}: {}", fpd, e))?
            } else {
                cutoff_naive
            };
            let mut dao_recompute_stage =
                RollbackStageProgress::new("recompute_dao_daily_snapshots");
            let mut ticks = 0u64;
            let mut d = recompute_start;
            while d <= cutoff_naive {
                // recompute_dao_daily_snapshot_for_date takes &mut StoreBatch,
                // while rollback_to_block accumulates into a raw WriteBatch.
                // Build a temporary StoreBatch, run the recompute, then extract
                // the inner WriteBatch and merge its serialized operations into
                // the main batch so all rollback writes commit atomically.
                let mut recompute_batch = crate::batch::StoreBatch::new(self);
                self.recompute_dao_daily_snapshot_for_date(
                    d,
                    rollback_to,
                    &mut recompute_batch,
                )?;
                let recompute_wb = recompute_batch.into_write_batch();
                if !recompute_wb.is_empty() {
                    // Merge via RocksDB WriteBatch wire format:
                    // [0..8] sequence (u64 LE), [8..12] entry count (u32 LE), [12..] ops.
                    let main_data = batch.data();
                    let extra_data = recompute_wb.data();
                    let main_count = u32::from_le_bytes(
                        main_data[8..12].try_into().expect("WriteBatch header >= 12 bytes"),
                    );
                    let extra_count = u32::from_le_bytes(
                        extra_data[8..12].try_into().expect("WriteBatch header >= 12 bytes"),
                    );
                    let total_count = main_count.checked_add(extra_count).expect(
                        "WriteBatch operation count overflow during DAO snapshot recompute merge",
                    );
                    let mut merged =
                        Vec::with_capacity(main_data.len() + extra_data.len() - 12);
                    merged.extend_from_slice(&main_data[..8]); // sequence from main
                    merged.extend_from_slice(&total_count.to_le_bytes()); // combined count
                    merged.extend_from_slice(&main_data[12..]); // ops from main
                    merged.extend_from_slice(&extra_data[12..]); // ops from recompute
                    batch = WriteBatch::from_data(&merged);
                }
                ticks += 1;
                dao_recompute_stage.tick(ticks);
                d += chrono::Duration::days(1);
            }
            dao_recompute_stage.finish(ticks);
            info!(
                cutoff = cutoff,
                recompute_start = %recompute_start,
                dates = ticks,
                "rollback: recomputed DAO daily snapshots from dao_deposits"
            );
        }

        // 8. Delete token_transfers entries > rollback_to
        // Key: type_hash(32) + block_num_desc(8) + tx_idx(4) = 44
        // Per-type_hash count of deleted transfers, for TokenInfo.transfers_count update.
        let mut transfer_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
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
                    let type_hash = key[0..32].to_vec();
                    *transfer_count_deltas.entry(type_hash).or_insert(0) += 1;
                }
            }
            stage.tick(token_transfers_removed);
        }
        stage.finish(token_transfers_removed);

        // 8b. Delete tx activity bundles for rolled-back blocks.
        // Activities are now in domain store, so we can delete directly.
        {
            let mut activities_removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_activities");
            let iter = self.iterator_cf(self.cf_tx_actions(), IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate activities in rollback_to_block cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::TX_ACTIONS_KEY_SIZE {
                    continue;
                }
                let (block_num, _tx_idx, _tx_hash) = keys::decode_tx_actions_key(&key);
                if block_num <= rollback_to {
                    // Keys are in descending block_num order; all remaining entries
                    // are also <= rollback_to, so stop scanning.
                    break;
                }
                batch.delete_cf(self.cf_tx_actions(), &key);
                activities_removed += 1;
                stage.tick(activities_removed);
            }
            stage.finish(activities_removed);
            if activities_removed > 0 {
                info!(activities_removed, "rollback: deleted tx activity bundles");
            }
        }

        // 8c. Delete addr_txs entries for rolled-back blocks.
        // Also count per-address tx removals to correct txs_count in stage 9a,
        // and track the latest surviving addr_tx per address for last_activity repair.
        let mut addr_txs_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
        let mut addr_latest_surviving: HashMap<Vec<u8>, (i64, Vec<u8>)> = HashMap::new();
        {
            let mut addr_txs_removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_addr_txs");
            let iter = self.iterator_cf(self.cf_addr_txs(), IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate addr_txs in rollback_to_block cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::ADDR_TX_KEY_SIZE {
                    continue;
                }
                let (lock_hash, block_num, _tx_idx, tx_hash) = keys::decode_addr_tx_key(&key);
                if block_num <= rollback_to {
                    // Track latest surviving entry per address (keys are desc by block_num,
                    // so first surviving entry per address is the latest)
                    addr_latest_surviving
                        .entry(lock_hash)
                        .or_insert_with(|| (block_num, tx_hash));
                    stage.tick(addr_txs_removed);
                    continue;
                }
                batch.delete_cf(self.cf_addr_txs(), &key);
                *addr_txs_count_deltas.entry(lock_hash).or_insert(0) += 1;
                addr_txs_removed += 1;
                stage.tick(addr_txs_removed);
            }
            stage.finish(addr_txs_removed);
            if addr_txs_removed > 0 {
                info!(addr_txs_removed, "rollback: deleted addr_txs entries");
            }
        }

        // 8d. Delete object_collection_activities entries for rolled-back blocks.
        // Also count surviving entries per collection to avoid a duplicate scan in stage 10.
        let mut object_activity_totals: HashMap<Vec<u8>, i64> = HashMap::new();
        {
            let mut removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_object_collection_activities");
            let iter =
                self.iterator_cf(self.cf_object_collection_activities(), IteratorMode::Start);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate object_collection_activities in rollback_to_block cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::OBJECT_COLLECTION_ACTIVITY_KEY_SIZE {
                    continue;
                }
                let (collection_id, block_num, _tx_idx, _block_hash, _tx_hash) =
                    keys::decode_object_collection_activity_key(&key);
                if block_num <= rollback_to {
                    // Surviving entry — count it for the aggregate rebuild.
                    let total = object_activity_totals
                        .entry(collection_id.to_vec())
                        .or_insert(0);
                    *total = total.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "object collection activities_count overflow while counting survivors in rollback"
                        )
                    })?;
                    stage.tick(removed);
                    continue;
                }
                batch.delete_cf(self.cf_object_collection_activities(), &key);
                removed += 1;
                stage.tick(removed);
            }
            stage.finish(removed);
            if removed > 0 {
                info!(
                    removed,
                    "rollback: deleted object_collection_activity entries"
                );
            }
        }

        // 8e. Delete identity_collection_activities entries for rolled-back blocks.
        // Also count surviving entries per collection to avoid a duplicate scan in stage 10.
        let mut identity_activity_totals: HashMap<Vec<u8>, i64> = HashMap::new();
        {
            let mut removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_identity_collection_activities");
            let iter = self.iterator_cf(
                self.cf_identity_collection_activities(),
                IteratorMode::Start,
            );
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate identity_collection_activities in rollback_to_block cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::OBJECT_COLLECTION_ACTIVITY_KEY_SIZE {
                    continue;
                }
                let (collection_id, block_num, _tx_idx, _block_hash, _tx_hash) =
                    keys::decode_object_collection_activity_key(&key);
                if block_num <= rollback_to {
                    // Surviving entry — count it for the aggregate rebuild.
                    let total = identity_activity_totals
                        .entry(collection_id.to_vec())
                        .or_insert(0);
                    *total = total.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "identity collection activities_count overflow while counting survivors in rollback"
                        )
                    })?;
                    stage.tick(removed);
                    continue;
                }
                batch.delete_cf(self.cf_identity_collection_activities(), &key);
                removed += 1;
                stage.tick(removed);
            }
            stage.finish(removed);
            if removed > 0 {
                info!(
                    removed,
                    "rollback: deleted identity_collection_activity entries"
                );
            }
        }

        // 8f. Delete Fiber channel data for rolled-back blocks.
        // Simplest approach for dev: wipe all Fiber CFs and let resync rebuild.
        {
            let mut fiber_removed = 0u64;
            let mut stage = RollbackStageProgress::new("delete_fiber_channels");

            // Delete all entries in CF_FIBER_CHANNELS where open_block > rollback_to.
            // Also collect channel_ids and participant info for secondary index cleanup.
            let iter = self.iterator_cf(self.cf_fiber_channels(), IteratorMode::Start);
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate fiber_channels in rollback_to_block cleanup: {}",
                        e
                    )
                })?;
                if key.len() != keys::FIBER_CHANNEL_KEY_SIZE {
                    continue;
                }
                let channel: FiberChannel = match bincode::deserialize(&value) {
                    Ok(ch) => ch,
                    Err(e) => {
                        info!(
                            error = %e,
                            key_hex = %crate::bytes_to_hex(&key),
                            "rollback: failed to deserialize fiber channel, deleting"
                        );
                        batch.delete_cf(self.cf_fiber_channels(), &key);
                        fiber_removed += 1;
                        continue;
                    }
                };

                if channel.open_block > rollback_to {
                    // Channel was opened in a rolled-back block — delete it and all indexes
                    batch.delete_cf(self.cf_fiber_channels(), &key);

                    // Delete addr_fiber_channel entries for each participant
                    for participant in &channel.participants {
                        let addr_key = keys::encode_addr_fiber_channel_key(participant, &key);
                        batch.delete_cf(self.cf_addr_fiber_channels(), &addr_key);
                    }

                    // Delete funding_args index
                    if !channel.funding_lock_args.is_empty() {
                        batch.delete_cf(
                            self.cf_fiber_channel_by_funding_args(),
                            &channel.funding_lock_args,
                        );
                    }

                    // Delete commitment index if present
                    // We don't have easy access to the commitment hash used as key,
                    // so we handle this in the full CF sweep below.

                    fiber_removed += 1;
                } else if channel.close_block.is_some_and(|b| b > rollback_to)
                    || channel.settlement_block.is_some_and(|b| b > rollback_to)
                {
                    // Channel was opened before rollback but modified after.
                    // Determine the correct restored state based on which
                    // events survive the rollback.
                    let close_survives = channel.close_block.is_some_and(|b| b <= rollback_to);
                    let mut reset_channel = channel.clone();

                    if close_survives {
                        // Force-close happened before rollback point — restore to
                        // ForceClosed, clearing only settlement fields.
                        reset_channel.state = FiberChannelState::ForceClosed;
                        reset_channel.settlement_tx_hash = None;
                        reset_channel.settlement_block = None;
                        reset_channel.settlement_timestamp = None;
                    } else {
                        // Close also happened after rollback — reset to Open.
                        reset_channel.state = FiberChannelState::Open;
                        reset_channel.close_tx_hash = None;
                        reset_channel.close_block = None;
                        reset_channel.close_timestamp = None;
                        reset_channel.commitment_tx_hash = None;
                        reset_channel.commitment_output_index = None;
                        reset_channel.delay_epoch = None;
                        reset_channel.settlement_tx_hash = None;
                        reset_channel.settlement_block = None;
                        reset_channel.settlement_timestamp = None;
                    }

                    let value = bincode::serialize(&reset_channel).expect("serialize FiberChannel");
                    batch.put_cf(self.cf_fiber_channels(), &key, &value);
                    fiber_removed += 1; // count as modified
                }

                stage.tick(fiber_removed);
            }

            // Sweep CF_FIBER_CHANNEL_BY_COMMITMENT: delete all entries whose channel_id
            // no longer exists or was reset. Simpler than tracking exact keys.
            let iter = self.iterator_cf(self.cf_fiber_channel_by_commitment(), IteratorMode::Start);
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate fiber_channel_by_commitment in rollback cleanup: {}",
                        e
                    )
                })?;
                // value is the channel_id; check if that channel was opened,
                // force-closed, or settled after rollback_to (i.e. it will be
                // deleted or reset to Open by the channel sweep above).
                // NOTE: this reads from the DB, which still holds the PRE-reset
                // channel state, so settlement_block is still set even though
                // the batch already reset the channel to Open.
                if let Ok(Some(ch)) = self.get_fiber_channel(&value) {
                    if ch.open_block > rollback_to
                        || ch.close_block.is_some_and(|b| b > rollback_to)
                        || ch.settlement_block.is_some_and(|b| b > rollback_to)
                    {
                        batch.delete_cf(self.cf_fiber_channel_by_commitment(), &key);
                    }
                } else {
                    batch.delete_cf(self.cf_fiber_channel_by_commitment(), &key);
                }
            }

            stage.finish(fiber_removed);
            if fiber_removed > 0 {
                info!(fiber_removed, "rollback: cleaned up Fiber channel data");
            }
        }

        // 9. Apply derived-CF deltas (addr_balance, script_info, token_holders, token_info).
        let stage = RollbackStageProgress::new("apply_derived_cf_deltas");
        let mut addr_balances_updated = 0u64;
        let mut script_infos_updated = 0u64;
        let mut holders_updated = 0u64;
        let mut holders_removed = 0u64;
        let mut tokens_updated = 0u64;

        // 9a. addr_balance
        // Collect all lock_hashes that need updating (from cell deltas OR addr_txs removals).
        let mut all_addr_keys: HashSet<Vec<u8>> = addr_balance_deltas.keys().cloned().collect();
        all_addr_keys.extend(addr_txs_count_deltas.keys().cloned());

        // Track holder count transitions for HODL tracker repair in stage 11.
        let mut holder_count_delta: i64 = 0;

        for lock_hash in &all_addr_keys {
            let (balance_delta, used_delta, live_delta, total_cells_delta) = addr_balance_deltas
                .get(lock_hash)
                .copied()
                .unwrap_or((0, 0, 0, 0));
            let txs_removed = addr_txs_count_deltas.get(lock_hash).copied().unwrap_or(0);

            if balance_delta == 0
                && used_delta == 0
                && live_delta == 0
                && total_cells_delta == 0
                && txs_removed == 0
            {
                continue;
            }

            let Some(mut ab) = self.get_addr_balance(lock_hash)? else {
                // Address has addr_txs entries but no addr_balance — can happen when
                // a cellbase-only address was never materialized. Skip gracefully.
                continue;
            };
            // Track holder transitions: capture pre-rollback live count, then
            // compare with post-rollback to detect 0↔>0 transitions.
            let old_live = ab.live_cells_count;
            ab.balance += balance_delta;
            ab.used_capacity += used_delta;
            ab.live_cells_count += live_delta;
            let new_live = ab.live_cells_count;
            if old_live > 0 && new_live == 0 {
                holder_count_delta -= 1;
            } else if old_live == 0 && new_live > 0 {
                holder_count_delta += 1;
            }
            let next_total_cells_count = ab
                .total_cells_count
                .checked_add(total_cells_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "addr_balance total_cells_count overflow during rollback: lock_hash=0x{}, total_cells_count={}, total_cells_delta={}",
                        bytes_to_hex(lock_hash),
                        ab.total_cells_count,
                        total_cells_delta
                    )
                })?;
            let next_txs_count = ab.txs_count.checked_sub(txs_removed).ok_or_else(|| {
                anyhow::anyhow!(
                    "addr_balance txs_count overflow during rollback: lock_hash=0x{}, txs_count={}, txs_removed={}",
                    bytes_to_hex(lock_hash),
                    ab.txs_count,
                    txs_removed
                )
            })?;
            if next_total_cells_count < 0 || next_txs_count < 0 {
                anyhow::bail!(
                    "addr_balance total_cells_count underflow or txs_count underflow during rollback: lock_hash=0x{}, total_cells_count={}, total_cells_delta={}, next_total_cells_count={}, txs_count={}, txs_removed={}, next_txs_count={}",
                    bytes_to_hex(lock_hash),
                    ab.total_cells_count,
                    total_cells_delta,
                    next_total_cells_count,
                    ab.txs_count,
                    txs_removed,
                    next_txs_count
                );
            }
            ab.total_cells_count = next_total_cells_count;
            ab.txs_count = next_txs_count;
            if ab.balance < 0 || ab.used_capacity < 0 || ab.live_cells_count < 0 {
                anyhow::bail!(
                    "addr_balance underflow during rollback: lock_hash=0x{}, balance={}, used={}, live_cells={}",
                    bytes_to_hex(lock_hash),
                    ab.balance,
                    ab.used_capacity,
                    ab.live_cells_count
                );
            }
            // Repair last_activity from the latest surviving addr_tx entry
            if ab.last_activity_block > rollback_to {
                if let Some((surv_block, surv_tx)) = addr_latest_surviving.get(lock_hash) {
                    ab.last_activity_block = *surv_block;
                    ab.last_activity_tx = surv_tx.clone();
                } else {
                    // No surviving addr_txs — reset to first_seen
                    ab.last_activity_block = ab.first_seen_block;
                    ab.last_activity_tx = ab.first_seen_tx.clone();
                }
            }
            batch.put_cf(
                self.cf_addr_balance(),
                lock_hash,
                bincode::serialize(&ab).expect("serialize AddressBalance"),
            );
            addr_balances_updated += 1;
        }

        // 9b. script_info
        for ((code_hash, is_type), (live_delta, live_cap_delta, live_occ_delta)) in
            &script_info_deltas
        {
            if *live_delta == 0 && *live_cap_delta == 0 && *live_occ_delta == 0 {
                continue;
            }
            let mut si = self.get_script_info(code_hash)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing script_info during rollback delta application: code_hash=0x{}, is_type={}",
                    bytes_to_hex(code_hash),
                    is_type
                )
            })?;
            if *is_type {
                si.type_live_cells_count += live_delta;
                si.type_owned_capacity_sum += live_cap_delta;
                si.type_owned_knowledge_sum += live_occ_delta;
                if si.type_live_cells_count < 0
                    || si.type_owned_capacity_sum < 0
                    || si.type_owned_knowledge_sum < 0
                {
                    anyhow::bail!(
                        "script_info type underflow during rollback: code_hash=0x{}, live={}, cap={}, occ={}",
                        bytes_to_hex(code_hash),
                        si.type_live_cells_count,
                        si.type_owned_capacity_sum,
                        si.type_owned_knowledge_sum
                    );
                }
            } else {
                si.lock_live_cells_count += live_delta;
                si.lock_owned_capacity_sum += live_cap_delta;
                si.lock_owned_knowledge_sum += live_occ_delta;
                if si.lock_live_cells_count < 0
                    || si.lock_owned_capacity_sum < 0
                    || si.lock_owned_knowledge_sum < 0
                {
                    anyhow::bail!(
                        "script_info lock underflow during rollback: code_hash=0x{}, live={}, cap={}, occ={}",
                        bytes_to_hex(code_hash),
                        si.lock_live_cells_count,
                        si.lock_owned_capacity_sum,
                        si.lock_owned_knowledge_sum
                    );
                }
            }
            batch.put_cf(
                self.cf_script_info(),
                code_hash,
                bincode::serialize(&si).expect("serialize ScriptInfo"),
            );
            script_infos_updated += 1;
        }

        // 9b2. script_reference_info — apply reference deltas so version/family rollups
        //      re-derive correctly after rollback.
        let mut script_refs_updated = 0u64;
        for ((code_hash, hash_type, is_type), (cells_d, live_d, cap_d, own_d, used_d, know_d)) in
            &script_reference_deltas
        {
            if *cells_d == 0
                && *live_d == 0
                && *cap_d == 0
                && *own_d == 0
                && *used_d == 0
                && *know_d == 0
            {
                continue;
            }
            let key = keys::encode_script_reference_key(*hash_type, code_hash);
            let mut sri = match self.get_script_reference_info(*hash_type, code_hash)? {
                Some(v) => v,
                None => {
                    // Reference info may not exist yet (e.g. script only appeared in
                    // rolled-back blocks). Skip — nothing to adjust.
                    continue;
                }
            };
            if *is_type {
                sri.type_cells_count += cells_d;
                sri.type_live_cells_count += live_d;
                sri.type_capacity_sum += cap_d;
                sri.type_owned_capacity_sum += own_d;
                sri.type_used_capacity_sum += used_d;
                sri.type_owned_knowledge_sum += know_d;
                if sri.type_live_cells_count < 0
                    || sri.type_owned_capacity_sum < 0
                    || sri.type_owned_knowledge_sum < 0
                {
                    anyhow::bail!(
                        "script_reference_info type underflow during rollback: code_hash=0x{}, hash_type={}, live={}, own_cap={}, own_know={}",
                        bytes_to_hex(code_hash),
                        hash_type,
                        sri.type_live_cells_count,
                        sri.type_owned_capacity_sum,
                        sri.type_owned_knowledge_sum
                    );
                }
            } else {
                sri.lock_cells_count += cells_d;
                sri.lock_live_cells_count += live_d;
                sri.lock_capacity_sum += cap_d;
                sri.lock_owned_capacity_sum += own_d;
                sri.lock_used_capacity_sum += used_d;
                sri.lock_owned_knowledge_sum += know_d;
                if sri.lock_live_cells_count < 0
                    || sri.lock_owned_capacity_sum < 0
                    || sri.lock_owned_knowledge_sum < 0
                {
                    anyhow::bail!(
                        "script_reference_info lock underflow during rollback: code_hash=0x{}, hash_type={}, live={}, own_cap={}, own_know={}",
                        bytes_to_hex(code_hash),
                        hash_type,
                        sri.lock_live_cells_count,
                        sri.lock_owned_capacity_sum,
                        sri.lock_owned_knowledge_sum
                    );
                }
            }
            batch.put_cf(
                self.cf_script_reference_info(),
                key,
                bincode::serialize(&sri).expect("serialize ScriptReferenceInfo"),
            );
            script_refs_updated += 1;
        }

        // 9c. token_holders — apply balance deltas, track per-type_hash holder count changes
        let mut type_hash_holder_changes: HashMap<Vec<u8>, (i128, i64)> = HashMap::new();
        for ((type_hash, lock_hash), balance_delta) in &token_holder_deltas {
            if *balance_delta == 0 {
                continue;
            }
            let current = self
                .get_token_holder_balance(type_hash, lock_hash)?
                .unwrap_or(0);
            let new_balance = current + balance_delta;
            let entry = type_hash_holder_changes
                .entry(type_hash.clone())
                .or_insert((0, 0));
            entry.0 += balance_delta; // total_supply delta

            if new_balance < 0 {
                anyhow::bail!(
                    "token_holder underflow during rollback: type=0x{}, lock=0x{}, current={}, delta={}",
                    bytes_to_hex(type_hash),
                    bytes_to_hex(lock_hash),
                    current,
                    balance_delta
                );
            }

            if current > 0 {
                batch.delete_cf(
                    self.cf_token_holders_by_balance(),
                    keys::encode_token_holder_balance_key(type_hash, current, lock_hash),
                );
                batch.delete_cf(
                    self.cf_addr_tokens_by_balance(),
                    keys::encode_addr_token_balance_key(lock_hash, current, type_hash),
                );
            }

            if new_balance == 0 {
                let key = keys::encode_token_holder_key(type_hash, lock_hash);
                batch.delete_cf(self.cf_token_holders(), key);
                if current > 0 {
                    entry.1 -= 1; // lost a holder
                }
                holders_removed += 1;
            } else {
                let key = keys::encode_token_holder_key(type_hash, lock_hash);
                batch.put_cf(self.cf_token_holders(), key, new_balance.to_le_bytes());
                batch.put_cf(
                    self.cf_token_holders_by_balance(),
                    keys::encode_token_holder_balance_key(type_hash, new_balance, lock_hash),
                    [],
                );
                batch.put_cf(
                    self.cf_addr_tokens_by_balance(),
                    keys::encode_addr_token_balance_key(lock_hash, new_balance, type_hash),
                    [],
                );
                if current == 0 {
                    entry.1 += 1; // gained a holder
                }
                holders_updated += 1;
            }
        }

        // 9d. token_info — merge holder changes and transfer count deltas
        let mut all_type_hashes: HashSet<Vec<u8>> =
            type_hash_holder_changes.keys().cloned().collect();
        all_type_hashes.extend(transfer_count_deltas.keys().cloned());
        for type_hash in &all_type_hashes {
            let (supply_delta, holders_delta) = type_hash_holder_changes
                .get(type_hash)
                .copied()
                .unwrap_or((0, 0));
            let transfers_removed = transfer_count_deltas.get(type_hash).copied().unwrap_or(0);
            if supply_delta == 0 && holders_delta == 0 && transfers_removed == 0 {
                continue;
            }
            if let Some(mut ti) = self.get_token(type_hash)? {
                ti.holders_count += holders_delta;
                if let Some(ref mut ts) = ti.total_supply {
                    *ts += supply_delta;
                }
                ti.transfers_count -= transfers_removed;
                batch.put_cf(
                    self.cf_tokens(),
                    type_hash.as_slice(),
                    bincode::serialize(&ti).expect("serialize TokenInfo"),
                );
                // Also update CF_STATS_TOKEN total transfers count
                if transfers_removed != 0 {
                    let current_count = self.get_token_transfers_count(type_hash)?;
                    let new_count = current_count - transfers_removed;
                    let stats_key = keys::encode_token_transfers_key(type_hash);
                    batch.put_cf(self.cf_stats_token(), &stats_key, new_count.to_le_bytes());
                }
                tokens_updated += 1;
            }
        }

        info!(
            addr_balances_updated,
            script_infos_updated,
            script_refs_updated,
            holders_updated,
            holders_removed,
            tokens_updated,
            "Rollback derived CF deltas applied"
        );
        stage.finish(
            addr_balances_updated
                + script_infos_updated
                + script_refs_updated
                + holders_updated
                + holders_removed
                + tokens_updated,
        );

        // 10. Repair Spore/Object domain state for orphaned blocks and rebuild secondary indexes.
        let mut stage = RollbackStageProgress::new("repair_spore_object_domain");
        let mut spore_deleted = 0u64;
        let mut object_deleted = 0u64;
        let mut secondary_keys_deleted = 0u64;
        let mut secondary_keys_written = 0u64;
        let mut aggregate_rows_written = 0u64;
        let mut cluster_owner_rows_written = 0u64;

        // Clear 5 secondary index/aggregate CFs via delete_range (O(1) per CF).
        let secondary_cfs = [
            self.cf_spore_by_cluster(),
            self.cf_cluster_agg(),
            self.cf_mnft_by_collection(),
            self.cf_mnft_collection_agg(),
            self.cf_identity_agg(),
        ];
        for cf in secondary_cfs {
            batch.delete_range_cf::<&[u8]>(cf, &[], &[0xFF; 128]);
        }

        // Clear cluster-owner counters (single-byte prefix in stats_spore CF).
        {
            let start = [keys::STATS_PREFIX_CLUSTER_OWNER];
            let end = [keys::STATS_PREFIX_CLUSTER_OWNER + 1];
            batch.delete_range_cf(self.cf_stats_spore(), start, end);
        }

        // Clear identity-owner counters and identity-by-collection index.
        batch.delete_range_cf::<&[u8]>(self.cf_stats_identity(), &[], &[0xFF; 128]);
        batch.delete_range_cf::<&[u8]>(self.cf_identity_by_collection(), &[], &[0xFF; 128]);

        // Clear object collection-owner counters (single-byte prefix in stats_mnft CF).
        {
            let start = [keys::STATS_PREFIX_OBJECT_COLLECTION_OWNER];
            let end = [keys::STATS_PREFIX_OBJECT_COLLECTION_OWNER + 1];
            batch.delete_range_cf(self.cf_stats_mnft(), start, end);
        }

        info!("rollback: cleared secondary index/aggregate CFs via delete_range");

        let mut cluster_aggs: HashMap<Vec<u8>, ClusterAggregate> = HashMap::new();
        let mut cluster_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64> = HashMap::new();
        let mut object_collection_aggs: HashMap<Vec<u8>, MnftCollectionAggregate> = HashMap::new();
        let mut object_collection_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64> = HashMap::new();

        let iter = self.iterator_cf(self.cf_spore_data(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_data while repairing rollback state: {}",
                    e
                )
            })?;
            let spore_id = key.to_vec();
            let entry: ObjectEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize spore_data entry during rollback repair: spore_id=0x{}, error={}",
                    bytes_to_hex(&spore_id),
                    e
                )
            })?;

            if entry.created_at_block > rollback_to {
                batch.delete_cf(self.cf_spore_data(), &key);
                // Clean up outpoint entries for this deleted spore using the
                // reverse index (SPORE_OUTPOINT_BY_ID → outpoints).
                let by_id_prefix = keys::encode_spore_outpoint_by_id_prefix(&spore_id);
                let by_id_iter = self.prefix_iterator_cf(self.cf_stats_spore(), &by_id_prefix);
                for by_id_item in by_id_iter {
                    let (by_id_key, _) = by_id_item.map_err(|e| {
                        anyhow::anyhow!(
                            "failed to iterate spore_outpoint_by_id during rollback cleanup: spore_id=0x{}, error={}",
                            bytes_to_hex(&spore_id),
                            e
                        )
                    })?;
                    if !by_id_key.starts_with(&by_id_prefix) {
                        break;
                    }
                    let (tx_hash, output_index) = keys::decode_spore_outpoint_by_id_key(&by_id_key);
                    let fwd_key = keys::encode_spore_outpoint_key(&tx_hash, output_index);
                    batch.delete_cf(self.cf_stats_spore(), fwd_key);
                    batch.delete_cf(self.cf_stats_spore(), &by_id_key);
                    secondary_keys_deleted += 1;
                }
                spore_deleted += 1;
                stage.tick(
                    spore_deleted
                        + object_deleted
                        + secondary_keys_deleted
                        + secondary_keys_written
                        + aggregate_rows_written
                        + cluster_owner_rows_written,
                );
                continue;
            }

            match entry.standard {
                ObjectStandard::SporeCluster => {
                    let agg = cluster_aggs.entry(spore_id).or_default();
                    agg.name = entry.name.clone();
                    agg.description = entry.description.clone();
                }
                ObjectStandard::Spore => {
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
                                ObjectExtra::Spore { media_profile, .. } => media_profile.tier,
                                _ => CompositionTier::Unknown,
                            };
                            let tier_slot = match tier {
                                CompositionTier::PureCkb => &mut agg.pure_ckb_count,
                                CompositionTier::BtcCkb => &mut agg.btc_ckb_count,
                                CompositionTier::DecentralizedMixture => {
                                    &mut agg.decentralized_mixture_count
                                }
                                CompositionTier::CentralizedMixture => {
                                    &mut agg.centralized_mixture_count
                                }
                                CompositionTier::Unknown => &mut agg.unknown_count,
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
                _ => {
                    // MnftIssuer, MnftClass, MnftToken are not stored in spore_data
                }
            }
            stage.tick(
                spore_deleted
                    + object_deleted
                    + secondary_keys_deleted
                    + secondary_keys_written
                    + aggregate_rows_written
                    + cluster_owner_rows_written,
            );
        }

        let iter = self.iterator_cf(self.cf_mnft_data(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate mnft_data while repairing rollback state: {}",
                    e
                )
            })?;
            let object_id = key.to_vec();
            let entry: ObjectEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize mnft_data entry during rollback repair: object_id=0x{}, error={}",
                    bytes_to_hex(&object_id),
                    e
                )
            })?;

            if entry.created_at_block > rollback_to {
                batch.delete_cf(self.cf_mnft_data(), &key);
                object_deleted += 1;
                stage.tick(
                    spore_deleted
                        + object_deleted
                        + secondary_keys_deleted
                        + secondary_keys_written
                        + aggregate_rows_written
                        + cluster_owner_rows_written,
                );
                continue;
            }

            match entry.standard {
                ObjectStandard::MnftClass => {
                    let agg = object_collection_aggs.entry(object_id).or_insert_with(|| {
                        MnftCollectionAggregate {
                            standard: ObjectStandard::MnftClass,
                            ..Default::default()
                        }
                    });
                    agg.standard = ObjectStandard::MnftClass;
                    if entry.name.is_some() {
                        agg.name = entry.name.clone();
                    }
                }
                ObjectStandard::MnftToken => {
                    if let Some(collection_id) = entry.collection_id.as_ref() {
                        let idx_key =
                            keys::encode_object_by_collection_key(collection_id, &object_id);
                        batch.put_cf(self.cf_mnft_by_collection(), idx_key, []);
                        secondary_keys_written += 1;
                        // Resolve storage tier from class entry before taking mutable borrow on agg
                        let token_tier = self
                            .get_mnft(collection_id)
                            .ok()
                            .flatten()
                            .map(|class_entry| match &class_entry.extra {
                                ObjectExtra::MnftClass {
                                    composition_tier, ..
                                } => *composition_tier,
                                _ => CompositionTier::Unknown,
                            })
                            .unwrap_or(CompositionTier::Unknown);
                        let agg = object_collection_aggs
                            .entry(collection_id.clone())
                            .or_insert_with(|| MnftCollectionAggregate {
                                standard: ObjectStandard::MnftClass,
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
                            let tier_slot = match token_tier {
                                CompositionTier::PureCkb => &mut agg.pure_ckb_count,
                                CompositionTier::BtcCkb => &mut agg.btc_ckb_count,
                                CompositionTier::DecentralizedMixture => {
                                    &mut agg.decentralized_mixture_count
                                }
                                CompositionTier::CentralizedMixture => {
                                    &mut agg.centralized_mixture_count
                                }
                                CompositionTier::Unknown => &mut agg.unknown_count,
                            };
                            *tier_slot = tier_slot.checked_add(1).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "mNFT tier count overflow while repairing rollback state: collection_id=0x{}",
                                    bytes_to_hex(collection_id)
                                )
                            })?;
                            let owner_lock_hash = entry.owner_lock_hash.as_ref().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "mNFT live entry missing owner_lock_hash while repairing rollback state: collection_id=0x{}, object_id=0x{}",
                                    bytes_to_hex(collection_id),
                                    bytes_to_hex(&object_id)
                                )
                            })?;
                            let owner_key = (collection_id.clone(), owner_lock_hash.clone());
                            let owner_count =
                                object_collection_owner_counts.entry(owner_key).or_insert(0);
                            *owner_count = owner_count.checked_add(1).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "mNFT owner count overflow while repairing rollback state: collection_id=0x{}",
                                    bytes_to_hex(collection_id)
                                )
                            })?;
                        }
                    }
                }
                ObjectStandard::MnftIssuer
                | ObjectStandard::Spore
                | ObjectStandard::SporeCluster => {
                    // Spore/SporeCluster are in spore_data CF, not object_data CF.
                    // MnftIssuer has no collection-level aggregation.
                }
            }
            stage.tick(
                spore_deleted
                    + object_deleted
                    + secondary_keys_deleted
                    + secondary_keys_written
                    + aggregate_rows_written
                    + cluster_owner_rows_written,
            );
        }

        // Repair identity data: scan CF_IDENTITY_DATA to rebuild identity aggregates,
        // identity_by_collection index, and identity owner counts.
        let mut identity_aggs: HashMap<Vec<u8>, IdentityCollectionAggregate> = HashMap::new();
        let mut identity_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64> = HashMap::new();

        let iter = self.iterator_cf(self.cf_identity_data(), IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate identity_data while repairing rollback state: {}",
                    e
                )
            })?;
            let identity_id = key.to_vec();
            let entry: IdentityEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize identity_data entry during rollback repair: identity_id=0x{}, error={}",
                    bytes_to_hex(&identity_id),
                    e
                )
            })?;

            if entry.created_at_block > rollback_to {
                batch.delete_cf(self.cf_identity_data(), &key);
                // Clean up dotbit outpoint entries for deleted identities.
                if entry.standard == IdentityStandard::DotBit && identity_id.len() >= 20 {
                    let by_id_prefix =
                        keys::encode_dotbit_outpoint_by_account_id_prefix(&identity_id);
                    let by_id_iter = self.prefix_iterator_cf(self.cf_stats_mnft(), &by_id_prefix);
                    for by_id_item in by_id_iter {
                        let (by_id_key, _) = by_id_item.map_err(|e| {
                            anyhow::anyhow!(
                                "failed to iterate dotbit_outpoint_by_account_id during rollback cleanup: identity_id=0x{}, error={}",
                                bytes_to_hex(&identity_id),
                                e
                            )
                        })?;
                        if !by_id_key.starts_with(&by_id_prefix) {
                            break;
                        }
                        let (tx_hash, output_index) =
                            keys::decode_dotbit_outpoint_by_account_id_key(&by_id_key);
                        let fwd_key =
                            keys::encode_dotbit_account_outpoint_key(&tx_hash, output_index);
                        batch.delete_cf(self.cf_stats_mnft(), fwd_key);
                        batch.delete_cf(self.cf_stats_mnft(), &by_id_key);
                    }
                }
                secondary_keys_deleted += 1;
                stage.tick(
                    spore_deleted
                        + object_deleted
                        + secondary_keys_deleted
                        + secondary_keys_written
                        + aggregate_rows_written
                        + cluster_owner_rows_written,
                );
                continue;
            }

            let collection_id = match entry.standard {
                IdentityStandard::DotBit => DOTBIT_SENTINEL_COLLECTION.to_vec(),
                IdentityStandard::DidCkb => DID_CKB_SENTINEL_COLLECTION.to_vec(),
            };

            // Rebuild identity_by_collection index
            let idx_key = keys::encode_identity_by_collection_key(&collection_id, &identity_id);
            batch.put_cf(self.cf_identity_by_collection(), idx_key, []);
            secondary_keys_written += 1;

            let agg = identity_aggs
                .entry(collection_id.clone())
                .or_insert_with(|| IdentityCollectionAggregate {
                    standard: entry.standard,
                    name: match entry.standard {
                        IdentityStandard::DotBit => Some(".bit".to_string()),
                        IdentityStandard::DidCkb => Some("did:ckb".to_string()),
                    },
                    ..Default::default()
                });
            agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "identity total_count overflow while repairing rollback state: collection_id=0x{}",
                    bytes_to_hex(&collection_id)
                )
            })?;
            if entry.is_live {
                agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "identity live_count overflow while repairing rollback state: collection_id=0x{}",
                        bytes_to_hex(&collection_id)
                    )
                })?;
                if let Some(owner_lock_hash) = entry.owner_lock_hash.as_ref() {
                    let owner_key = (collection_id, owner_lock_hash.clone());
                    let owner_count = identity_owner_counts.entry(owner_key).or_insert(0);
                    *owner_count = owner_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "identity owner count overflow while repairing rollback state"
                        )
                    })?;
                }
            }
            stage.tick(
                spore_deleted
                    + object_deleted
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
        // Sum surviving daily deltas to restore cumulative capacity on each cluster aggregate.
        {
            let prefix = [keys::STATS_PREFIX_CLUSTER_DAILY];
            let iter = self.prefix_iterator_cf(self.cf_stats_spore(), &prefix);
            for item in iter {
                let (key, value) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate cluster daily deltas during rollback capacity repair: {}",
                        e
                    )
                })?;
                if !key.starts_with(&prefix) {
                    break;
                }
                if key.len() != keys::CLUSTER_DAILY_KEY_SIZE {
                    continue;
                }
                let (cluster_id_bytes, _date) = keys::decode_cluster_daily_key(&key);
                let delta: ClusterDailyDelta = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize cluster daily delta during rollback capacity repair: cluster_id=0x{}, error={}",
                        bytes_to_hex(&cluster_id_bytes),
                        e
                    )
                })?;
                if let Some(agg) = cluster_aggs.get_mut(&cluster_id_bytes) {
                    agg.owned_capacity += delta.owned_capacity_delta;
                    agg.owned_knowledge += delta.owned_knowledge_delta;
                }
            }
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

        let mut object_owner_totals: HashMap<Vec<u8>, i64> = HashMap::new();
        for ((collection_id, lock_hash), count) in &object_collection_owner_counts {
            let owner_key = keys::encode_object_collection_owner_key(collection_id, lock_hash);
            batch.put_cf(
                self.cf_stats_mnft(),
                owner_key,
                count.to_le_bytes().as_slice(),
            );
            secondary_keys_written += 1;
            let holder_total = object_owner_totals
                .entry(collection_id.clone())
                .or_insert(0);
            *holder_total = holder_total.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "object collection holders_count overflow while repairing rollback state: collection_id=0x{}",
                    bytes_to_hex(collection_id)
                )
            })?;
        }

        // Collection activity counts (object_activity_totals, identity_activity_totals)
        // were already computed during stages 8d/8e to avoid a duplicate full-CF scan
        // and the data inconsistency from reading uncommitted batch deletes.

        for (collection_id, agg) in &mut object_collection_aggs {
            agg.holders_count = object_owner_totals.get(collection_id).copied().unwrap_or(0);
            agg.activities_count = object_activity_totals
                .get(collection_id)
                .copied()
                .unwrap_or(0);
            let encoded = bincode::serialize(agg).map_err(|e| {
                anyhow::anyhow!(
                    "failed to serialize object collection aggregate during rollback repair: collection_id=0x{}, error={}",
                    bytes_to_hex(collection_id),
                    e
                )
            })?;
            batch.put_cf(self.cf_mnft_collection_agg(), collection_id, &encoded);
            aggregate_rows_written += 1;
        }

        // Write rebuilt identity owner counts to CF_STATS_IDENTITY.
        let mut identity_holder_totals: HashMap<Vec<u8>, i64> = HashMap::new();
        for ((collection_id, lock_hash), count) in &identity_owner_counts {
            let owner_key = keys::encode_identity_owner_key(collection_id, lock_hash);
            batch.put_cf(
                self.cf_stats_identity(),
                owner_key,
                count.to_le_bytes().as_slice(),
            );
            secondary_keys_written += 1;
            let holder_total = identity_holder_totals
                .entry(collection_id.clone())
                .or_insert(0);
            *holder_total = holder_total.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "identity holders_count overflow while repairing rollback state: collection_id=0x{}",
                    bytes_to_hex(collection_id)
                )
            })?;
        }

        // Merge identity aggregates with activity counts and holder totals, then write.
        for (collection_id, agg) in &mut identity_aggs {
            agg.holders_count = identity_holder_totals
                .get(collection_id)
                .copied()
                .unwrap_or(0);
            agg.activities_count = identity_activity_totals
                .get(collection_id)
                .copied()
                .unwrap_or(0);
            let encoded = bincode::serialize(agg).map_err(|e| {
                anyhow::anyhow!(
                    "failed to serialize identity collection aggregate during rollback repair: collection_id=0x{}, error={}",
                    bytes_to_hex(collection_id),
                    e
                )
            })?;
            batch.put_cf(self.cf_identity_agg(), collection_id, &encoded);
            aggregate_rows_written += 1;
        }
        // For identity collections that have activities but no identity_data entries,
        // repair their activity count only.
        for (collection_id, total) in &identity_activity_totals {
            if identity_aggs.contains_key(collection_id) {
                continue; // Already handled above.
            }
            let mut agg = self
                .get_identity_collection_aggregate(collection_id)?
                .unwrap_or_default();
            agg.activities_count = *total;
            let encoded = bincode::serialize(&agg).map_err(|e| {
                anyhow::anyhow!(
                    "failed to serialize identity collection aggregate during rollback repair: collection_id=0x{}, error={}",
                    bytes_to_hex(collection_id),
                    e
                )
            })?;
            batch.put_cf(self.cf_identity_agg(), collection_id, &encoded);
            aggregate_rows_written += 1;
        }

        stage.finish(
            spore_deleted
                + object_deleted
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
            if truncate_hodl_tracker_state_for_rollback(
                &mut state,
                rollback_to,
                holder_count_delta,
            )? {
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

        // 12. Keep cell distribution tracker state aligned with rollback tip.
        let mut stage = RollbackStageProgress::new("repair_cell_dist_tracker_state");
        let mut cell_dist_tracker_repaired = 0u64;
        if rollback_to < 0 {
            if self
                .get_cf(self.cf_sync_meta(), keys::sync_meta_keys::CELL_DIST_TRACKER)?
                .is_some()
            {
                batch.delete_cf(self.cf_sync_meta(), keys::sync_meta_keys::CELL_DIST_TRACKER);
                cell_dist_tracker_repaired += 1;
            }
        } else if let Some(mut state) = self.get_cell_dist_tracker_state()? {
            if truncate_cell_dist_tracker_state_for_rollback(
                &mut state,
                rollback_to,
                &cell_dist_count_deltas,
                &cell_dist_capacity_deltas,
            )? {
                let encoded = bincode::serialize(&state).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to serialize repaired cell_dist tracker state during rollback cleanup: {}",
                        e
                    )
                })?;
                batch.put_cf(
                    self.cf_sync_meta(),
                    keys::sync_meta_keys::CELL_DIST_TRACKER,
                    &encoded,
                );
                cell_dist_tracker_repaired += 1;
            }
        }
        stage.tick(cell_dist_tracker_repaired);
        stage.finish(cell_dist_tracker_repaired);

        // Compute the updated sync_status BEFORE the batch commit so we can
        // include it in the same atomic WriteBatch.  This eliminates the crash
        // window where deletes are committed but totals are still stale.
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
        {
            let mut status = sync_status_before;
            status.tip_block_number = tip_number;
            status.tip_block_hash = tip_hash;
            status.total_transactions = checked_rollback_total(
                "total_transactions",
                status.total_transactions,
                rollback_total_transactions,
                tip_number,
            )?;
            status.total_cells_created = checked_rollback_total(
                "total_cells_created",
                status.total_cells_created,
                rollback_total_cells_created,
                tip_number,
            )?;
            status.total_cells_consumed = checked_rollback_total(
                "total_cells_consumed",
                status.total_cells_consumed,
                rollback_total_cells_consumed,
                tip_number,
            )?;
            status.last_synced_at = chrono::Utc::now().timestamp();
            let status_bytes = bincode::serialize(&status).map_err(|e| {
                anyhow::anyhow!("failed to serialize sync_status during rollback: {}", e)
            })?;
            batch.put_cf(
                self.cf_sync_meta(),
                keys::sync_meta_keys::SYNC_STATUS,
                &status_bytes,
            );
        }
        // Clear the rollback-in-progress marker in the same atomic batch.
        batch.delete_cf(
            self.cf_sync_meta(),
            keys::sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS,
        );

        // Commit all deletes, sync_status update, and cleanup marker atomically.
        self.write_batch(batch)?;

        info!(
            elapsed_secs = format!("{:.1}", rollback_started_at.elapsed().as_secs_f64()),
            blocks_removed,
            txs_removed,
            cells_removed,
            cells_restored,
            "Rollback cleanup write batch committed"
        );

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
        AddressBalance, AssetAction, CachedBlockHeader, CellDistributionTrackerState,
        CompositionTier, DaoDepositCacheEntry, HodlTrackerState, LiveCellInfo,
        MnftCollectionAggregate, ObjectCollectionActivityEntry, ObjectEntry, ObjectExtra,
        ObjectStandard, ParticipantDelta, ScriptInfo, SporeMediaProfile, SyncStatus, TokenInfo,
        TxActions, TxIndexEntry, UndoInputOutPoint, UndoLogEntry, UndoTxContext,
    };

    fn put_canonical_tx(batch: &mut StoreBatch<'_>, block_num: i64, tx_idx: i32, tx_hash: &[u8]) {
        batch.put_tx_hash_map(tx_hash, block_num, tx_idx);
        batch.put_tx_index(
            block_num,
            tx_idx,
            &TxIndexEntry {
                is_cellbase: false,
                timestamp: 1_700_000_000 + block_num,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
    }

    fn make_tx_actions(tx_hash: &[u8], block_num: i64, tx_idx: i32) -> TxActions {
        TxActions {
            tx_hash: tx_hash.to_vec(),
            block_hash: vec![0x40 | (block_num as u8); 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                lock_hash: vec![0xAA; 32],
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        }
    }

    fn seed_sync_status(
        store: &CkbadgerStore,
        tip_block_number: i64,
        tip_block_hash: &[u8],
        total_transactions: i64,
        total_cells_created: i64,
        total_cells_consumed: i64,
    ) {
        store
            .set_sync_status(&SyncStatus {
                tip_block_number,
                tip_block_hash: tip_block_hash.to_vec(),
                total_transactions,
                total_cells_created,
                total_cells_consumed,
                ..Default::default()
            })
            .unwrap();
    }

    #[test]
    fn test_should_delete_stats_for_replay_daily_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let key = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260211");
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key_old = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20260209");
        assert!(!should_delete_stats_for_replay(&key_old, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_hourly_and_miner_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let hourly = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_HOURLY, b"2026021001");
        assert!(should_delete_stats_for_replay(&hourly, cutoff, cutoff_hh, 0, 0).unwrap());

        let miner_suffix = [b"20260210".as_slice(), &[0xAA; 32]].concat();
        let miner = crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_MINER, &miner_suffix);
        assert!(should_delete_stats_for_replay(&miner, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_script_daily_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let code_hash = [0xAA; 32];

        let new_key = crate::keys::encode_script_daily_key(&code_hash, false, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff, cutoff_hh, 0, 0).unwrap());

        let old_key = crate::keys::encode_script_daily_key(&code_hash, true, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_token_daily_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let type_hash = [0xBB; 32];

        let new_key = crate::keys::encode_token_daily_key(&type_hash, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff, cutoff_hh, 0, 0).unwrap());

        let old_key = crate::keys::encode_token_daily_key(&type_hash, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_cluster_daily_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let cluster_id = [0xCC; 32];

        let new_key = crate::keys::encode_cluster_daily_key(&cluster_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff, cutoff_hh, 0, 0).unwrap());

        let old_key = crate::keys::encode_cluster_daily_key(&cluster_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_spore_daily_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let spore_id = [0xDD; 32];

        let new_key = crate::keys::encode_spore_daily_key(&spore_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff, cutoff_hh, 0, 0).unwrap());

        let old_key = crate::keys::encode_spore_daily_key(&spore_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_object_daily_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let collection_id = [0xEE; 24];

        let new_key = crate::keys::encode_object_daily_key(&collection_id, 20260211);
        assert!(should_delete_stats_for_replay(&new_key, cutoff, cutoff_hh, 0, 0).unwrap());

        let old_key = crate::keys::encode_object_daily_key(&collection_id, 20260209);
        assert!(!should_delete_stats_for_replay(&old_key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_epoch_prefix() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let cutoff_epoch: i64 = 100;

        // Epoch at cutoff → delete
        let key =
            crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_EPOCH, &100_i64.to_be_bytes());
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, cutoff_epoch).unwrap());

        // Epoch after cutoff → delete
        let key =
            crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_EPOCH, &101_i64.to_be_bytes());
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, cutoff_epoch).unwrap());

        // Epoch before cutoff → keep
        let key =
            crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_EPOCH, &99_i64.to_be_bytes());
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, cutoff_epoch).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_outpoint_and_index_prefixes_preserved() {
        // Outpoint/index entries are NOT deleted by should_delete_stats_for_replay
        // to prevent data loss for entities created before rollback_to.
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let tx_hash = [0xAA; 32];
        let output_index: i16 = 0;
        let spore_id = [0xBB; 32];
        let account_id = [0xCC; 20];
        let type_script_hash = [0xDD; 32];

        let key = crate::keys::encode_spore_outpoint_key(&tx_hash, output_index);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_spore_outpoint_by_id_key(&spore_id, &tx_hash, output_index);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_spore_type_index_key(&type_script_hash);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_mnft_class_outpoint_key(&tx_hash, output_index);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_mnft_token_outpoint_key(&tx_hash, output_index);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_dotbit_account_outpoint_key(&tx_hash, output_index);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_dotbit_outpoint_by_account_id_key(
            &account_id,
            &tx_hash,
            output_index,
        );
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        let key = crate::keys::encode_object_type_index_key(&type_script_hash);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_per_asset_hourly_prefixes() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021000";
        let type_hash = [0xAA; 32];
        let cluster_id = [0xBB; 32];
        let collection_id = [0xCC; 24];
        // cutoff_hour = 492_960 (arbitrary, corresponds to ~2026-03-10)
        let cutoff_hour: i64 = 492_960;

        // TOKEN_HOURLY at cutoff_hour → deleted
        let key = crate::keys::encode_token_hourly_key(&type_hash, cutoff_hour);
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, cutoff_hour, 0).unwrap());

        // TOKEN_HOURLY before cutoff → preserved
        let key = crate::keys::encode_token_hourly_key(&type_hash, cutoff_hour - 1);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, cutoff_hour, 0).unwrap());

        // SPORE_HOURLY at cutoff_hour → deleted
        let key = crate::keys::encode_spore_hourly_key(&cluster_id, cutoff_hour);
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, cutoff_hour, 0).unwrap());

        // SPORE_HOURLY before cutoff → preserved
        let key = crate::keys::encode_spore_hourly_key(&cluster_id, cutoff_hour - 1);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, cutoff_hour, 0).unwrap());

        // OBJECT_HOURLY at cutoff_hour → deleted
        let key = crate::keys::encode_object_hourly_key(&collection_id, cutoff_hour);
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, cutoff_hour, 0).unwrap());

        // OBJECT_HOURLY before cutoff → preserved
        let key = crate::keys::encode_object_hourly_key(&collection_id, cutoff_hour - 1);
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, cutoff_hour, 0).unwrap());
    }

    #[test]
    fn test_should_delete_stats_for_replay_errors_on_invalid_cutoff_date() {
        let cutoff = b"invalid-cutoff";
        let cutoff_hh = b"invalid-cutoff";
        let code_hash = [0xAA; 32];
        let key = crate::keys::encode_script_daily_key(&code_hash, false, 20260211);
        let err = should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap_err();
        assert!(err.to_string().contains("invalid cutoff date"));
    }

    #[test]
    fn test_rollback_cutoff_date_uses_ckb_utc8_day_boundary_for_daily_stats() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // 2026-03-04 17:41:36 UTC == 2026-03-05 01:41:36 UTC+8
        // Replay cutoff must be 20260305 (CKB UTC+8 boundary), not 20260304 (UTC).
        let replay_start_header = CachedBlockHeader {
            hash: vec![0x42; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_772_646_096_926,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &replay_start_header);
        batch.commit().unwrap();

        store
            .put_daily_stats("20260304", &Default::default())
            .unwrap();
        store
            .put_daily_stats("20260305", &Default::default())
            .unwrap();

        store.rollback_to_block(-1).unwrap();

        assert!(store.get_daily_stats("20260304").unwrap().is_some());
        assert!(store.get_daily_stats("20260305").unwrap().is_none());
    }

    #[test]
    fn test_rollback_to_block_succeeds_on_domain_store_with_activity_cfs() {
        // Domain store now contains all activity CFs (CF_ADDR_TXS,
        // CF_OBJECT_COLLECTION_ACTIVITIES, etc.), so rollback_to_block should
        // succeed without needing a separate append-only store.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        // Rollback to -1 (pre-genesis) on empty store succeeds trivially
        let result = store.rollback_to_block(-1).unwrap();
        assert_eq!(result.blocks_removed, 0);
    }

    #[test]
    fn test_rollback_to_block_deletes_tx_actions_above_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock_hash = [0xAA; 32];
        let tx_hash_keep = vec![0x10; 32];
        let tx_hash_drop = vec![0x20; 32];

        let header1 = CachedBlockHeader {
            hash: vec![0x41; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_001_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x42; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_002_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let keep_actions = make_tx_actions(&tx_hash_keep, 1, 0);
        let drop_actions = make_tx_actions(&tx_hash_drop, 2, 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        put_canonical_tx(&mut batch, 1, 0, &tx_hash_keep);
        put_canonical_tx(&mut batch, 2, 0, &tx_hash_drop);
        batch.put_tx_actions(&keep_actions);
        batch.put_tx_actions(&drop_actions);
        batch.put_addr_tx(
            &lock_hash,
            1,
            0,
            &tx_hash_keep,
            &AddrTxValue::new(0, false, true),
        );
        batch.put_addr_tx(
            &lock_hash,
            2,
            0,
            &tx_hash_drop,
            &AddrTxValue::new(0, false, true),
        );
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 2, 2, 2);

        store.rollback_to_block(1).unwrap();

        assert!(store.get_tx_actions(1, 0, &tx_hash_keep).unwrap().is_some());
        assert!(store.get_tx_actions(2, 0, &tx_hash_drop).unwrap().is_none());
        let addr_rows = store.list_addr_txs_recent(&lock_hash, 10, None).unwrap();
        assert_eq!(addr_rows.len(), 1);
        assert_eq!(addr_rows[0].0, 1);
        assert_eq!(addr_rows[0].1, 0);
        assert_eq!(addr_rows[0].2, tx_hash_keep);
    }

    #[test]
    fn test_rollback_to_block_decrements_sync_status_totals_for_deleted_tx_index_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x11; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_001_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_002_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            cycles: None,
        };

        let block1_tx = TxIndexEntry {
            is_cellbase: false,
            timestamp: header1.timestamp,
            inputs_count: 1,
            outputs_count: 2,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        };
        let block2_tx_a = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 3,
            outputs_count: 5,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        };
        let block2_tx_b = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 2,
            outputs_count: 4,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_index(1, 0, &block1_tx);
        batch.put_tx_index(2, 0, &block2_tx_a);
        batch.put_tx_index(2, 1, &block2_tx_b);
        batch.commit().unwrap();

        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 2,
                tip_block_hash: header2.hash.clone(),
                total_transactions: 3,
                total_cells_created: 11,
                total_cells_consumed: 6,
                ..Default::default()
            })
            .unwrap();

        store.rollback_to_block(1).unwrap();

        let status = store.get_sync_status().unwrap();
        assert_eq!(status.tip_block_number, 1);
        assert_eq!(status.tip_block_hash, header1.hash);
        assert_eq!(status.total_transactions, 1);
        assert_eq!(status.total_cells_created, 2);
        assert_eq!(status.total_cells_consumed, 1);
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
    fn test_rollback_rebuilds_object_collection_aggregate_with_canonical_activity_count_only() {
        let dir = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(dir.path()).unwrap();
        let class_id = vec![0x44; 32];
        let object_id = vec![0x55; 32];

        let header0 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header1 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let mut domain_batch = StoreBatch::new(&domain);
        domain_batch.put_block_header(0, &header0);
        domain_batch.put_block_header(1, &header1);
        domain_batch.put_mnft(
            &object_id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(object_id.clone()),
                owner_lock_hash: Some(vec![0x66; 32]),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 0,
                created_at_tx: vec![],
                extra: ObjectExtra::MnftToken {
                    token_index: 1,
                    characteristic: vec![],
                    configure: 0,
                    state: 0,
                },
            },
        );
        domain_batch.put_mnft_collection_aggregate(
            &class_id,
            &MnftCollectionAggregate {
                name: Some("stale".to_string()),
                standard: ObjectStandard::MnftClass,
                total_count: 99,
                live_count: 99,
                holders_count: 99,
                activities_count: 99,
                ..Default::default()
            },
        );
        put_canonical_tx(&mut domain_batch, 0, 0, &[0xA1; 32]);
        // Write collection activities to domain store (activities are now in domain)
        domain_batch.put_object_collection_activity(
            &class_id,
            0,
            0,
            &ObjectCollectionActivityEntry {
                tx_hash: vec![0xA1; 32],
                block_hash: header0.hash.clone(),
                timestamp_ms: 1_700_000_000_000,
                actions: vec![AssetAction::Mint],
            },
        );
        domain_batch.put_object_collection_activity(
            &class_id,
            0,
            0,
            &ObjectCollectionActivityEntry {
                tx_hash: vec![0xB2; 32],
                block_hash: vec![0xC2; 32],
                timestamp_ms: 1_700_000_000_001,
                actions: vec![AssetAction::Transfer],
            },
        );
        domain_batch.commit().unwrap();

        domain.rollback_to_block(0).unwrap();

        let rebuilt = domain
            .get_mnft_collection_aggregate(&class_id)
            .unwrap()
            .unwrap();
        assert_eq!(rebuilt.total_count, 1);
        assert_eq!(rebuilt.live_count, 1);
        assert_eq!(rebuilt.activities_count, 2);
    }

    #[test]
    fn test_rollback_excludes_rolled_back_activities_from_collection_aggregate_count() {
        // Regression test: verifies that collection activity counts only include
        // entries at or below the rollback target, not entries from rolled-back
        // blocks that are being deleted in the same write batch.
        let dir = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(dir.path()).unwrap();
        let class_id = vec![0x44; 32];
        let object_id = vec![0x55; 32];

        let header0 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header1 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let mut batch = StoreBatch::new(&domain);
        batch.put_block_header(0, &header0);
        batch.put_block_header(1, &header1);
        batch.put_mnft(
            &object_id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(object_id.clone()),
                owner_lock_hash: Some(vec![0x66; 32]),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 0,
                created_at_tx: vec![],
                extra: ObjectExtra::MnftToken {
                    token_index: 1,
                    characteristic: vec![],
                    configure: 0,
                    state: 0,
                },
            },
        );
        put_canonical_tx(&mut batch, 0, 0, &[0xA1; 32]);
        put_canonical_tx(&mut batch, 1, 0, &[0xA2; 32]);
        // Activity at block 0 (should survive rollback)
        batch.put_object_collection_activity(
            &class_id,
            0,
            0,
            &ObjectCollectionActivityEntry {
                tx_hash: vec![0xA1; 32],
                block_hash: header0.hash.clone(),
                timestamp_ms: 1_700_000_000_000,
                actions: vec![AssetAction::Mint],
            },
        );
        // Activity at block 1 (should be deleted by rollback)
        batch.put_object_collection_activity(
            &class_id,
            1,
            0,
            &ObjectCollectionActivityEntry {
                tx_hash: vec![0xA2; 32],
                block_hash: header1.hash.clone(),
                timestamp_ms: 1_700_000_010_000,
                actions: vec![AssetAction::Transfer],
            },
        );
        batch.commit().unwrap();

        domain.rollback_to_block(0).unwrap();

        let rebuilt = domain
            .get_mnft_collection_aggregate(&class_id)
            .unwrap()
            .unwrap();
        // Only the block-0 activity should remain.
        assert_eq!(
            rebuilt.activities_count, 1,
            "activities_count should exclude rolled-back block-1 entries"
        );
    }

    #[test]
    fn test_rollback_restores_consumed_cells_after_fork_point() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock_hash = vec![0xAB; 32];
        let tx_hash = vec![0x42; 32];

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let cell = LiveCellInfo {
            capacity: 500,
            lock_script_hash: lock_hash.clone(),
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 500,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&tx_hash, 0, &cell, 1);
        // Seed derived CFs so inline delta application can find them.
        batch.put_addr_balance(&lock_hash, &AddressBalance::default());
        batch.put_script_info(
            &[0x11; 32],
            &ScriptInfo {
                code_hash: vec![0x11; 32],
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        // Simulate consumption in block 2.
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, 0, &cell, 1, 2);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        assert!(store.get_cell(&tx_hash, 0, &store).unwrap().is_none());
        assert!(store
            .get_consumed_cell(&tx_hash, 0, &store)
            .unwrap()
            .is_some());

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&tx_hash, 0, &store).unwrap().is_some());

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
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let input_tx = vec![0x31; 32];
        let consuming_tx = vec![0x32; 32];
        let input_cell = LiveCellInfo {
            capacity: 400,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 400,
            udt_amount: None,
            data_hash: None,
        };
        let rollback_output_cell = LiveCellInfo {
            capacity: 200,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
            data_hash: None,
        };

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_cell(&input_tx, 0, &input_cell, 1);
        batch.put_cell(&consuming_tx, 0, &rollback_output_cell, 2);
        batch.put_consumed_cell_with_consumer(&input_tx, 0, &input_cell, 1, 2, Some(&consuming_tx));
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
        // Seed derived CFs so inline delta application can find them.
        batch.put_addr_balance(&[0xAA; 32], &AddressBalance::default());
        batch.put_addr_balance(
            &[0xBB; 32],
            &AddressBalance {
                balance: 200,
                used_capacity: 200,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &[0x11; 32],
            &ScriptInfo {
                code_hash: vec![0x11; 32],
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 200,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        );
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 1);

        assert!(store.get_cell(&input_tx, 0, &store).unwrap().is_none());
        assert!(store.get_cell(&consuming_tx, 0, &store).unwrap().is_some());
        assert!(store
            .get_consumed_cell(&input_tx, 0, &store)
            .unwrap()
            .is_some());

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&input_tx, 0, &store).unwrap().is_some());
        assert!(store.get_cell(&consuming_tx, 0, &store).unwrap().is_none());
        assert!(store
            .get_consumed_cell(&input_tx, 0, &store)
            .unwrap()
            .is_none());
    }

    /// Regression test: rollback_via_undo_log deletes undo entries, so the
    /// subsequent rollback_to_block must use pre-loaded tx-contexts rather
    /// than re-reading from the (now empty) undo log CF.
    #[test]
    fn test_rollback_via_undo_log_then_rollback_to_block_uses_preloaded_tx_contexts() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0x01; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let input_tx = vec![0x31; 32];
        let consuming_tx = vec![0x32; 32];
        let input_cell = LiveCellInfo {
            capacity: 400,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 400,
            udt_amount: None,
            data_hash: None,
        };
        let output_cell = LiveCellInfo {
            capacity: 200,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
            data_hash: None,
        };

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_cell(&input_tx, 0, &input_cell, 1);
        batch.put_cell(&consuming_tx, 0, &output_cell, 2);
        batch.put_consumed_cell_with_consumer(&input_tx, 0, &input_cell, 1, 2, Some(&consuming_tx));
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
        batch.put_addr_balance(&[0xAA; 32], &AddressBalance::default());
        batch.put_addr_balance(
            &[0xBB; 32],
            &AddressBalance {
                balance: 200,
                used_capacity: 200,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &[0x11; 32],
            &ScriptInfo {
                code_hash: vec![0x11; 32],
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 200,
                lock_owned_knowledge_sum: 200,
                ..Default::default()
            },
        );
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 1);

        // Step 1: rollback_via_undo_log deletes undo entries and returns
        // extracted TxContext entries.
        let undo_result = store.rollback_via_undo_log(&store, 1).unwrap();
        assert_eq!(undo_result.tx_contexts.len(), 1);

        // Verify undo log is now empty for block 2.
        assert!(!store.has_undo_log_entries_after(1).unwrap());

        // Step 2: rollback_to_block_with_tx_contexts uses the pre-loaded
        // tx-context entries (targeted lookup, not full CF scan).
        store
            .rollback_to_block_with_tx_contexts(1, None, undo_result.tx_contexts)
            .unwrap();

        // Verify cell state is correctly rolled back.
        assert!(
            store.get_cell(&input_tx, 0, &store).unwrap().is_some(),
            "input cell should be restored as live"
        );
        assert!(
            store.get_cell(&consuming_tx, 0, &store).unwrap().is_none(),
            "output cell should be removed"
        );
        assert!(
            store
                .get_consumed_cell(&input_tx, 0, &store)
                .unwrap()
                .is_none(),
            "consumed marker should be removed"
        );
    }

    #[test]
    fn test_rollback_restores_ranked_token_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let type_hash = vec![0x90; 32];
        let lock_a = vec![0xA1; 32];
        let lock_b = vec![0xB1; 32];
        let lock_code_hash = vec![0x11; 32];
        let type_code_hash = vec![0x22; 32];
        let input_tx = vec![0x41; 32];
        let transfer_tx = vec![0x42; 32];

        let input_cell = LiveCellInfo {
            capacity: 100,
            lock_script_hash: lock_a.clone(),
            lock_code_hash: lock_code_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(type_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x33; 20]),
            data_size: 16,
            occupied_capacity: 100,
            udt_amount: Some(100),
            data_hash: None,
        };
        let output_cell = LiveCellInfo {
            capacity: 100,
            lock_script_hash: lock_b.clone(),
            lock_code_hash: lock_code_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(type_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x33; 20]),
            data_size: 16,
            occupied_capacity: 100,
            udt_amount: Some(100),
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        put_canonical_tx(&mut batch, 2, 0, &transfer_tx);
        batch.put_cell(&input_tx, 0, &input_cell, 1);
        batch.put_cell(&transfer_tx, 0, &output_cell, 2);
        batch.put_consumed_cell_with_consumer(&input_tx, 0, &input_cell, 1, 2, Some(&transfer_tx));
        batch.delete_cell(&input_tx, 0);
        batch.put_reorg_undo_log_by_block(
            2,
            0,
            &UndoLogEntry::TxContext(UndoTxContext {
                tx_hash: transfer_tx.clone(),
                outputs_count: 1,
                inputs: vec![UndoInputOutPoint {
                    tx_hash: input_tx.clone(),
                    output_index: 0,
                }],
            }),
        );
        batch.put_addr_balance(&lock_a, &AddressBalance::default());
        batch.put_addr_balance(
            &lock_b,
            &AddressBalance {
                balance: 100,
                used_capacity: 100,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &lock_code_hash,
            &ScriptInfo {
                code_hash: lock_code_hash.clone(),
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 100,
                lock_owned_knowledge_sum: 100,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &type_code_hash,
            &ScriptInfo {
                code_hash: type_code_hash.clone(),
                type_live_cells_count: 1,
                type_owned_capacity_sum: 100,
                type_owned_knowledge_sum: 100,
                ..Default::default()
            },
        );
        batch.put_token(
            &type_hash,
            &TokenInfo {
                type_code_hash: type_code_hash.clone(),
                hash_type: 1,
                type_args: vec![0x33; 20],
                standard: "sudt".to_string(),
                name: None,
                symbol: None,
                decimals: Some(8),
                total_supply: Some(100),
                max_supply: None,
                holders_count: 1,
                first_seen_block: 1,
                icon_url: None,
                description: None,
                transfers_count: 0,
            },
        );
        batch.put_token_holder(&type_hash, &lock_b, 100);
        batch.put_token_holder_by_balance(&type_hash, &lock_b, 100);
        batch.put_addr_token_by_balance(&lock_b, &type_hash, 100);
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 1);

        assert_eq!(
            store
                .list_token_holders_by_balance(&type_hash, 10, None)
                .unwrap(),
            vec![(lock_b.clone(), 100)]
        );

        store.rollback_to_block(1).unwrap();

        assert_eq!(
            store.get_token_holder_balance(&type_hash, &lock_a).unwrap(),
            Some(100)
        );
        assert_eq!(
            store.get_token_holder_balance(&type_hash, &lock_b).unwrap(),
            None
        );
        assert_eq!(
            store
                .list_token_holders_by_balance(&type_hash, 10, None)
                .unwrap(),
            vec![(lock_a.clone(), 100)]
        );
        assert_eq!(
            store
                .list_address_tokens_by_balance(&lock_a, 10, None)
                .unwrap(),
            vec![(type_hash.clone(), 100)]
        );
        assert!(store
            .list_address_tokens_by_balance(&lock_b, 10, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_rollback_falls_back_to_full_scan_when_tx_contexts_are_partial() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            cycles: None,
        };

        let input_tx = vec![0x41; 32];
        let consuming_tx_a = vec![0x42; 32];
        let consuming_tx_b = vec![0x43; 32];
        let input_cell = LiveCellInfo {
            capacity: 400,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 400,
            udt_amount: None,
            data_hash: None,
        };
        let rollback_output_cell_a = LiveCellInfo {
            capacity: 200,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
            data_hash: None,
        };
        let rollback_output_cell_b = LiveCellInfo {
            capacity: 180,
            lock_script_hash: vec![0xCC; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 180,
            udt_amount: None,
            data_hash: None,
        };

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: header2.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_tx_index(2, 1, &tx_index);
        batch.put_cell(&input_tx, 0, &input_cell, 1);
        batch.put_cell(&consuming_tx_a, 0, &rollback_output_cell_a, 2);
        batch.put_cell(&consuming_tx_b, 0, &rollback_output_cell_b, 2);
        batch.put_consumed_cell_with_consumer(
            &input_tx,
            0,
            &input_cell,
            1,
            2,
            Some(&consuming_tx_a),
        );
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
        // Seed derived CFs so inline delta application can find them.
        batch.put_addr_balance(&[0xAA; 32], &AddressBalance::default());
        batch.put_addr_balance(
            &[0xBB; 32],
            &AddressBalance {
                balance: 200,
                used_capacity: 200,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_addr_balance(
            &[0xCC; 32],
            &AddressBalance {
                balance: 180,
                used_capacity: 180,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &[0x11; 32],
            &ScriptInfo {
                code_hash: vec![0x11; 32],
                lock_live_cells_count: 2,
                lock_owned_capacity_sum: 380,
                lock_owned_knowledge_sum: 380,
                ..Default::default()
            },
        );
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 2, 2, 2);

        assert!(store.get_cell(&input_tx, 0, &store).unwrap().is_none());
        assert!(store
            .get_cell(&consuming_tx_a, 0, &store)
            .unwrap()
            .is_some());
        assert!(store
            .get_cell(&consuming_tx_b, 0, &store)
            .unwrap()
            .is_some());
        assert!(store
            .get_consumed_cell(&input_tx, 0, &store)
            .unwrap()
            .is_some());

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&input_tx, 0, &store).unwrap().is_some());
        assert!(store
            .get_consumed_cell(&input_tx, 0, &store)
            .unwrap()
            .is_none());
        assert!(store
            .get_cell(&consuming_tx_a, 0, &store)
            .unwrap()
            .is_none());
        assert!(store
            .get_cell(&consuming_tx_b, 0, &store)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_removes_tx_hash_map_entries_above_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
            semantic_tags: 0,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_hash_map(&keep_tx, 1, 0);
        batch.put_tx_hash_map(&drop_tx, 2, 0);
        batch.put_tx_index(2, 0, &tx_index);
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 1);

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
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
            semantic_tags: 0,
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
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 1);

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
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
            semantic_tags: 0,
        };
        let cell = LiveCellInfo {
            capacity: 100,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 100,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_tx_hash_map(&cellbase_tx, 2, 0);
        batch.put_tx_index(2, 0, &tx_index);
        batch.put_cell(&cellbase_tx, 0, &cell, 2);
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
        // Seed derived CFs so inline delta application can find them.
        batch.put_addr_balance(
            &[0xAA; 32],
            &AddressBalance {
                balance: 100,
                used_capacity: 100,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &[0x11; 32],
            &ScriptInfo {
                code_hash: vec![0x11; 32],
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 100,
                lock_owned_knowledge_sum: 100,
                ..Default::default()
            },
        );
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 0);

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&cellbase_tx, 0, &store).unwrap().is_none());
        assert!(store.get_tx_index(2, 0).unwrap().is_none());
        assert_eq!(store.get_tx_location(&cellbase_tx).unwrap(), None);
    }

    #[test]
    fn test_rollback_cleans_rolled_back_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let keep_cell = LiveCellInfo {
            capacity: 100,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 100,
            udt_amount: None,
            data_hash: None,
        };
        let drop_live_cell = LiveCellInfo {
            capacity: 200,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 200,
            udt_amount: None,
            data_hash: None,
        };
        let drop_consumed_cell = LiveCellInfo {
            capacity: 300,
            lock_script_hash: vec![0xCC; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 300,
            udt_amount: None,
            data_hash: None,
        };

        let keep_tx = vec![0x10; 32];
        let drop_live_tx = vec![0x20; 32];
        let drop_consumed_tx = vec![0x30; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&keep_tx, 0, &keep_cell, 1);
        batch.put_cell(&drop_live_tx, 0, &drop_live_cell, 2);
        batch.put_cell(&drop_consumed_tx, 0, &drop_consumed_cell, 2);
        // Seed derived CFs so inline delta application can find them.
        batch.put_addr_balance(
            &[0xAA; 32],
            &AddressBalance {
                balance: 100,
                used_capacity: 100,
                live_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_addr_balance(
            &[0xBB; 32],
            &AddressBalance {
                balance: 200,
                used_capacity: 200,
                live_cells_count: 1,
                total_cells_count: 1,
                ..Default::default()
            },
        );
        batch.put_addr_balance(&[0xCC; 32], &AddressBalance::default());
        batch.put_script_info(
            &[0x11; 32],
            &ScriptInfo {
                code_hash: vec![0x11; 32],
                lock_live_cells_count: 2,
                lock_owned_capacity_sum: 300,
                lock_owned_knowledge_sum: 300,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&drop_consumed_tx, 0, &drop_consumed_cell, 2, 2);
        batch.delete_cell(&drop_consumed_tx, 0);
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_cell(&keep_tx, 0, &store).unwrap().is_some());
        assert!(store.get_cell(&drop_live_tx, 0, &store).unwrap().is_none());
        assert!(store
            .get_cell(&drop_consumed_tx, 0, &store)
            .unwrap()
            .is_none());
        assert!(store
            .get_consumed_cell(&drop_consumed_tx, 0, &store)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_repairs_dao_deposits_and_withdraw_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header3 = CachedBlockHeader {
            hash: vec![0x03; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_020_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
                deposit_timestamp: 0,
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
                deposit_timestamp: 0,
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
                deposit_timestamp: 0,
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
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
                deposit_timestamp: 0,
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
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
    fn test_rollback_to_block_errors_on_addr_balance_total_cells_and_txs_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let tx_hash = vec![0x21; 32];
        let lock_hash = vec![0xAA; 32];
        let lock_code_hash = vec![0x11; 32];
        let cell = LiveCellInfo {
            capacity: 100,
            lock_script_hash: lock_hash.clone(),
            lock_code_hash: lock_code_hash.clone(),
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 100,
            udt_amount: None,
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        put_canonical_tx(&mut batch, 2, 0, &tx_hash);
        batch.put_cell(&tx_hash, 0, &cell, 2);
        batch.put_addr_tx(
            &lock_hash,
            2,
            0,
            &tx_hash,
            &AddrTxValue::new(0, false, true),
        );
        batch.put_addr_balance(
            &lock_hash,
            &AddressBalance {
                balance: 100,
                used_capacity: 100,
                live_cells_count: 1,
                total_cells_count: 0,
                txs_count: 0,
                ..Default::default()
            },
        );
        batch.put_script_info(
            &lock_code_hash,
            &ScriptInfo {
                code_hash: lock_code_hash.clone(),
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 100,
                lock_owned_knowledge_sum: 100,
                ..Default::default()
            },
        );
        batch.commit().unwrap();
        seed_sync_status(&store, 2, &header2.hash, 1, 1, 0);

        let err = store.rollback_to_block(1).unwrap_err();
        assert!(err.to_string().contains("total_cells_count underflow"));
        assert!(err.to_string().contains("txs_count underflow"));
    }

    #[test]
    fn test_rollback_repairs_spore_object_domain_indexes_and_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_010_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let cluster_id = vec![0xAA; 32];
        let owner = vec![0xCC; 32];
        let spore_keep_id = vec![0x11; 32];
        let spore_drop_id = vec![0x22; 32];
        let object_keep_id = vec![0x33; 32];
        let object_drop_id = vec![0x44; 32];
        let class_id = vec![0x55; 24];

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_spore(
            &cluster_id,
            &ObjectEntry {
                standard: ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: None,
                name: Some("cluster-a".to_string()),
                description: Some("desc".to_string()),
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x01; 32],
                extra: ObjectExtra::SporeCluster,
            },
        );
        batch.put_spore(
            &spore_keep_id,
            &ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(cluster_id.clone()),
                token_id: None,
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x02; 32],
                extra: ObjectExtra::Spore {
                    content_type: "image/png".to_string(),
                    content_length: 8,
                    media_profile: SporeMediaProfile {
                        tier: CompositionTier::PureCkb,
                        ..Default::default()
                    },
                },
            },
        );
        batch.put_spore(
            &spore_drop_id,
            &ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(cluster_id.clone()),
                token_id: None,
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 2,
                created_at_tx: vec![0x03; 32],
                extra: ObjectExtra::Spore {
                    content_type: "image/png".to_string(),
                    content_length: 8,
                    media_profile: SporeMediaProfile {
                        tier: CompositionTier::PureCkb,
                        ..Default::default()
                    },
                },
            },
        );
        batch.put_mnft(
            &object_keep_id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(object_keep_id.clone()),
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![],
                extra: ObjectExtra::MnftToken {
                    token_index: 1,
                    characteristic: vec![],
                    configure: 0,
                    state: 0,
                },
            },
        );
        batch.put_mnft(
            &object_drop_id,
            &ObjectEntry {
                standard: ObjectStandard::MnftToken,
                collection_id: Some(class_id.clone()),
                token_id: Some(object_drop_id.clone()),
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 2,
                created_at_tx: vec![],
                extra: ObjectExtra::MnftToken {
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
        batch.put_mnft_by_collection(&class_id, &[0xEE; 32]);
        batch.put_mnft_collection_aggregate(
            &class_id,
            &MnftCollectionAggregate {
                name: Some("stale".to_string()),
                standard: ObjectStandard::MnftClass,
                total_count: 99,
                live_count: 99,
                holders_count: 99,
                activities_count: 99,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        store.rollback_to_block(1).unwrap();

        assert!(store.get_spore(&spore_keep_id).unwrap().is_some());
        assert!(store.get_spore(&spore_drop_id).unwrap().is_none());
        assert!(store.get_mnft(&object_keep_id).unwrap().is_some());
        assert!(store.get_mnft(&object_drop_id).unwrap().is_none());

        let spores_in_cluster = store.list_spores_by_cluster(&cluster_id, 10).unwrap();
        assert_eq!(spores_in_cluster.len(), 1);
        assert_eq!(spores_in_cluster[0].0, spore_keep_id);

        let class_tokens = store
            .list_mnft_ids_by_collection(&class_id, None, 10)
            .unwrap();
        assert_eq!(class_tokens.len(), 1);
        assert_eq!(class_tokens[0], object_keep_id);

        let cluster_agg = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(cluster_agg.total_count, 1);
        assert_eq!(cluster_agg.live_count, 1);
        assert_eq!(cluster_agg.owner_count, 1);
        assert_eq!(cluster_agg.pure_ckb_count, 1);

        let class_agg = store
            .get_mnft_collection_aggregate(&class_id)
            .unwrap()
            .unwrap();
        assert_eq!(class_agg.total_count, 1);
        assert_eq!(class_agg.live_count, 1);
    }

    #[test]
    fn test_rollback_repair_recomputes_cluster_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let cluster_id = vec![0x11u8; 32];
        let spore_id = vec![0x22u8; 32];
        let owner = vec![0xAAu8; 32];

        // Seed block header 200 so rollback_to_block(200) can update sync status.
        let header200 = CachedBlockHeader {
            hash: vec![0xC8; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_780_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 0,
            uncles_count: 0,
            cycles: None,
        };

        // Seed: one live spore in cluster, with daily deltas
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(200, &header200);
        batch.put_spore(
            &spore_id,
            &ObjectEntry {
                standard: ObjectStandard::Spore,
                collection_id: Some(cluster_id.clone()),
                token_id: None,
                owner_lock_hash: Some(owner.clone()),
                name: None,
                description: None,
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0x01; 32],
                extra: ObjectExtra::Spore {
                    content_type: "image/png".to_string(),
                    content_length: 8,
                    media_profile: SporeMediaProfile {
                        tier: CompositionTier::PureCkb,
                        ..Default::default()
                    },
                },
            },
        );
        batch.put_spore_by_cluster(&cluster_id, &spore_id);
        batch.put_cluster_daily_delta(
            &cluster_id,
            20260101,
            &ClusterDailyDelta {
                owned_capacity_delta: 500,
                owned_knowledge_delta: 200,
            },
        );
        batch.put_cluster_daily_delta(
            &cluster_id,
            20260102,
            &ClusterDailyDelta {
                owned_capacity_delta: 300,
                owned_knowledge_delta: 100,
            },
        );
        batch.put_cluster_owner_count(&cluster_id, &owner, 1);
        batch.commit().unwrap();

        // Run rollback (rollback_to > created_at_block so spore survives)
        store.rollback_to_block(200).unwrap();

        let agg = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(agg.owned_capacity, 800); // 500 + 300
        assert_eq!(agg.owned_knowledge, 300); // 200 + 100
    }

    #[test]
    fn test_rollback_truncates_hodl_tracker_state_to_tip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_086_400_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
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
                last_processed_block: Some(2),
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

    #[test]
    fn test_truncate_hodl_tracker_applies_holder_count_delta() {
        // Direct unit test of the truncation function with holder_count_delta.
        let mut state = HodlTrackerState {
            capacity_by_date: vec![("20231114".to_string(), 100), ("20231115".to_string(), 200)],
            date_transitions: vec![(1, "20231114".to_string()), (2, "20231115".to_string())],
            holder_count: 10,
            last_snapshot_date: Some("20231115".to_string()),
            last_processed_block: Some(2),
        };

        // Simulate: one address lost all live cells during rollback (delta = -1).
        let changed = truncate_hodl_tracker_state_for_rollback(&mut state, 1, -1).unwrap();
        assert!(changed);
        assert_eq!(state.holder_count, 9);
        assert_eq!(state.date_transitions, vec![(1, "20231114".to_string())]);
        assert_eq!(state.capacity_by_date, vec![("20231114".to_string(), 100)]);

        // Test with positive delta (address gained live cells on rollback — consumed cell restored).
        let mut state2 = HodlTrackerState {
            capacity_by_date: vec![("20231114".to_string(), 100)],
            date_transitions: vec![(1, "20231114".to_string())],
            holder_count: 5,
            last_snapshot_date: Some("20231114".to_string()),
            last_processed_block: Some(1),
        };
        let changed = truncate_hodl_tracker_state_for_rollback(&mut state2, 1, 2).unwrap();
        assert!(changed);
        assert_eq!(state2.holder_count, 7);

        // Test with zero delta — holder_count unchanged.
        let mut state3 = HodlTrackerState {
            capacity_by_date: vec![("20231114".to_string(), 100), ("20231115".to_string(), 200)],
            date_transitions: vec![(1, "20231114".to_string()), (2, "20231115".to_string())],
            holder_count: 10,
            last_snapshot_date: Some("20231115".to_string()),
            last_processed_block: Some(2),
        };
        let changed = truncate_hodl_tracker_state_for_rollback(&mut state3, 1, 0).unwrap();
        assert!(changed); // dates were truncated
        assert_eq!(state3.holder_count, 10); // holder_count unchanged
    }

    #[test]
    fn test_truncate_hodl_tracker_rejects_holder_count_underflow() {
        let mut state = HodlTrackerState {
            capacity_by_date: vec![("20231114".to_string(), 100)],
            date_transitions: vec![(1, "20231114".to_string())],
            holder_count: 2,
            last_snapshot_date: Some("20231114".to_string()),
            last_processed_block: Some(1),
        };
        // Delta of -5 would make holder_count negative → should error.
        let err = truncate_hodl_tracker_state_for_rollback(&mut state, 1, -5).unwrap_err();
        assert!(err.to_string().contains("holder_count underflow"));
    }

    #[test]
    fn test_rollback_truncates_cell_dist_tracker_state_to_tip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_086_400_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        put_canonical_tx(&mut batch, 1, 0, &[0x10; 32]);
        put_canonical_tx(&mut batch, 2, 0, &[0x20; 32]);
        batch.commit().unwrap();

        store
            .put_cell_dist_tracker_state(&CellDistributionTrackerState {
                count_by_bucket: [5, 3, 0, 0, 0, 0],
                total_capacity_by_bucket: [100, 200, 0, 0, 0, 0],
                date_transitions: vec![(1, "20231114".to_string()), (2, "20231115".to_string())],
                last_snapshot_date: Some("20231115".to_string()),
                cohort_accum: vec![],
                last_processed_block: Some(2),
            })
            .unwrap();

        store.rollback_to_block(1).unwrap();

        let repaired = store.get_cell_dist_tracker_state().unwrap().unwrap();
        assert_eq!(repaired.date_transitions, vec![(1, "20231114".to_string())]);
        // Size totals are preserved; rollback does not have enough per-cell data to recompute them.
        assert_eq!(repaired.total_capacity_by_bucket, [100, 200, 0, 0, 0, 0]);
        // Count totals are preserved for the same reason.
        assert_eq!(repaired.count_by_bucket, [5, 3, 0, 0, 0, 0]);
        assert_eq!(repaired.last_snapshot_date, Some("20231114".to_string()));
    }

    #[test]
    fn test_truncate_cell_dist_tracker_preserves_total_capacity() {
        let mut state = CellDistributionTrackerState {
            count_by_bucket: [10, 5, 3, 2, 0, 0],
            total_capacity_by_bucket: [80, 100, 200, 500, 0, 0],
            date_transitions: vec![
                (1, "20231114".to_string()),
                (100, "20231115".to_string()),
                (200, "20231116".to_string()),
            ],
            last_snapshot_date: Some("20231116".to_string()),
            cohort_accum: vec![],
            last_processed_block: Some(200),
        };

        let changed =
            truncate_cell_dist_tracker_state_for_rollback(&mut state, 150, &[0; 6], &[0; 6])
                .unwrap();
        assert!(changed);
        assert_eq!(state.date_transitions.len(), 2);
        assert_eq!(state.total_capacity_by_bucket, [80, 100, 200, 500, 0, 0]);
        assert_eq!(state.last_snapshot_date, Some("20231115".to_string()));
    }

    #[test]
    fn test_truncate_cell_dist_tracker_preserves_totals_without_age_entries() {
        let mut state = CellDistributionTrackerState {
            count_by_bucket: [10, 5, 3, 2, 0, 0],
            total_capacity_by_bucket: [80, 100, 200, 500, 0, 0],
            date_transitions: vec![
                (1, "20231114".to_string()),
                (100, "20231115".to_string()),
                (200, "20231116".to_string()),
            ],
            last_snapshot_date: Some("20231116".to_string()),
            cohort_accum: vec![],
            last_processed_block: Some(200),
        };

        let changed =
            truncate_cell_dist_tracker_state_for_rollback(&mut state, 150, &[0; 6], &[0; 6])
                .unwrap();
        assert!(changed);
        assert_eq!(state.date_transitions.len(), 2);
        assert_eq!(state.total_capacity_by_bucket, [80, 100, 200, 500, 0, 0]);
        assert_eq!(state.last_snapshot_date, Some("20231115".to_string()));
    }

    #[test]
    fn test_truncate_cell_dist_tracker_rejects_empty_transitions() {
        let mut state = CellDistributionTrackerState {
            count_by_bucket: [1, 0, 0, 0, 0, 0],
            total_capacity_by_bucket: [100, 0, 0, 0, 0, 0],
            date_transitions: vec![(100, "20231115".to_string())],
            last_snapshot_date: Some("20231115".to_string()),
            cohort_accum: vec![],
            last_processed_block: Some(100),
        };
        // Rollback to block 50 — before any transition → should error.
        let err = truncate_cell_dist_tracker_state_for_rollback(&mut state, 50, &[0; 6], &[0; 6])
            .unwrap_err();
        assert!(err.to_string().contains("no remaining date transitions"));
    }

    #[test]
    fn test_cell_dist_bucket_deltas_applied_during_rollback() {
        let mut state = CellDistributionTrackerState {
            count_by_bucket: [10, 5, 3, 0, 0, 0],
            total_capacity_by_bucket: [500, 3000, 50000, 0, 0, 0],
            date_transitions: vec![(1, "20231114".to_string()), (100, "20231115".to_string())],
            last_snapshot_date: Some("20231115".to_string()),
            cohort_accum: vec![],
            last_processed_block: Some(100),
        };
        // Simulate: 2 cells removed from bucket 0, 1 cell restored to bucket 1.
        let count_deltas: [i64; 6] = [-2, 1, 0, 0, 0, 0];
        let cap_deltas: [i128; 6] = [-100, 800, 0, 0, 0, 0];
        let changed = truncate_cell_dist_tracker_state_for_rollback(
            &mut state,
            50,
            &count_deltas,
            &cap_deltas,
        )
        .unwrap();
        assert!(changed);
        assert_eq!(state.count_by_bucket[0], 8); // 10 - 2
        assert_eq!(state.count_by_bucket[1], 6); // 5 + 1
        assert_eq!(state.total_capacity_by_bucket[0], 400); // 500 - 100
        assert_eq!(state.total_capacity_by_bucket[1], 3800); // 3000 + 800
    }

    #[test]
    fn test_cell_dist_bucket_underflow_is_rejected() {
        let mut state = CellDistributionTrackerState {
            count_by_bucket: [1, 0, 0, 0, 0, 0],
            total_capacity_by_bucket: [100, 0, 0, 0, 0, 0],
            date_transitions: vec![(1, "20231114".to_string())],
            last_snapshot_date: Some("20231114".to_string()),
            cohort_accum: vec![],
            last_processed_block: Some(1),
        };
        // Try to remove 5 cells from bucket 0 which only has 1.
        let count_deltas: [i64; 6] = [-5, 0, 0, 0, 0, 0];
        let cap_deltas: [i128; 6] = [0; 6];
        // rollback_to must be > 0 to avoid the early-return for genesis reset.
        let err = truncate_cell_dist_tracker_state_for_rollback(
            &mut state,
            1,
            &count_deltas,
            &cap_deltas,
        )
        .unwrap_err();
        assert!(err.to_string().contains("count_by_bucket underflow"));
    }

    #[test]
    fn test_fiber_commitment_sweep_cleans_settlement_after_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Set up: channel opened at block 50 (before rollback), force-closed
        // at block 100 (before rollback), settled at block 150 (after rollback=120).
        let channel_id = vec![0xCC; 32];
        let commitment_hash = vec![0xDD; 32];
        let channel = FiberChannel {
            funding_tx_hash: vec![0xBB; 32],
            funding_output_index: 0,
            open_block: 50,
            open_timestamp: 1000,
            state: FiberChannelState::Settled,
            capacity: 1000,
            udt_type_hash: None,
            udt_amount: None,
            participants: vec![],
            funding_lock_args: vec![],
            close_tx_hash: Some(vec![0xEE; 32]),
            close_block: Some(100),
            close_timestamp: Some(2000),
            commitment_tx_hash: Some(vec![0xFF; 32]),
            commitment_output_index: Some(0),
            delay_epoch: None,
            settlement_tx_hash: Some(vec![0xAA; 32]),
            settlement_block: Some(150),
            settlement_timestamp: Some(3000),
        };
        // Write channel and commitment index.
        let mut batch = StoreBatch::new(&store);
        batch.put_fiber_channel(&channel_id, &channel);
        batch.put_fiber_channel_by_commitment(&commitment_hash, &channel_id);

        // Need block headers, tx_index, and sync_status for rollback to work.
        for b in 50..=160 {
            batch.put_block_header(
                b,
                &CachedBlockHeader {
                    hash: vec![b as u8; 32],
                    parent_hash: vec![0u8; 32],
                    timestamp: 1_700_000_000_000 + b * 1000,
                    epoch_number: 0,
                    epoch_index: 0,
                    epoch_length: 1800,
                    dao: vec![0; 32],
                    transactions_count: 0,
            uncles_count: 0,
                    cycles: None,
                },
            );
        }
        batch.commit().unwrap();
        seed_sync_status(&store, 160, &[160u8; 32], 0, 0, 0);

        // Rollback to block 120.
        store.rollback_to_block(120).unwrap();

        // Verify commitment index was cleaned up.
        let commitment_entry = store
            .get_cf(store.cf_fiber_channel_by_commitment(), &commitment_hash)
            .unwrap();
        assert!(
            commitment_entry.is_none(),
            "commitment index should be deleted when settlement_block > rollback_to"
        );

        // Verify channel was restored to ForceClosed (close at block 100 survives
        // rollback to 120, only settlement at block 150 is rolled back).
        let ch = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(ch.state, FiberChannelState::ForceClosed);
        assert!(ch.settlement_block.is_none());
        assert!(ch.settlement_tx_hash.is_none());
        // Close fields are preserved because close_block (100) <= rollback_to (120).
        assert_eq!(ch.close_block, Some(100));
        assert_eq!(ch.close_tx_hash, Some(vec![0xEE; 32]));
    }

    #[test]
    fn test_cutoff_day_stats_preserved_during_partial_day_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Place all blocks on the same day (same UTC+8 date).
        // CKB uses UTC+8, so timestamps are in ms.
        // 2024-03-24 12:00:00 UTC+8 = 2024-03-24 04:00:00 UTC = 1711252800 seconds
        let base_ts_ms: i64 = 1_711_252_800_000;

        let mut batch = StoreBatch::new(&store);
        // Blocks 1..5 on the same day, 1 hour apart.
        for b in 1..=5 {
            batch.put_block_header(
                b,
                &CachedBlockHeader {
                    hash: vec![b as u8; 32],
                    parent_hash: vec![0u8; 32],
                    timestamp: base_ts_ms + b * 3_600_000, // 1h apart
                    epoch_number: 0,
                    epoch_index: 0,
                    epoch_length: 1800,
                    dao: vec![0; 32],
                    transactions_count: 3,
            uncles_count: 0,
                    cycles: None,
                },
            );
            // Each block has 1 cellbase + 1 non-cellbase tx.
            let cb_hash = [b as u8; 32];
            let tx_hash = [0x10 + b as u8; 32];
            batch.put_tx_hash_map(&cb_hash, b, 0);
            batch.put_tx_index(
                b,
                0,
                &TxIndexEntry {
                    is_cellbase: true,
                    timestamp: base_ts_ms + b * 3_600_000,
                    inputs_count: 0,
                    outputs_count: 1,
                    fee: 0,
                    tx_size: 100,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
            batch.put_tx_hash_map(&tx_hash, b, 1);
            batch.put_tx_index(
                b,
                1,
                &TxIndexEntry {
                    is_cellbase: false,
                    timestamp: base_ts_ms + b * 3_600_000,
                    inputs_count: 2,
                    outputs_count: 3,
                    fee: 100,
                    tx_size: 200,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
        }
        batch.commit().unwrap();
        // Total txs=10, cells_created=20, cells_consumed=10
        seed_sync_status(&store, 5, &[5u8; 32], 10, 20, 10);

        // Write daily stats for the day covering blocks 1-5.
        // Per block: 2 txs (1 cellbase + 1 non-cb), 4 cells_created (1+3), 2 cells_consumed (0+2)
        // 5 blocks: txs=10, cells_created=20, cells_consumed=10
        let date_str = "20240324";
        let daily_key = keys::encode_stats_key(keys::STATS_PREFIX_DAILY, date_str.as_bytes());
        let daily_stats = DailyStats {
            blocks_count: 5,
            transactions_count: 10,
            cells_created: 20,
            cells_consumed: 10,
            capacity_transferred: 5000,
            used_capacity_created: 0,
            used_capacity_consumed: 0,
            total_live_cells: 100,
            total_dead_cells: 10,
            total_all_cells: 110,
            total_data_size: 500,
            knowledge_size: None,
            avg_block_time_ms: None,
        };
        store
            .put_cf(
                store.cf_stats_chain(),
                &daily_key,
                &bincode::serialize(&daily_stats).unwrap(),
            )
            .unwrap();

        // Rollback to block 3 — blocks 4 and 5 are rolled back.
        // Fork point (block 3) is on the same day as the cutoff.
        store.rollback_to_block(3).unwrap();

        // The daily stats for 20240324 should be REPAIRED, not deleted.
        let repaired_raw = store.get_stats_key(&daily_key).unwrap();
        assert!(
            repaired_raw.is_some(),
            "cutoff-day daily stats must be preserved (repaired), not deleted"
        );
        let repaired: DailyStats = bincode::deserialize(&repaired_raw.unwrap()).unwrap();
        // Rolled back: 2 blocks, 4 txs (2 cb + 2 non-cb), 8 cells_created (2*4), 4 cells_consumed (2*2)
        assert_eq!(repaired.blocks_count, 3); // 5 - 2
        assert_eq!(repaired.transactions_count, 6); // 10 - 4
        assert_eq!(repaired.cells_created, 12); // 20 - 8
        assert_eq!(repaired.cells_consumed, 6); // 10 - 4
    }

    #[test]
    fn test_should_delete_hourly_uses_full_yyyymmddhh() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021015"; // cutoff at hour 15

        // Hour 14 on same date — canonical, should NOT be deleted
        let key_before = keys::encode_stats_key(keys::STATS_PREFIX_HOURLY, b"2026021014");
        assert!(!should_delete_stats_for_replay(&key_before, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hour 15 (cutoff hour) — should be deleted (repair handles it)
        let key_at = keys::encode_stats_key(keys::STATS_PREFIX_HOURLY, b"2026021015");
        assert!(should_delete_stats_for_replay(&key_at, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hour 16 — after cutoff, should be deleted
        let key_after = keys::encode_stats_key(keys::STATS_PREFIX_HOURLY, b"2026021016");
        assert!(should_delete_stats_for_replay(&key_after, cutoff, cutoff_hh, 0, 0).unwrap());

        // Previous day — should NOT be deleted
        let key_prev_day = keys::encode_stats_key(keys::STATS_PREFIX_HOURLY, b"2026020923");
        assert!(!should_delete_stats_for_replay(&key_prev_day, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_should_delete_activity_hourly_uses_full_yyyymmddhh() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021015";

        // Hour 14 — canonical, NOT deleted
        let key = keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_HOURLY, b"2026021014");
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hour 15 (cutoff) — deleted
        let key = keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_HOURLY, b"2026021015");
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hour 16 — deleted
        let key = keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_HOURLY, b"2026021016");
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_addr_set_preserved_on_cutoff_date() {
        let cutoff = b"20260210";
        let cutoff_hh = b"2026021015";

        // Daily ADDR_SET on cutoff date — preserved (strict >)
        let key = keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_DAILY_ADDR_SET, b"20260210");
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        // Daily ADDR_SET day after — deleted
        let key = keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_DAILY_ADDR_SET, b"20260211");
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hourly ADDR_SET at cutoff hour — preserved (strict >)
        let key =
            keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_HOURLY_ADDR_SET, b"2026021015");
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hourly ADDR_SET hour before cutoff — preserved
        let key =
            keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_HOURLY_ADDR_SET, b"2026021014");
        assert!(!should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());

        // Hourly ADDR_SET hour after cutoff — deleted
        let key =
            keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_HOURLY_ADDR_SET, b"2026021016");
        assert!(should_delete_stats_for_replay(&key, cutoff, cutoff_hh, 0, 0).unwrap());
    }

    #[test]
    fn test_repair_activity_daily_subtracts_deltas() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Write an ACTIVITY_DAILY entry for the cutoff date
        let date = "20260210";
        let original = DailyActivityStats {
            transfer_count: 100,
            dao_deposit_count: 10,
            dao_withdraw_request_count: 5,
            dao_withdraw_complete_count: 3,
            token_count: 20,
            object_count: 15,
            identity_count: 8,
            script_call_count: 12,
            unknown_count: 0,
            coinbase_count: 50,
            unique_address_count: 200,
            total_ckb_moved: 500_000,
            script_counts: std::collections::HashMap::new(),
            protocol_action_counts: std::collections::HashMap::new(),
        };
        store.put_daily_activity_stats(date, &original).unwrap();

        // Build deltas: pretend some activity was rolled back
        let activity_delta = DailyActivityStats {
            transfer_count: 3,
            coinbase_count: 1,
            total_ckb_moved: 10_000,
            ..Default::default()
        };

        let mut activity_date = std::collections::HashMap::new();
        activity_date.insert(date.to_string(), activity_delta);

        let deltas = RollbackStatsDeltas {
            date: std::collections::HashMap::new(),
            date_uncles: std::collections::HashMap::new(),
            hour: std::collections::HashMap::new(),
            date_capacity: std::collections::HashMap::new(),
            activity_date,
            activity_hour: std::collections::HashMap::new(),
            miner: std::collections::HashMap::new(),
        };

        let key = keys::encode_stats_key(keys::STATS_PREFIX_ACTIVITY_DAILY, date.as_bytes());
        let value = bincode::serialize(&original).unwrap();
        let mut batch = rocksdb::WriteBatch::default();

        let repaired = repair_cutoff_date_stats(
            &key,
            &value,
            date,
            "2026021015",
            &deltas,
            &store,
            &mut batch,
        )
        .unwrap();

        assert!(repaired, "ACTIVITY_DAILY should be repaired, not deleted");

        // Apply the batch and read back
        store.write_batch(batch).unwrap();
        let result = store.get_daily_activity_stats(date).unwrap().unwrap();
        assert_eq!(result.transfer_count, 97);
        assert_eq!(result.coinbase_count, 49);
        assert_eq!(result.total_ckb_moved, 490_000);
        // unique_address_count preserved
        assert_eq!(result.unique_address_count, 200);
    }

    #[test]
    fn test_repair_daily_block_subtracts_count() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let date = "20260210";
        let original = DailyBlockStats {
            avg_difficulty: 1000.0,
            block_count: 720,
            total_uncles: 5,
            avg_block_time_ms: Some(10000),
        };
        let key = keys::encode_stats_key(keys::STATS_PREFIX_DAILY_BLOCK, date.as_bytes());
        let value = bincode::serialize(&original).unwrap();

        let mut date_deltas = std::collections::HashMap::new();
        date_deltas.insert(date.to_string(), (2i32, 10, 5, 3)); // 2 blocks rolled back

        let deltas = RollbackStatsDeltas {
            date: date_deltas,
            date_uncles: std::collections::HashMap::new(),
            hour: std::collections::HashMap::new(),
            date_capacity: std::collections::HashMap::new(),
            activity_date: std::collections::HashMap::new(),
            activity_hour: std::collections::HashMap::new(),
            miner: std::collections::HashMap::new(),
        };

        let mut batch = rocksdb::WriteBatch::default();
        let repaired = repair_cutoff_date_stats(
            &key,
            &value,
            date,
            "2026021015",
            &deltas,
            &store,
            &mut batch,
        )
        .unwrap();

        assert!(repaired, "DAILY_BLOCK should be repaired");

        // Apply batch and verify block_count was decremented
        store.write_batch(batch).unwrap();
        let raw = store.get_stats_key(&key).unwrap().unwrap();
        let result: DailyBlockStats = bincode::deserialize(&raw).unwrap();
        assert_eq!(result.block_count, 718); // 720 - 2
    }

    #[test]
    fn test_repair_miner_preserves_unaffected() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let date = "20260210";
        let miner_hash = [0xBB; 32];
        let original = MinerStats {
            miner_lock_hash: miner_hash.to_vec(),
            blocks_count: 50,
            last_block_number: 1000,
        };

        let suffix = [date.as_bytes(), &miner_hash[..]].concat();
        let key = keys::encode_stats_key(keys::STATS_PREFIX_MINER, &suffix);
        let value = bincode::serialize(&original).unwrap();

        // Empty miner deltas — this miner not rolled back
        let deltas = RollbackStatsDeltas {
            date: std::collections::HashMap::new(),
            date_uncles: std::collections::HashMap::new(),
            hour: std::collections::HashMap::new(),
            date_capacity: std::collections::HashMap::new(),
            activity_date: std::collections::HashMap::new(),
            activity_hour: std::collections::HashMap::new(),
            miner: std::collections::HashMap::new(),
        };

        let mut batch = rocksdb::WriteBatch::default();
        let repaired = repair_cutoff_date_stats(
            &key,
            &value,
            date,
            "2026021015",
            &deltas,
            &store,
            &mut batch,
        )
        .unwrap();

        assert!(repaired, "unaffected MINER should be preserved (Ok(true))");
    }

    #[test]
    fn test_rollback_repairs_daily_block_stats_total_uncles() {
        // 2026-04-08 00:00 UTC+8 = 2026-04-07 16:00 UTC = 1775577600000 ms.
        // All four block timestamps land on 2026-04-08 in UTC+8, so:
        //   - fork_point (block 3) date == "20260408"
        //   - rolled-back block (block 4) date == "20260408"
        //   - is_partial_day == true → repair_cutoff_date_stats runs
        let day_start_ms: i64 = 1775577600000;

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let headers = [
            CachedBlockHeader {
                hash: vec![0x01; 32],
                parent_hash: vec![0u8; 32],
                timestamp: day_start_ms + 1000,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 1800,
                dao: vec![0u8; 32],
                transactions_count: 1,
                uncles_count: 0,
                cycles: None,
            },
            CachedBlockHeader {
                hash: vec![0x02; 32],
                parent_hash: vec![0x01; 32],
                timestamp: day_start_ms + 2000,
                epoch_number: 1,
                epoch_index: 1,
                epoch_length: 1800,
                dao: vec![0u8; 32],
                transactions_count: 1,
                uncles_count: 1, // this block has an uncle
                cycles: None,
            },
            CachedBlockHeader {
                hash: vec![0x03; 32],
                parent_hash: vec![0x02; 32],
                timestamp: day_start_ms + 3000,
                epoch_number: 1,
                epoch_index: 2,
                epoch_length: 1800,
                dao: vec![0u8; 32],
                transactions_count: 1,
                uncles_count: 0,
                cycles: None,
            },
            CachedBlockHeader {
                hash: vec![0x04; 32],
                parent_hash: vec![0x03; 32],
                timestamp: day_start_ms + 4000,
                epoch_number: 1,
                epoch_index: 3,
                epoch_length: 1800,
                dao: vec![0u8; 32],
                transactions_count: 1,
                uncles_count: 1, // this block (to be rolled back) has an uncle
                cycles: None,
            },
        ];

        let mut batch = StoreBatch::new(&store);
        for (i, h) in headers.iter().enumerate() {
            batch.put_block_header(i as i64 + 1, h);
        }
        batch.commit().unwrap();

        // Seed pre-reorg DailyBlockStats: 4 blocks, 2 uncles.
        let pre_reorg = crate::types::DailyBlockStats {
            avg_difficulty: 1.0,
            block_count: 4,
            total_uncles: 2,
            avg_block_time_ms: Some(1000),
        };
        store.put_daily_block_stats("20260408", &pre_reorg).unwrap();

        // Rollback block 4 — the one with 1 uncle.
        // rollback_to=3 means block 4 is removed.
        store.rollback_to_block(3).unwrap();

        // Verify: DailyBlockStats for 2026-04-08 should now have
        // block_count=3, total_uncles=1 (not 2).
        let repaired = store
            .get_daily_block_stats("20260408")
            .unwrap()
            .expect("DailyBlockStats for 20260408 must still exist after partial-day rollback");
        assert_eq!(repaired.block_count, 3, "block_count must be 3 after rollback");
        assert_eq!(
            repaired.total_uncles,
            1,
            "total_uncles must be decremented from 2 → 1 when the rolled-back block had 1 uncle (currently {})",
            repaired.total_uncles
        );
    }
}
