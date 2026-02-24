use crate::cache::CacheTtl;
use crate::routes::assets::AssetResponse;
use crate::routes::statistics::build_block_time_distribution_response;
use crate::utils::{
    accumulate_live_capacity, resolve_dob_collection_name, resolve_nft_collection_name,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);

// Cache keys for assets
pub const CACHE_KEY_ASSETS_TOKEN: &str = "assets:token";
pub const CACHE_KEY_ASSETS_NFT: &str = "assets:nft";

/// Cached asset entry with pre-computed metrics, ready for API serving.
#[derive(Clone, Serialize, Deserialize)]
pub struct CachedAssetEntry {
    pub id: String,
    pub asset_type: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub icon_url: Option<String>,
    pub holders_count: i64,
    pub transfers_count: i64,
    pub transfers_24h: i64,
    pub decimals: Option<i16>,
    pub total_supply: Option<String>,
    pub maximum_supply: Option<String>,
    pub content_type: Option<String>,
    pub content_size: Option<i32>,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
    pub live_capacity: Option<String>,
    pub live_occupied_capacity: Option<String>,
    // Token-specific fields (None for NFT entries)
    pub type_code_hash: Option<String>,
    pub type_hash_type: Option<String>,
    pub type_args: Option<String>,
    pub description: Option<String>,
}

impl CachedAssetEntry {
    pub fn to_asset_response(&self) -> AssetResponse {
        AssetResponse {
            id: self.id.clone(),
            asset_type: self.asset_type.clone(),
            standard: self.standard.clone(),
            name: self.name.clone(),
            symbol: self.symbol.clone(),
            icon_url: self.icon_url.clone(),
            published: false,
            famous: false,
            tags: None,
            holders_count: self.holders_count,
            transfers_count: self.transfers_count,
            transfers_24h: self.transfers_24h,
            decimals: self.decimals,
            total_supply: self.total_supply.clone(),
            content_type: self.content_type.clone(),
            content_size: self.content_size,
            cluster_id: self.cluster_id.clone(),
            cluster_name: self.cluster_name.clone(),
            live_capacity: self.live_capacity.clone(),
            live_occupied_capacity: self.live_occupied_capacity.clone(),
        }
    }
}

