//! Fiber Network protocol detector: identifies payment channel lifecycle events
//! by analyzing funding-lock and commitment-lock transitions.

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use ckbadger_store::{keys, types::ProtocolAction};

use crate::parser::fiber::{is_commitment_lock, is_funding_lock, parse_funding_lock_args};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

/// Fiber lock classification for a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FiberLockType {
    FundingLock,
    CommitmentLock,
    Other,
}

/// Detected Fiber channel event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FiberEvent {
    /// Funding-lock output created, no funding-lock input.
    ChannelOpen,
    /// Funding-lock input consumed, no commitment-lock output.
    ChannelClose,
    /// Funding-lock input consumed + commitment-lock output.
    ForceClose,
    /// Commitment-lock input consumed (final sweep, no new commitment output).
    Settlement,
    /// Commitment-lock input consumed + commitment-lock output (revocation/dispute).
    CommitmentRevocation,
}

/// UDT identity carried by a Fiber funding cell: the funding output's type
/// script hash plus the token amount held in its data.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FundingUdt {
    type_script_hash: Vec<u8>,
    amount: u128,
}

/// Summary of Fiber-related cells in a transaction.
#[derive(Debug, Default)]
struct FiberCellSummary {
    has_funding_input: bool,
    has_funding_output: bool,
    has_commitment_input: bool,
    has_commitment_output: bool,
    /// lock_script_hash of the funding-lock cell (input or output), used to exclude from owners.
    funding_lock_hashes: Vec<Vec<u8>>,
    /// lock_script_hash of the commitment-lock cell, used to exclude from owners.
    commitment_lock_hashes: Vec<Vec<u8>>,
    /// lock_args from the first funding-lock output (for channel_open metadata).
    funding_output_args: Option<Vec<u8>>,
    /// Capacity of the first funding-lock output (for channel_open metadata).
    funding_output_capacity: Option<i64>,
    /// Output index of the first funding-lock output (for channel_open outpoint).
    funding_output_index: Option<usize>,
    /// UDT identity of the first funding-lock output, when it carries a type
    /// script. `None` means the channel is funded with plain CKB.
    funding_output_udt: Option<FundingUdt>,
    /// lock_args from the first funding-lock input (for close/force_close metadata).
    funding_input_args: Option<Vec<u8>>,
    /// Capacity of the first funding-lock input (for close/force_close metadata).
    funding_input_capacity: Option<i64>,
    /// Consumed funding outpoint, which is the canonical channel identity.
    funding_input_outpoint: Option<(Vec<u8>, u32)>,
    /// lock_args from the first commitment-lock input (for settlement metadata).
    commitment_input_args: Option<Vec<u8>>,
    /// Capacity of the first commitment-lock input (for settlement metadata).
    commitment_input_capacity: Option<i64>,
    /// lock_args from the first commitment-lock output (for force_close metadata).
    commitment_output_args: Option<Vec<u8>>,
    /// Output index of the first commitment-lock output (force_close /
    /// commitment_revocation enrichment).
    commitment_output_index: Option<u32>,
    /// Non-fiber owners of this tx, sorted. These are the channel participants
    /// recorded on the channel row — the funding/commitment locks themselves
    /// are never participants.
    participants: BTreeSet<Vec<u8>>,
}

fn classify_lock(code_hash: &[u8]) -> FiberLockType {
    if is_funding_lock(code_hash) {
        FiberLockType::FundingLock
    } else if is_commitment_lock(code_hash) {
        FiberLockType::CommitmentLock
    } else {
        FiberLockType::Other
    }
}

pub(crate) struct FiberDetector;

impl FiberDetector {
    pub fn new(_is_mainnet: bool) -> Self {
        Self
    }

    /// Scan transaction cells and build a summary of Fiber-related cells.
    fn summarize_cells(&self, tx: &TxView<'_>) -> Result<FiberCellSummary> {
        let mut summary = FiberCellSummary::default();

        for input in &tx.inputs {
            match classify_lock(input.lock_code_hash) {
                FiberLockType::FundingLock => {
                    summary.has_funding_input = true;
                    summary
                        .funding_lock_hashes
                        .push(input.lock_script_hash.to_vec());
                    if summary.funding_input_args.is_none() {
                        summary.funding_input_args = Some(input.lock_args.to_vec());
                        summary.funding_input_capacity = Some(input.capacity);
                        summary.funding_input_outpoint =
                            Some((input.previous_tx_hash.to_vec(), input.previous_output_index));
                    }
                }
                FiberLockType::CommitmentLock => {
                    summary.has_commitment_input = true;
                    summary
                        .commitment_lock_hashes
                        .push(input.lock_script_hash.to_vec());
                    if summary.commitment_input_args.is_none() {
                        summary.commitment_input_args = Some(input.lock_args.to_vec());
                        summary.commitment_input_capacity = Some(input.capacity);
                    }
                }
                FiberLockType::Other => {
                    summary.participants.insert(input.lock_script_hash.to_vec());
                }
            }
        }

        for (idx, output) in tx.outputs.iter().enumerate() {
            match classify_lock(output.lock_code_hash) {
                FiberLockType::FundingLock => {
                    summary.has_funding_output = true;
                    summary
                        .funding_lock_hashes
                        .push(output.lock_script_hash.to_vec());
                    if summary.funding_output_args.is_none() {
                        let output_index = u32::try_from(idx).map_err(|_| {
                            anyhow::anyhow!("Fiber funding output_index exceeds u32 range: {}", idx)
                        })?;
                        summary.funding_output_udt = parse_funding_udt(
                            output.type_code_hash,
                            output.type_script_hash,
                            output.data,
                            tx,
                            output_index,
                        )?;
                        summary.funding_output_args = Some(output.lock_args.to_vec());
                        summary.funding_output_capacity = Some(output.capacity);
                        summary.funding_output_index = Some(idx);
                    }
                }
                FiberLockType::CommitmentLock => {
                    summary.has_commitment_output = true;
                    summary
                        .commitment_lock_hashes
                        .push(output.lock_script_hash.to_vec());
                    if summary.commitment_output_args.is_none() {
                        summary.commitment_output_args = Some(output.lock_args.to_vec());
                        summary.commitment_output_index =
                            Some(u32::try_from(idx).map_err(|_| {
                                anyhow::anyhow!(
                                    "Fiber commitment output_index exceeds u32 range: {}",
                                    idx
                                )
                            })?);
                    }
                }
                FiberLockType::Other => {
                    summary
                        .participants
                        .insert(output.lock_script_hash.to_vec());
                }
            }
        }

        Ok(summary)
    }

    /// Determine the Fiber event from the cell summary.
    fn classify_event(&self, summary: &FiberCellSummary) -> Option<FiberEvent> {
        if summary.has_funding_output && !summary.has_funding_input {
            Some(FiberEvent::ChannelOpen)
        } else if summary.has_funding_input && !summary.has_commitment_output {
            Some(FiberEvent::ChannelClose)
        } else if summary.has_funding_input && summary.has_commitment_output {
            Some(FiberEvent::ForceClose)
        } else if summary.has_commitment_input && summary.has_commitment_output {
            Some(FiberEvent::CommitmentRevocation)
        } else if summary.has_commitment_input {
            Some(FiberEvent::Settlement)
        } else {
            None
        }
    }

