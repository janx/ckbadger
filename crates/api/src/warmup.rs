use crate::cache::CacheTtl;
use crate::routes::assets::AssetResponse;
use crate::utils::shannon_to_ckb_u128;
use crate::AppState;
use ckbadger_common::dao::GENESIS_BURNT;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);

// Cache keys for assets
pub const CACHE_KEY_ASSETS_TOKEN: &str = "assets:token";
pub const CACHE_KEY_ASSETS_DOB: &str = "assets:dob";
pub const CACHE_KEY_ASSETS_NFT: &str = "assets:nft";

/// Cached asset entry — pre-computed and sorted, ready for API serving.
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
    pub content_type: Option<String>,
    pub content_size: Option<i32>,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
    // Token-specific fields (None for DOB/NFT entries)
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
        }
    }
}

/// Background loop that refreshes the assets cache every 30 seconds.
pub async fn refresh_assets_cache_loop(state: Arc<AppState>) {
    // Small initial delay so the API can start serving immediately
    tokio::time::sleep(Duration::from_secs(2)).await;

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
fn refresh_assets_cache_sync(state: &AppState) -> anyhow::Result<()> {
    let ttl = CacheTtl::ASSETS;
    let now_ms = chrono::Utc::now().timestamp_millis();

    // -- Token assets --
    let tokens = state.store.list_tokens()?;
    let mut token_assets: Vec<CachedAssetEntry> = Vec::with_capacity(tokens.len());

    for (hash, info) in &tokens {
        let transfers_count = info.transfers_count;
        let transfers_24h = state
            .store
            .get_token_24h_transfers(hash, now_ms)
            .unwrap_or(0);

        token_assets.push(CachedAssetEntry {
            id: format!("0x{}", hex::encode(hash)),
            asset_type: "token".to_string(),
            standard: info.standard.clone(),
            name: info.name.clone(),
            symbol: info.symbol.clone(),
            icon_url: info.icon_url.clone(),
            holders_count: info.holders_count,
            transfers_count,
            transfers_24h,
            decimals: info.decimals.map(|d| d as i16),
            total_supply: info.total_supply.map(|s| s.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
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

    // -- DOB (Spore) assets --
    let spores = state.store.list_spores(10_000)?;

    struct ClusterAgg {
        count: i64,
        owners: HashSet<Vec<u8>>,
    }

    let mut cluster_map: HashMap<Vec<u8>, ClusterAgg> = HashMap::new();

    for (id, entry) in &spores {
        if entry.standard.is_cluster() {
            continue;
        }
        let cluster_id_bytes = entry.collection_id.clone().unwrap_or_else(|| id.clone());
        let agg = cluster_map
            .entry(cluster_id_bytes)
            .or_insert_with(|| ClusterAgg {
                count: 0,
                owners: HashSet::new(),
            });
        agg.count += 1;
        if entry.is_live {
            if let Some(ref owner) = entry.owner_lock_hash {
                agg.owners.insert(owner.clone());
            }
        }
    }

    let mut dob_assets: Vec<CachedAssetEntry> = Vec::new();

    for (cluster_id_bytes, agg) in &cluster_map {
        let cluster_hex = format!("0x{}", hex::encode(cluster_id_bytes));
        let cluster_entry = state.store.get_spore(cluster_id_bytes).ok().flatten();
        let name = cluster_entry.as_ref().and_then(|e| e.name.clone());

        dob_assets.push(CachedAssetEntry {
            id: cluster_hex.clone(),
            asset_type: "dob".to_string(),
            standard: "spore".to_string(),
            name: name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.owners.len() as i64,
            transfers_count: agg.count,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(agg.count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(cluster_hex),
            cluster_name: name,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    dob_assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
    });

    state.mem_cache.set(CACHE_KEY_ASSETS_DOB, &dob_assets, ttl);

    // -- NFT assets --
    let nfts = state.store.list_nfts(10_000)?;

    let mut collection_map: HashMap<
        String,
        (Option<String>, i64, bool, ckbadger_store::NftStandard),
    > = HashMap::new();

    for (id, entry) in &nfts {
        let collection_hex = entry
            .collection_id
            .as_ref()
            .map(|c| format!("0x{}", hex::encode(c)))
            .unwrap_or_else(|| format!("0x{}", hex::encode(id)));

        let counter = collection_map
            .entry(collection_hex)
            .or_insert_with(|| (entry.name.clone(), 0, entry.is_live, entry.standard));
        counter.1 += 1;
    }

    let mut nft_assets: Vec<CachedAssetEntry> = Vec::new();

    for (collection_hex, (name, count, is_live, standard)) in &collection_map {
        if !is_live {
            continue;
        }
        nft_assets.push(CachedAssetEntry {
            id: collection_hex.clone(),
            asset_type: "nft".to_string(),
            standard: standard.asset_standard().to_string(),
            name: name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: *count,
            transfers_count: *count,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: name.clone(),
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
            "chart:block-time-distribution",
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
    let stats = state.store.list_daily_stats().map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = stats
        .into_iter()
        .filter_map(|s| {
            let avg_time_ms = s.avg_block_time_ms?;
            // We don't have the date directly on DailyStats — use empty placeholder
            // This will be populated from the stats key prefix in production
            Some(serde_json::json!({
                "value": format!("{:.2}", avg_time_ms as f64 / 1000.0)
            }))
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Average Block Time",
        "yAxisLabel": "Seconds"
    });

    state
        .cache
        .set("chart:average-block-time", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_hash_rate(state: &AppState) -> Result<(), String> {
    // Hash rate requires daily_block_stats which have avg_compact_target
    // These are stored in the stats CF with DAILY_BLOCK prefix
    // For now, use an empty response — will be populated when data exists
    let response = serde_json::json!({
        "data": [],
        "title": "Hash Rate",
        "yAxisLabel": "Hash Rate (H/s)"
    });

    state
        .cache
        .set("chart:hash-rate", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_difficulty(state: &AppState) -> Result<(), String> {
    let response = serde_json::json!({
        "data": [],
        "title": "Difficulty",
        "yAxisLabel": "Difficulty"
    });

    state
        .cache
        .set("chart:difficulty", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_uncle_rate(state: &AppState) -> Result<(), String> {
    let response = serde_json::json!({
        "data": [],
        "title": "Uncle Rate",
        "yAxisLabel": "Uncle Rate"
    });

    state
        .cache
        .set("chart:uncle-rate", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_block_time_distribution(state: &AppState) -> Result<(), String> {
    // Block time distribution is stored as individual bucket entries
    // Iterate over all buckets
    let mut buckets = Vec::new();
    for bucket_ms in (0..60_000).step_by(500) {
        if let Ok(Some(count)) = state.store.get_block_time_dist(bucket_ms) {
            if count > 0 {
                buckets.push((bucket_ms, count as i64));
            }
        }
    }

    let total_blocks: i64 = buckets.iter().map(|(_, count)| count).sum();

    let data: Vec<serde_json::Value> = buckets
        .into_iter()
        .map(|(bucket_ms, count)| {
            let time_seconds = bucket_ms as f64 / 1000.0;
            let ratio = if total_blocks > 0 {
                (count as f64 / total_blocks as f64 * 100.0 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            serde_json::json!({
                "date": format!("{:.1}", time_seconds),
                "value": format!("{:.3}", ratio)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Block Time Distribution (Recent 50000 blocks)",
        "yAxisLabel": "Block Ratio (%)"
    });

    state
        .cache
        .set("chart:block-time-distribution", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_epoch_time_distribution(state: &AppState) -> Result<(), String> {
    let response = serde_json::json!({
        "data": [],
        "title": "Epoch Time Distribution",
        "yAxisLabel": "Epochs"
    });

    state
        .cache
        .set("chart:epoch-time-distribution", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_epoch_time_length(state: &AppState) -> Result<(), String> {
    let response = serde_json::json!({
        "data": [],
        "title": "Epoch Time Length",
        "yAxisLabel": "Hours",
        "y2AxisLabel": "Blocks"
    });

    state
        .cache
        .set("chart:epoch-time-length", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_miner_distribution(state: &AppState) -> Result<(), String> {
    let response = serde_json::json!({
        "data": [],
        "title": "Miner Address Distribution",
        "totalBlocks": 0
    });

    state
        .cache
        .set(
            "chart:miner-address-distribution",
            &response,
            CHART_CACHE_TTL,
        )
        .await;

    Ok(())
}

async fn warmup_total_supply(state: &AppState) -> Result<(), String> {
    let _ = GENESIS_BURNT;
    let _ = shannon_to_ckb_u128;

    let response = serde_json::json!({
        "data": [],
        "series": [
            {"key": "circulating", "label": "Circulating", "color": "#00c389"},
            {"key": "locked", "label": "Locked in DAO", "color": "#8b5cf6"},
            {"key": "burnt", "label": "Burnt", "color": "#6b7280"}
        ],
        "title": "Total Supply"
    });

    state
        .cache
        .set("chart:total-supply", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_secondary_issuance(state: &AppState) -> Result<(), String> {
    let response = serde_json::json!({
        "data": [],
        "series": [
            {"key": "compensation", "label": "Deposit Compensation", "color": "#00c389"},
            {"key": "mining", "label": "Mining Reward", "color": "#8b5cf6"},
            {"key": "burnt", "label": "Burnt", "color": "#6b7280"}
        ],
        "title": "Secondary Issuance"
    });

    state
        .cache
        .set("chart:secondary-issuance", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

#[cfg(test)]
mod tests {
    fn compact_to_difficulty(compact: i64) -> u64 {
        let compact = compact as u32;
        let exponent = ((compact >> 24) & 0xFF) as i32;
        let mantissa = (compact & 0x00FFFFFF) as f64;

        if mantissa == 0.0 {
            return 0;
        }

        const GENESIS_EXP: i32 = 0x20;
        const GENESIS_MAN: f64 = 0x01_0000 as f64;

        let exp_diff = GENESIS_EXP - exponent;
        let difficulty = if exp_diff >= 0 {
            (GENESIS_MAN / mantissa) * (256.0_f64).powi(exp_diff)
        } else {
            (GENESIS_MAN / mantissa) / (256.0_f64).powi(-exp_diff)
        };

        difficulty as u64
    }

    #[test]
    fn test_compact_to_difficulty_genesis() {
        let difficulty = compact_to_difficulty(0x20010000);
        assert_eq!(difficulty, 1);
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
