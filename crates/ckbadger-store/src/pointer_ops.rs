//! Pointer resolution (`docs/prompts/DATA_DESIGN.md` principle 3: pointers
//! over hashes).
//!
//! A domain row that names a transaction stores its canonical position
//! `(block_number, tx_idx)`, never its hash; the hash is `tx_index` at that
//! position. Readers resolve the positions of a whole page or response at
//! once through the helpers here — one `multi_get` on `tx_index` per call,
//! never a point read per row. A position without a `tx_index` row is an
//! inconsistent pointer: an error naming the row kind, the position and the
//! `tx_index` key, never a skipped row.

use crate::bytes_to_hex;
use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TxIndexEntry;

impl CkbadgerStore {
    /// The `tx_index` entries at `positions`, in order (a position may repeat),
    /// read with one `multi_get`. `row_kind` names the rows holding the
    /// pointers, for the error.
    pub fn tx_index_entries_at(
        &self,
        row_kind: &str,
        positions: &[(i64, i32)],
    ) -> anyhow::Result<Vec<TxIndexEntry>> {
        if positions.is_empty() {
            return Ok(Vec::new());
        }
        let cf = self.cf_tx_index();
        let keys: Vec<Vec<u8>> = positions
            .iter()
            .map(|(block, tx_idx)| keys::encode_tx_slot_key(*block, *tx_idx))
            .collect();
        let values =
            self.multi_get_cf_sorted(keys.iter().map(|key| (cf, key.as_slice())).collect());
        positions
            .iter()
            .zip(keys.iter())
            .zip(values)
            .map(|(((block, tx_idx), key), value)| {
                let value = value
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "{row_kind}: reading tx_index at block={block}, tx_idx={tx_idx} (key 0x{}) failed: {e}",
                            bytes_to_hex(key)
                        )
                    })?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "{row_kind} points at block={block}, tx_idx={tx_idx}, which has no tx_index row (key 0x{})",
                            bytes_to_hex(key)
                        )
                    })?;
                bincode::deserialize::<TxIndexEntry>(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "{row_kind}: tx_index row at block={block}, tx_idx={tx_idx} (key 0x{}) does not decode: {e}",
                        bytes_to_hex(key)
                    )
                })
            })
            .collect()
    }

    /// The transaction hashes at `positions`, in order: [`Self::tx_index_entries_at`]'s
    /// `tx_hash`.
    pub fn tx_hashes_at(
        &self,
        row_kind: &str,
        positions: &[(i64, i32)],
    ) -> anyhow::Result<Vec<[u8; 32]>> {
        Ok(self
            .tx_index_entries_at(row_kind, positions)?
            .into_iter()
            .map(|entry| entry.tx_hash)
            .collect())
    }

    /// The hashes of the transactions that consumed cells, from their
    /// `consumed_cells` positions (`ConsumedCellMeta::consumer_position`,
    /// `ConsumedCellInfo::consumer_position`), in order: the one way a reader
    /// turns a consumed cell's consumer into a hash.
    pub fn consumer_tx_hashes(&self, positions: &[(i64, i32)]) -> anyhow::Result<Vec<[u8; 32]>> {
        self.tx_hashes_at("consumed_cells", positions)
    }
}

#[cfg(test)]
mod tests {
    use crate::batch::StoreBatch;
    use crate::store::CkbadgerStore;
    use crate::types::TxIndexEntry;

    fn entry(tx_hash: [u8; 32]) -> TxIndexEntry {
        TxIndexEntry {
            tx_hash,
            is_cellbase: false,
            timestamp: 0,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 0,
            cycles: None,
            semantic_tags: 0,
        }
    }

    #[test]
    fn consumer_tx_hashes_resolve_a_page_with_one_multi_get() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_index(10, 1, &entry([0x0a; 32]));
        batch.put_tx_index(12, 3, &entry([0x0c; 32]));
        batch.commit().unwrap();

        store.reset_read_call_counters();
        let hashes = store
            .consumer_tx_hashes(&[(12, 3), (10, 1), (12, 3)])
            .unwrap();
        assert_eq!(hashes, vec![[0x0c; 32], [0x0a; 32], [0x0c; 32]]);
        assert_eq!(store.read_call_counts(), (0, 1), "(get_cf, multi_get_cf)");
    }

    #[test]
    fn a_position_without_a_tx_index_row_is_an_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_index(10, 1, &entry([0x0a; 32]));
        batch.commit().unwrap();

        let err = store
            .consumer_tx_hashes(&[(10, 1), (11, 2)])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("consumed_cells points at block=11, tx_idx=2")
                && err.contains("key 0x000000000000000b00000002"),
            "{err}"
        );
    }
}
