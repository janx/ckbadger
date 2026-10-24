//! The one definition of a complete stats day.
//!
//! The indexer keys every daily stats row by the UTC+8 calendar date of the
//! blocks it aggregates ([`ckbadger_common::block_date`]). The day that holds
//! the indexed tip block is still in progress: the next block may land in it.
//! Every earlier day is complete, because a block dated after it is already
//! indexed. No row can be dated after the tip day — the tip is the newest block
//! the store has — so meeting one is an invariant violation, not a day to skip.
//!
//! Daily charts show complete days only (POSTMORTEM STATS-003, STATS-013). The
//! wall clock is not the reference: an indexer that lags the chain still has an
//! in-progress tip day after midnight has passed, and a day the store has only
//! partly indexed must not be charted as complete.

use chrono::{DateTime, NaiveDate};
use ckbadger_store::CkbadgerStore;

use crate::response::{ApiError, ApiRouteError};

/// Where a daily row's day stands relative to the indexed tip block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsDay {
    /// A block dated after this day is indexed: the day can gain no block.
    Complete,
    /// The indexed tip block's own day: later blocks may still land in it.
    InProgress,
}

/// The stats day of the indexed tip block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InProgressDay {
    date: NaiveDate,
}

impl InProgressDay {
    /// The tip block's stats day, `None` while the store has no block. Read it
    /// in the same request (the same pinned read view) as the rows it judges.
    pub fn load(store: &CkbadgerStore) -> Result<Option<Self>, ApiRouteError> {
        store
            .get_sync_tip_block()
            .map_err(|e| ApiError::internal(format!("failed to read the sync tip block: {e}")))?
            .map(|(number, header)| Self::of_block(number, header.timestamp))
            .transpose()
    }

    /// The stats day of block `number`, through the same `block_date` the
    /// indexer writes daily rows with.
    pub fn of_block(number: i64, timestamp_ms: i64) -> Result<Self, ApiRouteError> {
        let at = DateTime::from_timestamp_millis(timestamp_ms).ok_or_else(|| {
            ApiError::internal(format!(
                "block {number} has an out-of-range timestamp: {timestamp_ms} ms"
            ))
        })?;
        Ok(Self {
            date: ckbadger_common::block_date(at),
        })
    }

    pub fn date(&self) -> NaiveDate {
        self.date
    }

    /// Classify the `series` row dated `date`; a row after the tip day is an
    /// error naming the series, the row and the tip day.
    pub fn classify(&self, series: &str, date: NaiveDate) -> Result<StatsDay, ApiRouteError> {
        if date > self.date {
            return Err(ApiError::internal(format!(
                "{series} has a row for {date}, after the indexed tip block's day {}; \
                 no block can be dated after the tip",
                self.date
            )));
        }
        Ok(if date == self.date {
            StatsDay::InProgress
        } else {
            StatsDay::Complete
        })
    }
}

/// Whether the `series` row dated `date` aggregates a complete day.
///
/// `in_progress` is [`InProgressDay::load`]'s answer. A row in a store without
/// any block is an error: daily rows are written together with their blocks.
pub fn is_complete_day(
    in_progress: Option<InProgressDay>,
    series: &str,
    date: NaiveDate,
) -> Result<bool, ApiRouteError> {
    let Some(in_progress) = in_progress else {
        return Err(ApiError::internal(format!(
            "{series} has a row for {date}, but the store has no indexed block; \
             daily rows are written together with the blocks they aggregate"
        )));
    };
    Ok(in_progress.classify(series, date)? == StatsDay::Complete)
}

/// [`is_complete_day`] for a row keyed by a `YYYYMMDD` stats day key.
pub fn is_complete_day_key(
    in_progress: Option<InProgressDay>,
    series: &str,
    day_key: &str,
) -> Result<bool, ApiRouteError> {
    is_complete_day(in_progress, series, parse_day_key(series, day_key)?)
}

/// [`is_complete_day`] for a row dated `YYYY-MM-DD` (the DAO daily snapshots).
pub fn is_complete_iso_day(
    in_progress: Option<InProgressDay>,
    series: &str,
    date: &str,
) -> Result<bool, ApiRouteError> {
    is_complete_day(in_progress, series, parse_day(series, date, "%Y-%m-%d")?)
}

