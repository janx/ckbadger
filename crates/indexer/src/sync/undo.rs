use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::CkbadgerStore;

use super::types::{TxData, UndoSeqScope, UNDO_SEQ_LOCAL_MAX, UNDO_SEQ_SCOPE_SHIFT};

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

pub(crate) fn put_domain_delete_undo_entry(
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
        ckbadger_store::types::UndoLogStoreTarget::Domain,
        scope,
        block_num,
        cf_name,
        key,
    );
}

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

pub(crate) fn put_addr_tx_with_undo_log(
    domain_batch: &mut StoreBatch<'_>,
    append_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
) {
    let append_key = keys::encode_addr_tx_key(lock_hash, block_num, tx_idx);
    append_batch.put_addr_tx(lock_hash, block_num, tx_idx, tx_hash);
    put_append_delete_undo_entry(
        domain_batch,
        undo_seq_by_block,
        UndoSeqScope::AppendAddrTx,
        block_num,
        ckbadger_store::CF_ADDR_TXS,
        &append_key,
    );
}

pub(crate) fn put_activity_with_undo_log(
    domain_batch: &mut StoreBatch<'_>,
    activity_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    entry: &ckbadger_store::types::ActivityEntry,
) {
    let domain_key = keys::encode_activity_key(lock_hash, block_num, tx_idx);
    activity_batch.put_activity(lock_hash, block_num, tx_idx, entry);
    put_domain_delete_undo_entry(
        domain_batch,
        undo_seq_by_block,
        UndoSeqScope::AppendActivity,
        block_num,
        ckbadger_store::CF_ACTIVITIES,
        &domain_key,
    );
}

pub(crate) fn rollback_undo_log_after_batch_cleanup(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    cleanup_tip: i64,
    context: &str,
) -> Result<()> {
    let _ = store
        .rollback_via_undo_log(append_only_store, cleanup_tip)
        .map_err(|e| {
            anyhow!(
                "failed to rollback undo log after batch cleanup: cleanup_tip={}, context={}, error={:#}",
                cleanup_tip,
                context,
                e
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::keys;
    use ckbadger_store::CkbadgerStore;

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
            data_hash: vec![],
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
        }
    }

    #[test]
    fn test_next_undo_seq_scoped_prevents_cross_phase_collisions() {
        let mut tx_scope_map = HashMap::new();
        let mut addr_scope_map = HashMap::new();
        let block_num = 42;

        let tx_seq = next_undo_seq(&mut tx_scope_map, block_num, UndoSeqScope::TxContext);
        let addr_seq = next_undo_seq(&mut addr_scope_map, block_num, UndoSeqScope::AppendAddrTx);

        assert_ne!(tx_seq, addr_seq);
        assert_eq!(
            tx_seq >> UNDO_SEQ_SCOPE_SHIFT,
            UndoSeqScope::TxContext as u64
        );
        assert_eq!(
            addr_seq >> UNDO_SEQ_SCOPE_SHIFT,
            UndoSeqScope::AppendAddrTx as u64
        );
    }

    #[test]
    fn test_rollback_via_undo_log_preserves_append_history_cfs() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();
        let lock_hash = [0x44; 32];
        let collection_id = [0x55; 24];

        let addr_keep = keys::encode_addr_tx_key(&lock_hash, 10, 0);
        let addr_drop = keys::encode_addr_tx_key(&lock_hash, 20, 0);
        append_store
            .put_cf(append_store.cf_addr_txs(), &addr_keep, &[0x01])
            .unwrap();
        append_store
            .put_cf(append_store.cf_addr_txs(), &addr_drop, &[0x02])
            .unwrap();

        let act_keep = keys::encode_activity_key(&lock_hash, 11, 0);
        let act_drop = keys::encode_activity_key(&lock_hash, 21, 0);
        domain_store
            .put_cf(domain_store.cf_activities(), &act_keep, &[0x03])
            .unwrap();
        domain_store
            .put_cf(domain_store.cf_activities(), &act_drop, &[0x04])
            .unwrap();

        let nft_keep = keys::encode_nft_collection_activity_key(&collection_id, 12, 0);
        let nft_drop = keys::encode_nft_collection_activity_key(&collection_id, 22, 0);
        append_store
            .put_cf(
                append_store.cf_nft_collection_activities(),
                &nft_keep,
                &[0x05],
            )
            .unwrap();
        append_store
            .put_cf(
                append_store.cf_nft_collection_activities(),
                &nft_drop,
                &[0x06],
            )
            .unwrap();

        let mut domain_batch = StoreBatch::new(&domain_store);
        let mut undo_seq_by_block = HashMap::new();
        put_append_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            UndoSeqScope::AppendAddrTx,
            20,
            ckbadger_store::CF_ADDR_TXS,
            &addr_drop,
        );
        put_domain_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            UndoSeqScope::AppendActivity,
            21,
            ckbadger_store::CF_ACTIVITIES,
            &act_drop,
        );
        put_append_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            UndoSeqScope::AppendNftCollectionActivity,
            22,
            ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
            &nft_drop,
        );
        domain_batch.commit().unwrap();

        domain_store
            .rollback_via_undo_log(&append_store, 15)
            .unwrap();

        assert!(append_store
            .get_cf(append_store.cf_addr_txs(), &addr_keep)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_addr_txs(), &addr_drop)
            .unwrap()
            .is_some());
        assert!(domain_store
            .get_cf(domain_store.cf_activities(), &act_keep)
            .unwrap()
            .is_some());
        assert!(domain_store
            .get_cf(domain_store.cf_activities(), &act_drop)
            .unwrap()
            .is_none());
        assert!(append_store
            .get_cf(append_store.cf_nft_collection_activities(), &nft_keep)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_nft_collection_activities(), &nft_drop)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_put_activity_with_undo_log_records_domain_target() {
        let dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock_hash = [0x44; 32];
        let entry = ckbadger_store::types::ActivityEntry {
            tx_hash: vec![0xAB; 32],
            block_number: 42,
            tx_index: 3,
            timestamp: 1_700_000_000,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        };

        let mut domain_batch = StoreBatch::new(&domain_store);
        let mut activity_batch = StoreBatch::new(&domain_store);
        let mut undo_seq_by_block = HashMap::new();
        put_activity_with_undo_log(
            &mut domain_batch,
            &mut activity_batch,
            &mut undo_seq_by_block,
            &lock_hash,
            42,
            3,
            &entry,
        );
        domain_batch.commit().unwrap();
        activity_batch.commit().unwrap();

        let iter = domain_store.iterator_cf(
            domain_store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        );
        let mut found = false;
        for item in iter {
            let (_key, value) = item.unwrap();
            let decoded: ckbadger_store::types::UndoLogEntry =
                bincode::deserialize(&value).unwrap();
            if let ckbadger_store::types::UndoLogEntry::KeyMutation {
                target_store,
                cf_name,
                ..
            } = decoded
            {
                if cf_name == ckbadger_store::CF_ACTIVITIES {
                    assert_eq!(
                        target_store,
                        ckbadger_store::types::UndoLogStoreTarget::Domain
                    );
                    found = true;
                }
            }
        }
        assert!(found, "expected activities key mutation undo entry");
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
}