/// Background loop that refreshes the assets cache every 30 seconds.
pub async fn refresh_assets_cache_loop(state: Arc<AppState>) {
    loop {
        let state_clone = state.clone();
        let result =
            tokio::task::spawn_blocking(move || refresh_assets_cache_sync(&state_clone)).await;

        match result {
            Ok(Ok(())) => {
                tracing::debug!("Assets cache refreshed successfully");
            }
            Ok(Err(e)) => {
                tracing::warn!("Assets cache refresh failed: {}", e);
            }
            Err(e) => {
                tracing::warn!("Assets cache refresh task panicked: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

fn hash_type_to_string(hash_type: u8) -> String {
    match hash_type {
        0 => "data".to_string(),
        1 => "type".to_string(),
        2 => "data1".to_string(),
        4 => "data2".to_string(),
        _ => format!("unknown({})", hash_type),
    }
}

/// Sync function that computes and caches all asset lists.
/// Uses pre-aggregated CFs for NFTs, including Spore/DOB collections.
/// Uses a single scan for all token 24h transfers instead of N+1 per-token queries.
fn refresh_assets_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let ttl = CacheTtl::ASSETS;
    let now_ms = chrono::Utc::now().timestamp_millis();

    // -- Token assets (2 scans: list_tokens + scan_all_token_24h_transfers) --
    let tokens = state.store.list_tokens()?;
    let transfers_24h_map = state.store.scan_all_token_24h_transfers(now_ms)?;
    let mut token_assets: Vec<CachedAssetEntry> = Vec::with_capacity(tokens.len());

    for (hash, info) in &tokens {
        // Skip noise tokens: no name/symbol and no holders
        if info.name.is_none() && info.symbol.is_none() && info.holders_count == 0 {
            continue;
        }

        let transfers_24h = transfers_24h_map.get(hash.as_slice()).copied().unwrap_or(0);
        let token_daily = state.store.list_token_daily_deltas(hash)?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(token_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|err| {
                anyhow::anyhow!(
                    "invalid token daily deltas during warmup for type_hash=0x{}: {}",
                    hex::encode(hash),
                    err
                )
            })?;

        token_assets.push(CachedAssetEntry {
            id: format!("0x{}", hex::encode(hash)),
            asset_type: "token".to_string(),
            standard: info.standard.clone(),
            name: info.name.clone(),
            symbol: info.symbol.clone(),
            icon_url: info.icon_url.clone(),
            holders_count: info.holders_count,
            transfers_count: info.transfers_count,
            transfers_24h,
            decimals: info.decimals.map(|d| d as i16),
            total_supply: info.total_supply.map(|s| s.to_string()),
            maximum_supply: info.max_supply.map(|s| s.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            type_code_hash: Some(format!("0x{}", hex::encode(&info.type_code_hash))),
            type_hash_type: Some(hash_type_to_string(info.hash_type)),
            type_args: Some(format!("0x{}", hex::encode(&info.type_args))),
            description: info.description.clone(),
        });
    }

    token_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    state
        .mem_cache
        .set(CACHE_KEY_ASSETS_TOKEN, &token_assets, ttl);

    // -- NFT assets, including Spore/DOB collections --
    let mut nft_assets: Vec<CachedAssetEntry> = Vec::new();

    // Spore/DOB collections from pre-aggregated cluster_agg CF
    let cluster_aggs = state.heavy_store().list_cluster_aggregates()?;
    let spore_transfers_24h_map = state.heavy_store().scan_all_spore_24h_transfers(now_ms)?;
    nft_assets.reserve(cluster_aggs.len());

    for (cluster_id_bytes, agg) in &cluster_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let cluster_hex = format!("0x{}", hex::encode(cluster_id_bytes));
        let display_name = resolve_dob_collection_name(
            state.heavy_store().as_ref(),
            cluster_id_bytes,
            agg.name.as_deref(),
        );
        let transfers_24h = spore_transfers_24h_map
            .get(cluster_id_bytes.as_slice())
            .copied()
            .unwrap_or(0);
        let cluster_daily = state
            .heavy_store()
            .list_cluster_daily_deltas(cluster_id_bytes)?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(cluster_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| {
                anyhow::anyhow!(
                    "invalid cluster daily capacity deltas for cluster_id=0x{}: {}",
                    hex::encode(cluster_id_bytes),
                    e
                )
            })?;

        nft_assets.push(CachedAssetEntry {
            id: cluster_hex.clone(),
            asset_type: "nft".to_string(),
            standard: "spore".to_string(),
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.owner_count,
            transfers_count: agg.total_count,
            transfers_24h,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(cluster_hex),
            cluster_name: display_name,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    // NFT collections from pre-aggregated nft_collection_agg CF
    let nft_aggs = state.heavy_store().list_nft_collection_aggregates()?;
    let nft_transfers_24h_map = state.heavy_store().scan_all_nft_24h_transfers(now_ms)?;
    nft_assets.reserve(nft_aggs.len());

    for (collection_id_bytes, agg) in &nft_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let collection_hex = format!("0x{}", hex::encode(collection_id_bytes));
        let transfers_24h = nft_transfers_24h_map
            .get(collection_id_bytes.as_slice())
            .copied()
            .unwrap_or(0);
        let standard = agg.standard.asset_standard().to_string();
        let display_name = resolve_nft_collection_name(&standard, agg.name.as_deref());
        let nft_daily = state
            .heavy_store()
            .list_nft_daily_deltas(collection_id_bytes)?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(nft_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| {
                anyhow::anyhow!(
                    "invalid NFT daily capacity deltas for collection_id=0x{}: {}",
                    hex::encode(collection_id_bytes),
                    e
                )
            })?;

        nft_assets.push(CachedAssetEntry {
            id: collection_hex.clone(),
            asset_type: "nft".to_string(),
            standard,
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.live_count,
            transfers_count: agg.total_count,
            transfers_24h,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: display_name,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    nft_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    state.mem_cache.set(CACHE_KEY_ASSETS_NFT, &nft_assets, ttl);

    Ok(())
}

pub async fn warmup_chart_caches(state: Arc<AppState>) {
    info!("Starting cache warmup for charts...");

    // These chart caches used to be prefilled with placeholder payloads (often empty),
    // which overrides real chart handlers after Redis flush/restart.
    // Purge them on startup and let route handlers populate on first request.
    const STUB_CHART_KEYS: &[&str] = &[
        "chart:average-block-time",
        "chart:hash-rate",
        "chart:difficulty",
        "chart:uncle-rate",
        "chart:block-time-distribution",
        "chart:block-time-distribution:v2",
        "chart:epoch-time-distribution",
        "chart:epoch-time-length",
        "chart:miner-address-distribution",
        "chart:total-supply",
        "chart:secondary-issuance",
    ];
    for key in STUB_CHART_KEYS {
        state.cache.delete(key).await;
    }

    macro_rules! run_warmup {
        ($key:expr, $fn:ident) => {
            async {
                if state.cache.get::<serde_json::Value>($key).await.is_none() {
                    match $fn(&state).await {
                        Ok(_) => info!("Warmed up cache: {}", $key),
                        Err(e) => tracing::warn!("Failed to warmup {}: {}", $key, e),
                    }
                }
            }
        };
    }

    tokio::join!(
        run_warmup!("chart:average-block-time", warmup_average_block_time),
        run_warmup!("chart:hash-rate", warmup_hash_rate),
        run_warmup!("chart:difficulty", warmup_difficulty),
        run_warmup!("chart:uncle-rate", warmup_uncle_rate),
        run_warmup!(
            "chart:block-time-distribution:v2",
            warmup_block_time_distribution
        ),
        run_warmup!(
            "chart:epoch-time-distribution",
            warmup_epoch_time_distribution
        ),
        run_warmup!("chart:epoch-time-length", warmup_epoch_time_length),
        run_warmup!(
            "chart:miner-address-distribution",
            warmup_miner_distribution
        ),
        run_warmup!("chart:total-supply", warmup_total_supply),
        run_warmup!("chart:secondary-issuance", warmup_secondary_issuance),
    );

    info!("Cache warmup completed");
}

async fn warmup_average_block_time(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:average-block-time").await;
    Ok(())
}

async fn warmup_hash_rate(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:hash-rate").await;
    Ok(())
}

async fn warmup_difficulty(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:difficulty").await;
    Ok(())
}

async fn warmup_uncle_rate(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:uncle-rate").await;
    Ok(())
}

async fn warmup_block_time_distribution(state: &AppState) -> Result<(), String> {
    let response = build_block_time_distribution_response(state.store.as_ref())?;

    state
        .cache
        .set(
            "chart:block-time-distribution:v2",
            &response,
            CHART_CACHE_TTL,
        )
        .await;

    Ok(())
}

async fn warmup_epoch_time_distribution(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:epoch-time-distribution").await;
    Ok(())
}

async fn warmup_epoch_time_length(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:epoch-time-length").await;
    Ok(())
}

async fn warmup_miner_distribution(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:miner-address-distribution").await;
    Ok(())
}

async fn warmup_total_supply(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:total-supply").await;
    Ok(())
}

async fn warmup_secondary_issuance(state: &AppState) -> Result<(), String> {
    state.cache.delete("chart:secondary-issuance").await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;

    fn compact_to_difficulty(compact: i64) -> u64 {
        let difficulty = ckb_compact_to_difficulty(compact as u32);
        difficulty.to_string().parse::<u64>().unwrap_or(u64::MAX)
    }

    #[test]
    fn test_compact_to_difficulty_genesis() {
        let difficulty = compact_to_difficulty(0x20010000);
        assert_eq!(difficulty, 256);
    }

    #[test]
    fn test_compact_to_difficulty_higher() {
        let d1 = compact_to_difficulty(0x20010000);
        let d2 = compact_to_difficulty(0x20008000);
        assert_eq!(d2, d1 * 2);
    }

    #[test]
    fn test_compact_to_difficulty_zero_mantissa() {
        assert_eq!(compact_to_difficulty(0x20000000), 0);
    }

    #[test]
    fn test_compact_to_difficulty_lower_exponent() {
        let d_high_exp = compact_to_difficulty(0x20010000);
        let d_low_exp = compact_to_difficulty(0x1f010000);
        assert!(d_low_exp > d_high_exp);
        assert_eq!(d_low_exp, d_high_exp * 256);
    }
}
