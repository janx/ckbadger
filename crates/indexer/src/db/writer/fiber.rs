//! Fiber channel writer: processes TxActions and updates CF_FIBER_CHANNELS state.
//!
//! Scans protocol_actions for `protocol == "fiber"` and applies lifecycle
//! state transitions (open, close, force_close, settlement) to the channel store.

use anyhow::Result;
use tracing::warn;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{FiberChannel, FiberChannelState, TxActions};
use ckbadger_store::CkbadgerStore;

/// Process Fiber channel lifecycle events from a TxActions.
///
/// Scans TX-level `protocol_actions` for `protocol == "fiber"`,
/// reads the action/metadata, and applies state transitions to
/// CF_FIBER_CHANNELS, CF_ADDR_FIBER_CHANNELS, CF_FIBER_CHANNEL_BY_COMMITMENT,
/// and CF_FIBER_CHANNEL_BY_FUNDING_ARGS.
pub fn process_fiber_channel_events(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    tx_actions: &TxActions,
) -> Result<()> {
    for action in &tx_actions.protocol_actions {
        if action.protocol != "fiber" {
            continue;
        }

        let metadata = action.metadata_value().map_err(|e| {
            anyhow::anyhow!(
                "failed to decode fiber metadata for tx 0x{} action={}: {}",
                hex::encode(&tx_actions.tx_hash),
                action.action,
                e
            )
        })?;

        // Use the first participant's lock_hash as the "first participant" for channel_open.
        let first_participant_lock_hash = tx_actions
            .participants
            .first()
            .map(|p| p.lock_hash.as_slice())
            .unwrap_or(&[]);

        match action.action.as_str() {
            "channel_open" => {
                handle_channel_open(batch, tx_actions, first_participant_lock_hash, &metadata)?;
            }
            "channel_close" => {
                handle_channel_close(batch, store, tx_actions, &metadata)?;
            }
            "force_close" => {
                handle_force_close(batch, store, tx_actions, &metadata)?;
            }
            "settlement" => {
                handle_settlement(batch, store, tx_actions, &metadata)?;
            }
            "commitment_revocation" => {
                handle_commitment_revocation(batch, store, tx_actions, &metadata)?;
            }
            other => {
                warn!(
                    action = other,
                    tx_hash = %hex::encode(&tx_actions.tx_hash),
                    "unknown fiber action, skipping"
                );
            }
        }
    }
    Ok(())
}

/// Handle `channel_open`: create a new FiberChannel with state Open.
fn handle_channel_open(
    batch: &mut StoreBatch<'_>,
    tx_actions: &TxActions,
    first_participant_lock_hash: &[u8],
    metadata: &serde_json::Value,
) -> Result<()> {
    let channel_outpoint_hex = match metadata.get("channelOutpoint").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber channel_open missing channelOutpoint metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
            );
        }
    };

    let capacity_str = match metadata.get("capacity").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber channel_open missing capacity metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
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
                hex::encode(&tx_actions.tx_hash)
            )
        })?,
        None => Vec::new(),
    };

    // Collect participant lock_hashes from tx_actions.participants.
    // With TxActions, protocol_actions are TX-level, so all participants in the
    // transaction are potential channel participants.
    let mut participants: Vec<Vec<u8>> = tx_actions
        .participants
        .iter()
        .map(|p| p.lock_hash.clone())
        .collect();
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
        open_block: tx_actions.block_number,
        open_timestamp: tx_actions.timestamp,
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
    tx_actions: &TxActions,
    metadata: &serde_json::Value,
) -> Result<()> {
    let funding_lock_args_hex = match metadata.get("fundingLockArgs").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber channel_close missing fundingLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
            );
        }
    };

    let funding_lock_args = decode_hex_with_prefix(funding_lock_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber channel_close: failed to decode fundingLockArgs hex '{}' in tx 0x{}",
            funding_lock_args_hex,
            hex::encode(&tx_actions.tx_hash)
        )
    })?;

    let channel_id = match store.get_fiber_channel_id_by_funding_args(&funding_lock_args)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&tx_actions.tx_hash),
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
                tx_hash = %hex::encode(&tx_actions.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber channel_close: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    channel.state = FiberChannelState::CooperativelyClosed;
    channel.close_tx_hash = Some(tx_actions.tx_hash.clone());
    channel.close_block = Some(tx_actions.block_number);
    channel.close_timestamp = Some(tx_actions.timestamp);

    batch.put_fiber_channel(&channel_id, &channel);

    Ok(())
}

