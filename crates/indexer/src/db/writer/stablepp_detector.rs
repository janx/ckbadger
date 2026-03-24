//! Stable++ protocol detector: identifies CDP vault lifecycle events
//! by analyzing Vault Lock cell transitions and token deltas.

use ckbadger_store::types::{ItemDelta, LockCallEntry, ProtocolAction, TypeCallEntry, ITEM_KIND_TOKEN};

use crate::parser::stablepp::{
    is_stablepp_asset, is_stablepp_intent_lock, is_stablepp_script, is_stablepp_vault_lock,
};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

pub(crate) struct StableppDetector;

impl StableppDetector {
    pub fn new(_is_mainnet: bool) -> Self {
        Self
    }

    /// Check if any part of the transaction involves a Stable++ script.
    /// Checks type_calls/lock_calls AND directly scans tx inputs/outputs,
    /// because Stable++ Asset cells are classified as UDT upstream and
    /// won't appear in type_calls.
    fn has_stablepp_scripts(
        &self,
        tx: &TxView<'_>,
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> bool {
        type_calls
            .iter()
            .any(|tc| is_stablepp_script(&tc.type_code_hash))
            || lock_calls
                .iter()
                .any(|lc| is_stablepp_script(&lc.lock_code_hash))
            || tx.inputs.iter().any(|input| {
                is_stablepp_script(input.lock_code_hash)
                    || input.type_code_hash.is_some_and(is_stablepp_script)
            })
            || tx.outputs.iter().any(|output| {
                is_stablepp_script(output.lock_code_hash)
                    || output.type_code_hash.is_some_and(is_stablepp_script)
            })
    }

    /// Count vault lock cells in inputs and outputs.
    /// Returns `(vault_in_count, vault_out_count)`.
    fn count_vault_cells(&self, tx: &TxView<'_>) -> (usize, usize) {
        let vault_in = tx
            .inputs
            .iter()
            .filter(|input| is_stablepp_vault_lock(input.lock_code_hash))
            .count();
        let vault_out = tx
            .outputs
            .iter()
            .filter(|output| is_stablepp_vault_lock(output.lock_code_hash))
            .count();
        (vault_in, vault_out)
    }

    /// Check if any input has the Intent Lock code_hash.
    fn has_intent_in_inputs(&self, tx: &TxView<'_>) -> bool {
        tx.inputs
            .iter()
            .any(|input| is_stablepp_intent_lock(input.lock_code_hash))
    }

    /// Collect type_script_hashes of Stable++ Asset cells from tx inputs/outputs.
    fn stablepp_asset_type_script_hashes(&self, tx: &TxView<'_>) -> Vec<Vec<u8>> {
        let mut hashes = Vec::new();
        for input in &tx.inputs {
            if let Some(tc) = input.type_code_hash {
                if is_stablepp_asset(tc) {
                    if let Some(tsh) = input.type_script_hash {
                        hashes.push(tsh.to_vec());
                    }
                }
            }
        }
        for output in &tx.outputs {
            if let Some(tc) = output.type_code_hash {
                if is_stablepp_asset(tc) {
                    if let Some(tsh) = output.type_script_hash {
                        hashes.push(tsh.to_vec());
                    }
                }
            }
        }
        hashes.sort();
        hashes.dedup();
        hashes
    }

    /// Sum Token deltas only for Stable++ Asset tokens (identified by type_script_hash).
    fn stablepp_token_delta(
        &self,
        item_deltas: &[ItemDelta],
        stablepp_type_script_hashes: &[Vec<u8>],
    ) -> i128 {
        item_deltas
            .iter()
            .filter(|d| d.kind == ITEM_KIND_TOKEN && stablepp_type_script_hashes.contains(&d.item_id))
            .map(|d| d.delta)
            .sum()
    }

    /// Infer the Stable++ action from vault cell transitions and token delta.
    fn infer_action(&self, vault_in: usize, vault_out: usize, token_delta: i128) -> &'static str {
        match (vault_in > 0, vault_out > 0) {
            (false, true) => {
                // Vault created in outputs, none consumed from inputs
                if token_delta > 0 {
                    "open_vault"
                } else if token_delta == 0 {
                    "deposit"
                } else {
                    // token_delta < 0: vault opened with immediate borrow
                    "open_vault"
                }
            }
            (true, true) => {
                // Vault in both inputs and outputs (vault updated)
                if token_delta > 0 {
                    "borrow"
                } else if token_delta < 0 {
                    "repay"
                } else {
                    "adjust"
                }
            }
            (true, false) => {
                // Vault consumed from inputs, none created in outputs
                if token_delta < 0 {
                    "close_vault"
                } else {
                    // token_delta >= 0: liquidation
                    "liquidation"
                }
            }
            (false, false) => {
                // No vault cells at all
                if token_delta != 0 {
                    "redemption"
                } else {
                    "interaction"
                }
            }
        }
    }
}

