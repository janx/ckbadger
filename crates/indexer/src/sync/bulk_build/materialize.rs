use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
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

/// Compact descriptor for one Class-A history row.
///
/// Keys and values are stored back-to-back in [`EncodedHistoryChunk::payload`].
/// Keeping only lengths here avoids two heap allocations and two `Vec` headers
/// per row while preserving exact row order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EncodedHistoryRow {
    cf_name: &'static str,
    key_len: u32,
    value_len: u32,
}

/// Per-block, contiguous Class-A history payload produced by Rayon workers.
///
/// A chunk owns exactly two growable allocations regardless of row count: the
/// descriptor vector and its byte payload. The flush worker consumes whole
/// chunks so their memory is released incrementally while `WriteBatch` grows.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct EncodedHistoryChunk {
    rows: Vec<EncodedHistoryRow>,
    payload: Vec<u8>,
}

impl EncodedHistoryChunk {
    pub(crate) fn with_capacity(row_capacity: usize, payload_capacity: usize) -> Self {
        Self {
            rows: Vec::with_capacity(row_capacity),
            payload: Vec::with_capacity(payload_capacity),
        }
    }

    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(crate) fn allocated_bytes(&self) -> Result<usize> {
        self.rows
            .capacity()
            .checked_mul(std::mem::size_of::<EncodedHistoryRow>())
            .and_then(|bytes| bytes.checked_add(self.payload.capacity()))
            .ok_or_else(|| anyhow!("encoded history chunk allocated byte count overflow"))
    }

    pub(crate) fn push_raw(
        &mut self,
        cf_name: &'static str,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let (key_len, value_len) = self.reserve_row(cf_name, key.len(), value.len())?;
        self.payload.extend_from_slice(key);
        self.payload.extend_from_slice(value);
        self.rows.push(EncodedHistoryRow {
            cf_name,
            key_len,
            value_len,
        });
        Ok(())
    }

    pub(crate) fn push_serialized<T: serde::Serialize>(
        &mut self,
        cf_name: &'static str,
        key: &[u8],
        value: &T,
    ) -> Result<()> {
        let serialized_size = bincode::serialized_size(value)
            .map_err(|e| anyhow!("bincode size estimation failed for cf={cf_name}: {e}"))?;
        let value_len = usize::try_from(serialized_size).map_err(|_| {
            anyhow!(
                "serialized history value exceeds usize: cf={} value_bytes={}",
                cf_name,
                serialized_size
            )
        })?;
        let (key_len_u32, value_len_u32) = self.reserve_row(cf_name, key.len(), value_len)?;
        let payload_start = self.payload.len();
        self.payload.extend_from_slice(key);
        if let Err(error) = bincode::serialize_into(&mut self.payload, value) {
            self.payload.truncate(payload_start);
            return Err(anyhow!(
                "bincode history serialization failed: cf={} key_bytes={} value_bytes={} error={}",
                cf_name,
                key.len(),
                value_len,
                error
            ));
        }
        let actual_value_len = self
            .payload
            .len()
            .checked_sub(payload_start)
            .and_then(|bytes| bytes.checked_sub(key.len()))
            .ok_or_else(|| anyhow!("encoded history payload length invariant violated"))?;
        if actual_value_len != value_len {
            self.payload.truncate(payload_start);
            bail!(
                "bincode history size invariant violated: cf={} estimated_value_bytes={} actual_value_bytes={}",
                cf_name,
                value_len,
                actual_value_len
            );
        }
        self.rows.push(EncodedHistoryRow {
            cf_name,
            key_len: key_len_u32,
            value_len: value_len_u32,
        });
        Ok(())
    }

    pub(crate) fn push_materialized(&mut self, row: MaterializedRow) -> Result<()> {
        self.push_raw(row.cf_name, &row.key, &row.value)
    }

    #[cfg(test)]
    pub(crate) fn from_materialized_rows(rows: Vec<MaterializedRow>) -> Result<Self> {
        let mut encoded = Self::with_capacity(rows.len(), 0);
        for row in rows {
            encoded.push_materialized(row)?;
        }
        Ok(encoded)
    }

