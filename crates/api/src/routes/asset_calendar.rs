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

/// One day of a "CKB held" history (a collection's, an object's, a script's): its occupied part
/// (`used`), its free part (`unused`) and the whole its cells hold (`capacity`), all three stated
/// by the route so a page sums nothing. `unused` is the capacity less the occupied; the three
/// agree or the day fails, named with its date and its operands.
pub(crate) fn held_point(
    date: String,
    capacity: i128,
    used: i128,
    what: &str,
) -> Result<StackedAreaDataPoint, String> {
    let unused = capacity.checked_sub(used).ok_or_else(|| {
        format!("{what} at {date}: capacity {capacity} less occupied {used} overflows")
    })?;
    if used < 0 || unused < 0 || used.checked_add(unused) != Some(capacity) {
        return Err(format!(
            "{what} at {date}: occupied {used} and free {unused} do not make the capacity {capacity}"
        ));
    }
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

    #[test]
    fn a_held_day_whose_parts_do_not_make_its_capacity_fails_with_its_date_and_operands() {
        let error = held_point("2024-01-16".into(), 100, 150, "test history").unwrap_err();
        assert!(error.contains("2024-01-16"), "{error}");
        assert!(error.contains("150"), "{error}");
        assert!(error.contains("100"), "{error}");
    }
}
