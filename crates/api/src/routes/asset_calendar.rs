//! Complete-day bounds for the existing asset capacity chart builders.
use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::{
    response::{ApiError, ApiRouteError},
    utils::stats_bucket::ChainTime,
};
use ckbadger_store::CkbadgerStore;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Anchor {
    block_number: i64,
    block_hash: String,
    chain_time: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletedCapacityChart {
    #[serde(flatten)]
    chart: StackedAreaChartResponse,
    source: &'static str,
    complete_through: String,
    anchor: Anchor,
}

pub(crate) struct CapacityCalendar {
    pub from: Option<u32>,
    pub to: u32,
    complete_through: String,
    anchor: Anchor,
}
impl CapacityCalendar {
    pub fn read(
        store: &CkbadgerStore,
        from: Option<u32>,
        to: Option<u32>,
    ) -> Result<Self, ApiRouteError> {
        let (number, header) = store
            .get_sync_tip_block()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| {
                ApiError::internal("asset capacity chart requires an indexed chain-time anchor")
            })?;
        let hash = header.hash.as_slice().try_into().map_err(|_| {
            ApiError::internal(format!(
                "asset capacity calendar anchor hash width at block {number}"
            ))
        })?;
        let chain = ChainTime::from_header(number, header.timestamp, header.chain_time_ms, hash)?;
        let day = chain.last_complete_day()?;
        let key = day
            .format("%Y%m%d")
            .to_string()
            .parse::<u32>()
            .map_err(|e| {
                ApiError::internal(format!(
                    "asset capacity complete date out of range: {day}: {e}"
                ))
            })?;
        Ok(Self {
            from,
            to: to.map_or(key, |requested| requested.min(key)),
            complete_through: day.to_string(),
            anchor: Anchor {
                block_number: number,
                block_hash: format!("0x{}", hex::encode(header.hash)),
                chain_time: header.chain_time_ms.to_string(),
            },
        })
    }
    pub fn is_empty(&self) -> bool {
        self.from.is_some_and(|from| from > self.to)
    }
    pub fn wrap(self, chart: StackedAreaChartResponse) -> CompletedCapacityChart {
        CompletedCapacityChart {
            chart,
            source: "indexedChainTimeUtc8",
            complete_through: self.complete_through,
            anchor: self.anchor,
        }
    }
}

/// One day of a "CKB held" history (a collection's, a cluster's, an object's, a script's, a
/// token's): its occupied part (`occupiedCapacity`), its free part (`freeCapacity`) and the whole
/// its cells hold (`capacity`), all three stated by the route so a page sums nothing, under the
/// names every "CKB held" figure of the API takes (the token history's days, the cell versions;
/// lane exactness, part B, the third read's S4: these were `used` and `unused`). `capacity` and the occupied are the route's two running
/// sums of the day's deltas (one source each); the free part is their difference, by the one
/// helper (`free_part`), which refuses an occupied part under zero or above the whole, the day
/// failing with its date and its operands (lane exactness, part A2, the test review's SHOULD-1: it
/// also checked that the parts make the whole, true by the definition).
pub(crate) fn held_point(
    date: String,
    capacity: i128,
    used: i128,
    what: &str,
) -> Result<StackedAreaDataPoint, String> {
    // the one free part (`free_part`), with the history and the day it was read for
    let unused = crate::utils::assets::free_part(capacity, used)
        .map_err(|error| format!("{what} at {date}: {error}"))?;
    Ok(StackedAreaDataPoint {
        date,
        values: std::collections::HashMap::from([
            ("occupiedCapacity".to_string(), used.to_string()),
            ("freeCapacity".to_string(), unused.to_string()),
            ("capacity".to_string(), capacity.to_string()),
        ]),
    })
}

/// The two bands of a "CKB held" history, bottom first, in the page's words: the occupied part and
/// the free part (lane exactness, part B, the third read's S4: the one series of the five histories, which named them "Used" and "Unused"
/// in two colours).
pub(crate) fn held_series() -> Vec<StackedAreaSeries> {
    vec![
        StackedAreaSeries {
            id: None,
            key: "occupiedCapacity".to_string(),
            label: "Occupied".to_string(),
            color: "#f59e0b".to_string(),
        },
        StackedAreaSeries {
            id: None,
            key: "freeCapacity".to_string(),
            label: "Free".to_string(),
            color: "#00c389".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::held_point;

    #[test]
    fn a_held_day_states_its_capacity_beside_its_parts() {
        let point = held_point("2024-01-15".into(), 1000, 600, "test history").unwrap();
        assert_eq!(point.date, "2024-01-15");
        assert_eq!(point.values["occupiedCapacity"], "600");
        assert_eq!(point.values["freeCapacity"], "400");
        assert_eq!(point.values["capacity"], "1000");
    }

    // re-pinned for lane exactness, part A2 (the test review's SHOULD-1): named for the branch it reaches, an
    // occupied over its capacity (it was named for parts that do not make the capacity, which
    // cannot happen: unused is their difference)
    #[test]
    fn a_held_day_whose_occupied_is_over_its_capacity_fails_with_its_date_and_operands() {
        let error = held_point("2024-01-16".into(), 100, 150, "test history").unwrap_err();
        assert!(error.contains("2024-01-16"), "{error}");
        assert!(error.contains("150"), "{error}");
        assert!(error.contains("100"), "{error}");
    }

    #[test]
    fn a_held_day_with_a_negative_occupied_fails() {
        let error = held_point("2024-01-17".into(), 100, -1, "test history").unwrap_err();
        assert!(error.contains("-1"), "{error}");
    }
}
