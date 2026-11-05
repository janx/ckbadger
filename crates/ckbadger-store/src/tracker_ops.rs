//! The HODL wave and cell-distribution trackers' persisted state (store
//! refactor lane B, B-4).
//!
//! Each tracker is a set of small rows plus one cursor row in `sync_meta`:
//!
//! - HODL wave: `hodl_capacity_by_date` (UTC+8 creation day `YYYYMMDD` -> live
//!   capacity, i128 > 0) and `sync_meta` `hodl_cursor` (`HodlTrackerCursor`).
//! - Cell distribution: `cell_dist_cohort_by_month` (cohort month `YYYY-MM` ->
//!   `(used_capacity, balance)`, two i128 >= 0) and `sync_meta`
//!   `cell_dist_cursor` (`CellDistTrackerCursor`).
//!
//! The live writer writes, in the scope of each block, only the rows that
//! block moves plus the cursor, so the block journal holds a few small
//! entries per block instead of the whole tracker. Every encode and decode
//! below validates the row, so a malformed row fails where it is read or
//! written, with its key.

use anyhow::{anyhow, ensure, Context, Result};
use chrono::NaiveDate;
use rocksdb::IteratorMode;

use crate::bytes_to_hex;
use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;
use crate::types::{
    CellDistTrackerCursor, CellDistributionTrackerState, HodlTrackerCursor, HodlTrackerState,
};

const DAY_KEY_LEN: usize = 8;
const MONTH_KEY_LEN: usize = 7;
const HODL_VALUE_LEN: usize = 16;
const COHORT_VALUE_LEN: usize = 32;

/// `hodl_capacity_by_date` key of a creation day: `YYYYMMDD`, 8 ASCII bytes.
pub(crate) fn encode_hodl_day_key(day: NaiveDate) -> Result<[u8; DAY_KEY_LEN]> {
    let text = day.format("%Y%m%d").to_string();
    text.as_bytes()
        .try_into()
        .map_err(|_| anyhow!("hodl_capacity_by_date: day {day} does not format as YYYYMMDD"))
}

pub(crate) fn decode_hodl_day_key(key: &[u8]) -> Result<NaiveDate> {
    ensure!(
        key.len() == DAY_KEY_LEN && key.iter().all(u8::is_ascii_digit),
        "hodl_capacity_by_date key is not YYYYMMDD: 0x{}",
        bytes_to_hex(key)
    );
    let text = std::str::from_utf8(key).expect("ASCII digits are UTF-8");
    NaiveDate::parse_from_str(text, "%Y%m%d")
        .map_err(|e| anyhow!("hodl_capacity_by_date key {text} is not a calendar day: {e}"))
}

/// A creation day's live capacity: a row exists only while the day still
/// holds live capacity, so the value is always positive.
pub(crate) fn encode_hodl_capacity(day: NaiveDate, capacity: i128) -> Result<[u8; HODL_VALUE_LEN]> {
    ensure!(
        capacity > 0,
        "hodl_capacity_by_date {day}: capacity {capacity} is not positive (an emptied day has no row)"
    );
    Ok(capacity.to_le_bytes())
}

pub(crate) fn decode_hodl_capacity(key: &[u8], value: &[u8]) -> Result<i128> {
    let bytes: [u8; HODL_VALUE_LEN] = value.try_into().map_err(|_| {
        anyhow!(
            "hodl_capacity_by_date row 0x{}: value is {} bytes, expected {HODL_VALUE_LEN}",
            bytes_to_hex(key),
            value.len()
        )
    })?;
    let capacity = i128::from_le_bytes(bytes);
    ensure!(
        capacity > 0,
        "hodl_capacity_by_date row 0x{}: capacity {capacity} is not positive",
        bytes_to_hex(key)
    );
    Ok(capacity)
}

/// A cohort month as the tracker keys it: `YYYY-MM`, month 01..=12.
pub(crate) fn validate_cohort_month(month: &[u8]) -> Result<()> {
    let well_formed = month.len() == MONTH_KEY_LEN
        && month[..4].iter().all(u8::is_ascii_digit)
        && month[4] == b'-'
        && month[5..].iter().all(u8::is_ascii_digit)
        && matches!((month[5] - b'0') * 10 + (month[6] - b'0'), 1..=12);
    ensure!(
        well_formed,
        "cell_dist_cohort_by_month key is not YYYY-MM: 0x{}",
        bytes_to_hex(month)
    );
    Ok(())
}

