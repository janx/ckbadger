use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::{AddressBalance, CkbadgerStore, CF_ADDR_BALANCE};
use hashbrown::HashTable;
use rustc_hash::{FxHashMap, FxHasher};

use super::{BulkReducer, ReducerContext};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{CellFacts, ResolvedInputFacts, ResolvedTxFacts};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AddressTxDelta {
    pub(crate) balance_delta: i128,
    pub(crate) cells_created: i32,
    pub(crate) cells_consumed: i32,
    pub(crate) used_capacity_delta: i128,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactAddressBalance {
    first_seen_tx: [u8; 32],
    last_activity_tx: [u8; 32],
    pub(crate) balance: u64,
    pub(crate) used_capacity: u64,
    pub(crate) total_cells_count: i64,
    pub(crate) txs_count: i64,
    pub(crate) first_seen_block: i64,
    pub(crate) last_activity_block: i64,
    pub(crate) live_cells_count: i32,
}

impl CompactAddressBalance {
    fn to_stored(self) -> AddressBalance {
        AddressBalance {
            balance: i128::from(self.balance),
            used_capacity: i128::from(self.used_capacity),
            live_cells_count: self.live_cells_count,
            total_cells_count: self.total_cells_count,
            txs_count: self.txs_count,
            first_seen_block: self.first_seen_block,
            first_seen_tx: self.first_seen_tx.to_vec(),
            last_activity_block: self.last_activity_block,
            last_activity_tx: self.last_activity_tx.to_vec(),
        }
    }

    #[cfg(test)]
    fn for_test(seed: u64) -> Self {
        Self {
            first_seen_tx: [seed as u8; 32],
            last_activity_tx: [seed as u8; 32],
            balance: seed,
            used_capacity: seed,
            total_cells_count: seed as i64,
            txs_count: 1,
            first_seen_block: seed as i64,
            last_activity_block: seed as i64,
            live_cells_count: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddressId(u32);

impl AddressId {
    fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddressEntry {
    lock_hash: [u8; 32],
    balance: CompactAddressBalance,
}

const ADDRESS_ENTRY_CHUNK_LEN: usize = 64 * 1024;

#[derive(Debug, Default)]
struct AddressEntries {
    chunks: Vec<Vec<AddressEntry>>,
    len: usize,
}

impl AddressEntries {
    fn len(&self) -> usize {
        self.len
    }

    fn get(&self, id: AddressId) -> Option<&AddressEntry> {
        let index = id.as_usize();
        self.chunks
            .get(index / ADDRESS_ENTRY_CHUNK_LEN)
            .and_then(|chunk| chunk.get(index % ADDRESS_ENTRY_CHUNK_LEN))
    }

    fn get_mut(&mut self, id: AddressId) -> Option<&mut AddressEntry> {
        let index = id.as_usize();
        self.chunks
            .get_mut(index / ADDRESS_ENTRY_CHUNK_LEN)
            .and_then(|chunk| chunk.get_mut(index % ADDRESS_ENTRY_CHUNK_LEN))
    }

    fn push(&mut self, entry: AddressEntry) -> Result<AddressId> {
        let raw_id = u32::try_from(self.len).map_err(|_| {
            anyhow!(
                "bulk address ID space exhausted: address_count={} max={}",
                self.len,
                u32::MAX
            )
        })?;
        let needs_chunk = self
            .chunks
            .last()
            .map(|chunk| chunk.len() == ADDRESS_ENTRY_CHUNK_LEN)
            .unwrap_or(true);
        if needs_chunk {
            self.chunks.try_reserve(1).map_err(|e| {
                anyhow!(
                    "failed to reserve bulk address chunk directory: address_count={} chunks={} error={}",
                    self.len,
                    self.chunks.len(),
                    e
                )
            })?;
            let mut chunk = Vec::new();
            chunk.try_reserve_exact(ADDRESS_ENTRY_CHUNK_LEN).map_err(|e| {
                anyhow!(
                    "failed to reserve bulk address entry chunk: address_count={} chunk_entries={} entry_bytes={} error={}",
                    self.len,
                    ADDRESS_ENTRY_CHUNK_LEN,
                    std::mem::size_of::<AddressEntry>(),
                    e
                )
            })?;
            self.chunks.push(chunk);
        }
        self.chunks
            .last_mut()
            .expect("address entry chunk exists after allocation")
            .push(entry);
        self.len = self
            .len
            .checked_add(1)
            .ok_or_else(|| anyhow!("bulk address count overflow after ID allocation"))?;
        Ok(AddressId(raw_id))
    }

    fn iter(&self) -> impl Iterator<Item = &AddressEntry> {
        self.chunks.iter().flat_map(|chunk| chunk.iter())
    }

    fn allocated_bytes(&self) -> Result<u64> {
        let directory_bytes = self
            .chunks
            .capacity()
            .checked_mul(std::mem::size_of::<Vec<AddressEntry>>())
            .ok_or_else(|| anyhow!("bulk address chunk-directory byte count overflow"))?;
        let entry_bytes = self.chunks.iter().try_fold(0usize, |total, chunk| {
            chunk
                .capacity()
                .checked_mul(std::mem::size_of::<AddressEntry>())
                .and_then(|bytes| total.checked_add(bytes))
                .ok_or_else(|| anyhow!("bulk address entry byte count overflow"))
        })?;
        let total_bytes = directory_bytes
            .checked_add(entry_bytes)
            .ok_or_else(|| anyhow!("bulk address allocated byte count overflow"))?;
        u64::try_from(total_bytes).map_err(|_| anyhow!("bulk address allocated bytes exceed u64"))
    }

    #[cfg(test)]
    fn push_for_test(
        &mut self,
        lock_hash: [u8; 32],
        balance: CompactAddressBalance,
    ) -> Result<AddressId> {
        self.push(AddressEntry { lock_hash, balance })
    }

    #[cfg(test)]
    fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

#[derive(Debug, Default)]
pub(crate) struct AddressOwner {
    index: HashTable<AddressId>,
    entries: AddressEntries,
}

impl AddressOwner {
    pub(crate) fn get(&self, lock_hash: &[u8; 32]) -> Option<&CompactAddressBalance> {
        let hash = hash_lock_hash(lock_hash);
        let id = self.index.find(hash, |id| {
            let entry = self.entries.get(*id).unwrap_or_else(|| {
                panic!(
                    "bulk address lookup index points outside entry store: address_id={} address_count={}",
                    id.0,
                    self.entries.len()
                )
            });
            entry.lock_hash == *lock_hash
        })?;
        self.entries.get(*id).map(|entry| &entry.balance)
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        let owned_bytes = self
            .entries
            .allocated_bytes()
            .unwrap_or_else(|e| panic!("bulk address entry memory accounting failed: {e}"));
        let index_bytes = estimated_hash_table_allocation_bytes::<AddressId>(self.index.capacity())
            .unwrap_or_else(|e| panic!("bulk address index memory accounting failed: {e}"));
        (std::mem::size_of::<Self>() as u64)
            .checked_add(owned_bytes)
            .and_then(|bytes| bytes.checked_add(index_bytes))
            .unwrap_or_else(|| {
                panic!(
                    "bulk address total memory accounting overflow: entries_bytes={} index_bytes={}",
                    owned_bytes, index_bytes
                )
            })
    }

    pub(crate) fn apply_tx_with_deltas(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        ctx: &ReducerContext<'_>,
    ) -> Result<FxHashMap<[u8; 32], AddressTxDelta>> {
        let deltas = collect_tx_deltas(tx, ctx)?;
        self.apply_tx_deltas(tx, &deltas)?;
        Ok(deltas)
    }

    fn apply_tx_deltas(
        &mut self,
        tx: &ResolvedTxFacts<'_>,
        deltas: &FxHashMap<[u8; 32], AddressTxDelta>,
    ) -> Result<()> {
        for (lock_hash, delta) in deltas {
            let hash = hash_lock_hash(lock_hash);
            let existing_id = self
                .index
                .find(hash, |id| {
                    let entry = self.entries.get(*id).unwrap_or_else(|| {
                        panic!(
                            "bulk address update index points outside entry store: address_id={} address_count={}",
                            id.0,
                            self.entries.len()
                        )
                    });
                    entry.lock_hash == *lock_hash
                })
                .copied();
            match existing_id {
                Some(id) => {
                    let address_count = self.entries.len();
                    let balance = &mut self
                        .entries
                        .get_mut(id)
                        .ok_or_else(|| {
                            anyhow!(
                                "bulk address index points outside entry store: address_id={} address_count={} block={} tx=0x{}",
                                id.0,
                                address_count,
                                tx.block_number,
                                hex::encode(tx.tx_hash)
                            )
                        })?
                        .balance;
                    balance.balance = checked_add_u64(
                        balance.balance,
                        delta.balance_delta,
                        "address balance",
                        lock_hash,
                        tx,
                    )?;
                    balance.used_capacity = checked_add_u64(
                        balance.used_capacity,
                        delta.used_capacity_delta,
                        "address used capacity",
                        lock_hash,
                        tx,
                    )?;
                    balance.live_cells_count = checked_add_i32(
                        balance.live_cells_count,
                        delta.cells_created - delta.cells_consumed,
                        "address live_cells_count",
                        lock_hash,
                        tx,
                    )?;
                    balance.total_cells_count = checked_add_i64(
                        balance.total_cells_count,
                        i64::from(delta.cells_created),
                        "address total_cells_count",
                        lock_hash,
                        tx,
                    )?;
                    balance.txs_count =
                        checked_add_i64(balance.txs_count, 1, "address txs_count", lock_hash, tx)?;
                    balance.last_activity_block = tx.block_number;
                    balance.last_activity_tx = tx.tx_hash;
                }
                None => {
                    if delta.balance_delta < 0
                        || delta.used_capacity_delta < 0
                        || delta.cells_consumed > 0
                    {
                        bail!(
                            "address reducer underflow for unseen address: lock_hash=0x{}, balance_delta={}, used_delta={}, cells_created={}, cells_consumed={}, block={}, tx=0x{}, tx_index={}",
                            hex::encode(lock_hash),
                            delta.balance_delta,
                            delta.used_capacity_delta,
                            delta.cells_created,
                            delta.cells_consumed,
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index
                        );
                    }

                    let balance = u64::try_from(delta.balance_delta).map_err(|_| {
                        anyhow!(
                            "new address balance exceeds u64: lock_hash=0x{} balance={} block={} tx=0x{}",
                            hex::encode(lock_hash),
                            delta.balance_delta,
                            tx.block_number,
                            hex::encode(tx.tx_hash)
                        )
                    })?;
                    let used_capacity =
                        u64::try_from(delta.used_capacity_delta).map_err(|_| {
                            anyhow!(
                                "new address used capacity exceeds u64: lock_hash=0x{} used_capacity={} block={} tx=0x{}",
                                hex::encode(lock_hash),
                                delta.used_capacity_delta,
                                tx.block_number,
                                hex::encode(tx.tx_hash)
                            )
                        })?;
                    let entries = &self.entries;
                    self.index
                        .try_reserve(1, |id| {
                            let entry = entries.get(*id).unwrap_or_else(|| {
                                panic!(
                                    "bulk address index rehash ID outside entry store: address_id={} address_count={}",
                                    id.0,
                                    entries.len()
                                )
                            });
                            hash_lock_hash(&entry.lock_hash)
                        })
                        .map_err(|e| {
                            anyhow!(
                                "failed to grow bulk address ID index: address_count={} index_capacity={} block={} tx=0x{} error={:?}",
                                self.entries.len(),
                                self.index.capacity(),
                                tx.block_number,
                                hex::encode(tx.tx_hash),
                                e
                            )
                        })?;
                    let id = self.entries.push(AddressEntry {
                        lock_hash: *lock_hash,
                        balance: CompactAddressBalance {
                            balance,
                            used_capacity,
                            live_cells_count: delta.cells_created,
                            total_cells_count: i64::from(delta.cells_created),
                            txs_count: 1,
                            first_seen_block: tx.block_number,
                            first_seen_tx: tx.tx_hash,
                            last_activity_block: tx.block_number,
                            last_activity_tx: tx.tx_hash,
                        },
                    })?;
                    let entries = &self.entries;
                    self.index.insert_unique(hash, id, |existing_id| {
                        let entry = entries.get(*existing_id).unwrap_or_else(|| {
                            panic!(
                                "bulk address index insert ID outside entry store: address_id={} address_count={}",
                                existing_id.0,
                                entries.len()
                            )
                        });
                        hash_lock_hash(&entry.lock_hash)
                    });
                }
            }
        }

        Ok(())
    }
}

fn hash_lock_hash(lock_hash: &[u8; 32]) -> u64 {
    let mut hasher = FxHasher::default();
    lock_hash.hash(&mut hasher);
    hasher.finish()
}

fn estimated_hash_table_allocation_bytes<T>(capacity: usize) -> Result<u64> {
    if capacity == 0 {
        return Ok(0);
    }
    let bucket_count = if capacity < 8 {
        capacity
            .checked_add(1)
            .ok_or_else(|| anyhow!("bulk address hash-table bucket count overflow"))?
    } else {
        capacity
            .checked_mul(8)
            .and_then(|value| value.checked_div(7))
            .ok_or_else(|| anyhow!("bulk address hash-table bucket count overflow"))?
    };
    let payload = bucket_count
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| anyhow!("bulk address hash-table payload byte count overflow"))?;
    // SwissTable stores one control byte per bucket plus one SIMD group cloned at the end.
    // Add alignment slack conservatively so retained-byte accounting never under-reports.
    let allocation = payload
        .checked_add(bucket_count)
        .and_then(|bytes| bytes.checked_add(16))
        .and_then(|bytes| bytes.checked_add(std::mem::align_of::<T>() - 1))
        .ok_or_else(|| anyhow!("bulk address hash-table allocation byte count overflow"))?;
    u64::try_from(allocation)
        .map_err(|_| anyhow!("bulk address hash-table allocation bytes exceed u64"))
}

impl AddressOwner {
    pub(crate) fn emit_snapshot_rows<F>(&self, mut emit: F) -> Result<()>
    where
        F: FnMut(MaterializedRow) -> Result<()>,
    {
        for entry in self.entries.iter() {
            let stored = entry.balance.to_stored();
            emit(MaterializedRow::new(
                CF_ADDR_BALANCE,
                entry.lock_hash.to_vec(),
                bincode::serialize(&stored)?,
            ))?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn build_snapshot_rows(&self) -> Result<Vec<MaterializedRow>> {
        let mut rows = Vec::new();
        self.emit_snapshot_rows(|row| {
            rows.push(row);
            Ok(())
        })?;
        Ok(rows)
    }

    #[cfg(test)]
    pub(crate) fn build_final_rows(&self) -> Result<super::super::materialize::OwnerFinalRows> {
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: Vec::new(),
            snapshot_rows: self.build_snapshot_rows()?,
        })
    }
}

impl BulkReducer for AddressOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        self.apply_tx_with_deltas(tx, ctx).map(|_| ())
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        materializer.materialize_final_snapshot_bounded(|sink| {
            self.emit_snapshot_rows(|row| sink.push(row))
        })
    }
}

fn apply_input_deltas(
    input: &ResolvedInputFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut FxHashMap<[u8; 32], AddressTxDelta>,
) -> Result<()> {
    let lock_hash = resolve_lock_hash(ctx, input.lock_script_hash_id, input.outpoint, "input")?;
    let delta = deltas.entry(lock_hash).or_default();
    delta.balance_delta -= i128::from(input.capacity);
    delta.cells_consumed += 1;
    delta.used_capacity_delta -= i128::from(input.occupied_capacity);
    Ok(())
}

fn apply_output_deltas(
    cell: &CellFacts,
    ctx: &ReducerContext<'_>,
    deltas: &mut FxHashMap<[u8; 32], AddressTxDelta>,
) -> Result<()> {
    let lock_hash = resolve_lock_hash(ctx, cell.lock_script_hash_id, cell.outpoint, "output")?;
    let delta = deltas.entry(lock_hash).or_default();
    delta.balance_delta += i128::from(cell.capacity);
    delta.cells_created += 1;
    delta.used_capacity_delta += i128::from(cell.occupied_capacity);
    Ok(())
}

fn collect_tx_deltas(
    tx: &ResolvedTxFacts<'_>,
    ctx: &ReducerContext<'_>,
) -> Result<FxHashMap<[u8; 32], AddressTxDelta>> {
    let mut deltas = FxHashMap::default();

    for input in &tx.resolved_inputs {
        apply_input_deltas(input, ctx, &mut deltas)?;
    }

    for cell in tx.cells.iter() {
        apply_output_deltas(cell, ctx, &mut deltas)?;
    }

    Ok(deltas)
}

fn resolve_lock_hash(
    ctx: &ReducerContext<'_>,
    id: crate::sync::types::InternId,
    outpoint: crate::sync::bulk_build::facts::OutPointKey,
    direction: &str,
) -> Result<[u8; 32]> {
    let bytes = ctx.resolve_identity(id);
    bytes.try_into().map_err(|_| {
        anyhow!(
            "address reducer lock hash length invariant violated: direction={} outpoint=0x{}:{} intern_id={:?} expected=32 actual={}",
            direction,
            hex::encode(outpoint.tx_hash),
            outpoint.index,
            id,
            bytes.len()
        )
    })
}

fn checked_add_u64(
    current: u64,
    delta: i128,
    metric: &str,
    lock_hash: &[u8],
    tx: &ResolvedTxFacts<'_>,
) -> Result<u64> {
    let current_i128 = i128::from(current);
    let next = current_i128.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: lock_hash=0x{}, current={}, delta={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current_i128,
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
            current_i128,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        );
    }
    u64::try_from(next).map_err(|_| {
        anyhow!(
            "{} exceeds u64: lock_hash=0x{}, current={}, delta={}, next={}, block={}, tx=0x{}, tx_index={}",
            metric,
            hex::encode(lock_hash),
            current,
            delta,
            next,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        )
    })
}

fn checked_add_i64(
    current: i64,
    delta: i64,
    metric: &str,
    lock_hash: &[u8],
    tx: &ResolvedTxFacts<'_>,
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
    tx: &ResolvedTxFacts<'_>,
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
    let interner = IdentityInterner::default();
    let (arena, _) = build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let frozen = interner.snapshot_for_reads();
    let ctx = ReducerContext::new(&frozen);
    let mut owner = AddressOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = super::super::unique_temp_test_dir("bulk-build-address-owner");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{
        CellFacts, CellSemanticTag, OutPointKey, ResolvedInputFacts,
    };
    use crate::sync::types::InternId;

    #[test]
    fn address_index_keeps_only_compact_ids_in_hash_buckets() {
        assert_eq!(
            std::mem::size_of::<AddressId>(),
            4,
            "address hash buckets must contain only a compact ID"
        );
        assert_eq!(
            std::mem::size_of::<AddressEntry>(),
            152,
            "address entry layout changed; re-evaluate bulk retained memory"
        );
    }

    #[test]
    fn address_entries_grow_by_bounded_chunks_without_reallocating_old_entries() {
        let mut entries = AddressEntries::default();
        let first = entries
            .push_for_test([0x11; 32], CompactAddressBalance::for_test(1))
            .expect("push first");
        let first_ptr = entries.get(first).expect("first entry") as *const AddressEntry;

        for seed in 1..ADDRESS_ENTRY_CHUNK_LEN {
            entries
                .push_for_test(
                    [seed as u8; 32],
                    CompactAddressBalance::for_test(seed as u64),
                )
                .expect("fill first address chunk");
        }
        entries
            .push_for_test([0x33; 32], CompactAddressBalance::for_test(3))
            .expect("push into a new chunk");

        assert_eq!(entries.len(), ADDRESS_ENTRY_CHUNK_LEN + 1);
        assert_eq!(entries.chunk_count(), 2);
        assert_eq!(
            entries.get(first).expect("first entry after growth") as *const AddressEntry,
            first_ptr,
            "growing the entry store must not move prior address state"
        );
    }

    #[test]
    fn compact_address_balance_has_no_heap_backed_tx_hash_fields() {
        assert_eq!(
            std::mem::size_of::<CompactAddressBalance>(),
            120,
            "CompactAddressBalance layout changed; re-evaluate bulk retained memory"
        );
        let compact = CompactAddressBalance {
            balance: 1,
            used_capacity: 2,
            live_cells_count: 3,
            total_cells_count: 4,
            txs_count: 5,
            first_seen_block: 6,
            first_seen_tx: [0xAA; 32],
            last_activity_block: 7,
            last_activity_tx: [0xBB; 32],
        };
        let stored = compact.to_stored();
        assert_eq!(stored.first_seen_tx, vec![0xAA; 32]);
        assert_eq!(stored.last_activity_tx, vec![0xBB; 32]);
    }

    #[test]
    fn address_owner_reduces_same_block_create_then_consume() {
        let interner = IdentityInterner::default();
        let lock_a = interner.intern_bytes(vec![0xaa; 32]);
        let lock_b = interner.intern_bytes(vec![0xbb; 32]);
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let tx0 = ResolvedTxFacts {
            tx_hash: [0x11; 32],
            block_number: 100,
            block_hash: [0x01; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 1,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x11; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
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
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };
        let tx1 = ResolvedTxFacts {
            tx_hash: [0x22; 32],
            block_number: 100,
            block_hash: [0x01; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 1,
            tx_index: 1,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x11; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 1,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data_hash: None,
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
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0x22; 32], 0),
                    created_at_block: 100,
                    created_by_block_dao_ar: 1,
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
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0x22; 32], 1),
                    created_at_block: 100,
                    created_by_block_dao_ar: 1,
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
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ]
            .into(),
        };

        let mut owner = AddressOwner::default();
        owner.apply_tx(&tx0, &ctx).expect("apply tx0");
        owner.apply_tx(&tx1, &ctx).expect("apply tx1");

        let balance_a = owner.get(&[0xaa; 32]).expect("lock A");
        assert_eq!(balance_a.balance, 100_00000000);
        assert_eq!(balance_a.used_capacity, 61_00000000);
        assert_eq!(balance_a.live_cells_count, 1);
        assert_eq!(balance_a.total_cells_count, 2);
        assert_eq!(balance_a.txs_count, 2);

        let balance_b = owner.get(&[0xbb; 32]).expect("lock B");
        assert_eq!(balance_b.balance, 100_00000000);
        assert_eq!(balance_b.used_capacity, 61_00000000);
        assert_eq!(balance_b.live_cells_count, 1);
        assert_eq!(balance_b.total_cells_count, 1);
        assert_eq!(balance_b.txs_count, 1);
    }
}
