use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::{AddressBalance, CkbadgerStore, CF_ADDR_BALANCE};

use super::{BulkReducer, ReducerContext};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{CellFacts, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct AddressOwner {
    balances: HashMap<Vec<u8>, AddressBalance>,
}

impl AddressOwner {
    #[cfg(test)]
    pub(crate) fn balances(&self) -> &HashMap<Vec<u8>, AddressBalance> {
        &self.balances
    }
}

impl BulkReducer for AddressOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts, ctx: &ReducerContext<'_>) -> Result<()> {
        let mut tx_balance_changes: HashMap<Vec<u8>, i128> = HashMap::new();
        let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
        let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();
        let mut tx_used_capacity_changes: HashMap<Vec<u8>, i128> = HashMap::new();

        for input in &tx.resolved_inputs {
            apply_input_deltas(
                input,
                ctx,
                &mut tx_balance_changes,
                &mut tx_cells_consumed,
                &mut tx_used_capacity_changes,
            )?;
        }

        for cell in &tx.cells {
            apply_output_deltas(
                cell,
                ctx,
                &mut tx_balance_changes,
                &mut tx_cells_created,
                &mut tx_used_capacity_changes,
            )?;
        }

        let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
            .keys()
            .chain(tx_cells_created.keys())
            .chain(tx_cells_consumed.keys())
            .chain(tx_used_capacity_changes.keys())
            .cloned()
            .collect();

        for lock_hash in all_addresses {
            let balance_delta = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
            let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
            let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);
            let used_delta = tx_used_capacity_changes
                .get(&lock_hash)
                .copied()
                .unwrap_or(0);
            let existing = self.balances.get(&lock_hash).cloned();

            let updated = match existing {
                Some(mut balance) => {
                    balance.balance = checked_add_i128(
                        balance.balance,
                        balance_delta,
                        "address balance",
                        &lock_hash,
                        tx,
                    )?;
                    balance.used_capacity = checked_add_i128(
                        balance.used_capacity,
                        used_delta,
                        "address used capacity",
                        &lock_hash,
                        tx,
                    )?;
                    balance.live_cells_count = checked_add_i32(
                        balance.live_cells_count,
                        cells_created - cells_consumed,
                        "address live_cells_count",
                        &lock_hash,
                        tx,
                    )?;
                    balance.total_cells_count = checked_add_i64(
                        balance.total_cells_count,
                        i64::from(cells_created),
                        "address total_cells_count",
                        &lock_hash,
                        tx,
                    )?;
                    balance.txs_count =
                        checked_add_i64(balance.txs_count, 1, "address txs_count", &lock_hash, tx)?;
                    balance.last_activity_block = tx.block_number;
                    balance.last_activity_tx = tx.tx_hash.to_vec();
                    balance
                }
                None => {
                    if balance_delta < 0 || used_delta < 0 || cells_consumed > 0 {
                        bail!(
                            "address reducer underflow for unseen address: lock_hash=0x{}, balance_delta={}, used_delta={}, cells_created={}, cells_consumed={}, block={}, tx=0x{}, tx_index={}",
                            hex::encode(&lock_hash),
                            balance_delta,
                            used_delta,
                            cells_created,
                            cells_consumed,
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index
                        );
                    }

                    AddressBalance {
                        balance: balance_delta,
                        used_capacity: used_delta,
                        live_cells_count: cells_created,
                        total_cells_count: i64::from(cells_created),
                        txs_count: 1,
                        first_seen_block: tx.block_number,
                        first_seen_tx: tx.tx_hash.to_vec(),
                        last_activity_block: tx.block_number,
                        last_activity_tx: tx.tx_hash.to_vec(),
                    }
                }
            };

            self.balances.insert(lock_hash, updated);
        }

        Ok(())
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        let mut lock_hashes: Vec<&Vec<u8>> = self.balances.keys().collect();
        lock_hashes.sort();

        let rows = lock_hashes
            .into_iter()
            .map(|lock_hash| {
                let balance = self
                    .balances
                    .get(lock_hash)
                    .expect("sorted lock hash must exist in address owner");
                Ok(MaterializedRow::new(
                    CF_ADDR_BALANCE,
                    lock_hash.clone(),
                    bincode::serialize(balance)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        materializer.materialize_final_snapshot(&rows)
    }
}

fn apply_input_deltas(
    input: &ResolvedInputFacts,
    ctx: &ReducerContext<'_>,
    tx_balance_changes: &mut HashMap<Vec<u8>, i128>,
    tx_cells_consumed: &mut HashMap<Vec<u8>, i32>,
    tx_used_capacity_changes: &mut HashMap<Vec<u8>, i128>,
) -> Result<()> {
    let lock_hash = ctx.resolve_identity(input.lock_script_hash_id).to_vec();
    *tx_balance_changes.entry(lock_hash.clone()).or_default() -= i128::from(input.capacity);
    *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
    *tx_used_capacity_changes.entry(lock_hash).or_default() -= i128::from(input.occupied_capacity);
    Ok(())
}

fn apply_output_deltas(
    cell: &CellFacts,
    ctx: &ReducerContext<'_>,
    tx_balance_changes: &mut HashMap<Vec<u8>, i128>,
    tx_cells_created: &mut HashMap<Vec<u8>, i32>,
    tx_used_capacity_changes: &mut HashMap<Vec<u8>, i128>,
) -> Result<()> {
    let lock_hash = ctx.resolve_identity(cell.lock_script_hash_id).to_vec();
    *tx_balance_changes.entry(lock_hash.clone()).or_default() += i128::from(cell.capacity);
    *tx_cells_created.entry(lock_hash.clone()).or_default() += 1;
    *tx_used_capacity_changes.entry(lock_hash).or_default() += i128::from(cell.occupied_capacity);
    Ok(())
}

fn checked_add_i128(
    current: i128,
    delta: i128,
    metric: &str,
    lock_hash: &[u8],
    tx: &ResolvedTxFacts,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: lock_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: lock_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    Ok(next)
}

fn checked_add_i64(
    current: i64,
    delta: i64,
    metric: &str,
    lock_hash: &[u8],
    tx: &ResolvedTxFacts,
) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: lock_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: lock_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    Ok(next)
}

fn checked_add_i32(
    current: i32,
    delta: i32,
    metric: &str,
    lock_hash: &[u8],
    tx: &ResolvedTxFacts,
) -> Result<i32> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: lock_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: lock_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    Ok(next)
}

