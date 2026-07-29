pub mod address;
pub mod burn_policy;
pub mod cycles;
pub mod cycles_task;
pub mod dao;
pub mod error;
pub mod hardfork;
pub mod hex;
pub mod label_import;
pub mod network;
pub mod proposal;
pub mod sync;
pub mod token;
pub mod types;

pub use address::script_to_address;
pub use error::{Error, Result};
pub use hardfork::*;
pub use hex::{parse_capacity, parse_hex_to_bytes, parse_hex_to_hash, parse_hex_u32};
pub use label_import::*;
pub use proposal::*;
pub use sync::*;
pub use token::*;
pub use types::*;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, Utc};

/// CKB Explorer uses UTC+8 (Beijing time) for daily boundaries.
/// We match this so our daily stats align with the official explorer.
pub const CKB_UTC8_OFFSET: i32 = 8 * 3600;

/// Convert a UTC timestamp to a NaiveDate using CKB's UTC+8 day boundary.
pub fn block_date(ts: DateTime<Utc>) -> NaiveDate {
    let utc8 = FixedOffset::east_opt(CKB_UTC8_OFFSET).unwrap();
    ts.with_timezone(&utc8).date_naive()
}

/// Convert a UTC timestamp in milliseconds to a NaiveDate using UTC+8.
pub fn block_date_from_ms(timestamp_ms: i64) -> NaiveDate {
    let secs = timestamp_ms / 1000;
    let dt = DateTime::from_timestamp(secs, 0).unwrap_or_default();
    block_date(dt)
}

/// Convert a UTC timestamp in milliseconds to a NaiveDateTime using UTC+8.
/// Use this when you need hour-level (or finer) formatting (e.g. `%H`).
pub fn block_datetime_from_ms(timestamp_ms: i64) -> NaiveDateTime {
    let secs = timestamp_ms / 1000;
    let dt = DateTime::from_timestamp(secs, 0).unwrap_or_default();
    let utc8 = FixedOffset::east_opt(CKB_UTC8_OFFSET).unwrap();
    dt.with_timezone(&utc8).naive_local()
}

/// Current wall-clock time on the UTC+8 stats clock.
///
/// Single "now" source for read paths that window over UTC+8-keyed stat
/// buckets (the activity hourly/daily keys produced by
/// [`block_datetime_from_ms`] / [`block_date_from_ms`]). A cutoff computed on
/// the plain UTC clock sits 8 hours too early in `%Y%m%d%H` key space and
/// silently widens a "24h" window to ~33 buckets.
///
/// Chain-level hourly stats are UTC-keyed instead — see
/// [`utc_hour_key_from_ms`].
pub fn now_datetime_utc8() -> NaiveDateTime {
    let utc8 = FixedOffset::east_opt(CKB_UTC8_OFFSET).unwrap();
    Utc::now().with_timezone(&utc8).naive_local()
}

/// UTC hour-bucket key (`%Y%m%d%H`) for a UTC millisecond timestamp.
///
/// This is the key convention of the chain-level hourly stats CF rows
/// (`STATS_PREFIX_HOURLY`), whose live writer formats the UTC-truncated block
/// hour. Activity hourly buckets use UTC+8 keys instead (see
/// [`block_datetime_from_ms`]). Any consumer comparing against stored hour
/// keys — e.g. reorg rollback cutoffs — MUST pick the helper matching the CF
/// it touches.
pub fn utc_hour_key_from_ms(timestamp_ms: i64) -> String {
    let secs = timestamp_ms.div_euclid(1000);
    DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .format("%Y%m%d%H")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utc_hour_key_from_ms_formats_utc_hour() {
        // 2023-11-18T20:01:00Z
        assert_eq!(utc_hour_key_from_ms(1_700_337_660_000), "2023111820");
        // The UTC+8 hour string for the same instant differs by 8 hours —
        // the two bucket families deliberately use different clocks.
        assert_eq!(
            block_datetime_from_ms(1_700_337_660_000)
                .format("%Y%m%d%H")
                .to_string(),
            "2023111904"
        );
    }

    #[test]
    fn test_now_datetime_utc8_is_utc_plus_8() {
        let utc_now = Utc::now().naive_utc();
        let utc8_now = now_datetime_utc8();
        let offset_secs = (utc8_now - utc_now).num_seconds();
        // Exactly +8h, allowing for the sub-second gap between the two reads.
        assert!(
            (offset_secs - 8 * 3600).abs() <= 1,
            "expected +8h offset, got {}s",
            offset_secs
        );
    }
}
