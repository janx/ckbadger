//! UTXOSwap protocol detector: identifies intent lifecycle events
//! (submitted / settled) by analyzing Intent Lock cell transitions.

use ckbadger_store::types::{ItemDelta, LockCallEntry, ProtocolAction, TypeCallEntry};

use crate::parser::utxoswap::{is_intent_lock, parse_intent_args};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

pub(crate) struct UtxoSwapDetector;

impl UtxoSwapDetector {
    pub fn new(_is_mainnet: bool) -> Self {
        Self
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
            .any(|input| is_intent_lock(input.lock_code_hash))
            || tx
                .outputs
                .iter()
                .any(|output| is_intent_lock(output.lock_code_hash))
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        accum: &OwnerAccum<'_>,
        _item_deltas: &[ItemDelta],
        _type_calls: &[TypeCallEntry],
        _lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction> {
        let mut actions = Vec::new();

        let ckb_delta = accum.output_capacity - accum.input_capacity;

        // --- Submitted: intent lock on outputs, only for the intent's owner ---
        if ckb_delta < 0 {
            for output in &tx.outputs {
                if !is_intent_lock(output.lock_code_hash) {
                    continue;
                }
                let parsed = match parse_intent_args(output.lock_args) {
                    Some(p) => p,
                    None => continue,
                };

                // Only attribute to the actual intent owner, not co-signers or fee sponsors
                if owner_lock_hash.len() < 20 || owner_lock_hash[..20] != parsed.owner_lock_hash[..]
                {
                    continue;
                }

                let action_name = format!("{}_submitted", parsed.intent_type.action_name());

                actions.push(ProtocolAction::new(
                    "utxoswap",
                    action_name,
                    parsed.metadata_json(),
                ));
            }
        }

        // --- Settled: intent lock on inputs, owner_lock_hash prefix match ---
        for input in &tx.inputs {
            if !is_intent_lock(input.lock_code_hash) {
                continue;
            }
            let parsed = match parse_intent_args(input.lock_args) {
                Some(p) => p,
                None => continue,
            };

            // Check if the first 20 bytes of the current owner's lock_hash
            // match the owner_lock_hash stored in the intent args.
            if owner_lock_hash.len() >= 20 && owner_lock_hash[..20] == parsed.owner_lock_hash[..] {
                let action_name = format!("{}_settled", parsed.intent_type.action_name());

                actions.push(ProtocolAction::new(
                    "utxoswap",
                    action_name,
                    parsed.metadata_json(),
                ));
            }
        }

        actions
    }
}

#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use super::*;
    use crate::db::writer::activities::{build_tx_actions_for_block, OutputCellView, TxView};
    use crate::parser::utxoswap::INTENT_LOCK_CODE_HASH_MAINNET;
    use crate::rpc::parse_hex_to_bytes;

