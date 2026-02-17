//! Exhaustive-tier checks (hours, full scan).

use rocksdb::IteratorMode;

use super::checks::*;
use super::report::format_number;

/// E1: Every live cell has cell_by_lock entry; cells with type_script have cell_by_type entry.
pub struct FullLiveCellIndexScan;

impl Check for FullLiveCellIndexScan {
    fn name(&self) -> &'static str {
        "full_live_cell_index_scan"
    }
    fn description(&self) -> &'static str {
        "Every live cell has cell_by_lock and cell_by_type index entries"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Exhaustive
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_live_cells(), IteratorMode::Start);

        let mut count = 0u64;
        let mut findings = vec![];

        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != ckbadger_store::keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: ckbadger_store::types::LiveCellInfo = match bincode::deserialize(&value) {
                Ok(i) => i,
                Err(_) => continue,
            };
            let (tx_hash, output_index) = ckbadger_store::keys::decode_outpoint(&key);

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
            #[allow(clippy::manual_is_multiple_of)]
            if count % 10_000 == 0 {
                progress.inc(10_000);
            }

            // Cap findings to avoid overwhelming output
            if findings.len() >= 100 {
                break;
            }
        }
        progress.inc(count % 10_000);

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                count,
                format!("{} live cells verified", format_number(count)),
            ))
        } else {
            Ok(CheckResult::fail(count, findings))
        }
    }
}

/// E2: Sum all live cells by lock_hash, compare every addr_balance entry.
pub struct FullAddressBalanceRecompute;

impl Check for FullAddressBalanceRecompute {
    fn name(&self) -> &'static str {
        "full_address_balance_recompute"
    }
    fn description(&self) -> &'static str {
        "Recompute all address balances from live cells"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Exhaustive
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        // Phase 1: Sum all live cells by lock_hash
        let mut computed: std::collections::HashMap<Vec<u8>, (i128, i32)> =
            std::collections::HashMap::new();

        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_live_cells(), IteratorMode::Start);

        let mut cells_scanned = 0u64;
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != ckbadger_store::keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: ckbadger_store::types::LiveCellInfo = match bincode::deserialize(&value) {
                Ok(i) => i,
                Err(_) => continue,
            };
            let entry = computed
                .entry(info.lock_script_hash.clone())
                .or_insert((0, 0));
            entry.0 += info.capacity as i128;
            entry.1 += 1;

            cells_scanned += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if cells_scanned % 100_000 == 0 {
                progress.set_message(&format!(
                    "scanning live cells... {}",
                    format_number(cells_scanned)
                ));
                progress.inc(100_000);
            }
        }
        progress.set_message("comparing balances...");

        // Phase 2: Compare with addr_balance CF
        let mut findings = vec![];
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_addr_balance(), IteratorMode::Start);

        let mut addrs_checked = 0u64;
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != 32 {
                continue;
            }
            let stored: ckbadger_store::types::AddressBalance = match bincode::deserialize(&value) {
                Ok(b) => b,
                Err(_) => continue,
            };

            let (comp_balance, comp_count) = computed.remove(&key.to_vec()).unwrap_or((0, 0));

            let mut details = vec![];
            if stored.balance != comp_balance {
                details.push(format!(
                    "balance: stored={}, computed={} (Δ {})",
                    stored.balance,
                    comp_balance,
                    comp_balance - stored.balance,
                ));
            }
            if stored.live_cells_count != comp_count {
                details.push(format!(
                    "live_cells: stored={}, actual={}",
                    stored.live_cells_count, comp_count,
                ));
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("lock_hash: 0x{}", hex::encode(&key[..8])),
                    details,
                });
            }

            addrs_checked += 1;
            if findings.len() >= 100 {
                break;
            }
        }

        // Check for addresses with live cells but no addr_balance entry
        for (lock_hash, (balance, count)) in &computed {
            if *balance > 0 || *count > 0 {
                findings.push(Finding {
                    entity: format!("lock_hash: 0x{}", hex::encode(&lock_hash[..8])),
                    details: vec![format!(
                        "has {} live cells with {} balance but no addr_balance entry",
                        count, balance,
                    )],
                });
                if findings.len() >= 100 {
                    break;
                }
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                addrs_checked,
                format!(
                    "{} addresses verified from {} cells",
                    format_number(addrs_checked),
                    format_number(cells_scanned),
                ),
            ))
        } else {
            Ok(CheckResult::fail(addrs_checked, findings))
        }
    }
}

/// E3: Block numbers 0..tip form contiguous sequence with no gaps.
pub struct FullChainContinuity;

impl Check for FullChainContinuity {
    fn name(&self) -> &'static str {
        "full_chain_continuity"
    }
    fn description(&self) -> &'static str {
        "Block numbers 0..tip form contiguous sequence"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Exhaustive
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number;
        if tip <= 0 {
            return Ok(CheckResult::pass(0));
        }

        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_block_headers(), IteratorMode::Start);

        let mut expected = 0i64;
        let mut gaps = vec![];
        let mut count = 0u64;

        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() != 8 {
                continue;
            }
            let block_num = ckbadger_store::keys::decode_block_num(&key);

            if block_num != expected {
                gaps.push((expected, block_num));
                if gaps.len() >= 100 {
                    break;
                }
            }
            expected = block_num + 1;
            count += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if count % 1_000_000 == 0 {
                progress.inc(1_000_000);
            }
        }
        progress.inc(count % 1_000_000);

        if gaps.is_empty() {
            Ok(CheckResult::pass_with_detail(
                count,
                format!("contiguous chain from 0 to {}", format_number(tip as u64),),
            ))
        } else {
            let findings: Vec<Finding> = gaps
                .iter()
                .map(|(expected, actual)| Finding {
                    entity: format!("gap at block #{}", expected),
                    details: vec![format!("expected block {}, got {}", expected, actual,)],
                })
                .collect();
            Ok(CheckResult::fail(count, findings))
        }
    }
}

