//! Sampling-tier checks (minutes, N = --sample-count).

use std::collections::HashMap;

use rocksdb::IteratorMode;

use super::checks::*;
use super::sampling::LcgSampler;

/// S1: N random blocks: get_block_header(n).hash → get_block_number_by_hash(hash) == n.
pub struct BlockHashRoundtrip;

impl Check for BlockHashRoundtrip {
    fn name(&self) -> &'static str {
        "block_hash_roundtrip"
    }
    fn description(&self) -> &'static str {
        "Block hash ↔ block number roundtrip consistency"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number as u64;
        if tip == 0 {
            return Ok(CheckResult::pass(0));
        }

        let mut sampler = LcgSampler::new(ctx.seed);
        let blocks = sampler.sample_range(ctx.sample_count, tip + 1);
        let mut findings = vec![];

        for block_num in &blocks {
            let header = ctx.store.get_block_header(*block_num as i64)?;
            if let Some(header) = header {
                let reverse = ctx.store.get_block_number_by_hash(&header.hash)?;
                match reverse {
                    Some(n) if n == *block_num as i64 => {}
                    Some(n) => {
                        findings.push(Finding {
                            entity: format!("block #{}", block_num),
                            details: vec![format!(
                                "hash → block_number = {}, expected {}",
                                n, block_num
                            )],
                        });
                    }
                    None => {
                        findings.push(Finding {
                            entity: format!("block #{}", block_num),
                            details: vec![
                                "block_hash_index entry missing for this block's hash".to_string()
                            ],
                        });
                    }
                }
            } else {
                findings.push(Finding {
                    entity: format!("block #{}", block_num),
                    details: vec!["block header not found".to_string()],
                });
            }
            progress.inc(1);
        }

        let checked = blocks.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S2: N random blocks: pick first TX, verify tx_hash_map roundtrip.
pub struct TxHashRoundtrip;

