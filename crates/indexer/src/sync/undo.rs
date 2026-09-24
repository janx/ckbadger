use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};

use ckbadger_store::batch::StoreBatch;

use super::types::{TxData, UndoSeqScope, UNDO_SEQ_LOCAL_MAX, UNDO_SEQ_SCOPE_SHIFT};

/// The block-scoped undo sequence counter for one committed batch.
///
/// Every writer that records an undo entry for a block must draw from the SAME
/// counter. The Spore, mNFT and .bit batch states used to own three private
/// maps that all started at 0 and all stamped `UndoSeqScope::Object`, so two
/// object writes in one block computed the identical undo key and the second
/// overwrote the first inside the batch — the first entity's pre-image was gone
/// before rollback ever ran (POSTMORTEM IDX-008, pinned by
/// `db::writer::undo_seq_tests::object_scope_undo_seq_is_shared_across_entity_batch_states`).
///
/// `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>`: the batch write path is an
/// async fn whose future must stay `Send`.
#[derive(Clone, Default)]
pub(crate) struct SharedUndoSeq(Arc<Mutex<HashMap<i64, u64>>>);

impl SharedUndoSeq {
    pub(crate) fn next(&self, block_num: i64, scope: UndoSeqScope) -> u64 {
        let mut guard = self.0.lock().expect("shared undo seq mutex poisoned");
        next_undo_seq(&mut guard, block_num, scope)
    }

    /// Run `f` against the shared map itself, for callers that still take a
    /// plain `&mut HashMap` (the entity stats overlay).
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut HashMap<i64, u64>) -> R) -> R {
        let mut guard = self.0.lock().expect("shared undo seq mutex poisoned");
        f(&mut guard)
    }
}

pub(crate) fn next_undo_seq(
    undo_seq_by_block: &mut HashMap<i64, u64>,
    block_num: i64,
    scope: UndoSeqScope,
) -> u64 {
    let seq_entry = undo_seq_by_block.entry(block_num).or_insert(0);
    let local_seq = *seq_entry;
    assert!(
        local_seq <= UNDO_SEQ_LOCAL_MAX,
        "undo seq local counter overflow: block_num={}, scope={:?}, local_seq={}",
        block_num,
        scope,
        local_seq
    );
    *seq_entry = local_seq
        .checked_add(1)
        .expect("undo seq overflow for block-scoped rollback log");
    ((scope as u64) << UNDO_SEQ_SCOPE_SHIFT) | local_seq
}

/// One `(block, cf, key)` whose pre-image more than one undo scope recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UndoScopeOverlap {
    pub(crate) block: i64,
    pub(crate) cf_name: String,
    pub(crate) key: Vec<u8>,
    /// The scope ids (`UndoSeqScope as u64`) that recorded it, ascending.
    pub(crate) scopes: Vec<u64>,
}

/// Every key mutation in `from_block..=to_block` that more than one undo scope
/// recorded a pre-image for.
///
/// The undo key is `block ‖ (scope << 48 | local seq)`, so rollback replays one
/// block's entries scope-major (highest scope first), not in the order they
/// were written across scopes. That is exact only while each key a block
/// mutates is recorded by ONE scope: then its entries all sit in one scope and
/// replay LIFO within it. This is the check for that invariant; the live write
/// path runs it on every committed batch in debug builds.
pub(crate) fn undo_scope_overlaps(
    store: &ckbadger_store::CkbadgerStore,
    from_block: i64,
    to_block: i64,
) -> Result<Vec<UndoScopeOverlap>> {
    use std::collections::{BTreeMap, BTreeSet};

    let start = ckbadger_store::keys::encode_reorg_undo_log_key(from_block, 0);
    let mut recorded: BTreeMap<(i64, String, Vec<u8>), BTreeSet<u64>> = BTreeMap::new();
    for item in store.iterator_cf(
        store.cf_reorg_undo_log_by_block(),
        rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
    ) {
        let (key, value) = item?;
        let (block, seq) = ckbadger_store::keys::decode_reorg_undo_log_key(&key);
        if block > to_block {
            break;
        }
        let entry: ckbadger_store::types::UndoLogEntry =
            bincode::deserialize(&value).map_err(|e| {
                anyhow!("undo log entry at block {block} seq {seq} is not decodable: {e}")
            })?;
        if let ckbadger_store::types::UndoLogEntry::KeyMutation { cf_name, key, .. } = entry {
            recorded
                .entry((block, cf_name, key))
                .or_default()
                .insert(seq >> UNDO_SEQ_SCOPE_SHIFT);
        }
    }
    Ok(recorded
        .into_iter()
        .filter(|(_, scopes)| scopes.len() > 1)
        .map(|((block, cf_name, key), scopes)| UndoScopeOverlap {
            block,
            cf_name,
            key,
            scopes: scopes.into_iter().collect(),
        })
        .collect())
}

