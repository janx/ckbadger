//! Live-sync writer for `.cell` (DotCell) names.
//!
//! Mirrors the did:ckb identity writer: an undo pre-image for every key it
//! touches, checked arithmetic on every counter, and the owner index keyed by
//! the 20-byte owner prefix the chain actually stores. The ring root is
//! protocol infrastructure and deliberately never becomes an identity.

use anyhow::{anyhow, bail, Result};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::store::{
    CF_DOTCELL_NAME_BY_OWNER, CF_DOTCELL_RING, CF_IDENTITY_DATA, CF_STATS_SPORE,
};
use ckbadger_store::types::{
    derive_dotcell_id, DotCellNameData, DotCellRecord, DotCellRingRoot, IdentityEntry,
    IdentityExtra, IdentityStandard, DOTCELL_SENTINEL_COLLECTION,
};

use super::spore::{IdentityOwnerKeyKind, SporeBatchState};
use super::BatchWriter;

/// `blake2b("")[..20]` — the id every namespace's ring root carries.
pub(crate) fn dotcell_root_id() -> [u8; 20] {
    derive_dotcell_id("")
}

fn checked_inc(current: i64, label: &str, id: &[u8]) -> Result<i64> {
    current.checked_add(1).ok_or_else(|| {
        anyhow!(
            "{label} overflow: id=0x{}, current={current}",
            hex::encode(id)
        )
    })
}

fn checked_dec(current: i64, label: &str, id: &[u8]) -> Result<i64> {
    if current <= 0 {
        bail!(
            "{label} underflow: id=0x{}, current={current}",
            hex::encode(id)
        );
    }
    Ok(current - 1)
}

impl BatchWriter {
    /// Write the outpoint reverse-index rows for a name cell, with undo
    /// pre-images.
    ///
    /// The rollback's identity-repair stage cleans these rows by scanning
    /// surviving `CF_IDENTITY_DATA` entries — but the undo replay runs first
    /// and has already deleted the rolled-back identity by then, so the repair
    /// never sees it. Recording the pre-images here makes the rollback exact
    /// on its own, whatever order the two stages run in.
    fn put_dotcell_outpoint_rows(
        &self,
        id: &[u8; 20],
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) {
        for key in [
            keys::encode_spore_outpoint_key(tx_hash, output_index).to_vec(),
            keys::encode_spore_outpoint_by_id_key(id, tx_hash, output_index),
        ] {
            let previous = self
                .store
                .get_cf(self.store.cf_stats_spore(), &key)
                .ok()
                .flatten();
            self.record_object_undo(
                batch,
                block_number,
                CF_STATS_SPORE,
                &key,
                previous,
                &state.undo_seq_by_block,
            );
        }
        batch.put_spore_outpoint(tx_hash, output_index, id);
        state.put_spore_outpoint(tx_hash, output_index, id);
    }

    /// A network runs exactly one `.cell` namespace. Two would make bare
    /// 20-byte ids collide on identical labels, so a second one stops the sync
    /// rather than silently merging two name spaces into one collection.
    fn guard_dotcell_namespace(
        &self,
        namespace_args: &[u8; 20],
        state: &mut SporeBatchState,
    ) -> Result<()> {
        for known in state.dotcell_known_namespaces(self.store.as_ref())? {
            if known != *namespace_args {
                bail!(
                    "a second .cell namespace appeared: known=0x{}, new=0x{} — \
                     bare 20-byte name ids would collide across namespaces",
                    hex::encode(known),
                    hex::encode(namespace_args)
                );
            }
        }
        state.remember_dotcell_namespace(*namespace_args);
        Ok(())
    }