impl ProtocolDetector for StableppDetector {
    fn might_apply_batch(
        &self,
        lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
        type_code_hashes: &std::collections::HashSet<[u8; 32]>,
    ) -> bool {
        lock_code_hashes.iter().any(|h| is_stablepp_script(h))
            || type_code_hashes.iter().any(|h| is_stablepp_script(h))
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.inputs.iter().any(|input| {
            is_stablepp_script(input.lock_code_hash)
                || input.type_code_hash.is_some_and(is_stablepp_script)
        }) || tx.outputs.iter().any(|output| {
            is_stablepp_script(output.lock_code_hash)
                || output.type_code_hash.is_some_and(is_stablepp_script)
        })
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        _owner_lock_hash: &[u8],
        accum: &OwnerAccum<'_>,
        item_deltas: &[ItemDelta],
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction> {
        if !self.has_stablepp_scripts(tx, type_calls, lock_calls) {
            return vec![];
        }
        if accum.input_capacity == 0 {
            return vec![];
        }

        let (vault_in, vault_out) = self.count_vault_cells(tx);
        let has_intent = self.has_intent_in_inputs(tx);
        let stablepp_hashes = self.stablepp_asset_type_script_hashes(tx);
        let token_delta = self.stablepp_token_delta(item_deltas, &stablepp_hashes);

        let action = self.infer_action(vault_in, vault_out, token_delta);

        let vault_count = std::cmp::max(vault_in, vault_out);
        let metadata = serde_json::json!({
            "hasIntent": has_intent,
            "vaultCount": vault_count,
        });

        vec![ProtocolAction::new("stablepp", action, metadata)]
    }
}

#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::db::writer::activities::{
        build_tx_actions_for_block, OutputCellView, TxView,
    };
    use ckbadger_store::types::TAG_PROTOCOL;
    use crate::parser::stablepp::{
        INTENT_LOCK_CODE_HASH_MAINNET, POOL_CODE_HASH_MAINNET, VAULT_LOCK_CODE_HASH_MAINNET,
    };
    use crate::rpc::parse_hex_to_bytes;

    struct OwnedInput {
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
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> OwnedInput {
        OwnedInput {
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
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
    ) -> OwnedOutput {
        OwnedOutput {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash,
            lock_args,
            capacity,
            type_code_hash,
            type_args,
            data: vec![],
        }
    }

    #[test]
    fn test_no_stablepp_scripts_returns_empty() {
        // Standard lock only tx -> no stablepp protocol actions
        let standard_lock = vec![0x11; 32];
        let alice: u8 = 0xAA;
        let bob: u8 = 0xBB;

        let input = make_input(
            alice,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            None,
            None,
        );

        let outputs = vec![make_output(
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
            block_number: 5000,
            timestamp: 1_700_200_000,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        assert!(
            actions_list[0].protocol_actions.is_empty(),
            "no stablepp actions expected for standard-only tx"
        );
    }

    #[test]
    fn test_open_vault() {
        // Vault only in outputs + pool type call, no token delta -> deposit
        // (open_vault requires positive token delta, i.e. minted RUSD)
        let vault_code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        let pool_code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let vault_owner: u8 = 0xF0;
        let participant: u8 = 0xAA;

        // Participant sends CKB, vault cell created in output
        let input = make_input(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            500_00000000,
            Some(pool_code_hash.clone()),
            Some(vec![0x01; 32]),
        );

        let outputs = vec![
            make_output(
                vault_owner,
                vault_code_hash,
                vec![0xBB; 32],
                300_00000000,
                Some(pool_code_hash),
                Some(vec![0x01; 32]),
            ),
            make_output(
                participant,
                standard_lock,
                vec![0x22; 20],
                200_00000000,
                None,
                None,
            ),
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

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let deposit_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "stablepp" && a.action == "deposit")
            .expect("should have stablepp deposit action");
        let meta = deposit_action.metadata_value().unwrap();
        assert_eq!(meta["vaultCount"], 1);
    }

    #[test]
    fn test_close_vault() {
        // Vault only in inputs, token_delta < 0 -> close_vault
        let vault_code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        let pool_code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let vault_owner: u8 = 0xF0;
        let participant: u8 = 0xAA;

        // Vault cell consumed from input
        let vault_input = make_input(
            vault_owner,
            vault_code_hash,
            vec![0xBB; 32],
            300_00000000,
            Some(pool_code_hash.clone()),
            Some(vec![0x01; 32]),
        );

        // Participant also provides input (with pool type to trigger type_calls)
        let participant_input = make_input(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            100_00000000,
            Some(pool_code_hash),
            Some(vec![0x01; 32]),
        );

        // Output goes to participant (no vault)
        let outputs = vec![make_output(
            participant,
            standard_lock,
            vec![0x22; 20],
            400_00000000,
            None,
            None,
        )];

        let tx = TxView {
            tx_hash: &[0x51; 32],
            block_hash: &[0xD1; 32],
            tx_index: 1,
            block_number: 6001,
            timestamp: 1_700_300_010,
            is_cellbase: false,
            inputs: vec![vault_input.view(), participant_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let liquidation_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "stablepp" && a.action == "liquidation")
            .expect("should have stablepp liquidation action");
        assert_eq!(liquidation_action.protocol, "stablepp");
    }

    #[test]
    fn test_adjust_vault() {
        // Vault in both inputs and outputs, no token delta -> adjust
        let vault_code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        let pool_code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let vault_owner: u8 = 0xF0;
        let participant: u8 = 0xAA;

        // Vault cell consumed from input
        let vault_input = make_input(
            vault_owner,
            vault_code_hash.clone(),
            vec![0xBB; 32],
            300_00000000,
            Some(pool_code_hash.clone()),
            Some(vec![0x01; 32]),
        );

        let participant_input = make_input(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            100_00000000,
            Some(pool_code_hash.clone()),
            Some(vec![0x01; 32]),
        );

        // Vault cell created in output (vault continues)
        let outputs = vec![
            make_output(
                vault_owner,
                vault_code_hash,
                vec![0xBB; 32],
                350_00000000,
                Some(pool_code_hash),
                Some(vec![0x01; 32]),
            ),
            make_output(
                participant,
                standard_lock,
                vec![0x22; 20],
                50_00000000,
                None,
                None,
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x52; 32],
            block_hash: &[0xD2; 32],
            tx_index: 1,
            block_number: 6002,
            timestamp: 1_700_300_020,
            is_cellbase: false,
            inputs: vec![vault_input.view(), participant_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let adjust_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "stablepp" && a.action == "adjust")
            .expect("should have stablepp adjust action");
        let meta = adjust_action.metadata_value().unwrap();
        assert_eq!(meta["vaultCount"], 1);
        assert_eq!(meta["hasIntent"], false);
    }

    #[test]
    fn test_intent_lock_metadata() {
        // Intent lock in inputs -> hasIntent: true
        let vault_code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        let intent_code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let pool_code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let vault_owner: u8 = 0xF0;
        let intent_owner: u8 = 0xF1;
        let participant: u8 = 0xAA;

        // Intent lock cell consumed from input
        let intent_input = make_input(
            intent_owner,
            intent_code_hash,
            vec![0xCC; 32],
            100_00000000,
            Some(pool_code_hash.clone()),
            Some(vec![0x01; 32]),
        );

        let participant_input = make_input(
            participant,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            None,
            None,
        );

        // Vault created in output
        let outputs = vec![
            make_output(
                vault_owner,
                vault_code_hash,
                vec![0xBB; 32],
                250_00000000,
                Some(pool_code_hash),
                Some(vec![0x01; 32]),
            ),
            make_output(
                participant,
                standard_lock,
                vec![0x22; 20],
                50_00000000,
                None,
                None,
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x53; 32],
            block_hash: &[0xD3; 32],
            tx_index: 1,
            block_number: 6003,
            timestamp: 1_700_300_030,
            is_cellbase: false,
            inputs: vec![intent_input.view(), participant_input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let stablepp_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "stablepp")
            .expect("should have stablepp action");
        let meta = stablepp_action.metadata_value().unwrap();
        assert_eq!(meta["hasIntent"], true);
    }

    #[test]
    fn test_fallback_interaction() {
        // Pool type script but no vault cells, no token delta -> interaction
        let pool_code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let alice: u8 = 0xAA;
        let bob: u8 = 0xBB;

        // Input with pool type script
        let input = make_input(
            alice,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            Some(pool_code_hash.clone()),
            Some(vec![0x01; 32]),
        );

        // Output with pool type script (no vault cells)
        let outputs = vec![make_output(
            bob,
            standard_lock,
            vec![0x33; 20],
            200_00000000,
            Some(pool_code_hash),
            Some(vec![0x01; 32]),
        )];

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

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        let has_interaction = actions
            .protocol_actions
            .iter()
            .any(|a| a.protocol == "stablepp" && a.action == "interaction");
        assert!(
            has_interaction,
            "expected interaction action for pool-only tx"
        );
    }

    #[test]
    fn test_infer_action_truth_table() {
        let detector = StableppDetector::new(true);

        // (vault_in, vault_out, token_delta) -> expected action
        // false, true, positive -> open_vault
        assert_eq!(detector.infer_action(0, 1, 100), "open_vault");
        // false, true, zero -> deposit
        assert_eq!(detector.infer_action(0, 1, 0), "deposit");
        // false, true, negative -> open_vault
        assert_eq!(detector.infer_action(0, 1, -50), "open_vault");

        // true, true, positive -> borrow
        assert_eq!(detector.infer_action(1, 1, 100), "borrow");
        // true, true, negative -> repay
        assert_eq!(detector.infer_action(1, 1, -100), "repay");
        // true, true, zero -> adjust
        assert_eq!(detector.infer_action(1, 1, 0), "adjust");

        // true, false, negative -> close_vault
        assert_eq!(detector.infer_action(1, 0, -100), "close_vault");
        // true, false, zero -> liquidation
        assert_eq!(detector.infer_action(1, 0, 0), "liquidation");
        // true, false, positive -> liquidation
        assert_eq!(detector.infer_action(1, 0, 100), "liquidation");

        // false, false, positive or negative -> redemption
        assert_eq!(detector.infer_action(0, 0, 100), "redemption");
        assert_eq!(detector.infer_action(0, 0, -100), "redemption");
        // false, false, zero -> interaction
        assert_eq!(detector.infer_action(0, 0, 0), "interaction");
    }

    #[test]
    fn test_vault_lock_in_outputs_triggers_detection() {
        // Vault lock in outputs triggers detection via lock_calls
        // (Asset code hash is xudt_compatible, classified as UDT, won't appear in type_calls)
        let vault_code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        let standard_lock = vec![0x11; 32];

        let vault_owner: u8 = 0xF0;
        let alice: u8 = 0xAA;

        let input = make_input(
            alice,
            standard_lock.clone(),
            vec![0x22; 20],
            200_00000000,
            None,
            None,
        );

        let outputs = vec![
            make_output(
                vault_owner,
                vault_code_hash,
                vec![0xBB; 32],
                150_00000000,
                None,
                None,
            ),
            make_output(
                alice,
                standard_lock,
                vec![0x22; 20],
                50_00000000,
                None,
                None,
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x55; 32],
            block_hash: &[0xD5; 32],
            tx_index: 1,
            block_number: 6005,
            timestamp: 1_700_300_050,
            is_cellbase: false,
            inputs: vec![input.view()],
            outputs: outputs.iter().map(|o| o.view()).collect(),
        };

        let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(StableppDetector::new(true))];
        let actions_list =
            build_tx_actions_for_block(&[tx], &detectors).unwrap();

        assert_eq!(actions_list.len(), 1);
        let actions = &actions_list[0];

        // Protocol actions are TX-level — check for deposit
        let deposit_action = actions
            .protocol_actions
            .iter()
            .find(|a| a.protocol == "stablepp" && a.action == "deposit")
            .expect("should have stablepp deposit action");
        assert_eq!(deposit_action.protocol, "stablepp");
    }
}
