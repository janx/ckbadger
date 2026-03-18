//! UTXOSwap protocol detector: identifies intent lifecycle events
//! (submitted / settled) by analyzing Intent Lock cell transitions.

use ckbadger_store::types::{AssetChange, LockCallEntry, ProtocolAction, TypeCallEntry};

use crate::parser::utxoswap::{is_intent_lock, parse_intent_args};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

pub(crate) struct UtxoSwapDetector;

impl UtxoSwapDetector {
    pub fn new(_is_mainnet: bool) -> Self {
        Self
    }

    /// Format a byte slice as "0x..." hex string for JSON metadata.
    fn hex(bytes: &[u8]) -> String {
        format!("0x{}", hex::encode(bytes))
    }
}

impl ProtocolDetector for UtxoSwapDetector {
    fn might_apply_batch(
        &self,
        lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
        _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
    ) -> bool {
        lock_code_hashes.iter().any(|h| is_intent_lock(h))
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.inputs
            .iter()
            .any(|input| is_intent_lock(&input.lock_code_hash))
            || tx
                .outputs
                .iter()
                .any(|output| is_intent_lock(&output.lock_code_hash))
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        accum: &OwnerAccum,
        _asset_changes: &[AssetChange],
        _type_calls: &[TypeCallEntry],
        _lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction> {
        let mut actions = Vec::new();

        let ckb_delta = accum.output_capacity - accum.input_capacity;

        // --- Submitted: intent lock on outputs, only for the intent's owner ---
        if ckb_delta < 0 {
            for output in tx.outputs {
                if !is_intent_lock(&output.lock_code_hash) {
                    continue;
                }
                let parsed = match parse_intent_args(&output.lock_args) {
                    Some(p) => p,
                    None => continue,
                };

                // Only attribute to the actual intent owner, not co-signers or fee sponsors
                if owner_lock_hash.len() < 20 || owner_lock_hash[..20] != parsed.owner_lock_hash[..]
                {
                    continue;
                }

                let action_name = format!("{}_submitted", parsed.intent_type.action_name());

                let mut metadata = serde_json::json!({
                    "intentType": parsed.intent_type.display_name(),
                    "poolTypeHash": Self::hex(&parsed.pool_type_hash),
                    "amountIn": parsed.amount_in.to_string(),
                    "amountOutMin": parsed.amount_out_min.to_string(),
                    "assetInIndex": parsed.asset_in_index,
                });

                if let Some(extra) = &parsed.create_pool_extra {
                    metadata["assetX"] = serde_json::json!(Self::hex(&extra.asset_x));
                    metadata["assetY"] = serde_json::json!(Self::hex(&extra.asset_y));
                    metadata["amountX"] = serde_json::json!(extra.amount_x.to_string());
                    metadata["amountY"] = serde_json::json!(extra.amount_y.to_string());
                    metadata["totalFeeRate"] = serde_json::json!(extra.total_fee_rate);
                }

                actions.push(ProtocolAction::new("utxoswap", action_name, metadata));
            }
        }

        // --- Settled: intent lock on inputs, owner_lock_hash prefix match ---
        for input in &tx.inputs {
            if !is_intent_lock(&input.lock_code_hash) {
                continue;
            }
            let parsed = match parse_intent_args(&input.lock_args) {
                Some(p) => p,
                None => continue,
            };

            // Check if the first 20 bytes of the current owner's lock_hash
            // match the owner_lock_hash stored in the intent args.
            if owner_lock_hash.len() >= 20 && owner_lock_hash[..20] == parsed.owner_lock_hash[..] {
                let action_name = format!("{}_settled", parsed.intent_type.action_name());

                let mut metadata = serde_json::json!({
                    "intentType": parsed.intent_type.display_name(),
                    "poolTypeHash": Self::hex(&parsed.pool_type_hash),
                    "amountIn": parsed.amount_in.to_string(),
                    "amountOutMin": parsed.amount_out_min.to_string(),
                    "assetInIndex": parsed.asset_in_index,
                });

                if let Some(extra) = &parsed.create_pool_extra {
                    metadata["assetX"] = serde_json::json!(Self::hex(&extra.asset_x));
                    metadata["assetY"] = serde_json::json!(Self::hex(&extra.asset_y));
                    metadata["amountX"] = serde_json::json!(extra.amount_x.to_string());
                    metadata["amountY"] = serde_json::json!(extra.amount_y.to_string());
                    metadata["totalFeeRate"] = serde_json::json!(extra.total_fee_rate);
                }

                actions.push(ProtocolAction::new("utxoswap", action_name, metadata));
            }
        }

        actions
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
    use crate::parser::utxoswap::INTENT_LOCK_CODE_HASH_MAINNET;
    use crate::rpc::parse_hex_to_bytes;

    fn make_input(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
    ) -> InputCellView {
        InputCellView {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            capacity,
            occupied_capacity: 61_00000000,
            type_code_hash: None,
            type_hash_type: Some(1),
            type_script_hash: None,
            type_args: None,
            udt_amount: None,
            data: vec![],
            is_dao_withdraw_request: false,
            dao_compensation: None,
        }
    }

    fn make_output(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
    ) -> ParsedCell {
        ParsedCell {
            capacity,
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            lock_script_hash: vec![lock_hash_byte; 32],
            type_code_hash: None,
            type_hash_type: Some(1),
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0; 32],
            data_size: 0,
            data: vec![],
        }
    }

    /// Build intent lock args for non-CreatePool intent types.
    /// The `owner_lock_hash_prefix` is the 20-byte prefix written into args[0..20].
    fn build_intent_args(
        owner_lock_hash_prefix: &[u8; 20],
        intent_type: u8,
        asset_in_index: u8,
        amount_in: u128,
        amount_out_min: u128,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 90];
        buf[..20].copy_from_slice(owner_lock_hash_prefix);
        // pool_type_hash [20..40]
        for (i, b) in buf[20..40].iter_mut().enumerate() {
            *b = (i as u8) + 0x80;
        }
        // tx_fee [40..48] — zeros
        // expire_batch_id [48..56] — zeros
        buf[56] = intent_type;
        buf[57] = asset_in_index;
        buf[58..74].copy_from_slice(&amount_in.to_le_bytes());
        buf[74..90].copy_from_slice(&amount_out_min.to_le_bytes());
        buf
    }

