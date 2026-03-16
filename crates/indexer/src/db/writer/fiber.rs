//! Fiber channel writer: processes TxActivityBundles and updates CF_FIBER_CHANNELS state.
//!
//! Scans protocol_actions for `protocol == "fiber"` and applies lifecycle
//! state transitions (open, close, force_close, settlement) to the channel store.

use anyhow::Result;
use tracing::warn;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{FiberChannel, FiberChannelState, TxActivityBundle};
use ckbadger_store::CkbadgerStore;

/// Process Fiber channel lifecycle events from a TxActivityBundle.
///
/// Scans each owner's `protocol_actions` for `protocol == "fiber"`,
/// reads the action/metadata, and applies state transitions to
/// CF_FIBER_CHANNELS, CF_ADDR_FIBER_CHANNELS, CF_FIBER_CHANNEL_BY_COMMITMENT,
/// and CF_FIBER_CHANNEL_BY_FUNDING_ARGS.
pub fn process_fiber_channel_events(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    bundle: &TxActivityBundle,
) -> Result<()> {
    for owner in &bundle.owners {
        for action in &owner.protocol_actions {
            if action.protocol != "fiber" {
                continue;
            }

            let metadata = action.metadata_value().map_err(|e| {
                anyhow::anyhow!(
                    "failed to decode fiber metadata for tx 0x{} action={}: {}",
                    hex::encode(&bundle.tx_hash),
                    action.action,
                    e
                )
            })?;

            match action.action.as_str() {
                "channel_open" => {
                    handle_channel_open(batch, bundle, &owner.lock_hash, &metadata)?;
                }
                "channel_close" => {
                    handle_channel_close(batch, store, bundle, &metadata)?;
                }
                "force_close" => {
                    handle_force_close(batch, store, bundle, &metadata)?;
                }
                "settlement" => {
                    handle_settlement(batch, store, bundle, &metadata)?;
                }
                other => {
                    warn!(
                        action = other,
                        tx_hash = %hex::encode(&bundle.tx_hash),
                        "unknown fiber action, skipping"
                    );
                }
            }
        }
    }
    Ok(())
}

/// Handle `channel_open`: create a new FiberChannel with state Open.
fn handle_channel_open(
    batch: &mut StoreBatch<'_>,
    bundle: &TxActivityBundle,
    first_participant_lock_hash: &[u8],
    metadata: &serde_json::Value,
) -> Result<()> {
    let channel_outpoint_hex = match metadata.get("channelOutpoint").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber channel_open missing channelOutpoint metadata in tx 0x{} — detector bug",
                hex::encode(&bundle.tx_hash)
            );
        }
    };

    let capacity_str = match metadata.get("capacity").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber channel_open missing capacity metadata in tx 0x{} — detector bug",
                hex::encode(&bundle.tx_hash)
            );
        }
    };

    let capacity: u64 = capacity_str.parse().map_err(|e| {
        anyhow::anyhow!(
            "fiber channel_open: failed to parse capacity '{}': {}",
            capacity_str,
            e
        )
    })?;

    // Decode the outpoint: strip 0x prefix, decode hex -> 36 bytes (tx_hash 32 + output_index 4 LE)
    let outpoint_bytes = decode_hex_with_prefix(channel_outpoint_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber channel_open: failed to decode channelOutpoint hex: {}",
            channel_outpoint_hex
        )
    })?;
    if outpoint_bytes.len() < 36 {
        anyhow::bail!(
            "fiber channel_open: channelOutpoint too short: expected 36 bytes, got {}",
            outpoint_bytes.len()
        );
    }

    let funding_tx_hash = &outpoint_bytes[..32];
    let funding_output_index =
        u32::from_le_bytes(outpoint_bytes[32..36].try_into().expect("4 bytes for u32"));

    // Compute channel_id
    let channel_id = keys::encode_fiber_channel_id(funding_tx_hash, funding_output_index);

    // Parse funding_lock_args from metadata
    let funding_lock_args = match metadata.get("fundingLockArgs").and_then(|v| v.as_str()) {
        Some(hex_str) => decode_hex_with_prefix(hex_str).ok_or_else(|| {
            anyhow::anyhow!(
                "fiber channel_open: failed to decode fundingLockArgs hex '{}' in tx 0x{}",
                hex_str,
                hex::encode(&bundle.tx_hash)
            )
        })?,
        None => Vec::new(),
    };

    // Collect participant lock_hashes: all non-fiber-lock owners in this bundle.
    // The first_participant_lock_hash is the owner who received this action.
    // We also include all other owners who have the same fiber action.
    let mut participants: Vec<Vec<u8>> = Vec::new();
    for o in &bundle.owners {
        let has_fiber_open = o
            .protocol_actions
            .iter()
            .any(|a| a.protocol == "fiber" && a.action == "channel_open");
        if has_fiber_open {
            participants.push(o.lock_hash.clone());
        }
    }
    // Ensure the first participant is included (should already be, but be safe)
    if !participants
        .iter()
        .any(|p| p == first_participant_lock_hash)
    {
        participants.push(first_participant_lock_hash.to_vec());
    }

    let channel = FiberChannel {
        funding_tx_hash: funding_tx_hash.to_vec(),
        funding_output_index,
        state: FiberChannelState::Open,
        capacity,
        udt_type_hash: None,
        udt_amount: None,
        open_block: bundle.block_number,
        open_timestamp: bundle.timestamp,
        close_tx_hash: None,
        close_block: None,
        close_timestamp: None,
        commitment_tx_hash: None,
        commitment_output_index: None,
        delay_epoch: None,
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: participants.clone(),
        funding_lock_args: funding_lock_args.clone(),
    };

    batch.put_fiber_channel(&channel_id, &channel);

    // Index by funding_lock_args for close/force_close lookups
    if !funding_lock_args.is_empty() {
        batch.put_fiber_channel_by_funding_args(&funding_lock_args, &channel_id);
    }

    // Index by participant address
    for participant in &participants {
        batch.put_addr_fiber_channel(participant, &channel_id);
    }

    Ok(())
}