pub(crate) fn encode_cohort(
    month: &str,
    used: i128,
    balance: i128,
) -> Result<[u8; COHORT_VALUE_LEN]> {
    validate_cohort_month(month.as_bytes())?;
    ensure!(
        used >= 0 && balance >= 0,
        "cell_dist_cohort_by_month {month}: negative accumulator used={used} balance={balance}"
    );
    let mut value = [0u8; COHORT_VALUE_LEN];
    value[..16].copy_from_slice(&used.to_le_bytes());
    value[16..].copy_from_slice(&balance.to_le_bytes());
    Ok(value)
}

pub(crate) fn decode_cohort(key: &[u8], value: &[u8]) -> Result<(String, i128, i128)> {
    validate_cohort_month(key)?;
    ensure!(
        value.len() == COHORT_VALUE_LEN,
        "cell_dist_cohort_by_month row {}: value is {} bytes, expected {COHORT_VALUE_LEN}",
        String::from_utf8_lossy(key),
        value.len()
    );
    let used = i128::from_le_bytes(value[..16].try_into().expect("16 bytes"));
    let balance = i128::from_le_bytes(value[16..].try_into().expect("16 bytes"));
    let month = String::from_utf8(key.to_vec()).expect("validated ASCII");
    ensure!(
        used >= 0 && balance >= 0,
        "cell_dist_cohort_by_month row {month}: negative accumulator used={used} balance={balance}"
    );
    Ok((month, used, balance))
}

fn check_snapshot_day(tracker: &str, day: &str) -> Result<()> {
    NaiveDate::parse_from_str(day, "%Y%m%d")
        .map(drop)
        .map_err(|e| anyhow!("{tracker} cursor: last_snapshot_date {day:?} is not YYYYMMDD: {e}"))
}

fn check_hodl_cursor(cursor: &HodlTrackerCursor) -> Result<()> {
    ensure!(
        cursor.holder_count >= 0 && cursor.last_processed_block >= 0,
        "hodl cursor: negative holder_count={} or last_processed_block={}",
        cursor.holder_count,
        cursor.last_processed_block
    );
    check_snapshot_day("hodl", &cursor.last_snapshot_date)
}

fn check_cell_dist_cursor(cursor: &CellDistTrackerCursor) -> Result<()> {
    ensure!(
        cursor.count_by_bucket.iter().all(|count| *count >= 0)
            && cursor.total_capacity_by_bucket.iter().all(|cap| *cap >= 0)
            && cursor.last_processed_block >= 0,
        "cell_dist cursor: negative bucket count / capacity or last_processed_block: {cursor:?}"
    );
    check_snapshot_day("cell_dist", &cursor.last_snapshot_date)
}

pub(crate) fn encode_hodl_cursor(cursor: &HodlTrackerCursor) -> Result<Vec<u8>> {
    check_hodl_cursor(cursor)?;
    bincode::serialize(cursor).context("encode hodl cursor")
}

pub(crate) fn decode_hodl_cursor(value: &[u8]) -> Result<HodlTrackerCursor> {
    let cursor: HodlTrackerCursor = bincode::deserialize(value).context("decode hodl cursor")?;
    check_hodl_cursor(&cursor)?;
    Ok(cursor)
}

pub(crate) fn encode_cell_dist_cursor(cursor: &CellDistTrackerCursor) -> Result<Vec<u8>> {
    check_cell_dist_cursor(cursor)?;
    bincode::serialize(cursor).context("encode cell_dist cursor")
}

pub(crate) fn decode_cell_dist_cursor(value: &[u8]) -> Result<CellDistTrackerCursor> {
    let cursor: CellDistTrackerCursor =
        bincode::deserialize(value).context("decode cell_dist cursor")?;
    check_cell_dist_cursor(&cursor)?;
    Ok(cursor)
}

/// What [`CkbadgerStore::convert_tracker_blobs_to_rows`] did: for each
/// tracker, the number of rows written from its legacy blob, or `None` when
/// there was no blob.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TrackerBlobConversion {
    pub hodl_rows: Option<usize>,
    pub cell_dist_rows: Option<usize>,
}

