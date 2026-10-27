//! Entity membership discovery plus the one shared canonical feed projection.
use super::{
    asset_feed, asset_history,
    assets::{self, NftLifecycleStandard},
};
use crate::{
    response::{default_limit, ok, ApiError, ApiResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};

#[derive(Deserialize)]
struct Params {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

fn item_feed(
    state: &Arc<AppState>,
    raw: &str,
    standard: NftLifecycleStandard,
    params: Params,
    transactions: bool,
) -> ApiResult<Value> {
    let id = asset_history::parse_item_id(raw, standard)?;
    let versions = asset_history::checked_versions(state, &id, standard)?;
    let actions = assets::collect_nft_item_lifecycle_actions(state, &id, standard)?;
    let action_map: HashMap<_, _> = actions
        .iter()
        .map(|(hash, action)| (format!("0x{}", hex::encode(hash)), action))
        .collect();
    let scope =
        asset_feed::canonical_scope(&state.store, actions.iter().map(|(hash, _)| hash.clone()))
            .map_err(|e| ApiError::internal(format!("item=0x{}: {e:#}", hex::encode(&id))))?;
    let mut result = if transactions {
        serde_json::to_value(
            asset_feed::transactions(state, scope, params.limit, params.cursor.as_deref())?.0,
        )
    } else {
        serde_json::to_value(
            asset_feed::activities(state, scope, params.limit, params.cursor.as_deref())?.0,
        )
    }
    .map_err(|e| ApiError::internal(e.to_string()))?;
    for row in result["data"].as_array_mut().expect("canonical feed array") {
        let hash = row[if transactions { "hash" } else { "txHash" }]
            .as_str()
            .ok_or_else(|| ApiError::internal("canonical asset feed transaction hash missing"))?;
        let action = action_map
            .get(hash)
            .ok_or_else(|| ApiError::internal("canonical item feed action missing"))?;
        let created: Vec<_> = versions
            .iter()
            .filter(|v| v.response.tx_hash == hash)
            .map(|v| &v.response)
            .collect();
        let consumed: Vec<_> = versions
            .iter()
            .filter(|v| v.response.consumed_by_tx.as_deref() == Some(hash))
            .map(|v| &v.response)
            .collect();
        if created.len() > 1 || consumed.len() > 1 {
            return Err(ApiError::internal(format!(
                "item=0x{} transaction={hash} has multiple item cell versions",
                hex::encode(&id)
            )));
        }
        row["itemContext"] = json!({"itemId":format!("0x{}",hex::encode(&id)),"action":action,"createdCell":created.first(),"consumedCell":consumed.first()});
    }
    ok(result)
}

fn collection_feed(
    state: &Arc<AppState>,
    raw: &str,
    kind: &str,
    params: Params,
    transactions: bool,
) -> ApiResult<Value> {
    let id = match kind {
        "spore" => super::spore::parse_cluster_id_param(raw)?,
        "identity" => super::identities::decode_identity_collection_id(raw)?,
        _ => assets::decode_object_collection_id(raw)?,
    };
    if kind == "spore" {
        super::spore::read_cluster(state, &id)?;
    } else if assets::get_collection_aggregate(&state.store, &id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_none()
    {
        return Err(ApiError::not_found("Collection not found"));
    }
    let mut hashes = Vec::new();
    let mut cursor = None;
    loop {
        let rows = assets::list_canonical_nft_collection_activities_page(
            &state.store,
            &state.append_only_store,
            &id,
            1024,
            cursor,
            None,
        )
        .map_err(|e| ApiError::internal(format!("collection=0x{}: {e:#}", hex::encode(&id))))?;
        if rows.is_empty() {
            break;
        }
        cursor = rows.last().map(|(block, index, _)| (*block, *index));
        hashes.extend(rows.into_iter().map(|(_, _, entry)| entry.tx_hash));
    }
    let scope = asset_feed::canonical_scope(&state.store, hashes)
        .map_err(|e| ApiError::internal(format!("collection=0x{}: {e:#}", hex::encode(&id))))?;
    let response = if transactions {
        serde_json::to_value(
            asset_feed::transactions(state, scope, params.limit, params.cursor.as_deref())?.0,
        )
    } else {
        serde_json::to_value(
            asset_feed::activities(state, scope, params.limit, params.cursor.as_deref())?.0,
        )
    };
    ok(response.map_err(|e| ApiError::internal(e.to_string()))?)
}
macro_rules! item_handlers {
    ($activity:ident,$transactions:ident,$standard:ident) => {
        async fn $activity(
            State(state): State<Arc<AppState>>,
            Path(id): Path<String>,
            Query(params): Query<Params>,
        ) -> ApiResult<Value> {
            tokio::task::spawn_blocking(move || {
                item_feed(&state, &id, NftLifecycleStandard::$standard, params, false)
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
        }
        async fn $transactions(
            State(state): State<Arc<AppState>>,
            Path(id): Path<String>,
            Query(params): Query<Params>,
        ) -> ApiResult<Value> {
            tokio::task::spawn_blocking(move || {
                item_feed(&state, &id, NftLifecycleStandard::$standard, params, true)
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
        }
    };
}
item_handlers!(spore_activity, spore_transactions, Spore);
item_handlers!(mnft_activity, mnft_transactions, MnftToken);
item_handlers!(did_activity, did_transactions, DidCkb);
item_handlers!(bit_cell_activity, bit_cell_transactions, BitCell);
item_handlers!(dotbit_activity, dotbit_transactions, DotBit);
item_handlers!(dotcell_activity, dotcell_transactions, DotCell);
macro_rules! collection_handlers {
    ($activity:ident,$transactions:ident,$kind:literal) => {
        async fn $activity(
            State(state): State<Arc<AppState>>,
            Path(id): Path<String>,
            Query(params): Query<Params>,
        ) -> ApiResult<Value> {
            tokio::task::spawn_blocking(move || collection_feed(&state, &id, $kind, params, false))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        async fn $transactions(
            State(state): State<Arc<AppState>>,
            Path(id): Path<String>,
            Query(params): Query<Params>,
        ) -> ApiResult<Value> {
            tokio::task::spawn_blocking(move || collection_feed(&state, &id, $kind, params, true))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
    };
}
collection_handlers!(cluster_activity, cluster_transactions, "spore");
collection_handlers!(
    object_collection_activity,
    object_collection_transactions,
    "object"
);
collection_handlers!(
    identity_collection_activity,
    identity_collection_transactions,
    "identity"
);
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/spore/objects/{spore_id}/activity-feed",
            get(spore_activity),
        )
        .route(
            "/spore/objects/{spore_id}/transactions",
            get(spore_transactions),
        )
        .route(
            "/assets/objects/items/{object_id}/activity-feed",
            get(mnft_activity),
        )
        .route(
            "/assets/objects/items/{object_id}/transactions",
            get(mnft_transactions),
        )
        .route(
            "/assets/identities/did/items/{identity_id}/activity-feed",
            get(did_activity),
        )
        .route(
            "/assets/identities/did/items/{identity_id}/transactions",
            get(did_transactions),
        )
        .route(
            "/assets/identities/bit-cell/items/{identity_id}/activity-feed",
            get(bit_cell_activity),
        )
        .route(
            "/assets/identities/bit-cell/items/{identity_id}/transactions",
            get(bit_cell_transactions),
        )
        .route(
            "/assets/identities/dotbit/items/{identity_id}/activity-feed",
            get(dotbit_activity),
        )
        .route(
            "/assets/identities/dotbit/items/{identity_id}/transactions",
            get(dotbit_transactions),
        )
        .route(
            "/assets/identities/dotcell/items/{id_or_name}/activity-feed",
            get(dotcell_activity),
        )
        .route(
            "/assets/identities/dotcell/items/{id_or_name}/transactions",
            get(dotcell_transactions),
        )
        .route(
            "/spore/clusters/{cluster_id}/activity-feed",
            get(cluster_activity),
        )
        .route(
            "/spore/clusters/{cluster_id}/transactions",
            get(cluster_transactions),
        )
        .route(
            "/assets/objects/{collection_id}/activity-feed",
            get(object_collection_activity),
        )
        .route(
            "/assets/objects/{collection_id}/transactions",
            get(object_collection_transactions),
        )
        .route(
            "/assets/identities/{collection_id}/activity-feed",
            get(identity_collection_activity),
        )
        .route(
            "/assets/identities/{collection_id}/transactions",
            get(identity_collection_transactions),
        )
}
