use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/assets", get(list_assets))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetResponse {
    pub id: String,
    pub asset_type: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub icon_url: Option<String>,
    pub published: bool,
    pub famous: bool,
    pub tags: Option<Vec<String>>,
    pub holders_count: i64,
    pub transfers_count: i64,
    pub transfers_24h: i64,
    pub decimals: Option<i16>,
    pub total_supply: Option<String>,
    pub content_type: Option<String>,
    pub content_size: Option<i32>,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
}

async fn list_assets(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let limit = params.limit.clamp(1, 100);

    let (cursor_24h, cursor_holders, cursor_id, cursor_type) = params
        .cursor
        .as_ref()
        .and_then(|c| {
            let parts: Vec<&str> = c.split(':').collect();
            if parts.len() == 4 {
                Some((
                    parts[0].parse::<i64>().ok()?,
                    parts[1].parse::<i64>().ok()?,
                    parts[2].parse::<i64>().ok()?,
                    parts[3].to_string(),
                ))
            } else {
                None
            }
        })
        .unwrap_or((i64::MAX, i64::MAX, i64::MAX, "z".to_string()));

    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    let filter_type = params.asset_type.as_deref();

    let (total, rows) = fetch_assets(
        &state,
        filter_type,
        search_pattern.as_deref(),
        cursor_24h,
        cursor_holders,
        cursor_id,
        &cursor_type,
        limit,
    )
    .await?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| {
            format!(
                "{}:{}:{}:{}",
                r.transfers_24h, r.holders_count, r.id, r.asset_type
            )
        })
    } else {
        None
    };

    let assets: Vec<AssetResponse> = rows
        .into_iter()
        .map(|r| AssetResponse {
            id: r.id,
            asset_type: r.asset_type,
            standard: r.standard,
            name: r.name,
            symbol: r.symbol,
            icon_url: r.icon_url,
            published: r.published,
            famous: r.famous,
            tags: r.tags,
            holders_count: r.holders_count,
            transfers_count: r.transfers_count,
            transfers_24h: r.transfers_24h,
            decimals: r.decimals,
            total_supply: r.total_supply,
            content_type: r.content_type,
            content_size: r.content_size,
            cluster_id: r.cluster_id,
            cluster_name: r.cluster_name,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        assets,
        total,
        limit,
        next_cursor,
    ))
}