impl Check for TxHashRoundtrip {
    fn name(&self) -> &'static str {
        "tx_hash_roundtrip"
    }
    fn description(&self) -> &'static str {
        "TX hash → (block, index) roundtrip via tx_hash_map"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number as u64;
        if tip == 0 {
            return Ok(CheckResult::pass(0));
        }

        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(1));
        let blocks = sampler.sample_range(ctx.sample_count, tip + 1);
        let mut findings = vec![];
        let mut checked = 0u64;

        for block_num in &blocks {
            // Look up the tx_index CF for this block's transactions
            // The tx_index key format: block_num(8B) + tx_idx(4B) + tx_hash(32B)
            let prefix = ckbadger_store::keys::encode_block_num(*block_num as i64);
            let iter = ctx
                .store
                .prefix_iterator_cf(ctx.store.cf_tx_index(), &prefix);

            // Get first tx
            if let Some((key, _)) = iter.flatten().next() {
                if key.len() >= 44 && key.starts_with(&prefix) {
                    let tx_hash = &key[12..44]; // block_num(8) + tx_idx(4) + tx_hash(32)

                    // Verify tx_hash_map contains this hash
                    let lookup = ctx.store.get_cf(ctx.store.cf_tx_hash_map(), tx_hash)?;

                    match lookup {
                        Some(val) if val.len() == 12 => {
                            let mapped_block = ckbadger_store::keys::decode_block_num(&val[..8]);
                            if mapped_block != *block_num as i64 {
                                findings.push(Finding {
                                    entity: format!("block #{} tx", block_num),
                                    details: vec![format!(
                                        "tx_hash_map points to block {}, expected {}",
                                        mapped_block, block_num
                                    )],
                                });
                            }
                        }
                        Some(val) => {
                            findings.push(Finding {
                                entity: format!("block #{} tx", block_num),
                                details: vec![format!(
                                    "tx_hash_map value is {} bytes, expected 12",
                                    val.len()
                                )],
                            });
                        }
                        None => {
                            findings.push(Finding {
                                entity: format!("block #{} tx", block_num),
                                details: vec![
                                    "tx_hash_map entry missing for this transaction".to_string()
                                ],
                            });
                        }
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S3: N sampled live cells: verify cell_by_lock and cell_by_type index entries exist.
pub struct LiveCellIndexIntegrity;

impl Check for LiveCellIndexIntegrity {
    fn name(&self) -> &'static str {
        "live_cell_index_integrity"
    }
    fn description(&self) -> &'static str {
        "Sampled live cells have cell_by_lock and cell_by_type index entries"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_live_cells(), IteratorMode::Start);

        let mut count = 0u64;
        let mut total = 0u64;
        let skip = super::sampling::skip_interval(u64::MAX, ctx.sample_count);
        let mut findings = vec![];

        for item in iter.flatten() {
            total += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if total % skip != 0 {
                continue;
            }
            let (key, value) = item;
            if key.len() != ckbadger_store::keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: ckbadger_store::types::LiveCellInfo = match bincode::deserialize(&value) {
                Ok(i) => i,
                Err(_) => continue,
            };
            let (tx_hash, output_index) = ckbadger_store::keys::decode_outpoint(&key);

            // Check cell_by_lock index
            let lock_key = ckbadger_store::keys::encode_cell_index_key(
                &info.lock_script_hash,
                info.created_at_block,
                &tx_hash,
                output_index,
            );
            let has_lock = ctx
                .store
                .get_cf(ctx.store.cf_cell_by_lock(), &lock_key)?
                .is_some();
            if !has_lock {
                findings.push(Finding {
                    entity: format!("cell {}:{}", hex::encode(&tx_hash[..4]), output_index),
                    details: vec!["missing cell_by_lock index entry".to_string()],
                });
            }

            // Check cell_by_type index (if type script present)
            if let Some(ref type_hash) = info.type_script_hash {
                let type_key = ckbadger_store::keys::encode_cell_index_key(
                    type_hash,
                    info.created_at_block,
                    &tx_hash,
                    output_index,
                );
                let has_type = ctx
                    .store
                    .get_cf(ctx.store.cf_cell_by_type(), &type_key)?
                    .is_some();
                if !has_type {
                    findings.push(Finding {
                        entity: format!("cell {}:{}", hex::encode(&tx_hash[..4]), output_index),
                        details: vec!["missing cell_by_type index entry".to_string()],
                    });
                }
            }

            count += 1;
            progress.inc(1);
            if count >= ctx.sample_count as u64 {
                break;
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(count))
        } else {
            Ok(CheckResult::fail(count, findings))
        }
    }
}

/// S4: N sampled addresses: recompute balance from list_cells_by_lock, compare with addr_balance.
pub struct AddressBalanceAccuracy;

impl Check for AddressBalanceAccuracy {
    fn name(&self) -> &'static str {
        "address_balance_accuracy"
    }
    fn description(&self) -> &'static str {
        "Sampled address balances match sum of live cells"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_addr_balance(), IteratorMode::Start);

        let mut count = 0u64;
        let mut total = 0u64;
        let skip = super::sampling::skip_interval(u64::MAX, ctx.sample_count);
        let mut findings = vec![];

        for item in iter.flatten() {
            total += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if total % skip != 0 {
                continue;
            }
            let (key, value) = item;
            if key.len() != 32 {
                continue;
            }
            let stored: ckbadger_store::types::AddressBalance = match bincode::deserialize(&value) {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Recompute from live cells
            let cells = ctx.store.list_cells_by_lock(&key, usize::MAX, None, None)?;
            let computed_balance: i128 = cells.iter().map(|(_, _, c)| c.capacity as i128).sum();
            let computed_count = cells.len() as i32;

            let mut details = vec![];
            if stored.balance != computed_balance {
                details.push(format!(
                    "balance: stored = {}, computed = {} (Δ {})",
                    stored.balance,
                    computed_balance,
                    computed_balance - stored.balance,
                ));
            }
            if stored.live_cells_count != computed_count {
                details.push(format!(
                    "live_cells: stored = {}, actual = {}",
                    stored.live_cells_count, computed_count,
                ));
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("lock_hash: 0x{}", hex::encode(&key[..8])),
                    details,
                });
            }

            count += 1;
            progress.inc(1);
            if count >= ctx.sample_count as u64 {
                break;
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(count))
        } else {
            Ok(CheckResult::fail(count, findings))
        }
    }
}

/// S5: N sampled deposits: if status=0 then no withdraw fields, if status≠0 then withdraw fields present.
pub struct DaoDepositConsistency;

