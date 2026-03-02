use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use tracing::warn;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AssetAction, NftCollectionActivityEntry, NftCollectionAggregate, NftEntry, NftExtra,
    NftStandard,
};
use ckbadger_store::CkbadgerStore;

use crate::parser::dotbit::ParsedDotbitAccountOutput;

use super::BatchWriter;

/// Sentinel collection key for DotBit accounts (which have no collection_id).
/// 32-byte key: "dotbit_collection_______________" (padded to 32 bytes).
pub(crate) const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";

/// Map a DAS action string to AssetAction.
///
/// Returns `None` for suppressed actions (sub-account infra ops).
pub(crate) fn das_action_to_asset_action(action: &str) -> Option<AssetAction> {
    match action {
        "confirm_proposal" => Some(AssetAction::Mint),
        "transfer_account"
        | "buy_account"
        | "accept_offer"
        | "fulfill_approval"
        | "bid_expired_account_dutch_auction" => Some(AssetAction::Transfer),
        "recycle_expired_account" => Some(AssetAction::Recycle),
        "renew_account" => Some(AssetAction::Renew),
        "edit_records"
        | "edit_manager"
        | "start_account_sale"
        | "cancel_account_sale"
        | "edit_account_sale"
        | "force_recover_account_status"
        | "lock_account_for_cross_chain"
        | "unlock_account_for_cross_chain" => Some(AssetAction::Update),
        // Sub-account infrastructure — suppress collection activity
        "enable_sub_account"
        | "create_sub_account"
        | "edit_sub_account"
        | "renew_sub_account"
        | "recycle_sub_account"
        | "config_sub_account_custom_script"
        | "config_sub_account"
        | "collect_sub_account_profit"
        | "collect_sub_account_channel_profit" => None,
        _ => {
            // Unknown action — let caller fall back to generic detection
            None
        }
    }
}

/// Resolve .bit collection activity for a single transaction.
///
/// Uses the parsed DAS action to determine the correct `AssetAction`, with
/// neighbor suppression: for `confirm_proposal`, only new accounts (in
/// outputs but NOT inputs) get Mint; for `recycle_expired_account`, only
/// removed accounts (in inputs but NOT outputs) get Recycle.
pub(crate) fn resolve_dotbit_tx_activity(
    das_action: Option<&str>,
    created_account_ids: &HashSet<Vec<u8>>,
    consumed_account_ids: &HashSet<Vec<u8>>,
    tx_hash: &[u8],
    block_number: i64,
    tx_idx: i32,
    timestamp_ms: i64,
    batch: &mut StoreBatch,
) {
    let actions = match das_action.and_then(das_action_to_asset_action) {
        Some(asset_action) => {
            match asset_action {
                AssetAction::Mint => {
                    // confirm_proposal: only truly new accounts (output-only)
                    let new_only: Vec<_> = created_account_ids
                        .iter()
                        .filter(|id| !consumed_account_ids.contains(*id))
                        .collect();
                    if new_only.is_empty() {
                        return;
                    }
                    vec![AssetAction::Mint]
                }
                AssetAction::Recycle => {
                    // recycle_expired_account: only removed accounts (input-only)
                    let removed_only: Vec<_> = consumed_account_ids
                        .iter()
                        .filter(|id| !created_account_ids.contains(*id))
                        .collect();
                    if removed_only.is_empty() {
                        return;
                    }
                    vec![AssetAction::Recycle]
                }
                action => vec![action],
            }
        }
        None if das_action.is_some() => {
            // Known DAS action that was suppressed (sub-account ops) or unknown
            let action_str = das_action.unwrap_or("");
            // Sub-account ops are intentionally suppressed — no warning needed
            if das_action_to_asset_action(action_str).is_none()
                && !action_str.contains("sub_account")
                && !action_str.is_empty()
            {
                warn!(
                    action = action_str,
                    tx_hash = %format!("0x{}", hex::encode(tx_hash)),
                    "unknown DAS action, falling back to generic activity detection"
                );
            }
            // Fall back to generic Create/Consume detection
            resolve_generic_dotbit_actions(created_account_ids, consumed_account_ids)
        }
        None => {
            // No DAS action parsed — fall back to generic detection
            resolve_generic_dotbit_actions(created_account_ids, consumed_account_ids)
        }
    };

    if actions.is_empty() {
        return;
    }

    let entry = NftCollectionActivityEntry {
        tx_hash: tx_hash.to_vec(),
        timestamp_ms,
        actions,
    };

    batch.put_nft_collection_activity(&DOTBIT_SENTINEL_COLLECTION, block_number, tx_idx, &entry);
}