/// Handle `channel_close`: cooperative close (funding lock consumed, no commitment output).
fn handle_channel_close(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    bundle: &TxActivityBundle,
    metadata: &serde_json::Value,
) -> Result<()> {
    let funding_lock_args_hex = match metadata.get("fundingLockArgs").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber channel_close missing fundingLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&bundle.tx_hash)
            );
        }
    };

    let funding_lock_args = decode_hex_with_prefix(funding_lock_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber channel_close: failed to decode fundingLockArgs hex '{}' in tx 0x{}",
            funding_lock_args_hex,
            hex::encode(&bundle.tx_hash)
        )
    })?;

    let channel_id = match store.get_fiber_channel_id_by_funding_args(&funding_lock_args)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&bundle.tx_hash),
                funding_lock_args = %funding_lock_args_hex,
                "fiber channel_close: no channel found for fundingLockArgs, skipping"
            );
            return Ok(());
        }
    };

    let mut channel = match store.get_fiber_channel(&channel_id)? {
        Some(ch) => ch,
        None => {
            warn!(
                tx_hash = %hex::encode(&bundle.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber channel_close: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    channel.state = FiberChannelState::CooperativelyClosed;
    channel.close_tx_hash = Some(bundle.tx_hash.clone());
    channel.close_block = Some(bundle.block_number);
    channel.close_timestamp = Some(bundle.timestamp);

    batch.put_fiber_channel(&channel_id, &channel);

    Ok(())
}

/// Handle `force_close`: funding lock consumed + commitment lock output.
fn handle_force_close(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    bundle: &TxActivityBundle,
    metadata: &serde_json::Value,
) -> Result<()> {
    let funding_lock_args_hex = match metadata.get("fundingLockArgs").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber force_close missing fundingLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&bundle.tx_hash)
            );
        }
    };

    let funding_lock_args = decode_hex_with_prefix(funding_lock_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber force_close: failed to decode fundingLockArgs hex '{}' in tx 0x{}",
            funding_lock_args_hex,
            hex::encode(&bundle.tx_hash)
        )
    })?;

    let channel_id = match store.get_fiber_channel_id_by_funding_args(&funding_lock_args)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&bundle.tx_hash),
                funding_lock_args = %funding_lock_args_hex,
                "fiber force_close: no channel found for fundingLockArgs, skipping"
            );
            return Ok(());
        }
    };

    let mut channel = match store.get_fiber_channel(&channel_id)? {
        Some(ch) => ch,
        None => {
            warn!(
                tx_hash = %hex::encode(&bundle.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber force_close: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    channel.state = FiberChannelState::ForceClosed;
    channel.close_tx_hash = Some(bundle.tx_hash.clone());
    channel.close_block = Some(bundle.block_number);
    channel.close_timestamp = Some(bundle.timestamp);

    // Store commitment info if present in metadata
    if let Some(commitment_args_hex) = metadata.get("commitmentLockArgs").and_then(|v| v.as_str()) {
        let commitment_args = decode_hex_with_prefix(commitment_args_hex).ok_or_else(|| {
            anyhow::anyhow!(
                "fiber force_close: failed to decode commitmentLockArgs hex '{}' in tx 0x{}",
                commitment_args_hex,
                hex::encode(&bundle.tx_hash)
            )
        })?;
        if !commitment_args.is_empty() {
            // Use blake2b hash of commitment lock args as the commitment index key
            let commitment_hash = blake2b_hash(&commitment_args);
            channel.commitment_tx_hash = Some(bundle.tx_hash.clone());
            batch.put_fiber_channel_by_commitment(&commitment_hash, &channel_id);
        }
    }

    batch.put_fiber_channel(&channel_id, &channel);

    Ok(())
}

/// Handle `settlement`: commitment lock consumed.
fn handle_settlement(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    bundle: &TxActivityBundle,
    metadata: &serde_json::Value,
) -> Result<()> {
    let commitment_args_hex = match metadata.get("commitmentLockArgs").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber settlement missing commitmentLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&bundle.tx_hash)
            );
        }
    };

    let commitment_args = decode_hex_with_prefix(commitment_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber settlement: failed to decode commitmentLockArgs hex '{}' in tx 0x{}",
            commitment_args_hex,
            hex::encode(&bundle.tx_hash)
        )
    })?;
    let commitment_hash = blake2b_hash(&commitment_args);

    let channel_id = match store.get_fiber_channel_id_by_commitment(&commitment_hash)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&bundle.tx_hash),
                commitment_lock_args = %commitment_args_hex,
                "fiber settlement: no channel found for commitment, skipping"
            );
            return Ok(());
        }
    };

    let mut channel = match store.get_fiber_channel(&channel_id)? {
        Some(ch) => ch,
        None => {
            warn!(
                tx_hash = %hex::encode(&bundle.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber settlement: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    channel.state = FiberChannelState::Settled;
    channel.settlement_tx_hash = Some(bundle.tx_hash.clone());
    channel.settlement_block = Some(bundle.block_number);
    channel.settlement_timestamp = Some(bundle.timestamp);

    batch.put_fiber_channel(&channel_id, &channel);

    Ok(())
}

/// Decode a hex string, optionally prefixed with "0x".
fn decode_hex_with_prefix(hex_str: &str) -> Option<Vec<u8>> {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(stripped).ok()
}

/// Compute blake2b-256 hash using CKB personalization.
fn blake2b_hash(data: &[u8]) -> Vec<u8> {
    use ckb_hash::new_blake2b;
    let mut hasher = new_blake2b();
    hasher.update(data);
    let mut hash = vec![0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{OwnerActivityDelta, ProtocolAction};
    use tempfile::TempDir;

    fn make_bundle(
        tx_hash: &[u8],
        block_number: i64,
        timestamp: i64,
        owners: Vec<OwnerActivityDelta>,
    ) -> TxActivityBundle {
        TxActivityBundle {
            tx_hash: tx_hash.to_vec(),
            block_hash: vec![0xBB; 32],
            block_number,
            tx_index: 1,
            timestamp,
            is_cellbase: false,
            owners,
        }
    }

    fn make_owner_with_fiber_action(
        lock_hash: &[u8],
        action: &str,
        metadata: serde_json::Value,
    ) -> OwnerActivityDelta {
        OwnerActivityDelta {
            lock_hash: lock_hash.to_vec(),
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            ckb_delta: 0,
            used_delta: 0,
            has_type_script: false,
            involved_script_code_hashes: vec![],
            asset_changes: vec![],
            type_calls: None,
            lock_calls: None,
            protocol_actions: vec![ProtocolAction::new("fiber", action, metadata)],
            peers: vec![],
        }
    }

    fn make_channel_open_metadata(
        funding_tx_hash: &[u8],
        output_index: u32,
        capacity: u64,
        funding_lock_args: &[u8],
    ) -> serde_json::Value {
        let mut outpoint = Vec::with_capacity(36);
        outpoint.extend_from_slice(funding_tx_hash);
        outpoint.extend_from_slice(&output_index.to_le_bytes());

        serde_json::json!({
            "event": "channel_open",
            "channelOutpoint": format!("0x{}", hex::encode(&outpoint)),
            "capacity": capacity.to_string(),
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
        })
    }

    #[test]
    fn test_channel_open_creates_channel() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant = [0xAA; 32];

        let metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);
        let owner = make_owner_with_fiber_action(&participant, "channel_open", metadata);
        let bundle = make_bundle(&[0x40; 32], 5000, 1_700_000_000, vec![owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &bundle).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.state, FiberChannelState::Open);
        assert_eq!(channel.capacity, 500_00000000);
        assert_eq!(channel.funding_tx_hash, funding_tx_hash.to_vec());
        assert_eq!(channel.funding_output_index, 0);
        assert_eq!(channel.open_block, 5000);
        assert_eq!(channel.funding_lock_args, funding_lock_args.to_vec());
        assert!(channel.participants.contains(&participant.to_vec()));

        // Verify funding_args index
        let looked_up = store
            .get_fiber_channel_id_by_funding_args(&funding_lock_args)
            .unwrap()
            .unwrap();
        assert_eq!(looked_up, channel_id);

        // Verify addr_fiber_channel index
        let addr_channels = store.list_addr_fiber_channels(&participant, 10).unwrap();
        assert_eq!(addr_channels.len(), 1);
        assert_eq!(addr_channels[0].0, channel_id);
    }

    #[test]
    fn test_channel_close_updates_state() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant = [0xAA; 32];

        // First open the channel
        let open_metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);
        let open_owner = make_owner_with_fiber_action(&participant, "channel_open", open_metadata);
        let open_bundle = make_bundle(&[0x40; 32], 5000, 1_700_000_000, vec![open_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_bundle).unwrap();
        batch.commit().unwrap();

        // Now close the channel
        let close_metadata = serde_json::json!({
            "event": "channel_close",
            "capacity": "50000000000",
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
        });
        let close_owner =
            make_owner_with_fiber_action(&participant, "channel_close", close_metadata);
        let close_bundle = make_bundle(&[0x41; 32], 5001, 1_700_000_010, vec![close_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &close_bundle).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.state, FiberChannelState::CooperativelyClosed);
        assert_eq!(channel.close_tx_hash, Some(vec![0x41; 32]));
        assert_eq!(channel.close_block, Some(5001));
    }

    #[test]
    fn test_force_close_updates_state_and_adds_commitment() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant = [0xAA; 32];

        // Open channel
        let open_metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);
        let open_owner = make_owner_with_fiber_action(&participant, "channel_open", open_metadata);
        let open_bundle = make_bundle(&[0x40; 32], 5000, 1_700_000_000, vec![open_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_bundle).unwrap();
        batch.commit().unwrap();

        // Force close
        let mut commitment_args = vec![0xDD; 20]; // pubkey_hash
        commitment_args.extend_from_slice(&100u64.to_le_bytes());
        commitment_args.extend_from_slice(&1u64.to_be_bytes());
        commitment_args.extend_from_slice(&[0xEE; 20]);
        commitment_args.push(0x01);

        let force_close_metadata = serde_json::json!({
            "event": "force_close",
            "capacity": "50000000000",
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
            "commitmentLockArgs": format!("0x{}", hex::encode(&commitment_args)),
        });
        let fc_owner =
            make_owner_with_fiber_action(&participant, "force_close", force_close_metadata);
        let fc_bundle = make_bundle(&[0x42; 32], 5002, 1_700_000_020, vec![fc_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &fc_bundle).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.state, FiberChannelState::ForceClosed);
        assert_eq!(channel.close_tx_hash, Some(vec![0x42; 32]));
        assert_eq!(channel.commitment_tx_hash, Some(vec![0x42; 32]));

        // Verify commitment index
        let commitment_hash = blake2b_hash(&commitment_args);
        let looked_up = store
            .get_fiber_channel_id_by_commitment(&commitment_hash)
            .unwrap()
            .unwrap();
        assert_eq!(looked_up, channel_id);
    }

    #[test]
    fn test_settlement_updates_state() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant = [0xAA; 32];

        // Open channel
        let open_metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);
        let open_owner = make_owner_with_fiber_action(&participant, "channel_open", open_metadata);
        let open_bundle = make_bundle(&[0x40; 32], 5000, 1_700_000_000, vec![open_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_bundle).unwrap();
        batch.commit().unwrap();

        // Force close (to create commitment index)
        let mut commitment_args = vec![0xDD; 20];
        commitment_args.extend_from_slice(&100u64.to_le_bytes());
        commitment_args.extend_from_slice(&1u64.to_be_bytes());
        commitment_args.extend_from_slice(&[0xEE; 20]);
        commitment_args.push(0x01);

        let force_close_metadata = serde_json::json!({
            "event": "force_close",
            "capacity": "50000000000",
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
            "commitmentLockArgs": format!("0x{}", hex::encode(&commitment_args)),
        });
        let fc_owner =
            make_owner_with_fiber_action(&participant, "force_close", force_close_metadata);
        let fc_bundle = make_bundle(&[0x42; 32], 5002, 1_700_000_020, vec![fc_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &fc_bundle).unwrap();
        batch.commit().unwrap();

        // Settlement
        let settlement_metadata = serde_json::json!({
            "event": "settlement",
            "capacity": "50000000000",
            "commitmentLockArgs": format!("0x{}", hex::encode(&commitment_args)),
        });
        let settle_owner =
            make_owner_with_fiber_action(&participant, "settlement", settlement_metadata);
        let settle_bundle = make_bundle(&[0x43; 32], 5003, 1_700_000_030, vec![settle_owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &settle_bundle).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.state, FiberChannelState::Settled);
        assert_eq!(channel.settlement_tx_hash, Some(vec![0x43; 32]));
        assert_eq!(channel.settlement_block, Some(5003));
    }

    #[test]
    fn test_no_fiber_actions_is_noop() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let owner = OwnerActivityDelta {
            lock_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            ckb_delta: 100_00000000,
            used_delta: 0,
            has_type_script: false,
            involved_script_code_hashes: vec![],
            asset_changes: vec![],
            type_calls: None,
            lock_calls: None,
            protocol_actions: vec![], // no fiber actions
            peers: vec![],
        };
        let bundle = make_bundle(&[0x50; 32], 6000, 1_700_100_000, vec![owner]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &bundle).unwrap();
        batch.commit().unwrap();

        // No channels should exist
        let channels = store.list_fiber_channels(10, None, None).unwrap();
        assert!(channels.is_empty());
    }

    #[test]
    fn test_channel_close_missing_channel_skips() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let close_metadata = serde_json::json!({
            "event": "channel_close",
            "capacity": "50000000000",
            "fundingLockArgs": "0xcccccccccccccccccccccccccccccccccccccccc",
        });
        let owner = make_owner_with_fiber_action(&[0xAA; 32], "channel_close", close_metadata);
        let bundle = make_bundle(&[0x41; 32], 5001, 1_700_000_010, vec![owner]);

        let mut batch = StoreBatch::new(&store);
        // Should not error, just warn and skip
        process_fiber_channel_events(&mut batch, &store, &bundle).unwrap();
        batch.commit().unwrap();
    }

    #[test]
    fn test_channel_open_multiple_participants() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant_a = [0xAA; 32];
        let participant_b = [0xBB; 32];

        let metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);

        let owner_a =
            make_owner_with_fiber_action(&participant_a, "channel_open", metadata.clone());
        let owner_b = make_owner_with_fiber_action(&participant_b, "channel_open", metadata);
        let bundle = make_bundle(&[0x40; 32], 5000, 1_700_000_000, vec![owner_a, owner_b]);

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &bundle).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.participants.len(), 2);

        // Both participants should be indexed
        let a_channels = store.list_addr_fiber_channels(&participant_a, 10).unwrap();
        assert_eq!(a_channels.len(), 1);
        let b_channels = store.list_addr_fiber_channels(&participant_b, 10).unwrap();
        assert_eq!(b_channels.len(), 1);
    }
}
