//! Which stats buckets the API may serve as complete.
//!
//! The rule is [`ckbadger_common::stats_bucket`]'s, the one the bulk indexer
//! seals activity buckets with: a bucket (an hour, or a UTC+8 day) is complete
//! when its end is at or below the median time of the newest 37 indexed
//! blocks. CKB only bounds a block's timestamp by that median time, so a block
//! may be dated before its parent — across an hour or midnight — and neither
//! the tip block's date nor the wall clock says which buckets are final.
//!
//! This module reads the median time from the store, once per request under
//! the request's pinned read view, and judges rows with it. A row dated after
//! the tip block's own day is legal and simply in progress.

use chrono::NaiveDate;
use ckbadger_common::stats_bucket::{
    bucket_end_ms, is_bucket_complete, median_time_ms, utc8_day_end_ms, HOUR_MS,
    MEDIAN_TIME_BLOCK_COUNT,
};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

use crate::response::{ApiError, ApiRouteError};

/// The indexed chain's clock: its tip block and the median time of the newest
/// (at most 37) blocks ending at the tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainTime {
    tip_number: i64,
    tip_timestamp_ms: i64,
    median_time_ms: i64,
}

impl ChainTime {
    /// The store's chain time, `None` while it holds no block.
    pub fn load(store: &CkbadgerStore) -> Result<Option<Self>, ApiRouteError> {
        let headers = store
            .list_blocks_desc(None, MEDIAN_TIME_BLOCK_COUNT)
            .map_err(|e| {
                ApiError::internal(format!("failed to read the newest block headers: {e}"))
            })?;
        if headers.is_empty() {
            return Ok(None);
        }
        let window: Vec<(i64, i64)> = headers
            .iter()
            .map(|(number, header)| (*number, header.timestamp))
            .collect();
        Self::from_window(&window).map(Some)
    }

    /// Build from the newest headers as `(number, timestamp_ms)`, tip first.
    /// The window must be the consecutive blocks ending at the tip — all 37,
    /// or every block down to genesis on a younger chain; a gap is an error.
    pub fn from_window(newest_first: &[(i64, i64)]) -> Result<Self, ApiRouteError> {
        let Some(&(tip_number, tip_timestamp_ms)) = newest_first.first() else {
            return Err(ApiError::internal(
                "cannot derive the chain's median time from an empty header window",
            ));
        };
        let expected_len = usize::try_from(tip_number)
            .ok()
            .and_then(|tip| tip.checked_add(1))
            .map_or(MEDIAN_TIME_BLOCK_COUNT, |blocks| {
                blocks.min(MEDIAN_TIME_BLOCK_COUNT)
            });
        let consecutive = newest_first
            .iter()
            .zip(0i64..)
            .all(|(&(number, _), offset)| Some(number) == tip_number.checked_sub(offset));
        if tip_number < 0 || newest_first.len() != expected_len || !consecutive {
            let numbers: Vec<i64> = newest_first.iter().map(|(number, _)| *number).collect();
            return Err(ApiError::internal(format!(
                "the newest block headers are not the {expected_len} consecutive blocks ending at \
                 tip {tip_number}: got {numbers:?}; the median-time window needs every one"
            )));
        }
        let timestamps: Vec<i64> = newest_first.iter().map(|(_, ts)| *ts).collect();
        let median_time_ms = median_time_ms(&timestamps)
            .map_err(|e| ApiError::internal(format!("tip {tip_number}: {e}")))?;
        Ok(Self {
            tip_number,
            tip_timestamp_ms,
            median_time_ms,
        })
    }

    pub fn tip_number(&self) -> i64 {
        self.tip_number
    }

    pub fn tip_timestamp_ms(&self) -> i64 {
        self.tip_timestamp_ms
    }

    pub fn median_time_ms(&self) -> i64 {
        self.median_time_ms
    }

    /// The tip block's UTC+8 stats day.
    pub fn tip_day(&self) -> Result<NaiveDate, ApiRouteError> {
        utc8_date_of(self.tip_number, self.tip_timestamp_ms)
    }

