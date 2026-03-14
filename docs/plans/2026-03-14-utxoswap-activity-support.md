# UTXOSwap Activity Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add UTXOSwap DEX protocol action detection so swap/liquidity intent submissions and settlements appear as identified protocol activities instead of generic CKB transfers.

**Architecture:** A `UtxoSwapDetector` implementing the `ProtocolDetector` trait scans transaction inputs/outputs for UTXOSwap intent lock cells, parses the 90/154-byte lock args to determine intent type, and emits protocol actions for both submission (output) and settlement (input). A lock args decoder in the API layer provides structured display of intent parameters.

**Tech Stack:** Rust (parser, detector, API decoder), follows existing patterns from `stablepp.rs`/`StableppDetector`/`FiberDetector`

**Design doc:** `docs/plans/2026-03-14-utxoswap-activity-support-design.md`

---

### Task 1: UTXOSwap parser module — code hash constants and identification

**Files:**

- Create: `crates/indexer/src/parser/utxoswap.rs`
- Modify: `crates/indexer/src/parser/mod.rs:1` (add module declaration)

**Step 1: Write the parser module with constants and `is_intent_lock`**

Create `crates/indexer/src/parser/utxoswap.rs`:

```rust
use std::sync::LazyLock;

use crate::rpc::parse_hex_to_bytes;

// UTXOSwap Intent Lock (lock script)
pub const INTENT_LOCK_CODE_HASH_MAINNET: &str =
    "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e";
pub const INTENT_LOCK_CODE_HASH_TESTNET: &str =
    "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101";

static INTENT_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET));
static INTENT_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET));

pub fn is_intent_lock(code_hash: &[u8]) -> bool {
    code_hash == INTENT_MAINNET.as_slice() || code_hash == INTENT_TESTNET.as_slice()
}
```

**Step 2: Add module declaration**

In `crates/indexer/src/parser/mod.rs`, add `pub mod utxoswap;` (alphabetical, after `udt`).

**Step 3: Write tests for code hash identification**

Append to `utxoswap.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn test_is_intent_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_intent_lock_testnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET);
        assert!(is_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_intent_lock_rejects_other() {
        assert!(!is_intent_lock(&[0xAA; 32]));
        assert!(!is_intent_lock(&[0u8; 32]));
    }

    #[test]
    fn test_all_hashes_are_32_bytes() {
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET).len(), 32);
    }

    #[test]
    fn test_hashes_are_distinct() {
        let mainnet = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let testnet = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET);
        assert_ne!(mainnet, testnet);
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer utxoswap -- --nocapture`

Expected: All 5 tests pass.

**Step 5: Commit**

```bash
git add crates/indexer/src/parser/utxoswap.rs crates/indexer/src/parser/mod.rs
git commit -m "feat(indexer): add UTXOSwap parser constants and code hash helpers"
```

---

### Task 2: Intent args parsing

**Files:**

- Modify: `crates/indexer/src/parser/utxoswap.rs`

**Step 1: Add IntentType enum and parsed types**

Add after `is_intent_lock`:

```rust
/// UTXOSwap intent type encoded as byte 56 of lock args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentType {
    CreatePool = 0,
    AddLiquidity = 1,
    RemoveLiquidity = 2,
    SwapExactInputForOutput = 3,
    SwapInputForExactOutput = 4,
    ClaimProtocolLiquidity = 5,
}

impl IntentType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::CreatePool),
            1 => Some(Self::AddLiquidity),
            2 => Some(Self::RemoveLiquidity),
            3 => Some(Self::SwapExactInputForOutput),
            4 => Some(Self::SwapInputForExactOutput),
            5 => Some(Self::ClaimProtocolLiquidity),
            _ => None,
        }
    }

    pub fn action_name(&self) -> &'static str {
        match self {
            Self::CreatePool => "create_pool",
            Self::AddLiquidity => "add_liquidity",
            Self::RemoveLiquidity => "remove_liquidity",
            Self::SwapExactInputForOutput => "swap_exact_input",
            Self::SwapInputForExactOutput => "swap_exact_output",
            Self::ClaimProtocolLiquidity => "claim_protocol_liquidity",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::CreatePool => "CreatePool",
            Self::AddLiquidity => "AddLiquidity",
            Self::RemoveLiquidity => "RemoveLiquidity",
            Self::SwapExactInputForOutput => "SwapExactInputForOutput",
            Self::SwapInputForExactOutput => "SwapInputForExactOutput",
            Self::ClaimProtocolLiquidity => "ClaimProtocolLiquidity",
        }
    }
}

/// Extra fields for CreatePool intent (args bytes 57..154).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePoolExtra {
    pub total_fee_rate: u8,
    pub asset_x: [u8; 32],
    pub asset_y: [u8; 32],
    pub amount_x: u128,
    pub amount_y: u128,
}

/// Parsed UTXOSwap intent lock args.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIntentArgs {
    pub owner_lock_hash: [u8; 20],
    pub pool_type_hash: [u8; 20],
    pub intent_type: IntentType,
    pub asset_in_index: u8,
    pub amount_in: u128,
    pub amount_out_min: u128,
    pub create_pool_extra: Option<CreatePoolExtra>,
}

/// Parse UTXOSwap intent lock args.
/// Returns `None` if args are too short, have an unknown intent_type, or
/// CreatePool args are shorter than 154 bytes.
pub fn parse_intent_args(args: &[u8]) -> Option<ParsedIntentArgs> {
    if args.len() < 90 {
        return None;
    }

    let mut owner_lock_hash = [0u8; 20];
    owner_lock_hash.copy_from_slice(&args[0..20]);

    let mut pool_type_hash = [0u8; 20];
    pool_type_hash.copy_from_slice(&args[20..40]);

    // bytes 40..56 = tx_fee (8) + expire_batch_id (8) — skipped

    let intent_type = IntentType::from_byte(args[56])?;

    let create_pool_extra = if intent_type == IntentType::CreatePool {
        if args.len() < 154 {
            return None;
        }
        let total_fee_rate = args[57];
        let mut asset_x = [0u8; 32];
        asset_x.copy_from_slice(&args[58..90]);
        let mut asset_y = [0u8; 32];
        asset_y.copy_from_slice(&args[90..122]);
        let amount_x = u128::from_le_bytes(args[122..138].try_into().unwrap());
        let amount_y = u128::from_le_bytes(args[138..154].try_into().unwrap());
        Some(CreatePoolExtra {
            total_fee_rate,
            asset_x,
            asset_y,
            amount_x,
            amount_y,
        })
    } else {
        None
    };

    // For non-CreatePool: byte 57 = asset_in_index, bytes 58..74 = amount_in, bytes 74..90 = amount_out_min
    // For CreatePool: these fields are not meaningful (overloaded layout), set to 0
    let (asset_in_index, amount_in, amount_out_min) = if intent_type == IntentType::CreatePool {
        (0, 0, 0)
    } else {
        let asset_in_index = args[57];
        let amount_in = u128::from_le_bytes(args[58..74].try_into().unwrap());
        let amount_out_min = u128::from_le_bytes(args[74..90].try_into().unwrap());
        (asset_in_index, amount_in, amount_out_min)
    };

    Some(ParsedIntentArgs {
        owner_lock_hash,
        pool_type_hash,
        intent_type,
        asset_in_index,
        amount_in,
        amount_out_min,
        create_pool_extra,
    })
}
```

**Step 2: Write tests for intent args parsing**

Append inside `#[cfg(test)] mod tests`:

```rust
    // --- Intent args parsing tests ---

    fn make_swap_args(intent_type: u8, asset_in_index: u8, amount_in: u128, amount_out_min: u128) -> Vec<u8> {
        let mut args = vec![0u8; 90];
        // owner_lock_hash [0..20]
        for i in 0..20 { args[i] = 0xAA; }
        // pool_type_hash [20..40]
        for i in 20..40 { args[i] = 0xBB; }
        // tx_fee [40..48] and expire_batch_id [48..56] — zero
        // intent_type [56]
        args[56] = intent_type;
        // asset_in_index [57]
        args[57] = asset_in_index;
        // amount_in [58..74]
        args[58..74].copy_from_slice(&amount_in.to_le_bytes());
        // amount_out_min [74..90]
        args[74..90].copy_from_slice(&amount_out_min.to_le_bytes());
        args
    }

    fn make_create_pool_args(fee_rate: u8, amount_x: u128, amount_y: u128) -> Vec<u8> {
        let mut args = vec![0u8; 154];
        for i in 0..20 { args[i] = 0xAA; }
        for i in 20..40 { args[i] = 0xBB; }
        args[56] = 0; // CreatePool
        args[57] = fee_rate;
        // asset_x [58..90]
        for i in 58..90 { args[i] = 0xCC; }
        // asset_y [90..122]
        for i in 90..122 { args[i] = 0xDD; }
        // amount_x [122..138]
        args[122..138].copy_from_slice(&amount_x.to_le_bytes());
        // amount_y [138..154]
        args[138..154].copy_from_slice(&amount_y.to_le_bytes());
        args
    }

    #[test]
    fn test_parse_swap_intent_args() {
        let args = make_swap_args(3, 0, 1_000_000_000, 500_000_000);
        let parsed = parse_intent_args(&args).unwrap();
        assert_eq!(parsed.intent_type, IntentType::SwapExactInputForOutput);
        assert_eq!(parsed.asset_in_index, 0);
        assert_eq!(parsed.amount_in, 1_000_000_000);
        assert_eq!(parsed.amount_out_min, 500_000_000);
        assert!(parsed.create_pool_extra.is_none());
        assert_eq!(parsed.owner_lock_hash, [0xAA; 20]);
        assert_eq!(parsed.pool_type_hash, [0xBB; 20]);
    }

    #[test]
    fn test_parse_add_liquidity_args() {
        let args = make_swap_args(1, 1, 2_000_000_000, 100);
        let parsed = parse_intent_args(&args).unwrap();
        assert_eq!(parsed.intent_type, IntentType::AddLiquidity);
        assert_eq!(parsed.asset_in_index, 1);
        assert_eq!(parsed.amount_in, 2_000_000_000);
        assert_eq!(parsed.amount_out_min, 100);
    }

    #[test]
    fn test_parse_create_pool_args() {
        let args = make_create_pool_args(30, 5_000_000_000, 10_000_000_000);
        let parsed = parse_intent_args(&args).unwrap();
        assert_eq!(parsed.intent_type, IntentType::CreatePool);
        let extra = parsed.create_pool_extra.unwrap();
        assert_eq!(extra.total_fee_rate, 30);
        assert_eq!(extra.asset_x, [0xCC; 32]);
        assert_eq!(extra.asset_y, [0xDD; 32]);
        assert_eq!(extra.amount_x, 5_000_000_000);
        assert_eq!(extra.amount_y, 10_000_000_000);
        // swap fields zeroed for CreatePool
        assert_eq!(parsed.asset_in_index, 0);
        assert_eq!(parsed.amount_in, 0);
        assert_eq!(parsed.amount_out_min, 0);
    }

    #[test]
    fn test_parse_intent_args_too_short() {
        let args = vec![0u8; 89];
        assert!(parse_intent_args(&args).is_none());
    }

    #[test]
    fn test_parse_intent_args_create_pool_too_short() {
        // intent_type=0 (CreatePool) but only 90 bytes (needs 154)
        let mut args = vec![0u8; 90];
        args[56] = 0;
        assert!(parse_intent_args(&args).is_none());
    }

    #[test]
    fn test_parse_intent_args_unknown_type() {
        let mut args = vec![0u8; 90];
        args[56] = 99;
        assert!(parse_intent_args(&args).is_none());
    }

    #[test]
    fn test_intent_type_action_names() {
        assert_eq!(IntentType::CreatePool.action_name(), "create_pool");
        assert_eq!(IntentType::AddLiquidity.action_name(), "add_liquidity");
        assert_eq!(IntentType::RemoveLiquidity.action_name(), "remove_liquidity");
        assert_eq!(IntentType::SwapExactInputForOutput.action_name(), "swap_exact_input");
        assert_eq!(IntentType::SwapInputForExactOutput.action_name(), "swap_exact_output");
        assert_eq!(IntentType::ClaimProtocolLiquidity.action_name(), "claim_protocol_liquidity");
    }

    #[test]
    fn test_all_intent_types_roundtrip() {
        for byte in 0..=5u8 {
            let it = IntentType::from_byte(byte).unwrap();
            assert_eq!(it as u8, byte);
        }
        assert!(IntentType::from_byte(6).is_none());
    }
```

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer utxoswap -- --nocapture`

Expected: All tests pass (5 from Task 1 + 8 new = 13 total).

**Step 4: Commit**

```bash
git add crates/indexer/src/parser/utxoswap.rs
git commit -m "feat(indexer): add UTXOSwap intent args parsing"
```

---

### Task 3: UtxoSwapDetector implementation

**Files:**

- Create: `crates/indexer/src/db/writer/utxoswap_detector.rs`
- Modify: `crates/indexer/src/db/writer.rs:67` (add module declaration)

**Step 1: Implement the detector**

Create `crates/indexer/src/db/writer/utxoswap_detector.rs`:

```rust
//! UTXOSwap protocol detector: identifies DEX intent submissions and settlements
//! by scanning for intent lock cells in transaction inputs and outputs.

