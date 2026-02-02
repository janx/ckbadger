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
        (_, Some(pattern)) => sqlx::query_as(
            "SELECT COUNT(*) FROM tokens WHERE LOWER(name) LIKE $1 OR LOWER(symbol) LIKE $1",
        )
        .bind(pattern)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?,
        _ => sqlx::query_as("SELECT COUNT(*) FROM tokens")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let dob_count: (i64,) = match (filter_type, search_pattern) {
        (Some("token") | Some("nft"), _) => (0,),
        (_, Some(pattern)) => {
            sqlx::query_as(r#"SELECT COUNT(*) FROM spore_clusters WHERE LOWER(name) LIKE $1"#)
                .bind(pattern)
                .fetch_one(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        _ => sqlx::query_as("SELECT COUNT(*) FROM spore_clusters")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let nft_count: (i64,) = match (filter_type, search_pattern) {
        (Some("token") | Some("dob"), _) => (0,),
        (_, Some(pattern)) => {
            sqlx::query_as(r#"SELECT COUNT(*) FROM mnft_classes WHERE LOWER(name) LIKE $1"#)
                .bind(pattern)
                .fetch_one(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        _ => sqlx::query_as("SELECT COUNT(*) FROM mnft_classes")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let total = token_count.0 + dob_count.0 + nft_count.0;

    type TokenRow = (
        i64,
        Vec<u8>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
        bool,
        Option<Vec<String>>,
        i32,
        i64,
        i64,
        i16,
        String,
    );

    let tokens: Vec<TokenRow> = if matches!(filter_type, Some("nft") | Some("dob")) {
        vec![]
    } else if let Some(pattern) = search_pattern {
        let query_str = r#"
            SELECT id, type_script_hash, standard, name, symbol, icon_url,
                   COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                   tags, holders_count, transfers_count, transfers_24h,
                   decimals, total_supply::text
            FROM tokens
            WHERE (transfers_24h, holders_count, id) < ($1, $2, $3)
              AND (LOWER(name) LIKE $4 OR LOWER(symbol) LIKE $4)
            ORDER BY transfers_24h DESC, holders_count DESC, id DESC
            LIMIT $5
        "#;
        sqlx::query_as::<_, TokenRow>(query_str)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(pattern)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query_str = r#"
            SELECT id, type_script_hash, standard, name, symbol, icon_url,
                   COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                   tags, holders_count, transfers_count, transfers_24h,
                   decimals, total_supply::text
            FROM tokens
            WHERE (transfers_24h, holders_count, id) < ($1, $2, $3)
            ORDER BY transfers_24h DESC, holders_count DESC, id DESC
            LIMIT $4
        "#;
        sqlx::query_as::<_, TokenRow>(query_str)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    // ClusterRow: (id, cluster_id, name, description, spores_count, holders_count, transfers_count, transfers_24h)
    type ClusterRow = (
        i64,
        Vec<u8>,
        Option<String>,
        Option<String>,
        i32,
        i64,
        i64,
        i64,
    );

    let clusters: Vec<ClusterRow> = if matches!(filter_type, Some("token") | Some("nft")) {
        vec![]
    } else if let Some(pattern) = search_pattern {
        let query_str = r#"
            SELECT 
                sc.id, 
                sc.cluster_id, 
                sc.name, 
                sc.description, 
                sc.spores_count,
                COALESCE(h.holders_count, 0) AS holders_count,
                COALESCE(t.transfers_count, 0) AS transfers_count,
                COALESCE(t.transfers_24h, 0) AS transfers_24h
            FROM spore_clusters sc
            LEFT JOIN (
                SELECT cluster_id, COUNT(DISTINCT owner_lock_hash) AS holders_count
                FROM spore_cells
                WHERE cluster_id IS NOT NULL AND is_live = TRUE
                GROUP BY cluster_id
            ) h ON h.cluster_id = sc.cluster_id
            LEFT JOIN (
                SELECT 
                    DECODE(SUBSTRING(metadata->>'clusterId' FROM 3), 'hex') AS cluster_id,
                    COUNT(*) AS transfers_count,
                    COUNT(*) FILTER (WHERE timestamp > NOW() - INTERVAL '24 hours') AS transfers_24h
                FROM activities
                WHERE activity_category = 'dob' AND metadata->>'clusterId' IS NOT NULL
                GROUP BY metadata->>'clusterId'
            ) t ON t.cluster_id = sc.cluster_id
            WHERE sc.id < $1 AND LOWER(sc.name) LIKE $2
            ORDER BY COALESCE(t.transfers_24h, 0) DESC, COALESCE(h.holders_count, 0) DESC, sc.id DESC
            LIMIT $3
        "#;
        sqlx::query_as::<_, ClusterRow>(query_str)
            .bind(cursor_id)
            .bind(pattern)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query_str = r#"
            SELECT 
                sc.id, 
                sc.cluster_id, 
                sc.name, 
                sc.description, 
                sc.spores_count,
                COALESCE(h.holders_count, 0) AS holders_count,
                COALESCE(t.transfers_count, 0) AS transfers_count,
                COALESCE(t.transfers_24h, 0) AS transfers_24h
            FROM spore_clusters sc
            LEFT JOIN (
                SELECT cluster_id, COUNT(DISTINCT owner_lock_hash) AS holders_count
                FROM spore_cells
                WHERE cluster_id IS NOT NULL AND is_live = TRUE
                GROUP BY cluster_id
            ) h ON h.cluster_id = sc.cluster_id
            LEFT JOIN (
                SELECT 
                    DECODE(SUBSTRING(metadata->>'clusterId' FROM 3), 'hex') AS cluster_id,
                    COUNT(*) AS transfers_count,
                    COUNT(*) FILTER (WHERE timestamp > NOW() - INTERVAL '24 hours') AS transfers_24h
                FROM activities
                WHERE activity_category = 'dob' AND metadata->>'clusterId' IS NOT NULL
                GROUP BY metadata->>'clusterId'
            ) t ON t.cluster_id = sc.cluster_id
            WHERE sc.id < $1
            ORDER BY COALESCE(t.transfers_24h, 0) DESC, COALESCE(h.holders_count, 0) DESC, sc.id DESC
            LIMIT $2
        "#;
        sqlx::query_as::<_, ClusterRow>(query_str)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    // MnftClassRow: (id, class_id, issuer_id, name, description, total, issued, holders_count, transfers_count, transfers_24h)
    type MnftClassRow = (
        i64,
        Vec<u8>,
        Vec<u8>,
        Option<String>,
        Option<String>,
        i32,
        i32,
        i64,
        i64,
        i64,
    );

    let mnft_classes: Vec<MnftClassRow> = if matches!(filter_type, Some("token") | Some("dob")) {
        vec![]
    } else if let Some(pattern) = search_pattern {
        let query_str = r#"
            SELECT 
                id, class_id, issuer_id, name, description, total, issued,
                holders_count::bigint, transfers_count, transfers_24h::bigint
            FROM mnft_classes
            WHERE id < $1 AND LOWER(name) LIKE $2 AND is_live = TRUE
            ORDER BY transfers_24h DESC, holders_count DESC, id DESC
            LIMIT $3
        "#;
        sqlx::query_as::<_, MnftClassRow>(query_str)
            .bind(cursor_id)
            .bind(pattern)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query_str = r#"
            SELECT 
                id, class_id, issuer_id, name, description, total, issued,
                holders_count::bigint, transfers_count, transfers_24h::bigint
            FROM mnft_classes
            WHERE id < $1 AND is_live = TRUE
            ORDER BY transfers_24h DESC, holders_count DESC, id DESC
            LIMIT $2
        "#;
        sqlx::query_as::<_, MnftClassRow>(query_str)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let mut assets: Vec<AssetRow> = Vec::new();

    for (
        _id,
        hash,
        standard,
        name,
        symbol,
        icon_url,
        published,
        famous,
        tags,
        holders,
        transfers,
        transfers_24h,
        decimals,
        supply,
    ) in tokens
    {
        assets.push(AssetRow {
            id: format!("0x{}", hex::encode(&hash)),
            asset_type: "token".to_string(),
            standard,
            name,
            symbol,
            icon_url,
            published,
            famous,
            tags,
            holders_count: holders as i64,
            transfers_count: transfers,
            transfers_24h,
            decimals: Some(decimals),
            total_supply: Some(supply),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
        });
    }

    for (
        _id,
        cluster_id,
        name,
        _description,
        spores_count,
        holders_count,
        transfers_count,
        transfers_24h,
    ) in clusters
    {
        assets.push(AssetRow {
            id: format!("0x{}", hex::encode(&cluster_id)),
            asset_type: "dob".to_string(),
            standard: "spore".to_string(),
            name: name.clone(),
            symbol: None,
            icon_url: None,
            published: false,
            famous: false,
            tags: None,
            holders_count,
            transfers_count,
            transfers_24h,
            decimals: None,
            total_supply: Some(spores_count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(format!("0x{}", hex::encode(&cluster_id))),
            cluster_name: name,
        });
    }

    for (
        _id,
        class_id,
        _issuer_id,
        name,
        _description,
        total,
        issued,
        holders_count,
        transfers_count,
        transfers_24h,
    ) in mnft_classes
    {
        assets.push(AssetRow {
            id: format!("0x{}", hex::encode(&class_id)),
            asset_type: "nft".to_string(),
            standard: "m-nft".to_string(),
            name: name.clone(),
            symbol: None,
            icon_url: None,
            published: false,
            famous: false,
            tags: None,
            holders_count: if holders_count > 0 {
                holders_count
            } else {
                issued as i64
            },
            transfers_count: if transfers_count > 0 {
                transfers_count
            } else {
                issued as i64
            },
            transfers_24h,
            decimals: None,
            total_supply: if total > 0 {
                Some(total.to_string())
            } else {
                None
            },
            content_type: None,
            content_size: None,
            cluster_id: Some(format!("0x{}", hex::encode(&class_id))),
            cluster_name: name,
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
