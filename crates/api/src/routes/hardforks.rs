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
use crate::routes::scripts::first_code_cell_deployment;
use crate::utils::{hash_type_to_string, list_version_code_cells};
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
    pub script_deployments: Vec<ScriptDeploymentMarkerResponse>,
    /// One lane per reference `hash_type`, in VM order: `type`, `data1`,
    /// `data2`, `data` (CH-41).
    pub hash_type_lanes: Vec<HashTypeLaneResponse>,
}

/// How the registry's script versions are referenced with one `hash_type`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashTypeLaneResponse {
    pub hash_type: &'static str,
    /// Registered versions with at least one registered reference form of
    /// this `hash_type`.
    pub known_versions: i64,
    /// Live cells whose lock or type references one of those forms (a cell
    /// using a form as both lock and type counts twice), exact decimal.
    pub live_references: String,
}

/// The lanes in VM order: `type` (resolved by type id), then the
/// data-hash forms by the CKB-VM version they run on.
const HASH_TYPE_LANES: [u8; 4] = [1, 2, 4, 0];

/// Count, per `hash_type`, the registry's versions referenced that way and
/// the live references of those registered forms, from each form's
/// persisted per-form counter. A registered form with no counter row was
/// never used on chain: it contributes no live reference.
fn hash_type_lanes(
    store: &ckbadger_store::CkbadgerStore,
    versions: &[(Vec<u8>, ckbadger_store::ScriptVersionInfo)],
) -> anyhow::Result<Vec<HashTypeLaneResponse>> {
    let mut lanes = Vec::with_capacity(HASH_TYPE_LANES.len());
    for hash_type in HASH_TYPE_LANES {
        let mut known_versions = 0i64;
        let mut forms = std::collections::BTreeSet::new();
        for (version_hash, info) in versions {
            if info.family_id.is_none() {
                continue;
            }
            let mut referenced = false;
            for (reference, form_type) in &info.registered_references {
                if *form_type == hash_type {
                    referenced = true;
                    forms.insert(reference.clone());
                }
            }
            if referenced {
                known_versions = known_versions.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "known version count overflow at 0x{}",
                        hex::encode(version_hash)
                    )
                })?;
            }
        }
        let mut live_references = 0i128;
        for reference in &forms {
            let Some(counter) = store.get_script_reference_info(hash_type, reference)? else {
                continue; // never referenced on chain
            };
            anyhow::ensure!(
                counter.lock_live_cells_count >= 0 && counter.type_live_cells_count >= 0,
                "negative live reference counter: hash_type={hash_type} reference=0x{} lock={} type={}",
                hex::encode(reference),
                counter.lock_live_cells_count,
                counter.type_live_cells_count
            );
            live_references = live_references
                .checked_add(i128::from(counter.lock_live_cells_count))
                .and_then(|sum| sum.checked_add(i128::from(counter.type_live_cells_count)))
                .ok_or_else(|| anyhow::anyhow!("live reference count overflow"))?;
        }
        lanes.push(HashTypeLaneResponse {
            hash_type: ckbadger_common::hash_type_label(hash_type)
                .ok_or_else(|| anyhow::anyhow!("no label for hash_type {hash_type}"))?,
            known_versions,
            live_references: live_references.to_string(),
        });
    }
    Ok(lanes)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDeploymentMarkerResponse {
    pub family_name: String,
    pub version_name: String,
    pub version_hash: String,
    pub code_cell_tx_hash: String,
    pub tx_index: i32,
    pub code_cell_output_index: i16,
    pub block_number: i64,
    pub timestamp_ms: i64,
    pub epoch_number: i64,
    /// The hash types the version is registered under (its registered
    /// reference forms), in lane order `type`, `data1`, `data2`, `data` — the
    /// lanes of `hashTypeLanes` this deployment belongs to.
    pub hash_types: Vec<&'static str>,
}

/// A version's registered reference hash types, in [`HASH_TYPE_LANES`] order.
fn registered_hash_types(
    info: &ckbadger_store::ScriptVersionInfo,
) -> anyhow::Result<Vec<&'static str>> {
    HASH_TYPE_LANES
        .iter()
        .filter(|lane| {
            info.registered_references
                .iter()
                .any(|(_, form)| form == *lane)
        })
        .map(|lane| {
            hash_type_to_string(*lane)
                .ok_or_else(|| anyhow::anyhow!("hash type lane {lane} has no label"))
        })
        .collect()
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
    let evidence_state = state.clone();
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

        let versions = store.list_script_versions()?;
        let hash_type_lanes = hash_type_lanes(&store, &versions)?;
        let mut script_deployments = Vec::new();
        for (version_hash, info) in versions {
            let Some(family_id) = info.family_id.as_deref() else {
                continue;
            };
            let family = store.get_script_family(family_id)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "script version 0x{} points to missing family {}",
                    hex::encode(&version_hash),
                    family_id
                )
            })?;
            if family.family_id != family_id {
                anyhow::bail!(
                    "script version 0x{} family id mismatch: indexed={}, record={}",
                    hex::encode(&version_hash),
                    family_id,
                    family.family_id
                );
            }
            let code_cells =
                list_version_code_cells(&store, &evidence_state.append_only_store, &version_hash)?;
            let Some(first) =
                first_code_cell_deployment(&evidence_state, &version_hash, &code_cells)?
            else {
                continue;
            };
            let hash_types = registered_hash_types(&info)?;
            script_deployments.push(ScriptDeploymentMarkerResponse {
                family_name: family.name.clone(),
                version_name: info.name.unwrap_or(family.name),
                version_hash: format!("0x{}", hex::encode(version_hash)),
                code_cell_tx_hash: format!("0x{}", hex::encode(first.tx_hash)),
                tx_index: first.tx_index,
                code_cell_output_index: first.output_index,
                block_number: first.block_number,
                timestamp_ms: first.timestamp_ms,
                epoch_number: first.epoch_number,
                hash_types,
            });
        }
        script_deployments.sort_by(|left, right| {
            left.block_number
                .cmp(&right.block_number)
                .then_with(|| left.tx_index.cmp(&right.tx_index))
                .then_with(|| {
                    left.code_cell_output_index
                        .cmp(&right.code_cell_output_index)
                })
                .then_with(|| left.version_hash.cmp(&right.version_hash))
        });

        Ok(HardforkTimelineResponse {
            network: network_owned,
            tip_epoch,
            tip_block,
            events,
            consensus_features,
            script_deployments,
            hash_type_lanes,
        })
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(result)
}