#[cfg(test)]
pub(crate) fn put_append_delete_undo_entry(
    domain_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    scope: UndoSeqScope,
    block_num: i64,
    cf_name: &str,
    key: &[u8],
) {
    put_delete_undo_entry(
        domain_batch,
        undo_seq_by_block,
        ckbadger_store::types::UndoLogStoreTarget::AppendOnly,
        scope,
        block_num,
        cf_name,
        key,
    );
}

#[cfg(test)]
fn put_delete_undo_entry(
    domain_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    target_store: ckbadger_store::types::UndoLogStoreTarget,
    scope: UndoSeqScope,
    block_num: i64,
    cf_name: &str,
    key: &[u8],
) {
    let seq = next_undo_seq(undo_seq_by_block, block_num, scope);
    let undo = ckbadger_store::types::UndoLogEntry::KeyMutation {
        target_store,
        cf_name: cf_name.to_string(),
        key: key.to_vec(),
        previous_value: None,
    };
    domain_batch.put_reorg_undo_log_by_block(block_num, seq, &undo);
}

pub(crate) fn put_tx_context_undo_entries(
    domain_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    txs: &[TxData],
) -> Result<()> {
    for tx in txs {
        let mut inputs = Vec::with_capacity(tx.inputs.len());
        for input in &tx.inputs {
            let is_cellbase_sentinel =
                input.previous_output_index == -1 && input.previous_tx_hash == [0u8; 32];
            if is_cellbase_sentinel {
                if !tx.is_cellbase {
                    bail!(
                        "non-cellbase tx contains cellbase sentinel input in rollback tx-context: tx_hash=0x{}, block={}",
                        hex::encode(tx.hash),
                        tx.block_number
                    );
                }
                continue;
            }
            if input.previous_output_index < 0 {
                bail!(
                    "negative rollback tx-context input index: tx_hash=0x{}, block={}, previous_output_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    input.previous_output_index
                );
            }
            let output_index = i16::try_from(input.previous_output_index).map_err(|_| {
                anyhow!(
                    "rollback tx-context input index exceeds i16 range: tx_hash=0x{}, block={}, previous_output_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    input.previous_output_index
                )
            })?;
            inputs.push(ckbadger_store::types::UndoInputOutPoint {
                tx_hash: input.previous_tx_hash.to_vec(),
                output_index,
            });
        }

        let ctx = ckbadger_store::types::UndoTxContext {
            tx_hash: tx.hash.to_vec(),
            outputs_count: tx.outputs_count,
            inputs,
        };
        let seq = next_undo_seq(undo_seq_by_block, tx.block_number, UndoSeqScope::TxContext);
        domain_batch.put_reorg_undo_log_by_block(
            tx.block_number,
            seq,
            &ckbadger_store::types::UndoLogEntry::TxContext(ctx),
        );
    }
    Ok(())
}

pub(crate) fn put_addr_tx(
    batch: &mut StoreBatch<'_>,
    _undo_seq_by_block: &mut HashMap<i64, u64>,
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
    value: &ckbadger_store::types::AddrTxValue,
) {
    // addr_txs is now in domain store; rollback deletes entries directly (no undo log needed)
    batch.put_addr_tx(lock_hash, block_num, tx_idx, tx_hash, value);
}

/// `addr_txs_by_prefix` row for a protocol-named party.
///
/// Domain store, append-only semantics, no undo entry: rollback deletes the row
/// outright — exactly like [`put_addr_tx`].
pub(crate) fn put_addr_tx_by_prefix(
    batch: &mut StoreBatch<'_>,
    _undo_seq_by_block: &mut HashMap<i64, u64>,
    prefix: &[u8],
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
    value: &ckbadger_store::types::AddrTxValue,
) {
    batch.put_addr_tx_by_prefix(prefix, block_num, tx_idx, tx_hash, value);
}

pub(crate) fn put_tx_actions(
    batch: &mut StoreBatch<'_>,
    _undo_seq_by_block: &mut HashMap<i64, u64>,
    block_num: i64,
    actions: &ckbadger_store::types::TxActions,
) {
    assert_eq!(
        actions.block_number, block_num,
        "tx actions block number mismatch in undo helper"
    );
    // activities is now in domain store; rollback deletes entries directly (no undo log needed)
    batch.put_tx_actions(actions);
}

