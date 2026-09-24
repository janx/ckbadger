//! Unified reorg undo-log operations.

use rocksdb::{IteratorMode, WriteBatch};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{UndoLogEntry, UndoLogStoreTarget, UndoTxContext};

const UNDO_ROLLBACK_FLUSH_EVERY: usize = 50_000;

use crate::bytes_to_hex;

#[derive(Debug, Default)]
pub struct UndoRollbackResult {
    pub undo_entries_applied: u64,
    pub domain_ops_applied: u64,
    pub append_ops_skipped: u64,
    /// TxContext entries extracted from the undo log before deletion.
    /// Needed by `rollback_to_block_with_append_only_store` for targeted
    /// cell rollback instead of full CF scans.
    pub tx_contexts: Vec<UndoTxContext>,
}

fn flush_undo_batches(
    domain_store: &CkbadgerStore,
    domain_batch: &mut WriteBatch,
) -> anyhow::Result<()> {
    if !domain_batch.is_empty() {
        domain_store.write_batch(std::mem::take(domain_batch))?;
    }
    Ok(())
}

impl CkbadgerStore {
    /// Returns true if reorg undo-log contains entries with `entry_block > block_num`.
    pub fn has_undo_log_entries_after(&self, block_num: i64) -> anyhow::Result<bool> {
        if block_num < -1 {
            anyhow::bail!(
                "invalid undo-log probe target: block_num={} (expected >= -1)",
                block_num
            );
        }

        let start_key = keys::encode_block_num(block_num + 1);
        let iter = self.iterator_cf(
            self.cf_reorg_undo_log_by_block(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate reorg_undo_log_by_block while probing from block {}: {}",
                    block_num,
                    e
                )
            })?;
            if key.len() != keys::REORG_UNDO_LOG_KEY_SIZE {
                anyhow::bail!(
                    "invalid reorg_undo_log_by_block key length while probing: expected={}, got={}",
                    keys::REORG_UNDO_LOG_KEY_SIZE,
                    key.len()
                );
            }
            let (entry_block, _) = keys::decode_reorg_undo_log_key(&key);
            if entry_block > block_num {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Roll back mutations using reorg undo-log entries with `block_num > rollback_to`.
    ///
    /// Entries are replayed in reverse sequence order (LIFO) per block range so
    /// the original write order is inverted correctly.
    /// Drop `EntityStats` undo entries for blocks in `(from_block, to_block]`.
    ///
    /// Only that scope: the `TxContext`, `DotBit` and `Object` scopes have
    /// different lifetimes and are pruned only by replay. Entries are staged
    /// into `batch` so the new coverage floor and the deletions it describes
    /// commit together — a floor that advanced without its deletions, or
    /// deletions without the floor, would each be a lie about what this store
    /// can still roll back.
    ///
    /// Returns the number of entries staged for deletion.
    pub fn prune_entity_stats_undo_below(
        &self,
        batch: &mut crate::batch::StoreBatch,
        from_block: i64,
        to_block: i64,
    ) -> anyhow::Result<u64> {
        if to_block <= from_block {
            return Ok(0);
        }
        let start_key = keys::encode_reorg_undo_log_key(from_block + 1, 0);
        let iter = self.iterator_cf(
            self.cf_reorg_undo_log_by_block(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut pruned = 0u64;
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate reorg_undo_log_by_block while pruning entity stats undo: from_block={}, to_block={}, error={}",
                    from_block,
                    to_block,
                    e
                )
            })?;
            if key.len() != keys::REORG_UNDO_LOG_KEY_SIZE {
                anyhow::bail!(
                    "invalid reorg_undo_log_by_block key length while pruning: expected={}, got={}",
                    keys::REORG_UNDO_LOG_KEY_SIZE,
                    key.len()
                );
            }
            let (block_num, seq) = keys::decode_reorg_undo_log_key(&key);
            if block_num > to_block {
                break;
            }
            // Only `EntityStats` entries are retention's to prune.
            if keys::UndoSeqScope::EntityStats.owns(seq) {
                batch.delete_reorg_undo_log_key(&key);
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    pub fn rollback_via_undo_log(
        &self,
        _append_store: &CkbadgerStore,
        rollback_to: i64,
    ) -> anyhow::Result<UndoRollbackResult> {
        if rollback_to < -1 {
            anyhow::bail!(
                "invalid undo rollback target: rollback_to={} (expected >= -1)",
                rollback_to
            );
        }

        let start_key = keys::encode_block_num(rollback_to + 1);
        let iter = self.iterator_cf(
            self.cf_reorg_undo_log_by_block(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut pending: Vec<(Vec<u8>, UndoLogEntry)> = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate reorg_undo_log_by_block during rollback_to={}: {}",
                    rollback_to,
                    e
                )
            })?;
            if key.len() != keys::REORG_UNDO_LOG_KEY_SIZE {
                anyhow::bail!(
                    "invalid reorg_undo_log_by_block key length during rollback: expected={}, got={}",
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
                    "failed to decode undo log entry during rollback: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            pending.push((key.to_vec(), entry));
        }

        if pending.is_empty() {
            return Ok(UndoRollbackResult::default());
        }

        let mut domain_batch = WriteBatch::default();
        let mut result = UndoRollbackResult::default();

        for (undo_key, entry) in pending.into_iter().rev() {
            match entry {
                UndoLogEntry::KeyMutation {
                    target_store,
                    cf_name,
                    key,
                    previous_value,
                } => match target_store {
                    UndoLogStoreTarget::Domain => {
                        self.apply_batch_op_by_cf_name(
                            &mut domain_batch,
                            &cf_name,
                            &key,
                            previous_value.as_deref(),
                        )?;
                        result.domain_ops_applied += 1;
                    }
                    UndoLogStoreTarget::AppendOnly => {
                        let _ = (cf_name, key, previous_value);
                        // Append-only store is immutable after write.
                        // Reorg replay only prunes the undo-log entry.
                        result.append_ops_skipped += 1;
                    }
                },
                UndoLogEntry::TxContext(ctx) => {
                    // Preserve TxContext for rollback_to_block cell cleanup.
                    result.tx_contexts.push(ctx);
                }
            }

            domain_batch.delete_cf(self.cf_reorg_undo_log_by_block(), &undo_key);
            result.undo_entries_applied += 1;

            if (result.undo_entries_applied as usize).is_multiple_of(UNDO_ROLLBACK_FLUSH_EVERY) {
                flush_undo_batches(self, &mut domain_batch)?;
            }
        }

        flush_undo_batches(self, &mut domain_batch)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreBatch;
    use tempfile::TempDir;

    fn open_dual_store() -> (CkbadgerStore, CkbadgerStore, TempDir) {
        let root = TempDir::new().unwrap();
        let domain_path = root.path().join("domain");
        let append_path = root.path().join("append");
        let domain = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append = CkbadgerStore::open_append_only(&append_path).unwrap();
        (domain, append, root)
    }

    #[test]
    fn test_rollback_via_undo_log_restores_domain_and_preserves_append_state() {
        let (domain, append, _root) = open_dual_store();

        domain.put_cf(domain.cf_sync_meta(), b"k1", b"v1").unwrap();
        append.put_cf(append.cf_cells(), b"c1", b"cell1").unwrap();

        // Forward writes happened in block 10.
        domain.put_cf(domain.cf_sync_meta(), b"k1", b"v2").unwrap();
        domain
            .put_cf(domain.cf_sync_meta(), b"new", b"created")
            .unwrap();

        let mut batch = StoreBatch::new(&domain);
        batch.put_reorg_undo_log_by_block(
            10,
            0,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: crate::store::CF_SYNC_META.to_string(),
                key: b"k1".to_vec(),
                previous_value: Some(b"v1".to_vec()),
            },
        );
        batch.put_reorg_undo_log_by_block(
            10,
            1,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::AppendOnly,
                cf_name: crate::store::CF_CELLS.to_string(),
                key: b"c1".to_vec(),
                previous_value: Some(b"cell1".to_vec()),
            },
        );
        batch.put_reorg_undo_log_by_block(
            10,
            2,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: crate::store::CF_SYNC_META.to_string(),
                key: b"new".to_vec(),
                previous_value: None,
            },
        );
        batch.commit().unwrap();

        let res = domain.rollback_via_undo_log(&append, 9).unwrap();
        assert_eq!(res.undo_entries_applied, 3);
        assert_eq!(res.domain_ops_applied, 2);
        assert_eq!(res.append_ops_skipped, 1);

        assert_eq!(
            domain
                .get_cf(domain.cf_sync_meta(), b"k1")
                .unwrap()
                .unwrap()
                .as_slice(),
            b"v1"
        );
        assert!(domain
            .get_cf(domain.cf_sync_meta(), b"new")
            .unwrap()
            .is_none());
        assert_eq!(
            append
                .get_cf(append.cf_cells(), b"c1")
                .unwrap()
                .unwrap()
                .as_slice(),
            b"cell1"
        );

        let k0 = keys::encode_reorg_undo_log_key(10, 0);
        let k1 = keys::encode_reorg_undo_log_key(10, 1);
        let k2 = keys::encode_reorg_undo_log_key(10, 2);
        assert!(domain
            .get_cf(domain.cf_reorg_undo_log_by_block(), &k0)
            .unwrap()
            .is_none());
        assert!(domain
            .get_cf(domain.cf_reorg_undo_log_by_block(), &k1)
            .unwrap()
            .is_none());
        assert!(domain
            .get_cf(domain.cf_reorg_undo_log_by_block(), &k2)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_via_undo_log_ignores_entries_at_or_below_target() {
        let (domain, append, _root) = open_dual_store();
        domain.put_cf(domain.cf_sync_meta(), b"k", b"v2").unwrap();

        let mut batch = StoreBatch::new(&domain);
        batch.put_reorg_undo_log_by_block(
            9,
            0,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: crate::store::CF_SYNC_META.to_string(),
                key: b"k".to_vec(),
                previous_value: Some(b"v1".to_vec()),
            },
        );
        batch.commit().unwrap();

        let res = domain.rollback_via_undo_log(&append, 9).unwrap();
        assert_eq!(res.undo_entries_applied, 0);

        assert_eq!(
            domain
                .get_cf(domain.cf_sync_meta(), b"k")
                .unwrap()
                .unwrap()
                .as_slice(),
            b"v2"
        );

        let key = keys::encode_reorg_undo_log_key(9, 0);
        assert!(domain
            .get_cf(domain.cf_reorg_undo_log_by_block(), &key)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_has_undo_log_entries_after_detects_pending_entries() {
        let (domain, _append, _root) = open_dual_store();
        let mut batch = StoreBatch::new(&domain);
        batch.put_reorg_undo_log_by_block(
            5,
            0,
            &UndoLogEntry::TxContext(crate::types::UndoTxContext {
                tx_hash: vec![0x11; 32],
                outputs_count: 0,
                inputs: vec![],
            }),
        );
        batch.put_reorg_undo_log_by_block(
            8,
            0,
            &UndoLogEntry::TxContext(crate::types::UndoTxContext {
                tx_hash: vec![0x22; 32],
                outputs_count: 0,
                inputs: vec![],
            }),
        );
        batch.commit().unwrap();

        assert!(domain.has_undo_log_entries_after(4).unwrap());
        assert!(domain.has_undo_log_entries_after(7).unwrap());
        assert!(!domain.has_undo_log_entries_after(8).unwrap());
    }

    #[test]
    fn test_rollback_via_undo_log_extracts_tx_contexts() {
        let (domain, append, _root) = open_dual_store();
        let mut batch = StoreBatch::new(&domain);
        batch.put_reorg_undo_log_by_block(
            10,
            0,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: crate::store::CF_SYNC_META.to_string(),
                key: b"k".to_vec(),
                previous_value: Some(b"v".to_vec()),
            },
        );
        batch.put_reorg_undo_log_by_block(
            10,
            1,
            &UndoLogEntry::TxContext(crate::types::UndoTxContext {
                tx_hash: vec![0xAA; 32],
                outputs_count: 2,
                inputs: vec![],
            }),
        );
        batch.put_reorg_undo_log_by_block(
            10,
            2,
            &UndoLogEntry::TxContext(crate::types::UndoTxContext {
                tx_hash: vec![0xBB; 32],
                outputs_count: 1,
                inputs: vec![crate::types::UndoInputOutPoint {
                    tx_hash: vec![0xCC; 32],
                    output_index: 0,
                }],
            }),
        );
        batch.commit().unwrap();

        let res = domain.rollback_via_undo_log(&append, 9).unwrap();
        assert_eq!(res.undo_entries_applied, 3);
        assert_eq!(res.domain_ops_applied, 1);
        assert_eq!(res.tx_contexts.len(), 2);
        // Undo log replays in reverse (LIFO), so seq=2 (0xBB) comes first.
        assert_eq!(res.tx_contexts[0].tx_hash, vec![0xBB; 32]);
        assert_eq!(res.tx_contexts[0].inputs.len(), 1);
        assert_eq!(res.tx_contexts[1].tx_hash, vec![0xAA; 32]);
        assert_eq!(res.tx_contexts[1].outputs_count, 2);
    }

    #[test]
    fn test_has_undo_log_entries_after_fails_on_malformed_key() {
        let (domain, _append, _root) = open_dual_store();
        domain
            .put_cf(domain.cf_reorg_undo_log_by_block(), b"malformed", b"value")
            .unwrap();

        let err = domain.has_undo_log_entries_after(-1).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid reorg_undo_log_by_block key length"));
    }

    /// Task 2.5: the retention window prunes ONLY `EntityStats` entries, and
    /// only below the new floor. `TxContext` / `DotBit` / `Object` entries have
    /// different lifetimes and are consumed by replay, never by retention.
    #[test]
    fn test_prune_entity_stats_undo_keeps_other_scopes_and_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        const ENTITY: u64 = keys::UndoSeqScope::EntityStats.seq_base();
        const TX_CONTEXT: u64 = keys::UndoSeqScope::TxContext.seq_base();
        const OBJECT: u64 = keys::UndoSeqScope::Object.seq_base();

        {
            let mut batch = StoreBatch::new(&store);
            for block in 1..=1_200i64 {
                for (seq, cf) in [
                    (ENTITY, crate::store::CF_STATS_TOKEN),
                    (OBJECT, crate::store::CF_MNFT_DATA),
                ] {
                    batch.put_reorg_undo_log_by_block(
                        block,
                        seq,
                        &UndoLogEntry::KeyMutation {
                            target_store: UndoLogStoreTarget::Domain,
                            cf_name: cf.to_string(),
                            key: vec![block as u8; 8],
                            previous_value: None,
                        },
                    );
                }
                batch.put_reorg_undo_log_by_block(
                    block,
                    TX_CONTEXT,
                    &UndoLogEntry::TxContext(crate::types::UndoTxContext {
                        tx_hash: vec![block as u8; 32],
                        outputs_count: 1,
                        inputs: vec![],
                    }),
                );
            }
            batch.commit().unwrap();
        }

        // Retain the last 1000 blocks of a 1200-block chain: the floor moves to
        // 200, so EntityStats entries for blocks 1..=200 go.
        let mut batch = StoreBatch::new(&store);
        let pruned = store
            .prune_entity_stats_undo_below(&mut batch, 0, 200)
            .unwrap();
        assert_eq!(pruned, 200, "one EntityStats entry per block 1..=200");
        batch.put_entity_stats_undo_contract(&crate::types::EntityStatsUndoContract {
            version: crate::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 200,
            updated_at_block: 1_200,
        });
        batch.commit().unwrap();

        let mut entity = 0usize;
        let mut other = 0usize;
        let mut lowest_entity_block = i64::MAX;
        let iter = store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        );
        for item in iter {
            let (key, _) = item.unwrap();
            let (block, seq) = keys::decode_reorg_undo_log_key(&key);
            if keys::UndoSeqScope::EntityStats.owns(seq) {
                entity += 1;
                lowest_entity_block = lowest_entity_block.min(block);
            } else {
                other += 1;
            }
        }
        assert_eq!(entity, 1_000, "EntityStats entries left: blocks 201..=1200");
        assert_eq!(
            lowest_entity_block, 201,
            "nothing at or below the floor may survive"
        );
        assert_eq!(
            other, 2_400,
            "TxContext and Object entries are untouched by retention"
        );

        // The floor and its deletions committed together.
        let contract = store.get_entity_stats_undo_contract().unwrap().unwrap();
        assert_eq!(contract.coverage_floor_block, 200);
        assert_eq!(contract.updated_at_block, 1_200);
        assert_eq!(
            contract.version,
            crate::types::ENTITY_STATS_UNDO_CONTRACT_VERSION
        );
    }

    #[test]
    fn test_prune_entity_stats_undo_is_a_noop_when_the_floor_does_not_move() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        assert_eq!(
            store
                .prune_entity_stats_undo_below(&mut batch, 500, 500)
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .prune_entity_stats_undo_below(&mut batch, 500, 400)
                .unwrap(),
            0,
            "a floor that would move backwards prunes nothing"
        );
    }
}