    #[test]
    fn test_no_intent_lock_returns_empty() {
        // Standard locks only -> no actions
        let standard_lock = vec![0x11; 32];
        let alice: u8 = 0xAA;
        let bob: u8 = 0xBB;

        let input = make_input(alice, standard_lock.clone(), vec![0x22; 20], 200_00000000);
        let outputs = vec![make_output(
            bob,
            standard_lock,
            vec![0x33; 20],
            200_00000000,
        )];

        let tx = TxView {
            tx_hash: &[0x44; 32],
            block_hash: &[0xC4; 32],
            tx_index: 1,
            block_number: 5000,
            timestamp: 1_700_200_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);
        for owner in &bundles[0].owners {
            assert!(
                owner.protocol_actions.is_empty(),
                "no utxoswap actions expected for standard-only tx"
            );
        }
    }

    #[test]
    fn test_swap_submitted() {
        // Alice sends CKB to an intent lock output (intent_type=3 SwapExactInputForOutput).
        // Alice has ckb_delta < 0, so she should get a "swap_exact_input_submitted" action.
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let alice: u8 = 0xAA;
        let intent_owner: u8 = 0xF0;

        // The 20-byte owner prefix in intent args — use first 20 bytes of alice's lock_hash
        let alice_owner_prefix: [u8; 20] = [alice; 20];
        let intent_args = build_intent_args(&alice_owner_prefix, 3, 0, 1000, 500);

        // Alice provides 500 CKB input
        let input = make_input(alice, standard_lock.clone(), vec![0x22; 20], 500_00000000);

        // Intent lock output gets 300 CKB, alice gets 200 CKB change
        let outputs = vec![
            make_output(intent_owner, intent_code_hash, intent_args, 300_00000000),
            make_output(alice, standard_lock, vec![0x22; 20], 200_00000000),
        ];

        let tx = TxView {
            tx_hash: &[0x50; 32],
            block_hash: &[0xD0; 32],
            tx_index: 1,
            block_number: 6000,
            timestamp: 1_700_300_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let alice_owner = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![alice; 32])
            .expect("alice should be present");
        assert_eq!(alice_owner.protocol_actions.len(), 1);
        assert_eq!(alice_owner.protocol_actions[0].protocol, "utxoswap");
        assert_eq!(
            alice_owner.protocol_actions[0].action,
            "swap_exact_input_submitted"
        );

        let meta = alice_owner.protocol_actions[0].metadata_value().unwrap();
        assert_eq!(meta["intentType"], "SwapExactInputForOutput");
        assert_eq!(meta["amountIn"], "1000");
        assert_eq!(meta["amountOutMin"], "500");
        assert_eq!(meta["assetInIndex"], 0);
        // poolTypeHash should be a hex string
        assert!(meta["poolTypeHash"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn test_swap_settled() {
        // Intent lock input consumed, alice receives output,
        // owner_lock_hash prefix matches alice -> "swap_exact_input_settled"
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let alice: u8 = 0xAA;
        let intent_cell_owner: u8 = 0xF0;

        // The intent args encode alice's lock_hash prefix (first 20 bytes)
        let alice_owner_prefix: [u8; 20] = [alice; 20];
        let intent_args = build_intent_args(&alice_owner_prefix, 3, 1, 2000, 1500);

        // Intent lock cell consumed from input
        let intent_input = make_input(
            intent_cell_owner,
            intent_code_hash,
            intent_args,
            300_00000000,
        );

        // Alice receives output
        let outputs = vec![make_output(
            alice,
            standard_lock,
            vec![0x22; 20],
            300_00000000,
        )];

        let tx = TxView {
            tx_hash: &[0x51; 32],
            block_hash: &[0xD1; 32],
            tx_index: 1,
            block_number: 6001,
            timestamp: 1_700_300_010,
            is_cellbase: false,
            inputs: vec![intent_input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let alice_owner = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![alice; 32])
            .expect("alice should be present");
        assert_eq!(alice_owner.protocol_actions.len(), 1);
        assert_eq!(alice_owner.protocol_actions[0].protocol, "utxoswap");
        assert_eq!(
            alice_owner.protocol_actions[0].action,
            "swap_exact_input_settled"
        );

        let meta = alice_owner.protocol_actions[0].metadata_value().unwrap();
        assert_eq!(meta["intentType"], "SwapExactInputForOutput");
        assert_eq!(meta["amountIn"], "2000");
        assert_eq!(meta["amountOutMin"], "1500");
        assert_eq!(meta["assetInIndex"], 1);
    }

    #[test]
    fn test_add_liquidity_submitted() {
        // intent_type=1 -> "add_liquidity_submitted"
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let alice: u8 = 0xAA;
        let intent_owner: u8 = 0xF0;

        let alice_owner_prefix: [u8; 20] = [alice; 20];
        let intent_args = build_intent_args(&alice_owner_prefix, 1, 0, 5000, 4500);

        // Alice provides CKB input
        let input = make_input(alice, standard_lock.clone(), vec![0x22; 20], 500_00000000);

        // Intent lock output + alice change
        let outputs = vec![
            make_output(intent_owner, intent_code_hash, intent_args, 400_00000000),
            make_output(alice, standard_lock, vec![0x22; 20], 100_00000000),
        ];

        let tx = TxView {
            tx_hash: &[0x52; 32],
            block_hash: &[0xD2; 32],
            tx_index: 1,
            block_number: 6002,
            timestamp: 1_700_300_020,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        let alice_owner = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![alice; 32])
            .expect("alice should be present");
        assert_eq!(alice_owner.protocol_actions.len(), 1);
        assert_eq!(alice_owner.protocol_actions[0].protocol, "utxoswap");
        assert_eq!(
            alice_owner.protocol_actions[0].action,
            "add_liquidity_submitted"
        );

        let meta = alice_owner.protocol_actions[0].metadata_value().unwrap();
        assert_eq!(meta["intentType"], "AddLiquidity");
    }

    #[test]
    fn test_settled_no_match_for_wrong_prefix() {
        // Intent's owner_lock_hash doesn't match bob -> no settled action for bob
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let alice: u8 = 0xAA;
        let bob: u8 = 0xBB;
        let intent_cell_owner: u8 = 0xF0;

        // Intent args encode alice's lock_hash prefix
        let alice_owner_prefix: [u8; 20] = [alice; 20];
        let intent_args = build_intent_args(&alice_owner_prefix, 3, 0, 1000, 500);

        // Intent lock cell consumed from input
        let intent_input = make_input(
            intent_cell_owner,
            intent_code_hash,
            intent_args,
            300_00000000,
        );

        // Bob receives output (not alice)
        let outputs = vec![make_output(
            bob,
            standard_lock,
            vec![0x33; 20],
            300_00000000,
        )];

        let tx = TxView {
            tx_hash: &[0x53; 32],
            block_hash: &[0xD3; 32],
            tx_index: 1,
            block_number: 6003,
            timestamp: 1_700_300_030,
            is_cellbase: false,
            inputs: vec![intent_input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        // Bob should have no utxoswap actions because the prefix doesn't match
        let bob_owner = bundles[0]
            .owners
            .iter()
            .find(|o| o.lock_hash == vec![bob; 32])
            .expect("bob should be present");
        assert!(
            bob_owner
                .protocol_actions
                .iter()
                .all(|pa| pa.protocol != "utxoswap"),
            "bob should have no utxoswap actions because prefix mismatch"
        );
    }

    #[test]
    fn test_malformed_args_skipped() {
        // Short args on intent lock -> no actions
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let alice: u8 = 0xAA;
        let intent_owner: u8 = 0xF0;

        // Malformed: only 20 bytes (too short to parse)
        let short_args = vec![0x01; 20];

        // Alice provides CKB (ckb_delta < 0 to trigger submitted path)
        let input = make_input(alice, standard_lock.clone(), vec![0x22; 20], 500_00000000);

        let outputs = vec![
            make_output(
                intent_owner,
                intent_code_hash.clone(),
                short_args.clone(),
                300_00000000,
            ),
            make_output(alice, standard_lock, vec![0x22; 20], 200_00000000),
        ];

        let tx = TxView {
            tx_hash: &[0x54; 32],
            block_hash: &[0xD4; 32],
            tx_index: 1,
            block_number: 6004,
            timestamp: 1_700_300_040,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors)
                .unwrap();

        assert_eq!(bundles.len(), 1);

        for owner in &bundles[0].owners {
            assert!(
                owner
                    .protocol_actions
                    .iter()
                    .all(|pa| pa.protocol != "utxoswap"),
                "no utxoswap actions expected with malformed args"
            );
        }
    }
}