use ckbadger_store::types::{AssetChange, LockCallEntry, ProtocolAction, TypeCallEntry};

use crate::parser::utxoswap::{is_intent_lock, parse_intent_args};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

pub(crate) struct UtxoSwapDetector {
    #[allow(dead_code)]
    is_mainnet: bool,
}

impl UtxoSwapDetector {
    pub fn new(is_mainnet: bool) -> Self {
        Self { is_mainnet }
    }

    /// Build metadata JSON from parsed intent args.
    fn build_metadata(
        &self,
        parsed: &crate::parser::utxoswap::ParsedIntentArgs,
    ) -> serde_json::Value {
        let mut meta = serde_json::json!({
            "intentType": parsed.intent_type.display_name(),
            "poolTypeHash": format!("0x{}", hex::encode(parsed.pool_type_hash)),
            "amountIn": parsed.amount_in.to_string(),
            "amountOutMin": parsed.amount_out_min.to_string(),
            "assetInIndex": parsed.asset_in_index,
        });

        if let Some(extra) = &parsed.create_pool_extra {
            meta["assetX"] = serde_json::json!(format!("0x{}", hex::encode(extra.asset_x)));
            meta["assetY"] = serde_json::json!(format!("0x{}", hex::encode(extra.asset_y)));
            meta["amountX"] = serde_json::json!(extra.amount_x.to_string());
            meta["amountY"] = serde_json::json!(extra.amount_y.to_string());
            meta["totalFeeRate"] = serde_json::json!(extra.total_fee_rate);
        }

        meta
    }
}