impl CkbadgerStore {
    /// Every `hodl_capacity_by_date` row, by ascending creation day.
    pub fn list_hodl_capacity_by_date(&self) -> Result<Vec<(NaiveDate, i128)>> {
        let mut rows = Vec::new();
        for item in self.iterator_cf(self.cf_hodl_capacity_by_date(), IteratorMode::Start) {
            let (key, value) = item.context("iterate hodl_capacity_by_date")?;
            rows.push((
                decode_hodl_day_key(&key)?,
                decode_hodl_capacity(&key, &value)?,
            ));
        }
        Ok(rows)
    }

    /// One creation day's live capacity, `None` when the day holds none.
    pub fn get_hodl_capacity_by_date(&self, day: NaiveDate) -> Result<Option<i128>> {
        let key = encode_hodl_day_key(day)?;
        self.get_cf(self.cf_hodl_capacity_by_date(), &key)?
            .map(|value| decode_hodl_capacity(&key, &value))
            .transpose()
    }

    /// Every `cell_dist_cohort_by_month` row `(month, used, balance)`, by
    /// ascending month.
    pub fn list_cell_dist_cohorts(&self) -> Result<Vec<(String, i128, i128)>> {
        let mut rows = Vec::new();
        for item in self.iterator_cf(self.cf_cell_dist_cohort_by_month(), IteratorMode::Start) {
            let (key, value) = item.context("iterate cell_dist_cohort_by_month")?;
            rows.push(decode_cohort(&key, &value)?);
        }
        Ok(rows)
    }

    /// One cohort month's `(used, balance)`.
    pub fn get_cell_dist_cohort(&self, month: &str) -> Result<Option<(i128, i128)>> {
        validate_cohort_month(month.as_bytes())?;
        self.get_cf(self.cf_cell_dist_cohort_by_month(), month.as_bytes())?
            .map(|value| decode_cohort(month.as_bytes(), &value).map(|(_, used, bal)| (used, bal)))
            .transpose()
    }

    pub fn get_hodl_cursor(&self) -> Result<Option<HodlTrackerCursor>> {
        self.get_cf(self.cf_sync_meta(), sync_meta_keys::HODL_CURSOR)?
            .map(|value| decode_hodl_cursor(&value))
            .transpose()
    }

    pub fn get_cell_dist_cursor(&self) -> Result<Option<CellDistTrackerCursor>> {
        self.get_cf(self.cf_sync_meta(), sync_meta_keys::CELL_DIST_CURSOR)?
            .map(|value| decode_cell_dist_cursor(&value))
            .transpose()
    }

    /// Whether a tracker has any persisted row: its cursor or one row of its
    /// column family.
    fn tracker_rows_exist(&self, cf: &rocksdb::ColumnFamily, cursor_key: &[u8]) -> Result<bool> {
        if self.get_cf(self.cf_sync_meta(), cursor_key)?.is_some() {
            return Ok(true);
        }
        match self.iterator_cf(cf, IteratorMode::Start).next() {
            Some(item) => item.map(|_| true).context("probe a tracker column family"),
            None => Ok(false),
        }
    }