impl Check for DaoDepositConsistency {
    fn name(&self) -> &'static str {
        "dao_deposit_consistency"
    }
    fn description(&self) -> &'static str {
        "DAO deposit status fields are internally consistent"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_dao_deposits(), IteratorMode::Start);

        let mut count = 0u64;
        let mut total = 0u64;
        let skip = super::sampling::skip_interval(u64::MAX, ctx.sample_count);
        let mut findings = vec![];

        for item in iter.flatten() {
            total += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if total % skip != 0 {
                continue;
            }
            let (key, value) = item;
            let entry: ckbadger_store::types::DaoDepositCacheEntry =
                match bincode::deserialize(&value) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

            let entity = format!("deposit 0x{}", hex::encode(&key[..8]));

            if entry.status == 0 {
                // Active deposit: should have no withdraw fields
                if entry.withdraw_request_tx.is_some() {
                    findings.push(Finding {
                        entity: entity.clone(),
                        details: vec![
                            "status=0 (active) but withdraw_request_tx is Some".to_string()
                        ],
                    });
                }
                if entry.withdraw_block.is_some() {
                    findings.push(Finding {
                        entity,
                        details: vec!["status=0 (active) but withdraw_block is Some".to_string()],
                    });
                }
            } else {
                // Withdraw requested or completed
                if entry.withdraw_request_tx.is_none() {
                    findings.push(Finding {
                        entity: entity.clone(),
                        details: vec![format!(
                            "status={} but withdraw_request_tx is None",
                            entry.status
                        )],
                    });
                }
                if entry.status == 2 && entry.withdraw_block.is_none() {
                    findings.push(Finding {
                        entity,
                        details: vec![
                            "status=2 (completed) but withdraw_block is None".to_string(),
                        ],
                    });
                }
            }

            count += 1;
            progress.inc(1);
            if count >= ctx.sample_count as u64 {
                break;
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(count))
        } else {
            Ok(CheckResult::fail(count, findings))
        }
    }
}

/// S6: N random blocks: compare hash, tx count, DAO field against CKB RPC.
pub struct RpcBlockSpotCheck;

impl Check for RpcBlockSpotCheck {
    fn name(&self) -> &'static str {
        "rpc_block_spot_check"
    }
    fn description(&self) -> &'static str {
        "Compare block data against CKB RPC node"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let rpc = ctx
            .rpc
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("RPC client not available"))?;

        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number as u64;
        if tip == 0 {
            return Ok(CheckResult::pass(0));
        }

        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(2));
        let blocks = sampler.sample_range(ctx.sample_count, tip + 1);
        let mut findings = vec![];

        // Run RPC calls in a tokio runtime
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        for block_num in &blocks {
            let header = ctx.store.get_block_header(*block_num as i64)?;
            if let Some(header) = header {
                let rpc_block = rt.block_on(rpc.get_block_by_number(*block_num))?;
                if let Some(rpc_block) = rpc_block {
                    let rpc_header = &rpc_block.block.header;
                    let rpc_hash = ckbadger_common::parse_hex_to_bytes(&rpc_header.hash);
                    let rpc_tx_count = rpc_block.block.transactions.len() as i32;
                    let rpc_dao = ckbadger_common::parse_hex_to_bytes(&rpc_header.dao);

                    let mut details = vec![];
                    if header.hash != rpc_hash {
                        details.push(format!(
                            "hash mismatch: ours=0x{}, rpc={}",
                            hex::encode(&header.hash[..8]),
                            &rpc_header.hash[..18],
                        ));
                    }
                    if header.transactions_count != rpc_tx_count {
                        details.push(format!(
                            "tx_count: ours={}, rpc={}",
                            header.transactions_count, rpc_tx_count,
                        ));
                    }
                    if header.dao != rpc_dao {
                        details.push("DAO field mismatch".to_string());
                    }

                    if !details.is_empty() {
                        findings.push(Finding {
                            entity: format!("block #{}", block_num),
                            details,
                        });
                    }
                }
            }
            progress.inc(1);
        }

        let checked = blocks.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S7: Consecutive daily stats: cell delta consistency.
