//! Fiber Network protocol detector: identifies payment channel lifecycle events
//! by analyzing funding-lock and commitment-lock transitions.

use ckbadger_store::types::ProtocolAction;

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
    /// lock_args from the first funding-lock input (for close/force_close metadata).
    funding_input_args: Option<Vec<u8>>,
    /// Capacity of the first funding-lock input (for close/force_close metadata).
    funding_input_capacity: Option<i64>,
    /// lock_args from the first commitment-lock input (for settlement metadata).
    commitment_input_args: Option<Vec<u8>>,
    /// Capacity of the first commitment-lock input (for settlement metadata).
    commitment_input_capacity: Option<i64>,
    /// lock_args from the first commitment-lock output (for force_close metadata).
    commitment_output_args: Option<Vec<u8>>,
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
    fn summarize_cells(&self, tx: &TxView<'_>) -> FiberCellSummary {
        let mut summary = FiberCellSummary::default();

        for input in &tx.inputs {
            match classify_lock(&input.lock_code_hash) {
                FiberLockType::FundingLock => {
                    summary.has_funding_input = true;
                    summary
                        .funding_lock_hashes
                        .push(input.lock_script_hash.clone());
                    if summary.funding_input_args.is_none() {
                        summary.funding_input_args = Some(input.lock_args.clone());
                        summary.funding_input_capacity = Some(input.capacity);
                    }
                }
                FiberLockType::CommitmentLock => {
                    summary.has_commitment_input = true;
                    summary
                        .commitment_lock_hashes
                        .push(input.lock_script_hash.clone());
                    if summary.commitment_input_args.is_none() {
                        summary.commitment_input_args = Some(input.lock_args.clone());
                        summary.commitment_input_capacity = Some(input.capacity);
                    }
                }
                FiberLockType::Other => {}
            }
        }

        for (idx, output) in tx.outputs.iter().enumerate() {
            match classify_lock(&output.lock_code_hash) {
                FiberLockType::FundingLock => {
                    summary.has_funding_output = true;
                    summary
                        .funding_lock_hashes
                        .push(output.lock_script_hash.clone());
                    if summary.funding_output_args.is_none() {
                        summary.funding_output_args = Some(output.lock_args.clone());
                        summary.funding_output_capacity = Some(output.capacity);
                        summary.funding_output_index = Some(idx);
                    }
                }
                FiberLockType::CommitmentLock => {
                    summary.has_commitment_output = true;
                    summary
                        .commitment_lock_hashes
                        .push(output.lock_script_hash.clone());
                    if summary.commitment_output_args.is_none() {
                        summary.commitment_output_args = Some(output.lock_args.clone());
                    }
                }
                FiberLockType::Other => {}
            }
        }

        summary
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
                    // Encode outpoint as hex: tx_hash(32B) + output_index as LE u32(4B)
                    let mut outpoint_bytes = Vec::with_capacity(36);
                    outpoint_bytes.extend_from_slice(tx_hash);
                    outpoint_bytes.extend_from_slice(&output_index_u32.to_le_bytes());
                    meta.insert(
                        "channelOutpoint".to_string(),
                        serde_json::json!(format!("0x{}", hex::encode(&outpoint_bytes))),
                    );
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
            FiberEvent::ChannelClose => {
                let mut meta = serde_json::Map::new();
                meta.insert("event".to_string(), serde_json::json!("channel_close"));

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

                Ok(serde_json::Value::Object(meta))
            }
        }
    }

    /// Check if the given owner_lock_hash is a Fiber lock (funding or commitment) in this tx.
    fn is_fiber_lock_owner(&self, summary: &FiberCellSummary, owner_lock_hash: &[u8]) -> bool {
        summary
            .funding_lock_hashes
            .iter()
            .any(|h| h == owner_lock_hash)
            || summary
                .commitment_lock_hashes
                .iter()
                .any(|h| h == owner_lock_hash)
    }
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
            .any(|input| classify_lock(&input.lock_code_hash) != FiberLockType::Other)
            || tx
                .outputs
                .iter()
                .any(|output| classify_lock(&output.lock_code_hash) != FiberLockType::Other)
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        _accum: &OwnerAccum,
        _asset_changes: &[ckbadger_store::types::AssetChange],
        _type_calls: &[ckbadger_store::types::TypeCallEntry],
        _lock_calls: &[ckbadger_store::types::LockCallEntry],
    ) -> Vec<ProtocolAction> {
        let summary = self.summarize_cells(tx);

        // Only emit actions for owners who are NOT the funding/commitment lock owner.
        if self.is_fiber_lock_owner(&summary, owner_lock_hash) {
            return vec![];
        }

        let event = match self.classify_event(&summary) {
            Some(e) => e,
            None => return vec![],
        };

        let metadata = match self.build_metadata(event, &summary, tx.tx_hash) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(
                    "Fiber metadata build failed for tx 0x{}: {}",
                    hex::encode(tx.tx_hash),
                    e,
                );
                return vec![];
            }
        };

        let action = match event {
            FiberEvent::ChannelOpen => "channel_open",
            FiberEvent::ChannelClose => "channel_close",
            FiberEvent::ForceClose => "force_close",
            FiberEvent::Settlement => "settlement",
            FiberEvent::CommitmentRevocation => "commitment_revocation",
        };

        vec![ProtocolAction::new("fiber", action, metadata)]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::db::writer::activities::{
        build_activity_bundles_for_block_with_detectors, InputCellView, TxView,
    };
    use crate::parser::cell::ParsedCell;
    use crate::parser::fiber::{COMMITMENT_LOCK_CODE_HASH_MAINNET, FUNDING_LOCK_CODE_HASH_MAINNET};
    use crate::rpc::parse_hex_to_bytes;

    fn make_input_with_lock(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> InputCellView {
        InputCellView {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            capacity,
            occupied_capacity: 61_00000000,
            type_code_hash,
            type_hash_type: Some(1),
            type_script_hash: None,
            type_args,
            udt_amount: None,
            data: vec![],
            is_dao_withdraw_request: false,
            dao_compensation: None,
        }
    }

    fn make_output_with_lock(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> ParsedCell {
        ParsedCell {
            capacity,
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            lock_script_hash: vec![lock_hash_byte; 32],
            type_code_hash,
            type_hash_type: Some(1),
            type_args,
            type_script_hash: None,
            data_hash: [0; 32],
            data_size: 0,
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
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        // Participant should get channel_open action
        let participant_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert_eq!(participant_delta.protocol_actions.len(), 1);
        assert_eq!(participant_delta.protocol_actions[0].protocol, "fiber");
        assert_eq!(participant_delta.protocol_actions[0].action, "channel_open");

        // Verify metadata
        let meta = participant_delta.protocol_actions[0]
            .metadata_value()
            .unwrap();
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

        // Funding lock owner should NOT get action
        let funding_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![funding_owner; 32])
            .expect("funding owner should be present");
        assert!(
            funding_delta.protocol_actions.is_empty(),
            "funding lock owner should not receive protocol action"
        );
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
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let participant_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert_eq!(participant_delta.protocol_actions.len(), 1);
        assert_eq!(participant_delta.protocol_actions[0].protocol, "fiber");
        assert_eq!(
            participant_delta.protocol_actions[0].action,
            "channel_close"
        );

        let meta = participant_delta.protocol_actions[0]
            .metadata_value()
            .unwrap();
        assert_eq!(meta["event"], "channel_close");
        assert_eq!(meta["capacity"], "14500000000");
        assert_eq!(
            meta["fundingLockArgs"],
            format!("0x{}", hex::encode(&funding_args))
        );

        // Funding lock owner should NOT get action
        let funding_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![funding_owner; 32])
            .expect("funding owner should be present");
        assert!(funding_delta.protocol_actions.is_empty());
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
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let participant_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert_eq!(participant_delta.protocol_actions.len(), 1);
        assert_eq!(participant_delta.protocol_actions[0].protocol, "fiber");
        assert_eq!(participant_delta.protocol_actions[0].action, "force_close");

        let meta = participant_delta.protocol_actions[0]
            .metadata_value()
            .unwrap();
        assert_eq!(meta["event"], "force_close");
        assert_eq!(meta["capacity"], "14500000000");
        assert_eq!(
            meta["fundingLockArgs"],
            format!("0x{}", hex::encode(&funding_args))
        );
        assert_eq!(
            meta["commitmentLockArgs"],
            format!("0x{}", hex::encode(&commitment_args))
        );

        // Funding lock owner should NOT get action
        let funding_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![funding_owner; 32])
            .expect("funding owner should be present");
        assert!(funding_delta.protocol_actions.is_empty());

        // Commitment lock owner should NOT get action
        let commitment_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![commitment_owner; 32])
            .expect("commitment owner should be present");
        assert!(commitment_delta.protocol_actions.is_empty());
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
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let participant_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert_eq!(participant_delta.protocol_actions.len(), 1);
        assert_eq!(participant_delta.protocol_actions[0].protocol, "fiber");
        assert_eq!(participant_delta.protocol_actions[0].action, "settlement");

        let meta = participant_delta.protocol_actions[0]
            .metadata_value()
            .unwrap();
        assert_eq!(meta["event"], "settlement");
        assert_eq!(meta["capacity"], "10000000000");
        assert_eq!(
            meta["commitmentLockArgs"],
            format!("0x{}", hex::encode(&commitment_args))
        );

        // Commitment lock owner should NOT get action
        let commitment_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![commitment_owner; 32])
            .expect("commitment owner should be present");
        assert!(commitment_delta.protocol_actions.is_empty());
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
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);
        for owner in &bundles[0].owners {
            assert!(
                owner.protocol_actions.is_empty(),
                "no fiber actions expected for standard-only tx"
            );
        }
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
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        let participant_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert_eq!(participant_delta.protocol_actions.len(), 1);

        let meta = participant_delta.protocol_actions[0]
            .metadata_value()
            .unwrap();
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
            inputs: vec![input_commitment, input_fee],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(FiberDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let participant_delta = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![participant; 32])
            .expect("participant should be present");
        assert_eq!(participant_delta.protocol_actions.len(), 1);
        assert_eq!(participant_delta.protocol_actions[0].protocol, "fiber");
        assert_eq!(
            participant_delta.protocol_actions[0].action,
            "commitment_revocation"
        );

        let meta = participant_delta.protocol_actions[0]
            .metadata_value()
            .unwrap();
        assert_eq!(meta["event"], "commitment_revocation");
        assert_eq!(
            meta["oldCommitmentLockArgs"],
            format!("0x{}", hex::encode(&old_commitment_args))
        );
        assert_eq!(
            meta["newCommitmentLockArgs"],
            format!("0x{}", hex::encode(&new_commitment_args))
        );

        // Commitment lock owners should NOT get action
        let commitment_in = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![commitment_owner_in; 32]);
        if let Some(delta) = commitment_in {
            assert!(delta.protocol_actions.is_empty());
        }
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
