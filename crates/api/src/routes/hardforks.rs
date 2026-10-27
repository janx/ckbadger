use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use ckbadger_common::{hardforks_for_network, normalize_network};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::consensus::{load_hardfork_features, HardforkFeature};
use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct HardforksQuery {
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardforkResourceResponse {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardforkEventResponse {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub edition_year: i32,
    pub activation_epoch: i64,
    pub activation_date: String,
    pub activation_block: Option<i64>,
    pub status: String,
    pub summary: String,
    pub resources: Vec<HardforkResourceResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardforkTimelineResponse {
    pub network: String,
    pub tip_epoch: i64,
    pub tip_block: i64,
    pub events: Vec<HardforkEventResponse>,
    pub consensus_features: Vec<HardforkFeature>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/hardforks", get(list_hardforks))
}

async fn list_hardforks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HardforksQuery>,
) -> ApiResult<HardforkTimelineResponse> {
    let network_raw = query.network.as_deref().unwrap_or(&state.ckb_network);
    let network = normalize_network(network_raw).ok_or_else(|| {
        ApiError::bad_request(format!(
            "unsupported network '{}' (expected mainnet/testnet)",
            network_raw
        ))
    })?;
    if normalize_network(&state.ckb_network) != Some(network) {
        return Err(ApiError::bad_request(format!(
            "requested network '{}' differs from this node's configured network '{}'",
            network, state.ckb_network
        )));
    }

    let hardforks = hardforks_for_network(network).ok_or_else(|| {
        ApiError::internal(format!(
            "hardfork list missing for normalized network '{}'",
            network
        ))
    })?;

    let consensus_features = load_hardfork_features(&state.ckb_rpc_url)
        .await
        .map_err(ApiError::internal)?;
    for (rfc, expected) in [
        ("0032", hardforks[0].activation_epoch),
        ("0049", hardforks[1].activation_epoch),
    ] {
        let actual = consensus_features
            .iter()
            .find(|feature| feature.rfc == rfc)
            .and_then(|feature| feature.activation_epoch);
        if actual != Some(expected as u64) {
            return Err(ApiError::internal(format!(
                "get_consensus RFC {rfc} activation {:?} disagrees with {} edition epoch {expected}",
                actual, network
            )));
        }
    }
    let store = state.store.clone();
    let network_owned = network.to_string();
    let hardforks_owned: Vec<_> = hardforks.to_vec();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let (tip_block, tip_epoch) = store
            .get_sync_tip_block()?
            .map_or((0i64, 0i64), |(num, header)| (num, header.epoch_number));

        let mut events = Vec::with_capacity(hardforks_owned.len());
        for spec in &hardforks_owned {
            let activation_block = store
                .get_epoch_stats(spec.activation_epoch)?
                .map(|stats| stats.start_block);

            let status = if tip_epoch >= spec.activation_epoch {
                "activated"
            } else {
                "upcoming"
            };

            events.push(HardforkEventResponse {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                short_name: spec.short_name.to_string(),
                edition_year: spec.edition_year,
                activation_epoch: spec.activation_epoch,
                activation_date: spec.activation_date.to_string(),
                activation_block,
                status: status.to_string(),
                summary: spec.summary.to_string(),
                resources: spec
                    .resources
                    .iter()
                    .map(|resource| HardforkResourceResponse {
                        label: resource.label.to_string(),
                        url: resource.url.to_string(),
                    })
                    .collect(),
            });
        }

        Ok(HardforkTimelineResponse {
            network: network_owned,
            tip_epoch,
            tip_block,
            events,
            consensus_features,
        })
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(result)
}
