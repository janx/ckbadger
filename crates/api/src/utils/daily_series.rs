//! Exact calendar views of sparse cumulative statistics. A day without a
//! canonical block preserves the previous state; a missing block-bearing row
//! remains an error. This module only reads the domain store.

use chrono::{NaiveDate, TimeZone};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

use crate::response::{ApiError, ApiRouteError, ChartDataPoint};
use crate::utils::stats_bucket::{parse_iso_day, ChainTime};

/// Serve one cumulative point per complete day, from the first observed row
/// through the chain's last complete day. Both value columns are carried
/// unchanged only after canonical headers prove the intervening days blockless.
/// Never apply this to per-day counts or rates: they are not cumulative state.
pub(crate) async fn consecutive_daily_chart(
    store: Arc<CkbadgerStore>,
    chain: Option<ChainTime>,
    series: &'static str,
    points: Vec<ChartDataPoint>,
) -> Result<Vec<ChartDataPoint>, ApiRouteError> {
    consecutive_daily_records(
        store,
        chain,
        series,
        points,
        |point| &point.date,
        |point, date| ChartDataPoint {
            date,
            ..point.clone()
        },
    )
    .await
}

/// The one blockless-day certification and carry path for cumulative daily
/// records, including single-series and stacked/evidence chart DTOs.
pub(crate) async fn consecutive_daily_records<T: Clone + Send + 'static>(
    store: Arc<CkbadgerStore>,
    chain: Option<ChainTime>,
    series: &'static str,
    points: Vec<T>,
    date_of: fn(&T) -> &str,
    at_date: fn(&T, String) -> T,
) -> Result<Vec<T>, ApiRouteError> {
    if points.is_empty() {
        return Ok(points);
    }
    let chain = chain.ok_or_else(|| {
        ApiError::internal(format!("{series} has daily rows without an indexed block"))
    })?;
    tokio::task::spawn_blocking(move || {
        let mut data = Vec::with_capacity(points.len());
        for point in points {
            let date = parse_iso_day(series, date_of(&point))?;
            append_blockless_days(&store, series, &mut data, date, date_of, at_date)?;
            data.push(point);
        }
        // The tip day is the exclusive bound, including when the last
        // observed complete row precedes several blockless days.
        append_blockless_days(
            &store,
            series,
            &mut data,
            chain.tip_day()?,
            date_of,
            at_date,
        )?;
        Ok(data)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
}

fn append_blockless_days<T: Clone>(
    store: &CkbadgerStore,
    series: &str,
    data: &mut Vec<T>,
    next_date: NaiveDate,
    date_of: fn(&T) -> &str,
    at_date: fn(&T, String) -> T,
) -> Result<(), ApiRouteError> {
    let Some(previous) = data.last() else {
        return Ok(());
    };
    let previous_date = parse_iso_day(series, date_of(previous))?;
    if previous_date >= next_date {
        return Err(ApiError::internal(format!(
            "{series} dates are not strictly increasing before {next_date}: previous_date={previous_date}"
        )));
    }
    // next_date exists and is later, so this successor is within the date range.
    let mut date = previous_date + chrono::Duration::days(1);
    if date == next_date {
        return Ok(());
    }
    certify_blockless_daily_gap(store, series, date, next_date)?;
    let previous = previous.clone();
    while date < next_date {
        data.push(at_date(&previous, date.format("%Y-%m-%d").to_string()));
        date += chrono::Duration::days(1);
    }
    Ok(())
}

/// Prove that [first_missing_date, next_snapshot_date) has no canonical block,
/// and that the next date is anchored by a block. The same proof serves the
/// inflation chart and the homepage's cumulative daily series.
pub(crate) fn certify_blockless_daily_gap(
    store: &CkbadgerStore,
    series: &str,
    first_missing_date: chrono::NaiveDate,
    next_snapshot_date: chrono::NaiveDate,
) -> Result<(), ApiRouteError> {
    if first_missing_date >= next_snapshot_date {
        return Err(ApiError::internal(format!(
            "invalid {series} gap bounds: first_missing_date={}, next_snapshot_date={}",
            first_missing_date, next_snapshot_date
        )));
    }

    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)
        .ok_or_else(|| ApiError::internal("invalid CKB UTC+8 offset"))?;
    let day_start = first_missing_date.and_hms_opt(0, 0, 0).ok_or_else(|| {
        ApiError::internal(format!(
            "invalid start of missing {series} date {}",
            first_missing_date
        ))
    })?;
    let day_start_ms = utc8
        .from_local_datetime(&day_start)
        .single()
        .ok_or_else(|| {
            ApiError::internal(format!(
                "ambiguous start of missing {series} date {}",
                first_missing_date
            ))
        })?
        .timestamp_millis();

    let first_block = store
        .find_first_block_at_or_after_chain_time_ms(day_start_ms)
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to locate canonical block coverage for {series} gap starting {}: {}",
                first_missing_date, error
            ))
        })?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "{series} exists for {} but no canonical block exists at or after preceding gap start {}",
                next_snapshot_date, first_missing_date
            ))
        })?;
    let first_header = store
        .get_block_header(first_block)
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to read canonical block {} while validating {series} gap starting {}: {}",
                first_block, first_missing_date, error
            ))
        })?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "canonical block header disappeared while validating {series} gap: block={}, first_missing_date={}",
                first_block, first_missing_date
            ))
        })?;
    let first_block_date = ckbadger_common::stats_bucket::stats_day(first_header.chain_time_ms)
        .map_err(|error| ApiError::internal(format!(
            "invalid chain time while validating {series} gap: block={first_block} chain_time_ms={}: {error}",
            first_header.chain_time_ms
        )))?;

    if first_block_date < next_snapshot_date {
        return Err(ApiError::internal(format!(
            "missing complete-day {series} for block-bearing date: missing_date={}, first_block={}, next_snapshot_date={}",
            first_block_date, first_block, next_snapshot_date
        )));
    }
    if first_block_date > next_snapshot_date {
        return Err(ApiError::internal(format!(
            "{series} has no canonical block-date coverage: snapshot_date={}, first_block_at_or_after_gap={}, first_block_date={}",
            next_snapshot_date, first_block, first_block_date
        )));
    }

    Ok(())
}
