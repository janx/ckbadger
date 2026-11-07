//! The activity pass's row writers (`addr_txs`, `addr_txs_by_prefix`,
//! `tx_actions`): each writes in the caller's block scope, so the live batch's
//! block journal records what it creates.

use ckbadger_store::batch::StoreBatch;

pub(crate) fn put_addr_tx(
    batch: &mut StoreBatch<'_>,
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    value: &ckbadger_store::types::AddrTxValue,
) {
    // Written in the caller's block scope: the live batch's block journal
    // records the row's creation.
    batch.put_addr_tx(lock_hash, block_num, tx_idx, value);
}

/// `addr_txs_by_prefix` row for a protocol-named party.
///
/// Domain store, append-only semantics, written in the caller's block scope
/// (the block journal records the creation) — exactly like [`put_addr_tx`].
pub(crate) fn put_addr_tx_by_prefix(
    batch: &mut StoreBatch<'_>,
    prefix: &[u8],
    block_num: i64,
    tx_idx: i32,
    value: &ckbadger_store::types::AddrTxValue,
) {
    batch.put_addr_tx_by_prefix(prefix, block_num, tx_idx, value);
}

pub(crate) fn put_tx_actions(
    batch: &mut StoreBatch<'_>,
    block_num: i64,
    actions: &ckbadger_store::types::TxActions,
) {
    assert_eq!(
        actions.block_number, block_num,
        "tx actions block number mismatch"
    );
    // The `activities` row is RangeByBlock (rollback deletes it by block);
    // its `tx_actions_by_bucket` row is journaled in the caller's block scope.
    batch.put_tx_actions(actions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::keys;
    use ckbadger_store::CkbadgerStore;

    #[test]
    fn test_put_tx_actions_writes_to_domain_without_undo() {
        let domain_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let actions = ckbadger_store::types::TxActions {
            tx_hash: vec![0xAB; 32],
            block_hash: vec![0xBC; 32],
            block_number: 42,
            tx_index: 3,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ckbadger_store::types::ParticipantDelta {
                id: ckbadger_store::types::ParticipantId::Lock([0x44; 32]),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
        };

        let mut domain_batch = StoreBatch::new(&domain_store);
        put_tx_actions(&mut domain_batch, 42, &actions);
        domain_batch.commit().unwrap();

        let iter = domain_store.iterator_cf(
            domain_store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        );
        assert_eq!(iter.count(), 0);
        assert!(domain_store
            .get_tx_actions(42, 3, &actions.tx_hash)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_no_undo_entries_written_when_bulk_skip_pattern_used() {
        // Demonstrates the bulk-sync pattern: write data without undo log.
        // During bulk sync, callers use batch.put_addr_tx/put_activity directly
        // instead of the undo helper wrappers.
        let domain_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let lock_hash = [0x44; 32];

        // Bulk pattern: write directly to domain store without undo log
        let mut domain_batch = StoreBatch::new(&domain_store);
        domain_batch.put_addr_tx(
            &lock_hash,
            100,
            0,
            &ckbadger_store::types::AddrTxValue::new(0, false, true, 0),
        );
        domain_batch.commit().unwrap();

        // Verify data was written
        let key = keys::encode_addr_tx_key(&lock_hash, 100, 0);
        assert!(domain_store
            .get_cf(domain_store.cf_addr_txs(), &key)
            .unwrap()
            .is_some());

        // Verify NO undo entries in domain store
        let iter = domain_store.iterator_cf(
            domain_store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        );
        assert_eq!(
            iter.count(),
            0,
            "bulk sync should produce zero undo entries"
        );
    }
}
