use std::sync::Arc;

use anyhow::{anyhow, Result};
use ckbadger_store::keys;
use ckbadger_store::types::{encode_live_cell_marker, CachedBlockHeader, LiveCellInfo};
use ckbadger_store::{
    cf_write_policy, is_append_only_cf_name, CfWritePolicy, CkbadgerStore, StoreBatch,
    CF_BLOCK_HEADERS, CF_CELLS, CF_LIVE_CELLS,
};

#[doc(hidden)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MaterializationReport {
    pub streamed_history_rows: usize,
    pub sealed_aggregate_rows: usize,
    pub final_snapshot_rows: usize,
    pub history_flushes: usize,
    pub sealed_aggregate_flushes: usize,
    pub final_snapshot_flushes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MaterializedRow {
    pub(crate) cf_name: &'static str,
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
}

impl MaterializedRow {
    pub(crate) fn new(cf_name: &'static str, key: Vec<u8>, value: Vec<u8>) -> Self {
        Self {
            cf_name,
            key,
            value,
        }
    }
}

/// Rows produced by an owner's `build_final_rows()` method, split by write policy.
/// `sealed_rows` are daily/hourly aggregates (SealedAggregate policy).
/// `snapshot_rows` are current-state data (FinalSnapshot policy).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OwnerFinalRows {
    pub(crate) sealed_rows: Vec<MaterializedRow>,
    pub(crate) snapshot_rows: Vec<MaterializedRow>,
}

pub(crate) struct Materializer<'a> {
    domain_store: &'a CkbadgerStore,
    append_only_store: &'a CkbadgerStore,
    report: MaterializationReport,
}

impl<'a> Materializer<'a> {
    pub(crate) fn new(
        domain_store: &'a CkbadgerStore,
        append_only_store: &'a CkbadgerStore,
    ) -> Self {
        Self {
            domain_store,
            append_only_store,
            report: MaterializationReport::default(),
        }
    }

    pub(crate) fn stream_history_rows(&mut self, rows: &[MaterializedRow]) -> Result<()> {
        self.write_rows(rows, CfWritePolicy::AppendOnly, CounterKind::History)
    }

    pub(crate) fn stream_sealed_aggregate_rows(&mut self, rows: &[MaterializedRow]) -> Result<()> {
        self.write_rows(
            rows,
            CfWritePolicy::SealedAggregate,
            CounterKind::SealedAggregate,
        )
    }

    pub(crate) fn materialize_final_snapshot(&mut self, rows: &[MaterializedRow]) -> Result<()> {
        self.write_rows(
            rows,
            CfWritePolicy::FinalSnapshot,
            CounterKind::FinalSnapshot,
        )
    }