    pub(crate) fn consume<F>(self, mut consume_row: F) -> Result<()>
    where
        F: FnMut(&'static str, &[u8], &[u8]) -> Result<()>,
    {
        let Self { rows, payload } = self;
        let mut cursor = 0usize;
        for row in rows {
            let key_end = cursor
                .checked_add(row.key_len as usize)
                .ok_or_else(|| anyhow!("encoded history key offset overflow"))?;
            let value_end = key_end
                .checked_add(row.value_len as usize)
                .ok_or_else(|| anyhow!("encoded history value offset overflow"))?;
            if value_end > payload.len() {
                bail!(
                    "encoded history descriptor exceeds payload: cf={} cursor={} key_bytes={} value_bytes={} payload_bytes={}",
                    row.cf_name,
                    cursor,
                    row.key_len,
                    row.value_len,
                    payload.len()
                );
            }
            consume_row(
                row.cf_name,
                &payload[cursor..key_end],
                &payload[key_end..value_end],
            )?;
            cursor = value_end;
        }
        if cursor != payload.len() {
            bail!(
                "encoded history payload has unreferenced bytes: consumed_bytes={} payload_bytes={}",
                cursor,
                payload.len()
            );
        }
        Ok(())
    }

    fn reserve_row(
        &mut self,
        cf_name: &'static str,
        key_len: usize,
        value_len: usize,
    ) -> Result<(u32, u32)> {
        let key_len_u32 = u32::try_from(key_len).map_err(|_| {
            anyhow!(
                "history key exceeds per-row encoding limit: cf={} key_bytes={}",
                cf_name,
                key_len
            )
        })?;
        let value_len_u32 = u32::try_from(value_len).map_err(|_| {
            anyhow!(
                "history value exceeds per-row encoding limit: cf={} value_bytes={}",
                cf_name,
                value_len
            )
        })?;
        let payload_additional = key_len.checked_add(value_len).ok_or_else(|| {
            anyhow!(
                "history row payload byte count overflow: cf={} key_bytes={} value_bytes={}",
                cf_name,
                key_len,
                value_len
            )
        })?;
        self.rows.try_reserve(1).map_err(|e| {
            anyhow!(
                "failed to reserve encoded history descriptor: cf={} rows={} error={}",
                cf_name,
                self.rows.len(),
                e
            )
        })?;
        self.payload.try_reserve(payload_additional).map_err(|e| {
            anyhow!(
                "failed to reserve encoded history payload: cf={} current_bytes={} additional_bytes={} error={}",
                cf_name,
                self.payload.len(),
                payload_additional,
                e
            )
        })?;
        Ok((key_len_u32, value_len_u32))
    }

    #[cfg(test)]
    fn allocation_count(&self) -> usize {
        usize::from(self.rows.capacity() > 0) + usize::from(self.payload.capacity() > 0)
    }
}

/// Rows produced by an owner's `build_final_rows()` method, split by write policy.
/// `sealed_rows` are daily/hourly aggregates (SealedAggregate policy).
/// `snapshot_rows` are current-state data (FinalSnapshot policy).
#[derive(Debug, Default, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct OwnerFinalRows {
    pub(crate) sealed_rows: Vec<MaterializedRow>,
    pub(crate) snapshot_rows: Vec<MaterializedRow>,
}

/// Finalize writes are intentionally chunked by bytes rather than row count:
/// live-cell index keys and serialized domain values have very different
/// sizes. Keeping this well below the bulk batch budget prevents materializing
/// the final snapshot from becoming a second, unbounded copy of reducer state.
const DEFAULT_FINALIZE_BATCH_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoundedWriteStats {
    pub(crate) rows: usize,
    pub(crate) flushes: usize,
    pub(crate) peak_batch_bytes: usize,
}

pub(crate) struct BoundedRowSink<'a> {
    domain_store: &'a CkbadgerStore,
    batch: StoreBatch<'a>,
    expected_policy: CfWritePolicy,
    counter_kind: CounterKind,
    max_batch_bytes: usize,
    stats: BoundedWriteStats,
}

