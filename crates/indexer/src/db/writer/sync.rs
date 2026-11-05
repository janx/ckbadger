use anyhow::{bail, Result};
use tracing::{info, warn};

use ckbadger_store::keys;

use super::{reorg::RollbackCause, BatchWriter, ContractRequirement};

const STARTUP_CONTINUITY_SAMPLE_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupContinuityProbe {
    pub startup_tip: i64,
    pub header_tip: Option<i64>,
    pub tx_floor: Option<i64>,
    pub tx_tip: Option<i64>,
    pub first_header_gap: Option<i64>,
    pub recent_window_start: i64,
    pub recent_window_end: i64,
    pub missing_header_sample: Vec<i64>,
    pub missing_tx_block0_sample: Vec<i64>,
    pub missing_tx_incomplete_sample: Vec<(i64, i32, i32)>,
    pub full_header_gap_scan: bool,
}

impl StartupContinuityProbe {
    pub fn has_inconsistency(&self) -> bool {
        self.first_header_gap.is_some()
            || self
                .header_tip
                .zip(self.tx_tip)
                .is_some_and(|(header_tip, tx_tip)| header_tip != tx_tip)
            || self.tx_floor.is_some_and(|tx_floor| tx_floor > 0)
            || !self.missing_header_sample.is_empty()
            || !self.missing_tx_block0_sample.is_empty()
            || !self.missing_tx_incomplete_sample.is_empty()
    }
}

impl BatchWriter {
    pub fn find_last_consistent_block(&self) -> Result<Option<i64>> {
        // Get max block from block_headers CF
        let max_block = self.store.get_sync_tip_block()?.map(|(num, _)| num);

        // Get max block from tx_index CF
        let max_tx_block = self.tx_index_boundary_block(rocksdb::IteratorMode::End);

        match (max_block, max_tx_block) {
            (Some(mb), Some(mtb)) => {
                if mb > mtb {
                    warn!(
                        "Data inconsistency detected: blocks up to {} but transactions only up to {}",
                        mb, mtb
                    );
                    Ok(Some(mtb))
                } else {
                    Ok(Some(mb))
                }
            }
            (Some(mb), None) => {
                warn!(
                    "Data inconsistency: blocks exist up to {} but no transactions found",
                    mb
                );
                Ok(Some(-1))
            }
            (None, _) => Ok(None),
        }
    }

    pub fn probe_startup_continuity(
        &self,
        startup_tip: i64,
        window_size: i64,
        include_full_header_gap_scan: bool,
    ) -> Result<StartupContinuityProbe> {
        if startup_tip < -1 {
            bail!(
                "invalid startup tip for continuity probe: startup_tip={} (expected >= -1)",
                startup_tip
            );
        }
        if window_size <= 0 {
            bail!(
                "invalid continuity probe window_size={} (expected > 0)",
                window_size
            );
        }

        let header_tip = self.store.get_sync_tip_block()?.map(|(num, _)| num);
        let tx_floor = self.tx_index_boundary_block(rocksdb::IteratorMode::Start);
        let tx_tip = self.tx_index_boundary_block(rocksdb::IteratorMode::End);

        let mut missing_header_sample = Vec::new();
        let mut missing_tx_block0_sample = Vec::new();
        let mut missing_tx_incomplete_sample = Vec::new();
        let (recent_window_start, recent_window_end) = match header_tip {
            Some(header_tip) if header_tip >= 0 => {
                let window_end = std::cmp::min(startup_tip.max(0), header_tip);
                let window_start = (window_end - window_size + 1).max(0);
                for block_num in window_start..=window_end {
                    let header = match self.store.get_block_header(block_num)? {
                        Some(h) => h,
                        None => {
                            if missing_header_sample.len() < STARTUP_CONTINUITY_SAMPLE_LIMIT {
                                missing_header_sample.push(block_num);
                            }
                            continue;
                        }
                    };
                    if self.store.get_tx_index(block_num, 0)?.is_none() {
                        if missing_tx_block0_sample.len() < STARTUP_CONTINUITY_SAMPLE_LIMIT {
                            missing_tx_block0_sample.push(block_num);
                        }
                        continue;
                    }
                    // Verify the last expected tx also exists to catch half-committed blocks
                    let expected_tx_count = header.transactions_count;
                    if expected_tx_count > 1 {
                        let last_tx_idx = expected_tx_count - 1;
                        if self.store.get_tx_index(block_num, last_tx_idx)?.is_none()
                            && missing_tx_incomplete_sample.len() < STARTUP_CONTINUITY_SAMPLE_LIMIT
                        {
                            let mut last_found: i32 = 0;
                            for idx in (1..last_tx_idx).rev() {
                                if self.store.get_tx_index(block_num, idx)?.is_some() {
                                    last_found = idx;
                                    break;
                                }
                            }
                            missing_tx_incomplete_sample.push((
                                block_num,
                                expected_tx_count,
                                last_found,
                            ));
                        }
                    }
                }
                (window_start, window_end)
            }
            _ => (0, -1),
        };

        let first_header_gap = if include_full_header_gap_scan {
            self.store.find_first_block_header_gap()?
        } else {
            None
        };

        Ok(StartupContinuityProbe {
            startup_tip,
            header_tip,
            tx_floor,
            tx_tip,
            first_header_gap,
            recent_window_start,
            recent_window_end,
            missing_header_sample,
            missing_tx_block0_sample,
            missing_tx_incomplete_sample,
            full_header_gap_scan: include_full_header_gap_scan,
        })
    }