/// Parse a `YYYYMMDD` stats day key; a malformed key is an error naming it.
pub fn parse_day_key(series: &str, day_key: &str) -> Result<NaiveDate, ApiRouteError> {
    parse_day(series, day_key, "%Y%m%d")
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

    fn message(err: ApiRouteError) -> String {
        err.1 .0.message.clone()
    }

    /// The tip block's UTC+8 day — the day key the indexer writes its rows
    /// under — not its UTC date: 2026-10-23T16:30Z is 2026-10-24 00:30 UTC+8.
    #[test]
    fn in_progress_day_is_the_tip_block_s_utc8_day() {
        let tip = chrono::DateTime::parse_from_rfc3339("2026-10-23T16:30:00Z")
            .unwrap()
            .timestamp_millis();
        let in_progress = InProgressDay::of_block(20_819_353, tip).unwrap();
        assert_eq!(in_progress.date(), day("2026-10-24"));
        assert_eq!(
            in_progress.date(),
            ckbadger_common::block_date_from_ms(tip),
            "must agree with the writers' day key"
        );
    }

    #[test]
    fn rows_before_the_tip_day_are_complete_and_the_tip_day_is_not() {
        let tip_day = InProgressDay {
            date: day("2026-10-24"),
        };
        let in_progress = Some(tip_day);
        assert!(is_complete_day(in_progress, "s", day("2026-10-23")).unwrap());
        assert!(!is_complete_day(in_progress, "s", day("2026-10-24")).unwrap());
        assert!(is_complete_day_key(in_progress, "s", "20261023").unwrap());
        assert!(!is_complete_day_key(in_progress, "s", "20261024").unwrap());
        assert!(is_complete_iso_day(in_progress, "s", "2026-10-23").unwrap());
        assert!(!is_complete_iso_day(in_progress, "s", "2026-10-24").unwrap());
        assert_eq!(
            tip_day.classify("s", day("2026-10-24")).unwrap(),
            StatsDay::InProgress
        );
        assert_eq!(
            tip_day.classify("s", day("2026-10-23")).unwrap(),
            StatsDay::Complete
        );
    }

    #[test]
    fn a_row_after_the_tip_day_is_an_error_naming_series_row_and_tip() {
        let in_progress = Some(InProgressDay {
            date: day("2026-10-24"),
        });
        let err = message(is_complete_day_key(in_progress, "daily stats", "20261025").unwrap_err());
        assert!(
            err.contains("daily stats") && err.contains("2026-10-25") && err.contains("2026-10-24"),
            "{err}"
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
        let in_progress = Some(InProgressDay {
            date: day("2026-10-24"),
        });
        for key in ["2026-10-23", "2026102", "20261332"] {
            let err = message(is_complete_day_key(in_progress, "s", key).unwrap_err());
            assert!(err.contains(key), "{err}");
        }
        let err = message(is_complete_iso_day(in_progress, "s", "20261023").unwrap_err());
        assert!(err.contains("20261023"), "{err}");
    }

    #[test]
    fn an_out_of_range_tip_timestamp_is_an_error() {
        let err = message(InProgressDay::of_block(7, i64::MAX).unwrap_err());
        assert!(err.contains("block 7"), "{err}");
    }

    #[test]
    fn load_reads_the_sync_tip_block() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert_eq!(InProgressDay::load(&store).unwrap(), None);

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
        };
        let day_ms = 86_400_000;
        let mut batch = ckbadger_store::batch::StoreBatch::new(&store);
        batch.put_block_header(1, &header(1_700_000_000_000));
        batch.put_block_header(2, &header(1_700_000_000_000 + 3 * day_ms));
        batch.commit().unwrap();
        assert_eq!(
            InProgressDay::load(&store).unwrap().map(|d| d.date()),
            Some(ckbadger_common::block_date_from_ms(
                1_700_000_000_000 + 3 * day_ms
            ))
        );
    }
}
