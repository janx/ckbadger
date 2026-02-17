//! Fast-tier checks (seconds, always run).

use super::checks::*;
use super::report::format_number;

/// F1: sync_status.tip_block_number matches last key in block_headers CF.
pub struct SyncTipConsistency;

impl Check for SyncTipConsistency {
    fn name(&self) -> &'static str {
        "sync_tip_consistency"
    }
    fn description(&self) -> &'static str {
        "sync_status.tip_block_number matches last key in block_headers CF"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let sync_status = ctx.store.get_sync_status()?;
        let tip_from_status = sync_status.tip_block_number;

        let tip_from_headers = ctx
            .store
            .get_sync_tip_block()?
            .map(|(n, _)| n)
            .unwrap_or(-1);

        if tip_from_status == tip_from_headers {
            Ok(CheckResult::pass_with_detail(
                1,
                format!("tip = #{}", format_number(tip_from_status as u64)),
            ))
        } else {
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "sync_tip".to_string(),
                    details: vec![
                        format!("sync_status.tip_block_number = {}", tip_from_status),
                        format!("block_headers last key = {}", tip_from_headers),
                    ],
                }],
            ))
        }
    }
}

/// F2: total_cells_created - total_cells_consumed == live_cells_count().
pub struct CellCountBalance;

impl Check for CellCountBalance {
    fn name(&self) -> &'static str {
        "cell_count_balance"
    }
    fn description(&self) -> &'static str {
        "total_cells_created - total_cells_consumed == live_cells_count()"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let sync_status = ctx.store.get_sync_status()?;
        let created = sync_status.total_cells_created;
        let consumed = sync_status.total_cells_consumed;
        let expected_live = created - consumed;

        let actual_live = ctx.store.live_cells_count() as i64;

        if expected_live == actual_live {
            Ok(CheckResult::pass_with_detail(
                1,
                format!(
                    "created({}) - consumed({}) = {} live cells",
                    format_number(created as u64),
                    format_number(consumed as u64),
                    format_number(actual_live as u64),
                ),
            ))
        } else {
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "cell_count".to_string(),
                    details: vec![
                        format!(
                            "created({}) - consumed({}) = {} expected",
                            created, consumed, expected_live
                        ),
                        format!("live_cells_count() = {}", actual_live),
                        format!("delta = {}", actual_live - expected_live),
                    ],
                }],
            ))
        }
    }
}

/// F3: deep_fork_detected flag is false.
pub struct NoUnresolvedDeepFork;

impl Check for NoUnresolvedDeepFork {
    fn name(&self) -> &'static str {
        "no_unresolved_deep_fork"
    }
    fn description(&self) -> &'static str {
        "deep_fork_detected flag is false"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let has_fork = ctx.store.has_unresolved_deep_fork()?;
        if !has_fork {
            Ok(CheckResult::pass(1))
        } else {
            let info = ctx.store.get_deep_fork_info()?;
            let details = match info {
                Some(info) => vec![
                    format!("db_tip = {}, chain_tip = {}", info.db_tip, info.chain_tip),
                    format!("depth = {}, fork_point = {}", info.depth, info.fork_point),
                ],
                None => vec!["deep_fork_detected = true but no info available".to_string()],
            };
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "deep_fork".to_string(),
                    details,
                }],
            ))
        }
    }
}

/// F4: address_balances_deferred is false.
pub struct DeferredFlagsCleared;

impl Check for DeferredFlagsCleared {
    fn name(&self) -> &'static str {
        "deferred_flags_cleared"
    }
    fn description(&self) -> &'static str {
        "address_balances_deferred is false"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let mut findings = vec![];

        if status.address_balances_deferred {
            findings.push(Finding {
                entity: "deferred_flags".to_string(),
                details: vec!["address_balances_deferred = true".to_string()],
            });
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(1))
        } else {
            Ok(CheckResult::fail(1, findings))
        }
    }
}

/// F5: Block 0 exists with 32-byte DAO field.
pub struct GenesisBlockExists;

