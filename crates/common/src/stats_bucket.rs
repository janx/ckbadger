//! The one definition of a complete stats time bucket (an hour or a UTC+8 day).
//!
//! CKB consensus only requires a block's timestamp to exceed the median time
//! of the 37 blocks before it ([`median_time_ms`]). A block may therefore be
//! dated earlier than its parent, so "the tip block's day" is not the newest
//! day the chain can still add a block to: a tip at 23:59:58 may follow a
//! parent at 00:00:02 the next day.
//!
//! What the rule does guarantee is a floor. Every future block is dated after
//! the median time of the newest 37 indexed blocks (the window ending at the
//! tip), and that median never decreases. So a bucket `[start, end)` whose end
//! is at or below it can never receive another block: it is complete. Every
//! later bucket — including any dated after the tip's own — is in progress.
//!
//! The bulk indexer seals activity buckets with this rule and the API judges
//! every daily chart and the transaction stats with it; both call
//! [`is_bucket_complete`].

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;

use crate::CKB_UTC8_OFFSET;

/// Blocks in CKB's median-time window (consensus `median_time_block_count`).
pub const MEDIAN_TIME_BLOCK_COUNT: usize = 37;
pub const HOUR_MS: i64 = 60 * 60 * 1_000;
pub const DAY_MS: i64 = 24 * HOUR_MS;
const UTC8_OFFSET_MS: i64 = CKB_UTC8_OFFSET as i64 * 1_000;

/// CKB's median time of a window of consecutive block timestamps: the upper
/// median (index `len / 2` of the sorted window), at most
/// [`MEDIAN_TIME_BLOCK_COUNT`] blocks. Near genesis the window is every block
/// there is.
pub fn median_time_ms(timestamps: &[i64]) -> Result<i64> {
    if timestamps.is_empty() {
        bail!("cannot calculate CKB median time from an empty timestamp window");
    }
    if timestamps.len() > MEDIAN_TIME_BLOCK_COUNT {
        bail!(
            "CKB median-time window exceeds consensus limit: len={} max={}",
            timestamps.len(),
            MEDIAN_TIME_BLOCK_COUNT
        );
    }
    // Consensus fixes the window at at most 37 headers, so keep the working
    // set on the stack. This preserves the exact upper-median rule without a
    // heap allocation for every block in a full-chain replay.
    let mut ordered = [0i64; MEDIAN_TIME_BLOCK_COUNT];
    ordered[..timestamps.len()].copy_from_slice(timestamps);
    let middle = timestamps.len() / 2;
    let (_, median, _) = ordered[..timestamps.len()].select_nth_unstable(middle);
    Ok(*median)
}

/// Whether the bucket ending (exclusively) at `bucket_end_ms` is complete
/// under `median_time_ms`, the median time of the newest indexed blocks.
pub fn is_bucket_complete(bucket_end_ms: i64, median_time_ms: i64) -> bool {
    bucket_end_ms <= median_time_ms
}

/// Exclusive end of the UTC+8-aligned bucket of `bucket_ms` holding
/// `timestamp_ms`. Hour buckets align identically on the UTC clock (the
/// offset is whole hours), so this also ends a UTC-keyed hour.
pub fn bucket_end_ms(timestamp_ms: i64, bucket_ms: i64) -> Result<i64> {
    let shifted = timestamp_ms.checked_add(UTC8_OFFSET_MS).ok_or_else(|| {
        anyhow!(
            "bucket timestamp overflow while applying UTC+8 offset: timestamp_ms={}",
            timestamp_ms
        )
    })?;
    let next_bucket = shifted
        .div_euclid(bucket_ms)
        .checked_add(1)
        .ok_or_else(|| anyhow!("bucket index overflow: timestamp_ms={}", timestamp_ms))?;
    next_bucket
        .checked_mul(bucket_ms)
        .and_then(|end| end.checked_sub(UTC8_OFFSET_MS))
        .ok_or_else(|| {
            anyhow!(
                "bucket end overflow: timestamp_ms={} bucket_ms={}",
                timestamp_ms,
                bucket_ms
            )
        })
}

/// Exclusive end of a UTC+8 calendar day.
pub fn utc8_day_end_ms(date: NaiveDate) -> Result<i64> {
    date.and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow!("invalid midnight for day {}", date))?
        .and_utc()
        .timestamp_millis()
        .checked_add(DAY_MS)
        .and_then(|end| end.checked_sub(UTC8_OFFSET_MS))
        .ok_or_else(|| anyhow!("day end overflow for {}", date))
}