    /// The newest complete UTC+8 day: the day before the one the median time
    /// falls in (that day can still gain a block; the one before cannot).
    pub fn last_complete_day(&self) -> Result<NaiveDate, ApiRouteError> {
        let median_day = utc8_date_of(self.tip_number, self.median_time_ms)?;
        median_day.pred_opt().ok_or_else(|| {
            ApiError::internal(format!("no day precedes the median-time day {median_day}"))
        })
    }

    /// Start (UTC seconds) of the newest complete hour: the hour before the
    /// one the median time falls in.
    pub fn last_complete_hour_start_secs(&self) -> i64 {
        let median_secs = self.median_time_ms.div_euclid(1000);
        median_secs - median_secs.rem_euclid(3600) - 3600
    }

    /// Whether the UTC+8 day `date` is complete.
    pub fn is_day_complete(&self, date: NaiveDate) -> Result<bool, ApiRouteError> {
        let end = utc8_day_end_ms(date).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(is_bucket_complete(end, self.median_time_ms))
    }

    /// Whether the hour starting at `hour_start_secs` (UTC) is complete.
    pub fn is_hour_complete(&self, hour_start_secs: i64) -> Result<bool, ApiRouteError> {
        let start_ms = hour_start_secs.checked_mul(1000).ok_or_else(|| {
            ApiError::internal(format!("hour start out of range: {hour_start_secs} s"))
        })?;
        let end =
            bucket_end_ms(start_ms, HOUR_MS).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(is_bucket_complete(end, self.median_time_ms))
    }
}

/// Cache key of a response built from complete days only: `base` plus the
/// chain's last complete day. The median time moves back on a reorg (a
/// rollback lowers it to the fork point's) as well as forward at midnight, so
/// a cached value is only valid while the last complete day it was built for
/// still is; keying by it replaces the value the moment that changes.
pub fn complete_days_cache_key(
    base: &str,
    chain: Option<ChainTime>,
) -> Result<String, ApiRouteError> {
    Ok(match chain {
        Some(chain) => format!(
            "{base}:complete-through:{}",
            chain.last_complete_day()?.format("%Y%m%d")
        ),
        None => format!("{base}:complete-through:none"),
    })
}

/// [`ChainTime::load`] off the async runtime.
pub async fn load_chain_time(
    store: &Arc<CkbadgerStore>,
) -> Result<Option<ChainTime>, ApiRouteError> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || ChainTime::load(&store))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
}

/// The UTC+8 date of a block timestamp, through the indexer's `block_date`.
fn utc8_date_of(number: i64, timestamp_ms: i64) -> Result<NaiveDate, ApiRouteError> {
    let at = chrono::DateTime::from_timestamp_millis(timestamp_ms).ok_or_else(|| {
        ApiError::internal(format!(
            "block {number} has an out-of-range timestamp: {timestamp_ms} ms"
        ))
    })?;
    Ok(ckbadger_common::block_date(at))
}

/// Whether the `series` row dated `date` aggregates a complete day.
///
/// `chain` is [`ChainTime::load`]'s answer. A row in a store without any
/// block is an error: daily rows are written together with their blocks.
pub fn is_complete_day(
    chain: Option<ChainTime>,
    series: &str,
    date: NaiveDate,
) -> Result<bool, ApiRouteError> {
    let Some(chain) = chain else {
        return Err(ApiError::internal(format!(
            "{series} has a row for {date}, but the store has no indexed block; \
             daily rows are written together with the blocks they aggregate"
        )));
    };
    chain.is_day_complete(date)
}

/// [`is_complete_day`] for a row keyed by a `YYYYMMDD` stats day key.
pub fn is_complete_day_key(
    chain: Option<ChainTime>,
    series: &str,
    day_key: &str,
) -> Result<bool, ApiRouteError> {
    is_complete_day(chain, series, parse_day_key(series, day_key)?)
}

