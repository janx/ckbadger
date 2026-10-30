//! Which stats buckets the API may serve as complete.
//!
//! A bucket is complete when its exclusive end is at or below the canonical
//! tip's running-maximum chain time. The indexer assigns every stats row by
//! this same monotonic time, so no later block can reopen an earlier bucket.

use chrono::NaiveDate;
use ckbadger_common::stats_bucket::{
    bucket_end_ms, is_bucket_complete, stats_day, stats_hour, utc8_day_end_ms, HOUR_MS,
};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

use crate::response::{ApiError, ApiRouteError};

/// The indexed chain's clock and the first block of its incomplete day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainTime {
    tip_number: i64,
    tip_timestamp_ms: i64,
    chain_time_ms: i64,
    complete_day_boundary_hash: [u8; 32],
    /// The first block of the incomplete day; known once [`Self::load`]
    /// located it, `None` for a clock built from a header alone.
    complete_day_boundary_block: Option<i64>,
}

impl ChainTime {
    /// The store's chain time, `None` while it holds no block.
    pub fn load(store: &CkbadgerStore) -> Result<Option<Self>, ApiRouteError> {
        let headers = store.list_blocks_desc(None, 1).map_err(|e| {
            ApiError::internal(format!("failed to read the newest block headers: {e}"))
        })?;
        if headers.is_empty() {
            return Ok(None);
        }
        let (number, header) = &headers[0];
        let tip_hash: [u8; 32] = header.hash.as_slice().try_into().map_err(|_| {
            ApiError::internal(format!(
                "block {number} has invalid hash length {}",
                header.hash.len()
            ))
        })?;
        let mut chain =
            Self::from_header(*number, header.timestamp, header.chain_time_ms, tip_hash)?;
        let current_day_start_ms = utc8_day_end_ms(chain.last_complete_day()?)
            .map_err(|e| ApiError::internal(format!("invalid current-day boundary: {e}")))?;
        let boundary_block = store
            .find_first_block_at_or_after_chain_time_ms(current_day_start_ms)
            .map_err(|e| ApiError::internal(format!("failed to locate current-day boundary: {e}")))?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "missing current-day boundary at {current_day_start_ms}"
                ))
            })?;
        let boundary_header = store
            .get_block_header(boundary_block)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to read boundary block {boundary_block}: {e}"
                ))
            })?
            .ok_or_else(|| {
                ApiError::internal(format!("missing boundary block {boundary_block}"))
            })?;
        chain.complete_day_boundary_hash =
            boundary_header.hash.as_slice().try_into().map_err(|_| {
                ApiError::internal(format!(
                    "boundary block {boundary_block} has invalid hash length {}",
                    boundary_header.hash.len()
                ))
            })?;
        chain.complete_day_boundary_block = Some(boundary_block);
        Ok(Some(chain))
    }

    pub fn from_header(
        tip_number: i64,
        tip_timestamp_ms: i64,
        chain_time_ms: i64,
        tip_hash: [u8; 32],
    ) -> Result<Self, ApiRouteError> {
        if tip_number < 0 || chain_time_ms < tip_timestamp_ms {
            return Err(ApiError::internal(format!("invalid canonical chain time at block {tip_number}: raw_ms={tip_timestamp_ms} chain_time_ms={chain_time_ms}")));
        }
        Ok(Self {
            tip_number,
            tip_timestamp_ms,
            chain_time_ms,
            complete_day_boundary_hash: tip_hash,
            complete_day_boundary_block: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_window(newest_first: &[(i64, i64)]) -> Result<Self, ApiRouteError> {
        let Some(&(tip_number, tip_timestamp_ms)) = newest_first.first() else {
            return Err(ApiError::internal("empty chain-time fixture"));
        };
        let consecutive = newest_first
            .iter()
            .zip(0i64..)
            .all(|(&(number, _), offset)| Some(number) == tip_number.checked_sub(offset));
        if !consecutive || newest_first.last().map(|(number, _)| *number) != Some(0) {
            return Err(ApiError::internal(
                "fixture is not consecutive blocks ending at tip",
            ));
        }
        let chain_time_ms = newest_first
            .iter()
            .map(|(_, ms)| *ms)
            .max()
            .expect("non-empty fixture");
        Self::from_header(tip_number, tip_timestamp_ms, chain_time_ms, [0; 32])
    }

    pub fn tip_number(&self) -> i64 {
        self.tip_number
    }

    pub fn tip_timestamp_ms(&self) -> i64 {
        self.tip_timestamp_ms
    }

    pub fn chain_time_ms(&self) -> i64 {
        self.chain_time_ms
    }

    /// The first block of the incomplete (tip) day: every block before it
    /// belongs to a complete day.
    pub fn complete_day_boundary_block(&self) -> Result<i64, ApiRouteError> {
        self.complete_day_boundary_block.ok_or_else(|| {
            ApiError::internal("chain time was built without locating its complete-day boundary")
        })
    }

    /// The tip block's UTC+8 stats day.
    pub fn tip_day(&self) -> Result<NaiveDate, ApiRouteError> {
        utc8_date_of(self.tip_number, self.chain_time_ms)
    }

    /// The newest complete UTC+8 day precedes the tip's chain-time day.
    pub fn last_complete_day(&self) -> Result<NaiveDate, ApiRouteError> {
        let chain_day = utc8_date_of(self.tip_number, self.chain_time_ms)?;
        chain_day.pred_opt().ok_or_else(|| {
            ApiError::internal(format!("no day precedes the chain-time day {chain_day}"))
        })
    }

    /// Start (UTC seconds) of the newest complete hour: the hour before the
    /// one the chain time falls in.
    pub fn last_complete_hour_start_secs(&self) -> Result<i64, ApiRouteError> {
        stats_hour(self.chain_time_ms)
            .map_err(|e| ApiError::internal(format!("invalid tip chain hour: {e}")))?
            .timestamp()
            .checked_sub(3600)
            .ok_or_else(|| ApiError::internal("last complete hour timestamp underflow"))
    }

    /// Whether the UTC+8 day `date` is complete.
    pub fn is_day_complete(&self, date: NaiveDate) -> Result<bool, ApiRouteError> {
        let end = utc8_day_end_ms(date).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(is_bucket_complete(end, self.chain_time_ms))
    }

    /// Whether the hour starting at `hour_start_secs` (UTC) is complete.
    pub fn is_hour_complete(&self, hour_start_secs: i64) -> Result<bool, ApiRouteError> {
        let start_ms = hour_start_secs.checked_mul(1000).ok_or_else(|| {
            ApiError::internal(format!("hour start out of range: {hour_start_secs} s"))
        })?;
        let end =
            bucket_end_ms(start_ms, HOUR_MS).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(is_bucket_complete(end, self.chain_time_ms))
    }
}

/// Cache key of a response built from complete days only: `base` plus the
/// chain's last complete day and the first block of its incomplete day. That
/// block stays fixed during normal tip growth, and changes if a fork modifies
/// a completed day.
pub fn complete_days_cache_key(
    base: &str,
    chain: Option<ChainTime>,
) -> Result<String, ApiRouteError> {
    Ok(match chain {
        Some(chain) => format!(
            "{base}:complete-through:{}:boundary:{}",
            chain.last_complete_day()?.format("%Y%m%d"),
            hex::encode(chain.complete_day_boundary_hash),
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

/// The UTC+8 statistics day of a canonical block chain time.
fn utc8_date_of(number: i64, timestamp_ms: i64) -> Result<NaiveDate, ApiRouteError> {
    stats_day(timestamp_ms).map_err(|e| {
        ApiError::internal(format!(
            "block {number} has invalid chain time {timestamp_ms} ms: {e}"
        ))
    })
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

    /// A backward raw timestamp retains the parent's next-day chain time.
    #[test]
    fn a_tip_dated_before_its_parent_stays_in_the_parent_day() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T15:59:58Z")),
            (1, utc_ms("2026-10-23T16:00:02Z")),
            (0, utc_ms("2026-10-23T15:59:50Z")),
        ])
        .unwrap();
        assert_eq!(chain.chain_time_ms(), utc_ms("2026-10-23T16:00:02Z"));
        assert_eq!(chain.tip_day().unwrap(), day("2026-10-24"));
        assert!(chain.is_day_complete(day("2026-10-22")).unwrap());
        assert!(chain.is_day_complete(day("2026-10-23")).unwrap());
        assert!(!is_complete_day_key(Some(chain), "s", "20261024").unwrap());
        assert_eq!(chain.last_complete_day().unwrap(), day("2026-10-23"));
    }

    /// The backward block remains in the parent's 15:00 UTC hour.
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
        assert!(chain
            .is_hour_complete(hour("2026-10-23T14:00:00Z"))
            .unwrap());
        assert!(!chain
            .is_hour_complete(hour("2026-10-23T15:00:00Z"))
            .unwrap());
    }

    /// Crossing midnight completes the previous day immediately.
    #[test]
    fn a_tip_past_midnight_completes_the_previous_day() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T16:00:30Z")),
            (1, utc_ms("2026-10-23T15:59:55Z")),
            (0, utc_ms("2026-10-23T15:59:50Z")),
        ])
        .unwrap();
        assert_eq!(chain.tip_day().unwrap(), day("2026-10-24"));
        assert!(chain.is_day_complete(day("2026-10-23")).unwrap());
        assert_eq!(chain.last_complete_day().unwrap(), day("2026-10-23"));
    }

    #[test]
    fn fixtures_must_be_consecutive_and_persisted_chain_time_cannot_regress() {
        // Every block down to genesis on a young chain.
        assert!(ChainTime::from_window(&[(1, 2_000), (0, 1_000)]).is_ok());
        for window in [
            vec![(2, 2_000), (0, 1_000)], // gap
            vec![(5, 2_000), (4, 1_000)], // lower blocks missing
        ] {
            let err = message(ChainTime::from_window(&window).unwrap_err());
            assert!(err.contains("consecutive blocks ending at tip"), "{err}");
        }
        let err = message(ChainTime::from_header(5, 2_000, 1_999, [0; 32]).unwrap_err());
        assert!(err.contains("invalid canonical chain time"), "{err}");
    }

    #[test]
    fn cache_keys_follow_the_last_complete_day_and_hour() {
        let chain = ChainTime::from_window(&[
            (2, utc_ms("2026-10-23T16:00:30Z")),
            (1, utc_ms("2026-10-23T15:59:55Z")),
            (0, utc_ms("2026-10-23T15:59:50Z")),
        ])
        .unwrap();
        // Chain time 16:00:30Z completes 10-23 and the 15:00 UTC hour.
        assert_eq!(
            complete_days_cache_key("chart:x", Some(chain)).unwrap(),
            format!(
                "chart:x:complete-through:20261023:boundary:{}",
                "00".repeat(32)
            )
        );
        assert_eq!(
            complete_days_cache_key("chart:x", None).unwrap(),
            "chart:x:complete-through:none"
        );
        assert_eq!(
            chain.last_complete_hour_start_secs().unwrap(),
            utc_ms("2026-10-23T15:00:00Z") / 1000
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
    fn invalid_tip_chain_hour_is_reported_with_context() {
        let chain = ChainTime::from_header(1, 0, i64::MAX, [0; 32]).unwrap();
        let error = message(chain.last_complete_hour_start_secs().unwrap_err());
        assert!(error.contains("invalid tip chain hour"), "{error}");
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
        assert_eq!(chain.chain_time_ms(), 1_390);
    }

    #[test]
    fn complete_day_cache_survives_tip_growth_but_changes_on_boundary_fork() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let before = utc_ms("2026-10-23T15:59:59Z");
        let after = utc_ms("2026-10-23T16:00:01Z");
        let header = |hash: u8, timestamp: i64| ckbadger_store::types::CachedBlockHeader {
            hash: vec![hash; 32],
            parent_hash: vec![hash.wrapping_sub(1); 32],
            timestamp,
            chain_time_ms: timestamp,
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
        let mut batch = ckbadger_store::batch::StoreBatch::new(&store);
        batch.put_block_header(0, &header(1, before));
        batch.put_block_header(1, &header(2, after));
        batch.commit().unwrap();
        let first = complete_days_cache_key("chart:x", ChainTime::load(&store).unwrap()).unwrap();

        let mut batch = ckbadger_store::batch::StoreBatch::new(&store);
        batch.put_block_header(2, &header(3, after + 1_000));
        batch.commit().unwrap();
        let grown = complete_days_cache_key("chart:x", ChainTime::load(&store).unwrap()).unwrap();
        assert_eq!(
            first, grown,
            "tip growth within the incomplete day should reuse cache"
        );

        let mut batch = ckbadger_store::batch::StoreBatch::new(&store);
        batch.put_block_header(1, &header(4, after));
        batch.put_block_header(2, &header(5, after + 1_000));
        batch.commit().unwrap();
        let forked = complete_days_cache_key("chart:x", ChainTime::load(&store).unwrap()).unwrap();
        assert_ne!(
            first, forked,
            "a fork across the day boundary must invalidate cache"
        );
    }
}
