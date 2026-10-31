use crate::{
    asset_catalog::AssetCatalog,
    response::{ok, ApiError, ApiResult},
    AppState,
};
use anyhow::{ensure, Context};
use axum::extract::State;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Summary {
    source: &'static str,
    anchor: Option<Anchor>,
    counts: Counts,
    /// Catalog tokens per stored standard (IV-03), sorted by standard.
    token_standards: Vec<TokenStandardCount>,
    object_standards: Vec<StandardCount>,
    object_composition: Vec<CompositionCount>,
    fully_onchain_objects_count: i64,
    pending_items_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Anchor {
    block_number: i64,
    block_hash: String,
}
#[derive(Serialize)]
struct Counts {
    token: i64,
    object: i64,
    identity: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenStandardCount {
    standard: String,
    count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StandardCount {
    standard: String,
    collections_count: i64,
    live_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompositionCount {
    tier: String,
    collections_count: i64,
    live_count: i64,
}

pub(super) async fn get(State(state): State<Arc<AppState>>) -> ApiResult<Summary> {
    let summary = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        // The HTTP middleware pins this complete build, unlike background cache
        // refresh. Only catalog membership is read, not every token's history.
        let catalog = AssetCatalog::load(&state.store)?;
        let anchor = state
            .store
            .get_sync_tip_block()?
            .map(|(block_number, header)| Anchor {
                block_number,
                block_hash: format!("0x{}", hex::encode(header.hash)),
            });
        let mut counts = Counts {
            token: i64::try_from(catalog.tokens.len())?,
            object: 0,
            identity: 0,
        };
        // The catalog membership `counts.token` counts, grouped by the stored
        // standard `/assets?type=token&standard=` filters on (ASCII case
        // folded there), so each chip equals its list's total.
        let mut token_standards = BTreeMap::<String, i64>::new();
        for (hash, info) in &catalog.tokens {
            ensure!(
                !info.standard.trim().is_empty(),
                "catalog token 0x{} has an empty standard",
                hex::encode(hash)
            );
            if let Some(other) = token_standards
                .keys()
                .find(|known| known.eq_ignore_ascii_case(&info.standard) && **known != info.standard)
            {
                anyhow::bail!(
                    "catalog token 0x{} standard {:?} differs only in case from {other:?}; the token list filter folds case",
                    hex::encode(hash),
                    info.standard
                );
            }
            let count = token_standards.entry(info.standard.clone()).or_default();
            *count = count.checked_add(1).context("token standard count overflow")?;
        }
        let standard_total = token_standards
            .values()
            .try_fold(0i64, |total, count| total.checked_add(*count))
            .context("token standard total overflow")?;
        ensure!(
            standard_total == counts.token,
            "token standard counts sum to {standard_total}, catalog has {} tokens",
            counts.token
        );
        let mut standards = BTreeMap::<String, (i64, i64)>::new();
        let mut tiers: BTreeMap<String, (i64, i64)> = [
            "pure_ckb",
            "btc_ckb",
            "decentralized_mixture",
            "centralized_mixture",
            "unknown",
            "decode_pending",
        ]
        .into_iter()
        .map(|tier| (tier.into(), (0, 0)))
        .collect();
        let mut onchain = 0i64;
        for collection in catalog.collections.values() {
            if collection.asset_type == "identity" {
                counts.identity = counts
                    .identity
                    .checked_add(1)
                    .context("identity count overflow")?;
                continue;
            }
            ensure!(
                collection.asset_type == "object",
                "unexpected catalog class {}",
                collection.asset_type
            );
            counts.object = counts
                .object
                .checked_add(1)
                .context("object count overflow")?;
            let standard = standards.entry(collection.standard.clone()).or_default();
            add(standard, collection.live_count)?;
            let tier = tiers
                .get_mut(&collection.tier)
                .with_context(|| format!("unexpected inventory tier {}", collection.tier))?;
            add(tier, collection.live_count)?;
            ensure!(
                collection.onchain_count <= collection.live_count,
                "onchain items exceed live count standard={} onchain={} live={}",
                collection.standard,
                collection.onchain_count,
                collection.live_count
            );
            onchain = onchain
                .checked_add(collection.onchain_count)
                .context("onchain item count overflow")?;
        }
        Ok(Summary {
            source: "canonical_asset_indexes",
            anchor,
            counts,
            token_standards: token_standards
                .into_iter()
                .map(|(standard, count)| TokenStandardCount { standard, count })
                .collect(),
            object_standards: standards
                .into_iter()
                .map(
                    |(standard, (collections_count, live_count))| StandardCount {
                        standard,
                        collections_count,
                        live_count,
                    },
                )
                .collect(),
            object_composition: tiers
                .into_iter()
                .map(|(tier, (collections_count, live_count))| CompositionCount {
                    tier,
                    collections_count,
                    live_count,
                })
                .collect(),
            fully_onchain_objects_count: onchain,
            pending_items_count: catalog.pending_items_count,
        })
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(summary)
}
fn add(count: &mut (i64, i64), live: i64) -> anyhow::Result<()> {
    count.0 = count
        .0
        .checked_add(1)
        .context("collection count overflow")?;
    count.1 = count
        .1
        .checked_add(live)
        .context("live item count overflow")?;
    Ok(())
}
