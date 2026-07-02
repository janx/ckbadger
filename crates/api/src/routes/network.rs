use axum::{extract::State, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;
use ckbadger_store::LatestStatus;

/// Latest crawl-round status, camelCase-serialized for the API.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestStatusResponse {
    pub round_id: u64,
    pub started: u64,
    pub finished: u64,
    pub dialed: u64,
    pub reachable: u64,
    pub unreachable: u64,
    pub foreign_dropped: u64,
    pub new_nodes: u64,
    pub total_known: u64,
    pub frontier_drained: bool,
}

impl From<LatestStatus> for LatestStatusResponse {
    fn from(s: LatestStatus) -> Self {
        Self {
            round_id: s.round_id,
            started: s.started,
            finished: s.finished,
            dialed: s.dialed,
            reachable: s.reachable,
            unreachable: s.unreachable,
            foreign_dropped: s.foreign_dropped,
            new_nodes: s.new_nodes,
            total_known: s.total_known,
            frontier_drained: s.frontier_drained,
        }
    }
}

/// Top-level network crawler summary: whether the crawler is enabled, whether any
/// data has been persisted yet, and the most recent completed round (if any).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    pub enabled: bool,
    pub has_data: bool,
    pub last_round: Option<LatestStatusResponse>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/network/summary", get(summary))
}

async fn summary(State(state): State<Arc<AppState>>) -> ApiResult<NetworkSummary> {
    let last_round = match &state.network_store {
        Some(s) => s
            .get_network_status()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map(Into::into),
        None => None,
    };
    ok(NetworkSummary {
        enabled: state.crawler_enabled,
        has_data: last_round.is_some(),
        last_round,
    })
}