    pub fn init_sync_start(&self, start_block: i64, is_bulk_sync: bool) -> Result<()> {
        self.init_sync_start_with_options(start_block, is_bulk_sync, false)
    }

    pub fn init_sync_start_with_options(
        &self,
        start_block: i64,
        is_bulk_sync: bool,
        force_cleanup: bool,
    ) -> Result<()> {
        if start_block < -1 {
            bail!(
                "invalid startup sync tip: start_block={} (expected >= -1)",
                start_block
            );
        }
        let next_block = start_block + 1;
        let has_partial_data = self.has_partial_data_after_block(start_block)?;
        let cleanup_reason = if force_cleanup && has_partial_data {
            "forced_and_partial_data_detected"
        } else if force_cleanup {
            "forced_after_unclean_shutdown"
        } else if has_partial_data {
            "partial_data_detected"
        } else {
            "no_cleanup_needed"
        };
        info!(
            start_block,
            next_block, force_cleanup, has_partial_data, cleanup_reason, "Startup cleanup decision"
        );
        if has_partial_data {
            info!(
                start_block,
                next_block,
                force_cleanup,
                has_partial_data,
                cleanup_reason,
                "Cleaning up partial data before sync start"
            );

            let rollback_target =
                if start_block >= 0 && self.store.get_block_header(start_block)?.is_none() {
                    warn!(
                        start_block,
                        "Startup cleanup tip header missing; rolling back to -1 for full cleanup"
                    );
                    -1
                } else {
                    start_block
                };
            // The coverage floor matters most here: the branch above resets
            // to -1 whenever the tip header is missing, which no journal
            // reaches on a store whose floor is above it.
            self.rollback_to(
                rollback_target,
                ContractRequirement::OptionalOnFreshStore,
                "startup cleanup rollback target",
                RollbackCause::Cleanup {
                    target: rollback_target,
                },
                || Ok(()),
            )?;
            info!(
                start_block,
                rollback_target, next_block, cleanup_reason, "Startup cleanup complete"
            );
        } else if force_cleanup {
            info!(
                start_block,
                next_block,
                force_cleanup,
                has_partial_data,
                cleanup_reason,
                "Skipping rollback cleanup: force_cleanup requested but no partial data detected (atomic WriteBatch guarantees consistency)"
            );
        } else {
            info!(
                start_block,
                next_block, cleanup_reason, "Skipping startup rollback cleanup"
            );
        }

        // Re-derive the DAO tip summary (singletons and the open day's
        // snapshot) unconditionally on startup, so a crash between a batch
        // commit and its tip summary leaves no stale row.  Idempotent.
        self.refresh_latest_dao_statistics()?;

        // Align persistent sync tip to the startup tip to avoid stale sync_status metadata.
        let (tip_number, tip_hash) = if let Some((num, header)) = self.store.get_sync_tip_block()? {
            (num, Some(header.hash))
        } else {
            (0, None)
        };
        self.store.update_sync_status(|status| {
            status.tip_block_number = tip_number;
            match &tip_hash {
                Some(hash) => status.tip_block_hash = hash.clone(),
                None if tip_number == 0 => status.tip_block_hash.clear(),
                None => {}
            }
            status.init_sync_start(start_block, is_bulk_sync);
        })?;

        if let Some(cache) = &self.cache_invalidator {
            let cache = cache.clone();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    cache
                        .update_sync_status(|status| {
                            status.init_sync_start(start_block, is_bulk_sync);
                        })
                        .await;
                });
            });
        }
        Ok(())
    }

    pub fn needs_startup_cleanup(&self, start_block: i64) -> Result<bool> {
        self.needs_startup_cleanup_with_force(start_block, false)
    }

    pub fn needs_startup_cleanup_with_force(
        &self,
        start_block: i64,
        force_cleanup: bool,
    ) -> Result<bool> {
        if force_cleanup {
            return Ok(true);
        }
        self.has_partial_data_after_block(start_block)
    }

    pub fn cleanup_batch_range(
        &self,
        start_block: i64,
        end_block: i64,
    ) -> Result<ckbadger_store::RollbackResult> {
        info!(
            "Cleaning up partial batch data for blocks {} to {}",
            start_block, end_block
        );

        let cleanup_tip = start_block - 1;
        let ((), result) = self.rollback_to(
            cleanup_tip,
            ContractRequirement::OptionalOnFreshStore,
            "batch range cleanup target",
            RollbackCause::Cleanup {
                target: cleanup_tip,
            },
            || Ok(()),
        )?;

        info!(
            "Batch cleanup complete for blocks {} to {}",
            start_block, end_block
        );
        Ok(result)
    }

    fn has_partial_data_after_block(&self, start_block: i64) -> Result<bool> {
        let start_key = keys::encode_block_num(start_block + 1);

        let header_iter = self.store.iterator_cf(
            self.store.cf_block_headers(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        if header_iter.flatten().next().is_some() {
            return Ok(true);
        }

        let tx_iter = self.store.iterator_cf(
            self.store.cf_tx_index(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in tx_iter.flatten().take(1) {
            let (key, _) = item;
            if key.len() >= 8 && keys::decode_block_num(&key[..8]) > start_block {
                return Ok(true);
            }
        }

        if self.store.has_journal_after(start_block)? {
            return Ok(true);
        }

        // Check the tracker cursors for blocks beyond start_block. They are
        // written in the same atomic batch as the blocks they processed, so a
        // cursor ahead of the tip means rows of an unfinished tail survived.
        let hodl_last = self
            .store
            .get_hodl_cursor()?
            .map(|cursor| cursor.last_processed_block);
        let cell_dist_last = self
            .store
            .get_cell_dist_cursor()?
            .map(|cursor| cursor.last_processed_block);
        if hodl_last.is_some_and(|last| last > start_block)
            || cell_dist_last.is_some_and(|last| last > start_block)
        {
            return Ok(true);
        }

        Ok(false)
    }

    fn tx_index_boundary_block(&self, mode: rocksdb::IteratorMode) -> Option<i64> {
        let iter = self.store.iterator_cf(self.store.cf_tx_index(), mode);
        for item in iter.flatten().take(1) {
            let (key, _) = item;
            if key.len() >= 8 {
                return Some(keys::decode_block_num(&key[..8]));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ckbadger_store::{
        keys::sync_meta_keys, AddressBalance, CachedBlockHeader, CkbadgerStore, StoreBatch,
        SyncStatus, TxIndexEntry,
    };
    use tempfile::TempDir;

    use super::{BatchWriter, RollbackCause};

    fn setup() -> (TempDir, Arc<CkbadgerStore>, Arc<CkbadgerStore>, BatchWriter) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path().join("domain")).unwrap());
        let append_store =
            Arc::new(CkbadgerStore::open_append_only(dir.path().join("append")).unwrap());
        // The indexer always derives the genesis baseline at block 0 before any
        // stats refresh runs; seed the mainnet values so the estimated-APC path
        // (which reads GenesisBaseline::total_issuance) can compute.
        store
            .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
                // Exact mainnet genesis DAO `C` (not the rounded 33.6B).
                total_issuance: 3_360_000_145_238_488_200,
                burnt: 840_000_000_000_000_000,
                virtual_occupied: 504_000_000_000_000_000,
            })
            .unwrap();
        // Every rollback ends with the DAO tip summary, whose open day's
        // miner series needs the consensus secondary epoch reward the indexer
        // persists at startup.
        store
            .set_secondary_epoch_reward(61_369_863_013_698)
            .unwrap();
        let writer = BatchWriter::new(store.clone(), append_store.clone());
        (dir, store, append_store, writer)
    }

    fn make_header(hash_byte: u8, ts_ms: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![hash_byte; 32],
            parent_hash: vec![0u8; 32],
            timestamp: ts_ms,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            // A valid DAO field (C, AR, S, U): the DAO tip summary every
            // rollback ends with observes the tip with it.
            dao: {
                let mut dao = vec![0; 32];
                dao[0..8].copy_from_slice(&3_360_000_000_000_000_000u64.to_le_bytes());
                dao[8..16].copy_from_slice(&10_000_000_000_000_000u64.to_le_bytes());
                dao[24..32].copy_from_slice(&100_000_000_000_000u64.to_le_bytes());
                dao
            },
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1a08a97e,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: ts_ms,
        }
    }

    fn make_tx_index_entry() -> TxIndexEntry {
        TxIndexEntry {
            tx_hash: [0u8; 32],
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1,
            tx_size: 1,
            cycles: None,
            semantic_tags: 0,
        }
    }

    fn make_tx_index_entry_with_counts(inputs_count: i16, outputs_count: i16) -> TxIndexEntry {
        TxIndexEntry {
            tx_hash: [0u8; 32],
            inputs_count,
            outputs_count,
            ..make_tx_index_entry()
        }
    }

    fn seed_ten_second_histogram(store: &CkbadgerStore) {
        let key = ckbadger_store::keys::encode_stats_key(
            ckbadger_store::keys::STATS_PREFIX_BLOCK_TIME_DIST,
            &10i32.to_be_bytes(),
        );
        store
            .put_cf(store.cf_stats_chain(), &key, &1i32.to_le_bytes())
            .unwrap();
    }

    /// One block as the live writer commits it: its header, `rows`, and the
    /// sync tip it advances to (`tip_block_*`; `status` adjusts the running
    /// totals), all in the block's journaled scope.
    fn commit_journaled_sync_block(
        store: &CkbadgerStore,
        height: i64,
        header: &CachedBlockHeader,
        status: impl FnOnce(&mut SyncStatus),
    ) {
        let mut sync_status = store.get_sync_status().unwrap();
        sync_status.tip_block_number = height;
        sync_status.tip_block_hash = header.hash.clone();
        status(&mut sync_status);
        ckbadger_store::commit_journaled_block(store, height, |batch| {
            batch.put_block_header(height, header);
            batch.put_sync_meta(
                sync_meta_keys::SYNC_STATUS,
                &bincode::serialize(&sync_status).unwrap(),
            );
        })
        .unwrap();
    }

    fn seed_pending_reorg(store: &CkbadgerStore, superseded: bool) -> String {
        let key = ckbadger_store::keys::encode_reorg_event_key(1000, &[1; 16]).unwrap();
        let event = ckbadger_store::ReorgEvent {
            detected_at: 1,
            kind: ckbadger_store::ReorgEventKind::Automatic,
            fork_point: 8,
            fork_point_hash: vec![8; 32],
            old_tip: 10,
            old_tip_hash: vec![0xa0; 32],
            first_orphan_hash: None,
            new_tip: 11,
            new_tip_hash: vec![0xb1; 32],
            depth: 2,
            orphaned_blocks: 2,
            orphaned_txs: 0,
            reverted_by_event: None,
            superseded_by_tip_hash: superseded.then(|| vec![0xb2; 32]),
            reconcile_interrupted: false,
            reverses_events: Vec::new(),
        };
        let mut batch = StoreBatch::new(store);
        batch.put_sync_meta(key.as_bytes(), &bincode::serialize(&event).unwrap());
        batch.put_sync_meta(sync_meta_keys::REORG_RECONCILE_PENDING, key.as_bytes());
        if superseded {
            // A superseded observation can remain pending while an older
            // abandoned tip is still above the canonical replay height.
            let prior_key = ckbadger_store::keys::encode_reorg_event_key(500, &[2; 16]).unwrap();
            let prior = ckbadger_store::ReorgEvent {
                detected_at: 0,
                old_tip: 12,
                old_tip_hash: vec![0xd2; 32],
                new_tip: 10,
                new_tip_hash: vec![0xa0; 32],
                depth: 4,
                orphaned_blocks: 4,
                superseded_by_tip_hash: None,
                ..event
            };
            batch.put_sync_meta(prior_key.as_bytes(), &bincode::serialize(&prior).unwrap());
        }
        batch.commit().unwrap();
        // The chain 0..=11 as the live writer leaves it: each header and the
        // sync tip it advances to in its block's journaled scope.
        let mut parent = vec![0; 32];
        for height in 0..=11 {
            let hash = match height {
                9 => 0xb9,
                10 => 0xb0,
                11 if superseded => 0xb2,
                11 => 0xb1,
                _ => height as u8,
            };
            let mut header = make_header(hash, 1_700_000_000_000);
            header.parent_hash = parent;
            header.transactions_count = 0;
            parent = header.hash.clone();
            commit_journaled_sync_block(store, height, &header, |_| {});
        }
        // Startup and execute_reorg end with the DAO tip summary: every block
        // here is on one UTC+8 day that starts at genesis, so it computes the
        // open day from the zero baseline and needs no earlier row.
        let histogram_key = ckbadger_store::keys::encode_stats_key(
            ckbadger_store::keys::STATS_PREFIX_BLOCK_TIME_DIST,
            &0_i32.to_be_bytes(),
        );
        store
            .put_cf(
                store.cf_stats_chain(),
                &histogram_key,
                &11_i32.to_le_bytes(),
            )
            .unwrap();
        key
    }

    #[test]
    fn interrupted_rollback_survives_restart_without_misattributing_supersession() {
        for interrupt_pending in [false, true] {
            let target = 7;
            let (dir, store, append, writer) = setup();
            let key = seed_pending_reorg(&store, false);
            append
                .put_cf(append.cf_cells(), &[7; 36], b"immutable")
                .unwrap();
            // Simulate the crash window: old E0 is pending, rollback commits,
            // but no E1 detection event or replacement marker is ever written.
            if interrupt_pending {
                writer.begin_rollback(RollbackCause::NewForkEvent).unwrap();
            }
            store.rollback_to_block(target).unwrap();
            drop(writer);
            drop(store);
            let store = Arc::new(CkbadgerStore::open_domain(dir.path().join("domain")).unwrap());
            let writer = BatchWriter::new(store.clone(), append.clone());
            let mut batch = StoreBatch::new(&store);
            let mut parent = store.get_block_header(target).unwrap().unwrap().hash;
            for height in target + 1..=12 {
                let mut header = make_header(0xc0 + height as u8, 1_700_000_000_000);
                header.parent_hash = parent;
                header.transactions_count = 0;
                parent = header.hash.clone();
                batch.put_block_header(height, &header);
            }
            batch.commit().unwrap();
            store.update_sync_tip(12, &parent, 0).unwrap();
            assert_ne!(
                store.get_block_header(8).unwrap().unwrap().hash,
                vec![8; 32]
            );
            let result = writer.reconcile_reorg_events();
            let marker = store
                .get_cf(
                    store.cf_sync_meta(),
                    sync_meta_keys::REORG_RECONCILE_PENDING,
                )
                .unwrap();
            if interrupt_pending {
                result.unwrap();
                assert_eq!(marker, None);
            } else {
                // Control: the same crash/replay really reaches the retained
                // fork-point invariant unless the rollback clears E0 first.
                let error = result.unwrap_err().to_string();
                assert!(error.contains("fork point hash mismatch"), "{error}");
                assert!(error.contains(&format!("event={key} block=8")), "{error}");
                assert_eq!(marker, Some(key.as_bytes().to_vec()));
            }
            let event = store.get_reorg_event(1000).unwrap().unwrap();
            assert_eq!(event.key, key);
            assert_eq!(event.event.reconcile_interrupted, interrupt_pending);
            assert_eq!(event.event.superseded_by_tip_hash, None);
            assert_eq!(
                append.get_cf(append.cf_cells(), &[7; 36]).unwrap().unwrap(),
                b"immutable"
            );
            assert_eq!(
                append
                    .iterator_cf(append.cf_cells(), rocksdb::IteratorMode::Start)
                    .count(),
                1
            );
        }
    }

    fn seed_full_entity_stats_coverage(store: &CkbadgerStore) {
        store
            .put_entity_stats_undo_contract(&ckbadger_store::EntityStatsUndoContract {
                version: ckbadger_store::ENTITY_STATS_UNDO_CONTRACT_VERSION,
                coverage_floor_block: 0,
                updated_at_block: 11,
            })
            .unwrap();
    }

    /// W5: an automatic switch records the first block it dropped — height
    /// `fork_point + 1` of the abandoned branch — read from the store's own
    /// headers before the rollback deletes them. A one-block switch's first
    /// orphan is its old tip; a three-block switch's is two blocks below it.
    #[tokio::test]
    async fn execute_reorg_records_the_first_orphan_of_the_dropped_branch() {
        for (fork_point, depth) in [(10, 1), (8, 3)] {
            let (_dir, store, append, writer) = setup();
            seed_pending_reorg(&store, false);
            seed_full_entity_stats_coverage(&store);
            append
                .put_cf(append.cf_cells(), &[7; 36], b"immutable")
                .unwrap();
            let fork_hash = store.get_block_header(fork_point).unwrap().unwrap().hash;
            let first_orphan: [u8; 32] = store
                .get_block_header(fork_point + 1)
                .unwrap()
                .unwrap()
                .hash
                .try_into()
                .unwrap();

            let result = writer
                .execute_reorg(fork_point, &fork_hash, 11, &[0xb1; 32], 12, &[0xc2; 32])
                .await
                .unwrap();

            assert_eq!(result.depth, depth);
            let latest = store.get_latest_reorg_event().unwrap().unwrap();
            assert_eq!(latest.event.fork_point, fork_point);
            assert_eq!(
                latest.event.first_orphan_hash,
                Some(first_orphan),
                "{depth}-block switch at fork point {fork_point}"
            );
            assert_eq!(
                first_orphan.as_slice() == latest.event.old_tip_hash.as_slice(),
                depth == 1,
                "only a one-block switch's first orphan is its old tip"
            );
            assert!(store.get_block_header(fork_point + 1).unwrap().is_none());
            assert_append_payload_unchanged(&append);
        }
    }

    /// The recorded branch must link: the first orphan's parent is the fork
    /// point and the walk up its children reaches the recorded old tip. A
    /// mismatch is an error before anything is rolled back or recorded.
    #[tokio::test]
    async fn execute_reorg_refuses_an_old_branch_that_does_not_link() {
        for (fork_hash, old_tip_hash, expected) in [
            (vec![0xee; 32], vec![0xb1; 32], "parent"),
            (vec![8; 32], vec![0xee; 32], "old tip"),
        ] {
            let (_dir, store, append, writer) = setup();
            let pending = seed_pending_reorg(&store, false);
            seed_full_entity_stats_coverage(&store);
            append
                .put_cf(append.cf_cells(), &[7; 36], b"immutable")
                .unwrap();

            let err = writer
                .execute_reorg(8, &fork_hash, 11, &old_tip_hash, 12, &[0xc2; 32])
                .await
                .err()
                .unwrap_or_else(|| panic!("an unlinked {expected} must be refused"))
                .to_string();

            assert!(
                err.contains("first orphan")
                    && err.contains("fork point 8")
                    && err.contains(expected),
                "{err}"
            );
            assert!(
                store.get_block_header(9).unwrap().is_some(),
                "nothing rolled back"
            );
            let latest = store.get_latest_reorg_event().unwrap().unwrap();
            assert_eq!(latest.key, pending, "no event recorded");
            assert!(!latest.event.reconcile_interrupted);
            assert_append_payload_unchanged(&append);
        }
    }

    #[tokio::test]
    async fn execute_reorg_interrupts_pending_reconciliation_and_replaces_marker() {
        for target in [7, 8, 9] {
            let (_dir, store, append, writer) = setup();
            let first_key = seed_pending_reorg(&store, false);
            store
                .put_entity_stats_undo_contract(&ckbadger_store::EntityStatsUndoContract {
                    version: ckbadger_store::ENTITY_STATS_UNDO_CONTRACT_VERSION,
                    coverage_floor_block: 0,
                    updated_at_block: 11,
                })
                .unwrap();
            append
                .put_cf(append.cf_cells(), &[7; 36], b"immutable")
                .unwrap();
            let fork_hash = store.get_block_header(target).unwrap().unwrap().hash;

            let result = writer
                .execute_reorg(target, &fork_hash, 11, &[0xb1; 32], 12, &[0xc2; 32])
                .await
                .unwrap();

            assert_eq!(i64::from(result.orphaned_blocks), 11 - target);
            assert!(store.get_block_header(target + 1).unwrap().is_none());
            assert_eq!(store.get_sync_tip().unwrap(), (target, Some(fork_hash)));
            let first = store.get_reorg_event(1000).unwrap().unwrap();
            assert_eq!(first.key, first_key);
            assert!(
                first.event.reconcile_interrupted,
                "rollback target={target}"
            );
            assert_eq!(first.event.superseded_by_tip_hash, None);
            let latest = store.get_latest_reorg_event().unwrap().unwrap();
            assert_ne!(latest.key, first_key);
            assert_eq!(latest.event.fork_point, target);
            assert!(!latest.event.reconcile_interrupted);
            assert_eq!(
                store
                    .get_cf(
                        store.cf_sync_meta(),
                        sync_meta_keys::REORG_RECONCILE_PENDING
                    )
                    .unwrap(),
                Some(latest.key.as_bytes().to_vec())
            );
            assert_append_payload_unchanged(&append);
        }
    }

    fn assert_append_payload_unchanged(append: &CkbadgerStore) {
        assert_eq!(
            append.get_cf(append.cf_cells(), &[7; 36]).unwrap().unwrap(),
            b"immutable"
        );
        assert_eq!(
            append
                .iterator_cf(append.cf_cells(), rocksdb::IteratorMode::Start)
                .count(),
            1
        );
    }

    fn assert_cleanup_preserves_pending_reconciliation(cleanup: impl Fn(&BatchWriter, i64)) {
        for target in [8, 9] {
            let (_dir, store, append, writer) = setup();
            let key = seed_pending_reorg(&store, false);
            let prior_key = ckbadger_store::keys::encode_reorg_event_key(500, &[2; 16]).unwrap();
            // The earlier B -> A switch abandoned B10. The pending A -> B
            // observation must still link it after cleanup and replay of B.
            let prior = ckbadger_store::ReorgEvent {
                detected_at: 0,
                old_tip_hash: vec![0xb0; 32],
                new_tip: 10,
                new_tip_hash: vec![0xa0; 32],
                ..store.get_reorg_event(1000).unwrap().unwrap().event
            };
            let mut batch = StoreBatch::new(&store);
            batch.put_sync_meta(prior_key.as_bytes(), &bincode::serialize(&prior).unwrap());
            batch.commit().unwrap();
            let replay_headers: Vec<_> = (target + 1..=11)
                .map(|height| (height, store.get_block_header(height).unwrap().unwrap()))
                .collect();
            append
                .put_cf(append.cf_cells(), &[7; 36], b"immutable")
                .unwrap();

            cleanup(&writer, target);

            assert!(store.get_block_header(target + 1).unwrap().is_none());
            assert_eq!(
                store.get_block_header(8).unwrap().unwrap().hash,
                vec![8; 32]
            );
            assert_eq!(
                store
                    .get_cf(
                        store.cf_sync_meta(),
                        sync_meta_keys::REORG_RECONCILE_PENDING
                    )
                    .unwrap(),
                Some(key.as_bytes().to_vec()),
                "cleanup target={target} must retain the observation"
            );
            assert!(
                !store
                    .get_reorg_event(1000)
                    .unwrap()
                    .unwrap()
                    .event
                    .reconcile_interrupted
            );
            writer.reconcile_reorg_events().unwrap();
            assert_eq!(
                store
                    .get_reorg_event(500)
                    .unwrap()
                    .unwrap()
                    .event
                    .reverted_by_event,
                None
            );

            let mut batch = StoreBatch::new(&store);
            for (height, header) in replay_headers {
                batch.put_block_header(height, &header);
            }
            batch.commit().unwrap();
            store.update_sync_tip(11, &[0xb1; 32], 0).unwrap();
            writer.reconcile_reorg_events().unwrap();

            assert_eq!(
                store
                    .get_cf(
                        store.cf_sync_meta(),
                        sync_meta_keys::REORG_RECONCILE_PENDING
                    )
                    .unwrap(),
                None
            );
            let event = store.get_reorg_event(1000).unwrap().unwrap().event;
            assert!(!event.reconcile_interrupted);
            assert_eq!(event.superseded_by_tip_hash, None);
            assert_eq!(event.reverses_events, vec![prior_key]);
            assert_eq!(
                store
                    .get_reorg_event(500)
                    .unwrap()
                    .unwrap()
                    .event
                    .reverted_by_event,
                Some(key)
            );
            assert_append_payload_unchanged(&append);
        }
    }

    fn assert_cleanup_interrupts_pending_reconciliation(cleanup: impl Fn(&BatchWriter, i64)) {
        for target in [-1, 7] {
            let (_dir, store, append, writer) = setup();
            seed_pending_reorg(&store, true);
            append
                .put_cf(append.cf_cells(), &[7; 36], b"immutable")
                .unwrap();

            cleanup(&writer, target);

            assert!(store.get_block_header(target + 1).unwrap().is_none());
            assert!(store
                .get_cf(
                    store.cf_sync_meta(),
                    sync_meta_keys::REORG_RECONCILE_PENDING
                )
                .unwrap()
                .is_none());
            let event = store.get_reorg_event(1000).unwrap().unwrap().event;
            assert!(event.reconcile_interrupted, "cleanup target={target}");
            assert_eq!(event.superseded_by_tip_hash, Some(vec![0xb2; 32]));
            assert_append_payload_unchanged(&append);
        }
    }

    #[test]
    fn startup_cleanup_preserves_pending_reconciliation_at_or_above_fork_point() {
        assert_cleanup_preserves_pending_reconciliation(|writer, target| {
            writer.init_sync_start(target, false).unwrap();
        });
    }

    #[test]
    fn batch_cleanup_preserves_pending_reconciliation_at_or_above_fork_point() {
        assert_cleanup_preserves_pending_reconciliation(|writer, target| {
            writer.cleanup_batch_range(target + 1, 11).unwrap();
        });
    }

    #[test]
    fn startup_cleanup_interrupts_pending_reconciliation_below_fork_point() {
        assert_cleanup_interrupts_pending_reconciliation(|writer, target| {
            writer.init_sync_start(target, false).unwrap();
        });
    }

    #[test]
    fn batch_cleanup_interrupts_pending_reconciliation_below_fork_point() {
        assert_cleanup_interrupts_pending_reconciliation(|writer, target| {
            writer.cleanup_batch_range(target + 1, 11).unwrap();
        });
    }

    #[test]
    fn test_init_sync_start_skips_cleanup_when_no_partial_data() {
        let (_dir, store, _append_store, writer) = setup();
        let lock_hash = vec![0xAA; 32];
        store
            .put_addr_balance_direct(
                &lock_hash,
                &AddressBalance {
                    balance: 123,
                    ..Default::default()
                },
            )
            .unwrap();

        writer.init_sync_start(0, false).unwrap();

        assert!(store.get_addr_balance(&lock_hash).unwrap().is_some());
    }

    #[test]
    fn test_init_sync_start_cleans_when_partial_data_exists() {
        let (_dir, store, _append_store, writer) = setup();
        let lock_hash = vec![0xBB; 32];
        store
            .put_addr_balance_direct(
                &lock_hash,
                &AddressBalance {
                    balance: 456,
                    ..Default::default()
                },
            )
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &make_header(0x10, 1_700_000_000_000));
        batch.put_block_header(1, &make_header(0x11, 1_700_000_010_000));
        batch.commit().unwrap();
        seed_ten_second_histogram(&store);

        writer.init_sync_start(0, false).unwrap();

        assert!(store.get_block_header(1).unwrap().is_none());
    }

    #[test]
    fn test_needs_startup_cleanup_reports_partial_data() {
        let (_dir, store, _append_store, writer) = setup();
        assert!(!writer.needs_startup_cleanup(0).unwrap());

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &make_header(0x21, 1_700_000_020_000));
        batch.commit().unwrap();

        assert!(writer.needs_startup_cleanup(0).unwrap());
    }

    #[test]
    fn test_needs_startup_cleanup_reports_pending_undo_log_entries() {
        let (_dir, store, _append_store, writer) = setup();
        assert!(!writer.needs_startup_cleanup(0).unwrap());

        // A journal row of block 2 (a block-2 write through a journaled batch).
        ckbadger_store::commit_journaled_block(&store, 2, |batch| {
            batch.put_sync_meta(b"fixture", b"written at block 2")
        })
        .unwrap();

        assert!(writer.needs_startup_cleanup(0).unwrap());
        assert!(!writer.needs_startup_cleanup(2).unwrap());
    }

    #[test]
    fn test_probe_startup_continuity_reports_clean_state() {
        let (_dir, store, _append_store, writer) = setup();

        let mut batch = StoreBatch::new(&store);
        for block in 0..=2 {
            batch.put_block_header(block, &make_header(0x40 + block as u8, 1_700_000_000_000));
            batch.put_tx_index(block, 0, &make_tx_index_entry());
        }
        batch.commit().unwrap();

        let probe = writer.probe_startup_continuity(2, 32, true).unwrap();
        assert_eq!(probe.header_tip, Some(2));
        assert_eq!(probe.tx_floor, Some(0));
        assert_eq!(probe.tx_tip, Some(2));
        assert_eq!(probe.first_header_gap, None);
        assert!(probe.missing_header_sample.is_empty());
        assert!(probe.missing_tx_block0_sample.is_empty());
        assert!(!probe.has_inconsistency());
    }

    #[test]
    fn test_probe_startup_continuity_detects_missing_header_and_tx() {
        let (_dir, store, _append_store, writer) = setup();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &make_header(0x50, 1_700_000_000_000));
        batch.put_block_header(2, &make_header(0x52, 1_700_000_020_000));
        batch.put_tx_index(0, 0, &make_tx_index_entry());
        batch.commit().unwrap();

        let probe = writer.probe_startup_continuity(2, 32, true).unwrap();
        assert_eq!(probe.header_tip, Some(2));
        assert_eq!(probe.tx_floor, Some(0));
        assert_eq!(probe.tx_tip, Some(0));
        assert_eq!(probe.first_header_gap, Some(1));
        assert_eq!(probe.missing_header_sample, vec![1]);
        assert_eq!(probe.missing_tx_block0_sample, vec![2]);
        assert!(probe.has_inconsistency());
    }

    #[test]
    fn test_needs_startup_cleanup_with_force_reports_true_without_partial_data() {
        let (_dir, _store, _append_store, writer) = setup();
        assert!(writer.needs_startup_cleanup_with_force(0, true).unwrap());
    }

    #[test]
    fn test_init_sync_start_forces_cleanup_without_partial_data() {
        let (_dir, store, _append_store, writer) = setup();
        let lock_hash = vec![0xCC; 32];

        // Write some addr_balance data that would be wiped by a full rollback to -1
        store
            .put_addr_balance_direct(
                &lock_hash,
                &AddressBalance {
                    balance: 789,
                    ..Default::default()
                },
            )
            .unwrap();

        // force_cleanup=true but no partial data → should skip rollback, data preserved
        writer.init_sync_start_with_options(0, false, true).unwrap();

        // addr_balance should survive because no rollback was executed
        assert!(store.get_addr_balance(&lock_hash).unwrap().is_some());
        let status = store.get_sync_status().unwrap();
        assert!(status.sync_started_at.is_some());
    }

    #[test]
    fn test_init_sync_start_forces_cleanup_with_partial_data() {
        let (_dir, store, _append_store, writer) = setup();

        // Write block headers at 0 and 1, then init from block 0 with force_cleanup
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &make_header(0x60, 1_700_000_000_000));
        batch.put_block_header(1, &make_header(0x61, 1_700_000_010_000));
        batch.commit().unwrap();
        seed_ten_second_histogram(&store);

        // force_cleanup=true AND partial data (block 1 beyond start_block 0) → cleanup runs
        writer.init_sync_start_with_options(0, false, true).unwrap();

        // Block 1 should be cleaned up
        assert!(store.get_block_header(1).unwrap().is_none());
    }

    #[test]
    fn test_init_sync_start_cleanup_rolls_back_sync_status_totals() {
        let (_dir, store, _append_store, writer) = setup();

        // Blocks 0 and 1 as the live writer commits them: rows and the sync
        // totals each block leaves, journaled in the block's scope.
        commit_journaled_sync_block(&store, 0, &make_header(0x70, 1_700_000_000_000), |status| {
            status.total_transactions = 1;
            status.total_cells_created = 2;
            status.total_cells_consumed = 1;
        });
        commit_journaled_sync_block(&store, 1, &make_header(0x71, 1_700_000_010_000), |status| {
            status.total_transactions = 3;
            status.total_cells_created = 11;
            status.total_cells_consumed = 6;
        });
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_index(0, 0, &make_tx_index_entry_with_counts(1, 2));
        batch.put_tx_index(1, 0, &make_tx_index_entry_with_counts(3, 5));
        batch.put_tx_index(1, 1, &make_tx_index_entry_with_counts(2, 4));
        batch.commit().unwrap();

        writer.init_sync_start_with_options(0, false, true).unwrap();

        let status = store.get_sync_status().unwrap();
        assert_eq!(status.tip_block_number, 0);
        assert_eq!(status.tip_block_hash, vec![0x70; 32]);
        assert_eq!(status.total_transactions, 1);
        assert_eq!(status.total_cells_created, 2);
        assert_eq!(status.total_cells_consumed, 1);
    }

    #[test]
    fn test_init_sync_start_persists_bulk_sync_start_metadata() {
        let (_dir, store, _append_store, writer) = setup();

        writer.init_sync_start(128, true).unwrap();

        let status = store.get_sync_status().unwrap();
        assert_eq!(status.sync_started_block, 128);
        assert!(status.sync_started_at.is_some());
        assert_eq!(status.bulk_sync_completed_at, None);
        assert_eq!(status.bulk_sync_completed_block, None);
    }

    #[test]
    fn test_init_sync_start_errors_when_start_block_below_minus_one() {
        let (_dir, _store, _append_store, writer) = setup();
        let err = writer.init_sync_start(-2, false).unwrap_err();
        assert!(err.to_string().contains("expected >= -1"));
    }

    #[test]
    fn test_probe_startup_continuity_detects_incomplete_tx_rows() {
        let (_dir, store, _append_store, writer) = setup();

        let mut header = make_header(0x80, 1_700_000_000_000);
        header.transactions_count = 3;
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &header);
        // Only write tx0 and tx1, but header claims 3 txs
        batch.put_tx_index(0, 0, &make_tx_index_entry());
        batch.put_tx_index(0, 1, &make_tx_index_entry());
        batch.commit().unwrap();

        let probe = writer.probe_startup_continuity(0, 32, false).unwrap();
        assert!(
            probe.has_inconsistency(),
            "should detect incomplete tx rows: header claims 3 txs but only 2 exist"
        );
        assert_eq!(probe.missing_tx_incomplete_sample.len(), 1);
        assert_eq!(probe.missing_tx_incomplete_sample[0], (0, 3, 1));
    }
}
