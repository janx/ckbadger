use anyhow::{anyhow, bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use tracing::warn;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AssetAction, IdentityCollectionAggregate, IdentityEntry, IdentityExtra, IdentityStandard,
    ObjectCollectionActivityEntry,
};
use ckbadger_store::CkbadgerStore;

use crate::parser::dotbit::ParsedDotbitAccountOutput;

use super::entity_stats::SharedEntityStatsOverlay;

use super::BatchWriter;

use ckbadger_store::types::DOTBIT_SENTINEL_COLLECTION;

/// Classification of a DAS action: mapped to an AssetAction, known but
/// suppressed (no collection activity), or truly unknown.
#[derive(Debug)]
pub(crate) enum DasActionKind {
    Mapped(AssetAction),
    Suppressed,
    Unknown,
}

/// Classify a DAS action string.
///
/// Full DAS action catalogue sourced from:
/// - `dotbitHQ/did-contracts` libs/das-types/rust/src/constants.rs (Action enum)
/// - `dotbitHQ/das-lib` common/action.go (includes legacy actions)
pub(crate) fn classify_das_action(action: &str) -> DasActionKind {
    match action {
        // Registration
        "confirm_proposal" => DasActionKind::Mapped(AssetAction::Mint),
        // Transfers
        "transfer_account"
        | "buy_account"
        | "accept_offer"
        | "fulfill_approval"
        | "bid_expired_account_dutch_auction"
        // Pre-rename spelling of the above, still present in testnet history.
        | "bid_expired_account_auction"
        | "sell_account" => DasActionKind::Mapped(AssetAction::Transfer),
        // Recycle
        "recycle_expired_account" => DasActionKind::Mapped(AssetAction::Recycle),
        // Renew
        "renew_account" => DasActionKind::Mapped(AssetAction::Renew),
        // Account updates
        "edit_records"
        | "edit_manager"
        | "start_account_sale"
        | "cancel_account_sale"
        | "edit_account_sale"
        | "force_recover_account_status"
        | "lock_account_for_cross_chain"
        | "unlock_account_for_cross_chain"
        | "create_approval"
        | "delay_approval"
        | "revoke_approval"
        | "upgrade_did"
        | "account_cell_upgrade" => DasActionKind::Mapped(AssetAction::Update),
        // Sub-account infrastructure — suppress collection activity
        "enable_sub_account"
        | "create_sub_account"
        | "edit_sub_account"
        | "renew_sub_account"
        | "recycle_sub_account"
        | "update_sub_account"
        | "config_sub_account_custom_script"
        | "config_sub_account"
        | "collect_sub_account_profit"
        | "collect_sub_account_channel_profit"
        | "lock_sub_account_for_cross_chain"
        | "unlock_sub_account_for_cross_chain" => DasActionKind::Suppressed,
        // Registration infrastructure
        "apply_register"
        | "refund_apply"
        | "refund_pay"
        | "pre_register"
        | "refund_pre_register"
        | "propose"
        | "extend_proposal"
        | "recycle_proposal"
        | "config"
        | "deploy"
        | "init_account_chain" => DasActionKind::Suppressed,
        // Order / payment refunds
        "order_refund" | "cross_refund" => DasActionKind::Suppressed,
        // Offers — OfferCell, not AccountCell
        "make_offer" | "edit_offer" | "cancel_offer" => DasActionKind::Suppressed,
        // Income / Balance
        "create_income"
        | "consolidate_income"
        | "transfer"
        | "transfer_balance"
        | "withdraw_from_wallet" => DasActionKind::Suppressed,
        // DPoint
        "mint_dp" | "transfer_dp" | "burn_dp" => DasActionKind::Suppressed,
        // Reverse records
        "retract_reverse_record"
        | "create_reverse_record_root"
        | "update_reverse_record_root"
        | "declare_reverse_record"
        | "redeclare_reverse_record" => DasActionKind::Suppressed,
        // Device key list
        "create_device_key_list" | "update_device_key_list" | "destroy_device_key_list" => {
            DasActionKind::Suppressed
        }
        // Legacy actions from testnet-only contract versions that predate the
        // catalogues above. None carries an AccountCell on any output —
        // `quote_price_for_ckb` updates the CKB price oracle (QuoteCell),
        // `create_wallet` creates the WalletCell that `withdraw_from_wallet`
        // later drains, and `balance_deposit` funds a BalanceCell — so none
        // produces .bit collection activity.
        "quote_price_for_ckb" | "create_wallet" | "balance_deposit" => DasActionKind::Suppressed,
        _ => DasActionKind::Unknown,
    }
}

/// Only the pre-consumption fields needed for an item's output action. This
/// deliberately leaves the full identity (name, transaction and extra data)
/// in the owning state/undo record rather than cloning it for classification.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IdentityActionState {
    owner_lock_hash: Option<[u8; 32]>,
    expired_at: Option<u64>,
    standard: IdentityStandard,
    created_at_block: i64,
}

impl IdentityActionState {
    pub(crate) fn from_entry(entry: &IdentityEntry) -> Result<Self> {
        let owner_lock_hash = entry
            .owner_lock_hash
            .as_deref()
            .map(|owner| {
                owner.try_into().map_err(|_| {
                    anyhow!(
                        "identity action invalid prior owner width: standard={} created_at_block={} expected=32 actual={}",
                        entry.standard.as_str(),
                        entry.created_at_block,
                        owner.len()
                    )
                })
            })
            .transpose()?;
        let expired_at = match entry.extra {
            IdentityExtra::DotBit { expired_at, .. } => expired_at,
            IdentityExtra::BitCell { expired_at, .. } => Some(expired_at),
            _ => None,
        };
        Ok(Self {
            owner_lock_hash,
            expired_at,
            standard: entry.standard,
            created_at_block: entry.created_at_block,
        })
    }

    /// The consumed input is authoritative for this transaction. A dead
    /// historical entry with no matching input is a new mint, not an update.
    pub(crate) fn for_output(
        consumed: Option<Self>,
        existing: Option<&IdentityEntry>,
    ) -> Result<Option<Self>> {
        match consumed {
            Some(previous) => Ok(Some(previous)),
            None => existing
                .filter(|entry| entry.is_live)
                .map(Self::from_entry)
                .transpose(),
        }
    }
}

/// Per-item action, shared by bulk and live. Transaction-wide ActionData
/// cannot turn an unchanged neighbour into a mint, transfer or recycle;
/// classify from owner/expiry chain facts only. Collection activity handles
/// Unknown DAS ActionData separately (warn + no invented Mint/Recycle).
pub(crate) fn classify_identity_output_action(
    previous: Option<&IdentityActionState>,
    next_owner: &[u8],
    next_expiry: Option<u64>,
) -> Result<AssetAction> {
    let Some(previous) = previous else {
        return Ok(AssetAction::Mint);
    };
    let owner = previous.owner_lock_hash.as_ref().ok_or_else(|| {
        anyhow!(
            "identity action missing prior owner: standard={} created_at_block={}",
            previous.standard.as_str(),
            previous.created_at_block
        )
    })?;
    if owner != next_owner {
        return Ok(AssetAction::Transfer);
    }
    if previous
        .expired_at
        .zip(next_expiry)
        .is_some_and(|(old, new)| new > old)
    {
        return Ok(AssetAction::Renew);
    }
    Ok(AssetAction::Update)
}