    /// Build metadata JSON for the detected event.
    fn build_metadata(
        &self,
        event: FiberEvent,
        summary: &FiberCellSummary,
        tx_hash: &[u8],
    ) -> anyhow::Result<serde_json::Value> {
        match event {
            FiberEvent::ChannelOpen => {
                let mut meta = serde_json::Map::new();
                meta.insert("event".to_string(), serde_json::json!("channel_open"));

                if let (Some(ref args), Some(capacity), Some(output_index)) = (
                    &summary.funding_output_args,
                    summary.funding_output_capacity,
                    summary.funding_output_index,
                ) {
                    let output_index_u32 = u32::try_from(output_index).map_err(|_| {
                        anyhow::anyhow!(
                            "Fiber funding output_index exceeds u32 range: {}",
                            output_index
                        )
                    })?;
                    meta.insert(
                        "channelOutpoint".to_string(),
                        serde_json::json!(encode_channel_outpoint(tx_hash, output_index_u32)?),
                    );
                    meta.insert(
                        "capacity".to_string(),
                        serde_json::json!(capacity.to_string()),
                    );
                    // The funding lock args are persisted on the channel row, so
                    // unparseable args are an invariant violation — matching the
                    // bulk reducer, which bails on the same condition.
                    let parsed = parse_funding_lock_args(args).ok_or_else(|| {
                        anyhow::anyhow!(
                            "fiber funding output args invalid: tx=0x{} output_index={} args_len={}",
                            hex::encode(tx_hash),
                            output_index_u32,
                            args.len()
                        )
                    })?;
                    meta.insert(
                        "fundingLockArgs".to_string(),
                        serde_json::json!(format!("0x{}", hex::encode(&parsed.pubkey_hash))),
                    );
                    // UDT-funded channels carry the funding output's type
                    // script hash and token amount; CKB-only channels carry
                    // neither key.
                    if let Some(udt) = &summary.funding_output_udt {
                        meta.insert(
                            "udtTypeHash".to_string(),
                            serde_json::json!(format!("0x{}", hex::encode(&udt.type_script_hash))),
                        );
                        meta.insert(
                            "udtAmount".to_string(),
                            serde_json::json!(udt.amount.to_string()),
                        );
                    }
                    // Channel participants are the tx's non-fiber owners, in
                    // sorted order — the same set and order the bulk reducer
                    // records, so both paths write identical channel rows.
                    meta.insert(
                        "participants".to_string(),
                        serde_json::json!(summary
                            .participants
                            .iter()
                            .map(|p| format!("0x{}", hex::encode(p)))
                            .collect::<Vec<_>>()),
                    );
                }

                Ok(serde_json::Value::Object(meta))
            }
            FiberEvent::ChannelClose => {
                let mut meta = serde_json::Map::new();
                meta.insert("event".to_string(), serde_json::json!("channel_close"));

                if let Some((ref tx_hash, output_index)) = summary.funding_input_outpoint {
                    meta.insert(
                        "channelOutpoint".to_string(),
                        serde_json::json!(encode_channel_outpoint(tx_hash, output_index)?),
                    );
                }

                if let (Some(ref args), Some(capacity)) =
                    (&summary.funding_input_args, summary.funding_input_capacity)
                {
                    meta.insert(
                        "capacity".to_string(),
                        serde_json::json!(capacity.to_string()),
                    );
                    if let Some(parsed) = parse_funding_lock_args(args) {
                        meta.insert(
                            "fundingLockArgs".to_string(),
                            serde_json::json!(format!("0x{}", hex::encode(&parsed.pubkey_hash))),
                        );
                    }
                }

                Ok(serde_json::Value::Object(meta))
            }
            FiberEvent::ForceClose => {
                let mut meta = serde_json::Map::new();
                meta.insert("event".to_string(), serde_json::json!("force_close"));

                if let Some((ref tx_hash, output_index)) = summary.funding_input_outpoint {
                    meta.insert(
                        "channelOutpoint".to_string(),
                        serde_json::json!(encode_channel_outpoint(tx_hash, output_index)?),
                    );
                }

                if let (Some(ref args), Some(capacity)) =
                    (&summary.funding_input_args, summary.funding_input_capacity)
                {
                    meta.insert(
                        "capacity".to_string(),
                        serde_json::json!(capacity.to_string()),
                    );
                    if let Some(parsed) = parse_funding_lock_args(args) {
                        meta.insert(
                            "fundingLockArgs".to_string(),
                            serde_json::json!(format!("0x{}", hex::encode(&parsed.pubkey_hash))),
                        );
                    }
                }

                if let Some(ref commitment_args) = summary.commitment_output_args {
                    meta.insert(
                        "commitmentLockArgs".to_string(),
                        serde_json::json!(format!("0x{}", hex::encode(commitment_args))),
                    );
                }
                if let Some(index) = summary.commitment_output_index {
                    meta.insert(
                        "commitmentOutputIndex".to_string(),
                        serde_json::json!(index),
                    );
                }

                Ok(serde_json::Value::Object(meta))
            }
            FiberEvent::Settlement => {
                let mut meta = serde_json::Map::new();
                meta.insert("event".to_string(), serde_json::json!("settlement"));

                if let (Some(ref args), Some(capacity)) = (
                    &summary.commitment_input_args,
                    summary.commitment_input_capacity,
                ) {
                    meta.insert(
                        "capacity".to_string(),
                        serde_json::json!(capacity.to_string()),
                    );
                    meta.insert(
                        "commitmentLockArgs".to_string(),
                        serde_json::json!(format!("0x{}", hex::encode(args))),
                    );
                }

                Ok(serde_json::Value::Object(meta))
            }
            FiberEvent::CommitmentRevocation => {
                let mut meta = serde_json::Map::new();
                meta.insert(
                    "event".to_string(),
                    serde_json::json!("commitment_revocation"),
                );

                if let Some(ref args) = summary.commitment_input_args {
                    meta.insert(
                        "oldCommitmentLockArgs".to_string(),
                        serde_json::json!(format!("0x{}", hex::encode(args))),
                    );
                }
                if let Some(ref args) = summary.commitment_output_args {
                    meta.insert(
                        "newCommitmentLockArgs".to_string(),
                        serde_json::json!(format!("0x{}", hex::encode(args))),
                    );
                }
                if let Some(index) = summary.commitment_output_index {
                    meta.insert(
                        "newCommitmentOutputIndex".to_string(),
                        serde_json::json!(index),
                    );
                }

                Ok(serde_json::Value::Object(meta))
            }
        }
    }
}

/// Resolve the UDT identity of a Fiber funding output.
///
/// A funding output with no type script funds a plain-CKB channel (`None`).
/// One that carries a type script funds a UDT channel: its identity is the
/// type script hash and the amount held in cell data. Both must be present —
/// a typed funding cell that cannot yield either is an invariant violation and
/// fails immediately rather than being reported as a CKB-only channel.
fn parse_funding_udt(
    type_code_hash: Option<&[u8]>,
    type_script_hash: Option<&[u8]>,
    data: &[u8],
    tx: &TxView<'_>,
    output_index: u32,
) -> Result<Option<FundingUdt>> {
    if type_code_hash.is_none() {
        return Ok(None);
    }

    let Some(type_script_hash) = type_script_hash else {
        bail!(
            "fiber funding output carries a type script with no type_script_hash: \
             block={} tx=0x{} output_index={} type_code_hash=0x{}",
            tx.block_number,
            hex::encode(tx.tx_hash),
            output_index,
            hex::encode(type_code_hash.unwrap_or_default())
        );
    };

    let Some(amount) = crate::parser::fiber::parse_funding_udt_amount(data) else {
        bail!(
            "fiber funding output carries a type script but its data cannot hold a \
             {}-byte little-endian UDT amount: block={} tx=0x{} output_index={} \
             type_script_hash=0x{} data_len={}",
            crate::parser::fiber::FUNDING_UDT_AMOUNT_LEN,
            tx.block_number,
            hex::encode(tx.tx_hash),
            output_index,
            hex::encode(type_script_hash),
            data.len()
        );
    };

    Ok(Some(FundingUdt {
        type_script_hash: type_script_hash.to_vec(),
        amount,
    }))
}

fn encode_channel_outpoint(tx_hash: &[u8], output_index: u32) -> anyhow::Result<String> {
    if tx_hash.len() != 32 {
        anyhow::bail!(
            "Fiber channel outpoint tx hash must be 32 bytes, got {}",
            tx_hash.len()
        );
    }
    Ok(format!(
        "0x{}",
        hex::encode(keys::encode_fiber_outpoint(tx_hash, output_index))
    ))
}

