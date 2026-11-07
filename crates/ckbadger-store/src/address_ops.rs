//! Address balance operations.

use crate::store::CkbadgerStore;
use crate::types::{AddrPrefixStats, AddrTxValue, AddressBalance, LockScriptEntry};

use crate::bytes_to_hex;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Decode an `addr_prefix_stats` value, naming the prefix on failure.
fn decode_addr_prefix_stats(prefix: &[u8], value: &[u8]) -> anyhow::Result<AddrPrefixStats> {
    bincode::deserialize(value).map_err(|e| {
        anyhow::anyhow!(
            "failed to deserialize AddrPrefixStats: prefix=0x{}, error={}",
            bytes_to_hex(prefix),
            e
        )
    })
}

impl CkbadgerStore {
    pub fn get_addr_balance(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        match self.get_cf(self.cf_addr_balance(), lock_hash)? {
            Some(value) => Ok(Some(crate::types::decode_address_balance(
                lock_hash, &value,
            )?)),
            None => Ok(None),
        }
    }

    pub fn put_addr_balance_direct(
        &self,
        lock_hash: &[u8],
        balance: &AddressBalance,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(balance)?;
        self.put_cf(self.cf_addr_balance(), lock_hash, &value)
    }

    /// List transactions for an address (newest first), each with its hash.
    ///
    /// [`Self::list_addr_tx_positions_recent`]'s page with the transaction
    /// hashes resolved from the positions through `tx_index`, one `multi_get`
    /// for the page.
    #[allow(clippy::type_complexity)]
    pub fn list_addr_txs_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
        let rows = self.list_addr_tx_positions_recent(lock_hash, limit, cursor)?;
        self.with_addr_tx_hashes("addr_txs", rows)
    }

    /// The positions of an address's transactions (newest first).
    ///
    /// An address participates in a transaction in one of two ways: it held a
    /// cell (`CF_ADDR_TXS`, keyed by the full lock hash) or a protocol named it
    /// by its 20-byte lock-hash prefix (`CF_ADDR_TXS_BY_PREFIX`). Each CF has
    /// one row per (party, position) by construction (the key is the
    /// position), and the builder's merge pass puts a (party, tx) pair in
    /// exactly one of the two, so this is a plain descending merge of two
    /// already-descending scans — a position present in both is an upstream
    /// invariant violation, not a duplicate to dedupe away.
    pub fn list_addr_tx_positions_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, AddrTxValue)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_addr_txs_recent expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let by_lock = self.list_addr_tx_positions_by_lock(lock_hash, limit, cursor)?;
        let by_prefix = self.list_addr_tx_positions_by_prefix(&lock_hash[..20], limit, cursor)?;
        let mut out = Vec::with_capacity(limit);
        let (mut i, mut j) = (0usize, 0usize);
        while out.len() < limit && (i < by_lock.len() || j < by_prefix.len()) {
            let take_lock = match (by_lock.get(i), by_prefix.get(j)) {
                (Some(l), Some(p)) => {
                    let (lp, pp) = ((l.0, l.1), (p.0, p.1));
                    if lp == pp {
                        anyhow::bail!(
                            "address 0x{} has the same tx position in both addr_txs and addr_txs_by_prefix: block={}, tx_idx={} — builder merge invariant violated",
                            bytes_to_hex(lock_hash),
                            lp.0,
                            lp.1
                        );
                    }
                    lp > pp // descending: the higher block comes first
                }
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_lock {
                out.push(by_lock[i].clone());
                i += 1;
            } else {
                out.push(by_prefix[j].clone());
                j += 1;
            }
        }
        Ok(out)
    }

    /// `rows` with each position's transaction hash, resolved with one
    /// `tx_index` read for the page.
    #[allow(clippy::type_complexity)]
    fn with_addr_tx_hashes(
        &self,
        row_kind: &str,
        rows: Vec<(i64, i32, AddrTxValue)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
        let positions: Vec<(i64, i32)> = rows.iter().map(|(b, t, _)| (*b, *t)).collect();
        let hashes = self.tx_hashes_at(row_kind, &positions)?;
        Ok(rows
            .into_iter()
            .zip(hashes)
            .map(|((block, tx_idx, value), hash)| (block, tx_idx, hash.to_vec(), value))
            .collect())
    }

    /// One `addr_txs` / `addr_txs_by_prefix` value, naming its row on failure.
    fn decode_addr_tx_value(
        cf: &str,
        party: &[u8],
        block_num: i64,
        tx_idx: i32,
        value: &[u8],
    ) -> anyhow::Result<AddrTxValue> {
        if value.is_empty() {
            anyhow::bail!(
                "empty AddrTxValue in {cf} for party=0x{}, block={}, tx_idx={} — re-sync required",
                bytes_to_hex(party),
                block_num,
                tx_idx,
            );
        }
        bincode::deserialize(value).map_err(|e| {
            anyhow::anyhow!(
                "failed to deserialize AddrTxValue in {cf}: party=0x{}, block={}, tx_idx={}, error={}",
                bytes_to_hex(party),
                block_num,
                tx_idx,
                e
            )
        })
    }

    /// The cell-participation half of [`Self::list_addr_tx_positions_recent`].
    fn list_addr_tx_positions_by_lock(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, AddrTxValue)>> {
        // Descending position keys allow a simple forward prefix scan.
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                crate::keys::encode_addr_tx_seek_after_key(lock_hash, block_num, tx_idx)
            }
            None => lock_hash.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_addr_txs(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate addr_txs in list_addr_txs_recent: {}", e)
            })?;
            if !key.starts_with(lock_hash) {
                break;
            }
            let (_, block_num, tx_idx) = crate::keys::decode_addr_tx_key(&key)?;
            let addr_tx_value =
                Self::decode_addr_tx_value("addr_txs", lock_hash, block_num, tx_idx, &value)?;
            results.push((block_num, tx_idx, addr_tx_value));
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    /// The position `(block_num, tx_idx)` of the newest transaction at or
    /// below `block_num` in which `lock_hash` held a cell.
    ///
    /// Reads `CF_ADDR_TXS` only: a protocol NAMING the address by its 20-byte
    /// prefix (`CF_ADDR_TXS_BY_PREFIX`) is not cell activity, and
    /// `addr_balance.last_activity` — which both forward paths set from cell
    /// participation alone — must never be repaired onto one. One seek.
    pub fn latest_cell_addr_tx_at_or_below(
        &self,
        lock_hash: &[u8],
        block_num: i64,
    ) -> anyhow::Result<Option<(i64, i32)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "latest_cell_addr_tx_at_or_below expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if block_num < 0 {
            anyhow::bail!(
                "latest_cell_addr_tx_at_or_below expects a non-negative block, got {}: lock_hash=0x{}",
                block_num,
                bytes_to_hex(lock_hash)
            );
        }
        let seek_key = crate::keys::encode_addr_tx_block_seek_key(lock_hash, block_num);
        let mut iter = self.iterator_cf(
            self.cf_addr_txs(),
            rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Forward),
        );
        let Some(item) = iter.next() else {
            return Ok(None);
        };
        let (key, _) = item.map_err(|e| {
            anyhow::anyhow!(
                "failed to iterate addr_txs in latest_cell_addr_tx_at_or_below: lock_hash=0x{}, block={}, error={}",
                bytes_to_hex(lock_hash),
                block_num,
                e
            )
        })?;
        if !key.starts_with(lock_hash) {
            return Ok(None);
        }
        let (_, row_block, tx_idx) = crate::keys::decode_addr_tx_key(&key)?;
        if row_block > block_num {
            anyhow::bail!(
                "addr_txs seek for block <= {} landed on block {}: lock_hash=0x{} — descending key order violated",
                block_num,
                row_block,
                bytes_to_hex(lock_hash)
            );
        }
        Ok(Some((row_block, tx_idx)))
    }

    /// The transactions that named `prefix` (newest first), each with its
    /// hash resolved for the page with one `tx_index` read.
    #[allow(clippy::type_complexity)]
    pub fn list_addr_txs_by_prefix_recent(
        &self,
        prefix: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
        let rows = self.list_addr_tx_positions_by_prefix(prefix, limit, cursor)?;
        self.with_addr_tx_hashes("addr_txs_by_prefix", rows)
    }

    /// The protocol-named half of [`Self::list_addr_tx_positions_recent`].
    fn list_addr_tx_positions_by_prefix(
        &self,
        prefix: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, AddrTxValue)>> {
        if prefix.len() != 20 {
            anyhow::bail!(
                "list_addr_txs_by_prefix_recent expects a 20-byte lock hash prefix, got {} bytes",
                prefix.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                crate::keys::encode_addr_tx_by_prefix_seek_after_key(prefix, block_num, tx_idx)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_addr_txs_by_prefix(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate addr_txs_by_prefix in list_addr_txs_by_prefix_recent: {}",
                    e
                )
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            let (_, block_num, tx_idx) = crate::keys::decode_addr_tx_by_prefix_key(&key)?;
            let addr_tx_value = Self::decode_addr_tx_value(
                "addr_txs_by_prefix",
                prefix,
                block_num,
                tx_idx,
                &value,
            )?;
            results.push((block_num, tx_idx, addr_tx_value));
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn get_addr_prefix_stats(&self, prefix: &[u8]) -> anyhow::Result<Option<AddrPrefixStats>> {
        if prefix.len() != 20 {
            anyhow::bail!(
                "get_addr_prefix_stats expects a 20-byte lock hash prefix, got {} bytes",
                prefix.len()
            );
        }
        self.get_cf(self.cf_addr_prefix_stats(), prefix)?
            .map(|value| decode_addr_prefix_stats(prefix, &value))
            .transpose()
    }

    /// Stage a live batch's standalone protocol-named participations, counted
    /// per block and 20-byte prefix, into `batch` block by block: each
    /// touched prefix's row is read once through the batch (its block overlay
    /// on a live batch, one `multi_get`) and written in the scope of every
    /// block that adds to it, with its value at the end of that block, so the
    /// block journal restores it at any block of the batch.
    pub fn stage_addr_prefix_tx_counts(
        &self,
        counts_by_block: &BTreeMap<i64, BTreeMap<[u8; 20], i64>>,
        batch: &mut crate::batch::StoreBatch,
    ) -> anyhow::Result<()> {
        let prefixes: Vec<[u8; 20]> = counts_by_block
            .values()
            .flat_map(|counts| counts.keys().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let row_keys: Vec<Vec<u8>> = prefixes.iter().map(|prefix| prefix.to_vec()).collect();
        let stored = batch.get_current_many(crate::store::CF_ADDR_PREFIX_STATS, &row_keys)?;
        let mut current: HashMap<[u8; 20], Option<AddrPrefixStats>> =
            HashMap::with_capacity(prefixes.len());
        for (prefix, value) in prefixes.into_iter().zip(stored) {
            let stats = value
                .map(|bytes| decode_addr_prefix_stats(&prefix, &bytes))
                .transpose()?;
            current.insert(prefix, stats);
        }
        for (block, counts) in counts_by_block {
            let mut scope = batch.at(*block);
            for (prefix, delta) in counts {
                let previous = current.get_mut(prefix).ok_or_else(|| {
                    anyhow::anyhow!(
                        "addr_prefix_stats row not read before staging: prefix=0x{} block={block}",
                        bytes_to_hex(prefix)
                    )
                })?;
                let txs_count = match previous {
                    Some(stats) => stats.txs_count.checked_add(*delta),
                    None => Some(*delta),
                }
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "addr_prefix_stats txs_count overflow: prefix=0x{} block={block} delta={delta}",
                        bytes_to_hex(prefix)
                    )
                })?;
                if txs_count <= 0 {
                    anyhow::bail!(
                        "addr_prefix_stats txs_count must stay positive: prefix=0x{} block={block} previous={previous:?} delta={delta}",
                        bytes_to_hex(prefix)
                    );
                }
                let next = AddrPrefixStats { txs_count };
                scope.put_addr_prefix_stats(prefix, &next);
                *previous = Some(next);
            }
        }
        Ok(())
    }

    /// How many transactions an address took part in.
    ///
    /// Cell participations are counted by `addr_balance.txs_count`; participations
    /// a protocol named without the address holding a cell are counted by
    /// `addr_prefix_stats`. This is the ONE place the two are added — every
    /// consumer calls it rather than re-deriving the sum.
    ///
    /// `unwrap_or(0)` here means "no row yet", the honest zero for an address the
    /// index has never seen; a row that exists but is malformed fails in
    /// `bincode::deserialize` above rather than silently reading as zero.
    pub fn address_tx_count(&self, lock_hash: &[u8; 32]) -> anyhow::Result<i64> {
        let cell_part = self
            .get_addr_balance(lock_hash)?
            .map(|b| b.txs_count)
            .unwrap_or(0);
        let named_part = self
            .get_addr_prefix_stats(&lock_hash[..20])?
            .map(|s| s.txs_count)
            .unwrap_or(0);
        cell_part.checked_add(named_part).ok_or_else(|| {
            anyhow::anyhow!(
                "address_tx_count overflow: lock_hash=0x{}, cell_part={}, named_part={}",
                bytes_to_hex(lock_hash),
                cell_part,
                named_part
            )
        })
    }

    /// How many activities an address has: the rows `list_activities` pages
    /// through for it, unfiltered.
    ///
    /// Those rows are its `addr_txs` and `addr_txs_by_prefix` rows
    /// ([`Self::address_tx_count`]) except the cellbase ones, which have no
    /// `CF_TX_ACTIONS` row. Cellbase rows are only ever cell participations
    /// (a cellbase names no protocol party), and `AddressBalance` counts them
    /// in `cellbase_txs_count` — so this is one subtraction, never a scan.
    /// The ONE definition of the address activity total; the API serves it.
    pub fn address_activity_count(&self, lock_hash: &[u8; 32]) -> anyhow::Result<i64> {
        let transactions = self.address_tx_count(lock_hash)?;
        let Some(balance) = self.get_addr_balance(lock_hash)? else {
            // No cell participation at all: every transaction this address
            // took part in was protocol-named, and none of those is a cellbase.
            return Ok(transactions);
        };
        if balance.cellbase_txs_count < 0 || balance.cellbase_txs_count > balance.txs_count {
            anyhow::bail!(
                "addr_balance cellbase_txs_count outside 0..=txs_count: lock_hash=0x{}, cellbase_txs_count={}, txs_count={}",
                bytes_to_hex(lock_hash),
                balance.cellbase_txs_count,
                balance.txs_count
            );
        }
        transactions
            .checked_sub(balance.cellbase_txs_count)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "address_activity_count underflow: lock_hash=0x{}, transactions={}, cellbase_txs_count={}",
                    bytes_to_hex(lock_hash),
                    transactions,
                    balance.cellbase_txs_count
                )
            })
    }

    /// Resolve a 20-byte lock-hash prefix to the full lock hash and its script.
    ///
    /// `CF_LOCK_SCRIPTS` holds every lock ever seen and is never deleted, so a
    /// prefix seek over it is the complete answer: 0 hits → unresolved, 1 hit →
    /// the party, 2+ → an error naming both hashes (a 2^-160 event; guessing
    /// which one is meant would be inventing data).
    pub fn resolve_lock_hash_prefix(
        &self,
        prefix: &[u8; 20],
    ) -> anyhow::Result<Option<([u8; 32], LockScriptEntry)>> {
        let mut hits: Vec<([u8; 32], LockScriptEntry)> = Vec::with_capacity(2);
        for item in self.prefix_iterator_cf(self.cf_lock_scripts(), prefix) {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate lock_scripts for prefix 0x{}: {}",
                    bytes_to_hex(prefix),
                    e
                )
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() != 32 {
                anyhow::bail!(
                    "lock_scripts key is not 32 bytes: len={}, key=0x{}",
                    key.len(),
                    bytes_to_hex(&key)
                );
            }
            let entry: LockScriptEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize LockScriptEntry: lock_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            let hash: [u8; 32] = key[..]
                .try_into()
                .expect("lock_scripts key length checked above");
            hits.push((hash, entry));
            if hits.len() == 2 {
                anyhow::bail!(
                    "ambiguous lock hash prefix 0x{}: 0x{} and 0x{} both match",
                    bytes_to_hex(prefix),
                    bytes_to_hex(&hits[0].0),
                    bytes_to_hex(&hits[1].0)
                );
            }
        }
        Ok(hits.pop())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::tempdir;

    #[test]
    fn test_list_addr_txs_recent_rejects_non_32_byte_lock_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let err = store
            .list_addr_txs_recent(&[0xAA; 31], 10, None)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("list_addr_txs_recent expects 32-byte lock_hash"));
    }

    /// `tx_hash` indexed at `(block, tx_idx)`: what an `addr_txs` position
    /// resolves to.
    fn index_tx(batch: &mut StoreBatch, block: i64, tx_idx: i32, tx_hash: [u8; 32]) {
        batch.put_tx_index(
            block,
            tx_idx,
            &crate::types::TxIndexEntry {
                tx_hash,
                is_cellbase: false,
                timestamp: 0,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 0,
                cycles: None,
                semantic_tags: 0,
            },
        );
    }

    #[test]
    fn test_latest_cell_addr_tx_at_or_below_reads_cell_rows_only() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAA; 32];
        let next_lock = [0xAB; 32];
        let val = AddrTxValue::new(0, false, true, 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 1, 2, &val);
        batch.put_addr_tx(&lock, 5, 0, &val);
        // Named-only participation: never cell activity.
        batch.put_addr_tx_by_prefix(&lock[..20], 3, 0, &val);
        // The next lock's rows sit right after this lock's in key order.
        batch.put_addr_tx(&next_lock, 9, 0, &val);
        batch.commit().unwrap();

        let at = |block| store.latest_cell_addr_tx_at_or_below(&lock, block).unwrap();
        assert_eq!(at(4), Some((1, 2)));
        assert_eq!(at(3), Some((1, 2)));
        assert_eq!(at(5), Some((5, 0)));
        assert_eq!(at(100), Some((5, 0)));
        assert_eq!(at(0), None, "must not cross into the next lock's rows");
        assert_eq!(
            store
                .latest_cell_addr_tx_at_or_below(&[0xCC; 32], 100)
                .unwrap(),
            None
        );

        let err = store
            .latest_cell_addr_tx_at_or_below(&lock, -1)
            .unwrap_err();
        assert!(err.to_string().contains("non-negative block"));
        let err = store
            .latest_cell_addr_tx_at_or_below(&[0xAA; 31], 1)
            .unwrap_err();
        assert!(err.to_string().contains("expects 32-byte lock_hash"));
    }

    #[test]
    fn test_list_addr_txs_recent_limit_zero_returns_empty() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 0, &AddrTxValue::new(0, false, true, 0));
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 0, None).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn list_addr_txs_recent_resolves_a_page_of_hashes_with_one_tx_index_read() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAC; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 1, &AddrTxValue::new(0, false, true, 0));
        batch.put_addr_tx(&lock, 99, 0, &AddrTxValue::new(0, false, true, 0));
        index_tx(&mut batch, 100, 1, [0x10; 32]);
        index_tx(&mut batch, 99, 0, [0x20; 32]);
        batch.commit().unwrap();

        store.reset_read_call_counters();
        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].0, rows[0].1), (100, 1));
        assert_eq!(rows[0].2, vec![0x10; 32]);
        assert_eq!((rows[1].0, rows[1].1), (99, 0));
        assert_eq!(rows[1].2, vec![0x20; 32]);
        assert_eq!(store.read_call_counts(), (0, 1), "(get_cf, multi_get_cf)");
    }

    #[test]
    fn list_addr_txs_recent_fails_on_a_position_without_a_transaction() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAC; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 1, &AddrTxValue::new(0, false, true, 0));
        batch.commit().unwrap();

        let err = store
            .list_addr_txs_recent(&lock, 10, None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("addr_txs points at block=100, tx_idx=1, which has no tx_index row"),
            "{err}"
        );
    }

    /// The key is the position, so an address has at most one row per
    /// transaction position: a second write at the position replaces the
    /// first (the journal rollback leaves no ghost row to keep beside it).
    #[test]
    fn an_addr_txs_position_holds_one_row() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 1, &AddrTxValue::new(1, false, true, 0));
        batch.commit().unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 1, &AddrTxValue::new(2, false, true, 0));
        batch.put_addr_tx(&lock, 99, 0, &AddrTxValue::new(3, false, true, 0));
        batch.commit().unwrap();

        let rows = store
            .list_addr_tx_positions_recent(&lock, 10, None)
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (100, 1, AddrTxValue::new(2, false, true, 0)),
                (99, 0, AddrTxValue::new(3, false, true, 0)),
            ]
        );
        let next = store
            .list_addr_tx_positions_recent(&lock, 10, Some((100, 1)))
            .unwrap();
        assert_eq!(next, vec![(99, 0, AddrTxValue::new(3, false, true, 0))]);
    }

    #[test]
    fn addr_txs_rows_of_the_old_76_byte_layout_are_refused() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAB; 32];
        let mut old_key = crate::keys::encode_addr_tx_key(&lock, 100, 1);
        old_key.extend_from_slice(&[0x10; 32]);
        store
            .put_cf(
                store.cf_addr_txs(),
                &old_key,
                &bincode::serialize(&AddrTxValue::new(0, false, true, 0)).unwrap(),
            )
            .unwrap();
        let err = store
            .list_addr_tx_positions_recent(&lock, 10, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("re-sync required"), "{err}");
    }

    #[test]
    fn test_list_addr_txs_recent_returns_addr_tx_value() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xBB; 32];
        let val_sent = AddrTxValue::new(-500, true, false, 0);
        let val_recv = AddrTxValue::new(1000, false, true, 0);
        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 200, 0, &val_sent);
        batch.put_addr_tx(&lock, 100, 0, &val_recv);
        index_tx(&mut batch, 200, 0, [0xAA; 32]);
        index_tx(&mut batch, 100, 0, [0xBB; 32]);
        batch.commit().unwrap();
        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].3, val_sent);
        assert_eq!(rows[1].3, val_recv);
    }
}

