//! Address balance operations.

use crate::store::CkbadgerStore;
use crate::types::{AddrPrefixStats, AddrTxValue, AddressBalance, LockScriptEntry};

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_addr_balance(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        match self.get_cf(self.cf_addr_balance(), lock_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
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

    /// List transactions for an address (newest first).
    ///
    /// An address participates in a transaction in one of two ways: it held a
    /// cell (`CF_ADDR_TXS`, keyed by the full lock hash) or a protocol named it
    /// by its 20-byte lock-hash prefix (`CF_ADDR_TXS_BY_PREFIX`). The builder's
    /// merge pass guarantees a (party, tx) pair lands in exactly one of the two,
    /// so this is a plain descending merge of two already-descending scans — a
    /// position present in both is an upstream invariant violation, not a
    /// duplicate to dedupe away.
    #[allow(clippy::type_complexity)]
    pub fn list_addr_txs_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_addr_txs_recent expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let by_lock = self.list_addr_txs_by_lock_recent(lock_hash, limit, cursor)?;
        let by_prefix = self.list_addr_txs_by_prefix_recent(&lock_hash[..20], limit, cursor)?;
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

    /// The cell-participation half of [`Self::list_addr_txs_recent`].
    #[allow(clippy::type_complexity)]
    fn list_addr_txs_by_lock_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
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
            if key.len() == crate::keys::ADDR_TX_KEY_SIZE {
                let (_, block_num, tx_idx, tx_hash) = crate::keys::decode_addr_tx_key(&key);
                let addr_tx_value = if value.is_empty() {
                    anyhow::bail!(
                        "empty AddrTxValue for lock_hash=0x{}, block={}, tx_idx={} — re-sync required",
                        bytes_to_hex(lock_hash),
                        block_num,
                        tx_idx,
                    );
                } else {
                    bincode::deserialize(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize AddrTxValue: lock_hash=0x{}, block={}, tx_idx={}, error={}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            e
                        )
                    })?
                };
                results.push((block_num, tx_idx, tx_hash, addr_tx_value));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// The newest transaction at or below `block_num` in which `lock_hash`
    /// held a cell, as `(block_num, tx_idx, tx_hash)`.
    ///
    /// Reads `CF_ADDR_TXS` only: a protocol NAMING the address by its 20-byte
    /// prefix (`CF_ADDR_TXS_BY_PREFIX`) is not cell activity, and
    /// `addr_balance.last_activity` — which both forward paths set from cell
    /// participation alone — must never be repaired onto one. One seek.
    pub fn latest_cell_addr_tx_at_or_below(
        &self,
        lock_hash: &[u8],
        block_num: i64,
    ) -> anyhow::Result<Option<(i64, i32, Vec<u8>)>> {
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
        if key.len() != crate::keys::ADDR_TX_KEY_SIZE {
            anyhow::bail!(
                "addr_txs key is not {} bytes: len={}, lock_hash=0x{}",
                crate::keys::ADDR_TX_KEY_SIZE,
                key.len(),
                bytes_to_hex(lock_hash)
            );
        }
        let (_, row_block, tx_idx, tx_hash) = crate::keys::decode_addr_tx_key(&key);
        if row_block > block_num {
            anyhow::bail!(
                "addr_txs seek for block <= {} landed on block {}: lock_hash=0x{} — descending key order violated",
                block_num,
                row_block,
                bytes_to_hex(lock_hash)
            );
        }
        Ok(Some((row_block, tx_idx, tx_hash)))
    }

    /// The protocol-named half of [`Self::list_addr_txs_recent`].
    #[allow(clippy::type_complexity)]
    pub fn list_addr_txs_by_prefix_recent(
        &self,
        prefix: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
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
            if key.len() != crate::keys::ADDR_TX_BY_PREFIX_KEY_SIZE {
                anyhow::bail!(
                    "addr_txs_by_prefix key is not {} bytes: len={}, prefix=0x{}",
                    crate::keys::ADDR_TX_BY_PREFIX_KEY_SIZE,
                    key.len(),
                    bytes_to_hex(prefix)
                );
            }
            let (_, block_num, tx_idx, tx_hash) = crate::keys::decode_addr_tx_by_prefix_key(&key);
            if value.is_empty() {
                anyhow::bail!(
                    "empty AddrTxValue for lock_hash_prefix=0x{}, block={}, tx_idx={} — re-sync required",
                    bytes_to_hex(prefix),
                    block_num,
                    tx_idx,
                );
            }
            let addr_tx_value: AddrTxValue = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize AddrTxValue: lock_hash_prefix=0x{}, block={}, tx_idx={}, error={}",
                    bytes_to_hex(prefix),
                    block_num,
                    tx_idx,
                    e
                )
            })?;
            results.push((block_num, tx_idx, tx_hash, addr_tx_value));
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
        match self.get_cf(self.cf_addr_prefix_stats(), prefix)? {
            Some(value) => Ok(Some(bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize AddrPrefixStats: prefix=0x{}, error={}",
                    bytes_to_hex(prefix),
                    e
                )
            })?)),
            None => Ok(None),
        }
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

    #[test]
    fn test_latest_cell_addr_tx_at_or_below_reads_cell_rows_only() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAA; 32];
        let next_lock = [0xAB; 32];
        let val = AddrTxValue::new(0, false, true, 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 1, 2, &[0x01; 32], &val);
        batch.put_addr_tx(&lock, 5, 0, &[0x05; 32], &val);
        // Named-only participation: never cell activity.
        batch.put_addr_tx_by_prefix(&lock[..20], 3, 0, &[0x03; 32], &val);
        // The next lock's rows sit right after this lock's in key order.
        batch.put_addr_tx(&next_lock, 9, 0, &[0x09; 32], &val);
        batch.commit().unwrap();

        let at = |block| store.latest_cell_addr_tx_at_or_below(&lock, block).unwrap();
        assert_eq!(at(4), Some((1, 2, vec![0x01; 32])));
        assert_eq!(at(3), Some((1, 2, vec![0x01; 32])));
        assert_eq!(at(5), Some((5, 0, vec![0x05; 32])));
        assert_eq!(at(100), Some((5, 0, vec![0x05; 32])));
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
        batch.put_addr_tx(
            &lock,
            100,
            0,
            &[0x11; 32],
            &AddrTxValue::new(0, false, true, 0),
        );
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 0, None).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_addr_txs_recent_reads_tx_hash_from_key_with_empty_value() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAC; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(
            &lock,
            100,
            1,
            &[0x10; 32],
            &AddrTxValue::new(0, false, true, 0),
        );
        batch.put_addr_tx(
            &lock,
            99,
            0,
            &[0x20; 32],
            &AddrTxValue::new(0, false, true, 0),
        );
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 100);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[0].2, vec![0x10; 32]);
        assert_eq!(rows[1].0, 99);
        assert_eq!(rows[1].1, 0);
        assert_eq!(rows[1].2, vec![0x20; 32]);
    }

    #[test]
    fn test_list_addr_txs_recent_keeps_two_rows_same_position_different_tx_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(
            &lock,
            100,
            1,
            &[0x10; 32],
            &AddrTxValue::new(0, false, true, 0),
        );
        batch.put_addr_tx(
            &lock,
            100,
            1,
            &[0x20; 32],
            &AddrTxValue::new(0, false, true, 0),
        );
        batch.put_addr_tx(
            &lock,
            99,
            0,
            &[0x30; 32],
            &AddrTxValue::new(0, false, true, 0),
        );
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, 100);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[0].2, vec![0x10; 32]);
        assert_eq!(rows[1].0, 100);
        assert_eq!(rows[1].1, 1);
        assert_eq!(rows[1].2, vec![0x20; 32]);
        assert_eq!(rows[2].0, 99);
        assert_eq!(rows[2].1, 0);
        assert_eq!(rows[2].2, vec![0x30; 32]);

        let next = store
            .list_addr_txs_recent(&lock, 10, Some((100, 1)))
            .unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].0, 99);
        assert_eq!(next[0].1, 0);
        assert_eq!(next[0].2, vec![0x30; 32]);
    }

    #[test]
    fn test_list_addr_txs_recent_returns_addr_tx_value() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xBB; 32];
        let val_sent = AddrTxValue::new(-500, true, false, 0);
        let val_recv = AddrTxValue::new(1000, false, true, 0);
        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 200, 0, &[0xAA; 32], &val_sent);
        batch.put_addr_tx(&lock, 100, 0, &[0xBB; 32], &val_recv);
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

    #[test]
    fn list_addr_txs_recent_merges_prefix_rows_in_descending_order() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0x42u8; 32];
        let prefix = &lock[..20];
        let mut b = StoreBatch::new(&store);
        b.put_addr_tx(
            &lock,
            10,
            0,
            &[0xA1; 32],
            &AddrTxValue::new(5, false, true, 0),
        );
        b.put_addr_tx_by_prefix(
            prefix,
            11,
            0,
            &[0xA2; 32],
            &AddrTxValue::new(0, false, false, TAG_IDENTITY),
        );
        b.put_addr_tx(
            &lock,
            12,
            1,
            &[0xA3; 32],
            &AddrTxValue::new(-5, true, false, 0),
        );
        // 别人的前缀
        b.put_addr_tx_by_prefix(
            &[0x99; 20],
            13,
            0,
            &[0xA4; 32],
            &AddrTxValue::new(0, false, false, 0),
        );
        b.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        let positions: Vec<(i64, i32)> = rows.iter().map(|(b, t, _, _)| (*b, *t)).collect();
        assert_eq!(positions, vec![(12, 1), (11, 0), (10, 0)]);
        assert_eq!(rows[1].3.tx_type_str(), "named");

        // 游标同时作用于两路
        let page = store
            .list_addr_txs_recent(&lock, 10, Some((12, 1)))
            .unwrap();
        let positions: Vec<(i64, i32)> = page.iter().map(|(b, t, _, _)| (*b, *t)).collect();
        assert_eq!(positions, vec![(11, 0), (10, 0)]);
        // limit 截断在归并之后
        assert_eq!(store.list_addr_txs_recent(&lock, 2, None).unwrap().len(), 2);
    }

    #[test]
    fn list_addr_txs_recent_rejects_same_position_in_both_indexes() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0x42u8; 32];
        let mut b = StoreBatch::new(&store);
        b.put_addr_tx(
            &lock,
            10,
            0,
            &[0xA1; 32],
            &AddrTxValue::new(5, false, true, 0),
        );
        b.put_addr_tx_by_prefix(
            &lock[..20],
            10,
            0,
            &[0xA1; 32],
            &AddrTxValue::new(0, false, false, 0),
        );
        b.commit().unwrap();
        let err = store.list_addr_txs_recent(&lock, 10, None).unwrap_err();
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