    /// ONE-TIME CONVERSION, the only one the store refactor allows
    /// (docs/superpowers/plans/2026-11-04-store-cf-refactor-plan.md, B-4);
    /// deleted, with the legacy blob types and keys, after that plan's
    /// re-sync.
    ///
    /// For each tracker whose legacy whole-state blob (`sync_meta`
    /// `hodl_tracker` / `cell_dist_tracker`) exists: write the blob's
    /// `(date, capacity)` / `(month, used, balance)` entries as rows and its
    /// scalars as the cursor row, and delete the blob, in one atomic write.
    /// No value is computed: every row is a blob entry, validated by the row
    /// codecs. A blob beside existing rows (cursor or any row) is an
    /// invariant violation — two persisted states of one tracker — and fails
    /// without writing; so does a blob without the scalars a cursor needs.
    /// Without a blob nothing is read or written.
    pub fn convert_tracker_blobs_to_rows(&self) -> Result<TrackerBlobConversion> {
        // An unjournaled commit: OUTSIDE_JOURNAL covers the rows, the cursors
        // and the blobs, so the journal discipline checks every write.
        let mut batch = crate::StoreBatch::new(self);
        let mut result = TrackerBlobConversion::default();

        if let Some(blob) = self.get_cf(self.cf_sync_meta(), sync_meta_keys::HODL_TRACKER)? {
            ensure!(
                !self.tracker_rows_exist(self.cf_hodl_capacity_by_date(), sync_meta_keys::HODL_CURSOR)?,
                "the legacy hodl_tracker blob and hodl_capacity_by_date rows / hodl_cursor both exist: \
                 two persisted HODL tracker states; delete RocksDB and re-sync from genesis"
            );
            let state: HodlTrackerState =
                bincode::deserialize(&blob).context("decode the legacy hodl_tracker blob")?;
            let mut days = std::collections::BTreeSet::new();
            for (date, capacity) in &state.capacity_by_date {
                let day = decode_hodl_day_key(date.as_bytes()).with_context(|| {
                    format!("legacy hodl_tracker capacity_by_date entry {date:?}")
                })?;
                ensure!(
                    days.insert(day),
                    "legacy hodl_tracker: day {date} appears twice"
                );
                batch.put_raw_cf_by_name(
                    crate::store::CF_HODL_CAPACITY_BY_DATE,
                    &encode_hodl_day_key(day)?,
                    &encode_hodl_capacity(day, *capacity)?,
                )?;
            }
            let cursor = HodlTrackerCursor {
                holder_count: state.holder_count,
                last_snapshot_date: state
                    .last_snapshot_date
                    .ok_or_else(|| anyhow!("legacy hodl_tracker blob has no last_snapshot_date"))?,
                last_processed_block: state.last_processed_block.ok_or_else(|| {
                    anyhow!("legacy hodl_tracker blob has no last_processed_block")
                })?,
            };
            batch.put_raw_cf_by_name(
                crate::store::CF_SYNC_META,
                sync_meta_keys::HODL_CURSOR,
                &encode_hodl_cursor(&cursor)?,
            )?;
            batch
                .delete_raw_cf_by_name(crate::store::CF_SYNC_META, sync_meta_keys::HODL_TRACKER)?;
            result.hodl_rows = Some(days.len());
        }

        if let Some(blob) = self.get_cf(self.cf_sync_meta(), sync_meta_keys::CELL_DIST_TRACKER)? {
            ensure!(
                !self.tracker_rows_exist(
                    self.cf_cell_dist_cohort_by_month(),
                    sync_meta_keys::CELL_DIST_CURSOR
                )?,
                "the legacy cell_dist_tracker blob and cell_dist_cohort_by_month rows / \
                 cell_dist_cursor both exist: two persisted cell-distribution tracker states; \
                 delete RocksDB and re-sync from genesis"
            );
            let state: CellDistributionTrackerState =
                bincode::deserialize(&blob).context("decode the legacy cell_dist_tracker blob")?;
            let mut months = std::collections::BTreeSet::new();
            for (month, used, balance) in &state.cohort_accum {
                ensure!(
                    months.insert(month.clone()),
                    "legacy cell_dist_tracker: cohort month {month} appears twice"
                );
                batch.put_raw_cf_by_name(
                    crate::store::CF_CELL_DIST_COHORT_BY_MONTH,
                    month.as_bytes(),
                    &encode_cohort(month, *used, *balance)?,
                )?;
            }
            let cursor = CellDistTrackerCursor {
                count_by_bucket: state.count_by_bucket,
                total_capacity_by_bucket: state.total_capacity_by_bucket,
                last_snapshot_date: state.last_snapshot_date.ok_or_else(|| {
                    anyhow!("legacy cell_dist_tracker blob has no last_snapshot_date")
                })?,
                last_processed_block: state.last_processed_block.ok_or_else(|| {
                    anyhow!("legacy cell_dist_tracker blob has no last_processed_block")
                })?,
            };
            batch.put_raw_cf_by_name(
                crate::store::CF_SYNC_META,
                sync_meta_keys::CELL_DIST_CURSOR,
                &encode_cell_dist_cursor(&cursor)?,
            )?;
            batch.delete_raw_cf_by_name(
                crate::store::CF_SYNC_META,
                sync_meta_keys::CELL_DIST_TRACKER,
            )?;
            result.cell_dist_rows = Some(months.len());
        }

        if !batch.is_empty() {
            batch.commit()?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreBatch;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y%m%d").unwrap()
    }

    fn open() -> (CkbadgerStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (CkbadgerStore::open_domain(dir.path()).unwrap(), dir)
    }

    fn hodl_blob() -> HodlTrackerState {
        HodlTrackerState {
            capacity_by_date: vec![
                ("20240116".to_string(), 300_00000000),
                ("20240115".to_string(), 500_00000000),
            ],
            date_transitions: vec![(100, "20240115".to_string()), (200, "20240116".to_string())],
            holder_count: 42,
            last_snapshot_date: Some("20240116".to_string()),
            last_processed_block: Some(250),
        }
    }

    fn cell_dist_blob() -> CellDistributionTrackerState {
        CellDistributionTrackerState {
            count_by_bucket: [1, 2, 0, 0, 0, 0],
            total_capacity_by_bucket: [61_00000000, 300_00000000, 0, 0, 0, 0],
            date_transitions: vec![(100, "20240115".to_string())],
            last_snapshot_date: Some("20240116".to_string()),
            cohort_accum: vec![
                ("2024-01".to_string(), 122_00000000, 140_00000000),
                ("2023-12".to_string(), 0, 0),
            ],
            last_processed_block: Some(250),
        }
    }

    fn put_blob<T: serde::Serialize>(store: &CkbadgerStore, key: &[u8], blob: &T) {
        store
            .put_cf(
                store.cf_sync_meta(),
                key,
                &bincode::serialize(blob).unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn row_codecs_round_trip_and_refuse_malformed_rows() {
        let key = encode_hodl_day_key(day("20240115")).unwrap();
        assert_eq!(&key, b"20240115");
        assert_eq!(decode_hodl_day_key(&key).unwrap(), day("20240115"));
        assert!(decode_hodl_day_key(b"2024011").is_err());
        assert!(decode_hodl_day_key(b"20241301").is_err());
        assert!(decode_hodl_day_key(b"2024-115").is_err());

        let value = encode_hodl_capacity(day("20240115"), 7).unwrap();
        assert_eq!(decode_hodl_capacity(&key, &value).unwrap(), 7);
        assert!(encode_hodl_capacity(day("20240115"), 0).is_err());
        assert!(encode_hodl_capacity(day("20240115"), -1).is_err());
        assert!(decode_hodl_capacity(&key, &0i128.to_le_bytes()).is_err());
        assert!(decode_hodl_capacity(&key, &[1u8; 15]).is_err());

        let value = encode_cohort("2024-01", 5, 9).unwrap();
        assert_eq!(
            decode_cohort(b"2024-01", &value).unwrap(),
            ("2024-01".to_string(), 5, 9)
        );
        assert!(encode_cohort("2024-01", 0, 0).is_ok());
        assert!(encode_cohort("2024-01", -1, 0).is_err());
        assert!(encode_cohort("2024-13", 1, 1).is_err());
        assert!(encode_cohort("2024-00", 1, 1).is_err());
        assert!(encode_cohort("202401", 1, 1).is_err());
        assert!(decode_cohort(b"2024-01", &[0u8; 31]).is_err());

        let cursor = HodlTrackerCursor {
            holder_count: 3,
            last_snapshot_date: "20240115".to_string(),
            last_processed_block: 9,
        };
        assert_eq!(
            decode_hodl_cursor(&encode_hodl_cursor(&cursor).unwrap()).unwrap(),
            cursor
        );
        assert!(encode_hodl_cursor(&HodlTrackerCursor {
            holder_count: -1,
            ..cursor.clone()
        })
        .is_err());
        assert!(encode_hodl_cursor(&HodlTrackerCursor {
            last_snapshot_date: "2024-01-15".to_string(),
            ..cursor
        })
        .is_err());
        let cursor = CellDistTrackerCursor {
            count_by_bucket: [1, 0, 0, 0, 0, 0],
            total_capacity_by_bucket: [61, 0, 0, 0, 0, 0],
            last_snapshot_date: "20240115".to_string(),
            last_processed_block: 9,
        };
        assert_eq!(
            decode_cell_dist_cursor(&encode_cell_dist_cursor(&cursor).unwrap()).unwrap(),
            cursor
        );
        assert!(encode_cell_dist_cursor(&CellDistTrackerCursor {
            count_by_bucket: [-1, 0, 0, 0, 0, 0],
            ..cursor
        })
        .is_err());
    }

    #[test]
    fn tracker_rows_and_cursors_round_trip_through_a_batch() {
        let (store, _dir) = open();
        assert!(store.list_hodl_capacity_by_date().unwrap().is_empty());
        assert!(store.get_hodl_cursor().unwrap().is_none());
        assert!(store.get_cell_dist_cursor().unwrap().is_none());

        let hodl_cursor = HodlTrackerCursor {
            holder_count: 2,
            last_snapshot_date: "20240116".to_string(),
            last_processed_block: 12,
        };
        let cell_dist_cursor = CellDistTrackerCursor {
            count_by_bucket: [2, 0, 0, 0, 0, 0],
            total_capacity_by_bucket: [122, 0, 0, 0, 0, 0],
            last_snapshot_date: "20240116".to_string(),
            last_processed_block: 12,
        };
        let mut batch = StoreBatch::new(&store);
        batch
            .put_hodl_capacity_by_date(day("20240116"), 30)
            .unwrap();
        batch
            .put_hodl_capacity_by_date(day("20240115"), 50)
            .unwrap();
        batch.put_cell_dist_cohort("2024-01", 122, 140).unwrap();
        batch.put_cell_dist_cohort("2023-12", 0, 0).unwrap();
        batch.put_hodl_cursor(&hodl_cursor).unwrap();
        batch.put_cell_dist_cursor(&cell_dist_cursor).unwrap();
        assert!(batch.put_hodl_capacity_by_date(day("20240117"), 0).is_err());
        batch.commit().unwrap();

        assert_eq!(
            store.list_hodl_capacity_by_date().unwrap(),
            vec![(day("20240115"), 50), (day("20240116"), 30)]
        );
        assert_eq!(
            store.get_hodl_capacity_by_date(day("20240116")).unwrap(),
            Some(30)
        );
        assert_eq!(
            store.get_hodl_capacity_by_date(day("20240117")).unwrap(),
            None
        );
        assert_eq!(
            store.list_cell_dist_cohorts().unwrap(),
            vec![
                ("2023-12".to_string(), 0, 0),
                ("2024-01".to_string(), 122, 140)
            ]
        );
        assert_eq!(
            store.get_cell_dist_cohort("2024-01").unwrap(),
            Some((122, 140))
        );
        assert_eq!(store.get_cell_dist_cohort("2024-02").unwrap(), None);
        assert_eq!(store.get_hodl_cursor().unwrap(), Some(hodl_cursor));
        assert_eq!(
            store.get_cell_dist_cursor().unwrap(),
            Some(cell_dist_cursor)
        );

        let mut batch = StoreBatch::new(&store);
        batch.delete_hodl_capacity_by_date(day("20240115")).unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store.list_hodl_capacity_by_date().unwrap(),
            vec![(day("20240116"), 30)]
        );
    }

    /// The one-time conversion writes the blob's entries as rows and its
    /// scalars as the cursor, and deletes the blob.
    #[test]
    fn a_tracker_blob_without_rows_becomes_rows_and_a_cursor() {
        let (store, _dir) = open();
        put_blob(&store, sync_meta_keys::HODL_TRACKER, &hodl_blob());
        put_blob(&store, sync_meta_keys::CELL_DIST_TRACKER, &cell_dist_blob());

        let converted = store.convert_tracker_blobs_to_rows().unwrap();
        assert_eq!(
            converted,
            TrackerBlobConversion {
                hodl_rows: Some(2),
                cell_dist_rows: Some(2)
            }
        );
        assert_eq!(
            store.list_hodl_capacity_by_date().unwrap(),
            vec![
                (day("20240115"), 500_00000000),
                (day("20240116"), 300_00000000)
            ]
        );
        assert_eq!(
            store.get_hodl_cursor().unwrap(),
            Some(HodlTrackerCursor {
                holder_count: 42,
                last_snapshot_date: "20240116".to_string(),
                last_processed_block: 250,
            })
        );
        assert_eq!(
            store.list_cell_dist_cohorts().unwrap(),
            vec![
                ("2023-12".to_string(), 0, 0),
                ("2024-01".to_string(), 122_00000000, 140_00000000)
            ]
        );
        assert_eq!(
            store.get_cell_dist_cursor().unwrap(),
            Some(CellDistTrackerCursor {
                count_by_bucket: [1, 2, 0, 0, 0, 0],
                total_capacity_by_bucket: [61_00000000, 300_00000000, 0, 0, 0, 0],
                last_snapshot_date: "20240116".to_string(),
                last_processed_block: 250,
            })
        );
        for key in [
            sync_meta_keys::HODL_TRACKER,
            sync_meta_keys::CELL_DIST_TRACKER,
        ] {
            assert!(store.get_cf(store.cf_sync_meta(), key).unwrap().is_none());
        }

        // A second start finds no blob and changes nothing.
        assert_eq!(
            store.convert_tracker_blobs_to_rows().unwrap(),
            TrackerBlobConversion::default()
        );
    }

    /// Rows without a blob are the post-conversion state: untouched.
    #[test]
    fn tracker_rows_without_a_blob_are_left_untouched() {
        let (store, _dir) = open();
        let mut batch = StoreBatch::new(&store);
        batch
            .put_hodl_capacity_by_date(day("20240115"), 50)
            .unwrap();
        batch
            .put_hodl_cursor(&HodlTrackerCursor {
                holder_count: 1,
                last_snapshot_date: "20240115".to_string(),
                last_processed_block: 3,
            })
            .unwrap();
        batch.commit().unwrap();
        let before = (
            store.list_hodl_capacity_by_date().unwrap(),
            store.get_hodl_cursor().unwrap(),
        );
        assert_eq!(
            store.convert_tracker_blobs_to_rows().unwrap(),
            TrackerBlobConversion::default()
        );
        assert_eq!(
            (
                store.list_hodl_capacity_by_date().unwrap(),
                store.get_hodl_cursor().unwrap()
            ),
            before
        );
    }

    /// A blob beside rows is two persisted states of one tracker: an
    /// invariant violation, refused without writing anything.
    #[test]
    fn a_tracker_blob_beside_rows_is_refused() {
        let (store, _dir) = open();
        put_blob(&store, sync_meta_keys::HODL_TRACKER, &hodl_blob());
        let mut batch = StoreBatch::new(&store);
        batch.put_hodl_capacity_by_date(day("20240101"), 9).unwrap();
        batch.commit().unwrap();
        let error = store.convert_tracker_blobs_to_rows().unwrap_err();
        assert!(error.to_string().contains("both exist"), "{error}");
        assert!(store
            .get_cf(store.cf_sync_meta(), sync_meta_keys::HODL_TRACKER)
            .unwrap()
            .is_some());
        assert!(store.get_hodl_cursor().unwrap().is_none());

        let (store, _dir) = open();
        put_blob(&store, sync_meta_keys::CELL_DIST_TRACKER, &cell_dist_blob());
        let mut batch = StoreBatch::new(&store);
        batch
            .put_cell_dist_cursor(&CellDistTrackerCursor {
                count_by_bucket: [0; 6],
                total_capacity_by_bucket: [0; 6],
                last_snapshot_date: "20240101".to_string(),
                last_processed_block: 1,
            })
            .unwrap();
        batch.commit().unwrap();
        let error = store.convert_tracker_blobs_to_rows().unwrap_err();
        assert!(error.to_string().contains("both exist"), "{error}");
        assert!(store.list_cell_dist_cohorts().unwrap().is_empty());
    }

    /// The conversion computes nothing: a blob it cannot copy as valid rows
    /// fails, and nothing is written.
    #[test]
    fn a_malformed_tracker_blob_fails_the_conversion() {
        let (store, _dir) = open();
        let mut blob = hodl_blob();
        blob.last_processed_block = None;
        put_blob(&store, sync_meta_keys::HODL_TRACKER, &blob);
        assert!(store.convert_tracker_blobs_to_rows().is_err());
        assert!(store.list_hodl_capacity_by_date().unwrap().is_empty());

        let (store, _dir) = open();
        let mut blob = hodl_blob();
        blob.capacity_by_date.push(("20240117".to_string(), 0));
        put_blob(&store, sync_meta_keys::HODL_TRACKER, &blob);
        assert!(store.convert_tracker_blobs_to_rows().is_err());
        assert!(store.get_hodl_cursor().unwrap().is_none());

        let (store, _dir) = open();
        let mut blob = cell_dist_blob();
        blob.cohort_accum.push(("2024-01".to_string(), 1, 1));
        put_blob(&store, sync_meta_keys::CELL_DIST_TRACKER, &blob);
        let error = store.convert_tracker_blobs_to_rows().unwrap_err();
        assert!(error.to_string().contains("appears twice"), "{error}");
    }
}