    pub(crate) fn domain_store(&self) -> &'a CkbadgerStore {
        self.domain_store
    }

    /// Track rows that were flushed externally (e.g. via the flush channel
    /// pipeline in a background `spawn_blocking` task).
    pub(crate) fn add_external_counts(
        &mut self,
        history: usize,
        sealed: usize,
        flush_count: usize,
    ) {
        self.report.streamed_history_rows += history;
        if history > 0 {
            self.report.history_flushes += flush_count;
        }
        self.report.sealed_aggregate_rows += sealed;
        if sealed > 0 {
            self.report.sealed_aggregate_flushes += flush_count;
        }
    }

    pub(crate) fn finish(self) -> MaterializationReport {
        self.report
    }

    pub(crate) fn merge_report(&mut self, other: MaterializationReport) {
        self.report.streamed_history_rows += other.streamed_history_rows;
        self.report.sealed_aggregate_rows += other.sealed_aggregate_rows;
        self.report.final_snapshot_rows += other.final_snapshot_rows;
        self.report.history_flushes += other.history_flushes;
        self.report.sealed_aggregate_flushes += other.sealed_aggregate_flushes;
        self.report.final_snapshot_flushes += other.final_snapshot_flushes;
    }

    fn write_rows(
        &mut self,
        rows: &[MaterializedRow],
        expected_policy: CfWritePolicy,
        counter_kind: CounterKind,
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut domain_batch = StoreBatch::new(self.domain_store);
        let mut append_batch = StoreBatch::new(self.append_only_store);

        for row in rows {
            if matches!(counter_kind, CounterKind::FinalSnapshot)
                && is_append_only_cf_name(row.cf_name)
            {
                return Err(anyhow!(
                    "final snapshot cannot target append-only cf: {}",
                    row.cf_name
                ));
            }

            let actual_policy = cf_write_policy(row.cf_name);
            if actual_policy != expected_policy {
                return Err(anyhow!(
                    "materializer write-policy mismatch: cf={} expected={:?} actual={:?}",
                    row.cf_name,
                    expected_policy,
                    actual_policy
                ));
            }

            if is_append_only_cf_name(row.cf_name) {
                append_batch.put_raw_cf_by_name(row.cf_name, &row.key, &row.value)?;
            } else {
                domain_batch.put_raw_cf_by_name(row.cf_name, &row.key, &row.value)?;
            }
        }

        if !append_batch.is_empty() {
            append_batch.commit_no_wal()?;
        }
        if !domain_batch.is_empty() {
            domain_batch.commit_no_wal()?;
        }

        match counter_kind {
            CounterKind::History => {
                self.report.streamed_history_rows += rows.len();
                self.report.history_flushes += 1;
            }
            CounterKind::SealedAggregate => {
                self.report.sealed_aggregate_rows += rows.len();
                self.report.sealed_aggregate_flushes += 1;
            }
            CounterKind::FinalSnapshot => {
                self.report.final_snapshot_rows += rows.len();
                self.report.final_snapshot_flushes += 1;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum CounterKind {
    History,
    SealedAggregate,
    FinalSnapshot,
}

/// Result of a background flush operation (used by `flush_rows_to_stores` tests).
#[cfg(test)]
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct FlushResult {
    pub(crate) history_rows: usize,
    pub(crate) sealed_rows: usize,
    pub(crate) flush_ms: f64,
    pub(crate) prepare_ms: f64,
    pub(crate) commit_ms: f64,
}

/// Flush materialized rows to RocksDB via `StoreBatch` (test-only path).
///
/// The production flush pipeline builds MaterializedRows in
/// `build_history_rows_for_block` and converts via `prepare_flush`.
/// This function is retained for its test coverage of `CfWritePolicy` validation.
#[cfg(test)]
pub(crate) fn flush_rows_to_stores(
    domain_store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    history_rows: Vec<MaterializedRow>,
    sealed_rows: Vec<MaterializedRow>,
) -> Result<FlushResult> {
    let flush_started = std::time::Instant::now();
    let mut domain_batch = StoreBatch::new(domain_store);
    let mut append_batch = StoreBatch::new(append_only_store);

    for row in &history_rows {
        let actual_policy = cf_write_policy(row.cf_name);
        if actual_policy != CfWritePolicy::AppendOnly {
            return Err(anyhow!(
                "flush_rows_to_stores: history row has wrong write policy: cf={} policy={:?}",
                row.cf_name,
                actual_policy
            ));
        }
        if is_append_only_cf_name(row.cf_name) {
            append_batch.put_raw_cf_by_name(row.cf_name, &row.key, &row.value)?;
        } else {
            domain_batch.put_raw_cf_by_name(row.cf_name, &row.key, &row.value)?;
        }
    }
    for row in &sealed_rows {
        let actual_policy = cf_write_policy(row.cf_name);
        if actual_policy != CfWritePolicy::SealedAggregate {
            return Err(anyhow!(
                "flush_rows_to_stores: sealed row has wrong write policy: cf={} policy={:?}",
                row.cf_name,
                actual_policy
            ));
        }
        domain_batch.put_raw_cf_by_name(row.cf_name, &row.key, &row.value)?;
    }

    let prepare_ms = flush_started.elapsed().as_secs_f64() * 1000.0;
    let commit_started = std::time::Instant::now();

    if !append_batch.is_empty() {
        append_batch.commit_no_wal()?;
    }
    if !domain_batch.is_empty() {
        domain_batch.commit_no_wal()?;
    }

    let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
    Ok(FlushResult {
        history_rows: history_rows.len(),
        sealed_rows: sealed_rows.len(),
        flush_ms: flush_started.elapsed().as_secs_f64() * 1000.0,
        prepare_ms,
        commit_ms,
    })
}

pub(crate) fn run_sample_bulk_materialization_for_test() -> Result<MaterializationReport> {
    let root = super::unique_temp_test_dir("bulk-build-materialize");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let report = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);

        let outpoint_key = keys::encode_outpoint(&[0x11; 32], 0);
        let cell_info = LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x21; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x23; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let block_header = CachedBlockHeader {
            hash: vec![0x31; 32],
            parent_hash: vec![0x00; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 42,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        materializer.stream_history_rows(&[
            MaterializedRow::new(
                CF_CELLS,
                outpoint_key.to_vec(),
                bincode::serialize(&cell_info)?,
            ),
            MaterializedRow::new(
                CF_BLOCK_HEADERS,
                keys::encode_block_num(42).to_vec(),
                bincode::serialize(&block_header)?,
            ),
        ])?;
        materializer.materialize_final_snapshot(&[MaterializedRow::new(
            CF_LIVE_CELLS,
            outpoint_key.to_vec(),
            encode_live_cell_marker(42).to_vec(),
        )])?;

        assert!(append_store
            .get_cf(append_store.cf_cells(), &outpoint_key)?
            .is_some());
        assert!(domain_store
            .get_cf(domain_store.cf_block_headers(), &keys::encode_block_num(42))?
            .is_some());
        assert!(domain_store
            .get_cf(domain_store.cf_live_cells(), &outpoint_key)?
            .is_some());

        materializer.finish()
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(report)
}

/// A WriteBatch pair ready for sequential commit.  Built by `prepare_flush`
/// from MaterializedRows produced by `build_history_rows_for_block`.
pub(crate) struct PreparedBatch {
    pub(crate) append_batch: rocksdb::WriteBatch,
    pub(crate) domain_batch: rocksdb::WriteBatch,
    #[allow(dead_code)] // read only in tests
    pub(crate) history_count: usize,
    #[allow(dead_code)] // read only in tests
    pub(crate) sealed_count: usize,
}

/// Build two WriteBatch objects directly from MaterializedRows.
///
/// Bypasses StoreBatch / AppendBatchOp intermediate layers:
/// - No per-row `cf_write_policy` string comparison (caller guarantees correctness).
/// - No AppendBatchOp clone + HashMap dedup (keys are unique by construction in bulk build).
/// - Single pass over each row vector.
///
/// The returned WriteBatch objects are ready for `store.write_batch_no_wal_bulk()`.
///
/// Used by the flush worker to convert `PendingFlush` rows into WriteBatch
/// pairs, and by test helpers to flush rows to stores.
pub(crate) fn prepare_flush(
    domain_store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    history_rows: Vec<MaterializedRow>,
    sealed_rows: Vec<MaterializedRow>,
) -> Result<PreparedBatch> {
    // Single pass: route each row to its target WriteBatch by CF name.
    let mut append_batch = rocksdb::WriteBatch::default();
    let mut domain_batch = rocksdb::WriteBatch::default();

    for row in &history_rows {
        if is_append_only_cf_name(row.cf_name) {
            let cf = append_only_store.cf(row.cf_name);
            append_batch.put_cf(cf, &row.key, &row.value);
        } else {
            let cf = domain_store.cf(row.cf_name);
            domain_batch.put_cf(cf, &row.key, &row.value);
        }
    }
    for row in &sealed_rows {
        let cf = domain_store.cf(row.cf_name);
        domain_batch.put_cf(cf, &row.key, &row.value);
    }

    Ok(PreparedBatch {
        append_batch,
        domain_batch,
        history_count: history_rows.len(),
        sealed_count: sealed_rows.len(),
    })
}

#[derive(Debug, Default)]
pub(crate) struct FlushChannelStats {
    pub(crate) total_history_rows: usize,
    pub(crate) total_sealed_rows: usize,
    pub(crate) flush_count: usize,
    pub(crate) last_flush_ms: f64,
    pub(crate) total_prepare_ms: f64,
    pub(crate) total_commit_ms: f64,
}

pub(crate) struct FlushChannelHandle {
    tx: tokio::sync::mpsc::Sender<super::PendingFlush>,
    worker_handle: tokio::task::JoinHandle<Result<FlushChannelStats>>,
    flush_ms_rx: tokio::sync::watch::Receiver<f64>,
}

pub(crate) struct FlushDrainHandle {
    worker_handle: tokio::task::JoinHandle<Result<FlushChannelStats>>,
}

impl FlushDrainHandle {
    pub(crate) async fn wait(self) -> Result<FlushChannelStats> {
        self.worker_handle
            .await
            .map_err(|e| anyhow!("flush channel worker panicked: {}", e))?
    }
}

impl FlushChannelHandle {
    pub(crate) fn new(
        depth: usize,
        domain_store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel::<super::PendingFlush>(depth);
        let (flush_ms_tx, flush_ms_rx) = tokio::sync::watch::channel(0.0_f64);
        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::flush_worker(rx, domain_store, append_only_store, flush_ms_tx)
        });
        Self {
            tx,
            worker_handle,
            flush_ms_rx,
        }
    }

    fn flush_worker(
        mut rx: tokio::sync::mpsc::Receiver<super::PendingFlush>,
        domain_store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
        flush_ms_tx: tokio::sync::watch::Sender<f64>,
    ) -> Result<FlushChannelStats> {
        // Flush loop: receives PendingFlush (pure row data), converts to
        // WriteBatch via prepare_flush, then commits to RocksDB.
        let mut stats = FlushChannelStats::default();
        while let Some(pending) = rx.blocking_recv() {
            let flush_started = std::time::Instant::now();

            let history_count = pending.history_rows.len();
            let sealed_count = pending.sealed_rows.len();

            let prepared = prepare_flush(
                &domain_store,
                &append_only_store,
                pending.history_rows,
                pending.sealed_rows,
            )?;

            let prepare_ms = flush_started.elapsed().as_secs_f64() * 1000.0;
            let commit_started = std::time::Instant::now();

            // Commit append-only and domain stores in parallel — they are
            // independent RocksDB instances with separate write mutexes.
            let append_batch = prepared.append_batch;
            let domain_batch = prepared.domain_batch;
            std::thread::scope(|s| -> Result<()> {
                let append_handle = if !append_batch.is_empty() {
                    Some(s.spawn(|| append_only_store.write_batch_no_wal_bulk(append_batch)))
                } else {
                    None
                };
                if !domain_batch.is_empty() {
                    domain_store.write_batch_no_wal_bulk(domain_batch)?;
                }
                if let Some(handle) = append_handle {
                    handle.join().expect("append commit thread panicked")?;
                }
                Ok(())
            })?;

            let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
            let flush_ms = flush_started.elapsed().as_secs_f64() * 1000.0;
            stats.total_history_rows += history_count;
            stats.total_sealed_rows += sealed_count;
            stats.flush_count += 1;
            stats.total_prepare_ms += prepare_ms;
            stats.total_commit_ms += commit_ms;
            stats.last_flush_ms = flush_ms;
            let _ = flush_ms_tx.send(flush_ms);
        }
        Ok(stats)
    }

    pub(crate) async fn send(&self, pending: super::PendingFlush) -> Result<()> {
        self.tx
            .send(pending)
            .await
            .map_err(|_| anyhow!("flush channel worker has terminated unexpectedly"))
    }

    pub(crate) fn begin_shutdown(self) -> FlushDrainHandle {
        drop(self.tx);
        FlushDrainHandle {
            worker_handle: self.worker_handle,
        }
    }

    #[cfg(test)]
    pub(crate) async fn close_and_wait(self) -> Result<FlushChannelStats> {
        self.begin_shutdown().wait().await
    }

    pub(crate) fn last_flush_ms(&self) -> f64 {
        *self.flush_ms_rx.borrow()
    }

    /// Number of batches currently pending in the flush channel (0..=capacity).
    pub(crate) fn pending(&self) -> usize {
        self.tx.max_capacity() - self.tx.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializer_rejects_append_only_rows_in_final_snapshot() {
        let root = super::super::unique_temp_test_dir("bulk-build-materialize-reject");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();
        let mut materializer = Materializer::new(&domain_store, &append_store);

        let err = materializer
            .materialize_final_snapshot(&[MaterializedRow::new(
                CF_CELLS,
                b"k1".to_vec(),
                b"v1".to_vec(),
            )])
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("final snapshot cannot target append-only"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn flush_rows_to_stores_writes_history_and_sealed_rows() {
        let root = super::super::unique_temp_test_dir("bulk-build-flush-rows");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let outpoint_key = keys::encode_outpoint(&[0xAA; 32], 1);
        let cell_info = LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x21; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x23; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let block_header = CachedBlockHeader {
            hash: vec![0xBB; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 100,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let history_rows = vec![
            MaterializedRow::new(
                CF_CELLS,
                outpoint_key.to_vec(),
                bincode::serialize(&cell_info).unwrap(),
            ),
            MaterializedRow::new(
                CF_BLOCK_HEADERS,
                keys::encode_block_num(100).to_vec(),
                bincode::serialize(&block_header).unwrap(),
            ),
        ];

        let result =
            flush_rows_to_stores(&domain_store, &append_store, history_rows, vec![]).unwrap();
        assert_eq!(result.history_rows, 2);
        assert_eq!(result.sealed_rows, 0);

        // Verify data was written to the correct stores.
        assert!(append_store
            .get_cf(append_store.cf_cells(), &outpoint_key)
            .unwrap()
            .is_some());
        assert!(domain_store
            .get_cf(
                domain_store.cf_block_headers(),
                &keys::encode_block_num(100)
            )
            .unwrap()
            .is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn flush_rows_to_stores_rejects_wrong_policy_history() {
        let root = super::super::unique_temp_test_dir("bulk-build-flush-reject-hist");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        // CF_LIVE_CELLS is FinalSnapshot policy, not AppendOnly — should be rejected.
        let bad_rows = vec![MaterializedRow::new(
            CF_LIVE_CELLS,
            b"k1".to_vec(),
            b"v1".to_vec(),
        )];

        let err = flush_rows_to_stores(&domain_store, &append_store, bad_rows, vec![]).unwrap_err();
        assert!(
            err.to_string()
                .contains("history row has wrong write policy"),
            "unexpected error: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn flush_rows_to_stores_rejects_wrong_policy_sealed() {
        let root = super::super::unique_temp_test_dir("bulk-build-flush-reject-seal");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        // CF_LIVE_CELLS is FinalSnapshot policy, not SealedAggregate — should be rejected.
        let bad_sealed = vec![MaterializedRow::new(
            CF_LIVE_CELLS,
            b"k2".to_vec(),
            b"v2".to_vec(),
        )];

        let err =
            flush_rows_to_stores(&domain_store, &append_store, vec![], bad_sealed).unwrap_err();
        assert!(
            err.to_string()
                .contains("sealed row has wrong write policy"),
            "unexpected error: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn flush_channel_handle_flushes_all_queued_batches() {
        let root = super::super::unique_temp_test_dir("flush-channel-handle");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = Arc::new(CkbadgerStore::open_domain(&domain_path).unwrap());
        let append_store = Arc::new(CkbadgerStore::open_append_only(&append_path).unwrap());

        let handle = FlushChannelHandle::new(4, domain_store.clone(), append_store.clone());

        let mut outpoint_keys = Vec::new();
        let mut header_keys = Vec::new();

        for i in 0u8..3 {
            let outpoint_key = keys::encode_outpoint(&[i; 32], 0);
            let header_key = keys::encode_block_num(i as i64);
            outpoint_keys.push(outpoint_key);
            header_keys.push(header_key);

            let cell_info = LiveCellInfo {
                capacity: 100_00000000,
                lock_script_hash: vec![0x21; 32],
                lock_code_hash: vec![0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![0x23; 20],
                type_script_hash: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 0,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: None,
            };
            let block_header = CachedBlockHeader {
                hash: vec![i; 32],
                parent_hash: vec![0u8; 32],
                timestamp: 1_700_000_000_000,
                epoch_number: i as i64,
                epoch_index: 0,
                epoch_length: 1800,
                dao: vec![0x00; 32],
                transactions_count: 1,
            uncles_count: 0,
                cycles: None,
            };

            let pending = super::super::PendingFlush {
                history_rows: vec![
                    MaterializedRow::new(
                        CF_CELLS,
                        outpoint_key.to_vec(),
                        bincode::serialize(&cell_info).unwrap(),
                    ),
                    MaterializedRow::new(
                        CF_BLOCK_HEADERS,
                        header_key.to_vec(),
                        bincode::serialize(&block_header).unwrap(),
                    ),
                ],
                sealed_rows: Vec::new(),
            };
            handle.send(pending).await.unwrap();
        }

        let stats = handle.close_and_wait().await.unwrap();
        assert!(stats.flush_count >= 1);
        assert!(stats.flush_count <= 3);
        assert_eq!(stats.total_history_rows, 6);
        assert_eq!(stats.total_sealed_rows, 0);
        assert!(stats.last_flush_ms > 0.0);

        // Verify all 3 cells exist in append-only store.
        for key in &outpoint_keys {
            assert!(
                append_store
                    .get_cf(append_store.cf_cells(), key)
                    .unwrap()
                    .is_some(),
                "cell not found for outpoint key"
            );
        }

        // Verify all 3 headers exist in domain store.
        for key in &header_keys {
            assert!(
                domain_store
                    .get_cf(domain_store.cf_block_headers(), key)
                    .unwrap()
                    .is_some(),
                "block header not found"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn flush_channel_handle_backpressure_drains_all() {
        let root = super::super::unique_temp_test_dir("flush-channel-backpressure");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = Arc::new(CkbadgerStore::open_domain(&domain_path).unwrap());
        let append_store = Arc::new(CkbadgerStore::open_append_only(&append_path).unwrap());

        // Deliberately small channel depth to exercise backpressure.
        let handle = FlushChannelHandle::new(2, domain_store.clone(), append_store.clone());

        let cell_info = LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: vec![0x21; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x23; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let cell_bytes = bincode::serialize(&cell_info).unwrap();

        for i in 0u8..10 {
            let outpoint_key = keys::encode_outpoint(&[i; 32], 0);
            let pending = super::super::PendingFlush {
                history_rows: vec![MaterializedRow::new(
                    CF_CELLS,
                    outpoint_key.to_vec(),
                    cell_bytes.clone(),
                )],
                sealed_rows: Vec::new(),
            };
            handle.send(pending).await.unwrap();
        }

        let stats = handle.close_and_wait().await.unwrap();
        assert!(stats.flush_count >= 1);
        assert!(stats.flush_count <= 10);
        assert_eq!(stats.total_history_rows, 10);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn add_external_counts_updates_report() {
        let root = super::super::unique_temp_test_dir("bulk-build-external-counts");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();
        let mut materializer = Materializer::new(&domain_store, &append_store);

        materializer.add_external_counts(100, 50, 3);
        materializer.add_external_counts(200, 75, 5);

        let report = materializer.finish();
        assert_eq!(report.streamed_history_rows, 300);
        assert_eq!(report.sealed_aggregate_rows, 125);
        assert_eq!(report.history_flushes, 8);
        assert_eq!(report.sealed_aggregate_flushes, 8);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_flush_builds_correct_write_batches() {
        let root = super::super::unique_temp_test_dir("prepare-flush");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();

        let outpoint_key = keys::encode_outpoint(&[0xCC; 32], 2);
        let cell_info = LiveCellInfo {
            capacity: 300_00000000,
            lock_script_hash: vec![0x21; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x23; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let block_header = CachedBlockHeader {
            hash: vec![0xDD; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 200,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        };

        let history_rows = vec![
            MaterializedRow::new(
                CF_CELLS,
                outpoint_key.to_vec(),
                bincode::serialize(&cell_info).unwrap(),
            ),
            MaterializedRow::new(
                CF_BLOCK_HEADERS,
                keys::encode_block_num(200).to_vec(),
                bincode::serialize(&block_header).unwrap(),
            ),
        ];
        use ckbadger_store::CF_STATS_CHAIN;
        let sealed_rows = vec![MaterializedRow::new(
            CF_STATS_CHAIN,
            b"day:2026-03-30".to_vec(),
            b"stats-value".to_vec(),
        )];

        let prepared =
            prepare_flush(&domain_store, &append_store, history_rows, sealed_rows).unwrap();

        assert_eq!(prepared.history_count, 2);
        assert_eq!(prepared.sealed_count, 1);

        // Commit the prepared batches and verify data landed correctly.
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .unwrap();
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .unwrap();

        assert!(append_store
            .get_cf(append_store.cf_cells(), &outpoint_key)
            .unwrap()
            .is_some());
        assert!(domain_store
            .get_cf(
                domain_store.cf_block_headers(),
                &keys::encode_block_num(200)
            )
            .unwrap()
            .is_some());

        let _ = std::fs::remove_dir_all(&root);
    }
}
