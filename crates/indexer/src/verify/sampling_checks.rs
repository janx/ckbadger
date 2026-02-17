//! Sampling-tier checks (minutes, N = --sample-count).

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

/// Return all sampling-tier checks.
pub fn sampling_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(BlockHashRoundtrip),
        Box::new(TxHashRoundtrip),
        Box::new(LiveCellIndexIntegrity),
        Box::new(AddressBalanceAccuracy),
        Box::new(DaoDepositConsistency),
        Box::new(RpcBlockSpotCheck),
    ]
}