/// Handle `force_close`: funding lock consumed + commitment lock output.
fn handle_force_close(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    tx_actions: &TxActions,
    metadata: &serde_json::Value,
) -> Result<()> {
    let funding_lock_args_hex = match metadata.get("fundingLockArgs").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber force_close missing fundingLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
            );
        }
    };

    let funding_lock_args = decode_hex_with_prefix(funding_lock_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber force_close: failed to decode fundingLockArgs hex '{}' in tx 0x{}",
            funding_lock_args_hex,
            hex::encode(&tx_actions.tx_hash)
        )
    })?;

    let channel_id = match store.get_fiber_channel_id_by_funding_args(&funding_lock_args)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&tx_actions.tx_hash),
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
                tx_hash = %hex::encode(&tx_actions.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber force_close: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    channel.state = FiberChannelState::ForceClosed;
    channel.close_tx_hash = Some(tx_actions.tx_hash.clone());
    channel.close_block = Some(tx_actions.block_number);
    channel.close_timestamp = Some(tx_actions.timestamp);

    // Store commitment info if present in metadata
    if let Some(commitment_args_hex) = metadata.get("commitmentLockArgs").and_then(|v| v.as_str()) {
        let commitment_args = decode_hex_with_prefix(commitment_args_hex).ok_or_else(|| {
            anyhow::anyhow!(
                "fiber force_close: failed to decode commitmentLockArgs hex '{}' in tx 0x{}",
                commitment_args_hex,
                hex::encode(&tx_actions.tx_hash)
            )
        })?;
        if !commitment_args.is_empty() {
            // Use blake2b hash of commitment lock args as the commitment index key
            let commitment_hash = blake2b_hash(&commitment_args);
            channel.commitment_tx_hash = Some(tx_actions.tx_hash.clone());
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
    tx_actions: &TxActions,
    metadata: &serde_json::Value,
) -> Result<()> {
    let commitment_args_hex = match metadata.get("commitmentLockArgs").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber settlement missing commitmentLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
            );
        }
    };

    let commitment_args = decode_hex_with_prefix(commitment_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber settlement: failed to decode commitmentLockArgs hex '{}' in tx 0x{}",
            commitment_args_hex,
            hex::encode(&tx_actions.tx_hash)
        )
    })?;
    let commitment_hash = blake2b_hash(&commitment_args);

    let channel_id = match store.get_fiber_channel_id_by_commitment(&commitment_hash)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&tx_actions.tx_hash),
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
                tx_hash = %hex::encode(&tx_actions.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber settlement: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    channel.state = FiberChannelState::Settled;
    channel.settlement_tx_hash = Some(tx_actions.tx_hash.clone());
    channel.settlement_block = Some(tx_actions.block_number);
    channel.settlement_timestamp = Some(tx_actions.timestamp);

    batch.put_fiber_channel(&channel_id, &channel);

    Ok(())
}