/// Convenience: extract the mapped AssetAction (if any).
pub(crate) fn das_action_to_asset_action(action: &str) -> Option<AssetAction> {
    match classify_das_action(action) {
        DasActionKind::Mapped(a) => Some(a),
        _ => None,
    }
}

/// Resolve .bit collection activity for a single transaction.
///
/// Uses the parsed DAS action to determine the correct `AssetAction`, with
/// neighbor suppression: for `confirm_proposal`, only new accounts (in
/// outputs but NOT inputs) get Mint; for `recycle_expired_account`, only
/// removed accounts (in inputs but NOT outputs) get Recycle.
pub(crate) fn build_dotbit_tx_activity_entry<S: BuildHasher>(
    das_action: Option<&str>,
    created_account_ids: &HashSet<Vec<u8>, S>,
    consumed_account_ids: &HashSet<Vec<u8>, S>,
    tx_hash: &[u8],
    block_hash: &[u8],
    timestamp_ms: i64,
) -> Option<ObjectCollectionActivityEntry> {
    let actions = match das_action.and_then(das_action_to_asset_action) {
        Some(asset_action) => match asset_action {
            AssetAction::Mint => {
                let new_only: Vec<_> = created_account_ids
                    .iter()
                    .filter(|id| !consumed_account_ids.contains(*id))
                    .collect();
                if new_only.is_empty() {
                    return None;
                }
                vec![AssetAction::Mint]
            }
            AssetAction::Recycle => {
                let removed_only: Vec<_> = consumed_account_ids
                    .iter()
                    .filter(|id| !created_account_ids.contains(*id))
                    .collect();
                if removed_only.is_empty() {
                    return None;
                }
                vec![AssetAction::Recycle]
            }
            action => vec![action],
        },
        None if das_action.is_some() => {
            let action_str = das_action.unwrap_or("");
            if matches!(classify_das_action(action_str), DasActionKind::Unknown)
                && !action_str.is_empty()
            {
                warn!(
                    action = action_str,
                    tx_hash = %format!("0x{}", hex::encode(tx_hash)),
                    "unknown DAS action; skipping .bit collection activity"
                );
                return None;
            }
            resolve_generic_dotbit_actions(created_account_ids, consumed_account_ids)
        }
        None => resolve_generic_dotbit_actions(created_account_ids, consumed_account_ids),
    };

    if actions.is_empty() {
        return None;
    }

    Some(ObjectCollectionActivityEntry {
        tx_hash: tx_hash.to_vec(),
        block_hash: block_hash.to_vec(),
        timestamp_ms,
        actions,
    })
}

/// Write the `.bit` collection activity row of one transaction (if it has
/// one) and fold it into its `collection_activity_daily` day `day` under its
/// block.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_dotbit_tx_activity(
    das_action: Option<&str>,
    created_account_ids: &HashSet<Vec<u8>>,
    consumed_account_ids: &HashSet<Vec<u8>>,
    tx_hash: &[u8],
    block_hash: &[u8],
    block_number: i64,
    tx_idx: i32,
    timestamp_ms: i64,
    day: u32,
    batch: &mut StoreBatch,
    daily: &mut ckbadger_store::CollectionActivityDailyBlockDeltas,
) -> anyhow::Result<bool> {
    let Some(entry) = build_dotbit_tx_activity_entry(
        das_action,
        created_account_ids,
        consumed_account_ids,
        tx_hash,
        block_hash,
        timestamp_ms,
    ) else {
        return Ok(false);
    };

    batch.put_identity_collection_activity(
        &DOTBIT_SENTINEL_COLLECTION,
        block_number,
        tx_idx,
        &entry,
    );
    daily.add_row(
        block_number,
        &DOTBIT_SENTINEL_COLLECTION,
        day,
        &entry.actions,
    )?;
    Ok(true)
}

/// Generic fallback: same-account in both → Transfer, output-only → Mint, input-only → Burn.
fn resolve_generic_dotbit_actions<S: BuildHasher>(
    created: &HashSet<Vec<u8>, S>,
    consumed: &HashSet<Vec<u8>, S>,
) -> Vec<AssetAction> {
    let mut has_mint = false;
    let mut has_transfer = false;
    let mut has_burn = false;

    for id in created {
        if consumed.contains(id) {
            has_transfer = true;
        } else {
            has_mint = true;
        }
    }
    for id in consumed {
        if !created.contains(id) {
            has_burn = true;
        }
    }

    let mut actions = Vec::new();
    if has_mint {
        actions.push(AssetAction::Mint);
    }
    if has_transfer {
        actions.push(AssetAction::Transfer);
    }
    if has_burn {
        actions.push(AssetAction::Burn);
    }
    actions
}

/// Diagnostic context for dotbit identity owner transitions.
struct OwnerTransitionContext<'a> {
    operation: &'a str,
    account_id: &'a [u8],
    block_number: i64,
    tx_hash: &'a [u8],
    existing_entry: Option<&'a IdentityEntry>,
}

pub(crate) struct DotbitBatchState {
    accounts: HashMap<Vec<u8>, Option<IdentityEntry>>,
    consumed_for_actions: HashMap<(Vec<u8>, Vec<u8>), IdentityActionState>,
    stats: SharedEntityStatsOverlay,
    identity_aggs: HashMap<Vec<u8>, IdentityCollectionAggregate>,
    identity_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
}