/// [`is_complete_day`] for a row dated `YYYY-MM-DD` (the DAO daily snapshots).
pub fn is_complete_iso_day(
    chain: Option<ChainTime>,
    series: &str,
    date: &str,
) -> Result<bool, ApiRouteError> {
    is_complete_day(chain, series, parse_iso_day(series, date)?)
}

/// Parse a `YYYYMMDD` stats day key; a malformed key is an error naming it.
pub fn parse_day_key(series: &str, day_key: &str) -> Result<NaiveDate, ApiRouteError> {
    parse_day(series, day_key, "%Y%m%d")
}

/// Parse a `YYYY-MM-DD` date; a malformed date is an error naming it.
pub fn parse_iso_day(series: &str, date: &str) -> Result<NaiveDate, ApiRouteError> {
    parse_day(series, date, "%Y-%m-%d")
}

/// Parse `date` in `format`, accepting only its canonical (zero-padded) form —
/// the only form the writers produce, and the one byte order sorts correctly.
fn parse_day(series: &str, date: &str, format: &str) -> Result<NaiveDate, ApiRouteError> {
    let parsed = NaiveDate::parse_from_str(date, format)
        .ok()
        .filter(|parsed| parsed.format(format).to_string() == date);
    parsed.ok_or_else(|| {
        ApiError::internal(format!(
            "{series} has a malformed date {date:?} (expected {format})"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn utc_ms(rfc3339: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .timestamp_millis()
    }

    fn message(err: ApiRouteError) -> String {
        err.1 .0.message.clone()
    }

    /// The tip (23:59:58 on 10-23, UTC+8) follows a parent dated 00:00:02 on
    /// 10-24 — legal under CKB's median-time rule. Neither day is complete,
    /// the row for 10-24 is in progress rather than corrupt, and 10-22 is the
    /// last complete day.
    #[test]
    fn a_tip_dated_before_its_parent_leaves_both_days_in_progress() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T15:59:58Z")),
            (1, utc_ms("2026-10-23T16:00:02Z")),
            (0, utc_ms("2026-10-23T15:59:50Z")),
        ])
        .unwrap();
        assert_eq!(chain.median_time_ms(), utc_ms("2026-10-23T15:59:58Z"));
        assert_eq!(chain.tip_day().unwrap(), day("2026-10-23"));
        assert!(chain.is_day_complete(day("2026-10-22")).unwrap());
        assert!(!chain.is_day_complete(day("2026-10-23")).unwrap());
        assert!(!is_complete_day_key(Some(chain), "s", "20261024").unwrap());
        assert_eq!(chain.last_complete_day().unwrap(), day("2026-10-22"));
    }

    /// Hours follow the same rule: the 15:00 UTC hour holding the parent and
    /// the 14:00 hour holding the tip are both in progress.
    #[test]
    fn a_tip_dated_before_its_parent_leaves_both_hours_in_progress() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T14:59:58Z")),
            (1, utc_ms("2026-10-23T15:00:02Z")),
            (0, utc_ms("2026-10-23T14:59:50Z")),
        ])
        .unwrap();
        let hour = |s| utc_ms(s) / 1000;
        assert!(chain
            .is_hour_complete(hour("2026-10-23T13:00:00Z"))
            .unwrap());
        assert!(!chain
            .is_hour_complete(hour("2026-10-23T14:00:00Z"))
            .unwrap());
        assert!(!chain
            .is_hour_complete(hour("2026-10-23T15:00:00Z"))
            .unwrap());
    }

    /// A tip already past midnight does not complete the previous day while
    /// the median time is still before midnight.
    #[test]
    fn a_tip_past_midnight_waits_for_the_median_time() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T16:00:30Z")),
            (1, utc_ms("2026-10-23T15:59:55Z")),
            (0, utc_ms("2026-10-23T15:59:50Z")),
        ])
        .unwrap();
        assert_eq!(chain.tip_day().unwrap(), day("2026-10-24"));
        assert!(!chain.is_day_complete(day("2026-10-23")).unwrap());
        assert_eq!(chain.last_complete_day().unwrap(), day("2026-10-22"));
    }

    #[test]
    fn the_window_must_be_the_consecutive_blocks_ending_at_the_tip() {
        // Every block down to genesis on a young chain.
        assert!(ChainTime::from_window(&[(1, 2_000), (0, 1_000)]).is_ok());
        for window in [
            vec![(2, 2_000), (0, 1_000)],              // gap
            vec![(5, 2_000), (4, 1_000)],              // lower blocks missing
            vec![(40, 2_000), (39, 1_000), (38, 900)], // short of 37
        ] {
            let err = message(ChainTime::from_window(&window).unwrap_err());
            assert!(err.contains("consecutive blocks ending at tip"), "{err}");
        }
        let full: Vec<(i64, i64)> = (0..37).map(|i| (100 - i, 10_000 - i)).collect();
        let chain = ChainTime::from_window(&full).unwrap();
        // Upper median of 37 values 9_964..=10_000.
        assert_eq!(chain.median_time_ms(), 9_982);
    }

    #[test]
    fn cache_keys_follow_the_last_complete_day_and_hour() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T16:00:30Z")),
            (1, utc_ms("2026-10-23T15:59:55Z")),
            (0, utc_ms("2026-10-23T15:59:50Z")),
        ])
        .unwrap();
        // Median time 15:59:55Z (23:59:55 UTC+8 on 10-23): 10-22 and the
        // 14:00 UTC hour are the newest complete buckets.
        assert_eq!(
            complete_days_cache_key("chart:x", Some(chain)).unwrap(),
            "chart:x:complete-through:20261022"
        );
        assert_eq!(
            complete_days_cache_key("chart:x", None).unwrap(),
            "chart:x:complete-through:none"
        );
        assert_eq!(
            chain.last_complete_hour_start_secs(),
            utc_ms("2026-10-23T14:00:00Z") / 1000
        );
    }

    #[test]
    fn a_row_in_a_store_without_blocks_is_an_error() {
        let err = message(is_complete_day_key(None, "daily stats", "20261023").unwrap_err());
        assert!(
            err.contains("daily stats") && err.contains("no indexed block"),
            "{err}"
        );
    }

    #[test]
    fn malformed_dates_are_errors_naming_the_value() {
        let chain = Some(ChainTime::from_window(&[(0, utc_ms("2026-10-24T04:00:00Z"))]).unwrap());
        for key in ["2026-10-23", "2026102", "20261332"] {
            let err = message(is_complete_day_key(chain, "s", key).unwrap_err());
            assert!(err.contains(key), "{err}");
        }
        let err = message(is_complete_iso_day(chain, "s", "20261023").unwrap_err());
        assert!(err.contains("20261023"), "{err}");
        assert!(is_complete_iso_day(chain, "s", "2026-10-23").unwrap());
    }

    #[test]
    fn an_out_of_range_tip_timestamp_is_an_error() {
        let chain = ChainTime::from_window(&[(0, i64::MAX)]).unwrap();
        let err = message(chain.tip_day().unwrap_err());
        assert!(err.contains("block 0"), "{err}");
    }

    #[test]
    fn load_reads_the_newest_headers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert_eq!(ChainTime::load(&store).unwrap(), None);

        let header = |ts: i64| ckbadger_store::types::CachedBlockHeader {
            hash: vec![0; 32],
            parent_hash: vec![0; 32],
            timestamp: ts,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: ts,
        };
        let mut batch = ckbadger_store::batch::StoreBatch::new(&store);
        for number in 0..40 {
            batch.put_block_header(number, &header(1_000 + number * 10));
        }
        batch.commit().unwrap();
        let chain = ChainTime::load(&store).unwrap().unwrap();
        assert_eq!(chain.tip_number(), 39);
        // Window 3..=39 → timestamps 1_030..=1_390, upper median 1_210.
        assert_eq!(chain.median_time_ms(), 1_210);
    }
}
