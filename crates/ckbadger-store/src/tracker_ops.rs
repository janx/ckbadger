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
use crate::types::{CellDistTrackerCursor, HodlTrackerCursor};

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
}
