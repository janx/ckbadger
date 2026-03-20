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

#[derive(Debug, Clone)]
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

    /// Track rows that were flushed externally (e.g. via `flush_rows_to_stores`
    /// in a background `spawn_blocking` task).
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

/// Result of a background flush operation.
#[derive(Debug)]
pub(crate) struct FlushResult {
    pub(crate) history_rows: usize,
    pub(crate) sealed_rows: usize,
    pub(crate) flush_ms: f64,
}

/// Flush materialized rows to RocksDB. Designed for use in `spawn_blocking`
/// since it takes owned data (not borrowed references).
///
/// Replicates the `CfWritePolicy` validation from `Materializer::write_rows`
/// to ensure store boundary invariants are enforced on the background path.
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

    if !append_batch.is_empty() {
        append_batch.commit_no_wal()?;
    }
    if !domain_batch.is_empty() {
        domain_batch.commit_no_wal()?;
    }

    Ok(FlushResult {
        history_rows: history_rows.len(),
        sealed_rows: sealed_rows.len(),
        flush_ms: flush_started.elapsed().as_secs_f64() * 1000.0,
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
            timestamp: 1_700_000_000_000,
            epoch_number: 42,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 1,
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
            timestamp: 1_700_000_000_000,
            epoch_number: 100,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 1,
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
}