impl ProtocolDetector for UtxoSwapDetector {
    fn protocol_name(&self) -> &str {
        "utxoswap"
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

        // --- Submitted: intent lock on outputs ---
        // Only attribute to owners with CKB flowing out (funding the intent cell)
        let ckb_delta = accum.output_capacity - accum.input_capacity;
        if ckb_delta < 0 {
            for output in tx.outputs {
                if !is_intent_lock(&output.lock_code_hash) {
                    continue;
                }
                if let Some(parsed) = parse_intent_args(&output.lock_args) {
                    let action_name =
                        format!("{}_submitted", parsed.intent_type.action_name());
                    actions.push(ProtocolAction {
                        protocol: "utxoswap".to_string(),
                        action: action_name,
                        metadata: self.build_metadata(&parsed),
                    });
                }
            }
        }

        // --- Settled: intent lock on inputs ---
        // Attribute to owner whose lock_hash prefix matches the intent's owner_lock_hash (20 bytes)
        for input in &tx.inputs {
            if !is_intent_lock(&input.lock_code_hash) {
                continue;
            }
            if let Some(parsed) = parse_intent_args(&input.lock_args) {
                // Prefix match: compare first 20 bytes of owner's lock_hash
                if owner_lock_hash.len() >= 20
                    && owner_lock_hash[..20] == parsed.owner_lock_hash
                {
                    let action_name =
                        format!("{}_settled", parsed.intent_type.action_name());
                    actions.push(ProtocolAction {
                        protocol: "utxoswap".to_string(),
                        action: action_name,
                        metadata: self.build_metadata(&parsed),
                    });
                }
            }
        }

        actions
    }
}
```

**Step 2: Add module declaration**

In `crates/indexer/src/db/writer.rs`, add after `pub(crate) mod stablepp_detector;`:

```rust
pub(crate) mod utxoswap_detector;
```

**Step 3: Write tests**

Append to `utxoswap_detector.rs`. The tests follow the exact pattern from `stablepp_detector.rs` — using `build_activity_bundles_for_block_with_detectors` with mock `TxView` data.

The key helper: test intent lock args are constructed with `make_swap_args` / `make_create_pool_args` from the parser (but since those are private to `parser::utxoswap::tests`, we rebuild minimal helpers here).

```rust
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

    fn intent_lock_code_hash() -> Vec<u8> {
        parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET)
    }

    fn standard_lock() -> Vec<u8> {
        vec![0x11; 32]
    }

    /// Build 90-byte swap intent args with the given owner_lock_hash prefix.
    fn make_intent_args(owner_prefix: &[u8; 20], intent_type: u8) -> Vec<u8> {
        let mut args = vec![0u8; 90];
        args[0..20].copy_from_slice(owner_prefix);
        // pool_type_hash [20..40]
        for i in 20..40 {
            args[i] = 0xBB;
        }
        // tx_fee + expire_batch_id [40..56] = 0
        args[56] = intent_type;
        args[57] = 0; // asset_in_index
        // amount_in [58..74] = 1000
        args[58..74].copy_from_slice(&1000u128.to_le_bytes());
        // amount_out_min [74..90] = 500
        args[74..90].copy_from_slice(&500u128.to_le_bytes());
        args
    }

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

    fn run_detector(tx: TxView<'_>) -> Vec<(Vec<u8>, Vec<ProtocolAction>)> {
        let detectors: Vec<Box<dyn ProtocolDetector>> =
            vec![Box::new(UtxoSwapDetector::new(true))];
        let bundles =
            build_activity_bundles_for_block_with_detectors(&[tx], &HashMap::new(), &detectors);
        bundles
            .into_iter()
            .flat_map(|b| {
                b.owners
                    .into_iter()
                    .filter(|o| !o.protocol_actions.is_empty())
                    .map(|o| (o.lock_script_hash.clone(), o.protocol_actions.clone()))
            })
            .collect()
    }

    #[test]
    fn test_utxoswap_detector_protocol_name() {
        let detector = UtxoSwapDetector::new(true);
        assert_eq!(detector.protocol_name(), "utxoswap");
    }

    #[test]
    fn test_no_intent_lock_returns_empty() {
        let alice: u8 = 0xAA;
        let bob: u8 = 0xBB;

        let input = make_input(alice, standard_lock(), vec![0x22; 20], 200_00000000);
        let outputs = vec![make_output(bob, standard_lock(), vec![0x33; 20], 200_00000000)];

        let tx = TxView {
            tx_hash: &[0x44; 32],
            block_hash: &[0xC4; 32],
            tx_index: 1,
            block_number: 5000,
            timestamp: 1_700_200_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };

        let results = run_detector(tx);
        assert!(results.is_empty(), "no utxoswap actions for standard-only tx");
    }

    #[test]
    fn test_swap_submitted() {
        // Alice (standard lock) sends CKB to an intent lock output -> submitted
        let alice: u8 = 0xAA;
        let intent_args = make_intent_args(&[0xAA; 20], 3); // SwapExactInputForOutput

        let input = make_input(alice, standard_lock(), vec![alice; 20], 200_00000000);
        let outputs = vec![make_output(
            0xEE, // intent lock has its own lock_hash
            intent_lock_code_hash(),
            intent_args,
            150_00000000,
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
            witnesses: &[],
        };

        let results = run_detector(tx);
        assert_eq!(results.len(), 1);
        let (_, actions) = &results[0];
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].protocol, "utxoswap");
        assert_eq!(actions[0].action, "swap_exact_input_submitted");
        assert_eq!(actions[0].metadata["intentType"], "SwapExactInputForOutput");
    }

    #[test]
    fn test_swap_settled() {
        // Sequencer consumes intent lock input, Alice receives output
        // Intent args have owner_lock_hash = first 20 bytes of alice's lock_hash
        let alice: u8 = 0xAA;
        let alice_lock_hash_prefix: [u8; 20] = [alice; 20];
        let intent_args = make_intent_args(&alice_lock_hash_prefix, 3);

        // Intent cell being consumed (input)
        let intent_input = make_input(
            0xEE, // intent cell's lock_hash
            intent_lock_code_hash(),
            intent_args,
            150_00000000,
        );

        // Alice receives the swap result (output with standard lock)
        let outputs = vec![make_output(alice, standard_lock(), vec![alice; 20], 140_00000000)];

        let tx = TxView {
            tx_hash: &[0x55; 32],
            block_hash: &[0xC5; 32],
            tx_index: 2,
            block_number: 5001,
            timestamp: 1_700_200_100,
            is_cellbase: false,
            inputs: vec![intent_input],
            outputs: &outputs,
            witnesses: &[],
        };

        let results = run_detector(tx);
        // Alice should have a settled action (prefix match)
        let alice_actions: Vec<_> = results
            .iter()
            .filter(|(lh, _)| lh == &vec![alice; 32])
            .collect();
        assert_eq!(alice_actions.len(), 1);
        assert_eq!(alice_actions[0].1[0].action, "swap_exact_input_settled");
    }

    #[test]
    fn test_add_liquidity_submitted() {
        let alice: u8 = 0xAA;
        let intent_args = make_intent_args(&[0xAA; 20], 1); // AddLiquidity

        let input = make_input(alice, standard_lock(), vec![alice; 20], 200_00000000);
        let outputs = vec![make_output(
            0xEE,
            intent_lock_code_hash(),
            intent_args,
            150_00000000,
        )];

        let tx = TxView {
            tx_hash: &[0x66; 32],
            block_hash: &[0xC6; 32],
            tx_index: 1,
            block_number: 6000,
            timestamp: 1_700_300_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };

        let results = run_detector(tx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1[0].action, "add_liquidity_submitted");
    }

    #[test]
    fn test_settled_no_match_for_wrong_prefix() {
        // Intent has owner_lock_hash = [0xAA; 20], but only Bob (0xBB) is in the tx
        let bob: u8 = 0xBB;
        let intent_args = make_intent_args(&[0xAA; 20], 3);

        let intent_input = make_input(0xEE, intent_lock_code_hash(), intent_args, 150_00000000);
        let outputs = vec![make_output(bob, standard_lock(), vec![bob; 20], 140_00000000)];

        let tx = TxView {
            tx_hash: &[0x77; 32],
            block_hash: &[0xC7; 32],
            tx_index: 1,
            block_number: 7000,
            timestamp: 1_700_400_000,
            is_cellbase: false,
            inputs: vec![intent_input],
            outputs: &outputs,
            witnesses: &[],
        };

        let results = run_detector(tx);
        // Bob should NOT get a settled action (prefix mismatch)
        let bob_settled: Vec<_> = results
            .iter()
            .filter(|(_, actions)| actions.iter().any(|a| a.action.contains("settled")))
            .collect();
        assert!(bob_settled.is_empty());
    }

    #[test]
    fn test_malformed_args_skipped() {
        // Intent lock output with args too short -> no protocol action
        let alice: u8 = 0xAA;
        let short_args = vec![0u8; 50]; // < 90 bytes

        let input = make_input(alice, standard_lock(), vec![alice; 20], 200_00000000);
        let outputs = vec![make_output(
            0xEE,
            intent_lock_code_hash(),
            short_args,
            150_00000000,
        )];

        let tx = TxView {
            tx_hash: &[0x88; 32],
            block_hash: &[0xC8; 32],
            tx_index: 1,
            block_number: 8000,
            timestamp: 1_700_500_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };

        let results = run_detector(tx);
        assert!(results.is_empty(), "malformed args should be skipped");
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer utxoswap -- --nocapture`

Expected: All tests pass (13 parser + 7 detector = 20 total).

**Step 5: Commit**

```bash
git add crates/indexer/src/db/writer/utxoswap_detector.rs crates/indexer/src/db/writer.rs
git commit -m "feat(indexer): implement UtxoSwapDetector for intent lifecycle detection"
```

---

### Task 4: Register detector in sync pipeline

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:1676-1686` (bulk sync detector list)
- Modify: `crates/indexer/src/sync/batch.rs:4111-4121` (live sync detector list)

**Step 1: Add UtxoSwapDetector to both detector lists**

In `crates/indexer/src/sync/batch.rs`, find the bulk sync detector list (~line 1676) and add after `StableppDetector`:

```rust
Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
    self.config.is_mainnet(),
)),
```

Find the live sync detector list (~line 4111) and add the same line after `StableppDetector`.

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-indexer`

Expected: Compiles without errors.

**Step 3: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "feat(indexer): register UtxoSwapDetector in bulk and live sync pipelines"
```

---

### Task 5: API lock args decoder

**Files:**

- Modify: `crates/api/src/routes/activities.rs` (add decoder function and register in `LOCK_ARGS_DECODERS`)

**Step 1: Add decoder function**

After `decode_fiber_commitment_lock_args` (~line 395), add:

```rust
fn decode_utxoswap_intent_args(args: &[u8]) -> Option<serde_json::Value> {
    use ckbadger_indexer::parser::utxoswap::parse_intent_args;

    let parsed = parse_intent_args(args)?;

    let mut result = serde_json::json!({
        "protocol": "utxoswap",
        "intentType": parsed.intent_type.display_name(),
        "poolTypeHash": format!("0x{}", hex::encode(parsed.pool_type_hash)),
        "amountIn": parsed.amount_in.to_string(),
        "amountOutMin": parsed.amount_out_min.to_string(),
        "assetInIndex": parsed.asset_in_index,
    });

    if let Some(extra) = &parsed.create_pool_extra {
        result["assetX"] = serde_json::json!(format!("0x{}", hex::encode(extra.asset_x)));
        result["assetY"] = serde_json::json!(format!("0x{}", hex::encode(extra.asset_y)));
        result["amountX"] = serde_json::json!(extra.amount_x.to_string());
        result["amountY"] = serde_json::json!(extra.amount_y.to_string());
        result["totalFeeRate"] = serde_json::json!(extra.total_fee_rate);
    }

    Some(result)
}
```

**Step 2: Register in LOCK_ARGS_DECODERS**

Inside the `LOCK_ARGS_DECODERS` LazyLock closure (after the Fiber commitment lock block, before `m`), add:

```rust
    // UTXOSwap intent lock
    for hex in [
        "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e", // mainnet
        "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101", // testnet
    ] {
        m.insert(
            parse_hex_code_hash(hex),
            decode_utxoswap_intent_args as ArgsDecoder,
        );
    }
```

**Step 3: Write tests**

Append in the `#[cfg(test)]` module:

```rust
    #[test]
    fn test_decode_utxoswap_intent_swap() {
        let mut args = vec![0u8; 90];
        for i in 0..20 { args[i] = 0xAA; }
        for i in 20..40 { args[i] = 0xBB; }
        args[56] = 3; // SwapExactInputForOutput
        args[57] = 1; // asset_in_index
        args[58..74].copy_from_slice(&1_000_000u128.to_le_bytes());
        args[74..90].copy_from_slice(&500_000u128.to_le_bytes());

        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "SwapExactInputForOutput");
        assert_eq!(result["assetInIndex"], 1);
        assert_eq!(result["amountIn"], "1000000");
        assert_eq!(result["amountOutMin"], "500000");
        assert!(result.get("assetX").is_none()); // no create_pool fields
    }

    #[test]
    fn test_decode_utxoswap_intent_create_pool() {
        let mut args = vec![0u8; 154];
        for i in 0..20 { args[i] = 0xAA; }
        for i in 20..40 { args[i] = 0xBB; }
        args[56] = 0; // CreatePool
        args[57] = 30; // total_fee_rate
        for i in 58..90 { args[i] = 0xCC; }
        for i in 90..122 { args[i] = 0xDD; }
        args[122..138].copy_from_slice(&5_000u128.to_le_bytes());
        args[138..154].copy_from_slice(&10_000u128.to_le_bytes());

        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "CreatePool");
        assert_eq!(result["totalFeeRate"], 30);
        assert_eq!(result["amountX"], "5000");
        assert_eq!(result["amountY"], "10000");
        assert!(result["assetX"].as_str().unwrap().starts_with("0x"));
        assert!(result["assetY"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn test_decode_utxoswap_intent_too_short() {
        let args = vec![0u8; 89];
        assert!(decode_utxoswap_intent_args(&args).is_none());
    }

    #[test]
    fn test_utxoswap_intent_locks_have_decoders() {
        let mainnet = parse_hex_code_hash(
            "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e",
        );
        let testnet = parse_hex_code_hash(
            "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101",
        );
        assert!(LOCK_ARGS_DECODERS.contains_key(&mainnet));
        assert!(LOCK_ARGS_DECODERS.contains_key(&testnet));
    }
```

**Step 4: Run tests**

Run: `cargo test -p ckbadger-api utxoswap -- --nocapture`

Expected: All 4 new tests pass.

**Step 5: Commit**

```bash
git add crates/api/src/routes/activities.rs
git commit -m "feat(api): add UTXOSwap intent lock args decoder"
```

---

### Task 6: Script label

**Files:**

- Create: `docs/token-labels/information/script/utxoswap-intent-lock/index.json`

**Step 1: Create the label file**

```json
{
  "$schema": "../schema.json",
  "name": "UTXOSwap Intent Lock",
  "description": "UTXOSwap DEX intent cell lock for swap/liquidity orders",
  "rfc": "",
  "website": "https://utxoswap.xyz/",
  "sourceUrl": "https://github.com/UTXOSwap/utxoswap-sdk-js",
  "deployments": {
    "mainnet": [
      {
        "tag": "",
        "hashType": "type",
        "dataHash": "",
        "typeHash": "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e",
        "codeHash": "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e",
        "deprecated": false,
        "cellDeps": [
          {
            "txHash": "0x5292c77c62f108e3a33e54ed3bdcc4457a9d7d88be0c6ef3c1811f473394e2f7",
            "index": 0,
            "depType": "code"
          }
        ]
      }
    ],
    "testnet": [
      {
        "tag": "",
        "hashType": "type",
        "dataHash": "",
        "typeHash": "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101",
        "codeHash": "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101",
        "deprecated": false,
        "cellDeps": [
          {
            "txHash": "0xad80b3b20c1ce63f2b0863c1ef697a98184dfee1158e623de6a132d271f01e76",
            "index": 0,
            "depType": "code"
          }
        ]
      }
    ]
  }
}
```

**Step 2: Verify build picks it up**

Run: `cargo check -p ckbadger-indexer`

Expected: Compiles (build.rs may reference script labels but intent lock has no `decoderType`, so it won't be included in the udt bundled list — correct behavior).

**Step 3: Commit**

```bash
git add docs/token-labels/information/script/utxoswap-intent-lock/
git commit -m "feat(labels): add UTXOSwap intent lock script label"
```

---

### Task 7: Full test suite verification

**Step 1: Run all Rust tests**

Run: `cargo test`

Expected: All tests pass, no regressions.

**Step 2: Run clippy**

Run: `cargo clippy`

Expected: No new warnings.

**Step 3: Run frontend checks**

Run: `cd frontend && pnpm type-check && pnpm lint`

Expected: Pass (no frontend changes made).

**Step 4: Final commit (if any fixups needed)**

If clippy or tests revealed issues, fix and commit.
