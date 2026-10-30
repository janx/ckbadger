//! Entity membership discovery plus the one shared canonical feed projection.
use super::{
    asset_feed, asset_history,
    assets::{self, NftLifecycleStandard},
    collection_context::{collection_context, CollectionMembers},
};
use crate::{
    response::{default_limit, ok, ApiError, ApiResult, CursorPaginatedResponse},
    utils::parse_optional_block_tx_cursor,
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
    let lifecycle = assets::collect_nft_item_lifecycle_actions(state, &id, standard)?;
    let lifecycle_by_tx: HashMap<_, _> = lifecycle
        .iter()
        .map(|row| (format!("0x{}", hex::encode(&row.tx_hash)), row))
        .collect();
    let scope = asset_feed::canonical_scope(
        &state.store,
        lifecycle.iter().map(|row| row.tx_hash.clone()),
    )
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
        let meaning = lifecycle_by_tx
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
        // The row's meaning for this item is the lifecycle computation's,
        // never re-derived: `protocolActions` (the item's own actions, in
        // protocol order: bare `.cell` action names, `dotbit:<action>` for
        // `.bit`), `ringRelink` (why a `.cell` neighbour's cell was rewritten)
        // and `actedOn` (`neighbour` when the actions were about another item).
        // `versionDiff` joins the two versions this row moves between; a mint or
        // burn row has only one, so it has none.
        let version_diff = match (consumed.first(), created.first()) {
            (Some(old), Some(new)) => Some(
                super::asset_protocol::version_diff(standard, old, new).map_err(|e| {
                    ApiError::internal(format!(
                        "item=0x{} transaction={hash}: {e:#}",
                        hex::encode(&id)
                    ))
                })?,
            ),
            _ => None,
        };
        row["itemContext"] = json!({
            "itemId": format!("0x{}", hex::encode(&id)),
            "action": meaning.action,
            "protocolActions": meaning.protocol_actions,
            "ringRelink": meaning.ring_relink,
            "actedOn": meaning.acted_on,
            "createdCell": created.first(),
            "consumedCell": consumed.first(),
            "versionDiff": version_diff,
        });
    }
    ok(result)
}

/// One page of a collection's Activities / Transactions tab, read at the
/// source: the collection's activity index is seeked from the cursor for
/// `limit + 1` canonical rows, so a page costs the same whether the
/// collection has 100 or 653,018 activities (ID-COL-1). `total` is the
/// collection's one activity count (`ResolvedCollection::activities_count`),
/// the same number its detail page reports.
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
    }
    let collection = assets::get_collection_aggregate(&state.store, &id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Collection not found"))?;
    let context =
        |e: anyhow::Error| ApiError::internal(format!("collection=0x{}: {e:#}", hex::encode(&id)));
    let limit = asset_feed::feed_limit(params.limit);
    let cursor =
        parse_optional_block_tx_cursor(params.cursor.as_deref(), "asset transaction cursor")?;
    let page_size =
        usize::try_from(limit).map_err(|_| ApiError::internal("feed limit overflow"))?;
    let mut rows = assets::list_canonical_nft_collection_activities_page(
        &state.store,
        &state.append_only_store,
        &id,
        page_size + 1,
        cursor,
        None,
    )
    .map_err(context)?;
    let more = rows.len() > page_size;
    rows.truncate(page_size);
    let page = asset_feed::canonical_scope(
        &state.store,
        rows.iter().map(|(_, _, entry)| entry.tx_hash.clone()),
    )
    .map_err(context)?;
    let located: Vec<_> = page
        .iter()
        .map(|row| (row.block_number, row.tx_index))
        .collect();
    let indexed: Vec<_> = rows
        .iter()
        .map(|(block, index, _)| (*block, *index))
        .collect();
    if located != indexed {
        return Err(ApiError::internal(format!(
            "collection=0x{} activity index rows {indexed:?} do not match their canonical locations {located:?}",
            hex::encode(&id)
        )));
    }
    let next = if more {
        page.last().map(asset_feed::feed_cursor)
    } else {
        None
    };
    // What each row did to THIS collection's items (OB-C13), computed once.
    let members = CollectionMembers::of(collection.source, &id)?;
    let contexts = page
        .iter()
        .map(|row| {
            let actions = asset_feed::canonical_actions(state, row).map_err(context)?;
            collection_context(&state.store, &members, &actions, row.entry.outputs_count)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let total = collection.activities_count(state, &id).map_err(context)?;
    let response = if transactions {
        serde_json::to_value(CursorPaginatedResponse::new(
            asset_feed::project_transactions(state, page)?,
            total,
            limit,
            next,
        ))
    } else {
        serde_json::to_value(CursorPaginatedResponse::new(
            asset_feed::project_activities(state, page)?,
            total,
            limit,
            next,
        ))
    };
    let mut response = response.map_err(|e| ApiError::internal(e.to_string()))?;
    let rows = response["data"]
        .as_array_mut()
        .ok_or_else(|| ApiError::internal("collection feed data is not an array"))?;
    for (row, context) in rows.iter_mut().zip(contexts) {
        row["collectionContext"] =
            serde_json::to_value(context).map_err(|e| ApiError::internal(e.to_string()))?;
    }
    ok(response)
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