    /// Index one `.cell` name cell (or record the ring root it is).
    pub(crate) fn insert_dotcell_name(
        &self,
        name: &DotCellNameData,
        records: &[DotCellRecord],
        namespace_args: &[u8; 20],
        tx_hash: &[u8],
        output_index: i16,
        block_number: i64,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<()> {
        self.guard_dotcell_namespace(namespace_args, state)?;
        super::ensure_outpoint_indexable_item_id(&name.id, "dotcell", tx_hash, output_index)?;

        if name.is_root() {
            // Protocol infrastructure: the ring root has no owner and is not a
            // name anyone holds. It still needs a reverse index row so its
            // re-creation resolves on the consume side.
            let previous = state.get_dotcell_ring(self.store.as_ref(), namespace_args)?;
            self.record_object_undo(
                batch,
                block_number,
                CF_DOTCELL_RING,
                namespace_args,
                previous
                    .as_ref()
                    .map(bincode::serialize)
                    .transpose()
                    .map_err(|e| {
                        anyhow!(
                            "failed to serialize DotCellRingRoot pre-image: namespace=0x{}, {e}",
                            hex::encode(namespace_args)
                        )
                    })?,
                &state.undo_seq_by_block,
            );
            let root = DotCellRingRoot {
                root_tx_hash: tx_hash.to_vec(),
                root_output_index: output_index,
                first_id: name.next_id,
                created_at_block: previous
                    .as_ref()
                    .map(|p| p.created_at_block)
                    .unwrap_or(block_number),
            };
            batch.put_dotcell_ring(namespace_args, &root);
            state.put_dotcell_ring(namespace_args, root);
            let root_id = dotcell_root_id();
            self.put_dotcell_outpoint_rows(
                &root_id,
                tx_hash,
                output_index,
                block_number,
                batch,
                state,
            );
            return Ok(());
        }

        let existing = state.get_identity(self.store.as_ref(), &name.id)?;
        if let Some(entry) = existing.as_ref() {
            if entry.standard != IdentityStandard::DotCell {
                bail!(
                    "identity id 0x{} already used by standard {} — dotcell id collision",
                    hex::encode(name.id),
                    entry.standard.as_str()
                );
            }
        }
        self.record_object_undo(
            batch,
            block_number,
            CF_IDENTITY_DATA,
            &name.id,
            existing.as_ref().and_then(|e| bincode::serialize(e).ok()),
            &state.undo_seq_by_block,
        );
        let was_live = existing.as_ref().is_some_and(|e| e.is_live);
        let old_owner20 = existing
            .as_ref()
            .filter(|_| was_live)
            .map(|e| dotcell_owner20(&e.extra, &name.id))
            .transpose()?;

        let entry = IdentityEntry {
            standard: IdentityStandard::DotCell,
            // The chain stores a 20-byte prefix; a fabricated 32-byte hash
            // would be a lock hash nobody can look up.
            owner_lock_hash: None,
            name: Some(format!("{}.cell", name.label)),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: IdentityExtra::DotCell {
                label: name.label.clone(),
                namespace_args: *namespace_args,
                layout_version: name.layout_version,
                expired_at: name.expired_at,
                owner_hash20: name.owner_hash20,
                manager_hash20: name.manager_hash20,
                next_id: name.next_id,
                records_hash: name.records_hash,
                records: records.to_vec(),
                parent_id: name.parent_id(),
            },
        };
        batch.put_identity(&name.id, &entry);
        state.put_identity(&name.id, entry);
        self.put_dotcell_outpoint_rows(&name.id, tx_hash, output_index, block_number, batch, state);

        // Owner index. The row's value is empty, so its pre-image is
        // `Some(empty)` when the row existed and `None` when it did not.
        if let Some(old) = old_owner20 {
            if old != name.owner_hash20 {
                self.record_object_undo(
                    batch,
                    block_number,
                    CF_DOTCELL_NAME_BY_OWNER,
                    &keys::encode_dotcell_name_by_owner_key(&old, &name.id),
                    Some(Vec::new()),
                    &state.undo_seq_by_block,
                );
                batch.delete_dotcell_name_by_owner(&old, &name.id);
            }
        }
        if old_owner20 != Some(name.owner_hash20) {
            self.record_object_undo(
                batch,
                block_number,
                CF_DOTCELL_NAME_BY_OWNER,
                &keys::encode_dotcell_name_by_owner_key(&name.owner_hash20, &name.id),
                None,
                &state.undo_seq_by_block,
            );
            batch.put_dotcell_name_by_owner(&name.owner_hash20, &name.id);
        }

        // Sub-names are listed under their parent through the same
        // collection index, keyed by the parent's padded id.
        if existing.is_none() {
            if let Some(parent) = name.parent_id() {
                batch.put_identity_by_collection(&keys::pad_id_32(&parent), &name.id);
            }
        }

        let collection = &DOTCELL_SENTINEL_COLLECTION;
        let mut agg = state.get_identity_agg(self.store.as_ref(), collection)?;
        if agg.standard == IdentityStandard::default() && agg.total_count == 0 {
            agg.standard = IdentityStandard::DotCell;
            agg.name = Some(".cell".to_string());
        }
        if existing.is_none() {
            batch.put_identity_by_collection(collection, &name.id);
            agg.total_count = checked_inc(agg.total_count, "dotcell total_count", &name.id)?;
            agg.live_count = checked_inc(agg.live_count, "dotcell live_count", &name.id)?;
        } else if !was_live {
            agg.live_count =
                checked_inc(agg.live_count, "dotcell live_count on reactivate", &name.id)?;
        }
        self.apply_identity_owner_transition(
            collection,
            IdentityOwnerKeyKind::Prefix20,
            old_owner20.as_ref().map(|o| &o[..]),
            Some(&name.owner_hash20[..]),
            &mut agg,
            batch,
            state,
        )?;
        state.put_identity_agg(collection, agg, batch);
        Ok(())
    }

    /// Mark a `.cell` name consumed and return the state it had, which the
    /// activity builder needs: an input cell reaches it without its data.
    pub(crate) fn consume_dotcell_name(
        &self,
        id: &[u8; 20],
        block_number: i64,
        batch: &mut StoreBatch,
        state: &mut SporeBatchState,
    ) -> Result<Option<DotCellNameData>> {
        if *id == dotcell_root_id() {
            // The ring root is re-created by every registration; it owns no
            // identity state to consume.
            return Ok(None);
        }
        let mut entry = state
            .get_identity(self.store.as_ref(), id)?
            .ok_or_else(|| {
                anyhow!(
                    "missing dotcell identity during consume: id=0x{}",
                    hex::encode(id)
                )
            })?;
        if entry.standard != IdentityStandard::DotCell {
            bail!(
                "identity id 0x{} is a {} identity, not a .cell name",
                hex::encode(id),
                entry.standard.as_str()
            );
        }
        if !entry.is_live {
            bail!(
                "dotcell identity already consumed: id=0x{}",
                hex::encode(id)
            );
        }
        let previous = dotcell_name_from_extra(&entry.extra, id)?;
        let old_owner = previous.owner_hash20;

        self.record_object_undo(
            batch,
            block_number,
            CF_IDENTITY_DATA,
            id,
            Some(bincode::serialize(&entry).map_err(|e| {
                anyhow!(
                    "failed to serialize dotcell identity pre-image: id=0x{}, {e}",
                    hex::encode(id)
                )
            })?),
            &state.undo_seq_by_block,
        );
        // `owner_hash20` stays as it was so a recycled name can still show its
        // last owner; only the live index and counters drop it.
        entry.is_live = false;
        batch.put_identity(id, &entry);
        state.put_identity(id, entry);

        self.record_object_undo(
            batch,
            block_number,
            CF_DOTCELL_NAME_BY_OWNER,
            &keys::encode_dotcell_name_by_owner_key(&old_owner, id),
            Some(Vec::new()),
            &state.undo_seq_by_block,
        );
        batch.delete_dotcell_name_by_owner(&old_owner, id);

        let collection = &DOTCELL_SENTINEL_COLLECTION;
        let mut agg = state.get_identity_agg(self.store.as_ref(), collection)?;
        agg.live_count = checked_dec(agg.live_count, "dotcell live_count on consume", id)?;
        self.apply_identity_owner_transition(
            collection,
            IdentityOwnerKeyKind::Prefix20,
            Some(&old_owner[..]),
            None,
            &mut agg,
            batch,
            state,
        )?;
        state.put_identity_agg(collection, agg, batch);
        Ok(Some(previous))
    }
}

fn dotcell_owner20(extra: &IdentityExtra, id: &[u8; 20]) -> Result<[u8; 20]> {
    match extra {
        IdentityExtra::DotCell { owner_hash20, .. } => Ok(*owner_hash20),
        other => Err(anyhow!(
            "dotcell identity 0x{} carries {:?} instead of DotCell extra",
            hex::encode(id),
            std::mem::discriminant(other)
        )),
    }
}

/// Rebuild the on-chain name data from the identity entry the indexer wrote
/// when the cell was created. This is what makes the live path's input view
/// equal to the bulk path's, which reads the same fields from stored facts.
fn dotcell_name_from_extra(extra: &IdentityExtra, id: &[u8; 20]) -> Result<DotCellNameData> {
    match extra {
        IdentityExtra::DotCell {
            label,
            layout_version,
            expired_at,
            owner_hash20,
            manager_hash20,
            next_id,
            records_hash,
            ..
        } => {
            let derived = derive_dotcell_id(label);
            if derived != *id {
                bail!(
                    "dotcell identity 0x{} stores label {:?}, which hashes to 0x{}",
                    hex::encode(id),
                    label,
                    hex::encode(derived)
                );
            }
            Ok(DotCellNameData {
                layout_version: *layout_version,
                records_hash: *records_hash,
                next_id: *next_id,
                expired_at: *expired_at,
                owner_hash20: *owner_hash20,
                manager_hash20: *manager_hash20,
                label: label.clone(),
                id: *id,
            })
        }
        other => Err(anyhow!(
            "dotcell identity 0x{} carries {:?} instead of DotCell extra",
            hex::encode(id),
            std::mem::discriminant(other)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::test_helpers::real_dotcell as fixture;
    use crate::parser::{DotCellNameData, DotCellParser};
    use crate::rpc::parse_hex_to_bytes;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{
        IdentityExtra, IdentityStandard, UndoLogEntry, DOTCELL_SENTINEL_COLLECTION,
    };
    use ckbadger_store::CkbadgerStore;
    use std::sync::Arc;

    use crate::db::writer::entity_stats::SharedEntityStatsOverlay;
    use crate::db::writer::BatchWriter;
    use crate::sync::undo::SharedUndoSeq;

    fn writer_for_test() -> (Arc<CkbadgerStore>, BatchWriter, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        (store, writer, dir)
    }

    fn new_state(writer: &BatchWriter) -> SporeBatchState {
        writer.new_spore_batch_state(SharedEntityStatsOverlay::new(), SharedUndoSeq::default())
    }

    fn mainnet_namespace() -> [u8; 20] {
        parse_hex_to_bytes(fixture::NAMESPACE_ARGS_MAINNET)
            .try_into()
            .unwrap()
    }

    fn hex20(hex: &str) -> [u8; 20] {
        parse_hex_to_bytes(hex).try_into().unwrap()
    }

    /// `support.cell`, exactly as mainnet block 20,518,306 wrote it.
    fn support_name() -> DotCellNameData {
        DotCellParser::parse_name_data(&parse_hex_to_bytes(fixture::M2_OUT1_DATA)).unwrap()
    }

    fn ring_root_name() -> DotCellNameData {
        DotCellParser::parse_name_data(&parse_hex_to_bytes(fixture::M1_OUT0_DATA)).unwrap()
    }

    fn undo_cfs(store: &CkbadgerStore) -> Vec<String> {
        let mut out = Vec::new();
        for item in store.iterator_cf(
            store.cf_reorg_undo_log_by_block(),
            rocksdb::IteratorMode::Start,
        ) {
            let (_, value) = item.unwrap();
            if let UndoLogEntry::KeyMutation { cf_name, .. } = bincode::deserialize(&value).unwrap()
            {
                out.push(cf_name);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn insert_dotcell_name_writes_entry_index_owner_row_and_agg() {
        let (store, writer, _dir) = writer_for_test();
        let name = support_name();
        let tx_hash = parse_hex_to_bytes(fixture::M2_REGISTER_SUPPORT.tx_hash);
        let owner = hex20("0x57d926a44d83fc13b21ce037b1e31f4223e3c867");

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &name,
                &[],
                &mainnet_namespace(),
                &tx_hash,
                1,
                20_518_306,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let entry = store
            .get_identity(&name.id)
            .unwrap()
            .expect("identity entry under the 20-byte name id");
        assert_eq!(entry.standard, IdentityStandard::DotCell);
        assert_eq!(entry.name.as_deref(), Some("support.cell"));
        assert!(entry.is_live);
        assert_eq!(
            entry.owner_lock_hash, None,
            "the chain gives a 20-byte prefix, never a full lock hash"
        );
        assert_eq!(entry.created_at_block, 20_518_306);
        assert_eq!(entry.created_at_tx, tx_hash);
        match &entry.extra {
            IdentityExtra::DotCell {
                label,
                namespace_args,
                layout_version,
                expired_at,
                owner_hash20,
                manager_hash20,
                next_id,
                records,
                parent_id,
                ..
            } => {
                assert_eq!(label, "support");
                assert_eq!(*namespace_args, mainnet_namespace());
                assert_eq!(*layout_version, 3);
                assert_eq!(*expired_at, 1_821_507_678);
                assert_eq!(*owner_hash20, owner);
                assert_eq!(*manager_hash20, owner);
                assert_eq!(
                    next_id.to_vec(),
                    parse_hex_to_bytes("0x65b5fe7e7070b506f69bd8cabf9e427211106645")
                );
                assert!(records.is_empty());
                assert_eq!(*parent_id, None);
            }
            other => panic!("expected DotCell extra, got {other:?}"),
        }

        assert_eq!(
            store
                .list_identity_ids_by_collection(&DOTCELL_SENTINEL_COLLECTION, None, 10)
                .unwrap(),
            vec![name.id.to_vec()]
        );
        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner, None, 10)
                .unwrap(),
            vec![name.id]
        );
        assert_eq!(
            store
                .get_dotcell_owner20_count(&DOTCELL_SENTINEL_COLLECTION, &owner)
                .unwrap(),
            1
        );

        let agg = store
            .get_identity_collection_aggregate(&DOTCELL_SENTINEL_COLLECTION)
            .unwrap()
            .expect("dotcell aggregate");
        assert_eq!(agg.standard, IdentityStandard::DotCell);
        assert_eq!(agg.name.as_deref(), Some(".cell"));
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 1);
        assert_eq!(agg.holders_count, 1);

        assert_eq!(
            writer.get_spore_id_by_outpoint(&tx_hash, 1).unwrap(),
            Some(name.id.to_vec()),
            "the per-item activity feed reads the outpoint reverse index"
        );
    }

    #[test]
    fn ring_root_is_recorded_but_not_an_identity() {
        let (store, writer, _dir) = writer_for_test();
        let root = ring_root_name();
        let tx_hash = parse_hex_to_bytes(fixture::M1_RING_ROOT.tx_hash);

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &root,
                &[],
                &mainnet_namespace(),
                &tx_hash,
                0,
                20_515_882,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        assert!(
            store.get_identity(&root.id).unwrap().is_none(),
            "the ring root is protocol infrastructure, not a name anyone holds"
        );
        assert!(store
            .get_identity_collection_aggregate(&DOTCELL_SENTINEL_COLLECTION)
            .unwrap()
            .is_none());

        let ring = store
            .get_dotcell_ring(&mainnet_namespace())
            .unwrap()
            .expect("ring root row");
        assert_eq!(ring.root_tx_hash, tx_hash);
        assert_eq!(ring.root_output_index, 0);
        assert_eq!(ring.first_id, root.next_id);
        assert_eq!(ring.created_at_block, 20_515_882);

        assert_eq!(
            writer.get_spore_id_by_outpoint(&tx_hash, 0).unwrap(),
            Some(dotcell_root_id().to_vec()),
            "the root cell still needs a reverse index so its re-creation resolves"
        );
    }

    #[test]
    fn second_namespace_args_is_an_error() {
        let (_store, writer, _dir) = writer_for_test();
        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &ring_root_name(),
                &[],
                &mainnet_namespace(),
                &parse_hex_to_bytes(fixture::M1_RING_ROOT.tx_hash),
                0,
                20_515_882,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        let err = writer
            .insert_dotcell_name(
                &support_name(),
                &[],
                &[0xFF; 20],
                &parse_hex_to_bytes(fixture::M2_REGISTER_SUPPORT.tx_hash),
                1,
                20_518_306,
                &mut batch,
                &mut state,
            )
            .unwrap_err();
        assert!(err.to_string().contains("namespace"), "{err}");
    }

    #[test]
    fn consume_then_reinsert_moves_owner_and_keeps_total() {
        let (store, writer, _dir) = writer_for_test();
        let mut name = support_name();
        let owner_a = name.owner_hash20;
        let tx_hash = parse_hex_to_bytes(fixture::M2_REGISTER_SUPPORT.tx_hash);

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &name,
                &[],
                &mainnet_namespace(),
                &tx_hash,
                1,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        // Transfer: the same id is consumed and re-created under a new owner.
        let owner_b = hex20("0xac55d7dab2e9a4b85775a811bb4063e94cc98182");
        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        let prev = writer
            .consume_dotcell_name(&name.id, 101, &mut batch, &mut state)
            .unwrap()
            .expect("consume returns the state the name had");
        assert_eq!(prev.owner_hash20, owner_a);
        name.owner_hash20 = owner_b;
        name.manager_hash20 = owner_b;
        writer
            .insert_dotcell_name(
                &name,
                &[],
                &mainnet_namespace(),
                &[0xEE; 32],
                0,
                101,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_identity_collection_aggregate(&DOTCELL_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 1, "the same name, not a second one");
        assert_eq!(agg.live_count, 1);
        assert_eq!(agg.holders_count, 1);
        assert!(store
            .list_dotcell_names_by_owner20(&owner_a, None, 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner_b, None, 10)
                .unwrap(),
            vec![name.id]
        );
        assert_eq!(
            store
                .get_dotcell_owner20_count(&DOTCELL_SENTINEL_COLLECTION, &owner_a)
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .get_dotcell_owner20_count(&DOTCELL_SENTINEL_COLLECTION, &owner_b)
                .unwrap(),
            1
        );
    }

    #[test]
    fn consume_dotcell_drops_the_owner_row_and_live_count() {
        let (store, writer, _dir) = writer_for_test();
        let name = support_name();
        let owner = name.owner_hash20;

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &name,
                &[],
                &mainnet_namespace(),
                &[0xAA; 32],
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .consume_dotcell_name(&name.id, 101, &mut batch, &mut state)
            .unwrap();
        batch.commit().unwrap();

        let entry = store.get_identity(&name.id).unwrap().unwrap();
        assert!(!entry.is_live);
        match &entry.extra {
            IdentityExtra::DotCell { owner_hash20, .. } => assert_eq!(
                *owner_hash20, owner,
                "a recycled name keeps its last owner for display"
            ),
            other => panic!("{other:?}"),
        }
        assert!(store
            .list_dotcell_names_by_owner20(&owner, None, 10)
            .unwrap()
            .is_empty());
        let agg = store
            .get_identity_collection_aggregate(&DOTCELL_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);

        let err = writer
            .consume_dotcell_name(
                &name.id,
                102,
                &mut StoreBatch::new(writer.store()),
                &mut new_state(&writer),
            )
            .unwrap_err();
        assert!(err.to_string().contains("already consumed"), "{err}");
    }

    #[test]
    fn consuming_a_name_that_was_never_indexed_is_an_error() {
        let (_store, writer, _dir) = writer_for_test();
        let err = writer
            .consume_dotcell_name(
                &[0x11; 20],
                100,
                &mut StoreBatch::new(writer.store()),
                &mut new_state(&writer),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("missing dotcell identity"),
            "{err}"
        );
    }

    #[test]
    fn every_dotcell_write_records_undo_previous_values() {
        let (store, writer, _dir) = writer_for_test();
        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &ring_root_name(),
                &[],
                &mainnet_namespace(),
                &[0xAA; 32],
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_dotcell_name(
                &support_name(),
                &[],
                &mainnet_namespace(),
                &[0xBB; 32],
                0,
                100,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let cfs = undo_cfs(&store);
        for expected in [
            ckbadger_store::CF_IDENTITY_DATA,
            ckbadger_store::CF_DOTCELL_NAME_BY_OWNER,
            ckbadger_store::CF_DOTCELL_RING,
        ] {
            assert!(
                cfs.iter().any(|cf| cf == expected),
                "no undo pre-image recorded for {expected}: {cfs:?}"
            );
        }
    }

    #[test]
    fn a_name_id_already_used_by_another_standard_is_an_error() {
        let (_store, writer, _dir) = writer_for_test();
        let name = support_name();

        let mut batch = StoreBatch::new(writer.store());
        batch.put_identity(
            &name.id,
            &ckbadger_store::types::IdentityEntry {
                standard: IdentityStandard::DidCkb,
                owner_lock_hash: Some(vec![0x33; 32]),
                name: None,
                is_live: true,
                created_at_block: 1,
                created_at_tx: vec![0x44; 32],
                extra: IdentityExtra::DidCkb,
            },
        );
        batch.commit().unwrap();

        let err = writer
            .insert_dotcell_name(
                &name,
                &[],
                &mainnet_namespace(),
                &[0xAA; 32],
                0,
                100,
                &mut StoreBatch::new(writer.store()),
                &mut new_state(&writer),
            )
            .unwrap_err();
        assert!(err.to_string().contains("collision"), "{err}");
    }

    #[test]
    fn sub_name_is_indexed_under_its_parent_id_too() {
        let (store, writer, _dir) = writer_for_test();
        let sub =
            DotCellParser::parse_name_data(&parse_hex_to_bytes(fixture::T3_OUT1_DATA)).unwrap();
        let parent_id = sub.parent_id().expect("sub-name has a parent");
        let namespace: [u8; 20] = parse_hex_to_bytes(fixture::NAMESPACE_ARGS_TESTNET)
            .try_into()
            .unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = new_state(&writer);
        writer
            .insert_dotcell_name(
                &sub,
                &[],
                &namespace,
                &[0xAA; 32],
                1,
                22_367_979,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        assert_eq!(
            store
                .list_identity_ids_by_collection(&parent_id, None, 10)
                .unwrap(),
            vec![sub.id.to_vec()],
            "the parent's children are listed through the same collection index"
        );
        assert_eq!(
            store
                .list_identity_ids_by_collection(&DOTCELL_SENTINEL_COLLECTION, None, 10)
                .unwrap(),
            vec![sub.id.to_vec()],
            "and it is still a name of the .cell collection"
        );
    }
}