impl DotbitBatchState {
    fn get_account(
        &mut self,
        store: &CkbadgerStore,
        account_id: &[u8],
    ) -> Result<Option<IdentityEntry>> {
        if let Some(cached) = self.accounts.get(account_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_identity(account_id)?;
        self.accounts.insert(account_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_account(&mut self, account_id: &[u8], entry: IdentityEntry) {
        self.accounts.insert(account_id.to_vec(), Some(entry));
    }

    fn get_identity_agg(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
    ) -> Result<IdentityCollectionAggregate> {
        if let Some(cached) = self.identity_aggs.get(collection_id) {
            return Ok(cached.clone());
        }
        let agg = store
            .get_identity_collection_aggregate(collection_id)?
            .unwrap_or_default();
        self.identity_aggs
            .insert(collection_id.to_vec(), agg.clone());
        Ok(agg)
    }

    fn put_identity_agg(
        &mut self,
        collection_id: &[u8],
        agg: IdentityCollectionAggregate,
        batch: &mut StoreBatch,
    ) {
        batch.put_identity_collection_aggregate(collection_id, &agg);
        self.identity_aggs.insert(collection_id.to_vec(), agg);
    }

    /// Returns `(count, from_cache)`.
    fn get_identity_owner_count(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
        lock_hash: &[u8],
    ) -> Result<(i64, bool)> {
        let key = (collection_id.to_vec(), lock_hash.to_vec());
        if let Some(cached) = self.identity_owner_counts.get(&key) {
            return Ok((*cached, true));
        }
        let loaded = store.get_identity_owner_count(collection_id, lock_hash)?;
        self.identity_owner_counts.insert(key, loaded);
        Ok((loaded, false))
    }

    fn put_identity_owner_count(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        count: i64,
        batch: &mut StoreBatch,
    ) {
        batch.put_identity_owner_count(collection_id, lock_hash, count);
        self.identity_owner_counts
            .insert((collection_id.to_vec(), lock_hash.to_vec()), count);
    }

    fn delete_identity_owner(
        &mut self,
        collection_id: &[u8],
        lock_hash: &[u8],
        batch: &mut StoreBatch,
    ) {
        batch.delete_identity_owner(collection_id, lock_hash);
        self.identity_owner_counts
            .insert((collection_id.to_vec(), lock_hash.to_vec()), 0);
    }

    pub(crate) fn pending_identity_aggs(&self) -> &HashMap<Vec<u8>, IdentityCollectionAggregate> {
        &self.identity_aggs
    }

    /// Add `rows` identity collection activity rows of `block` to the
    /// collection's `activities_count`, on top of the aggregate this batch
    /// left (else the committed one, which must exist), in the block's scope
    /// (`batch`).
    pub(crate) fn add_identity_collection_activities(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
        rows: i64,
        block: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let agg = super::object_activity_acc::identity_aggregate_with_activities(
            self.identity_aggs.get(collection_id),
            store,
            collection_id,
            rows,
            block,
        )?;
        self.put_identity_agg(collection_id, agg, batch);
        Ok(())
    }
}

impl BatchWriter {
    pub(crate) fn new_dotbit_batch_state(
        &self,
        stats: SharedEntityStatsOverlay,
    ) -> DotbitBatchState {
        DotbitBatchState {
            accounts: HashMap::new(),
            consumed_for_actions: HashMap::new(),
            stats,
            identity_aggs: HashMap::new(),
            identity_owner_counts: HashMap::new(),
        }
    }

    fn apply_dotbit_identity_owner_transition(
        &self,
        collection_id: &[u8],
        old_owner: Option<&[u8]>,
        new_owner: Option<&[u8]>,
        agg: &mut IdentityCollectionAggregate,
        batch: &mut StoreBatch,
        state: &mut DotbitBatchState,
        ctx: &OwnerTransitionContext<'_>,
    ) -> Result<()> {
        if old_owner == new_owner {
            return Ok(());
        }

        if let Some(old_lock) = old_owner {
            let (old_count, from_cache) =
                state.get_identity_owner_count(self.store.as_ref(), collection_id, old_lock)?;
            if old_count <= 0 {
                // Cross-check: read the raw DB value directly (bypassing cache)
                // to confirm whether the inconsistency is in the cache or DB.
                let raw_db_count = self
                    .store
                    .get_identity_owner_count(collection_id, old_lock)
                    .unwrap_or(-999);
                bail!(
                    "dotbit identity owner count underflow: \
                     operation={}, account_id=0x{}, block={}, tx=0x{}, \
                     lock_hash=0x{}, owner_count={}, count_from_cache={}, raw_db_count={}, \
                     existing_entry={{ is_live={}, owner=0x{}, created_at_block={} }}, \
                     new_owner={}",
                    ctx.operation,
                    hex::encode(ctx.account_id),
                    ctx.block_number,
                    hex::encode(ctx.tx_hash),
                    hex::encode(old_lock),
                    old_count,
                    from_cache,
                    raw_db_count,
                    ctx.existing_entry
                        .map(|e| e.is_live.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    ctx.existing_entry
                        .and_then(|e| e.owner_lock_hash.as_ref())
                        .map(hex::encode)
                        .unwrap_or_else(|| "none".to_string()),
                    ctx.existing_entry
                        .map(|e| e.created_at_block.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    new_owner
                        .map(|h| format!("0x{}", hex::encode(h)))
                        .unwrap_or_else(|| "None".to_string()),
                );
            } else if old_count == 1 {
                if agg.holders_count <= 0 {
                    bail!(
                        "dotbit identity aggregate holders_count underflow: \
                         operation={}, account_id=0x{}, block={}, tx=0x{}, \
                         collection_id=0x{}, holders_count={}",
                        ctx.operation,
                        hex::encode(ctx.account_id),
                        ctx.block_number,
                        hex::encode(ctx.tx_hash),
                        hex::encode(collection_id),
                        agg.holders_count
                    );
                }
                state.delete_identity_owner(collection_id, old_lock, batch);
                agg.holders_count -= 1;
            } else {
                state.put_identity_owner_count(collection_id, old_lock, old_count - 1, batch);
            }
        }

        if let Some(new_lock) = new_owner {
            let (cur_count, _) =
                state.get_identity_owner_count(self.store.as_ref(), collection_id, new_lock)?;
            if cur_count == 0 {
                agg.holders_count = agg
                    .holders_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("dotbit identity aggregate holders_count overflow"))?;
            }
            let next = cur_count
                .checked_add(1)
                .ok_or_else(|| anyhow!("dotbit identity owner count overflow"))?;
            state.put_identity_owner_count(collection_id, new_lock, next, batch);
        }

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn insert_dotbit_account_with_state(
        &self,
        account: &ParsedDotbitAccountOutput,
        tx_hash: &[u8],
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
        state: &mut DotbitBatchState,
    ) -> Result<()> {
        self.insert_dotbit_account_with_action(
            account,
            tx_hash,
            block_number,
            timestamp_ms,
            batch,
            state,
        )
    }

    pub(crate) fn insert_dotbit_account_with_action(
        &self,
        account_output: &ParsedDotbitAccountOutput,
        tx_hash: &[u8],
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
        state: &mut DotbitBatchState,
    ) -> Result<()> {
        let account = &account_output.account;
        let account_name = account
            .account
            .clone()
            .unwrap_or_else(|| format!("0x{}", hex::encode(&account.account_id)));
        let existing = state.get_account(self.store.as_ref(), &account.account_id)?;
        let consumed = state
            .consumed_for_actions
            .remove(&(account.account_id.clone(), tx_hash.to_vec()));
        let previous =
            IdentityActionState::for_output(consumed, existing.as_ref()).with_context(|| {
                format!(
                    ".bit action snapshot: item=0x{} block={block_number} tx=0x{}",
                    hex::encode(&account.account_id),
                    hex::encode(tx_hash)
                )
            })?;
        let action = classify_identity_output_action(
            previous.as_ref(),
            &account.owner_lock_hash,
            account.expired_at,
        )
        .map_err(|error| {
            anyhow!(
                ".bit item=0x{} block={block_number} tx=0x{}: {error}",
                hex::encode(&account.account_id),
                hex::encode(tx_hash)
            )
        })?;
        let was_live = existing.as_ref().is_some_and(|entry| entry.is_live);
        let old_owner = if was_live {
            existing
                .as_ref()
                .and_then(|entry| entry.owner_lock_hash.clone())
        } else {
            None
        };

        let entry = IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(account.owner_lock_hash.clone()),
            name: Some(account_name),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            created_at_tx: existing
                .as_ref()
                .map(|e| e.created_at_tx.clone())
                .unwrap_or_else(|| tx_hash.to_vec()),
            extra: IdentityExtra::DotBit {
                expired_at: account.expired_at,
                registered_at: account.registered_at,
                status: account.status,
            },
        };
        batch.put_identity(&account.account_id, &entry);
        state.put_account(&account.account_id, entry);

        // Update identity collection aggregate
        let cid = &DOTBIT_SENTINEL_COLLECTION;
        let mut agg = state.get_identity_agg(self.store.as_ref(), cid)?;
        if agg.standard == IdentityStandard::default() && agg.total_count == 0 {
            agg.standard = IdentityStandard::DotBit;
            agg.name = Some(".bit".to_string());
        }
        if existing.is_none() {
            // New identity — add to identity collection index
            batch.put_identity_by_collection(cid, &account.account_id);
            agg.total_count = agg.total_count.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "dotbit identity total_count overflow: account_id=0x{}",
                    hex::encode(&account.account_id)
                )
            })?;
            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "dotbit identity live_count overflow: account_id=0x{}",
                    hex::encode(&account.account_id)
                )
            })?;
        } else if !was_live {
            // Re-activate consumed identity
            agg.live_count = agg.live_count.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "dotbit identity live_count overflow on reactivate: account_id=0x{}",
                    hex::encode(&account.account_id)
                )
            })?;
        }
        let owner_from = if was_live { old_owner.as_deref() } else { None };
        let op = if existing.is_none() {
            "insert_new"
        } else if was_live {
            "insert_transfer"
        } else {
            "insert_reactivate"
        };
        let ctx = OwnerTransitionContext {
            operation: op,
            account_id: &account.account_id,
            block_number,
            tx_hash,
            existing_entry: existing.as_ref(),
        };
        self.apply_dotbit_identity_owner_transition(
            cid,
            owner_from,
            Some(account.owner_lock_hash.as_slice()),
            &mut agg,
            batch,
            state,
            &ctx,
        )?;
        state.put_identity_agg(cid, agg, batch);

        // Track hourly transfers for existing live accounts being transferred
        if was_live {
            if old_owner.is_none() {
                bail!(
                    "dotbit live account missing owner_lock_hash during transfer: account_id=0x{}",
                    hex::encode(&account.account_id)
                );
            }
            if old_owner.as_deref() != Some(account.owner_lock_hash.as_slice()) {
                let hour_bucket = ckbadger_common::stats_bucket::stats_hour_index(timestamp_ms)?;
                let key = ckbadger_store::keys::encode_object_hourly_key(
                    &DOTBIT_SENTINEL_COLLECTION,
                    hour_bucket,
                );
                state.stats.mutate_hourly(
                    self.store.as_ref(),
                    block_number,
                    &key,
                    1,
                    &|| {
                        format!(
                            "object_hourly .bit collection hour_bucket={hour_bucket} block={block_number}"
                        )
                    },
                )?;
            }
        }
        batch.put_dotbit_account_outpoint(
            tx_hash,
            account_output.output_index,
            &account.account_id,
        );
        batch.put_dotbit_outpoint_by_account_id(
            &account.account_id,
            tx_hash,
            account_output.output_index,
            action,
        );
        Ok(())
    }

    /// Record an AccountCell-typed output that names no indexable account, so
    /// its later consumption is classified as "never an account" instead of
    /// failing as a lost `CF_DOTBIT_OUTPOINT` row. Domain store
    /// (`stats_mnft`), in the writing block's scope like the account outpoint
    /// rows (the block journal records its creation).
    pub(crate) fn insert_dotbit_unindexed_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        batch.put_dotbit_unindexed_outpoint(tx_hash, output_index);
        Ok(())
    }

    /// Consume a .bit account. Returns `Some(DOTBIT_SENTINEL_COLLECTION)` if consumed.
    pub(crate) fn consume_dotbit_account_with_state(
        &self,
        account_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
        state: &mut DotbitBatchState,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(mut entry) = state.get_account(self.store.as_ref(), account_id)? {
            if !entry.is_live {
                bail!(
                    "dotbit account already consumed: account_id=0x{}, block={}, tx=0x{}, \
                     created_at_block={}",
                    hex::encode(account_id),
                    block_number,
                    hex::encode(tx_hash),
                    entry.created_at_block
                );
            }
            let old_owner = entry.owner_lock_hash.clone();
            if old_owner.is_none() {
                bail!(
                    "dotbit live account missing owner_lock_hash during consume: account_id=0x{}, \
                     block={}, tx=0x{}",
                    hex::encode(account_id),
                    block_number,
                    hex::encode(tx_hash)
                );
            }
            // Snapshot entry state before mutation for diagnostic context
            let entry_snapshot = entry.clone();
            state.consumed_for_actions.insert(
                (account_id.to_vec(), tx_hash.to_vec()),
                IdentityActionState::from_entry(&entry).with_context(|| {
                    format!(
                        ".bit action snapshot: item=0x{} block={block_number} tx=0x{}",
                        hex::encode(account_id),
                        hex::encode(tx_hash)
                    )
                })?,
            );
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_identity(account_id, &entry);
            state.put_account(account_id, entry);

            // Update identity collection aggregate
            let cid = &DOTBIT_SENTINEL_COLLECTION;
            let mut agg = state.get_identity_agg(self.store.as_ref(), cid)?;
            if agg.live_count <= 0 {
                bail!(
                    "dotbit identity live_count underflow on consume: account_id=0x{}, \
                     block={}, tx=0x{}, live_count={}, created_at_block={}",
                    hex::encode(account_id),
                    block_number,
                    hex::encode(tx_hash),
                    agg.live_count,
                    entry_snapshot.created_at_block
                );
            }
            agg.live_count -= 1;
            let ctx = OwnerTransitionContext {
                operation: "consume",
                account_id,
                block_number,
                tx_hash,
                existing_entry: Some(&entry_snapshot),
            };
            self.apply_dotbit_identity_owner_transition(
                cid,
                old_owner.as_deref(),
                None,
                &mut agg,
                batch,
                state,
                &ctx,
            )?;
            state.put_identity_agg(cid, agg, batch);

            return Ok(Some(DOTBIT_SENTINEL_COLLECTION.to_vec()));
        }
        Ok(None)
    }

    pub fn get_dotbit_account_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        self.store
            .get_dotbit_account_id_by_outpoint(tx_hash, output_index)
    }

    /// Batch lookup: find account_ids for multiple outpoints.
    pub fn get_dotbit_account_ids_by_outpoints_batch(
        &self,
        tx_hashes: &[Vec<u8>],
        output_indices: &[i16],
    ) -> Result<Vec<(Vec<u8>, i16, Vec<u8>)>> {
        let outpoints: Vec<(&[u8], i16)> = tx_hashes
            .iter()
            .zip(output_indices.iter())
            .map(|(hash, idx)| (hash.as_slice(), *idx))
            .collect();
        self.store
            .get_dotbit_account_ids_by_outpoints_batch(&outpoints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::BatchWriter;
    use ckbadger_store::store::CkbadgerStore;
    use ckbadger_store::{
        CachedBlockHeader, LiveCellInfo, ScriptInfo, ScriptReferenceInfo, TxIndexEntry,
    };
    use std::sync::Arc;

    /// One batch's .bit writer state, built the way `write_parsed_batch`
    /// builds it (one entity stats overlay), around a single write.
    fn in_one_batch<R>(
        writer: &BatchWriter,
        batch: &mut StoreBatch,
        f: impl FnOnce(&mut StoreBatch, &mut DotbitBatchState) -> Result<R>,
    ) -> Result<R> {
        let entity_stats = SharedEntityStatsOverlay::standalone(writer.store()).unwrap();
        let mut state = writer.new_dotbit_batch_state(entity_stats.clone());
        let out = f(batch, &mut state)?;
        entity_stats.stage_final(batch)?;
        Ok(out)
    }

    fn insert_account(
        writer: &BatchWriter,
        account_output: &ParsedDotbitAccountOutput,
        tx_hash: &[u8],
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        in_one_batch(writer, batch, |batch, state| {
            writer.insert_dotbit_account_with_state(
                account_output,
                tx_hash,
                block_number,
                timestamp_ms,
                batch,
                state,
            )
        })
    }

    fn consume_account(
        writer: &BatchWriter,
        account_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<Option<Vec<u8>>> {
        in_one_batch(writer, batch, |batch, state| {
            writer.consume_dotbit_account_with_state(
                account_id,
                block_number,
                tx_hash,
                batch,
                state,
            )
        })
    }

    fn test_split_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
        std::mem::forget(domain_dir);
        std::mem::forget(append_dir);
        (domain, append)
    }

    fn make_header(block_num: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![block_num as u8; 32],
            timestamp: 1_000_000 + block_num * 1000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0u8; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x1a08a97e,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_000_000 + block_num * 1000,
        }
    }

    fn make_tx_entry(outputs_count: i16) -> TxIndexEntry {
        TxIndexEntry {
            tx_hash: [0u8; 32],
            is_cellbase: false,
            timestamp: 1_000_000,
            inputs_count: 0,
            outputs_count,
            fee: 1000,
            tx_size: 128,
            cycles: Some(10_000),
            semantic_tags: 0,
        }
    }

    /// The live payload of the reactivated account cell; its owner is the
    /// derived hash of the lock `[0x55; 32]` / `type` / `[0x66; 20]`. The cell
    /// carries no type script: the fixture inserts the `.bit` rows directly.
    fn make_live_cell() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 100_00000000,
            lock_code_hash: [0x55; 32],
            lock_hash_type: 1,
            lock_args: vec![0x66; 20],
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            udt_amount: None,
            data_hash: None,
        }
    }

    #[test]
    fn identity_action_rejects_invalid_prior_owner_width() {
        let previous = IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![1; 31]),
            name: None,
            is_live: true,
            created_at_block: 42,
            created_at_tx: vec![2; 32],
            extra: IdentityExtra::DidCkb,
        };
        let error = IdentityActionState::from_entry(&previous)
            .expect_err("a malformed prior owner must not classify as a transfer");
        let message = error.to_string();
        assert!(
            message.contains("owner") && message.contains("31"),
            "{message}"
        );
        assert!(
            message.contains("did_ckb") && message.contains("42"),
            "{message}"
        );
    }

    #[test]
    fn identity_action_snapshot_preserves_input_and_live_selection() {
        let mut entry = IdentityEntry {
            standard: IdentityStandard::BitCell,
            owner_lock_hash: Some(vec![1; 32]),
            name: Some("alice.bit".into()),
            is_live: true,
            created_at_block: 42,
            created_at_tx: vec![2; 32],
            extra: IdentityExtra::BitCell {
                account_id: vec![3; 20],
                expired_at: 100,
            },
        };
        let consumed = IdentityActionState::from_entry(&entry).unwrap();
        entry.owner_lock_hash = None;
        entry.is_live = false;
        entry.extra = IdentityExtra::BitCell {
            account_id: vec![3; 20],
            expired_at: 200,
        };
        let previous = IdentityActionState::for_output(Some(consumed), Some(&entry)).unwrap();
        assert_eq!(
            classify_identity_output_action(previous.as_ref(), &[1; 32], Some(150)).unwrap(),
            AssetAction::Renew
        );
        assert_eq!(
            classify_identity_output_action(previous.as_ref(), &[2; 32], Some(150)).unwrap(),
            AssetAction::Transfer
        );
        assert!(IdentityActionState::for_output(None, Some(&entry))
            .unwrap()
            .is_none());

        // Some missing-owner input is not the absence of an input. Preserve
        // it in the snapshot and report the existing actionable error only
        // when an output asks for classification.
        let missing_owner = IdentityActionState::from_entry(&entry).unwrap();
        let previous = IdentityActionState::for_output(Some(missing_owner), None).unwrap();
        let message = classify_identity_output_action(previous.as_ref(), &[1; 32], None)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("missing prior owner") && message.contains("42"),
            "{message}"
        );
        entry.owner_lock_hash = Some(vec![4; 32]);
        entry.is_live = true;
        let previous = IdentityActionState::for_output(None, Some(&entry)).unwrap();
        assert_eq!(
            classify_identity_output_action(previous.as_ref(), &[4; 32], Some(200)).unwrap(),
            AssetAction::Update
        );
        let previous = IdentityActionState::for_output(Some(consumed), Some(&entry)).unwrap();
        assert_eq!(
            classify_identity_output_action(previous.as_ref(), &[1; 32], Some(100)).unwrap(),
            AssetAction::Update
        );
        assert_eq!(
            classify_identity_output_action(None, &[1; 32], None).unwrap(),
            AssetAction::Mint
        );
    }

    #[test]
    fn item_actions_use_owner_and_expiry_not_the_transaction_neighbour_action() {
        let previous = IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![1; 32]),
            name: Some("valar.bit".into()),
            is_live: true,
            created_at_block: 12,
            created_at_tx: vec![2; 32],
            extra: IdentityExtra::DotBit {
                expired_at: Some(100),
                registered_at: Some(1),
                status: Some(153),
            },
        };
        let previous = IdentityActionState::from_entry(&previous).unwrap();
        assert_eq!(
            classify_identity_output_action(None, &[1; 32], Some(100)).unwrap(),
            AssetAction::Mint
        );
        assert_eq!(
            classify_identity_output_action(Some(&previous), &[1; 32], Some(100)).unwrap(),
            AssetAction::Update
        );
        assert_eq!(
            classify_identity_output_action(Some(&previous), &[2; 32], Some(100)).unwrap(),
            AssetAction::Transfer
        );
        assert_eq!(
            classify_identity_output_action(Some(&previous), &[1; 32], Some(200)).unwrap(),
            AssetAction::Renew
        );
        for action in [
            AssetAction::Mint,
            AssetAction::Transfer,
            AssetAction::Burn,
            AssetAction::Recycle,
            AssetAction::Renew,
            AssetAction::Update,
        ] {
            assert_eq!(
                AssetAction::from_compact(&[action.compact()]).unwrap(),
                action
            );
        }
        assert!(AssetAction::from_compact(&[]).is_err());
        assert!(AssetAction::from_compact(&[255]).is_err());
    }

    #[test]
    fn test_dotbit_outpoint_lookups_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let account = ParsedDotbitAccountOutput {
            output_index: 6,
            account: crate::parser::dotbit::ParsedDotbitAccount {
                account_id: vec![0x11; 20],
                account: Some("alice.bit".to_string()),
                type_script_hash: vec![0x21; 32],
                next_account_id: None,
                expired_at: None,
                registered_at: None,
                status: None,
                owner_lock_hash: vec![0x31; 32],
            },
        };
        let tx_hash = vec![0x41; 32];

        let mut batch = StoreBatch::new(writer.store());
        insert_account(&writer, &account, &tx_hash, 1, 0, &mut batch).unwrap();
        batch.commit().unwrap();

        let loaded = writer
            .get_dotbit_account_id_by_outpoint(&tx_hash, 6)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, account.account.account_id);

        let entry = writer
            .store()
            .get_identity(&account.account.account_id)
            .unwrap()
            .expect("dotbit identity exists");
        assert_eq!(entry.name.as_deref(), Some("alice.bit"));

        let batch_loaded = writer
            .get_dotbit_account_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[6])
            .unwrap();
        assert_eq!(batch_loaded.len(), 1);
        assert_eq!(batch_loaded[0].0, tx_hash);
        assert_eq!(batch_loaded[0].1, 6);
    }

    #[test]
    fn test_consume_dotbit_account_errors_on_double_consume() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let account = ParsedDotbitAccountOutput {
            output_index: 6,
            account: crate::parser::dotbit::ParsedDotbitAccount {
                account_id: vec![0x11; 20],
                account: Some("alice.bit".to_string()),
                type_script_hash: vec![0x21; 32],
                next_account_id: None,
                expired_at: None,
                registered_at: None,
                status: None,
                owner_lock_hash: vec![0x31; 32],
            },
        };
        let tx_hash = vec![0x41; 32];

        let mut batch = StoreBatch::new(writer.store());
        insert_account(&writer, &account, &tx_hash, 1, 0, &mut batch).unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        consume_account(
            &writer,
            &account.account.account_id,
            2,
            &tx_hash,
            &mut batch,
        )
        .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let err = consume_account(
            &writer,
            &account.account.account_id,
            3,
            &tx_hash,
            &mut batch,
        )
        .unwrap_err();
        assert!(err.to_string().contains("already consumed"));
    }

    #[test]
    fn test_reactivate_dotbit_account() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let account = ParsedDotbitAccountOutput {
            output_index: 6,
            account: crate::parser::dotbit::ParsedDotbitAccount {
                account_id: vec![0x11; 20],
                account: Some("alice.bit".to_string()),
                type_script_hash: vec![0x21; 32],
                next_account_id: None,
                expired_at: None,
                registered_at: None,
                status: None,
                owner_lock_hash: vec![0x31; 32],
            },
        };
        let create_tx_hash = vec![0x41; 32];
        let consume_tx_hash = vec![0x42; 32];
        let recreate_tx_hash = vec![0x43; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .insert_dotbit_account_with_state(
                &account,
                &create_tx_hash,
                1,
                0,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .consume_dotbit_account_with_state(
                &account.account.account_id,
                2,
                &consume_tx_hash,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .insert_dotbit_account_with_state(
                &account,
                &recreate_tx_hash,
                3,
                0,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let entry = writer
            .store()
            .get_identity(&account.account.account_id)
            .unwrap()
            .unwrap();
        assert!(entry.is_live);
    }

    #[test]
    fn test_consume_dotbit_account_reads_uncommitted_insert_from_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let account = ParsedDotbitAccountOutput {
            output_index: 6,
            account: crate::parser::dotbit::ParsedDotbitAccount {
                account_id: vec![0x11; 20],
                account: Some("alice.bit".to_string()),
                type_script_hash: vec![0x21; 32],
                next_account_id: None,
                expired_at: None,
                registered_at: None,
                status: None,
                owner_lock_hash: vec![0x31; 32],
            },
        };
        let tx_hash = vec![0x41; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .insert_dotbit_account_with_state(&account, &tx_hash, 1, 0, &mut batch, &mut state)
            .unwrap();
        writer
            .consume_dotbit_account_with_state(
                &account.account.account_id,
                1,
                &tx_hash,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let entry = writer
            .store()
            .get_identity(&account.account.account_id)
            .unwrap()
            .unwrap();
        assert!(!entry.is_live);
    }

    #[test]
    fn test_get_hourly_transfer_errors_on_invalid_existing_value_length() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());
        let state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());

        let key = ckbadger_store::keys::encode_object_hourly_key(&DOTBIT_SENTINEL_COLLECTION, 1);
        let mut seed = StoreBatch::new(writer.store());
        seed.put_stats(&key, &[1, 2, 3, 4]);
        seed.commit().unwrap();

        let err = state
            .stats
            .current_hourly(writer.store(), &key)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid entity hourly transfer value length"));
    }

    #[test]
    fn test_das_action_to_asset_action_approval_actions() {
        assert!(matches!(
            das_action_to_asset_action("create_approval"),
            Some(AssetAction::Update)
        ));
        assert!(matches!(
            das_action_to_asset_action("delay_approval"),
            Some(AssetAction::Update)
        ));
        assert!(matches!(
            das_action_to_asset_action("revoke_approval"),
            Some(AssetAction::Update)
        ));
        // fulfill_approval is a transfer (existing mapping)
        assert!(matches!(
            das_action_to_asset_action("fulfill_approval"),
            Some(AssetAction::Transfer)
        ));
    }

    #[test]
    fn test_das_action_to_asset_action_new_mappings() {
        assert!(matches!(
            das_action_to_asset_action("upgrade_did"),
            Some(AssetAction::Update)
        ));
        assert!(matches!(
            das_action_to_asset_action("account_cell_upgrade"),
            Some(AssetAction::Update)
        ));
        assert!(matches!(
            das_action_to_asset_action("sell_account"),
            Some(AssetAction::Transfer)
        ));
    }

    #[test]
    fn unknown_das_action_does_not_invent_collection_mint() {
        let created: HashSet<Vec<u8>> = HashSet::from([vec![0x11; 20]]);
        let consumed: HashSet<Vec<u8>> = HashSet::new();
        let entry = build_dotbit_tx_activity_entry(
            Some("invented_future_action"),
            &created,
            &consumed,
            &[0xAAu8; 32],
            &[0xBBu8; 32],
            1_700_000_000_000,
        );
        assert!(
            entry.is_none(),
            "unknown DAS action must not invent collection Mint via generic fallback"
        );
        // No ActionData still uses generic detection (created-only → Mint).
        let legacy = build_dotbit_tx_activity_entry(
            None,
            &created,
            &consumed,
            &[0xAAu8; 32],
            &[0xBBu8; 32],
            1_700_000_000_000,
        )
        .expect("missing ActionData still resolves generically");
        assert_eq!(legacy.actions, vec![AssetAction::Mint]);
    }

    #[test]
    fn test_classify_das_action_suppressed_vs_unknown() {
        // Suppressed actions (known, no activity) must NOT trigger warning
        let suppressed = [
            "update_sub_account",
            "lock_sub_account_for_cross_chain",
            "unlock_sub_account_for_cross_chain",
            "propose",
            "apply_register",
            "pre_register",
            "recycle_proposal",
            "consolidate_income",
            "deploy",
            "transfer_balance",
            "config",
            "make_offer",
            "mint_dp",
            "retract_reverse_record",
            "create_device_key_list",
            "refund_pay",
            "order_refund",
            "cross_refund",
        ];
        for action in &suppressed {
            assert!(
                matches!(classify_das_action(action), DasActionKind::Suppressed),
                "{action} should be Suppressed"
            );
        }
        // Unknown action triggers warning
        assert!(matches!(
            classify_das_action("some_future_action"),
            DasActionKind::Unknown
        ));
    }

    /// Legacy testnet actions from contract versions that predate the current
    /// upstream catalogue. Classified from the transactions themselves: none
    /// of the three below carries an AccountCell (`0x1106d9ea…`) type script
    /// on any output, so they produce no .bit collection activity.
    #[test]
    fn test_classify_das_action_legacy_non_account_cell_actions() {
        for action in ["quote_price_for_ckb", "create_wallet", "balance_deposit"] {
            assert!(
                matches!(classify_das_action(action), DasActionKind::Suppressed),
                "{action} touches no AccountCell and should be Suppressed"
            );
        }
    }

    /// `bid_expired_account_auction` is the pre-rename spelling of
    /// `bid_expired_account_dutch_auction`: its outputs do carry an AccountCell,
    /// and the winning bid moves the expired account to a new owner.
    #[test]
    fn test_classify_das_action_legacy_expired_account_auction_is_transfer() {
        // Both spellings name the same on-chain action.
        assert!(matches!(
            das_action_to_asset_action("bid_expired_account_auction"),
            Some(AssetAction::Transfer)
        ));
        assert!(matches!(
            das_action_to_asset_action("bid_expired_account_dutch_auction"),
            Some(AssetAction::Transfer)
        ));
    }

    fn make_test_account(
        account_id: &[u8],
        owner_lock_hash: &[u8],
        name: &str,
    ) -> ParsedDotbitAccountOutput {
        ParsedDotbitAccountOutput {
            output_index: 0,
            account: crate::parser::dotbit::ParsedDotbitAccount {
                account_id: account_id.to_vec(),
                account: Some(name.to_string()),
                type_script_hash: vec![0x21; 32],
                next_account_id: None,
                expired_at: None,
                registered_at: None,
                status: None,
                owner_lock_hash: owner_lock_hash.to_vec(),
            },
        }
    }

    #[test]
    fn test_insert_dotbit_updates_identity_collection_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let owner_a = vec![0xA1; 32];
        let owner_b = vec![0xB2; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .insert_dotbit_account_with_state(
                &make_test_account(&[0x01; 20], &owner_a, "alice.bit"),
                &[0xF1; 32],
                100,
                100_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_dotbit_account_with_state(
                &make_test_account(&[0x02; 20], &owner_b, "bob.bit"),
                &[0xF2; 32],
                100,
                100_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_identity_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .expect("dotbit aggregate should exist");
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 2);
        assert_eq!(agg.holders_count, 2);
        assert_eq!(agg.standard, IdentityStandard::DotBit);
    }

    #[test]
    fn test_consume_dotbit_decrements_identity_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let owner = vec![0xA1; 32];
        let account_id = [0x01u8; 20];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .insert_dotbit_account_with_state(
                &make_test_account(&account_id, &owner, "alice.bit"),
                &[0xF1; 32],
                100,
                100_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        let result = writer
            .consume_dotbit_account_with_state(
                &account_id,
                200,
                &[0xFF; 32],
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        assert_eq!(result, Some(DOTBIT_SENTINEL_COLLECTION.to_vec()));

        let agg = store
            .get_identity_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);
    }

    #[test]
    fn test_dotbit_same_owner_two_accounts_holders_count_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let store = Arc::new(store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let owner = vec![0xA1; 32];

        let mut batch = StoreBatch::new(writer.store());
        let mut state = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        writer
            .insert_dotbit_account_with_state(
                &make_test_account(&[0x01; 20], &owner, "alice.bit"),
                &[0xF1; 32],
                100,
                100_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        writer
            .insert_dotbit_account_with_state(
                &make_test_account(&[0x02; 20], &owner, "bob.bit"),
                &[0xF2; 32],
                100,
                100_000,
                &mut batch,
                &mut state,
            )
            .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_identity_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 2);
        assert_eq!(agg.live_count, 2);
        assert_eq!(agg.holders_count, 1, "same owner should count as 1 holder");
    }

    /// The unindexed-AccountCell marker is journaled in its block: a rollback
    /// past its block removes it, and a marker written at or below the fork
    /// point survives.
    #[test]
    fn test_dotbit_unindexed_marker_is_undone_by_rollback_and_survivor_stays() {
        let (domain, append) = test_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());
        let survivor_tx = [0xE1u8; 32];
        let rolled_back_tx = [0xE2u8; 32];

        // Each marker in its block's journaled scope, as the object loop
        // writes it.
        crate::db::writer::journal_test_support::journaled_headers(&domain, 150..=200);
        ckbadger_store::commit_journaled_block(domain.as_ref(), 100, |batch| {
            writer
                .insert_dotbit_unindexed_outpoint(&survivor_tx, 0, batch)
                .unwrap();
        })
        .unwrap();
        ckbadger_store::commit_journaled_block(domain.as_ref(), 200, |batch| {
            writer
                .insert_dotbit_unindexed_outpoint(&rolled_back_tx, 1, batch)
                .unwrap();
        })
        .unwrap();
        assert!(domain
            .is_dotbit_unindexed_outpoint(&survivor_tx, 0)
            .unwrap());
        assert!(domain
            .is_dotbit_unindexed_outpoint(&rolled_back_tx, 1)
            .unwrap());

        domain.rollback_to_block(150).unwrap();

        assert!(
            domain
                .is_dotbit_unindexed_outpoint(&survivor_tx, 0)
                .unwrap(),
            "a marker at or below the fork point survives"
        );
        assert!(
            !domain
                .is_dotbit_unindexed_outpoint(&rolled_back_tx, 1)
                .unwrap(),
            "a rolled-back marker is removed by its journal entry"
        );
    }

    #[test]
    fn test_dotbit_reorg_restores_consumed_state_after_reactivation() {
        let (domain, append) = test_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());

        let owner = make_live_cell().lock_script_hash().to_vec();
        let account_id = [0x11u8; 20];
        let initial_tx_hash = vec![0xB1; 32];
        let reactivate_tx_hash = vec![0xC1; 32];

        // The lock script rollups each block leaves (lock code hash 0x55):
        // created at 100, consumed at 120, a second cell created at 200.
        let script_rows = |batch: &mut StoreBatch, cells: i64, live: i64, owned: i128| {
            let capacity = i128::from(cells) * 100_00000000;
            let used = i128::from(cells) * 61_00000000;
            let knowledge = i128::from(live) * 61_00000000;
            batch.put_script_info(
                &[0x55; 32],
                &ScriptInfo {
                    code_hash: vec![0x55; 32],
                    hash_type: 1,
                    lock_cells_count: cells,
                    lock_live_cells_count: live,
                    lock_capacity_sum: capacity,
                    lock_owned_capacity_sum: owned,
                    lock_used_capacity_sum: used,
                    lock_owned_knowledge_sum: knowledge,
                    ..Default::default()
                },
            );
            batch.put_script_reference_info(
                1,
                &[0x55; 32],
                &ScriptReferenceInfo {
                    reference_hash: vec![0x55; 32],
                    hash_type: 1,
                    lock_cells_count: cells,
                    lock_live_cells_count: live,
                    lock_capacity_sum: capacity,
                    lock_owned_capacity_sum: owned,
                    lock_used_capacity_sum: used,
                    lock_owned_knowledge_sum: knowledge,
                    ..Default::default()
                },
            );
        };

        // Contiguous headers across the fork point and the rolled-back range,
        // each in its block's journaled scope as the live writer commits them.
        ckbadger_store::commit_journaled(domain.as_ref(), |headers| {
            for block in 150..=200i64 {
                headers
                    .at(block)
                    .put_block_header(block, &make_header(block));
            }
            // Tx position 1: position 0 of every block is its cellbase.
            headers.put_tx_index(200, 1, &make_tx_entry(1));
        })
        .unwrap();

        // Each block's rows in its journaled scope, as the live writer
        // commits them.
        ckbadger_store::commit_journaled_block(domain.as_ref(), 100, |batch| {
            insert_account(
                &writer,
                &make_test_account(&account_id, &owner, "alice.bit"),
                &initial_tx_hash,
                100,
                100_000,
                batch,
            )
            .unwrap();
            script_rows(batch, 1, 1, 100_00000000);
        })
        .unwrap();
        ckbadger_store::commit_journaled_block(domain.as_ref(), 120, |batch| {
            consume_account(&writer, &account_id, 120, &[0xD1; 32], batch).unwrap();
            script_rows(batch, 1, 0, 0);
        })
        .unwrap();

        let mut append_batch = StoreBatch::new(append.as_ref());
        append_batch
            .put_cell_payload_by_outpoint(&reactivate_tx_hash, 0, &make_live_cell())
            .unwrap();
        append_batch.commit().unwrap();

        ckbadger_store::commit_journaled_block(domain.as_ref(), 200, |batch| {
            insert_account(
                &writer,
                &make_test_account(&account_id, &owner, "alice.bit"),
                &reactivate_tx_hash,
                200,
                200_000,
                batch,
            )
            .unwrap();
            batch.put_live_cell_marker_by_outpoint(&reactivate_tx_hash, 0, 200);
            batch.put_tx_hash_map(&reactivate_tx_hash, 200, 1);
            // The forward writer records one addr_txs row per tx participant;
            // the owner of the reactivation output is this tx's only participant.
            batch.put_addr_tx(
                &owner,
                200,
                1,
                &ckbadger_store::types::AddrTxValue::new(0, false, true, 0),
            );
            batch.put_addr_balance(
                &owner,
                &ckbadger_store::AddressBalance {
                    balance: 100_00000000,
                    used_capacity: 61_00000000,
                    live_cells_count: 1,
                    total_cells_count: 2,
                    txs_count: 1,
                    cellbase_txs_count: 0,
                    first_seen_block: 100,
                    first_seen_tx_idx: 0,
                    last_activity_block: 200,
                    last_activity_tx_idx: 0,
                },
            );
            script_rows(batch, 2, 1, 100_00000000);
        })
        .unwrap();

        let before = domain.get_identity(&account_id).unwrap().unwrap();
        assert!(before.is_live, "reactivation should make the account live");

        domain.rollback_to_block(150).unwrap();

        let restored = domain.get_identity(&account_id).unwrap().unwrap();
        assert!(
            !restored.is_live,
            "rollback should restore the consumed state that existed at block 150"
        );
        assert!(
            restored.owner_lock_hash.is_none(),
            "consumed .bit account should not retain a live owner after rollback"
        );

        let agg = domain
            .get_identity_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);

        let live_outpoints = domain
            .get_live_dotbit_outpoints_by_account_ids(&[account_id.to_vec()], append.as_ref())
            .unwrap();
        assert!(
            !live_outpoints.contains_key(account_id.as_slice()),
            "rolled-back reactivation must not leave a live outpoint behind"
        );
        let script = domain.get_script_info(&[0x55; 32]).unwrap().unwrap();
        assert_eq!(script.lock_cells_count, 1);
        assert_eq!(script.lock_live_cells_count, 0);
        assert_eq!(script.lock_capacity_sum, 100_00000000);
        assert_eq!(script.lock_owned_capacity_sum, 0);
        let reference = domain
            .get_script_reference_info(1, &[0x55; 32])
            .unwrap()
            .unwrap();
        assert_eq!(reference.lock_cells_count, 1);
        assert_eq!(reference.lock_live_cells_count, 0);
    }
}
