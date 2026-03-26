//! Unified reorg undo-log operations.

use rocksdb::{IteratorMode, WriteBatch};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{UndoLogEntry, UndoLogStoreTarget};

const UNDO_ROLLBACK_FLUSH_EVERY: usize = 50_000;

use crate::bytes_to_hex;

#[derive(Debug, Default)]
pub struct UndoRollbackResult {
    pub undo_entries_applied: u64,
    pub domain_ops_applied: u64,
    pub append_ops_skipped: u64,
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
            let entry: UndoLogEntry = postcard::from_bytes(&value).map_err(|e| {
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
                UndoLogEntry::TxContext(_) => {
                    // Cell rollback is derived from TxContext during rollback_to_block.
                    // rollback_via_undo_log only prunes this journal entry.
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
}