    struct OwnedInput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        data: Vec<u8>,
    }

    impl OwnedInput {
        fn view(&self) -> crate::db::writer::activities::InputCellView<'_> {
            crate::db::writer::activities::InputCellView {
                previous_tx_hash: &[0u8; 32],
                previous_output_index: 0,
                lock_script_hash: &self.lock_script_hash,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                capacity: self.capacity,
                occupied_capacity: 61_00000000,
                type_code_hash: None,
                type_hash_type: Some(1),
                type_script_hash: None,
                type_args: None,
                udt_amount: None,
                bit_cell_identity_id: None,
                data: &self.data,
                is_dao_withdraw_request: false,
                dao_compensation: None,
            }
        }
    }

    fn make_input(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
    ) -> OwnedInput {
        OwnedInput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_args,
            capacity,
            data: vec![],
        }
    }

    struct OwnedOutput {
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
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
                type_code_hash: None,
                type_hash_type: Some(1),
                type_args: None,
                type_script_hash: None,
                data_hash: &[],
                data_size: 0,
                data: &self.data,
            }
        }
    }

    fn make_output(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
    ) -> OwnedOutput {
        OwnedOutput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_args,
            capacity,
            data: vec![],
        }
    }

    /// Build 90-byte swap-layout intent lock args.
    /// The `owner_lock_hash_prefix` is the 20-byte prefix written into args[0..20].
    /// Only intent types 3 and 4 genuinely carry this layout; passing another
    /// type deliberately produces a length/type mismatch.
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
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        assert!(
            actions_list[0].protocol_actions.is_empty(),
            "no utxoswap actions expected for standard-only tx"
        );
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
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let submit_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "utxoswap" && a.action == "swap_exact_input_submitted")
            .expect("should have utxoswap swap_exact_input_submitted action");
        let meta = submit_action.metadata_value().unwrap();
        assert_eq!(meta["intentType"], "SwapExactInputForOutput");
        assert_eq!(meta["amountIn"], "1000");
        assert_eq!(meta["amountOutMin"], "500");
        assert_eq!(meta["assetInIndex"], 0);
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
            inputs: vec![intent_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let settle_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "utxoswap" && a.action == "swap_exact_input_settled")
            .expect("should have utxoswap swap_exact_input_settled action");
        let meta = settle_action.metadata_value().unwrap();
        assert_eq!(meta["intentType"], "SwapExactInputForOutput");
        assert_eq!(meta["amountIn"], "2000");
        assert_eq!(meta["amountOutMin"], "1500");
        assert_eq!(meta["assetInIndex"], 1);
    }

    /// A type-1 intent carrying the 90-byte swap layout is malformed: the real
    /// AddLiquidity payload is 121 bytes. The action name still derives from the
    /// (trustworthy, type-independent) header, but no amounts may be invented —
    /// this is exactly the case the old catch-all decode turned into garbage.
    #[test]
    fn test_add_liquidity_submitted() {
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
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let add_liq_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "utxoswap" && a.action == "add_liquidity_submitted")
            .expect("should have utxoswap add_liquidity_submitted action");
        let meta = add_liq_action.metadata_value().unwrap();
        assert_eq!(meta["intentType"], "AddLiquidity");

        // Malformed payload -> flagged, never decoded with the swap layout.
        assert_eq!(meta["payloadUnparsed"], true);
        assert_eq!(meta["argsLen"], 90);
        assert!(meta.get("desiredX").is_none(), "meta: {meta}");
        assert!(meta.get("amountIn").is_none(), "meta: {meta}");
        assert!(meta.get("amountOutMin").is_none(), "meta: {meta}");
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
            inputs: vec![intent_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        // No utxoswap actions at TX level because prefix doesn't match bob
        assert!(
            !actions
                .protocol_actions
                .iter()
                .any(|a| a.protocol == "utxoswap"),
            "no utxoswap actions expected because prefix mismatch"
        );
    }

    // =====================================================================
    // Real captured mainnet vectors (byte-exact intent lock args fetched from
    // a local CKB node on 2026-08-03). These pin the PER-TYPE payload layouts:
    //   type 1 AddLiquidity    = 121 B: 4×u128 LE at [57..121]
    //                            (desired_x, min_x, desired_y, min_y)
    //   type 2 RemoveLiquidity = 105 B: 3×u128 LE at [57..105]
    //                            (lp_amount, min_x, min_y)
    //   type 3/4 swaps         =  90 B: index u8 + 2×u128 LE (unchanged)
    // The old decoder applied the 90-B swap layout to every non-CreatePool
    // type, yielding 2^127-scale garbage amounts for types 1/2.
    // =====================================================================

    /// AddLiquidity submit tx
    /// 0x18d1b37ea5a3e83a9a58cb11ec164fb161b4d29f66543c69786a2108f62e7684
    /// (mainnet block 14,046,271; outputs 0 and 1 carry this 121-byte args).
    /// True decode: desired_x=9969978, min_x=9920128 (0.5% below),
    /// desired_y=224336, min_y=223214 (0.5% below).
    const REAL_ADD_LIQUIDITY_ARGS: &str = "0x0001d85947f67df16556a1caef3b7f939a69fb2329273406698f36e9bdf46db404176859b0ba3a6b00000000000000000000000000000000013a219800000000000000000000000000805e9700000000000000000000000000506c0300000000000000000000000000ee670300000000000000000000000000";

    /// The 2^127-scale garbage the old catch-all decode produced for the
    /// AddLiquidity vector above (2^127 + (9969978 >> 8)). Must never appear.
    const OLD_ADD_LIQUIDITY_GARBAGE: &str = "170141183460469231731687303715884144673";

    /// RemoveLiquidity submit tx
    /// 0x416ed0a39468cf54179f23aa25626a92ee8fdb5117c8418545d4e0bb8cf53a7e
    /// (mainnet block 20,003,047; output 0 carries this 105-byte args).
    /// True decode: lp_amount=52147210375003, min_x=5728619911607,
    /// min_y=516029247141147.
    const REAL_REMOVE_LIQUIDITY_ARGS: &str = "0xc41696293f5b16b471f9116631da82a4102c5b01b82e9073fee07b9caf625f0be45d3ec061be221200000000000000000100000000000000025b4ff3776d2f00000000000000000000b7d95acc3505000000000000000000001b35f86b53d501000000000000000000";

    /// Old catch-all garbage for the RemoveLiquidity vector above.
    const OLD_REMOVE_LIQUIDITY_GARBAGE_IN: &str = "243248723228639604741396692235003097935";
    const OLD_REMOVE_LIQUIDITY_GARBAGE_OUT: &str = "35889155886192728568402790649946725081";

    /// SwapExactInputForOutput submit tx
    /// 0x44f659be62ba97589f44d24c26eceab9db0501c364fc6e87e381e62f7e5759c4
    /// (mainnet block 13,845,652; output 0 carries this 90-byte args).
    /// True decode: asset_in_index=1, amount_in=730000000,
    /// amount_out_min=147392188210 — swap decoding must stay UNCHANGED.
    const REAL_SWAP_ARGS: &str = "0xbefc0a6053441e9bcba6d3f6c1599c37a1d8187a235edb927fc68f446e06f2e677fb52aa7f158ae800000000000000000000000000000000030180ea822b000000000000000000000000324f4251220000000000000000000000";

    /// The intent args carry the first 20 bytes of the owner's lock hash in
    /// [0..20]; build a 32-byte owner lock_script_hash with that real prefix.
    fn owner_hash_from_args_prefix(args: &[u8]) -> Vec<u8> {
        let mut hash = vec![0x77u8; 32];
        hash[..20].copy_from_slice(&args[..20]);
        hash
    }

    /// Run the detector over a submit-shaped tx (owner funds an intent output)
    /// and return the single utxoswap action's (action, metadata).
    fn detect_submitted(intent_args: &[u8]) -> (String, serde_json::Value) {
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];
        let owner_hash = owner_hash_from_args_prefix(intent_args);

        let input = OwnedInput {
            lock_script_hash: owner_hash.clone(),
            lock_code_hash: standard_lock.clone(),
            lock_args: vec![0x22; 20],
            capacity: 700_00000000,
            data: vec![],
        };
        let intent_output = OwnedOutput {
            lock_script_hash: vec![0xF0; 32],
            lock_code_hash: intent_code_hash,
            lock_args: intent_args.to_vec(),
            capacity: 300_00000000,
            data: vec![],
        };
        let change_output = OwnedOutput {
            lock_script_hash: owner_hash,
            lock_code_hash: standard_lock,
            lock_args: vec![0x22; 20],
            capacity: 300_00000000,
            data: vec![],
        };
        let outputs = vec![intent_output, change_output];

        let tx = TxView {
            tx_hash: &[0x60; 32],
            block_hash: &[0xE0; 32],
            tx_index: 1,
            block_number: 14_046_271,
            timestamp: 1_700_400_000,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();
        let utxoswap: Vec<_> = actions_list[0]
            .protocol_actions
            .iter()
            .filter(|a| a.protocol == "utxoswap")
            .collect();
        assert_eq!(
            utxoswap.len(),
            1,
            "expected exactly one utxoswap action, got: {:?}",
            utxoswap
                .iter()
                .map(|a| a.action.clone())
                .collect::<Vec<_>>()
        );
        (
            utxoswap[0].action.clone(),
            utxoswap[0].metadata_value().unwrap(),
        )
    }

    #[test]
    fn real_add_liquidity_vector_decodes_four_u128_payload() {
        let args = parse_hex_to_bytes(REAL_ADD_LIQUIDITY_ARGS);
        assert_eq!(args.len(), 121, "captured AddLiquidity args must be 121 B");
        let (action, meta) = detect_submitted(&args);

        assert_eq!(action, "add_liquidity_submitted");
        assert_eq!(meta["intentType"], "AddLiquidity");
        assert_eq!(
            meta["poolTypeHash"],
            "0x29273406698f36e9bdf46db404176859b0ba3a6b"
        );
        assert_eq!(meta["desiredX"], "9969978");
        assert_eq!(meta["minX"], "9920128");
        assert_eq!(meta["desiredY"], "224336");
        assert_eq!(meta["minY"], "223214");

        // The swap-layout fields must be gone for AddLiquidity — they were
        // never part of this payload.
        assert!(meta.get("amountIn").is_none(), "meta: {meta}");
        assert!(meta.get("amountOutMin").is_none(), "meta: {meta}");
        assert!(meta.get("assetInIndex").is_none(), "meta: {meta}");
        assert!(
            !meta.to_string().contains(OLD_ADD_LIQUIDITY_GARBAGE),
            "old 2^127-scale garbage resurfaced: {meta}"
        );
    }

    #[test]
    fn real_remove_liquidity_vector_decodes_three_u128_payload() {
        let args = parse_hex_to_bytes(REAL_REMOVE_LIQUIDITY_ARGS);
        assert_eq!(
            args.len(),
            105,
            "captured RemoveLiquidity args must be 105 B"
        );
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];
        let owner_hash = owner_hash_from_args_prefix(&args);

        // Settle-shaped tx: the intent cell is consumed, the owner receives.
        let intent_input = OwnedInput {
            lock_script_hash: vec![0xF0; 32],
            lock_code_hash: intent_code_hash,
            lock_args: args.clone(),
            capacity: 300_00000000,
            data: vec![],
        };
        let output = OwnedOutput {
            lock_script_hash: owner_hash,
            lock_code_hash: standard_lock,
            lock_args: vec![0x22; 20],
            capacity: 300_00000000,
            data: vec![],
        };
        let outputs = vec![output];

        let tx = TxView {
            tx_hash: &[0x61; 32],
            block_hash: &[0xE1; 32],
            tx_index: 1,
            block_number: 20_003_047,
            timestamp: 1_700_400_010,
            is_cellbase: false,
            inputs: vec![intent_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();
        let utxoswap: Vec<_> = actions_list[0]
            .protocol_actions
            .iter()
            .filter(|a| a.protocol == "utxoswap")
            .collect();
        assert_eq!(utxoswap.len(), 1);
        assert_eq!(utxoswap[0].action, "remove_liquidity_settled");

        let meta = utxoswap[0].metadata_value().unwrap();
        assert_eq!(meta["intentType"], "RemoveLiquidity");
        assert_eq!(
            meta["poolTypeHash"],
            "0xb82e9073fee07b9caf625f0be45d3ec061be2212"
        );
        assert_eq!(meta["lpAmount"], "52147210375003");
        assert_eq!(meta["minX"], "5728619911607");
        assert_eq!(meta["minY"], "516029247141147");

        assert!(meta.get("amountIn").is_none(), "meta: {meta}");
        assert!(meta.get("amountOutMin").is_none(), "meta: {meta}");
        assert!(meta.get("assetInIndex").is_none(), "meta: {meta}");
        let raw = meta.to_string();
        assert!(
            !raw.contains(OLD_REMOVE_LIQUIDITY_GARBAGE_IN)
                && !raw.contains(OLD_REMOVE_LIQUIDITY_GARBAGE_OUT),
            "old 2^127-scale garbage resurfaced: {meta}"
        );
    }

    #[test]
    fn real_swap_vector_decoding_is_unchanged() {
        let args = parse_hex_to_bytes(REAL_SWAP_ARGS);
        assert_eq!(args.len(), 90, "captured swap args must be 90 B");
        let (action, meta) = detect_submitted(&args);

        assert_eq!(action, "swap_exact_input_submitted");
        assert_eq!(meta["intentType"], "SwapExactInputForOutput");
        assert_eq!(
            meta["poolTypeHash"],
            "0x235edb927fc68f446e06f2e677fb52aa7f158ae8"
        );
        assert_eq!(meta["assetInIndex"], 1);
        assert_eq!(meta["amountIn"], "730000000");
        assert_eq!(meta["amountOutMin"], "147392188210");
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
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(UtxoSwapDetector::new(true))];
        let actions_list = build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);

        assert!(
            !actions_list[0]
                .protocol_actions
                .iter()
                .any(|a| a.protocol == "utxoswap"),
            "no utxoswap actions expected with malformed args"
        );
    }
}
