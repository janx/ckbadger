//! `collectionContext`: what one transaction did to the items of ONE
//! collection (OB-C13).
//!
//! A collection feed row is a whole transaction, and a transaction can touch
//! several collections at once (block 18,883,769 moved twelve Nervape spores,
//! burned a thirteenth and minted another cluster's spore). The row's label
//! must come from this collection's items only, so the API computes it here,
//! once per row, and the frontend displays it.
//!
//! Each item's change is the item feed's own meaning
//! ([`assets::nft_item_tx_meaning`]) — never a second classification. Which
//! items of the collection the transaction touched comes from two facts the
//! indexer persisted: the participants' item deltas (Layer 2: arrivals and
//! departures) and the item outpoint index of the transaction's outputs (the
//! item cells it created, which also finds a cell rewritten for its holder,
//! the one change Layer 2 does not record).

use std::collections::{BTreeMap, HashMap};

use ckbadger_store::{
    types::{identity_sentinel_standard, ItemKind},
    AssetAction, CkbadgerStore, IdentityStandard, TxActions,
};
use serde::Serialize;

use super::assets::{self, NftLifecycleStandard, ObjectCollectionSource};
use crate::response::{ApiError, ApiRouteError};

/// Bytes an m-NFT token id adds to its class id (`type_args` = class id (24
/// bytes) + token index (4 bytes)).
const MNFT_TOKEN_INDEX_LEN: usize = 4;

/// Which items belong to a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CollectionMembers {
    /// Spores whose `collection_id` is this cluster (the Sole Spores sentinel
    /// for clusterless spores).
    SporeCluster(Vec<u8>),
    /// m-NFT tokens whose id is this class id plus a token index.
    MnftClass(Vec<u8>),
    /// Identities of one standard.
    Identity(IdentityStandard),
}

impl CollectionMembers {
    pub(super) fn of(source: ObjectCollectionSource, id: &[u8]) -> Result<Self, ApiRouteError> {
        Ok(match source {
            ObjectCollectionSource::SporeCluster => Self::SporeCluster(id.to_vec()),
            ObjectCollectionSource::Mnft => Self::MnftClass(id.to_vec()),
            ObjectCollectionSource::Identity => {
                Self::Identity(identity_sentinel_standard(id).ok_or_else(|| {
                    ApiError::internal(format!(
                        "identity collection 0x{} is not an identity sentinel",
                        hex::encode(id)
                    ))
                })?)
            }
        })
    }

    fn lifecycle_standard(&self) -> NftLifecycleStandard {
        match self {
            Self::SporeCluster(_) => NftLifecycleStandard::Spore,
            Self::MnftClass(_) => NftLifecycleStandard::MnftToken,
            Self::Identity(IdentityStandard::DotBit) => NftLifecycleStandard::DotBit,
            Self::Identity(IdentityStandard::BitCell) => NftLifecycleStandard::BitCell,
            Self::Identity(IdentityStandard::DidCkb) => NftLifecycleStandard::DidCkb,
            Self::Identity(IdentityStandard::DotCell) => NftLifecycleStandard::DotCell,
        }
    }

    /// Whether an item delta of `kind` can be an item of this collection.
    fn delta_kind_matches(&self, kind: ItemKind) -> bool {
        match self {
            Self::SporeCluster(_) | Self::MnftClass(_) => kind == ItemKind::Object,
            Self::Identity(standard) => kind == ItemKind::Identity(*standard),
        }
    }

    /// The item id held by each output of `tx_hash` that carries one of this
    /// collection's item standard, by output index.
    fn created_items(
        &self,
        store: &CkbadgerStore,
        tx_hash: &[u8],
        outputs_count: i16,
    ) -> anyhow::Result<Vec<(i16, Vec<u8>)>> {
        let outpoints: Vec<(&[u8], i16)> = (0..outputs_count).map(|i| (tx_hash, i)).collect();
        let found = match self {
            Self::MnftClass(_) => store.get_mnft_token_ids_by_outpoints_batch(&outpoints)?,
            Self::Identity(IdentityStandard::DotBit) => {
                store.get_dotbit_account_ids_by_outpoints_batch(&outpoints)?
            }
            Self::SporeCluster(_) | Self::Identity(_) => {
                store.get_spore_ids_by_outpoints_batch(&outpoints)?
            }
        };
        Ok(found
            .into_iter()
            .map(|(_, output, id)| (output, id))
            .collect())
    }