impl Check for GenesisBlockExists {
    fn name(&self) -> &'static str {
        "genesis_block_exists"
    }
    fn description(&self) -> &'static str {
        "Block 0 exists with 32-byte DAO field"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        match ctx.store.get_block_header(0)? {
            Some(header) => {
                if header.dao.len() == 32 {
                    Ok(CheckResult::pass(1))
                } else {
                    Ok(CheckResult::fail(
                        1,
                        vec![Finding {
                            entity: "block_0".to_string(),
                            details: vec![format!(
                                "DAO field is {} bytes, expected 32",
                                header.dao.len()
                            )],
                        }],
                    ))
                }
            }
            None => Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "block_0".to_string(),
                    details: vec!["Block 0 not found in block_headers CF".to_string()],
                }],
            )),
        }
    }
}

/// F6: Tip block exists with valid DAO field and tx count > 0.
pub struct TipBlockExists;

impl Check for TipBlockExists {
    fn name(&self) -> &'static str {
        "tip_block_exists"
    }
    fn description(&self) -> &'static str {
        "Tip block exists with valid DAO field and tx count > 0"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number;

        match ctx.store.get_block_header(tip)? {
            Some(header) => {
                let mut findings = vec![];
                if header.dao.len() != 32 {
                    findings.push(Finding {
                        entity: format!("block_{}", tip),
                        details: vec![format!(
                            "DAO field is {} bytes, expected 32",
                            header.dao.len()
                        )],
                    });
                }
                if header.transactions_count <= 0 {
                    findings.push(Finding {
                        entity: format!("block_{}", tip),
                        details: vec![format!(
                            "transactions_count = {}, expected > 0",
                            header.transactions_count
                        )],
                    });
                }
                if findings.is_empty() {
                    Ok(CheckResult::pass(1))
                } else {
                    Ok(CheckResult::fail(1, findings))
                }
            }
            None => Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: format!("block_{}", tip),
                    details: vec![format!("Tip block {} not found in block_headers CF", tip)],
                }],
            )),
        }
    }
}

/// F7: DAO C/S/U fields at tip >= genesis (spot-check midpoint too).
pub struct DaoMonotonicity;

impl Check for DaoMonotonicity {
    fn name(&self) -> &'static str {
        "dao_monotonicity"
    }
    fn description(&self) -> &'static str {
        "DAO C/S/U fields at tip >= genesis (spot-check midpoint too)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let tip = status.tip_block_number;
        if tip <= 0 {
            return Ok(CheckResult::pass_with_detail(1, "tip=0, skipping"));
        }

        let genesis = ctx.store.get_block_header(0)?;
        let tip_header = ctx.store.get_block_header(tip)?;
        let mid_header = ctx.store.get_block_header(tip / 2)?;

        let (genesis, tip_header) = match (genesis, tip_header) {
            (Some(g), Some(t)) => (g, t),
            _ => {
                return Ok(CheckResult::fail(
                    1,
                    vec![Finding {
                        entity: "dao_monotonicity".to_string(),
                        details: vec!["Could not read genesis or tip block header".to_string()],
                    }],
                ));
            }
        };

        fn parse_csu(dao: &[u8]) -> Option<(u64, u64, u64)> {
            if dao.len() < 32 {
                return None;
            }
            let c = u64::from_le_bytes(dao[0..8].try_into().ok()?);
            let s = u64::from_le_bytes(dao[16..24].try_into().ok()?);
            let u = u64::from_le_bytes(dao[24..32].try_into().ok()?);
            Some((c, s, u))
        }

        let genesis_csu = parse_csu(&genesis.dao);
        let tip_csu = parse_csu(&tip_header.dao);

        let (gc, gs, gu) = genesis_csu.unwrap_or((0, 0, 0));
        let (tc, ts, tu) = tip_csu.unwrap_or((0, 0, 0));

        let mut findings = vec![];
        if tc < gc {
            findings.push(Finding {
                entity: "dao_C".to_string(),
                details: vec![format!("tip C ({}) < genesis C ({})", tc, gc)],
            });
        }
        if ts < gs {
            findings.push(Finding {
                entity: "dao_S".to_string(),
                details: vec![format!("tip S ({}) < genesis S ({})", ts, gs)],
            });
        }
        if tu < gu {
            findings.push(Finding {
                entity: "dao_U".to_string(),
                details: vec![format!("tip U ({}) < genesis U ({})", tu, gu)],
            });
        }

        // Check midpoint too
        if let Some(mid) = mid_header {
            if let Some((mc, ms, _mu)) = parse_csu(&mid.dao) {
                if mc > tc {
                    findings.push(Finding {
                        entity: "dao_C_mid".to_string(),
                        details: vec![format!("midpoint C ({}) > tip C ({})", mc, tc)],
                    });
                }
                if ms > ts {
                    findings.push(Finding {
                        entity: "dao_S_mid".to_string(),
                        details: vec![format!("midpoint S ({}) > tip S ({})", ms, ts)],
                    });
                }
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(3))
        } else {
            Ok(CheckResult::fail(3, findings))
        }
    }
}

