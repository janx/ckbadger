use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, Result};

use crate::CkbadgerStore;

/// A batch of (cf_name, key, value) entries to be written as sorted SST files
/// and ingested into a RocksDB store.
pub struct SstIngestBatch {
    entries: Vec<(&'static str, Vec<u8>, Vec<u8>)>,
}

/// Result of an SST ingest operation.
#[derive(Debug, Default)]
pub struct SstIngestResult {
    pub files_written: usize,
    pub rows_ingested: usize,
    pub sst_write_ms: f64,
    pub ingest_ms: f64,
}

impl Default for SstIngestBatch {
    fn default() -> Self {
        Self::new()
    }
}

impl SstIngestBatch {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
        }
    }

    pub fn push(&mut self, cf_name: &'static str, key: Vec<u8>, value: Vec<u8>) {
        self.entries.push((cf_name, key, value));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Sort entries by (cf_name, key), write one SST file per CF, and ingest
    /// all files into `store`.
    ///
    /// `sst_dir` must be on the same filesystem as the store's data directory
    /// so that `move_files=true` can use rename(2) instead of copy.
    /// The directory must already exist.
    ///
    /// `batch_id` is used to create unique SST filenames within `sst_dir`.
    pub fn write_and_ingest(
        mut self,
        store: &CkbadgerStore,
        sst_dir: &Path,
        batch_id: u64,
    ) -> Result<SstIngestResult> {
        if self.entries.is_empty() {
            return Ok(SstIngestResult::default());
        }

        // Sort by (cf_name, key) using lexicographic str comparison on cf_name
        // to group same-CF entries together, then bytewise ascending on key
        // to satisfy SstFileWriter's sorted-key requirement.
        self.entries
            .sort_unstable_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(&b.1)));

        let total_rows = self.entries.len();
        let sst_write_started = Instant::now();
        let mut sst_files: Vec<(&'static str, PathBuf)> = Vec::new();
        let mut file_idx: u32 = 0;

        // Write one SST file per contiguous CF group.
        let mut i = 0;
        while i < self.entries.len() {
            let cf_name = self.entries[i].0;
            let group_start = i;

            // Find end of this CF group
            while i < self.entries.len() && self.entries[i].0 == cf_name {
                i += 1;
            }

            let sst_path = sst_dir.join(format!("batch{}-{}-{}.sst", batch_id, file_idx, cf_name));
            file_idx += 1;

            let opts = CkbadgerStore::cf_options_for_sst(cf_name);
            let mut writer = rocksdb::SstFileWriter::create(&opts);
            writer.open(&sst_path).map_err(|e| {
                anyhow!(
                    "SstFileWriter::open failed: cf={} path={} error={}",
                    cf_name,
                    sst_path.display(),
                    e
                )
            })?;

            for entry in &self.entries[group_start..i] {
                writer.put(&entry.1, &entry.2).map_err(|e| {
                    anyhow!(
                        "SstFileWriter::put failed: cf={} key_len={} error={}",
                        cf_name,
                        entry.1.len(),
                        e
                    )
                })?;
            }

            writer.finish().map_err(|e| {
                anyhow!(
                    "SstFileWriter::finish failed: cf={} path={} error={}",
                    cf_name,
                    sst_path.display(),
                    e
                )
            })?;

            sst_files.push((cf_name, sst_path));
        }

        let sst_write_ms = sst_write_started.elapsed().as_secs_f64() * 1000.0;

        // Ingest each SST file into its target CF.
        let ingest_started = Instant::now();
        for (cf_name, sst_path) in &sst_files {
            store.ingest_sst_files_cf(cf_name, vec![sst_path.clone()])?;
        }
        let ingest_ms = ingest_started.elapsed().as_secs_f64() * 1000.0;

        Ok(SstIngestResult {
            files_written: sst_files.len(),
            rows_ingested: total_rows,
            sst_write_ms,
            ingest_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CF_BLOCK_HEADERS, CF_CONSUMED_CELLS, CF_TX_INDEX};

    #[test]
    fn empty_batch_is_noop() {
        let root = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(root.path().join("db")).unwrap();
        let sst_dir = root.path().join("sst");
        std::fs::create_dir_all(&sst_dir).unwrap();

        let batch = SstIngestBatch::new();
        let result = batch.write_and_ingest(&store, &sst_dir, 0).unwrap();
        assert_eq!(result.files_written, 0);
        assert_eq!(result.rows_ingested, 0);
    }

    #[test]
    fn single_cf_sorts_and_ingests() {
        let root = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(root.path().join("db")).unwrap();
        let sst_dir = root.path().join("sst");
        std::fs::create_dir_all(&sst_dir).unwrap();

        let mut batch = SstIngestBatch::new();
        // Insert out of order — the batch must sort them
        batch.push(CF_BLOCK_HEADERS, b"key_ccc".to_vec(), b"val_c".to_vec());
        batch.push(CF_BLOCK_HEADERS, b"key_aaa".to_vec(), b"val_a".to_vec());
        batch.push(CF_BLOCK_HEADERS, b"key_bbb".to_vec(), b"val_b".to_vec());

        let result = batch.write_and_ingest(&store, &sst_dir, 1).unwrap();
        assert_eq!(result.files_written, 1);
        assert_eq!(result.rows_ingested, 3);

        let cf = store.cf(CF_BLOCK_HEADERS);
        assert_eq!(
            store.get_cf(cf, b"key_aaa").unwrap().as_deref(),
            Some(b"val_a".as_slice())
        );
        assert_eq!(
            store.get_cf(cf, b"key_bbb").unwrap().as_deref(),
            Some(b"val_b".as_slice())
        );
        assert_eq!(
            store.get_cf(cf, b"key_ccc").unwrap().as_deref(),
            Some(b"val_c".as_slice())
        );
    }

    #[test]
    fn multi_cf_groups_and_ingests() {
        let root = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(root.path().join("db")).unwrap();
        let sst_dir = root.path().join("sst");
        std::fs::create_dir_all(&sst_dir).unwrap();

        let mut batch = SstIngestBatch::new();
        batch.push(CF_TX_INDEX, b"tx_bbb".to_vec(), b"tv_b".to_vec());
        batch.push(CF_BLOCK_HEADERS, b"bh_aaa".to_vec(), b"bv_a".to_vec());
        batch.push(CF_TX_INDEX, b"tx_aaa".to_vec(), b"tv_a".to_vec());
        batch.push(CF_BLOCK_HEADERS, b"bh_bbb".to_vec(), b"bv_b".to_vec());
        batch.push(CF_CONSUMED_CELLS, b"cc_aaa".to_vec(), b"cv_a".to_vec());

        let result = batch.write_and_ingest(&store, &sst_dir, 2).unwrap();
        assert_eq!(result.files_written, 3);
        assert_eq!(result.rows_ingested, 5);

        assert_eq!(
            store
                .get_cf(store.cf(CF_BLOCK_HEADERS), b"bh_aaa")
                .unwrap()
                .as_deref(),
            Some(b"bv_a".as_slice())
        );
        assert_eq!(
            store
                .get_cf(store.cf(CF_TX_INDEX), b"tx_bbb")
                .unwrap()
                .as_deref(),
            Some(b"tv_b".as_slice())
        );
        assert_eq!(
            store
                .get_cf(store.cf(CF_CONSUMED_CELLS), b"cc_aaa")
                .unwrap()
                .as_deref(),
            Some(b"cv_a".as_slice())
        );
    }

    #[test]
    fn sst_files_are_moved_after_ingest() {
        let root = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(root.path().join("db")).unwrap();
        let sst_dir = root.path().join("sst");
        std::fs::create_dir_all(&sst_dir).unwrap();

        let mut batch = SstIngestBatch::new();
        batch.push(CF_BLOCK_HEADERS, b"key_a".to_vec(), b"val_a".to_vec());

        batch.write_and_ingest(&store, &sst_dir, 3).unwrap();

        let remaining: Vec<_> = std::fs::read_dir(&sst_dir).unwrap().collect();
        assert!(
            remaining.is_empty(),
            "SST files should be moved, not copied"
        );
    }

    #[test]
    fn nonexistent_sst_dir_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(root.path().join("db")).unwrap();
        let bad_dir = root.path().join("does-not-exist");

        let mut batch = SstIngestBatch::new();
        batch.push(CF_BLOCK_HEADERS, b"key_a".to_vec(), b"val_a".to_vec());

        let err = batch.write_and_ingest(&store, &bad_dir, 0).unwrap_err();
        assert!(
            err.to_string().contains("SstFileWriter::open failed"),
            "unexpected error: {}",
            err
        );
    }
}