    /// The subset of `candidates` that are items of this collection.
    fn members(
        &self,
        store: &CkbadgerStore,
        candidates: Vec<Vec<u8>>,
        known_identities: &[Vec<u8>],
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        match self {
            Self::SporeCluster(cluster) => Ok(store
                .get_spores_batch(&candidates)?
                .into_iter()
                .filter(|(_, entry)| {
                    entry
                        .as_ref()
                        .is_some_and(|entry| entry.collection_id.as_deref() == Some(cluster))
                })
                .map(|(id, _)| id)
                .collect()),
            Self::MnftClass(class) => Ok(candidates
                .into_iter()
                .filter(|id| {
                    id.len() == class.len() + MNFT_TOKEN_INDEX_LEN && id.starts_with(class)
                })
                .collect()),
            // The `.bit` outpoint index holds only `.bit` accounts.
            Self::Identity(IdentityStandard::DotBit) => Ok(candidates),
            // The spore outpoint index is shared by spores and the other
            // identity standards: an id is this collection's when its identity
            // row has this standard (ids already typed by an item delta skip
            // the read).
            Self::Identity(standard) => {
                let mut out = Vec::new();
                for id in candidates {
                    if known_identities.contains(&id)
                        || store
                            .get_identity(&id)?
                            .is_some_and(|entry| entry.standard == *standard)
                    {
                        out.push(id);
                    }
                }
                Ok(out)
            }
        }
    }
}

/// One item of the collection and what the transaction did to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct CollectionItemChange {
    pub(super) id: String,
    pub(super) change: &'static str,
}

/// What one transaction did to the items of one collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct CollectionContext {
    /// The single non-zero kind (`mint`, `burn`, `transfer`, `update`), or
    /// `mixed` when more than one kind is non-zero.
    pub(super) action: &'static str,
    pub(super) minted: u32,
    pub(super) burned: u32,
    pub(super) transferred: u32,
    pub(super) updated: u32,
    /// Only items of this collection, by item id.
    pub(super) items: Vec<CollectionItemChange>,
}

/// An item's change from the collection's side: the item feed's action,
/// in the collection vocabulary.
fn change_of(action: AssetAction) -> &'static str {
    match action {
        AssetAction::Mint => "minted",
        AssetAction::Burn | AssetAction::Recycle => "burned",
        AssetAction::Transfer => "transferred",
        AssetAction::Renew | AssetAction::Update => "updated",
    }
}

#[derive(Default)]
struct ItemFacts {
    arrived: u32,
    departed: u32,
    created_output: Option<i16>,
}

