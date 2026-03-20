use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use tracing::warn;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::store::CF_STATS_IDENTITY;
use ckbadger_store::types::{
    AssetAction, IdentityCollectionAggregate, IdentityEntry, IdentityExtra, IdentityStandard,
    ObjectCollectionActivityEntry, UndoLogEntry, UndoLogStoreTarget,
};
use ckbadger_store::{CkbadgerStore, CF_IDENTITY_AGG, CF_IDENTITY_DATA, CF_STATS_OBJECT};

use crate::parser::dotbit::ParsedDotbitAccountOutput;
use crate::sync::types::UndoSeqScope;
use crate::sync::undo::next_undo_seq;

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
        _ => DasActionKind::Unknown,
    }
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
                    "unknown DAS action, falling back to generic activity detection"
                );
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

pub(crate) fn resolve_dotbit_tx_activity(
    das_action: Option<&str>,
    created_account_ids: &HashSet<Vec<u8>>,
    consumed_account_ids: &HashSet<Vec<u8>>,
    tx_hash: &[u8],
    block_hash: &[u8],
    block_number: i64,
    tx_idx: i32,
    timestamp_ms: i64,
    batch: &mut StoreBatch,
) -> bool {
    let Some(entry) = build_dotbit_tx_activity_entry(
        das_action,
        created_account_ids,
        consumed_account_ids,
        tx_hash,
        block_hash,
        timestamp_ms,
    ) else {
        return false;
    };

    batch.put_identity_collection_activity(
        &DOTBIT_SENTINEL_COLLECTION,
        block_number,
        tx_idx,
        &entry,
    );
    true
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

#[derive(Default)]
pub(crate) struct DotbitBatchState {
    accounts: HashMap<Vec<u8>, Option<IdentityEntry>>,
    hourly_transfers: HashMap<Vec<u8>, i64>,
    identity_aggs: HashMap<Vec<u8>, IdentityCollectionAggregate>,
    identity_owner_counts: HashMap<(Vec<u8>, Vec<u8>), i64>,
    undo_seq_by_block: HashMap<i64, u64>,
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

    fn get_hourly_transfer(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        if let Some(cached) = self.hourly_transfers.get(key) {
            return Ok(*cached);
        }
        let loaded = match store.get_stats_key(key)? {
            Some(v) => {
                if v.len() != 8 {
                    bail!(
                        "invalid .bit hourly transfer value length in stats CF: key=0x{}, len={}",
                        hex::encode(key),
                        v.len()
                    );
                }
                i64::from_le_bytes(v[..8].try_into().map_err(|_| {
                    anyhow!(
                        "failed to decode .bit hourly transfer value as i64: key=0x{}",
                        hex::encode(key)
                    )
                })?)
            }
            None => 0,
        };
        self.hourly_transfers.insert(key.to_vec(), loaded);
        Ok(loaded)
    }

    fn put_hourly_transfer(&mut self, key: Vec<u8>, count: i64) {
        self.hourly_transfers.insert(key, count);
    }

    fn get_identity_agg_with_existence(
        &mut self,
        store: &CkbadgerStore,
        collection_id: &[u8],
    ) -> Result<(IdentityCollectionAggregate, bool)> {
        if let Some(cached) = self.identity_aggs.get(collection_id) {
            return Ok((cached.clone(), true));
        }
        let loaded = store.get_identity_collection_aggregate(collection_id)?;
        let agg = loaded.clone().unwrap_or_default();
        self.identity_aggs
            .insert(collection_id.to_vec(), agg.clone());
        Ok((agg, loaded.is_some()))
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
}

impl BatchWriter {
    fn record_dotbit_domain_undo(
        &self,
        batch: &mut StoreBatch<'_>,
        block_number: i64,
        cf_name: &'static str,
        key: &[u8],
        previous_value: Option<Vec<u8>>,
        undo_seq: &mut HashMap<i64, u64>,
    ) {
        if self.store.is_bulk_sync_mode() {
            return;
        }
        let seq = next_undo_seq(undo_seq, block_number, UndoSeqScope::DotBit);
        batch.put_reorg_undo_log_by_block(
            block_number,
            seq,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: cf_name.to_string(),
                key: key.to_vec(),
                previous_value,
            },
        );
    }

    fn record_dotbit_identity_undo(
        &self,
        batch: &mut StoreBatch<'_>,
        block_number: i64,
        account_id: &[u8],
        previous_entry: Option<&IdentityEntry>,
        undo_seq: &mut HashMap<i64, u64>,
    ) {
        let previous_value = previous_entry.map(|entry| {
            bincode::serialize(entry).expect("serialize previous dotbit identity entry for undo")
        });
        self.record_dotbit_domain_undo(
            batch,
            block_number,
            CF_IDENTITY_DATA,
            account_id,
            previous_value,
            undo_seq,
        );
    }

    fn record_dotbit_identity_agg_undo(
        &self,
        batch: &mut StoreBatch<'_>,
        block_number: i64,
        collection_id: &[u8],
        previous_agg: &IdentityCollectionAggregate,
        existed: bool,
        undo_seq: &mut HashMap<i64, u64>,
    ) {
        let previous_value = existed.then(|| {
            bincode::serialize(previous_agg)
                .expect("serialize previous dotbit identity aggregate for undo")
        });
        self.record_dotbit_domain_undo(
            batch,
            block_number,
            CF_IDENTITY_AGG,
            collection_id,
            previous_value,
            undo_seq,
        );
    }

    fn record_dotbit_identity_owner_undo(
        &self,
        batch: &mut StoreBatch<'_>,
        block_number: i64,
        collection_id: &[u8],
        lock_hash: &[u8],
        previous_count: i64,
        undo_seq: &mut HashMap<i64, u64>,
    ) {
        let key = ckbadger_store::keys::encode_identity_owner_key(collection_id, lock_hash);
        let previous_value = (previous_count > 0).then(|| previous_count.to_le_bytes().to_vec());
        self.record_dotbit_domain_undo(
            batch,
            block_number,
            CF_STATS_IDENTITY,
            &key,
            previous_value,
            undo_seq,
        );
    }

    fn record_dotbit_stats_object_undo(
        &self,
        batch: &mut StoreBatch<'_>,
        block_number: i64,
        key: &[u8],
        previous_value: Option<Vec<u8>>,
        undo_seq: &mut HashMap<i64, u64>,
    ) {
        self.record_dotbit_domain_undo(
            batch,
            block_number,
            CF_STATS_OBJECT,
            key,
            previous_value,
            undo_seq,
        );
    }

    pub(crate) fn new_dotbit_batch_state(&self) -> DotbitBatchState {
        DotbitBatchState::default()
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
                self.record_dotbit_identity_owner_undo(
                    batch,
                    ctx.block_number,
                    collection_id,
                    old_lock,
                    old_count,
                    &mut state.undo_seq_by_block,
                );
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
                self.record_dotbit_identity_owner_undo(
                    batch,
                    ctx.block_number,
                    collection_id,
                    old_lock,
                    old_count,
                    &mut state.undo_seq_by_block,
                );
                state.put_identity_owner_count(collection_id, old_lock, old_count - 1, batch);
            }
        }

        if let Some(new_lock) = new_owner {
            let (cur_count, _) =
                state.get_identity_owner_count(self.store.as_ref(), collection_id, new_lock)?;
            self.record_dotbit_identity_owner_undo(
                batch,
                ctx.block_number,
                collection_id,
                new_lock,
                cur_count,
                &mut state.undo_seq_by_block,
            );
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

    pub fn insert_dotbit_account(
        &self,
        account_output: &ParsedDotbitAccountOutput,
        tx_hash: &[u8],
        block_number: i64,
        timestamp_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let mut state = self.new_dotbit_batch_state();
        self.insert_dotbit_account_with_state(
            account_output,
            tx_hash,
            block_number,
            timestamp_ms,
            batch,
            &mut state,
        )
    }

    pub(crate) fn insert_dotbit_account_with_state(
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
        self.record_dotbit_identity_undo(
            batch,
            block_number,
            &account.account_id,
            existing.as_ref(),
            &mut state.undo_seq_by_block,
        );
        batch.put_identity(&account.account_id, &entry);
        state.put_account(&account.account_id, entry);

        // Update identity collection aggregate
        let cid = &DOTBIT_SENTINEL_COLLECTION;
        let (agg_before, agg_existed) =
            state.get_identity_agg_with_existence(self.store.as_ref(), cid)?;
        self.record_dotbit_identity_agg_undo(
            batch,
            block_number,
            cid,
            &agg_before,
            agg_existed,
            &mut state.undo_seq_by_block,
        );
        let mut agg = agg_before;
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
                let hour_bucket = timestamp_ms / 3_600_000;
                let key = ckbadger_store::keys::encode_nft_hourly_key(
                    &DOTBIT_SENTINEL_COLLECTION,
                    hour_bucket,
                );
                let current = state.get_hourly_transfer(self.store.as_ref(), &key)?;
                let next = current.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "hourly transfer counter overflow for .bit collection at hour_bucket={}",
                        hour_bucket
                    )
                })?;
                batch.put_object_hourly_transfer(&DOTBIT_SENTINEL_COLLECTION, hour_bucket, next);
                state.put_hourly_transfer(key, next);
            }
        }
        let fwd_key = ckbadger_store::keys::encode_dotbit_account_outpoint_key(
            tx_hash,
            account_output.output_index,
        );
        let fwd_previous = self.store.get_cf(self.store.cf_stats_object(), &fwd_key)?;
        self.record_dotbit_stats_object_undo(
            batch,
            block_number,
            &fwd_key,
            fwd_previous,
            &mut state.undo_seq_by_block,
        );
        batch.put_dotbit_account_outpoint(
            tx_hash,
            account_output.output_index,
            &account.account_id,
        );
        let rev_key = ckbadger_store::keys::encode_dotbit_outpoint_by_account_id_key(
            &account.account_id,
            tx_hash,
            account_output.output_index,
        );
        let rev_previous = self.store.get_cf(self.store.cf_stats_object(), &rev_key)?;
        self.record_dotbit_stats_object_undo(
            batch,
            block_number,
            &rev_key,
            rev_previous,
            &mut state.undo_seq_by_block,
        );
        batch.put_dotbit_outpoint_by_account_id(
            &account.account_id,
            tx_hash,
            account_output.output_index,
        );
        Ok(())
    }

    pub fn consume_dotbit_account(
        &self,
        account_id: &[u8],
        block_number: i64,
        tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<Option<Vec<u8>>> {
        let mut state = self.new_dotbit_batch_state();
        self.consume_dotbit_account_with_state(account_id, block_number, tx_hash, batch, &mut state)
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
            entry.is_live = false;
            entry.owner_lock_hash = None;
            self.record_dotbit_identity_undo(
                batch,
                block_number,
                account_id,
                Some(&entry_snapshot),
                &mut state.undo_seq_by_block,
            );
            batch.put_identity(account_id, &entry);
            state.put_account(account_id, entry);

            // Update identity collection aggregate
            let cid = &DOTBIT_SENTINEL_COLLECTION;
            let (agg_before, agg_existed) =
                state.get_identity_agg_with_existence(self.store.as_ref(), cid)?;
            self.record_dotbit_identity_agg_undo(
                batch,
                block_number,
                cid,
                &agg_before,
                agg_existed,
                &mut state.undo_seq_by_block,
            );
            let mut agg = agg_before;
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
        CachedBlockHeader, LiveCellInfo, ScriptInfo, TxIndexEntry, UndoLogEntry, UndoTxContext,
    };
    use std::sync::Arc;

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
        }
    }

    fn make_tx_entry(outputs_count: i16) -> TxIndexEntry {
        TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_000_000,
            inputs_count: 0,
            outputs_count,
            fee: 1000,
            tx_size: 128,
            cycles: Some(10_000),
        }
    }

    fn make_live_cell(account_id: &[u8], owner_lock_hash: &[u8]) -> LiveCellInfo {
        LiveCellInfo {
            capacity: 100_00000000,
            lock_script_hash: owner_lock_hash.to_vec(),
            lock_code_hash: vec![0x55; 32],
            lock_hash_type: 1,
            lock_args: vec![0x66; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: Some(account_id.to_vec()),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
            data_hash: None,
        }
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
        writer
            .insert_dotbit_account(&account, &tx_hash, 1, 0, &mut batch)
            .unwrap();
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
        writer
            .insert_dotbit_account(&account, &tx_hash, 1, 0, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        writer
            .consume_dotbit_account(&account.account.account_id, 2, &tx_hash, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let err = writer
            .consume_dotbit_account(&account.account.account_id, 3, &tx_hash, &mut batch)
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();

        let key = ckbadger_store::keys::encode_nft_hourly_key(&DOTBIT_SENTINEL_COLLECTION, 1);
        let mut seed = StoreBatch::new(writer.store());
        seed.put_stats(&key, &[1, 2, 3, 4]);
        seed.commit().unwrap();

        let err = state.get_hourly_transfer(writer.store(), &key).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid .bit hourly transfer value length"));
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();
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
        let mut state = writer.new_dotbit_batch_state();
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

    #[test]
    fn test_dotbit_reorg_restores_consumed_state_after_reactivation() {
        let (domain, append) = test_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());

        let owner = vec![0xA1; 32];
        let account_id = [0x11u8; 20];
        let initial_tx_hash = vec![0xB1; 32];
        let reactivate_tx_hash = vec![0xC1; 32];

        let mut create_batch = StoreBatch::new(domain.as_ref());
        writer
            .insert_dotbit_account(
                &make_test_account(&account_id, &owner, "alice.bit"),
                &initial_tx_hash,
                100,
                100_000,
                &mut create_batch,
            )
            .unwrap();
        create_batch.commit().unwrap();

        let mut consume_batch = StoreBatch::new(domain.as_ref());
        writer
            .consume_dotbit_account(&account_id, 120, &[0xD1; 32], &mut consume_batch)
            .unwrap();
        consume_batch.commit().unwrap();

        let mut reactivate_batch = StoreBatch::new(domain.as_ref());
        writer
            .insert_dotbit_account(
                &make_test_account(&account_id, &owner, "alice.bit"),
                &reactivate_tx_hash,
                200,
                200_000,
                &mut reactivate_batch,
            )
            .unwrap();
        reactivate_batch.commit().unwrap();

        let mut append_batch = StoreBatch::new(append.as_ref());
        append_batch.put_cell_payload_by_outpoint(
            &reactivate_tx_hash,
            0,
            &make_live_cell(&account_id, &owner),
        );
        append_batch.commit().unwrap();

        let mut domain_batch = StoreBatch::new(domain.as_ref());
        domain_batch.put_block_header(150, &make_header(150));
        domain_batch.put_live_cell_marker_by_outpoint(&reactivate_tx_hash, 0, 200);
        domain_batch.put_block_header(200, &make_header(200));
        domain_batch.put_tx_index(200, 0, &make_tx_entry(1));
        domain_batch.put_tx_hash_map(&reactivate_tx_hash, 200, 0);
        domain_batch.put_script_info(
            &[0x55; 32],
            &ScriptInfo {
                code_hash: vec![0x55; 32],
                hash_type: 1,
                lock_live_cells_count: 1,
                lock_owned_capacity_sum: 100_00000000,
                lock_owned_knowledge_sum: 61_00000000,
                ..Default::default()
            },
        );
        domain_batch.put_reorg_undo_log_by_block(
            200,
            0,
            &UndoLogEntry::TxContext(UndoTxContext {
                tx_hash: reactivate_tx_hash.clone(),
                outputs_count: 1,
                inputs: vec![],
            }),
        );
        domain_batch.commit().unwrap();

        let before = domain.get_identity(&account_id).unwrap().unwrap();
        assert!(before.is_live, "reactivation should make the account live");

        domain
            .rollback_to_block_with_append_only_store(150, Some(append.as_ref()))
            .unwrap();
        domain.rollback_via_undo_log(append.as_ref(), 150).unwrap();

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
    }
}