impl<'a> BoundedRowSink<'a> {
    fn new(
        domain_store: &'a CkbadgerStore,
        expected_policy: CfWritePolicy,
        counter_kind: CounterKind,
        max_batch_bytes: usize,
    ) -> Result<Self> {
        if max_batch_bytes == 0 {
            return Err(anyhow!("bounded materializer batch limit must be positive"));
        }
        Ok(Self {
            domain_store,
            batch: StoreBatch::new(domain_store),
            expected_policy,
            counter_kind,
            max_batch_bytes,
            stats: BoundedWriteStats::default(),
        })
    }

    pub(crate) fn push(&mut self, row: MaterializedRow) -> Result<()> {
        self.push_parts(row.cf_name, &row.key, &row.value)
    }

    fn push_borrowed(&mut self, row: &MaterializedRow) -> Result<()> {
        self.push_parts(row.cf_name, &row.key, &row.value)
    }

    fn push_parts(&mut self, cf_name: &'static str, key: &[u8], value: &[u8]) -> Result<()> {
        if is_append_only_cf_name(cf_name) {
            if matches!(self.counter_kind, CounterKind::FinalSnapshot) {
                return Err(anyhow!(
                    "final snapshot cannot target append-only cf: {}",
                    cf_name
                ));
            }
            return Err(anyhow!(
                "bounded finalize materialization cannot target append-only cf: {}",
                cf_name
            ));
        }
        let actual_policy = cf_write_policy(cf_name);
        if actual_policy != self.expected_policy {
            return Err(anyhow!(
                "materializer write-policy mismatch: cf={} expected={:?} actual={:?}",
                cf_name,
                self.expected_policy,
                actual_policy
            ));
        }

        // Flush before copying the next row when its payload cannot fit. The
        // RocksDB batch may exceed the limit only by one row's encoding
        // overhead (or when one row itself is larger than the configured cap).
        let next_payload_bytes = key
            .len()
            .checked_add(value.len())
            .and_then(|bytes| bytes.checked_add(cf_name.len()))
            .ok_or_else(|| {
                anyhow!(
                    "bounded materializer row byte size overflow: cf={} key_bytes={} value_bytes={}",
                    cf_name,
                    key.len(),
                    value.len()
                )
            })?;
        if !self.batch.is_empty()
            && self
                .batch
                .size_in_bytes()
                .saturating_add(next_payload_bytes)
                > self.max_batch_bytes
        {
            self.flush()?;
        }

        self.batch.put_raw_cf_by_name(cf_name, key, value)?;
        self.stats.rows += 1;
        self.stats.peak_batch_bytes = self.stats.peak_batch_bytes.max(self.batch.size_in_bytes());
        if self.batch.size_in_bytes() >= self.max_batch_bytes {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let batch = std::mem::replace(&mut self.batch, StoreBatch::new(self.domain_store));
        batch.commit_no_wal()?;
        self.stats.flushes += 1;
        Ok(())
    }

    fn finish(mut self) -> Result<BoundedWriteStats> {
        self.flush()?;
        Ok(self.stats)
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

    pub(crate) fn stream_sealed_aggregate_rows_bounded<F>(&mut self, emit: F) -> Result<()>
    where
        F: FnOnce(&mut BoundedRowSink<'_>) -> Result<()>,
    {
        self.write_bounded(
            CfWritePolicy::SealedAggregate,
            CounterKind::SealedAggregate,
            DEFAULT_FINALIZE_BATCH_BYTES,
            emit,
        )
        .map(|_| ())
    }

    pub(crate) fn materialize_final_snapshot_bounded<F>(&mut self, emit: F) -> Result<()>
    where
        F: FnOnce(&mut BoundedRowSink<'_>) -> Result<()>,
    {
        self.write_bounded(
            CfWritePolicy::FinalSnapshot,
            CounterKind::FinalSnapshot,
            DEFAULT_FINALIZE_BATCH_BYTES,
            emit,
        )
        .map(|_| ())
    }

    #[cfg(test)]
    fn materialize_final_snapshot_bounded_for_test<F>(
        &mut self,
        max_batch_bytes: usize,
        emit: F,
    ) -> Result<BoundedWriteStats>
    where
        F: FnOnce(&mut BoundedRowSink<'_>) -> Result<()>,
    {
        self.write_bounded(
            CfWritePolicy::FinalSnapshot,
            CounterKind::FinalSnapshot,
            max_batch_bytes,
            emit,
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
        // Per-batch history contains CF_CELLS and therefore still uses the
        // dual-store path. Finalize classes are domain-only and use the
        // byte-bounded sink.
        if matches!(counter_kind, CounterKind::History) {
            let mut domain_batch = StoreBatch::new(self.domain_store);
            let mut append_batch = StoreBatch::new(self.append_only_store);
            for row in rows {
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
            self.report.streamed_history_rows += rows.len();
            self.report.history_flushes += 1;
            return Ok(());
        }

        self.write_bounded(
            expected_policy,
            counter_kind,
            DEFAULT_FINALIZE_BATCH_BYTES,
            |sink| {
                for row in rows {
                    sink.push_borrowed(row)?;
                }
                Ok(())
            },
        )?;
        Ok(())
    }

    fn write_bounded<F>(
        &mut self,
        expected_policy: CfWritePolicy,
        counter_kind: CounterKind,
        max_batch_bytes: usize,
        emit: F,
    ) -> Result<BoundedWriteStats>
    where
        F: FnOnce(&mut BoundedRowSink<'_>) -> Result<()>,
    {
        let mut sink = BoundedRowSink::new(
            self.domain_store,
            expected_policy,
            counter_kind,
            max_batch_bytes,
        )?;
        emit(&mut sink)?;
        let stats = sink.finish()?;
        self.record_bounded_stats(counter_kind, stats);
        Ok(stats)
    }

    fn record_bounded_stats(&mut self, counter_kind: CounterKind, stats: BoundedWriteStats) {
        match counter_kind {
            CounterKind::History => {
                self.report.streamed_history_rows += stats.rows;
                self.report.history_flushes += stats.flushes;
            }
            CounterKind::SealedAggregate => {
                self.report.sealed_aggregate_rows += stats.rows;
                self.report.sealed_aggregate_flushes += stats.flushes;
            }
            CounterKind::FinalSnapshot => {
                self.report.final_snapshot_rows += stats.rows;
                self.report.final_snapshot_flushes += stats.flushes;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A WriteBatch pair ready for commit. Built by `prepare_flush` from encoded
/// per-block history chunks and bounded sealed aggregate rows.
pub(crate) struct PreparedBatch {
    pub(crate) append_batch: rocksdb::WriteBatch,
    pub(crate) domain_batch: rocksdb::WriteBatch,
    #[allow(dead_code)] // read only in tests
    pub(crate) history_count: usize,
    #[allow(dead_code)] // read only in tests
    pub(crate) sealed_count: usize,
}

/// Build two WriteBatch objects directly from encoded history chunks.
///
/// Bypasses StoreBatch / AppendBatchOp intermediate layers:
/// - No per-row `cf_write_policy` string comparison (caller guarantees correctness).
/// - No AppendBatchOp clone + HashMap dedup (keys are unique by construction in bulk build).
/// - Single pass over each contiguous per-block payload.
///
/// The returned WriteBatch objects are ready for `store.write_batch_no_wal_bulk()`.
///
/// Used by the flush worker to convert `PendingFlush` rows into WriteBatch
/// pairs, and by test helpers to flush rows to stores.
pub(crate) fn prepare_flush(
    domain_store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    history_chunks: Vec<EncodedHistoryChunk>,
    sealed_rows: Vec<MaterializedRow>,
) -> Result<PreparedBatch> {
    // Single pass: route each row to its target WriteBatch by CF name.
    let mut append_batch = rocksdb::WriteBatch::default();
    let mut domain_batch = rocksdb::WriteBatch::default();

    let history_count = history_chunks.iter().try_fold(0usize, |total, chunk| {
        total
            .checked_add(chunk.row_count())
            .ok_or_else(|| anyhow!("prepared history row count overflow"))
    })?;
    for chunk in history_chunks {
        chunk.consume(|cf_name, key, value| {
            if is_append_only_cf_name(cf_name) {
                let cf = append_only_store.cf(cf_name);
                append_batch.put_cf(cf, key, value);
            } else {
                let cf = domain_store.cf(cf_name);
                domain_batch.put_cf(cf, key, value);
            }
            Ok(())
        })?;
    }
    let sealed_count = sealed_rows.len();
    for row in sealed_rows {
        let cf = domain_store.cf(row.cf_name);
        domain_batch.put_cf(cf, &row.key, &row.value);
    }

    Ok(PreparedBatch {
        append_batch,
        domain_batch,
        history_count,
        sealed_count,
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
    tx: tokio::sync::mpsc::Sender<QueuedFlush>,
    worker_handle: tokio::task::JoinHandle<Result<FlushChannelStats>>,
    flush_ms_rx: tokio::sync::watch::Receiver<f64>,
    byte_budget: Arc<tokio::sync::Semaphore>,
    byte_budget_units: u32,
}

const FLUSH_QUEUE_BUDGET_UNIT_BYTES: usize = 1024 * 1024;

fn flush_reserved_bytes(byte_budget_units: u32, available_permits: usize) -> Result<u64> {
    let budget_units = usize::try_from(byte_budget_units).map_err(|_| {
        anyhow!(
            "flush byte-budget units exceed usize: budget_units={}",
            byte_budget_units
        )
    })?;
    let reserved_units = budget_units.checked_sub(available_permits).ok_or_else(|| {
        anyhow!(
            "flush available permits exceed budget: available_permits={} budget_units={}",
            available_permits,
            budget_units
        )
    })?;
    let reserved_units = u64::try_from(reserved_units)
        .map_err(|_| anyhow!("flush reserved permit units exceed u64: units={reserved_units}"))?;
    reserved_units
        .checked_mul(FLUSH_QUEUE_BUDGET_UNIT_BYTES as u64)
        .ok_or_else(|| {
            anyhow!(
                "flush reserved byte count overflow: reserved_units={} unit_bytes={}",
                reserved_units,
                FLUSH_QUEUE_BUDGET_UNIT_BYTES
            )
        })
}

struct QueuedFlush {
    pending: super::PendingFlush,
    _byte_permit: tokio::sync::OwnedSemaphorePermit,
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
        byte_budget_bytes: u64,
        domain_store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
    ) -> Result<Self> {
        if depth == 0 {
            return Err(anyhow!("flush channel depth must be positive"));
        }
        if byte_budget_bytes == 0 {
            return Err(anyhow!("flush queue byte budget must be positive"));
        }
        let budget_bytes = usize::try_from(byte_budget_bytes).map_err(|_| {
            anyhow!(
                "flush queue byte budget exceeds usize: byte_budget_bytes={}",
                byte_budget_bytes
            )
        })?;
        let budget_units = budget_bytes
            .checked_add(FLUSH_QUEUE_BUDGET_UNIT_BYTES - 1)
            .ok_or_else(|| {
                anyhow!(
                    "flush queue byte budget rounding overflow: byte_budget_bytes={}",
                    byte_budget_bytes
                )
            })?
            / FLUSH_QUEUE_BUDGET_UNIT_BYTES;
        let byte_budget_units = u32::try_from(budget_units).map_err(|_| {
            anyhow!(
                "flush queue byte budget exceeds semaphore range: byte_budget_bytes={} units={}",
                byte_budget_bytes,
                budget_units
            )
        })?;
        let byte_budget = Arc::new(tokio::sync::Semaphore::new(budget_units));
        let (tx, rx) = tokio::sync::mpsc::channel::<QueuedFlush>(depth);
        let (flush_ms_tx, flush_ms_rx) = tokio::sync::watch::channel(0.0_f64);
        let worker_handle = tokio::task::spawn_blocking(move || {
            Self::flush_worker(rx, domain_store, append_only_store, flush_ms_tx)
        });
        Ok(Self {
            tx,
            worker_handle,
            flush_ms_rx,
            byte_budget,
            byte_budget_units,
        })
    }

    fn flush_worker(
        mut rx: tokio::sync::mpsc::Receiver<QueuedFlush>,
        domain_store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
        flush_ms_tx: tokio::sync::watch::Sender<f64>,
    ) -> Result<FlushChannelStats> {
        // Flush loop: receives PendingFlush (pure row data), converts to
        // WriteBatch via prepare_flush, then commits to RocksDB.
        let mut stats = FlushChannelStats::default();
        while let Some(queued) = rx.blocking_recv() {
            let QueuedFlush {
                pending,
                _byte_permit,
            } = queued;
            let flush_started = std::time::Instant::now();

            let history_count = pending.history_row_count();
            let sealed_count = pending.sealed_rows.len();

            let prepared = prepare_flush(
                &domain_store,
                &append_only_store,
                pending.history_chunks,
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

    #[cfg(test)]
    pub(crate) async fn send(&self, pending: super::PendingFlush) -> Result<()> {
        let message_bytes = pending.allocated_bytes()?;
        self.send_with_allocated_bytes(pending, message_bytes).await
    }

    pub(crate) async fn send_with_allocated_bytes(
        &self,
        pending: super::PendingFlush,
        message_bytes: usize,
    ) -> Result<()> {
        let message_units = message_bytes
            .checked_add(FLUSH_QUEUE_BUDGET_UNIT_BYTES - 1)
            .ok_or_else(|| {
                anyhow!(
                    "flush message byte rounding overflow: message_bytes={}",
                    message_bytes
                )
            })?
            / FLUSH_QUEUE_BUDGET_UNIT_BYTES;
        let message_units = u32::try_from(message_units.max(1)).map_err(|_| {
            anyhow!(
                "flush message exceeds semaphore range: message_bytes={}",
                message_bytes
            )
        })?;
        if message_units > self.byte_budget_units {
            return Err(anyhow!(
                "flush queue byte budget is smaller than one message: message_bytes={} message_units={} budget_bytes={} budget_units={}",
                message_bytes,
                message_units,
                u64::from(self.byte_budget_units) * FLUSH_QUEUE_BUDGET_UNIT_BYTES as u64,
                self.byte_budget_units
            ));
        }
        let permit = Arc::clone(&self.byte_budget)
            .acquire_many_owned(message_units)
            .await
            .map_err(|_| anyhow!("flush queue byte budget closed unexpectedly"))?;
        self.tx
            .send(QueuedFlush {
                pending,
                _byte_permit: permit,
            })
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

    /// Byte permits retained by queued or currently flushing batches.
    ///
    /// Values are rounded up to the semaphore's 1 MiB accounting unit, so
    /// this is a conservative reservation rather than a sampled RSS value.
    pub(crate) fn reserved_bytes(&self) -> Result<u64> {
        flush_reserved_bytes(self.byte_budget_units, self.byte_budget.available_permits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_reserved_bytes_are_derived_from_semaphore_permits() {
        assert_eq!(
            flush_reserved_bytes(4, 1).unwrap(),
            3 * FLUSH_QUEUE_BUDGET_UNIT_BYTES as u64
        );
        let error = flush_reserved_bytes(4, 5).unwrap_err().to_string();
        assert!(error.contains("available permits exceed budget"), "{error}");
    }

    #[test]
    fn encoded_history_chunk_packs_rows_into_one_payload() {
        assert_eq!(
            std::mem::size_of::<EncodedHistoryRow>(),
            24,
            "encoded history descriptor layout changed"
        );
        let mut chunk = EncodedHistoryChunk::with_capacity(2, 32);
        chunk
            .push_raw(CF_BLOCK_HEADERS, b"header-key", b"header-value")
            .expect("push header");
        chunk
            .push_serialized(ckbadger_store::CF_BLOCK_HASH_INDEX, b"hash-key", &42_i64)
            .expect("push serialized value");

        assert_eq!(chunk.row_count(), 2);
        assert_eq!(chunk.allocation_count(), 2);
        let mut decoded = Vec::new();
        chunk
            .consume(|cf_name, key, value| {
                decoded.push((cf_name, key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("consume encoded rows");
        assert_eq!(
            decoded[0],
            (
                CF_BLOCK_HEADERS,
                b"header-key".to_vec(),
                b"header-value".to_vec()
            )
        );
        assert_eq!(decoded[1].0, ckbadger_store::CF_BLOCK_HASH_INDEX);
        assert_eq!(decoded[1].1, b"hash-key".to_vec());
        assert_eq!(bincode::deserialize::<i64>(&decoded[1].2).unwrap(), 42);
    }

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
    fn bounded_final_snapshot_sink_flushes_before_rows_can_accumulate() {
        let root = super::super::unique_temp_test_dir("bounded-final-snapshot");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append_store = CkbadgerStore::open_append_only(&append_path).unwrap();
        let mut materializer = Materializer::new(&domain_store, &append_store);
        let byte_limit = 512;

        let stats = materializer
            .materialize_final_snapshot_bounded_for_test(byte_limit, |sink| {
                for i in 0u32..100 {
                    sink.push(MaterializedRow::new(
                        CF_LIVE_CELLS,
                        i.to_be_bytes().to_vec(),
                        vec![0xA5; 64],
                    ))?;
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(stats.rows, 100);
        assert!(stats.flushes > 1);
        assert!(
            stats.peak_batch_bytes <= byte_limit + 128,
            "peak={} limit={}",
            stats.peak_batch_bytes,
            byte_limit
        );
        for i in 0u32..100 {
            assert_eq!(
                domain_store
                    .get_cf(domain_store.cf_live_cells(), &i.to_be_bytes())
                    .unwrap(),
                Some(vec![0xA5; 64])
            );
        }

        let report = materializer.finish();
        assert_eq!(report.final_snapshot_rows, 100);
        assert_eq!(report.final_snapshot_flushes, stats.flushes);
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

        let handle = FlushChannelHandle::new(
            4,
            16 * 1024 * 1024,
            domain_store.clone(),
            append_store.clone(),
        )
        .unwrap();

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
                history_chunks: vec![EncodedHistoryChunk::from_materialized_rows(vec![
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
                ])
                .unwrap()],
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
        let handle = FlushChannelHandle::new(
            2,
            16 * 1024 * 1024,
            domain_store.clone(),
            append_store.clone(),
        )
        .unwrap();

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
                history_chunks: vec![EncodedHistoryChunk::from_materialized_rows(vec![
                    MaterializedRow::new(CF_CELLS, outpoint_key.to_vec(), cell_bytes.clone()),
                ])
                .unwrap()],
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

    #[tokio::test]
    async fn flush_channel_rejects_one_message_larger_than_its_byte_budget() {
        let root = super::super::unique_temp_test_dir("flush-channel-byte-budget");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();

        let domain_store = Arc::new(CkbadgerStore::open_domain(&domain_path).unwrap());
        let append_store = Arc::new(CkbadgerStore::open_append_only(&append_path).unwrap());
        let handle = FlushChannelHandle::new(4, 1024 * 1024, domain_store, append_store).unwrap();
        let pending = super::super::PendingFlush {
            history_chunks: vec![EncodedHistoryChunk::from_materialized_rows(vec![
                MaterializedRow::new(
                    CF_BLOCK_HEADERS,
                    b"oversized".to_vec(),
                    vec![0x55; 2 * 1024 * 1024],
                ),
            ])
            .unwrap()],
            sealed_rows: Vec::new(),
        };

        let error = handle.send(pending).await.unwrap_err().to_string();
        assert!(error.contains("flush queue byte budget"), "{error}");
        assert!(error.contains("message_bytes="), "{error}");
        handle.close_and_wait().await.unwrap();
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

        let history_chunks =
            vec![EncodedHistoryChunk::from_materialized_rows(history_rows)
                .expect("encode history rows")];
        let prepared =
            prepare_flush(&domain_store, &append_store, history_chunks, sealed_rows).unwrap();

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