#[cfg(test)]
mod tests {
    fn rollback_undo_log_after_batch_cleanup(
        store: &CkbadgerStore,
        append_only_store: &CkbadgerStore,
        cleanup_tip: i64,
        context: &str,
    ) -> anyhow::Result<()> {
        let _ = store
            .rollback_via_undo_log(append_only_store, cleanup_tip)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to rollback undo log after batch cleanup: cleanup_tip={}, context={}, error={:#}",
                    cleanup_tip,
                    context,
                    e
                )
            })?;
        Ok(())
    }

    use super::*;
    use chrono::Utc;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::keys;
    use ckbadger_store::CkbadgerStore;

    #[test]
    fn undo_scope_overlaps_reports_a_key_recorded_by_two_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let entry = |key: &[u8]| ckbadger_store::types::UndoLogEntry::KeyMutation {
            target_store: ckbadger_store::types::UndoLogStoreTarget::Domain,
            cf_name: ckbadger_store::CF_IDENTITY_AGG.to_string(),
            key: key.to_vec(),
            previous_value: None,
        };
        let seq = SharedUndoSeq::default();
        let mut batch = StoreBatch::new(&store);
        // Block 7: the same key under DotBit and Object — an overlap.
        batch.put_reorg_undo_log_by_block(7, seq.next(7, UndoSeqScope::DotBit), &entry(b"shared"));
        batch.put_reorg_undo_log_by_block(7, seq.next(7, UndoSeqScope::Object), &entry(b"shared"));
        // Block 7: one key per scope, twice under one scope — fine.
        batch.put_reorg_undo_log_by_block(7, seq.next(7, UndoSeqScope::Object), &entry(b"obj"));
        batch.put_reorg_undo_log_by_block(7, seq.next(7, UndoSeqScope::Object), &entry(b"obj"));
        // Block 8: the same key again, but in another block — fine.
        batch.put_reorg_undo_log_by_block(
            8,
            seq.next(8, UndoSeqScope::EntityStats),
            &entry(b"shared"),
        );
        batch.commit().unwrap();

        assert_eq!(
            undo_scope_overlaps(&store, 7, 8).unwrap(),
            vec![UndoScopeOverlap {
                block: 7,
                cf_name: ckbadger_store::CF_IDENTITY_AGG.to_string(),
                key: b"shared".to_vec(),
                scopes: vec![UndoSeqScope::DotBit as u64, UndoSeqScope::Object as u64],
            }]
        );
        assert!(undo_scope_overlaps(&store, 8, 8).unwrap().is_empty());
    }

    fn dummy_dao_cell(capacity: i64, is_deposit: bool) -> crate::parser::cell::ParsedCell {
        crate::parser::cell::ParsedCell {
            capacity,
            lock_code_hash: vec![],
            lock_hash_type: 0,
            lock_args: vec![],
            lock_script_hash: vec![],
            type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                crate::parser::dao::DAO_CODE_HASH,
            )),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: None,
            data_hash: [0u8; 32],
            data_size: 8,
            data: if is_deposit {
                vec![0u8; 8]
            } else {
                1u64.to_le_bytes().to_vec()
            },
        }
    }

    fn dummy_tx_data(
        hash: [u8; 32],
        is_cellbase: bool,
        inputs: Vec<crate::parser::transaction::ParsedInput>,
        cells: Vec<crate::parser::cell::ParsedCell>,
        witnesses: Vec<String>,
        outputs_data: Vec<String>,
    ) -> TxData {
        let inputs_count =
            i16::try_from(inputs.len()).expect("test helper inputs_count exceeds i16 range");
        let outputs_count =
            i16::try_from(cells.len()).expect("test helper outputs_count exceeds i16 range");
        TxData {
            hash,
            block_number: 0,
            tx_index: 0,
            inputs_count,
            outputs_count,
            is_cellbase,
            inputs,
            cells,
            witnesses,
            outputs_data,
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc::now(),
            semantic_tags: 0,
        }
    }

    #[test]
    fn test_next_undo_seq_scoped_increments_correctly() {
        let mut scope_map = HashMap::new();
        let block_num = 42;

        let seq1 = next_undo_seq(&mut scope_map, block_num, UndoSeqScope::TxContext);
        let seq2 = next_undo_seq(&mut scope_map, block_num, UndoSeqScope::TxContext);

        assert_ne!(seq1, seq2);
        assert_eq!(seq1 >> UNDO_SEQ_SCOPE_SHIFT, UndoSeqScope::TxContext as u64);
        assert_eq!(seq2 >> UNDO_SEQ_SCOPE_SHIFT, UndoSeqScope::TxContext as u64);
        // Second call should have local seq = 1
        assert_eq!(seq2 & UNDO_SEQ_LOCAL_MAX, 1);
    }

    #[test]
    fn test_rollback_via_undo_log_preserves_append_only_cells() {
        // After dual-store refactor, CF_CELLS is the only append-only CF.
        // Undo log entries targeting AppendOnly are skipped during rollback,
        // so cells written to append-only store survive rollback.
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();

        let cell_key_keep = ckbadger_store::keys::encode_outpoint(&[0xA1; 32], 0);
        let cell_key_drop = ckbadger_store::keys::encode_outpoint(&[0xA2; 32], 0);
        append_store
            .put_cf(append_store.cf_cells(), &cell_key_keep, &[0x01])
            .unwrap();
        append_store
            .put_cf(append_store.cf_cells(), &cell_key_drop, &[0x02])
            .unwrap();

        // Write an undo entry targeting append-only store for CF_CELLS
        let mut domain_batch = StoreBatch::new(&domain_store);
        let mut undo_seq_by_block = HashMap::new();
        put_append_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            UndoSeqScope::TxContext, // scope is just for sequence partitioning
            20,
            ckbadger_store::CF_CELLS,
            &cell_key_drop,
        );
        domain_batch.commit().unwrap();

        domain_store
            .rollback_via_undo_log(&append_store, 15)
            .unwrap();

        // Both cells survive because append-only entries are skipped during rollback
        assert!(append_store
            .get_cf(append_store.cf_cells(), &cell_key_keep)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_cells(), &cell_key_drop)
            .unwrap()
            .is_some());
    }

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
        let mut undo_seq_by_block = HashMap::new();
        put_tx_actions(&mut domain_batch, &mut undo_seq_by_block, 42, &actions);
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
    fn test_rollback_undo_log_after_batch_cleanup_prunes_valid_entries() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();

        let mut batch = StoreBatch::new(&domain_store);
        batch.put_reorg_undo_log_by_block(
            6,
            0,
            &ckbadger_store::types::UndoLogEntry::TxContext(ckbadger_store::types::UndoTxContext {
                tx_hash: vec![0x88; 32],
                outputs_count: 0,
                inputs: vec![],
            }),
        );
        batch.commit().unwrap();

        rollback_undo_log_after_batch_cleanup(&domain_store, &append_store, 5, "unit-test")
            .unwrap();

        let undo_key = keys::encode_reorg_undo_log_key(6, 0);
        assert!(domain_store
            .get_cf(domain_store.cf_reorg_undo_log_by_block(), &undo_key)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_undo_log_after_batch_cleanup_fails_on_malformed_undo_key() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();

        domain_store
            .put_cf(
                domain_store.cf_reorg_undo_log_by_block(),
                b"bad-key",
                b"bad-value",
            )
            .unwrap();

        let err = rollback_undo_log_after_batch_cleanup(
            &domain_store,
            &append_store,
            -1,
            "unit-test malformed key",
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("failed to rollback undo log after batch cleanup"));
    }

    #[test]
    fn test_put_tx_context_undo_entries_skips_cellbase_sentinel_input() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let cellbase_tx = dummy_tx_data(
            [0xAA; 32],
            true,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: [0u8; 32],
                previous_output_index: -1,
                since: 0,
            }],
            vec![dummy_dao_cell(100_00000000, true)],
            vec![],
            vec![],
        );

        let mut batch = StoreBatch::new(&store);
        let mut undo_seq_by_block = HashMap::new();
        put_tx_context_undo_entries(&mut batch, &mut undo_seq_by_block, &[cellbase_tx]).unwrap();
        batch.commit().unwrap();

        let mut iter = store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        );
        let (_key, value) = iter.next().unwrap().unwrap();
        let entry: ckbadger_store::types::UndoLogEntry = bincode::deserialize(&value).unwrap();
        let ckbadger_store::types::UndoLogEntry::TxContext(ctx) = entry else {
            panic!("undo entry should be tx context");
        };
        assert!(ctx.inputs.is_empty());
        assert_eq!(ctx.outputs_count, 1);
    }

    #[test]
    fn test_put_tx_context_undo_entries_rejects_cellbase_sentinel_in_non_cellbase_tx() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let invalid_tx = dummy_tx_data(
            [0xBB; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: [0u8; 32],
                previous_output_index: -1,
                since: 0,
            }],
            vec![],
            vec![],
            vec![],
        );

        let mut batch = StoreBatch::new(&store);
        let mut undo_seq_by_block = HashMap::new();
        let err = put_tx_context_undo_entries(&mut batch, &mut undo_seq_by_block, &[invalid_tx])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("non-cellbase tx contains cellbase sentinel input"));
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
            &[0xAA; 32],
            &ckbadger_store::types::AddrTxValue::new(0, false, true, 0),
        );
        domain_batch.commit().unwrap();

        // Verify data was written
        let key = keys::encode_addr_tx_key(&lock_hash, 100, 0, &[0xAA; 32]);
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