impl ProtocolDetector for FiberDetector {
    fn might_apply_batch(
        &self,
        lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
        _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
    ) -> bool {
        lock_code_hashes
            .iter()
            .any(|h| classify_lock(h) != FiberLockType::Other)
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.inputs
            .iter()
            .any(|input| classify_lock(input.lock_code_hash) != FiberLockType::Other)
            || tx
                .outputs
                .iter()
                .any(|output| classify_lock(output.lock_code_hash) != FiberLockType::Other)
    }

    /// Fiber channel lifecycle events are properties of the TRANSACTION, not of
    /// any single owner: a force close is a funding-lock input becoming a
    /// commitment-lock output, and in the normal case those fiber locks are the
    /// tx's ONLY owners. Emitting per-owner and skipping fiber-lock owners
    /// therefore dropped the event entirely for exactly the shapes that matter.
    ///
    /// The event is emitted for every owner instead, carrying owner-independent
    /// metadata; `dedup_protocol_actions` collapses the identical copies into
    /// the single tx-level action. Channel participants are named inside the
    /// metadata (the non-fiber owners), so the fiber locks never become
    /// participants of the channel they implement.
    fn detect(
        &self,
        tx: &TxView<'_>,
        _owner_lock_hash: &[u8],
        _accum: &OwnerAccum<'_>,
        _item_deltas: &[ckbadger_store::types::ItemDelta],
        _type_calls: &[ckbadger_store::types::TypeCallEntry],
        _lock_calls: &[ckbadger_store::types::LockCallEntry],
    ) -> Result<Vec<ProtocolAction>> {
        let summary = self.summarize_cells(tx)?;

        let Some(event) = self.classify_event(&summary) else {
            return Ok(vec![]);
        };

        let metadata = self.build_metadata(event, &summary, tx.tx_hash)?;

        let action = match event {
            FiberEvent::ChannelOpen => "channel_open",
            FiberEvent::ChannelClose => "channel_close",
            FiberEvent::ForceClose => "force_close",
            FiberEvent::Settlement => "settlement",
            FiberEvent::CommitmentRevocation => "commitment_revocation",
        };

        Ok(vec![ProtocolAction::new("fiber", action, metadata)])
    }
}