/// F8: block_headers_count() == tip_block_number + 1.
pub struct BlockCountMatchesTip;

impl Check for BlockCountMatchesTip {
    fn name(&self) -> &'static str {
        "block_count_matches_tip"
    }
    fn description(&self) -> &'static str {
        "block_headers_count() == tip_block_number + 1"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let status = ctx.store.get_sync_status()?;
        let expected = (status.tip_block_number + 1) as usize;
        let actual = ctx.store.block_headers_count();

        if actual == expected {
            Ok(CheckResult::pass_with_detail(
                1,
                format!("{} block headers", format_number(actual as u64)),
            ))
        } else {
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "block_count".to_string(),
                    details: vec![
                        format!(
                            "expected: tip({}) + 1 = {}",
                            status.tip_block_number, expected
                        ),
                        format!("actual block_headers_count: {}", actual),
                        format!("delta: {}", actual as i64 - expected as i64),
                    ],
                }],
            ))
        }
    }
}

/// Return all fast-tier checks.
pub fn fast_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(SyncTipConsistency),
        Box::new(CellCountBalance),
        Box::new(NoUnresolvedDeepFork),
        Box::new(DeferredFlagsCleared),
        Box::new(GenesisBlockExists),
        Box::new(TipBlockExists),
        Box::new(DaoMonotonicity),
        Box::new(BlockCountMatchesTip),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::CkbadgerStore;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_ctx(store: Arc<CkbadgerStore>) -> CheckContext {
        CheckContext {
            store,
            rpc: None,
            explorer_url: None,
            http_client: reqwest::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
        }
    }

    fn no_progress() -> ProgressReporter {
        ProgressReporter::new(None)
    }

    #[test]
    fn test_sync_tip_consistency_pass_empty_store() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let ctx = test_ctx(store);
        // Empty store: sync_status tip = 0 (default), no block headers
        // This will fail because tip is 0 but no block at 0
        // get_sync_tip_block returns None -> -1, so 0 != -1
        let result = SyncTipConsistency.run(&ctx, &no_progress()).unwrap();
        assert!(!result.passed);
    }

    #[test]
    fn test_no_unresolved_deep_fork_pass() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let ctx = test_ctx(store);
        let result = NoUnresolvedDeepFork.run(&ctx, &no_progress()).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_deferred_flags_cleared_pass() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let ctx = test_ctx(store);
        let result = DeferredFlagsCleared.run(&ctx, &no_progress()).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_deferred_flags_cleared_fail() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        store
            .update_sync_status(|s| {
                s.address_balances_deferred = true;
            })
            .unwrap();
        let ctx = test_ctx(store);
        let result = DeferredFlagsCleared.run(&ctx, &no_progress()).unwrap();
        assert!(!result.passed);
    }

    #[test]
    fn test_genesis_block_missing() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let ctx = test_ctx(store);
        let result = GenesisBlockExists.run(&ctx, &no_progress()).unwrap();
        assert!(!result.passed);
    }

    #[test]
    fn test_genesis_block_exists_pass() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());

        // Write a block header at position 0
        let header = ckbadger_store::types::CachedBlockHeader {
            hash: vec![0u8; 32],
            timestamp: 1000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1000,
            dao: vec![0u8; 32],
            transactions_count: 1,
        };
        let key = ckbadger_store::keys::encode_block_num(0);
        let value = bincode::serialize(&header).unwrap();
        store
            .put_cf(store.cf_block_headers(), &key, &value)
            .unwrap();

        let ctx = test_ctx(store);
        let result = GenesisBlockExists.run(&ctx, &no_progress()).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_block_count_matches_tip_empty() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let ctx = test_ctx(store);
        // Default sync status has tip=0, block_headers_count=0
        // expected = 0+1 = 1, actual = 0 -> fail
        let result = BlockCountMatchesTip.run(&ctx, &no_progress()).unwrap();
        assert!(!result.passed);
    }
}