/// Generic fallback: same-account in both → Transfer, output-only → Mint, input-only → Burn.
fn resolve_generic_dotbit_actions(
    created: &HashSet<Vec<u8>>,
    consumed: &HashSet<Vec<u8>>,
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

#[derive(Default)]
pub(crate) struct DotbitBatchState {
    accounts: HashMap<Vec<u8>, Option<NftEntry>>,
    collection_agg_loaded: bool,
    collection_agg: Option<NftCollectionAggregate>,
    hourly_transfers: HashMap<Vec<u8>, i64>,
}

impl DotbitBatchState {
    fn get_account(
        &mut self,
        store: &CkbadgerStore,
        account_id: &[u8],
    ) -> Result<Option<NftEntry>> {
        if let Some(cached) = self.accounts.get(account_id) {
            return Ok(cached.clone());
        }
        let loaded = store.get_nft(account_id)?;
        self.accounts.insert(account_id.to_vec(), loaded.clone());
        Ok(loaded)
    }

    fn put_account(&mut self, account_id: &[u8], entry: NftEntry) {
        self.accounts.insert(account_id.to_vec(), Some(entry));
    }

    fn get_collection_aggregate(
        &mut self,
        store: &CkbadgerStore,
    ) -> Result<Option<NftCollectionAggregate>> {
        if self.collection_agg_loaded {
            return Ok(self.collection_agg.clone());
        }
        let loaded = store.get_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)?;
        self.collection_agg = loaded.clone();
        self.collection_agg_loaded = true;
        Ok(loaded)
    }

    fn put_collection_aggregate(&mut self, agg: NftCollectionAggregate, batch: &mut StoreBatch) {
        batch.put_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION, &agg);
        self.collection_agg = Some(agg);
        self.collection_agg_loaded = true;
    }

    fn get_hourly_transfer(&mut self, store: &CkbadgerStore, key: &[u8]) -> Result<i64> {
        if let Some(cached) = self.hourly_transfers.get(key) {
            return Ok(*cached);
        }
        let loaded = match store.get_stats_key(key)? {
            Some(v) if v.len() == 8 => i64::from_le_bytes(v[..8].try_into().unwrap()),
            _ => 0,
        };
        self.hourly_transfers.insert(key.to_vec(), loaded);
        Ok(loaded)
    }

    fn put_hourly_transfer(&mut self, key: Vec<u8>, count: i64) {
        self.hourly_transfers.insert(key, count);
    }
}

impl BatchWriter {
    pub(crate) fn new_dotbit_batch_state(&self) -> DotbitBatchState {
        DotbitBatchState::default()
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

        let entry = NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(account.account_id.clone()),
            owner_lock_hash: Some(account.owner_lock_hash.clone()),
            name: Some(account_name),
            is_live: true,
            created_at_block: existing
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(block_number),
            extra: NftExtra::DotBit {
                expired_at: account.expired_at,
                registered_at: account.registered_at,
                status: account.status,
            },
        };
        batch.put_nft(&account.account_id, &entry);
        state.put_account(&account.account_id, entry);
        if !was_live {
            batch.put_nft_by_collection(&DOTBIT_SENTINEL_COLLECTION, &account.account_id);
        }