#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use super::*;
    use crate::db::writer::activities::{build_tx_actions_for_block, OutputCellView, TxView};
    use crate::parser::fiber::{
        COMMITMENT_LOCK_CODE_HASH_MAINNET, FUNDING_LOCK_CODE_HASH_MAINNET,
        FUNDING_LOCK_CODE_HASH_TESTNET,
    };
    use crate::rpc::parse_hex_to_bytes;
    use ckbadger_store::types::TAG_PROTOCOL;

    struct OwnedInput {
        previous_tx_hash: Vec<u8>,
        previous_output_index: u32,
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        capacity: i64,
        data: Vec<u8>,
    }

    impl OwnedInput {
        fn view(&self) -> crate::db::writer::activities::InputCellView<'_> {
            crate::db::writer::activities::InputCellView {
                previous_tx_hash: &self.previous_tx_hash,
                previous_output_index: self.previous_output_index,
                lock_script_hash: &self.lock_script_hash,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                capacity: self.capacity,
                occupied_capacity: 61_00000000,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: Some(1),
                type_script_hash: None,
                type_args: self.type_args.as_deref(),
                udt_amount: None,
                bit_cell_identity_id: None,
                data: &self.data,
                is_dao_withdraw_request: false,
                dao_compensation: None,
            }
        }
    }

    fn make_input_with_lock(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> OwnedInput {
        OwnedInput {
            previous_tx_hash: vec![lock_hash_byte; 32],
            previous_output_index: 0,
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_args,
            capacity,
            type_code_hash,
            type_args,
            data: vec![],
        }
    }

    struct OwnedOutput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        capacity: i64,
        data: Vec<u8>,
    }

    impl OwnedOutput {
        fn view(&self) -> OutputCellView<'_> {
            OutputCellView {
                capacity: self.capacity,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                lock_script_hash: &self.lock_script_hash,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: Some(1),
                type_args: self.type_args.as_deref(),
                type_script_hash: self.type_script_hash.as_deref(),
                data_hash: &[],
                data_size: 0,
                data: &self.data,
            }
        }
    }

    fn make_output_with_lock(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> OwnedOutput {
        let type_script_hash = type_code_hash.as_ref().map(|code_hash| {
            crate::parser::script::ScriptParser::compute_script_hash_raw(
                code_hash,
                1,
                type_args.as_deref().unwrap_or(&[]),
            )
        });
        OwnedOutput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_args,
            capacity,
            type_code_hash,
            type_args,
            type_script_hash,
            data: vec![],
        }
    }

    #[test]
    fn test_channel_open() {
        // Funding-lock output, no funding-lock input -> channel_open
        let funding_code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let funding_owner: u8 = 0xF0; // funding lock owner (should NOT get action)
        let participant: u8 = 0xAA; // regular participant (SHOULD get action)

        let funding_args = vec![0xBB; 20]; // pubkey_hash

        // participant sends CKB, funding-lock output created
        let input = make_input_with_lock(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            None,
            None,
        );

        let outputs = vec![
            make_output_with_lock(
                funding_owner,
                funding_code_hash.clone(),
                funding_args.clone(),
                145_00000000,
                None,
                None,
            ),
            make_output_with_lock(
                participant,
                standard_lock,
                vec![0x22; 20],
                55_00000000,
                None,
                None,
            ),
        ];

        let tx_hash = [0x40; 32];
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xC0; 32],
            tx_index: 1,
            block_number: 5000,
            timestamp: 1_700_200_000,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        // Protocol actions are TX-level — check for channel_open
        let open_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == "channel_open")
            .expect("should have fiber channel_open action");

        // Participant should have TAG_PROTOCOL
        let participant_p = actions
            .participants
            .iter()
            .find(|p| p.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert!(participant_p.tags & TAG_PROTOCOL != 0);

        // Verify metadata
        let meta = open_action.metadata_value().unwrap();
        assert_eq!(meta["event"], "channel_open");
        assert!(meta["channelOutpoint"].as_str().unwrap().starts_with("0x"));
        assert_eq!(meta["capacity"], "14500000000");
        assert_eq!(
            meta["fundingLockArgs"],
            format!("0x{}", hex::encode(&funding_args))
        );

        // Verify outpoint encoding: tx_hash + output_index(0) as LE u32
        let expected_outpoint = {
            let mut buf = Vec::with_capacity(36);
            buf.extend_from_slice(&tx_hash);
            buf.extend_from_slice(&0u32.to_le_bytes());
            format!("0x{}", hex::encode(&buf))
        };
        assert_eq!(meta["channelOutpoint"], expected_outpoint);

        // Funding lock owner should still be a participant (but protocol_actions are TX-level)
        assert!(actions
            .participants
            .iter()
            .any(|p| p.lock_hash == vec![funding_owner; 32]));
    }

    #[test]
    fn test_channel_close() {
        // Funding-lock input consumed, no commitment-lock output -> channel_close
        let funding_code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let funding_owner: u8 = 0xF0;
        let participant: u8 = 0xAA;

        let funding_args = vec![0xBB; 20];

        // funding-lock input consumed
        let input = make_input_with_lock(
            funding_owner,
            funding_code_hash.clone(),
            funding_args.clone(),
            145_00000000,
            None,
            None,
        );

        // output goes to participant (standard lock, no commitment lock)
        let outputs = vec![make_output_with_lock(
            participant,
            standard_lock,
            vec![0x22; 20],
            145_00000000,
            None,
            None,
        )];

        let tx = TxView {
            tx_hash: &[0x41; 32],
            block_hash: &[0xC1; 32],
            tx_index: 1,
            block_number: 5001,
            timestamp: 1_700_200_010,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let close_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == "channel_close")
            .expect("should have fiber channel_close action");
        let meta = close_action.metadata_value().unwrap();
        assert_eq!(meta["event"], "channel_close");
        assert_eq!(
            meta["channelOutpoint"],
            encode_channel_outpoint(&[funding_owner; 32], 0).unwrap()
        );
        assert_eq!(meta["capacity"], "14500000000");
        assert_eq!(
            meta["fundingLockArgs"],
            format!("0x{}", hex::encode(&funding_args))
        );

        // Funding lock owner should still be a participant
        assert!(actions
            .participants
            .iter()
            .any(|p| p.lock_hash == vec![funding_owner; 32]));
    }

    #[test]
    fn test_force_close() {
        // Funding-lock input consumed + commitment-lock output -> force_close
        let funding_code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let commitment_code_hash = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let funding_owner: u8 = 0xF0;
        let commitment_owner: u8 = 0xF1;
        let participant: u8 = 0xAA;

        let funding_args = vec![0xBB; 20];
        // commitment lock args: 20B pubkey_hash + 8B delay_epoch + 8B version + 20B settlement_hash + 1B flag = 57B
        let mut commitment_args = vec![0xCC; 20]; // pubkey_hash
        commitment_args.extend_from_slice(&100u64.to_le_bytes()); // delay_epoch
        commitment_args.extend_from_slice(&1u64.to_be_bytes()); // version
        commitment_args.extend_from_slice(&[0xDD; 20]); // settlement_hash
        commitment_args.push(0x01); // settlement_flag

        // funding-lock input consumed
        let input = make_input_with_lock(
            funding_owner,
            funding_code_hash,
            funding_args.clone(),
            145_00000000,
            None,
            None,
        );

        // commitment-lock output created + some CKB back to participant
        let outputs = vec![
            make_output_with_lock(
                commitment_owner,
                commitment_code_hash,
                commitment_args.clone(),
                100_00000000,
                None,
                None,
            ),
            make_output_with_lock(
                participant,
                standard_lock,
                vec![0x22; 20],
                45_00000000,
                None,
                None,
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x42; 32],
            block_hash: &[0xC2; 32],
            tx_index: 1,
            block_number: 5002,
            timestamp: 1_700_200_020,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let force_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == "force_close")
            .expect("should have fiber force_close action");
        let meta = force_action.metadata_value().unwrap();
        assert_eq!(meta["event"], "force_close");
        assert_eq!(
            meta["channelOutpoint"],
            encode_channel_outpoint(&[funding_owner; 32], 0).unwrap()
        );
        assert_eq!(meta["capacity"], "14500000000");
        assert_eq!(
            meta["fundingLockArgs"],
            format!("0x{}", hex::encode(&funding_args))
        );
        assert_eq!(
            meta["commitmentLockArgs"],
            format!("0x{}", hex::encode(&commitment_args))
        );

        // All participants should be present
        assert!(actions
            .participants
            .iter()
            .any(|p| p.lock_hash == vec![funding_owner; 32]));
        assert!(actions
            .participants
            .iter()
            .any(|p| p.lock_hash == vec![commitment_owner; 32]));
    }

    #[test]
    fn test_settlement() {
        // Commitment-lock input consumed -> settlement
        let commitment_code_hash = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let commitment_owner: u8 = 0xF1;
        let participant: u8 = 0xAA;

        let mut commitment_args = vec![0xCC; 20]; // pubkey_hash
        commitment_args.extend_from_slice(&100u64.to_le_bytes());
        commitment_args.extend_from_slice(&1u64.to_be_bytes());
        commitment_args.extend_from_slice(&[0xDD; 20]);
        commitment_args.push(0x01);

        // commitment-lock input consumed
        let input = make_input_with_lock(
            commitment_owner,
            commitment_code_hash,
            commitment_args.clone(),
            100_00000000,
            None,
            None,
        );

        // output goes to participant
        let outputs = vec![make_output_with_lock(
            participant,
            standard_lock,
            vec![0x22; 20],
            100_00000000,
            None,
            None,
        )];

        let tx = TxView {
            tx_hash: &[0x43; 32],
            block_hash: &[0xC3; 32],
            tx_index: 1,
            block_number: 5003,
            timestamp: 1_700_200_030,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let settle_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == "settlement")
            .expect("should have fiber settlement action");
        let meta = settle_action.metadata_value().unwrap();
        assert_eq!(meta["event"], "settlement");
        assert_eq!(meta["capacity"], "10000000000");
        assert_eq!(
            meta["commitmentLockArgs"],
            format!("0x{}", hex::encode(&commitment_args))
        );

        // Commitment lock owner should still be a participant
        assert!(actions
            .participants
            .iter()
            .any(|p| p.lock_hash == vec![commitment_owner; 32]));
    }

    /// Splice shape: one tx CONSUMES a funding-lock cell and CREATES a new one.
    /// This is a real Fiber operation, not malformed data.
    ///
    /// `classify_event` checks `has_funding_input && !has_commitment_output`
    /// before anything else, so a splice classifies as a plain `channel_close`
    /// on the OLD funding outpoint and nothing is ever recorded for the NEW
    /// funding cell. Full splice support (emitting Close + Open from one tx) is
    /// future work; this test pins today's classification so the follow-on
    /// behaviour is explicit rather than accidental.
    ///
    /// The consequence: when that new funding cell is later spent, the close
    /// names a channel with no indexed open. Both sync paths now treat that as
    /// an invariant violation and fail fast — the audit found zero splices in
    /// the whole history of either network, and the bulk reducer has always
    /// halted here, so a silent skip in live sync could only mask a real bug.
    #[test]
    fn test_splice_classifies_as_close_and_new_funding_spend_fails_fast() {
        use crate::db::writer::fiber::apply_fiber_channel_events;
        use ckbadger_store::batch::StoreBatch;
        use ckbadger_store::CkbadgerStore;

        let funding_code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];
        let old_funding_owner: u8 = 0xF0;
        let new_funding_owner: u8 = 0xF2;
        let participant: u8 = 0xAA;
        let funding_args = vec![0xBB; 20];

        // Funding-lock input (old channel) + funding-lock output (new channel),
        // plus the participant topping the channel up — a splice.
        let inputs = vec![
            make_input_with_lock(
                old_funding_owner,
                funding_code_hash.clone(),
                funding_args.clone(),
                145_00000000,
                None,
                None,
            ),
            make_input_with_lock(
                participant,
                standard_lock.clone(),
                vec![0x22; 20],
                100_00000000,
                None,
                None,
            ),
        ];
        let outputs = vec![
            make_output_with_lock(
                new_funding_owner,
                funding_code_hash,
                funding_args,
                200_00000000,
                None,
                None,
            ),
            make_output_with_lock(
                participant,
                standard_lock,
                vec![0x22; 20],
                45_00000000,
                None,
                None,
            ),
        ];

        let splice_tx_hash = [0x47; 32];
        let tx = TxView {
            tx_hash: &splice_tx_hash,
            block_hash: &[0xC7; 32],
            tx_index: 1,
            block_number: 5007,
            timestamp: 1_700_200_070,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();
        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        // Current classification: a single channel_close on the OLD outpoint.
        let fiber_actions: Vec<_> = actions
            .protocol_actions
            .iter()
            .filter(|a| a.protocol == "fiber")
            .collect();
        assert_eq!(fiber_actions.len(), 1);
        assert_eq!(fiber_actions[0].action, "channel_close");
        assert_eq!(
            fiber_actions[0].metadata_value().unwrap()["channelOutpoint"],
            encode_channel_outpoint(&[old_funding_owner; 32], 0).unwrap()
        );
        // No channel_open is emitted for the new funding cell.
        assert!(!fiber_actions.iter().any(|a| a.action == "channel_open"));

        // The new funding cell therefore has no channel record. Spending it
        // later yields a close for an unknown channel: fatal, same as bulk.
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let spend_metadata = serde_json::json!({
            "event": "channel_close",
            "channelOutpoint": encode_channel_outpoint(&splice_tx_hash, 0).unwrap(),
            "capacity": "14500000000",
            "fundingLockArgs": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        });
        let spend_actions = ckbadger_store::types::TxActions {
            tx_hash: vec![0x48; 32],
            block_hash: vec![0xC8; 32],
            block_number: 5008,
            tx_index: 1,
            timestamp: 1_700_200_080,
            is_cellbase: false,
            protocol_actions: vec![ProtocolAction::new(
                "fiber",
                "channel_close",
                spend_metadata,
            )],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![],
        };

        let mut batch = StoreBatch::new(&store);
        let error = apply_fiber_channel_events(&mut batch, &spend_actions)
            .expect_err("a close naming a channel with no indexed open must fail fast");
        let message = error.to_string();
        assert!(
            message.contains("references a channel with no indexed open"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("5008"),
            "error must carry the block number: {message}"
        );
        assert!(store
            .list_fiber_channels(10, None, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_no_fiber_action_for_standard_locks_only() {
        // No fiber locks in tx -> no fiber protocol actions
        let standard_lock = vec![0x11; 32];
        let alice: u8 = 0xAA;
        let bob: u8 = 0xBB;

        let input = make_input_with_lock(
            alice,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            None,
            None,
        );

        let outputs = vec![make_output_with_lock(
            bob,
            standard_lock,
            vec![0x33; 20],
            200_00000000,
            None,
            None,
        )];

        let tx = TxView {
            tx_hash: &[0x44; 32],
            block_hash: &[0xC4; 32],
            tx_index: 1,
            block_number: 5004,
            timestamp: 1_700_200_040,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        assert!(
            actions_list[0].protocol_actions.is_empty(),
            "no fiber actions expected for standard-only tx"
        );
    }

    #[test]
    fn test_channel_open_outpoint_encoding() {
        // Verify the outpoint is tx_hash + LE u32 output_index
        let funding_code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let funding_owner: u8 = 0xF0;
        let participant: u8 = 0xAA;
        let funding_args = vec![0xBB; 20];

        let input = make_input_with_lock(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            None,
            None,
        );

        // Put funding output at index 1 (after a change output)
        let outputs = vec![
            make_output_with_lock(
                participant,
                standard_lock,
                vec![0x22; 20],
                55_00000000,
                None,
                None,
            ),
            make_output_with_lock(
                funding_owner,
                funding_code_hash,
                funding_args,
                145_00000000,
                None,
                None,
            ),
        ];

        let tx_hash = [0x50; 32];
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xD0; 32],
            tx_index: 1,
            block_number: 6000,
            timestamp: 1_700_300_000,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        let actions = &actions_list[0];
        let open_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == "channel_open")
            .expect("should have fiber channel_open action");
        let meta = open_action.metadata_value().unwrap();
        // output_index = 1 as LE u32 -> [0x01, 0x00, 0x00, 0x00]
        let expected_outpoint = {
            let mut buf = Vec::with_capacity(36);
            buf.extend_from_slice(&tx_hash);
            buf.extend_from_slice(&1u32.to_le_bytes());
            format!("0x{}", hex::encode(&buf))
        };
        assert_eq!(meta["channelOutpoint"], expected_outpoint);
    }

    #[test]
    fn test_classify_event_priority() {
        // When both funding input AND funding output exist (unlikely but possible),
        // the logic should check funding_input + commitment_output first.
        // funding input + no commitment output -> channel_close
        let detector = FiberDetector::new(true);

        let summary_close = FiberCellSummary {
            has_funding_input: true,
            has_funding_output: true, // unusual: also has funding output
            has_commitment_input: false,
            has_commitment_output: false,
            ..Default::default()
        };
        // funding input consumed, no commitment output => channel_close
        assert_eq!(
            detector.classify_event(&summary_close),
            Some(FiberEvent::ChannelClose)
        );

        let summary_force = FiberCellSummary {
            has_funding_input: true,
            has_funding_output: false,
            has_commitment_input: false,
            has_commitment_output: true,
            ..Default::default()
        };
        assert_eq!(
            detector.classify_event(&summary_force),
            Some(FiberEvent::ForceClose)
        );
    }

    #[test]
    fn test_commitment_revocation() {
        // Commitment-lock input consumed + commitment-lock output -> commitment_revocation
        let commitment_code_hash = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let commitment_owner_in: u8 = 0xF1;
        let commitment_owner_out: u8 = 0xF2;
        let participant: u8 = 0xAA;

        let mut old_commitment_args = vec![0xCC; 20];
        old_commitment_args.extend_from_slice(&100u64.to_le_bytes());
        old_commitment_args.extend_from_slice(&1u64.to_be_bytes());
        old_commitment_args.extend_from_slice(&[0xDD; 20]);
        old_commitment_args.push(0x01);

        let mut new_commitment_args = vec![0xCC; 20];
        new_commitment_args.extend_from_slice(&100u64.to_le_bytes());
        new_commitment_args.extend_from_slice(&1u64.to_be_bytes());
        new_commitment_args.extend_from_slice(&[0xEE; 20]); // different settlement_hash
        new_commitment_args.push(0x01);

        // commitment-lock input consumed (old commitment)
        let input_commitment = make_input_with_lock(
            commitment_owner_in,
            commitment_code_hash.clone(),
            old_commitment_args.clone(),
            100_00000000,
            None,
            None,
        );
        // fee input from participant
        let input_fee = make_input_with_lock(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            10_00000000,
            None,
            None,
        );

        // commitment-lock output created (new commitment) + change
        let outputs = vec![
            make_output_with_lock(
                commitment_owner_out,
                commitment_code_hash,
                new_commitment_args.clone(),
                100_00000000,
                None,
                None,
            ),
            make_output_with_lock(
                participant,
                standard_lock,
                vec![0x22; 20],
                9_00000000,
                None,
                None,
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x45; 32],
            block_hash: &[0xC5; 32],
            tx_index: 1,
            block_number: 5005,
            timestamp: 1_700_200_050,
            is_cellbase: false,
            inputs: vec![input_commitment.view(), input_fee.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let revoke_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == "commitment_revocation")
            .expect("should have fiber commitment_revocation action");
        let meta = revoke_action.metadata_value().unwrap();
        assert_eq!(meta["event"], "commitment_revocation");
        assert_eq!(
            meta["oldCommitmentLockArgs"],
            format!("0x{}", hex::encode(&old_commitment_args))
        );
        assert_eq!(
            meta["newCommitmentLockArgs"],
            format!("0x{}", hex::encode(&new_commitment_args))
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Real captured chain vectors (fetched from local mainnet/testnet CKB
    // nodes on 2026-08-03; hermetic constants, no network in tests).
    // All lock/type scripts in these vectors use hash_type "type" (= 1).
    // ═══════════════════════════════════════════════════════════════════

    const SECP_CODE_HASH: &str =
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";

    /// Build an input fixture from a real captured cell, computing the lock
    /// script hash the same way the cell parser does.
    #[allow(clippy::too_many_arguments)]
    fn real_input(
        prev_tx_hash: &str,
        prev_index: u32,
        capacity: i64,
        lock_code_hash: &str,
        lock_args: &str,
        type_script: Option<(&str, &str)>,
        data: &str,
    ) -> OwnedInput {
        let lock_code_hash = parse_hex_to_bytes(lock_code_hash);
        let lock_args = parse_hex_to_bytes(lock_args);
        let lock_script_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &lock_code_hash,
            1,
            &lock_args,
        );
        let (type_code_hash, type_args) = match type_script {
            Some((code_hash, args)) => (
                Some(parse_hex_to_bytes(code_hash)),
                Some(parse_hex_to_bytes(args)),
            ),
            None => (None, None),
        };
        OwnedInput {
            previous_tx_hash: parse_hex_to_bytes(prev_tx_hash),
            previous_output_index: prev_index,
            lock_script_hash,
            lock_code_hash,
            lock_args,
            capacity,
            type_code_hash,
            type_args,
            data: parse_hex_to_bytes(data),
        }
    }

    /// Build an output fixture from a real captured cell, computing lock and
    /// type script hashes the same way the cell parser does.
    fn real_output(
        capacity: i64,
        lock_code_hash: &str,
        lock_args: &str,
        type_script: Option<(&str, &str)>,
        data: &str,
    ) -> OwnedOutput {
        let lock_code_hash = parse_hex_to_bytes(lock_code_hash);
        let lock_args = parse_hex_to_bytes(lock_args);
        let lock_script_hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
            &lock_code_hash,
            1,
            &lock_args,
        );
        let (type_code_hash, type_args, type_script_hash) = match type_script {
            Some((code_hash, args)) => {
                let code_hash = parse_hex_to_bytes(code_hash);
                let args = parse_hex_to_bytes(args);
                let hash = crate::parser::script::ScriptParser::compute_script_hash_raw(
                    &code_hash, 1, &args,
                );
                (Some(code_hash), Some(args), Some(hash))
            }
            None => (None, None, None),
        };
        OwnedOutput {
            lock_script_hash,
            lock_code_hash,
            lock_args,
            type_code_hash,
            type_args,
            type_script_hash,
            capacity,
            data: parse_hex_to_bytes(data),
        }
    }

    fn fiber_detect_mainnet(tx: TxView<'_>) -> ckbadger_store::types::TxActions {
        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let mut list = build_tx_actions_for_block(&[tx], &detectors).unwrap();
        assert_eq!(list.len(), 1);
        list.remove(0)
    }

    fn find_fiber_action<'a>(
        actions: &'a ckbadger_store::types::TxActions,
        action: &str,
    ) -> &'a ProtocolAction {
        actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "fiber" && a.action == action)
            .unwrap_or_else(|| {
                panic!(
                    "expected fiber {action} action, got {:?}",
                    actions.protocol_actions
                )
            })
    }

    /// Bug 1 red test (audit B2): USDI-funded channel open, testnet tx
    /// 0x4d49307f8d0572947e53bfbf35b06ce9c56a4affa43eef1a3ded311b67e28e4c
    /// (block 20565583, tx_index 1). The funding output carries the USDI type
    /// script and a 16-byte LE amount; the emitted channel_open metadata must
    /// carry the funding output's type script hash and amount — and must NOT
    /// pick up the USDI change output (index 1) that also carries the type.
    #[test]
    fn test_channel_open_udt_funded_real_vector_carries_udt_metadata() {
        const OPEN_TX_HASH: &str =
            "0x4d49307f8d0572947e53bfbf35b06ce9c56a4affa43eef1a3ded311b67e28e4c";
        const USDI_CODE_HASH: &str =
            "0xcc9dc33ef234e14bc788c43a4848556a5fb16401a04662fc55db9bb201987037";
        const USDI_ARGS: &str =
            "0x71fd1985b2971a9903e4d8ed0d59e6710166985217ca0681437883837b86162f";
        // blake2b-256(molecule(Script{USDI_CODE_HASH, type, USDI_ARGS})), ckb personalization.
        const USDI_TYPE_SCRIPT_HASH: &str =
            "0x07ac97b5ff3df4b49f59a59f4d80d33d22c1263a57467c512c93b9c29b7a0de3";

        // Cross-check the pinned hash against the standard computation.
        assert_eq!(
            crate::parser::script::ScriptParser::compute_script_hash_raw(
                &parse_hex_to_bytes(USDI_CODE_HASH),
                1,
                &parse_hex_to_bytes(USDI_ARGS),
            ),
            parse_hex_to_bytes(USDI_TYPE_SCRIPT_HASH)
        );

        let usdi = Some((USDI_CODE_HASH, USDI_ARGS));
        let inputs = vec![
            real_input(
                "0x7f76488c606aadf8f9cae974336c9c81a97829618b7fccb14d4097bf4ca4a3c5",
                1,
                14_200_000_000,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                usdi,
                "0xcec99a3b000000000000000000000000",
            ),
            real_input(
                "0x7f76488c606aadf8f9cae974336c9c81a97829618b7fccb14d4097bf4ca4a3c5",
                2,
                9_981_999_999_031,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                None,
                "0x",
            ),
            real_input(
                "0x45533ceb6acc9f9845b3c749a5882ea72e934460e9f1c459cd80c76573fac338",
                0,
                14_200_000_000,
                SECP_CODE_HASH,
                "0x2c71fb9e1c558782f6ca013fd7e8612d98990177",
                usdi,
                "0x00ca9a3b000000000000000000000000",
            ),
            real_input(
                "0x6ac8f0ad3c2408d63e2bbe756bd058c21f725f4970a6350a3d191ffb1215b6e1",
                0,
                10_000_000_000_000,
                SECP_CODE_HASH,
                "0x2c71fb9e1c558782f6ca013fd7e8612d98990177",
                None,
                "0x",
            ),
        ];
        let outputs = vec![
            // Funding output: USDI-typed, amount 1_000_000_050 (LE u128).
            real_output(
                36_000_000_000,
                FUNDING_LOCK_CODE_HASH_TESTNET,
                "0x00510ea5249c2b102ab35607ee04418ae47cb83b",
                usdi,
                "0x32ca9a3b000000000000000000000000",
            ),
            // USDI change output — must NOT be mistaken for the funding UDT.
            real_output(
                14_200_000_000,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                usdi,
                "0x9cc99a3b000000000000000000000000",
            ),
            real_output(
                9_963_999_998_062,
                SECP_CODE_HASH,
                "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
                None,
                "0x",
            ),
            real_output(
                9_996_199_999_787,
                SECP_CODE_HASH,
                "0x2c71fb9e1c558782f6ca013fd7e8612d98990177",
                None,
                "0x",
            ),
        ];

        let tx_hash = parse_hex_to_bytes(OPEN_TX_HASH);
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF1; 32],
            tx_index: 1,
            block_number: 20565583,
            timestamp: 1_774_580_273_479,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(false))];
        let mut list = build_tx_actions_for_block(&[tx], &detectors).unwrap();
        let actions = list.remove(0);
        let open = find_fiber_action(&actions, "channel_open");
        let meta = open.metadata_value().unwrap();

        assert_eq!(meta["capacity"], "36000000000");
        assert_eq!(meta["udtTypeHash"], USDI_TYPE_SCRIPT_HASH);
        assert_eq!(meta["udtAmount"], "1000000050");
        assert_eq!(
            meta["fundingLockArgs"],
            "0x00510ea5249c2b102ab35607ee04418ae47cb83b"
        );

        // Channel participants: the non-fiber owners, sorted — the funding
        // lock's own script hash must be excluded (bulk reducer semantics).
        let expected_participants = serde_json::json!([
            "0x82d8b56da3115bcf8e7f4ebb05415e271967d14a2326d285d32bff3290f5b34f",
            "0xe121dad700de2ac100e79d7bdbc505e4b63760aed7718c4170f0a46a2afb13cb",
        ]);
        assert_eq!(meta["participants"], expected_participants);
    }

    /// Bug 1 contrast + regression: CKB-only channel open (mainnet tx
    /// 0x4867bd9201a29591c2359cedd6cee74bee7448eb3b23b942aabf3b19b7ea7c32,
    /// block 18906109, tx_index 1) keeps emitting channel_open and carries no
    /// UDT metadata. Participants must equal the two secp owners recorded by
    /// the bulk reducer for this very channel (DB evidence, channel
    /// 0x0248a680…).
    #[test]
    fn test_channel_open_ckb_only_real_vector_has_no_udt_metadata() {
        let inputs = vec![
            real_input(
                "0x1823261b6b521ec5a06131cf73f29d70ce4b0c1b93d8c63a259079b12f15b9a6",
                1,
                29_899_849_299,
                SECP_CODE_HASH,
                "0x7f0a30a2da9ee266d6f901daa52605d855c04449",
                None,
                "0x",
            ),
            real_input(
                "0xbeef1f91e3242cf5e7d0f129496759f93d2467ff9ead8a621d5994162da70f96",
                0,
                49_999_999_456,
                SECP_CODE_HASH,
                "0x7f0a30a2da9ee266d6f901daa52605d855c04449",
                None,
                "0x",
            ),
            real_input(
                "0xdb6391f04b3cf1aabfc9012b9224cac7ec403c35988658118f96df2fc58403d3",
                12,
                10_000_000_000,
                SECP_CODE_HASH,
                "0xb2811a989616fdd2c5a676a798c6f9aa64eb6338",
                None,
                "0x",
            ),
            real_input(
                "0xdb6391f04b3cf1aabfc9012b9224cac7ec403c35988658118f96df2fc58403d3",
                13,
                10_000_000_000,
                SECP_CODE_HASH,
                "0xb2811a989616fdd2c5a676a798c6f9aa64eb6338",
                None,
                "0x",
            ),
            real_input(
                "0xdb6391f04b3cf1aabfc9012b9224cac7ec403c35988658118f96df2fc58403d3",
                11,
                10_000_000_000,
                SECP_CODE_HASH,
                "0xb2811a989616fdd2c5a676a798c6f9aa64eb6338",
                None,
                "0x",
            ),
            real_input(
                "0xdb6391f04b3cf1aabfc9012b9224cac7ec403c35988658118f96df2fc58403d3",
                14,
                10_000_000_000,
                SECP_CODE_HASH,
                "0xb2811a989616fdd2c5a676a798c6f9aa64eb6338",
                None,
                "0x",
            ),
        ];
        let outputs = vec![
            real_output(
                75_000_000_000,
                FUNDING_LOCK_CODE_HASH_MAINNET,
                "0x8547e600b96d479693916e4c6e056fe264f2a991",
                None,
                "0x",
            ),
            real_output(
                29_899_848_134,
                SECP_CODE_HASH,
                "0x7f0a30a2da9ee266d6f901daa52605d855c04449",
                None,
                "0x",
            ),
            real_output(
                14_999_999_493,
                SECP_CODE_HASH,
                "0xb2811a989616fdd2c5a676a798c6f9aa64eb6338",
                None,
                "0x",
            ),
        ];

        let tx_hash = parse_hex_to_bytes(
            "0x4867bd9201a29591c2359cedd6cee74bee7448eb3b23b942aabf3b19b7ea7c32",
        );
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF2; 32],
            tx_index: 1,
            block_number: 18906109,
            timestamp: 1_774_231_914_818,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions = fiber_detect_mainnet(tx);
        let open = find_fiber_action(&actions, "channel_open");
        let meta = open.metadata_value().unwrap();

        assert_eq!(meta["capacity"], "75000000000");
        assert_eq!(
            meta["fundingLockArgs"],
            "0x8547e600b96d479693916e4c6e056fe264f2a991"
        );
        assert!(
            meta.get("udtTypeHash").is_none(),
            "CKB-only channel must not carry udtTypeHash"
        );
        assert!(
            meta.get("udtAmount").is_none(),
            "CKB-only channel must not carry udtAmount"
        );
        // DB evidence cross-check: bulk reducer recorded exactly these two
        // participants for channel 0x0248a680… (sorted lock script hashes).
        let expected_participants = serde_json::json!([
            "0x8ac9f88b828e21113b4966a3b4608a53bdf8e3ec6ddf9a397379925a899af68b",
            "0x9a040b1fa0257fb0abe427dcec79441e4dc277a6eed602a290b75acaa2d059e2",
        ]);
        assert_eq!(meta["participants"], expected_participants);
    }

    /// Bug 1 fail-fast: a funding output that carries a type script whose cell
    /// data cannot hold the 16-byte LE UDT amount is an invariant violation and
    /// must fail the whole build, not silently produce a CKB-only channel.
    #[test]
    fn test_channel_open_udt_funding_output_with_short_data_fails() {
        let inputs = vec![real_input(
            "0x7f76488c606aadf8f9cae974336c9c81a97829618b7fccb14d4097bf4ca4a3c5",
            2,
            9_981_999_999_031,
            SECP_CODE_HASH,
            "0xd2c9b058568578c884e108e3a82ee111af6a9f4b",
            None,
            "0x",
        )];
        let outputs = vec![real_output(
            36_000_000_000,
            FUNDING_LOCK_CODE_HASH_TESTNET,
            "0x00510ea5249c2b102ab35607ee04418ae47cb83b",
            Some((
                "0xcc9dc33ef234e14bc788c43a4848556a5fb16401a04662fc55db9bb201987037",
                "0x71fd1985b2971a9903e4d8ed0d59e6710166985217ca0681437883837b86162f",
            )),
            "0x0102", // 2 bytes: cannot hold a u128 LE amount
        )];

        let tx_hash = [0x77; 32];
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF3; 32],
            tx_index: 1,
            block_number: 20565584,
            timestamp: 1_774_580_280_000,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(false))];
        let error = build_tx_actions_for_block(&[tx], &detectors)
            .expect_err("short UDT amount data on a typed funding output must fail the build");
        let message = error.to_string();
        assert!(
            message.contains("7777777777"),
            "error should name the tx: {message}"
        );
    }

    /// Bug 2 red test (audit agent D): a real mainnet force close whose only
    /// owners are the fiber locks themselves. Mainnet tx
    /// 0x2bce4f4cdd42a23386325ae204bbebfb94dba267dedbe80569e1553c2fedcc7f
    /// (block 15676315, tx_index 14): funding-lock input
    /// 0x18e21924…:0 → commitment-lock output 0, no other cells.
    /// The tx-level fiber force_close action must be emitted even though every
    /// owner is a fiber lock.
    #[test]
    fn test_force_close_all_fiber_owners_emits_action() {
        const FUNDING_TX_HASH: &str =
            "0x18e21924d3590e865473aa6e16be7a1d45c2b990d5519c03b82d371467783789";
        // 36-byte short-format commitment args captured from the chain:
        // pubkey_hash ac47cad1… + delay_epoch LE 06000000000100a0 + version 0.
        const COMMITMENT_ARGS: &str =
            "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef06000000000100a00000000000000000";

        let inputs = vec![real_input(
            FUNDING_TX_HASH,
            0,
            106_200_000_000,
            FUNDING_LOCK_CODE_HASH_MAINNET,
            "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef",
            None,
            "0x",
        )];
        let outputs = vec![real_output(
            106_199_999_545,
            COMMITMENT_LOCK_CODE_HASH_MAINNET,
            COMMITMENT_ARGS,
            None,
            "0x",
        )];

        let tx_hash = parse_hex_to_bytes(
            "0x2bce4f4cdd42a23386325ae204bbebfb94dba267dedbe80569e1553c2fedcc7f",
        );
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF4; 32],
            tx_index: 14,
            block_number: 15676315,
            timestamp: 1_742_365_295_267,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions = fiber_detect_mainnet(tx);
        let force_close = find_fiber_action(&actions, "force_close");
        let meta = force_close.metadata_value().unwrap();

        assert_eq!(
            meta["channelOutpoint"],
            encode_channel_outpoint(&parse_hex_to_bytes(FUNDING_TX_HASH), 0).unwrap()
        );
        assert_eq!(meta["capacity"], "106200000000");
        assert_eq!(meta["commitmentLockArgs"], COMMITMENT_ARGS);
        assert_eq!(meta["commitmentOutputIndex"], 0);
        assert_eq!(
            meta["fundingLockArgs"],
            "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef"
        );
        // Both fiber-lock owners stay participants of the activity row.
        assert_eq!(actions.participants.len(), 2);
        assert!(actions
            .participants
            .iter()
            .all(|p| p.tags & TAG_PROTOCOL != 0));
        // Emitting per-owner must still yield ONE tx-level action: the metadata
        // is owner-independent, so dedup collapses the two identical copies.
        assert_eq!(
            actions
                .protocol_actions
                .iter()
                .filter(|a| a.protocol == "fiber")
                .count(),
            1,
            "per-owner emission must dedup to a single tx-level fiber action"
        );
        // A force close names no channel participants of its own — the channel
        // row keeps the participants recorded at open time.
        assert!(meta.get("participants").is_none());
    }

    /// Bug 2 breadth: a commitment revocation whose owners are all commitment
    /// locks (commitment input + commitment output, nothing else) must also
    /// emit its tx-level event. Uses the real mainnet commitment lock with
    /// synthetic-but-well-formed args (no revocation has occurred on mainnet).
    #[test]
    fn test_commitment_revocation_all_fiber_owners_emits_action() {
        let mut old_args = vec![0xCC; 20];
        old_args.extend_from_slice(&100u64.to_le_bytes());
        old_args.extend_from_slice(&1u64.to_be_bytes());
        let mut new_args = vec![0xCC; 20];
        new_args.extend_from_slice(&100u64.to_le_bytes());
        new_args.extend_from_slice(&2u64.to_be_bytes());

        let inputs = vec![real_input(
            "0x2bce4f4cdd42a23386325ae204bbebfb94dba267dedbe80569e1553c2fedcc7f",
            0,
            106_199_999_545,
            COMMITMENT_LOCK_CODE_HASH_MAINNET,
            &format!("0x{}", hex::encode(&old_args)),
            None,
            "0x",
        )];
        let outputs = vec![real_output(
            106_199_999_000,
            COMMITMENT_LOCK_CODE_HASH_MAINNET,
            &format!("0x{}", hex::encode(&new_args)),
            None,
            "0x",
        )];

        let tx_hash = [0x66; 32];
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF5; 32],
            tx_index: 1,
            block_number: 15676999,
            timestamp: 1_742_366_000_000,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions = fiber_detect_mainnet(tx);
        let revocation = find_fiber_action(&actions, "commitment_revocation");
        let meta = revocation.metadata_value().unwrap();
        assert_eq!(
            meta["oldCommitmentLockArgs"],
            format!("0x{}", hex::encode(&old_args))
        );
        assert_eq!(
            meta["newCommitmentLockArgs"],
            format!("0x{}", hex::encode(&new_args))
        );
        assert_eq!(meta["newCommitmentOutputIndex"], 0);
    }

    /// Regression (must stay working): cooperative close with a non-fiber
    /// owner. Mainnet tx 0xd1c4d645… (block 18906128, tx_index 2) spends
    /// funding cell 0x4867bd92…:0 into two secp outputs.
    #[test]
    fn test_channel_close_real_vector_still_emits() {
        const FUNDING_TX_HASH: &str =
            "0x4867bd9201a29591c2359cedd6cee74bee7448eb3b23b942aabf3b19b7ea7c32";
        let inputs = vec![real_input(
            FUNDING_TX_HASH,
            0,
            75_000_000_000,
            FUNDING_LOCK_CODE_HASH_MAINNET,
            "0x8547e600b96d479693916e4c6e056fe264f2a991",
            None,
            "0x",
        )];
        let outputs = vec![
            real_output(
                49_999_999_456,
                SECP_CODE_HASH,
                "0x7f0a30a2da9ee266d6f901daa52605d855c04449",
                None,
                "0x",
            ),
            real_output(
                25_000_000_006,
                SECP_CODE_HASH,
                "0xb2811a989616fdd2c5a676a798c6f9aa64eb6338",
                None,
                "0x",
            ),
        ];

        let tx_hash = parse_hex_to_bytes(
            "0xd1c4d6458fc57488752fd493c53c6111ec015030b43acc546f491bb780a14b7a",
        );
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF6; 32],
            tx_index: 2,
            block_number: 18906128,
            timestamp: 1_774_232_059_719,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions = fiber_detect_mainnet(tx);
        let close = find_fiber_action(&actions, "channel_close");
        let meta = close.metadata_value().unwrap();
        assert_eq!(
            meta["channelOutpoint"],
            encode_channel_outpoint(&parse_hex_to_bytes(FUNDING_TX_HASH), 0).unwrap()
        );
        assert_eq!(meta["capacity"], "75000000000");
    }

    /// Regression (must stay working): settlement with non-fiber owners.
    /// Mainnet tx 0x648c489a… (block 15684947, tx_index 5) sweeps commitment
    /// cell 0x2bce4f4c…:0 plus a secp fee cell into three secp outputs.
    #[test]
    fn test_settlement_real_vector_still_emits() {
        const COMMITMENT_ARGS: &str =
            "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef06000000000100a00000000000000000";
        let inputs = vec![
            real_input(
                "0x2bce4f4cdd42a23386325ae204bbebfb94dba267dedbe80569e1553c2fedcc7f",
                0,
                106_199_999_545,
                COMMITMENT_LOCK_CODE_HASH_MAINNET,
                COMMITMENT_ARGS,
                None,
                "0x",
            ),
            real_input(
                "0x18e21924d3590e865473aa6e16be7a1d45c2b990d5519c03b82d371467783789",
                1,
                39_999_997_543,
                SECP_CODE_HASH,
                "0xa9631ecab784e46aba801e4871c57a092929a6fa",
                None,
                "0x",
            ),
        ];
        let outputs = vec![
            real_output(
                6_199_999_545,
                SECP_CODE_HASH,
                "0x9c636e7c3f711fc3b6784b073eac7777742c3d8f",
                None,
                "0x",
            ),
            real_output(
                99_999_999_545,
                SECP_CODE_HASH,
                "0xa9631ecab784e46aba801e4871c57a092929a6fa",
                None,
                "0x",
            ),
            real_output(
                39_999_996_731,
                SECP_CODE_HASH,
                "0xa9631ecab784e46aba801e4871c57a092929a6fa",
                None,
                "0x",
            ),
        ];

        let tx_hash = parse_hex_to_bytes(
            "0x648c489a392b17e8345644f1612b8f112c6a1d680eff92034988b42946abceac",
        );
        let tx = TxView {
            tx_hash: &tx_hash,
            block_hash: &[0xF7; 32],
            tx_index: 5,
            block_number: 15684947,
            timestamp: 1_742_450_994_140,
            is_cellbase: false,
            inputs: inputs.iter().map(|i| i.view()).collect(),
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let actions = fiber_detect_mainnet(tx);
        let settlement = find_fiber_action(&actions, "settlement");
        let meta = settlement.metadata_value().unwrap();
        assert_eq!(meta["commitmentLockArgs"], COMMITMENT_ARGS);
        assert_eq!(meta["capacity"], "106199999545");
    }

    #[test]
    fn test_classify_commitment_revocation_vs_settlement() {
        let detector = FiberDetector::new(true);

        // commitment input + commitment output -> revocation
        let summary_revocation = FiberCellSummary {
            has_commitment_input: true,
            has_commitment_output: true,
            ..Default::default()
        };
        assert_eq!(
            detector.classify_event(&summary_revocation),
            Some(FiberEvent::CommitmentRevocation)
        );

        // commitment input only -> settlement
        let summary_settlement = FiberCellSummary {
            has_commitment_input: true,
            has_commitment_output: false,
            ..Default::default()
        };
        assert_eq!(
            detector.classify_event(&summary_settlement),
            Some(FiberEvent::Settlement)
        );

        // funding input + commitment input + commitment output -> force_close (takes priority)
        let summary_force = FiberCellSummary {
            has_funding_input: true,
            has_commitment_input: true,
            has_commitment_output: true,
            ..Default::default()
        };
        assert_eq!(
            detector.classify_event(&summary_force),
            Some(FiberEvent::ForceClose)
        );
    }
}