/// The `collectionContext` of one canonical transaction for one collection.
///
/// Fails when the facts contradict each other (an item arriving twice, an
/// arrival without a created cell, an item created twice) or when the
/// transaction — which the collection's activity index lists — touched no
/// item of the collection.
pub(super) fn collection_context(
    store: &CkbadgerStore,
    members: &CollectionMembers,
    actions: &TxActions,
    outputs_count: i16,
) -> Result<CollectionContext, ApiRouteError> {
    let tx = format!("0x{}", hex::encode(&actions.tx_hash));
    let internal =
        |message: String| ApiError::internal(format!("collection context tx={tx}: {message}"));
    let mut facts: BTreeMap<Vec<u8>, ItemFacts> = BTreeMap::new();
    let mut typed_identities = Vec::new();
    for participant in &actions.participants {
        for delta in &participant.item_deltas {
            if !members.delta_kind_matches(delta.kind) {
                continue;
            }
            if matches!(delta.kind, ItemKind::Identity(_)) {
                typed_identities.push(delta.item_id.clone());
            }
            let item = facts.entry(delta.item_id.clone()).or_default();
            if delta.negative {
                item.departed += 1;
            } else {
                item.arrived += 1;
            }
        }
    }
    for (output, id) in members
        .created_items(store, &actions.tx_hash, outputs_count)
        .map_err(|e| internal(format!("{e:#}")))?
    {
        let item = facts.entry(id.clone()).or_default();
        if let Some(first) = item.created_output.replace(output) {
            return Err(internal(format!(
                "item 0x{} created at outputs {first} and {output}",
                hex::encode(&id)
            )));
        }
    }
    let member_ids: HashMap<Vec<u8>, ()> = members
        .members(store, facts.keys().cloned().collect(), &typed_identities)
        .map_err(|e| internal(format!("{e:#}")))?
        .into_iter()
        .map(|id| (id, ()))
        .collect();

    let standard = members.lifecycle_standard();
    let mut context = CollectionContext {
        action: "mixed",
        minted: 0,
        burned: 0,
        transferred: 0,
        updated: 0,
        items: Vec::new(),
    };
    for (id, item) in facts {
        if !member_ids.contains_key(&id) {
            continue;
        }
        if item.arrived > 1 || item.departed > 1 {
            return Err(internal(format!(
                "item 0x{} arrived {} and departed {} times",
                hex::encode(&id),
                item.arrived,
                item.departed
            )));
        }
        if item.arrived == 1 && item.created_output.is_none() {
            return Err(internal(format!(
                "item 0x{} arrived without a created cell",
                hex::encode(&id)
            )));
        }
        // A departure consumed the item's cell; so did a rewrite: a created
        // cell no holder gained (a mint always records its arrival).
        let consumed = item.departed == 1 || (item.created_output.is_some() && item.arrived == 0);
        let meaning = assets::nft_item_tx_meaning(
            store,
            standard,
            &id,
            &actions.tx_hash,
            item.created_output,
            consumed,
            &actions.protocol_actions,
        )?;
        let change = change_of(meaning.action);
        match change {
            "minted" => context.minted += 1,
            "burned" => context.burned += 1,
            "transferred" => context.transferred += 1,
            _ => context.updated += 1,
        }
        context.items.push(CollectionItemChange {
            id: format!("0x{}", hex::encode(&id)),
            change,
        });
    }
    let kinds = [
        ("mint", context.minted),
        ("burn", context.burned),
        ("transfer", context.transferred),
        ("update", context.updated),
    ];
    let mut non_zero = kinds.iter().filter(|(_, count)| *count > 0);
    context.action = match (non_zero.next(), non_zero.next()) {
        (Some((kind, _)), None) => kind,
        (Some(_), Some(_)) => "mixed",
        (None, _) => return Err(internal(
            "the collection's activity index lists it, but it touched no item of the collection"
                .to_string(),
        )),
    };
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;

    #[test]
    fn collection_change_vocabulary_covers_every_item_action() {
        assert_eq!(change_of(AssetAction::Mint), "minted");
        assert_eq!(change_of(AssetAction::Burn), "burned");
        assert_eq!(change_of(AssetAction::Recycle), "burned");
        assert_eq!(change_of(AssetAction::Transfer), "transferred");
        assert_eq!(change_of(AssetAction::Renew), "updated");
        assert_eq!(change_of(AssetAction::Update), "updated");
    }

    #[test]
    fn sole_spores_is_a_cluster_membership() {
        assert_eq!(
            CollectionMembers::of(
                ObjectCollectionSource::SporeCluster,
                &SOLE_SPORES_SENTINEL_COLLECTION
            )
            .unwrap(),
            CollectionMembers::SporeCluster(SOLE_SPORES_SENTINEL_COLLECTION.to_vec())
        );
    }
}