/// Handle `commitment_revocation`: commitment-lock input consumed + new commitment-lock output.
/// Rotates the commitment hash index to the new commitment args.
fn handle_commitment_revocation(
    batch: &mut StoreBatch<'_>,
    store: &CkbadgerStore,
    tx_actions: &TxActions,
    metadata: &serde_json::Value,
) -> Result<()> {
    let old_args_hex = match metadata
        .get("oldCommitmentLockArgs")
        .and_then(|v| v.as_str())
    {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber commitment_revocation missing oldCommitmentLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
            );
        }
    };
    let new_args_hex = match metadata
        .get("newCommitmentLockArgs")
        .and_then(|v| v.as_str())
    {
        Some(s) => s,
        None => {
            anyhow::bail!(
                "fiber commitment_revocation missing newCommitmentLockArgs metadata in tx 0x{} — detector bug",
                hex::encode(&tx_actions.tx_hash)
            );
        }
    };

    let old_args = decode_hex_with_prefix(old_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber commitment_revocation: failed to decode oldCommitmentLockArgs hex '{}' in tx 0x{}",
            old_args_hex,
            hex::encode(&tx_actions.tx_hash)
        )
    })?;
    let new_args = decode_hex_with_prefix(new_args_hex).ok_or_else(|| {
        anyhow::anyhow!(
            "fiber commitment_revocation: failed to decode newCommitmentLockArgs hex '{}' in tx 0x{}",
            new_args_hex,
            hex::encode(&tx_actions.tx_hash)
        )
    })?;

    let old_hash = blake2b_hash(&old_args);
    let new_hash = blake2b_hash(&new_args);

    let channel_id = match store.get_fiber_channel_id_by_commitment(&old_hash)? {
        Some(id) => id,
        None => {
            warn!(
                tx_hash = %hex::encode(&tx_actions.tx_hash),
                old_commitment_lock_args = %old_args_hex,
                "fiber commitment_revocation: no channel found for old commitment, skipping"
            );
            return Ok(());
        }
    };

    let mut channel = match store.get_fiber_channel(&channel_id)? {
        Some(ch) => ch,
        None => {
            warn!(
                tx_hash = %hex::encode(&tx_actions.tx_hash),
                channel_id = %hex::encode(&channel_id),
                "fiber commitment_revocation: channel not found by id, skipping"
            );
            return Ok(());
        }
    };

    // Update commitment metadata, keep state as ForceClosed
    channel.commitment_tx_hash = Some(tx_actions.tx_hash.clone());

    // Rotate commitment hash index: remove old, insert new
    batch.delete_fiber_channel_by_commitment(&old_hash);
    batch.put_fiber_channel_by_commitment(&new_hash, &channel_id);
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
    use ckbadger_store::types::{ParticipantDelta, ProtocolAction};
    use tempfile::TempDir;

    fn make_tx_actions(
        tx_hash: &[u8],
        block_number: i64,
        timestamp: i64,
        participants: Vec<ParticipantDelta>,
        protocol_actions: Vec<ProtocolAction>,
    ) -> TxActions {
        TxActions {
            tx_hash: tx_hash.to_vec(),
            block_hash: vec![0xBB; 32],
            block_number,
            tx_index: 1,
            timestamp,
            is_cellbase: false,
            protocol_actions,
            type_calls: vec![],
            lock_calls: vec![],
            participants,
        }
    }

    fn make_participant(lock_hash: &[u8]) -> ParticipantDelta {
        ParticipantDelta {
            lock_hash: lock_hash.to_vec(),
            ckb_delta: 0,
            used_delta: 0,
            item_deltas: vec![],
            tags: 0,
        }
    }

    /// Convenience: build TxActions from a single participant + fiber action.
    /// Replicates the old test pattern of `make_owner_with_fiber_action` + `make_bundle`.
    fn make_tx_actions_with_fiber_action(
        tx_hash: &[u8],
        block_number: i64,
        timestamp: i64,
        lock_hash: &[u8],
        action: &str,
        metadata: serde_json::Value,
    ) -> TxActions {
        make_tx_actions(
            tx_hash,
            block_number,
            timestamp,
            vec![make_participant(lock_hash)],
            vec![ProtocolAction::new("fiber", action, metadata)],
        )
    }

    /// Convenience: build TxActions from multiple participants + fiber action.
    fn make_tx_actions_with_participants_fiber_action(
        tx_hash: &[u8],
        block_number: i64,
        timestamp: i64,
        lock_hashes: &[&[u8]],
        action: &str,
        metadata: serde_json::Value,
    ) -> TxActions {
        let participants = lock_hashes.iter().map(|lh| make_participant(lh)).collect();
        make_tx_actions(
            tx_hash,
            block_number,
            timestamp,
            participants,
            vec![ProtocolAction::new("fiber", action, metadata)],
        )
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
        let actions = make_tx_actions_with_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &participant,
            "channel_open",
            metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &actions).unwrap();
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
        let open_actions = make_tx_actions_with_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &participant,
            "channel_open",
            open_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_actions).unwrap();
        batch.commit().unwrap();

        // Now close the channel
        let close_metadata = serde_json::json!({
            "event": "channel_close",
            "capacity": "50000000000",
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
        });
        let close_actions = make_tx_actions_with_fiber_action(
            &[0x41; 32],
            5001,
            1_700_000_010,
            &participant,
            "channel_close",
            close_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &close_actions).unwrap();
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
        let open_actions = make_tx_actions_with_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &participant,
            "channel_open",
            open_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_actions).unwrap();
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
        let fc_actions = make_tx_actions_with_fiber_action(
            &[0x42; 32],
            5002,
            1_700_000_020,
            &participant,
            "force_close",
            force_close_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &fc_actions).unwrap();
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
        let open_actions = make_tx_actions_with_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &participant,
            "channel_open",
            open_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_actions).unwrap();
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
        let fc_actions = make_tx_actions_with_fiber_action(
            &[0x42; 32],
            5002,
            1_700_000_020,
            &participant,
            "force_close",
            force_close_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &fc_actions).unwrap();
        batch.commit().unwrap();

        // Settlement
        let settlement_metadata = serde_json::json!({
            "event": "settlement",
            "capacity": "50000000000",
            "commitmentLockArgs": format!("0x{}", hex::encode(&commitment_args)),
        });
        let settle_actions = make_tx_actions_with_fiber_action(
            &[0x43; 32],
            5003,
            1_700_000_030,
            &participant,
            "settlement",
            settlement_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &settle_actions).unwrap();
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

        let actions = make_tx_actions(
            &[0x50; 32],
            6000,
            1_700_100_000,
            vec![make_participant(&[0xAA; 32])],
            vec![], // no fiber actions
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &actions).unwrap();
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
        let actions = make_tx_actions_with_fiber_action(
            &[0x41; 32],
            5001,
            1_700_000_010,
            &[0xAA; 32],
            "channel_close",
            close_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        // Should not error, just warn and skip
        process_fiber_channel_events(&mut batch, &store, &actions).unwrap();
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

        let actions = make_tx_actions_with_participants_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &[&participant_a[..], &participant_b[..]],
            "channel_open",
            metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &actions).unwrap();
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

    #[test]
    fn test_commitment_revocation_rotates_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant = [0xAA; 32];

        // Open channel
        let open_metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);
        let open_actions = make_tx_actions_with_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &participant,
            "channel_open",
            open_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_actions).unwrap();
        batch.commit().unwrap();

        // Force close
        let mut commitment_args_v1 = vec![0xDD; 20];
        commitment_args_v1.extend_from_slice(&100u64.to_le_bytes());
        commitment_args_v1.extend_from_slice(&1u64.to_be_bytes());
        commitment_args_v1.extend_from_slice(&[0xE1; 20]);
        commitment_args_v1.push(0x01);

        let force_close_metadata = serde_json::json!({
            "event": "force_close",
            "capacity": "50000000000",
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
            "commitmentLockArgs": format!("0x{}", hex::encode(&commitment_args_v1)),
        });
        let fc_actions = make_tx_actions_with_fiber_action(
            &[0x42; 32],
            5002,
            1_700_000_020,
            &participant,
            "force_close",
            force_close_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &fc_actions).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let hash_v1 = blake2b_hash(&commitment_args_v1);
        assert!(store
            .get_fiber_channel_id_by_commitment(&hash_v1)
            .unwrap()
            .is_some());

        // Commitment revocation: v1 → v2
        let mut commitment_args_v2 = vec![0xDD; 20];
        commitment_args_v2.extend_from_slice(&100u64.to_le_bytes());
        commitment_args_v2.extend_from_slice(&1u64.to_be_bytes());
        commitment_args_v2.extend_from_slice(&[0xE2; 20]); // different settlement_hash
        commitment_args_v2.push(0x01);

        let revocation_metadata = serde_json::json!({
            "event": "commitment_revocation",
            "oldCommitmentLockArgs": format!("0x{}", hex::encode(&commitment_args_v1)),
            "newCommitmentLockArgs": format!("0x{}", hex::encode(&commitment_args_v2)),
        });
        let rev_actions = make_tx_actions_with_fiber_action(
            &[0x44; 32],
            5004,
            1_700_000_040,
            &participant,
            "commitment_revocation",
            revocation_metadata,
        );

        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &rev_actions).unwrap();
        batch.commit().unwrap();

        // Old hash should be gone, new hash should map to the channel
        let hash_v2 = blake2b_hash(&commitment_args_v2);
        assert!(store
            .get_fiber_channel_id_by_commitment(&hash_v1)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_fiber_channel_id_by_commitment(&hash_v2)
                .unwrap()
                .unwrap(),
            channel_id
        );

        // Channel state should remain ForceClosed
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.state, FiberChannelState::ForceClosed);
        assert_eq!(channel.commitment_tx_hash, Some(vec![0x44; 32]));
    }

    #[test]
    fn test_revocation_chain_then_settlement() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let funding_tx_hash = [0x10; 32];
        let funding_lock_args = [0xCC; 20];
        let participant = [0xAA; 32];

        // Open
        let open_metadata =
            make_channel_open_metadata(&funding_tx_hash, 0, 500_00000000, &funding_lock_args);
        let open_actions = make_tx_actions_with_fiber_action(
            &[0x40; 32],
            5000,
            1_700_000_000,
            &participant,
            "channel_open",
            open_metadata,
        );
        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &open_actions).unwrap();
        batch.commit().unwrap();

        // Force close with v1
        let mut args_v1 = vec![0xDD; 20];
        args_v1.extend_from_slice(&100u64.to_le_bytes());
        args_v1.extend_from_slice(&1u64.to_be_bytes());
        args_v1.extend_from_slice(&[0xE1; 20]);
        args_v1.push(0x01);

        let fc_meta = serde_json::json!({
            "event": "force_close",
            "capacity": "50000000000",
            "fundingLockArgs": format!("0x{}", hex::encode(funding_lock_args)),
            "commitmentLockArgs": format!("0x{}", hex::encode(&args_v1)),
        });
        let fc_actions = make_tx_actions_with_fiber_action(
            &[0x42; 32],
            5002,
            1_700_000_020,
            &participant,
            "force_close",
            fc_meta,
        );
        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &fc_actions).unwrap();
        batch.commit().unwrap();

        // Revocation 1: v1 → v2
        let mut args_v2 = vec![0xDD; 20];
        args_v2.extend_from_slice(&100u64.to_le_bytes());
        args_v2.extend_from_slice(&1u64.to_be_bytes());
        args_v2.extend_from_slice(&[0xE2; 20]);
        args_v2.push(0x01);

        let rev1_meta = serde_json::json!({
            "event": "commitment_revocation",
            "oldCommitmentLockArgs": format!("0x{}", hex::encode(&args_v1)),
            "newCommitmentLockArgs": format!("0x{}", hex::encode(&args_v2)),
        });
        let rev1_actions = make_tx_actions_with_fiber_action(
            &[0x44; 32],
            5004,
            1_700_000_040,
            &participant,
            "commitment_revocation",
            rev1_meta,
        );
        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &rev1_actions).unwrap();
        batch.commit().unwrap();

        // Revocation 2: v2 → v3
        let mut args_v3 = vec![0xDD; 20];
        args_v3.extend_from_slice(&100u64.to_le_bytes());
        args_v3.extend_from_slice(&1u64.to_be_bytes());
        args_v3.extend_from_slice(&[0xE3; 20]);
        args_v3.push(0x01);

        let rev2_meta = serde_json::json!({
            "event": "commitment_revocation",
            "oldCommitmentLockArgs": format!("0x{}", hex::encode(&args_v2)),
            "newCommitmentLockArgs": format!("0x{}", hex::encode(&args_v3)),
        });
        let rev2_actions = make_tx_actions_with_fiber_action(
            &[0x45; 32],
            5005,
            1_700_000_050,
            &participant,
            "commitment_revocation",
            rev2_meta,
        );
        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &rev2_actions).unwrap();
        batch.commit().unwrap();

        // Settlement using v3
        let settle_meta = serde_json::json!({
            "event": "settlement",
            "capacity": "50000000000",
            "commitmentLockArgs": format!("0x{}", hex::encode(&args_v3)),
        });
        let settle_actions = make_tx_actions_with_fiber_action(
            &[0x46; 32],
            5006,
            1_700_000_060,
            &participant,
            "settlement",
            settle_meta,
        );
        let mut batch = StoreBatch::new(&store);
        process_fiber_channel_events(&mut batch, &store, &settle_actions).unwrap();
        batch.commit().unwrap();

        let channel_id = keys::encode_fiber_channel_id(&funding_tx_hash, 0);
        let channel = store.get_fiber_channel(&channel_id).unwrap().unwrap();
        assert_eq!(channel.state, FiberChannelState::Settled);
        assert_eq!(channel.settlement_tx_hash, Some(vec![0x46; 32]));
        assert_eq!(channel.settlement_block, Some(5006));

        // Only v3 hash should exist, v1 and v2 should be gone
        let hash_v1 = blake2b_hash(&args_v1);
        let hash_v2 = blake2b_hash(&args_v2);
        let hash_v3 = blake2b_hash(&args_v3);
        assert!(store
            .get_fiber_channel_id_by_commitment(&hash_v1)
            .unwrap()
            .is_none());
        assert!(store
            .get_fiber_channel_id_by_commitment(&hash_v2)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_fiber_channel_id_by_commitment(&hash_v3)
                .unwrap()
                .unwrap(),
            channel_id
        );
    }
}
