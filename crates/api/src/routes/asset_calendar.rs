//! Complete-day bounds for the existing asset capacity chart builders.
use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint};
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
/// token's): its occupied part (`used`), its free part (`unused`) and the whole its cells hold
/// (`capacity`), all three stated by the route so a page sums nothing. `capacity` and `used` are
/// the route's two running sums of the day's deltas (one source each); `unused` is defined as their
/// difference, so the only thing to check is that the occupied lies within the held:
/// `0 ≤ used ≤ capacity`, else the day fails with its date and its operands (lane exactness, part A2, the test
/// review's SHOULD-1: it also checked `used + unused == capacity`, true by the definition).
pub(crate) fn held_point(
    date: String,
    capacity: i128,
    used: i128,
    what: &str,
) -> Result<StackedAreaDataPoint, String> {
    if used < 0 || used > capacity {
        return Err(format!(
            "{what} at {date}: occupied {used} is not within the capacity {capacity}"
        ));
    }
    let unused = capacity - used;
    Ok(StackedAreaDataPoint {
        date,
        values: std::collections::HashMap::from([
            ("used".to_string(), used.to_string()),
            ("unused".to_string(), unused.to_string()),
            ("capacity".to_string(), capacity.to_string()),
        ]),
    })
}

#[cfg(test)]
mod tests {
    use super::held_point;

    #[test]
    fn a_held_day_states_its_capacity_beside_its_parts() {
        let point = held_point("2024-01-15".into(), 1000, 600, "test history").unwrap();
        assert_eq!(point.date, "2024-01-15");
        assert_eq!(point.values["used"], "600");
        assert_eq!(point.values["unused"], "400");
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