/// E4: Every block's TXs have matching tx_hash_map entries.
pub struct FullTxIndexIntegrity;

impl Check for FullTxIndexIntegrity {
    fn name(&self) -> &'static str {
        "full_tx_index_integrity"
    }
    fn description(&self) -> &'static str {
        "Every block's TXs have matching tx_hash_map entries"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Exhaustive
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_tx_index(), IteratorMode::Start);

        let mut count = 0u64;
        let mut findings = vec![];

        for item in iter.flatten() {
            let (key, _) = item;
            // tx_index key: block_num(8) + tx_idx(4) + tx_hash(32) = 44
            if key.len() != 44 {
                continue;
            }
            let block_num = ckbadger_store::keys::decode_block_num(&key[..8]);
            let tx_hash = &key[12..44];

            let lookup = ctx.store.get_cf(ctx.store.cf_tx_hash_map(), tx_hash)?;
            match lookup {
                Some(val) if val.len() == 12 => {
                    let mapped_block = ckbadger_store::keys::decode_block_num(&val[..8]);
                    if mapped_block != block_num {
                        findings.push(Finding {
                            entity: format!(
                                "tx 0x{} in block #{}",
                                hex::encode(&tx_hash[..4]),
                                block_num,
                            ),
                            details: vec![format!(
                                "tx_hash_map points to block {}, expected {}",
                                mapped_block, block_num,
                            )],
                        });
                    }
                }
                Some(val) => {
                    findings.push(Finding {
                        entity: format!(
                            "tx 0x{} in block #{}",
                            hex::encode(&tx_hash[..4]),
                            block_num,
                        ),
                        details: vec![format!(
                            "tx_hash_map value is {} bytes, expected 12",
                            val.len(),
                        )],
                    });
                }
                None => {
                    findings.push(Finding {
                        entity: format!(
                            "tx 0x{} in block #{}",
                            hex::encode(&tx_hash[..4]),
                            block_num,
                        ),
                        details: vec!["tx_hash_map entry missing".to_string()],
                    });
                }
            }

            count += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if count % 1_000_000 == 0 {
                progress.inc(1_000_000);
            }
            if findings.len() >= 100 {
                break;
            }
        }
        progress.inc(count % 1_000_000);

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                count,
                format!("{} transactions verified", format_number(count)),
            ))
        } else {
            Ok(CheckResult::fail(count, findings))
        }
    }
}

/// E5: Sum of all live cells' occupied_capacity ≤ DAO U field at tip.
pub struct OccupiedCapacityVsDaoU;

impl Check for OccupiedCapacityVsDaoU {
    fn name(&self) -> &'static str {
        "occupied_capacity_vs_dao_u"
    }
    fn description(&self) -> &'static str {
        "Sum of live cells' occupied_capacity ≤ DAO U field at tip"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Exhaustive
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number;

        let tip_header = ctx.store.get_block_header(tip)?;
        let dao_u = tip_header
            .as_ref()
            .and_then(|h| {
                if h.dao.len() >= 32 {
                    Some(u64::from_le_bytes(h.dao[24..32].try_into().ok()?))
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let iter = ctx
            .store
            .iterator_cf(ctx.store.cf_live_cells(), IteratorMode::Start);

        let mut total_occupied: u128 = 0;
        let mut count = 0u64;

        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != ckbadger_store::keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: ckbadger_store::types::LiveCellInfo = match bincode::deserialize(&value) {
                Ok(i) => i,
                Err(_) => continue,
            };
            total_occupied += info.occupied_capacity as u128;
            count += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if count % 100_000 == 0 {
                progress.inc(100_000);
            }
        }
        progress.inc(count % 100_000);

        if total_occupied <= dao_u as u128 {
            Ok(CheckResult::pass_with_detail(
                count,
                format!(
                    "occupied={} ≤ DAO U={} ({:.1}%)",
                    format_number(total_occupied as u64),
                    format_number(dao_u),
                    (total_occupied as f64 / dao_u.max(1) as f64) * 100.0,
                ),
            ))
        } else {
            Ok(CheckResult::fail(
                count,
                vec![Finding {
                    entity: "occupied_capacity".to_string(),
                    details: vec![
                        format!("sum of occupied_capacity = {}", total_occupied),
                        format!("DAO U field at tip = {}", dao_u),
                        format!("overflow = {} shannons", total_occupied - dao_u as u128,),
                    ],
                }],
            ))
        }
    }
}

/// Return all exhaustive-tier checks.
pub fn exhaustive_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(FullLiveCellIndexScan),
        Box::new(FullAddressBalanceRecompute),
        Box::new(FullChainContinuity),
        Box::new(FullTxIndexIntegrity),
        Box::new(OccupiedCapacityVsDaoU),
    ]
}