/// Exclusive end of the bucket a UTC+8 stats key names: `YYYYMMDDHH` for an
/// hour (`hourly`), `YYYYMMDD` for a day.
pub fn bucket_end_from_utc8_key(bucket: &str, hourly: bool) -> Result<i64> {
    if hourly {
        if bucket.len() != 10 {
            bail!(
                "invalid hourly bucket key length: bucket={} len={} expected=10",
                bucket,
                bucket.len()
            );
        }
        let date = NaiveDate::parse_from_str(&bucket[..8], "%Y%m%d")
            .map_err(|err| anyhow!("invalid hourly bucket date {}: {}", bucket, err))?;
        let hour = bucket[8..]
            .parse::<u32>()
            .map_err(|err| anyhow!("invalid hourly bucket hour {}: {}", bucket, err))?;
        date.and_hms_opt(hour, 0, 0)
            .ok_or_else(|| anyhow!("invalid hour in hourly bucket {}", bucket))?
            .and_utc()
            .timestamp_millis()
            .checked_add(HOUR_MS)
            .and_then(|end| end.checked_sub(UTC8_OFFSET_MS))
            .ok_or_else(|| anyhow!("bucket end overflow for key={}", bucket))
    } else {
        utc8_day_end_ms(
            NaiveDate::parse_from_str(bucket, "%Y%m%d")
                .map_err(|err| anyhow!("invalid daily bucket {}: {}", bucket, err))?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc_ms(rfc3339: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn median_time_uses_the_upper_value_for_an_even_window() {
        assert_eq!(median_time_ms(&[10, 20]).unwrap(), 20);
        assert_eq!(median_time_ms(&[30, 10, 20]).unwrap(), 20);
        assert!(median_time_ms(&[]).is_err());
        assert!(median_time_ms(&[0; MEDIAN_TIME_BLOCK_COUNT + 1]).is_err());
    }

    /// A tip dated before its parent across midnight: the median time is the
    /// tip's 23:59:58, so neither that day nor the parent's next day is
    /// complete, and the day before is.
    #[test]
    fn a_backward_tip_across_midnight_leaves_both_days_in_progress() {
        let window = [
            utc_ms("2026-10-23T15:59:50Z"), // 23:59:50 UTC+8, 10-23
            utc_ms("2026-10-23T16:00:02Z"), // 00:00:02 UTC+8, 10-24
            utc_ms("2026-10-23T15:59:58Z"), // 23:59:58 UTC+8, 10-23 (tip)
        ];
        let mtp = median_time_ms(&window).unwrap();
        assert_eq!(mtp, utc_ms("2026-10-23T15:59:58Z"));
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert!(is_bucket_complete(
            utc8_day_end_ms(day("2026-10-22")).unwrap(),
            mtp
        ));
        assert!(!is_bucket_complete(
            utc8_day_end_ms(day("2026-10-23")).unwrap(),
            mtp
        ));
        assert!(!is_bucket_complete(
            utc8_day_end_ms(day("2026-10-24")).unwrap(),
            mtp
        ));
    }

    #[test]
    fn a_bucket_ending_exactly_at_the_median_time_is_complete() {
        // A future block must be dated strictly after the median time, so it
        // cannot land in a bucket that ends at it.
        assert!(is_bucket_complete(1_000, 1_000));
        assert!(!is_bucket_complete(1_001, 1_000));
    }

    #[test]
    fn bucket_ends_follow_the_utc8_calendar() {
        // 2026-10-23T15:30Z is 23:30 UTC+8: its day ends at 16:00Z, its hour too.
        let ts = utc_ms("2026-10-23T15:30:00Z");
        assert_eq!(
            bucket_end_ms(ts, DAY_MS).unwrap(),
            utc_ms("2026-10-23T16:00:00Z")
        );
        assert_eq!(
            bucket_end_ms(ts, HOUR_MS).unwrap(),
            utc_ms("2026-10-23T16:00:00Z")
        );
        assert_eq!(
            utc8_day_end_ms(NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()).unwrap(),
            utc_ms("2026-10-23T16:00:00Z")
        );
        assert_eq!(
            bucket_end_from_utc8_key("20261023", false).unwrap(),
            utc_ms("2026-10-23T16:00:00Z")
        );
        assert_eq!(
            bucket_end_from_utc8_key("2026102323", true).unwrap(),
            utc_ms("2026-10-23T16:00:00Z")
        );
        assert!(bucket_end_from_utc8_key("202610232", true).is_err());
        assert!(bucket_end_from_utc8_key("2026-10-23", false).is_err());
    }
}