#[doc(hidden)]
pub(crate) fn materialize_address_balances_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<HashMap<Vec<u8>, AddressBalance>> {
    let mut interner = IdentityInterner::default();
    let arena = build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let ctx = ReducerContext::new(&interner);
    let mut owner = AddressOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = unique_temp_test_dir("bulk-build-address-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let balances = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();

        let iter =
            domain_store.iterator_cf(domain_store.cf_addr_balance(), rocksdb::IteratorMode::Start);
        let mut snapshot = HashMap::new();
        for item in iter {
            let (key, value) = item?;
            let balance: AddressBalance = bincode::deserialize(&value)?;
            snapshot.insert(key.to_vec(), balance);
        }
        snapshot
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(balances)
}

fn unique_temp_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ckbadger-{}-{}-{}",
        prefix,
        std::process::id(),
        nanos
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{
        CellFacts, CellSemanticTag, OutPointKey, ResolvedInputFacts,
    };
    use crate::sync::types::InternId;

    #[test]
    fn address_owner_reduces_same_block_create_then_consume() {
        let mut interner = IdentityInterner::default();
        let lock_a = interner.intern_bytes(vec![0xaa; 32]);
        let lock_b = interner.intern_bytes(vec![0xbb; 32]);
        let ctx = ReducerContext::new(&interner);
        let tx0 = ResolvedTxFacts {
            tx_hash: [0x11; 32],
            block_number: 100,
            tx_index: 0,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x11; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(2),
                lock_hash_type: 1,
                lock_args_id: InternId::new(3),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Plain,
            }],
        };
        let tx1 = ResolvedTxFacts {
            tx_hash: [0x22; 32],
            block_number: 100,
            tx_index: 1,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x11; 32], 0),
                created_at_block: 100,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                data_size: 0,
                udt_amount: None,
                lock_script_hash_id: lock_a,
                lock_code_hash_id: InternId::new(2),
                lock_hash_type: 1,
                lock_args_id: InternId::new(3),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                semantic_tag: CellSemanticTag::Plain,
            }],
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x22; 32], 0),
                    created_at_block: 100,
                    capacity: 100_00000000,
                    lock_script_hash_id: lock_a,
                    lock_code_hash_id: InternId::new(2),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(3),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x22; 32], 1),
                    created_at_block: 100,
                    capacity: 100_00000000,
                    lock_script_hash_id: lock_b,
                    lock_code_hash_id: InternId::new(4),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(5),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                },
            ],
        };

        let mut owner = AddressOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply tx0");
        owner.apply_tx(&tx1, &ctx).expect("apply tx1");

        let balance_a = owner.balances().get(&vec![0xaa; 32]).expect("lock A");
        assert_eq!(balance_a.balance, 100_00000000);
        assert_eq!(balance_a.used_capacity, 61_00000000);
        assert_eq!(balance_a.live_cells_count, 1);
        assert_eq!(balance_a.total_cells_count, 2);
        assert_eq!(balance_a.txs_count, 2);

        let balance_b = owner.balances().get(&vec![0xbb; 32]).expect("lock B");
        assert_eq!(balance_b.balance, 100_00000000);
        assert_eq!(balance_b.used_capacity, 61_00000000);
        assert_eq!(balance_b.live_cells_count, 1);
        assert_eq!(balance_b.total_cells_count, 1);
        assert_eq!(balance_b.txs_count, 1);
    }
}
