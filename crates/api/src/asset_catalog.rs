//! Inventory membership and collection classification shared by warmup and summary.
use crate::utils::{
    resolve_collection_standard, resolve_object_collection_composition_tier_override,
};
use anyhow::{ensure, Context};
use ckbadger_common::TokenBalance;
use ckbadger_store::{
    CkbadgerStore, ClusterAggregate, CompositionTier, IdentityCollectionAggregate,
    MnftCollectionAggregate, ObjectEntry, ObjectExtra, ObjectStandard, TokenInfo,
};
use std::collections::HashMap;

pub(crate) struct AssetCatalog {
    pub tokens: Vec<(Vec<u8>, TokenInfo)>,
    pub clusters: Vec<(Vec<u8>, ClusterAggregate)>,
    pub mnfts: Vec<(Vec<u8>, MnftCollectionAggregate)>,
    pub identities: Vec<(Vec<u8>, IdentityCollectionAggregate)>,
    pub collections: HashMap<Vec<u8>, Collection>,
    pub pending_items_count: i64,
}
pub(crate) struct Collection {
    pub asset_type: &'static str,
    pub standard: String,
    pub tier: String,
    pub live_count: i64,
    pub onchain_count: i64,
}

fn positive_holder_exists(store: &CkbadgerStore, hash: &[u8]) -> anyhow::Result<bool> {
    for item in store.prefix_iterator_cf(store.cf_token_holders(), hash) {
        let (key, value) = item?;
        if !key.starts_with(hash) {
            break;
        }
        ensure!(
            key.len() == 64,
            "asset catalog invalid holder key: token=0x{} key=0x{}",
            hex::encode(hash),
            hex::encode(&key)
        );
        let balance = TokenBalance::from_be_bytes(&value).with_context(|| {
            format!(
                "asset catalog invalid holder balance key=0x{}",
                hex::encode(&key)
            )
        })?;
        if !balance.is_zero() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn tier(btc: i64, pure: i64, decentralized: i64, centralized: i64, unknown: i64) -> String {
    if centralized > 0 {
        "centralized_mixture"
    } else if decentralized > 0 {
        "decentralized_mixture"
    } else if (btc > 0 || pure > 0) && unknown == 0 {
        if btc > 0 {
            "btc_ckb"
        } else {
            "pure_ckb"
        }
    } else {
        "unknown"
    }
    .into()
}

impl AssetCatalog {
    pub fn load(store: &CkbadgerStore) -> anyhow::Result<Self> {
        let mut tokens = Vec::new();
        for (hash, info) in store.list_tokens()? {
            if info.name.is_some() || info.symbol.is_some() || positive_holder_exists(store, &hash)?
            {
                tokens.push((hash, info));
            }
        }
        let mut clusters = store.list_cluster_aggregates()?;
        let mut mnfts = store.list_mnft_collection_aggregates()?;
        let mut identities = store.list_identity_collection_aggregates()?;
        clusters.retain(|(_, a)| a.total_count != 0);
        mnfts.retain(|(_, a)| a.total_count != 0);
        identities.retain(|(_, a)| a.total_count != 0);
        let mut pending = HashMap::<Vec<u8>, i64>::new();
        for item in store.iterator_cf(store.cf_spore_data(), rocksdb::IteratorMode::Start) {
            let (key, value) = item?;
            let entry: ObjectEntry = bincode::deserialize(&value).with_context(|| {
                format!("asset catalog spore decode id=0x{}", hex::encode(&key))
            })?;
            if let ObjectExtra::Spore { media_profile, .. } = &entry.extra {
                ensure!(
                    entry.standard == ObjectStandard::Spore,
                    "asset catalog spore standard/payload mismatch id=0x{}",
                    hex::encode(&key)
                );
                if entry.is_live && media_profile.tier == CompositionTier::DecodePending {
                    let collection = entry.collection_id.with_context(|| {
                        format!(
                            "pending spore missing collection id=0x{}",
                            hex::encode(&key)
                        )
                    })?;
                    let n = pending.entry(collection).or_default();
                    *n = n.checked_add(1).context("pending item count overflow")?;
                }
            }
        }
        let mut pending_items_count = 0i64;
        let mut collections = HashMap::new();
        for (id, a) in &clusters {
            validate_counts(
                id,
                a.total_count,
                a.live_count,
                &[
                    a.btc_ckb_count,
                    a.pure_ckb_count,
                    a.decentralized_mixture_count,
                    a.centralized_mixture_count,
                    a.unknown_count,
                ],
            )?;
            let count = pending.remove(id).unwrap_or(0);
            ensure!(count<=a.unknown_count && count<=a.live_count,"pending composition exceeds aggregate: collection=0x{} pending={count} unknown={} live={}",hex::encode(id),a.unknown_count,a.live_count);
            pending_items_count = pending_items_count
                .checked_add(count)
                .context("pending item total overflow")?;
            let tier = if count > 0 && count == a.live_count {
                "decode_pending".into()
            } else {
                tier(
                    a.btc_ckb_count,
                    a.pure_ckb_count,
                    a.decentralized_mixture_count,
                    a.centralized_mixture_count,
                    a.unknown_count,
                )
            };
            let onchain_count = a
                .btc_ckb_count
                .checked_add(a.pure_ckb_count)
                .context("collection onchain count overflow")?;
            collections.insert(
                id.clone(),
                Collection {
                    asset_type: "object",
                    standard: "spore".into(),
                    tier,
                    live_count: a.live_count,
                    onchain_count,
                },
            );
        }
        ensure!(
            pending.is_empty(),
            "pending spores reference missing collection aggregates: ids={:?}",
            pending.keys().map(hex::encode).collect::<Vec<_>>()
        );
        for (id, a) in &mnfts {
            validate_counts(
                id,
                a.total_count,
                a.live_count,
                &[
                    a.btc_ckb_count,
                    a.pure_ckb_count,
                    a.decentralized_mixture_count,
                    a.centralized_mixture_count,
                    a.unknown_count,
                ],
            )?;
            let standard = resolve_collection_standard(id, a.standard.asset_standard());
            let has_tiers = a.pure_ckb_count > 0
                || a.btc_ckb_count > 0
                || a.decentralized_mixture_count > 0
                || a.centralized_mixture_count > 0;
            let tier = if has_tiers {
                tier(
                    a.btc_ckb_count,
                    a.pure_ckb_count,
                    a.decentralized_mixture_count,
                    a.centralized_mixture_count,
                    a.unknown_count,
                )
            } else {
                resolve_object_collection_composition_tier_override(&standard)
                    .unwrap_or("unknown")
                    .into()
            };
            let onchain_count = if has_tiers {
                a.pure_ckb_count
                    .checked_add(a.btc_ckb_count)
                    .context("mNFT onchain count overflow")?
            } else if matches!(tier.as_str(), "pure_ckb" | "btc_ckb") {
                a.live_count
            } else {
                0
            };
            let asset_type = if matches!(standard.as_str(), "dotbit" | "did_ckb") {
                "identity"
            } else {
                "object"
            };
            ensure!(
                collections
                    .insert(
                        id.clone(),
                        Collection {
                            asset_type,
                            standard,
                            tier,
                            live_count: a.live_count,
                            onchain_count
                        }
                    )
                    .is_none(),
                "duplicate asset collection id=0x{}",
                hex::encode(id)
            );
        }
        for (id, a) in &identities {
            validate_counts(id, a.total_count, a.live_count, &[])?;
            let standard = resolve_collection_standard(id, a.standard.asset_standard());
            let tier = resolve_object_collection_composition_tier_override(&standard)
                .unwrap_or("unknown")
                .to_string();
            let onchain_count = if matches!(tier.as_str(), "pure_ckb" | "btc_ckb") {
                a.live_count
            } else {
                0
            };
            ensure!(
                collections
                    .insert(
                        id.clone(),
                        Collection {
                            asset_type: "identity",
                            standard,
                            tier,
                            live_count: a.live_count,
                            onchain_count
                        }
                    )
                    .is_none(),
                "duplicate identity collection id=0x{}",
                hex::encode(id)
            );
        }
        Ok(Self {
            tokens,
            clusters,
            mnfts,
            identities,
            collections,
            pending_items_count,
        })
    }
}
fn validate_counts(id: &[u8], total: i64, live: i64, tiers: &[i64]) -> anyhow::Result<()> {
    ensure!(
        total >= 0 && live >= 0 && live <= total && tiers.iter().all(|n| *n >= 0 && *n <= live),
        "invalid inventory aggregate collection=0x{} total={total} live={live} tiers={tiers:?}",
        hex::encode(id)
    );
    let classified = tiers.iter().try_fold(0i64, |sum, count| {
        sum.checked_add(*count).with_context(|| {
            format!(
                "inventory composition count overflow collection=0x{}",
                hex::encode(id)
            )
        })
    })?;
    ensure!(
        classified <= live,
        "inventory composition counts exceed live: collection=0x{} classified={classified} live={live}",
        hex::encode(id)
    );
    Ok(())
}