#[cfg(test)]
mod prefix_participation_tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::types::{AddrPrefixStats, LockScriptEntry, TAG_IDENTITY};
    use tempfile::tempdir;

    /// A live batch's per-prefix counts land block by block: two blocks of
    /// one batch adding to one prefix thread through one read, each block
    /// writes the row in its own scope (the journal holds the value at the
    /// end of the previous block for each), and a later batch adds on top.
    #[test]
    fn stage_addr_prefix_tx_counts_writes_each_block_in_its_scope() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let (a, b) = ([0x0au8; 20], [0x0bu8; 20]);
        let mut seed = StoreBatch::new(&store);
        seed.put_addr_prefix_stats(&a, &AddrPrefixStats { txs_count: 3 });
        seed.commit().unwrap();

        let counts = BTreeMap::from([
            (40, BTreeMap::from([(a, 1)])),
            (41, BTreeMap::from([(a, 2), (b, 1)])),
        ]);
        let overlay = crate::block_overlay::SharedBlockOverlay::new(&store).unwrap();
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        store
            .stage_addr_prefix_tx_counts(&counts, &mut batch)
            .unwrap();
        let mut counters = HashMap::new();
        batch
            .stage_journal(&mut |block| crate::block_overlay::journal_seq(&mut counters, block))
            .unwrap();
        batch.commit().unwrap();

        let stats = |n| Some(bincode::serialize(&AddrPrefixStats { txs_count: n }).unwrap());
        let mut journal: Vec<(i64, Vec<u8>, Option<Vec<u8>>)> = store
            .iterator_cf(
                store.cf_reorg_undo_log_by_block(),
                rocksdb::IteratorMode::Start,
            )
            .map(|row| {
                let (key, value) = row.unwrap();
                let (block, _) = crate::keys::decode_reorg_undo_log_key(&key);
                let entry = crate::block_overlay::decode_journal_value(&value).unwrap();
                assert_eq!(entry.cf, crate::store::CF_ADDR_PREFIX_STATS);
                (
                    block,
                    entry.key.to_vec(),
                    entry.previous.map(<[u8]>::to_vec),
                )
            })
            .collect();
        journal.sort();
        assert_eq!(
            journal,
            vec![
                (40, a.to_vec(), stats(3)),
                (41, a.to_vec(), stats(4)),
                (41, b.to_vec(), None),
            ]
        );
        assert_eq!(
            store.get_addr_prefix_stats(&a).unwrap(),
            Some(AddrPrefixStats { txs_count: 6 })
        );
        assert_eq!(
            store.get_addr_prefix_stats(&b).unwrap(),
            Some(AddrPrefixStats { txs_count: 1 })
        );

        let mut next = StoreBatch::new(&store);
        store
            .stage_addr_prefix_tx_counts(
                &BTreeMap::from([(42, BTreeMap::from([(b, 1)]))]),
                &mut next,
            )
            .unwrap();
        next.commit().unwrap();
        assert_eq!(
            store.get_addr_prefix_stats(&b).unwrap(),
            Some(AddrPrefixStats { txs_count: 2 })
        );
    }

    #[test]
    fn list_addr_txs_recent_merges_prefix_rows_in_descending_order() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0x42u8; 32];
        let prefix = &lock[..20];
        let mut b = StoreBatch::new(&store);
        b.put_addr_tx(&lock, 10, 0, &AddrTxValue::new(5, false, true, 0));
        b.put_addr_tx_by_prefix(
            prefix,
            11,
            0,
            &AddrTxValue::new(0, false, false, TAG_IDENTITY),
        );
        b.put_addr_tx(&lock, 12, 1, &AddrTxValue::new(-5, true, false, 0));
        // 别人的前缀
        b.put_addr_tx_by_prefix(&[0x99; 20], 13, 0, &AddrTxValue::new(0, false, false, 0));
        b.commit().unwrap();

        let rows = store
            .list_addr_tx_positions_recent(&lock, 10, None)
            .unwrap();
        let positions: Vec<(i64, i32)> = rows.iter().map(|(b, t, _)| (*b, *t)).collect();
        assert_eq!(positions, vec![(12, 1), (11, 0), (10, 0)]);
        assert_eq!(rows[1].2.tx_type_str(), "named");

        // 游标同时作用于两路
        let page = store
            .list_addr_tx_positions_recent(&lock, 10, Some((12, 1)))
            .unwrap();
        let positions: Vec<(i64, i32)> = page.iter().map(|(b, t, _)| (*b, *t)).collect();
        assert_eq!(positions, vec![(11, 0), (10, 0)]);
        // limit 截断在归并之后
        assert_eq!(
            store
                .list_addr_tx_positions_recent(&lock, 2, None)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn list_addr_txs_recent_rejects_same_position_in_both_indexes() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0x42u8; 32];
        let mut b = StoreBatch::new(&store);
        b.put_addr_tx(&lock, 10, 0, &AddrTxValue::new(5, false, true, 0));
        b.put_addr_tx_by_prefix(&lock[..20], 10, 0, &AddrTxValue::new(0, false, false, 0));
        b.commit().unwrap();
        let err = store
            .list_addr_tx_positions_recent(&lock, 10, None)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("both addr_txs and addr_txs_by_prefix"),
            "{err}"
        );
    }

    #[test]
    fn address_tx_count_sums_lock_and_prefix_participations() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0x42u8; 32];
        let mut b = StoreBatch::new(&store);
        b.put_addr_balance(
            &lock,
            &AddressBalance {
                txs_count: 3,
                ..Default::default()
            },
        );
        b.put_addr_prefix_stats(&lock[..20], &AddrPrefixStats { txs_count: 2 });
        b.commit().unwrap();
        assert_eq!(store.address_tx_count(&lock).unwrap(), 5);
        assert_eq!(store.address_tx_count(&[0x43u8; 32]).unwrap(), 0);
    }

    #[test]
    fn address_activity_count_is_transactions_minus_cellbase_rows() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let miner = [0x42u8; 32];
        let named_only = [0x44u8; 32];
        let corrupt = [0x45u8; 32];
        let mut b = StoreBatch::new(&store);
        // 3 cell participations, 2 of them cellbases, plus 2 protocol-named ones.
        b.put_addr_balance(
            &miner,
            &AddressBalance {
                txs_count: 3,
                cellbase_txs_count: 2,
                ..Default::default()
            },
        );
        b.put_addr_prefix_stats(&miner[..20], &AddrPrefixStats { txs_count: 2 });
        b.put_addr_prefix_stats(&named_only[..20], &AddrPrefixStats { txs_count: 4 });
        b.put_addr_balance(
            &corrupt,
            &AddressBalance {
                txs_count: 1,
                cellbase_txs_count: 2,
                ..Default::default()
            },
        );
        b.commit().unwrap();
        assert_eq!(store.address_activity_count(&miner).unwrap(), 3);
        assert_eq!(store.address_activity_count(&named_only).unwrap(), 4);
        assert_eq!(store.address_activity_count(&[0x43u8; 32]).unwrap(), 0);
        let err = store
            .address_activity_count(&corrupt)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cellbase_txs_count outside 0..=txs_count"),
            "{err}"
        );
    }

    #[test]
    fn resolve_lock_hash_prefix_zero_one_many() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let entry = LockScriptEntry {
            code_hash: vec![0x11; 32],
            hash_type: 1,
            args: vec![0x22; 20],
        };
        let a = {
            let mut h = [0x42u8; 32];
            h[31] = 1;
            h
        };
        let b2 = {
            let mut h = [0x42u8; 32];
            h[31] = 2;
            h
        };
        let mut b = StoreBatch::new(&store);
        b.put_lock_script(&a, &entry);
        b.commit().unwrap();
        assert!(store
            .resolve_lock_hash_prefix(&[0x43u8; 20])
            .unwrap()
            .is_none());
        let (hash, got) = store
            .resolve_lock_hash_prefix(&a[..20].try_into().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(hash, a);
        assert_eq!(got.args, entry.args);
        let mut b = StoreBatch::new(&store);
        b.put_lock_script(&b2, &entry);
        b.commit().unwrap();
        let err = store
            .resolve_lock_hash_prefix(&a[..20].try_into().unwrap())
            .unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
    }
}