/// Verifies that cumulative cell counters track correctly day-over-day.
pub struct DailyStatsCellDeltaConsistency;

impl Check for DailyStatsCellDeltaConsistency {
    fn name(&self) -> &'static str {
        "daily_stats_cell_delta_consistency"
    }
    fn description(&self) -> &'static str {
        "Daily cell counters are self-consistent across consecutive days"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let all_stats = ctx.store.list_daily_stats_with_dates()?;
        if all_stats.len() < 2 {
            return Ok(CheckResult::pass(0));
        }

        let mut findings = vec![];
        let mut checked = 0u64;

        // Sample consecutive pairs
        let max_pairs = all_stats.len() - 1;
        let n = ctx.sample_count.min(max_pairs);
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(7));
        let indices = sampler.sample_range(n, max_pairs as u64);

        for idx in &indices {
            let i = *idx as usize;
            let (date_prev, prev) = &all_stats[i];
            let (date_next, next) = &all_stats[i + 1];

            let mut details = vec![];

            // Check: next.total_live_cells - prev.total_live_cells == next.cells_created - next.cells_consumed
            let live_delta = next.total_live_cells - prev.total_live_cells;
            let expected_live_delta = next.cells_created as i64 - next.cells_consumed as i64;
            if live_delta != expected_live_delta {
                details.push(format!(
                    "live_cells delta: {} ({}→{}), but created-consumed = {}-{} = {}",
                    live_delta,
                    prev.total_live_cells,
                    next.total_live_cells,
                    next.cells_created,
                    next.cells_consumed,
                    expected_live_delta,
                ));
            }

            // Check: next.total_dead_cells - prev.total_dead_cells == next.cells_consumed
            let dead_delta = next.total_dead_cells - prev.total_dead_cells;
            if dead_delta != next.cells_consumed as i64 {
                details.push(format!(
                    "dead_cells delta: {} ({}→{}), but cells_consumed = {}",
                    dead_delta, prev.total_dead_cells, next.total_dead_cells, next.cells_consumed,
                ));
            }

            // Check: total_all_cells == total_live_cells + total_dead_cells
            let expected_all = next.total_live_cells + next.total_dead_cells;
            if next.total_all_cells != expected_all {
                details.push(format!(
                    "total_all_cells = {}, but live + dead = {} + {} = {}",
                    next.total_all_cells,
                    next.total_live_cells,
                    next.total_dead_cells,
                    expected_all,
                ));
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("{} → {}", date_prev, date_next),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S8: Cross-check knowledge_size (DailyStats) vs occupied_capacity (DaoDailySnapshot).
/// Both derive from the DAO U field through different code paths.
pub struct DailyStatsKnowledgeSizeVsDao;

impl Check for DailyStatsKnowledgeSizeVsDao {
    fn name(&self) -> &'static str {
        "daily_stats_knowledge_size_vs_dao"
    }
    fn description(&self) -> &'static str {
        "knowledge_size matches occupied_capacity from DAO snapshots"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        // Genesis burnt: 8,400,000,000 CKB * 0.6 = 5,040,000,000 CKB = 504,000,000,000,000,000 shannons
        const BURN_OFFSET: i128 = 504_000_000_000_000_000;

        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        let snap_map: HashMap<String, _> = snapshots
            .into_iter()
            .map(|s| (s.date.replace('-', ""), s))
            .collect();

        let all_stats = ctx.store.list_daily_stats_with_dates()?;
        if all_stats.is_empty() {
            return Ok(CheckResult::pass(0));
        }

        // Build list of dates that have both knowledge_size and a snapshot
        let matchable: Vec<(String, i128, i128)> = all_stats
            .iter()
            .filter_map(|(date, stats)| {
                let ks = stats.knowledge_size?;
                let snap = snap_map.get(date)?;
                Some((date.clone(), ks, snap.occupied_capacity))
            })
            .collect();

        let n = ctx.sample_count.min(matchable.len());
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(8));
        let indices = sampler.sample_range(n, matchable.len() as u64);

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in &indices {
            let (date, ks, occ) = &matchable[*idx as usize];
            let expected_ks = occ - BURN_OFFSET;
            if *ks != expected_ks {
                findings.push(Finding {
                    entity: date.clone(),
                    details: vec![format!(
                        "knowledge_size = {}, but occupied_capacity - burn_offset = {} - {} = {}",
                        ks, occ, BURN_OFFSET, expected_ks,
                    )],
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S9: DAO snapshot monotonicity check.
/// Verifies cumulative fields never decrease across chronological snapshots.
pub struct DaoSnapshotMonotonicity;

impl Check for DaoSnapshotMonotonicity {
    fn name(&self) -> &'static str {
        "dao_snapshot_monotonicity"
    }
    fn description(&self) -> &'static str {
        "DAO cumulative fields are monotonically non-decreasing"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        if snapshots.len() < 2 {
            return Ok(CheckResult::pass(0));
        }

        let mut findings = vec![];
        let mut checked = 0u64;

        // Sample consecutive pairs
        let max_pairs = snapshots.len() - 1;
        let n = ctx.sample_count.min(max_pairs);
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(9));
        let indices = sampler.sample_range(n, max_pairs as u64);

        for idx in &indices {
            let i = *idx as usize;
            let prev = &snapshots[i];
            let next = &snapshots[i + 1];

            let mut details = vec![];

            if next.total_issuance < prev.total_issuance {
                details.push(format!(
                    "total_issuance decreased: {} → {}",
                    prev.total_issuance, next.total_issuance,
                ));
            }
            if next.cum_miner_secondary < prev.cum_miner_secondary {
                details.push(format!(
                    "cum_miner_secondary decreased: {} → {}",
                    prev.cum_miner_secondary, next.cum_miner_secondary,
                ));
            }
            if next.cum_dao_compensation < prev.cum_dao_compensation {
                details.push(format!(
                    "cum_dao_compensation decreased: {} → {}",
                    prev.cum_dao_compensation, next.cum_dao_compensation,
                ));
            }
            if next.cum_treasury < prev.cum_treasury {
                details.push(format!(
                    "cum_treasury decreased: {} → {}",
                    prev.cum_treasury, next.cum_treasury,
                ));
            }
            if next.new_deposits < prev.new_deposits {
                details.push(format!(
                    "new_deposits (cumulative) decreased: {} → {}",
                    prev.new_deposits, next.new_deposits,
                ));
            }
            if next.withdrawals < prev.withdrawals {
                details.push(format!(
                    "withdrawals (cumulative) decreased: {} → {}",
                    prev.withdrawals, next.withdrawals,
                ));
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("{} → {}", prev.date, next.date),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S10: DAO secondary issuance sum check.
/// Verifies increments of cumulative secondary issuance components are non-negative
/// across consecutive snapshots.
pub struct DaoSecondaryIssuanceSum;

impl Check for DaoSecondaryIssuanceSum {
    fn name(&self) -> &'static str {
        "dao_secondary_issuance_sum"
    }
    fn description(&self) -> &'static str {
        "DAO secondary issuance component increments are non-negative"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        if snapshots.len() < 2 {
            return Ok(CheckResult::pass(0));
        }

        let mut findings = vec![];
        let mut checked = 0u64;

        let max_pairs = snapshots.len() - 1;
        let n = ctx.sample_count.min(max_pairs);
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(10));
        let indices = sampler.sample_range(n, max_pairs as u64);

        for idx in &indices {
            let i = *idx as usize;
            let prev = &snapshots[i];
            let next = &snapshots[i + 1];

            let delta_miner = next.cum_miner_secondary - prev.cum_miner_secondary;
            let delta_dao = next.cum_dao_compensation - prev.cum_dao_compensation;
            let delta_treasury = next.cum_treasury - prev.cum_treasury;

            let mut details = vec![];

            if delta_miner < 0 {
                details.push(format!(
                    "cum_miner_secondary delta is negative: {}",
                    delta_miner,
                ));
            }
            if delta_dao < 0 {
                details.push(format!(
                    "cum_dao_compensation delta is negative: {}",
                    delta_dao,
                ));
            }
            if delta_treasury < 0 {
                details.push(format!(
                    "cum_treasury delta is negative: {}",
                    delta_treasury,
                ));
            }

            // Cross-check: delta_dao + delta_treasury should approximately equal
            // the S field delta (secondary_pool delta), since S tracks depositor + treasury share
            let delta_s = next.secondary_pool - prev.secondary_pool;
            let dao_plus_treasury = delta_dao + delta_treasury;
            if delta_s > 0 && dao_plus_treasury > 0 {
                let diff = (delta_s - dao_plus_treasury).abs();
                // Allow 1 shannon tolerance per block (~576 blocks/day) for rounding
                if diff > 576 {
                    details.push(format!(
                        "S-field delta ({}) != dao+treasury delta ({}) (diff: {})",
                        delta_s, dao_plus_treasury, diff,
                    ));
                }
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("{} → {}", prev.date, next.date),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S11: Cross-check DailyBlockStats.block_count vs DailyStats.blocks_count.
/// Two independent accumulators tracking the same thing.
pub struct DailyBlockStatsCountVsDailyStats;

impl Check for DailyBlockStatsCountVsDailyStats {
    fn name(&self) -> &'static str {
        "daily_block_stats_count_vs_daily_stats"
    }
    fn description(&self) -> &'static str {
        "DailyBlockStats.block_count matches DailyStats.blocks_count"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let daily_stats = ctx.store.list_daily_stats_with_dates()?;
        let block_stats = ctx.store.list_daily_block_stats()?;

        // Build a map of date -> block_count from DailyBlockStats
        let block_stats_map: HashMap<String, i32> = block_stats
            .into_iter()
            .map(|(date, stats)| (date, stats.block_count))
            .collect();

        // Build list of matchable entries
        let matchable: Vec<(String, i32, i32)> = daily_stats
            .iter()
            .filter_map(|(date, stats)| {
                let bs_count = block_stats_map.get(date)?;
                Some((date.clone(), stats.blocks_count, *bs_count))
            })
            .collect();

        if matchable.is_empty() {
            return Ok(CheckResult::pass(0));
        }

        let n = ctx.sample_count.min(matchable.len());
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(11));
        let indices = sampler.sample_range(n, matchable.len() as u64);

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in &indices {
            let (date, daily_count, block_count) = &matchable[*idx as usize];
            if daily_count != block_count {
                findings.push(Finding {
                    entity: date.clone(),
                    details: vec![format!(
                        "DailyStats.blocks_count = {}, DailyBlockStats.block_count = {}",
                        daily_count, block_count,
                    )],
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S12: Circulating supply sanity check.
/// Verifies basic invariants: circulating > 0, circulating < total_issuance, etc.
pub struct CirculatingSupplySanity;

impl Check for CirculatingSupplySanity {
    fn name(&self) -> &'static str {
        "circulating_supply_sanity"
    }
    fn description(&self) -> &'static str {
        "Circulating supply invariants (positive, bounded)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        // Genesis burnt: 8,400,000,000 CKB = 840,000,000,000,000,000 shannons
        const BURNT_SHANNONS: i128 = 840_000_000_000_000_000;

        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        if snapshots.is_empty() {
            return Ok(CheckResult::pass(0));
        }

        let n = ctx.sample_count.min(snapshots.len());
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(12));
        let indices = sampler.sample_range(n, snapshots.len() as u64);

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in &indices {
            let snap = &snapshots[*idx as usize];
            let circulating = snap.total_issuance - BURNT_SHANNONS - snap.total_deposited;

            let mut details = vec![];

            if circulating <= 0 {
                details.push(format!(
                    "circulating = {} (total_issuance={} - burnt={} - deposited={})",
                    circulating, snap.total_issuance, BURNT_SHANNONS, snap.total_deposited,
                ));
            }
            if circulating >= snap.total_issuance {
                details.push(format!(
                    "circulating ({}) >= total_issuance ({})",
                    circulating, snap.total_issuance,
                ));
            }
            if snap.total_deposited < 0 {
                details.push(format!(
                    "total_deposited is negative: {}",
                    snap.total_deposited,
                ));
            }
            if snap.total_deposited >= snap.total_issuance {
                details.push(format!(
                    "total_deposited ({}) >= total_issuance ({})",
                    snap.total_deposited, snap.total_issuance,
                ));
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: snap.date.clone(),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S13: Cross-check DailyStats.transactions_count against block header tx counts.
/// Scans block headers forward, groups by date, and compares accumulated
/// transaction counts against stored daily stats for sampled days.
pub struct DailyStatsTxCountVsBlocks;

impl Check for DailyStatsTxCountVsBlocks {
    fn name(&self) -> &'static str {
        "daily_stats_tx_count_vs_blocks"
    }
    fn description(&self) -> &'static str {
        "DailyStats.transactions_count matches sum of block tx counts"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let daily_stats = ctx.store.list_daily_stats_with_dates()?;
        if daily_stats.is_empty() {
            return Ok(CheckResult::pass(0));
        }

        // Build set of sampled dates
        let n = ctx.sample_count.min(daily_stats.len());
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(13));
        let indices = sampler.sample_range(n, daily_stats.len() as u64);
        let sampled_dates: std::collections::HashSet<String> = indices
            .iter()
            .map(|i| daily_stats[*i as usize].0.clone())
            .collect();

        // Build map of stored daily stats for quick lookup
        let stats_map: HashMap<String, &ckbadger_store::types::DailyStats> =
            daily_stats.iter().map(|(d, s)| (d.clone(), s)).collect();

        // Scan block headers, accumulate tx counts per date
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_block_headers(), IteratorMode::Start);

        let mut tx_counts_by_date: HashMap<String, i64> = HashMap::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != 8 {
                continue;
            }
            let header: ckbadger_store::types::CachedBlockHeader =
                match bincode::deserialize(&value) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
            // Convert timestamp_ms to date key "YYYYMMDD"
            let date_key = match chrono::DateTime::from_timestamp_millis(header.timestamp) {
                Some(dt) => dt.format("%Y%m%d").to_string(),
                None => continue,
            };
            // Only accumulate for sampled dates to save memory
            if sampled_dates.contains(&date_key) {
                *tx_counts_by_date.entry(date_key).or_insert(0) += header.transactions_count as i64;
            }
        }

        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &sampled_dates {
            if let (Some(stored), Some(computed)) =
                (stats_map.get(date), tx_counts_by_date.get(date))
            {
                if stored.transactions_count as i64 != *computed {
                    findings.push(Finding {
                        entity: date.clone(),
                        details: vec![format!(
                            "DailyStats.transactions_count = {}, sum of block tx counts = {}",
                            stored.transactions_count, computed,
                        )],
                    });
                }
                checked += 1;
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S14: DAO snapshot cumulative_deposit_amount monotonicity.
/// This field tracks the gross lifetime deposit amount and must never decrease.
pub struct DaoCumulativeDepositMonotonicity;

impl Check for DaoCumulativeDepositMonotonicity {
    fn name(&self) -> &'static str {
        "dao_cumulative_deposit_monotonicity"
    }
    fn description(&self) -> &'static str {
        "cumulative_deposit_amount is monotonically non-decreasing"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        if snapshots.len() < 2 {
            return Ok(CheckResult::pass(0));
        }

        let max_pairs = snapshots.len() - 1;
        let n = ctx.sample_count.min(max_pairs);
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(14));
        let indices = sampler.sample_range(n, max_pairs as u64);

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in &indices {
            let i = *idx as usize;
            let prev = &snapshots[i];
            let next = &snapshots[i + 1];

            let mut details = vec![];

            if next.cumulative_deposit_amount < prev.cumulative_deposit_amount {
                details.push(format!(
                    "cumulative_deposit_amount decreased: {} → {} (Δ {})",
                    prev.cumulative_deposit_amount,
                    next.cumulative_deposit_amount,
                    next.cumulative_deposit_amount - prev.cumulative_deposit_amount,
                ));
            }

            // Also: cumulative_deposit_amount >= total_deposited (gross >= net)
            if next.cumulative_deposit_amount < next.total_deposited {
                details.push(format!(
                    "cumulative_deposit_amount ({}) < total_deposited ({})",
                    next.cumulative_deposit_amount, next.total_deposited,
                ));
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("{} → {}", prev.date, next.date),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S15: DAO issuance decomposition consistency.
/// Verifies that the three secondary issuance components (miner, depositor, treasury)
/// sum correctly relative to total issuance and the DAO S field.
///
/// Key relationship: total secondary issuance = cum_miner + S (where S = cum_dao + cum_treasury).
/// So: total_issuance - genesis_primary(33.6B CKB) should approximately equal
/// secondary_accumulated + primary_since_genesis.
///
/// This check verifies:
/// 1. cum_miner_secondary + secondary_pool ≈ total_secondary (from C field)
/// 2. The secondary sum never exceeds total_issuance minus genesis
pub struct DaoIssuanceDecomposition;

impl Check for DaoIssuanceDecomposition {
    fn name(&self) -> &'static str {
        "dao_issuance_decomposition"
    }
    fn description(&self) -> &'static str {
        "Secondary issuance components sum consistently with DAO fields"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        // Genesis total issuance: 33,600,000,000 CKB = 3,360,000,000,000,000,000 shannons
        const GENESIS_ISSUANCE: i128 = 3_360_000_000_000_000_000;

        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        if snapshots.is_empty() {
            return Ok(CheckResult::pass(0));
        }

        let n = ctx.sample_count.min(snapshots.len());
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(15));
        let indices = sampler.sample_range(n, snapshots.len() as u64);

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in &indices {
            let snap = &snapshots[*idx as usize];
            let mut details = vec![];

            let secondary_sum =
                snap.cum_miner_secondary + snap.cum_dao_compensation + snap.cum_treasury;

            // total_issuance must be at least genesis (CKB only inflates)
            if snap.total_issuance < GENESIS_ISSUANCE {
                details.push(format!(
                    "total_issuance ({}) < genesis issuance ({})",
                    snap.total_issuance, GENESIS_ISSUANCE,
                ));
            }

            // Total issuance since genesis = primary_since_genesis + secondary_since_genesis
            // secondary_sum should be the total secondary issuance we've tracked
            // It must be non-negative
            if secondary_sum < 0 {
                details.push(format!(
                    "secondary issuance sum is negative: miner({}) + dao({}) + treasury({}) = {}",
                    snap.cum_miner_secondary,
                    snap.cum_dao_compensation,
                    snap.cum_treasury,
                    secondary_sum,
                ));
            }

            // Secondary cannot exceed total issuance minus genesis
            let issuance_since_genesis = snap.total_issuance - GENESIS_ISSUANCE;
            if secondary_sum > issuance_since_genesis {
                details.push(format!(
                    "secondary sum ({}) > issuance since genesis ({})",
                    secondary_sum, issuance_since_genesis,
                ));
            }

            // Cross-check: cum_miner_secondary should be roughly consistent with
            // total_secondary - S_field. The S field in the DAO header tracks the
            // cumulative depositor + treasury secondary issuance.
            // So: cum_miner_secondary ≈ total_secondary - secondary_pool
            // Which means: cum_miner_secondary + secondary_pool ≈ total_secondary
            // And: cum_dao_compensation + cum_treasury ≈ secondary_pool
            let dao_plus_treasury = snap.cum_dao_compensation + snap.cum_treasury;
            if snap.secondary_pool > 0 && dao_plus_treasury > 0 {
                let diff = (snap.secondary_pool - dao_plus_treasury).abs();
                // Allow small tolerance (1 shannon per block, ~18M blocks max)
                if diff > 20_000_000 {
                    details.push(format!(
                        "S field ({}) != dao+treasury ({}) (diff: {})",
                        snap.secondary_pool, dao_plus_treasury, diff,
                    ));
                }
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: snap.date.clone(),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// Return all sampling-tier checks.
pub fn sampling_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(BlockHashRoundtrip),
        Box::new(TxHashRoundtrip),
        Box::new(LiveCellIndexIntegrity),
        Box::new(AddressBalanceAccuracy),
        Box::new(DaoDepositConsistency),
        Box::new(RpcBlockSpotCheck),
        Box::new(DailyStatsCellDeltaConsistency),
        Box::new(DailyStatsKnowledgeSizeVsDao),
        Box::new(DaoSnapshotMonotonicity),
        Box::new(DaoSecondaryIssuanceSum),
        Box::new(DailyBlockStatsCountVsDailyStats),
        Box::new(CirculatingSupplySanity),
        Box::new(DailyStatsTxCountVsBlocks),
        Box::new(DaoCumulativeDepositMonotonicity),
        Box::new(DaoIssuanceDecomposition),
    ]
}