        // Update collection aggregate for new account or account re-activation.
        if existing.is_none() {
            let mut agg = state
                .get_collection_aggregate(self.store.as_ref())?
                .unwrap_or_else(|| NftCollectionAggregate {
                    name: Some(".bit".to_string()),
                    standard: NftStandard::DotBit,
                    ..Default::default()
                });
            agg.total_count += 1;
            agg.live_count += 1;
            state.put_collection_aggregate(agg, batch);
        } else if !was_live {
            let Some(mut agg) = state.get_collection_aggregate(self.store.as_ref())? else {
                bail!(
                    "dotbit collection aggregate missing while re-activating account: account_id=0x{}",
                    hex::encode(&account.account_id)
                );
            };
            agg.live_count += 1;
            state.put_collection_aggregate(agg, batch);
        } else {
            // Re-insert (transfer) — increment hourly bucket for 24h tracking
            let hour_bucket = timestamp_ms / 3_600_000;
            let key = ckbadger_store::keys::encode_nft_hourly_key(
                &DOTBIT_SENTINEL_COLLECTION,
                hour_bucket,
            );
            let current = state.get_hourly_transfer(self.store.as_ref(), &key)?;
            let next = current + 1;
            batch.put_nft_hourly_transfer(&DOTBIT_SENTINEL_COLLECTION, hour_bucket, next);
            state.put_hourly_transfer(key, next);
        }
        batch.put_dotbit_account_outpoint(
            tx_hash,
            account_output.output_index,
            &account.account_id,
        );
        Ok(())
    }

    pub fn consume_dotbit_account(
        &self,
        account_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
    ) -> Result<Option<Vec<u8>>> {
        let mut state = self.new_dotbit_batch_state();
        self.consume_dotbit_account_with_state(
            account_id,
            _block_number,
            _tx_hash,
            batch,
            &mut state,
        )
    }

    /// Consume a .bit account. Returns `Some(DOTBIT_SENTINEL_COLLECTION)` if consumed.
    pub(crate) fn consume_dotbit_account_with_state(
        &self,
        account_id: &[u8],
        _block_number: i64,
        _tx_hash: &[u8],
        batch: &mut StoreBatch,
        state: &mut DotbitBatchState,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(mut entry) = state.get_account(self.store.as_ref(), account_id)? {
            if !entry.is_live {
                bail!(
                    "dotbit account already consumed: account_id=0x{}",
                    hex::encode(account_id)
                );
            }
            entry.is_live = false;
            entry.owner_lock_hash = None;
            batch.put_nft(account_id, &entry);
            state.put_account(account_id, entry);

            // Decrement collection's live_count
            let Some(mut agg) = state.get_collection_aggregate(self.store.as_ref())? else {
                bail!("dotbit collection aggregate missing");
            };
            if agg.live_count <= 0 {
                bail!(
                    "dotbit collection live_count underflow: live_count={}",
                    agg.live_count
                );
            }
            agg.live_count -= 1;
            state.put_collection_aggregate(agg, batch);
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
        Ok(self
            .store
            .get_dotbit_account_ids_by_outpoints_batch(&outpoints))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::BatchWriter;
    use ckbadger_store::store::CkbadgerStore;
    use std::sync::Arc;

    #[test]
    fn test_dotbit_outpoint_lookups_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

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
            .get_nft(&account.account.account_id)
            .unwrap()
            .expect("dotbit nft exists");
        assert_eq!(entry.name.as_deref(), Some("alice.bit"));

        let batch_loaded = writer
            .get_dotbit_account_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[6])
            .unwrap();
        assert_eq!(batch_loaded.len(), 1);
        assert_eq!(batch_loaded[0].0, tx_hash);
        assert_eq!(batch_loaded[0].1, 6);

        let dotbit_collection = b"dotbit_collection_______________";
        let collection_ids = writer
            .store()
            .list_nft_ids_by_collection(dotbit_collection, None, 10)
            .unwrap();
        assert_eq!(collection_ids, vec![account.account.account_id]);
    }

    #[test]
    fn test_consume_dotbit_account_errors_on_live_count_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

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

        let mut agg = writer
            .store()
            .get_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        agg.live_count = 0;
        let mut batch = StoreBatch::new(writer.store());
        batch.put_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION, &agg);
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(writer.store());
        let err = writer
            .consume_dotbit_account(&account.account.account_id, 2, &tx_hash, &mut batch)
            .unwrap_err();
        assert!(err.to_string().contains("live_count underflow"));
    }

    #[test]
    fn test_consume_dotbit_account_errors_on_double_consume() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

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
    fn test_reactivate_dotbit_account_increments_live_count() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

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

        let agg = writer
            .store()
            .get_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 1);
        let entry = writer
            .store()
            .get_nft(&account.account.account_id)
            .unwrap()
            .unwrap();
        assert!(entry.is_live);
    }

    #[test]
    fn test_consume_dotbit_account_reads_uncommitted_insert_from_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let writer = BatchWriter::new(Arc::new(store));

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
            .get_nft(&account.account.account_id)
            .unwrap()
            .unwrap();
        assert!(!entry.is_live);
        let agg = writer
            .store()
            .get_nft_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)
            .unwrap()
            .unwrap();
        assert_eq!(agg.total_count, 1);
        assert_eq!(agg.live_count, 0);
    }
}
