//! Complete-day bounds for the existing asset capacity chart builders.
use super::statistics::StackedAreaChartResponse;
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