#[derive(Debug)]
struct AssetRow {
    id: String,
    asset_type: String,
    standard: String,
    name: Option<String>,
    symbol: Option<String>,
    icon_url: Option<String>,
    published: bool,
    famous: bool,
    tags: Option<Vec<String>>,
    holders_count: i64,
    transfers_count: i64,
    transfers_24h: i64,
    decimals: Option<i16>,
    total_supply: Option<String>,
    content_type: Option<String>,
    content_size: Option<i32>,
    cluster_id: Option<String>,
    cluster_name: Option<String>,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct CountRow {
    count: i64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct TokenRowData {
    id: i64,
    type_script_hash: Vec<u8>,
    standard: String,
    name: Option<String>,
    symbol: Option<String>,
    icon_url: Option<String>,
    published: bool,
    famous: bool,
    tags: Option<Vec<String>>,
    holders_count: i32,
    transfers_count: i64,
    transfers_24h: i64,
    decimals: i16,
    total_supply: String,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct ClusterRowData {
    id: i64,
    cluster_id: Vec<u8>,
    name: Option<String>,
    description: Option<String>,
    spores_count: i32,
    holders_count: i64,
    transfers_count: i64,
    transfers_24h: i64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct MnftClassRowData {
    id: i64,
    class_id: Vec<u8>,
    issuer_id: Vec<u8>,
    name: Option<String>,
    description: Option<String>,
    total: i32,
    issued: i32,
    holders_count: i64,
    transfers_count: i64,
    transfers_24h: i64,
}

#[allow(clippy::too_many_arguments)]
async fn fetch_assets(
    state: &Arc<AppState>,
    filter_type: Option<&str>,
    search_pattern: Option<&str>,
    cursor_24h: i64,
    cursor_holders: i64,
    cursor_id: i64,
    _cursor_type: &str,
    limit: i64,
) -> Result<(i64, Vec<AssetRow>), (StatusCode, Json<ApiError>)> {
    let token_count: (i64,) = match (filter_type, search_pattern) {
        (Some("nft") | Some("dob"), _) => (0,),
        (_, Some(pattern)) => {
            let query = format!(
                "SELECT COUNT(*) as count FROM tokens WHERE lower(name) LIKE '{}' OR lower(symbol) LIKE '{}'",
                pattern, pattern
            );
            let count_row = state
                .clickhouse
                .client()
                .query(&query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (count_row.count,)
        }
        _ => {
            let count_row = state
                .clickhouse
                .client()
                .query("SELECT COUNT(*) as count FROM tokens")
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (count_row.count,)
        }
    };

    let dob_count: (i64,) = match (filter_type, search_pattern) {
        (Some("token") | Some("nft"), _) => (0,),
        (_, Some(pattern)) => {
            let query = format!(
                "SELECT COUNT(*) as count FROM spore_clusters WHERE lower(name) LIKE '{}'",
                pattern
            );
            let count_row = state
                .clickhouse
                .client()
                .query(&query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (count_row.count,)
        }
        _ => {
            let count_row = state
                .clickhouse
                .client()
                .query("SELECT COUNT(*) as count FROM spore_clusters")
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (count_row.count,)
        }
    };

    let nft_count: (i64,) = match (filter_type, search_pattern) {
        (Some("token") | Some("dob"), _) => (0,),
        (_, Some(pattern)) => {
            let query = format!(
                "SELECT COUNT(*) as count FROM mnft_classes WHERE lower(name) LIKE '{}'",
                pattern
            );
            let count_row = state
                .clickhouse
                .client()
                .query(&query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (count_row.count,)
        }
        _ => {
            let count_row = state
                .clickhouse
                .client()
                .query("SELECT COUNT(*) as count FROM mnft_classes")
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            (count_row.count,)
        }
    };

    let total = token_count.0 + dob_count.0 + nft_count.0;

    let tokens: Vec<TokenRowData> = if matches!(filter_type, Some("nft") | Some("dob")) {
        vec![]
    } else if let Some(pattern) = search_pattern {
        let query = format!(
            "SELECT id, type_script_hash, standard, name, symbol, icon_url, \
             published, famous, tags, holders_count, transfers_count, transfers_24h, \
             decimals, total_supply \
             FROM tokens \
             WHERE (transfers_24h, holders_count, id) < ({}, {}, {}) \
             AND (lower(name) LIKE '{}' OR lower(symbol) LIKE '{}') \
             ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
             LIMIT {}",
            cursor_24h,
            cursor_holders,
            cursor_id,
            pattern,
            pattern,
            limit + 1
        );
        state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<TokenRowData>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query = format!(
            "SELECT id, type_script_hash, standard, name, symbol, icon_url, \
             published, famous, tags, holders_count, transfers_count, transfers_24h, \
             decimals, total_supply \
             FROM tokens \
             WHERE (transfers_24h, holders_count, id) < ({}, {}, {}) \
             ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
             LIMIT {}",
            cursor_24h,
            cursor_holders,
            cursor_id,
            limit + 1
        );
        state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<TokenRowData>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let clusters: Vec<ClusterRowData> = if matches!(filter_type, Some("token") | Some("nft")) {
        vec![]
    } else if let Some(pattern) = search_pattern {
        let query = format!(
            "SELECT \
             sc.id, sc.cluster_id, sc.name, sc.description, sc.spores_count, \
             COALESCE(h.holders_count, 0) AS holders_count, \
             COALESCE(t.transfers_count, 0) AS transfers_count, \
             COALESCE(t.transfers_24h, 0) AS transfers_24h \
             FROM spore_clusters sc \
             LEFT JOIN ( \
                 SELECT cluster_id, COUNT(DISTINCT owner_lock_hash) AS holders_count \
                 FROM spore_cells \
                 WHERE cluster_id IS NOT NULL AND is_live = true \
                 GROUP BY cluster_id \
             ) h ON h.cluster_id = sc.cluster_id \
             LEFT JOIN ( \
                 SELECT cluster_id, COUNT(*) AS transfers_count, \
                 COUNT(IF(timestamp > now() - INTERVAL 24 HOUR, 1, NULL)) AS transfers_24h \
                 FROM dob_transfers \
                 WHERE cluster_id IS NOT NULL \
                 GROUP BY cluster_id \
             ) t ON t.cluster_id = sc.cluster_id \
             WHERE sc.id < {} AND lower(sc.name) LIKE '{}' \
             ORDER BY COALESCE(t.transfers_24h, 0) DESC, COALESCE(h.holders_count, 0) DESC, sc.id DESC \
             LIMIT {}",
            cursor_id, pattern, limit + 1
        );
        state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<ClusterRowData>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query = format!(
            "SELECT \
             sc.id, sc.cluster_id, sc.name, sc.description, sc.spores_count, \
             COALESCE(h.holders_count, 0) AS holders_count, \
             COALESCE(t.transfers_count, 0) AS transfers_count, \
             COALESCE(t.transfers_24h, 0) AS transfers_24h \
             FROM spore_clusters sc \
             LEFT JOIN ( \
                 SELECT cluster_id, COUNT(DISTINCT owner_lock_hash) AS holders_count \
                 FROM spore_cells \
                 WHERE cluster_id IS NOT NULL AND is_live = true \
                 GROUP BY cluster_id \
             ) h ON h.cluster_id = sc.cluster_id \
             LEFT JOIN ( \
                 SELECT cluster_id, COUNT(*) AS transfers_count, \
                 COUNT(IF(timestamp > now() - INTERVAL 24 HOUR, 1, NULL)) AS transfers_24h \
                 FROM dob_transfers \
                 WHERE cluster_id IS NOT NULL \
                 GROUP BY cluster_id \
             ) t ON t.cluster_id = sc.cluster_id \
             WHERE sc.id < {} \
             ORDER BY COALESCE(t.transfers_24h, 0) DESC, COALESCE(h.holders_count, 0) DESC, sc.id DESC \
             LIMIT {}",
            cursor_id, limit + 1
        );
        state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<ClusterRowData>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let mnft_classes: Vec<MnftClassRowData> = if matches!(filter_type, Some("token") | Some("dob"))
    {
        vec![]
    } else if let Some(pattern) = search_pattern {
        let query = format!(
            "SELECT \
             mc.id, mc.class_id, mc.issuer_id, mc.name, mc.description, \
             mc.total, mc.issued, \
             COALESCE(h.holders_count, 0) AS holders_count, \
             COALESCE(t.transfers_count, 0) AS transfers_count, \
             COALESCE(t.transfers_24h, 0) AS transfers_24h \
             FROM mnft_classes mc \
             LEFT JOIN ( \
                 SELECT class_id, COUNT(DISTINCT to_lock_hash) AS holders_count \
                 FROM nft_transfers \
                 WHERE class_id IS NOT NULL AND event_type != 'burn' \
                 GROUP BY class_id \
             ) h ON h.class_id = mc.class_id \
             LEFT JOIN ( \
                 SELECT class_id, COUNT(*) AS transfers_count, \
                 COUNT(IF(timestamp > now() - INTERVAL 24 HOUR, 1, NULL)) AS transfers_24h \
                 FROM nft_transfers \
                 WHERE class_id IS NOT NULL \
                 GROUP BY class_id \
             ) t ON t.class_id = mc.class_id \
             WHERE mc.id < {} AND lower(mc.name) LIKE '{}' AND mc.is_live = true \
             ORDER BY COALESCE(t.transfers_24h, 0) DESC, COALESCE(h.holders_count, 0) DESC, mc.id DESC \
             LIMIT {}",
            cursor_id, pattern, limit + 1
        );
        state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<MnftClassRowData>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query = format!(
            "SELECT \
             mc.id, mc.class_id, mc.issuer_id, mc.name, mc.description, \
             mc.total, mc.issued, \
             COALESCE(h.holders_count, 0) AS holders_count, \
             COALESCE(t.transfers_count, 0) AS transfers_count, \
             COALESCE(t.transfers_24h, 0) AS transfers_24h \
             FROM mnft_classes mc \
             LEFT JOIN ( \
                 SELECT class_id, COUNT(DISTINCT to_lock_hash) AS holders_count \
                 FROM nft_transfers \
                 WHERE class_id IS NOT NULL AND event_type != 'burn' \
                 GROUP BY class_id \
             ) h ON h.class_id = mc.class_id \
             LEFT JOIN ( \
                 SELECT class_id, COUNT(*) AS transfers_count, \
                 COUNT(IF(timestamp > now() - INTERVAL 24 HOUR, 1, NULL)) AS transfers_24h \
                 FROM nft_transfers \
                 WHERE class_id IS NOT NULL \
                 GROUP BY class_id \
             ) t ON t.class_id = mc.class_id \
             WHERE mc.id < {} AND mc.is_live = true \
             ORDER BY COALESCE(t.transfers_24h, 0) DESC, COALESCE(h.holders_count, 0) DESC, mc.id DESC \
             LIMIT {}",
            cursor_id, limit + 1
        );
        state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<MnftClassRowData>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let mut assets: Vec<AssetRow> = Vec::new();

    for row in tokens {
        assets.push(AssetRow {
            id: format!("0x{}", hex::encode(&row.type_script_hash)),
            asset_type: "token".to_string(),
            standard: row.standard,
            name: row.name,
            symbol: row.symbol,
            icon_url: row.icon_url,
            published: row.published,
            famous: row.famous,
            tags: row.tags,
            holders_count: row.holders_count as i64,
            transfers_count: row.transfers_count,
            transfers_24h: row.transfers_24h,
            decimals: Some(row.decimals),
            total_supply: Some(row.total_supply),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
        });
    }

    for row in clusters {
        assets.push(AssetRow {
            id: format!("0x{}", hex::encode(&row.cluster_id)),
            asset_type: "dob".to_string(),
            standard: "spore".to_string(),
            name: row.name.clone(),
            symbol: None,
            icon_url: None,
            published: false,
            famous: false,
            tags: None,
            holders_count: row.holders_count,
            transfers_count: row.transfers_count,
            transfers_24h: row.transfers_24h,
            decimals: None,
            total_supply: Some(row.spores_count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(format!("0x{}", hex::encode(&row.cluster_id))),
            cluster_name: row.name,
        });
    }

    for row in mnft_classes {
        assets.push(AssetRow {
            id: format!("0x{}", hex::encode(&row.class_id)),
            asset_type: "nft".to_string(),
            standard: "m-nft".to_string(),
            name: row.name.clone(),
            symbol: None,
            icon_url: None,
            published: false,
            famous: false,
            tags: None,
            holders_count: if row.holders_count > 0 {
                row.holders_count
            } else {
                row.issued as i64
            },
            transfers_count: if row.transfers_count > 0 {
                row.transfers_count
            } else {
                row.issued as i64
            },
            transfers_24h: row.transfers_24h,
            decimals: None,
            total_supply: if row.total > 0 {
                Some(row.total.to_string())
            } else {
                None
            },
            content_type: None,
            content_size: None,
            cluster_id: Some(format!("0x{}", hex::encode(&row.class_id))),
            cluster_name: row.name,
        });
    }

    assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
            .then_with(|| a.asset_type.cmp(&b.asset_type))
            .then_with(|| b.id.cmp(&a.id))
    });

    assets.truncate((limit + 1) as usize);

    Ok((total, assets))
}
