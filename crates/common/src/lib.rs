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
pub mod types;

pub use error::{Error, Result};
pub use hardfork::*;
pub use hex::{parse_capacity, parse_hex_to_bytes, parse_hex_to_hash, parse_hex_u32};
pub use label_import::*;
pub use proposal::*;
pub use sync::*;
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
