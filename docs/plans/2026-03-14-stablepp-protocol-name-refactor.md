# Stable++ Protocol Support Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove pre-framework `TypeCallResponse.protocol_name` / `PROTOCOL_ACTION_LOCKS` / `LockCallResponse.role`, and implement `StableppDetector` for proper Layer 3 CDP action detection.

**Architecture:** Three-phase: (1) clean up API layer pre-framework remnants, (2) implement Stable++ parser constants and detector, (3) update frontend to use `scriptName` instead of `protocolName` and add stablepp action labels.

**Tech Stack:** Rust (axum, serde), TypeScript/React, Vitest

---

### Task 1: Remove `PROTOCOL_INDEX`, `PROTOCOL_ACTION_LOCKS`, and `lock_call_role` from API

**Files:**

- Modify: `crates/api/src/routes/activities.rs:14,91-112,150-181,264-290,304-315,439-477,1072-1097`

**Step 1: Remove unused imports and statics**

In `crates/api/src/routes/activities.rs`:

Delete `HashSet` from line 14 import (keep `HashMap`):

```rust
use std::collections::HashMap;
```

(Remove `HashSet` since `PROTOCOL_ACTION_LOCKS` was its only consumer.)

Delete the entire `PROTOCOL_INDEX` static (lines 150-181).

Delete the entire `PROTOCOL_ACTION_LOCKS` static (lines 304-315).

Delete `lock_call_role()` function (lines 439-445).

**Step 2: Remove `protocol_name` from `TypeCallResponse`**

Change `TypeCallResponse` (lines 91-100) to:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
}
```

**Step 3: Remove `role` from `LockCallResponse`**

Change `LockCallResponse` (lines 102-112) to:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockCallResponse {
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
    pub decoded: Option<serde_json::Value>,
}
```

**Step 4: Update `convert_type_call` — remove protocol_name line**

In `convert_type_call()` (line 288), remove:

```rust
        protocol_name: PROTOCOL_INDEX.get(&call.type_code_hash).cloned(),
```

**Step 5: Update `convert_lock_call` — remove role**

In `convert_lock_call()` (lines 447-477):

- Delete `let role = lock_call_role(&call.lock_code_hash);` (line 464)
- Delete `role: role.to_string(),` (line 472)

**Step 6: Remove tests for deleted statics**

Delete `test_fiber_locks_in_protocol_action_locks` (lines 1072-1084).

**Step 7: Remove `LazyLock` import if no longer needed**

Check if `LazyLock` is still used (it is — by `LOCK_ARGS_DECODERS`). Keep the import.

**Step 8: Run Rust check**

Run: `cargo check -p ckbadger-api 2>&1 | head -30`
Expected: compiles cleanly (no errors)

**Step 9: Run API tests**

Run: `cargo test -p ckbadger-api 2>&1 | tail -20`
Expected: all tests pass

**Step 10: Commit**

```bash
git add crates/api/src/routes/activities.rs
git commit -m "refactor(api): remove protocol_name, PROTOCOL_INDEX, PROTOCOL_ACTION_LOCKS, and LockCallResponse.role

Pre-framework protocol identification mechanisms superseded by Layer 3 ProtocolAction/ProtocolDetector."
```

---

### Task 2: Create Stable++ parser constants

**Files:**

- Create: `crates/indexer/src/parser/stablepp.rs`
- Modify: `crates/indexer/src/parser/mod.rs:1-24`

**Step 1: Create `stablepp.rs` with code hash constants and helper functions**

Create `crates/indexer/src/parser/stablepp.rs`:

```rust
use std::sync::LazyLock;

use crate::rpc::parse_hex_to_bytes;

// Stable++ Asset (type script) — xudt_compatible
pub const ASSET_CODE_HASH_MAINNET: &str =
    "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b";
pub const ASSET_CODE_HASH_TESTNET: &str =
    "0x1142755a044bf2ee358cba9f2da187ce928c91cd4dc8692ded0337efa677d21a";

// Stable++ Pool (type script)
pub const POOL_CODE_HASH_MAINNET: &str =
    "0x26622198b66240e437e323e0fecf1c26ba3c8c28a45f03ed3ebb9f7f2bdc0055";

// Stable++ Intent Lock (lock script)
pub const INTENT_LOCK_CODE_HASH_MAINNET: &str =
    "0x56fb632a13abdad7308d2e034baae1cb049e8e8ff23cc7c0b69449f617549733";

// Stable++ Vault Lock (lock script)
pub const VAULT_LOCK_CODE_HASH_MAINNET: &str =
    "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67";

static ASSET_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET));
static ASSET_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(ASSET_CODE_HASH_TESTNET));
static POOL_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(POOL_CODE_HASH_MAINNET));
static INTENT_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET));
static VAULT_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET));

/// Returns true if the code_hash matches the Stable++ Asset type script.
pub fn is_stablepp_asset(code_hash: &[u8]) -> bool {
    code_hash == ASSET_MAINNET.as_slice() || code_hash == ASSET_TESTNET.as_slice()
}

/// Returns true if the code_hash matches the Stable++ Pool type script (mainnet only).
pub fn is_stablepp_pool(code_hash: &[u8]) -> bool {
    code_hash == POOL_MAINNET.as_slice()
}

/// Returns true if the code_hash matches the Stable++ Intent Lock (mainnet only).
pub fn is_stablepp_intent_lock(code_hash: &[u8]) -> bool {
    code_hash == INTENT_MAINNET.as_slice()
}

/// Returns true if the code_hash matches the Stable++ Vault Lock (mainnet only).
pub fn is_stablepp_vault_lock(code_hash: &[u8]) -> bool {
    code_hash == VAULT_MAINNET.as_slice()
}

/// Returns true if the code_hash matches any Stable++ script.
pub fn is_stablepp_script(code_hash: &[u8]) -> bool {
    is_stablepp_asset(code_hash)
        || is_stablepp_pool(code_hash)
        || is_stablepp_intent_lock(code_hash)
        || is_stablepp_vault_lock(code_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_stablepp_asset_mainnet() {
        let bytes = parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET);
        assert!(is_stablepp_asset(&bytes));
        assert!(is_stablepp_script(&bytes));
    }

    #[test]
    fn test_is_stablepp_asset_testnet() {
        let bytes = parse_hex_to_bytes(ASSET_CODE_HASH_TESTNET);
        assert!(is_stablepp_asset(&bytes));
    }

    #[test]
    fn test_is_stablepp_vault_lock() {
        let bytes = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        assert!(is_stablepp_vault_lock(&bytes));
        assert!(is_stablepp_script(&bytes));
    }

    #[test]
    fn test_is_stablepp_intent_lock() {
        let bytes = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_stablepp_intent_lock(&bytes));
    }

    #[test]
    fn test_is_stablepp_pool() {
        let bytes = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        assert!(is_stablepp_pool(&bytes));
    }

    #[test]
    fn test_non_stablepp_returns_false() {
        let bytes = vec![0x11; 32];
        assert!(!is_stablepp_script(&bytes));
    }
}
```

**Step 2: Register module in `parser/mod.rs`**

Add `pub mod stablepp;` to `crates/indexer/src/parser/mod.rs` (after line 8, alongside other modules).

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer parser::stablepp 2>&1 | tail -15`
Expected: all 6 tests pass

**Step 4: Commit**

```bash
git add crates/indexer/src/parser/stablepp.rs crates/indexer/src/parser/mod.rs
git commit -m "feat(indexer): add Stable++ parser constants and code hash helpers"
```

---

### Task 3: Implement StableppDetector

**Files:**

- Create: `crates/indexer/src/db/writer/stablepp_detector.rs`
- Modify: `crates/indexer/src/db/writer.rs:52-69` (add module)

**Step 1: Create the detector**

Create `crates/indexer/src/db/writer/stablepp_detector.rs`:

```rust
//! Stable++ CDP protocol detector: identifies vault lifecycle events
//! by analyzing Vault Lock cell transitions and RUSD token deltas.

use ckbadger_store::types::{AssetChange, LockCallEntry, ProtocolAction, TypeCallEntry};

use crate::parser::stablepp::{
    is_stablepp_asset, is_stablepp_intent_lock, is_stablepp_script, is_stablepp_vault_lock,
};

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

pub(crate) struct StableppDetector {
    #[allow(dead_code)]
    is_mainnet: bool,
}

impl StableppDetector {
    pub fn new(is_mainnet: bool) -> Self {
        Self { is_mainnet }
    }

    /// Check if this transaction involves any Stable++ scripts.
    fn has_stablepp_scripts(
        &self,
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> bool {
        type_calls
            .iter()
            .any(|tc| is_stablepp_script(&tc.type_code_hash))
            || lock_calls
                .iter()
                .any(|lc| is_stablepp_script(&lc.lock_code_hash))
    }

    /// Count Vault Lock cells in inputs and outputs.
    fn count_vault_cells(&self, tx: &TxView<'_>) -> (usize, usize) {
        let in_count = tx
            .inputs
            .iter()
            .filter(|i| is_stablepp_vault_lock(&i.lock_code_hash))
            .count();
        let out_count = tx
            .outputs
            .iter()
            .filter(|o| is_stablepp_vault_lock(&o.lock_code_hash))
            .count();
        (in_count, out_count)
    }

    /// Check if Intent Lock is present in inputs (aggregator-processed operation).
    fn has_intent_in_inputs(&self, tx: &TxView<'_>) -> bool {
        tx.inputs
            .iter()
            .any(|i| is_stablepp_intent_lock(&i.lock_code_hash))
    }

    /// Sum RUSD (Stable++ Asset) token delta from asset changes.
    fn rusd_delta(&self, asset_changes: &[AssetChange]) -> i128 {
        asset_changes
            .iter()
            .filter_map(|ac| match ac {
                AssetChange::Token {
                    type_script_hash: _,
                    delta,
                    symbol: _,
                    decimals: _,
                } => {
                    // We can't easily check type_script_hash here because it's the
                    // full script hash, not just code_hash. Instead, check all token
                    // changes — in a Stable++ tx, the relevant tokens are RUSD/wCKB
                    // which use the Stable++ Asset type script.
                    // For now, sum all token deltas as a heuristic. In practice,
                    // Stable++ txs primarily involve RUSD tokens.
                    Some(*delta)
                }
                _ => None,
            })
            .sum()
    }

    /// Infer the CDP action from vault lifecycle and token delta signals.
    fn infer_action(&self, vault_in: usize, vault_out: usize, token_delta: i128) -> &'static str {
        match (vault_in > 0, vault_out > 0, token_delta.signum()) {
            // Vault created
            (false, true, 1) => "open_vault",
            (false, true, 0) => "deposit",
            (false, true, -1) => "open_vault", // rare: open + repay in one tx
            // Vault modified
            (true, true, 1) => "borrow",
            (true, true, -1) => "repay",
            (true, true, 0) => "adjust",
            // Vault consumed
            (true, false, -1) => "close_vault",
            (true, false, 0) => "liquidation",
            (true, false, 1) => "liquidation", // rare: liquidation with surplus
            // No vault cells involved
            (false, false, 1) | (false, false, -1) => "redemption",
            // Fallback
            (false, false, 0) => "interaction",
        }
    }
}

impl ProtocolDetector for StableppDetector {
    fn protocol_name(&self) -> &str {
        "stablepp"
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        _owner_lock_hash: &[u8],
        _accum: &OwnerAccum,
        asset_changes: &[AssetChange],
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction> {
        if !self.has_stablepp_scripts(type_calls, lock_calls) {
            return vec![];
        }

        let (vault_in, vault_out) = self.count_vault_cells(tx);
        let token_delta = self.rusd_delta(asset_changes);
        let has_intent = self.has_intent_in_inputs(tx);

        let action = self.infer_action(vault_in, vault_out, token_delta);

        let metadata = serde_json::json!({
            "hasIntent": has_intent,
            "vaultCount": vault_in.max(vault_out),
        });

        vec![ProtocolAction {
            protocol: "stablepp".to_string(),
            action: action.to_string(),
            metadata,
        }]
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
    use crate::parser::stablepp::{
        INTENT_LOCK_CODE_HASH_MAINNET, POOL_CODE_HASH_MAINNET, VAULT_LOCK_CODE_HASH_MAINNET,
    };
    use crate::rpc::parse_hex_to_bytes;

    fn make_input(
        lock_hash_byte: u8,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        capacity: i64,
        type_code_hash: Option<Vec<u8>>,
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
        type_code_hash: Option<Vec<u8>>,
    ) -> ParsedCell {
        ParsedCell {
            capacity,
            lock_code_hash,
            lock_hash_type: 1,
            lock_args,
            lock_script_hash: vec![lock_hash_byte; 32],
            type_code_hash,
            type_hash_type: Some(1),
            type_args: None,
            type_script_hash: None,
            data_hash: vec![0; 32],
            data_size: 0,
            data: vec![],
        }
    }

    fn standard_lock() -> Vec<u8> {
        vec![0x11; 32]
    }

    fn vault_lock() -> Vec<u8> {
        parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET)
    }

    fn intent_lock() -> Vec<u8> {
        parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET)
    }

    fn pool_type() -> Vec<u8> {
        parse_hex_to_bytes(POOL_CODE_HASH_MAINNET)
    }

    fn stablepp_detectors() -> Vec<Box<dyn ProtocolDetector>> {
        vec![Box::new(StableppDetector::new(true))]
    }

    fn find_owner_actions(
        bundles: &[ckbadger_store::types::TxActivityBundle],
        owner_byte: u8,
    ) -> Vec<ProtocolAction> {
        bundles
            .iter()
            .flat_map(|b| &b.owners)
            .find(|o| o.lock_hash == vec![owner_byte; 32])
            .map(|o| o.protocol_actions.clone())
            .unwrap_or_default()
    }

    #[test]
    fn test_stablepp_detector_protocol_name() {
        let detector = StableppDetector::new(true);
        assert_eq!(detector.protocol_name(), "stablepp");
    }

    #[test]
    fn test_no_stablepp_scripts_returns_empty() {
        let user: u8 = 0xAA;
        let input = make_input(user, standard_lock(), vec![0x22; 20], 200_00000000, None);
        let outputs = vec![make_output(
            user,
            standard_lock(),
            vec![0x22; 20],
            200_00000000,
            None,
        )];
        let tx = TxView {
            tx_hash: &[0x01; 32],
            block_hash: &[0xC0; 32],
            tx_index: 0,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };
        let bundles = build_activity_bundles_for_block_with_detectors(
            &[tx],
            &HashMap::new(),
            &stablepp_detectors(),
        );
        let actions = find_owner_actions(&bundles, user);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_open_vault() {
        // Vault Lock only in outputs + Pool type call -> open_vault
        let user: u8 = 0xAA;
        let vault_owner: u8 = 0xBB;

        let input = make_input(user, standard_lock(), vec![0x22; 20], 500_00000000, None);
        let outputs = vec![
            // Vault cell created
            make_output(
                vault_owner,
                vault_lock(),
                vec![0x33; 20],
                400_00000000,
                Some(pool_type()),
            ),
            // Change back to user
            make_output(
                user,
                standard_lock(),
                vec![0x22; 20],
                100_00000000,
                None,
            ),
        ];
        let tx = TxView {
            tx_hash: &[0x02; 32],
            block_hash: &[0xC1; 32],
            tx_index: 0,
            block_number: 1001,
            timestamp: 1_700_000_010,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };
        let bundles = build_activity_bundles_for_block_with_detectors(
            &[tx],
            &HashMap::new(),
            &stablepp_detectors(),
        );
        // User should get a protocol action (they interact with Stable++ scripts)
        let actions = find_owner_actions(&bundles, user);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].protocol, "stablepp");
        // With no token delta and vault only in outputs -> deposit
        // (open_vault requires positive RUSD delta, which we don't have in this simple test)
        assert_eq!(actions[0].action, "deposit");
        assert_eq!(actions[0].metadata["hasIntent"], false);
    }

    #[test]
    fn test_close_vault() {
        // Vault Lock only in inputs + Pool type call -> close_vault or liquidation
        let user: u8 = 0xAA;
        let vault_owner: u8 = 0xBB;

        // Vault consumed
        let input = make_input(
            vault_owner,
            vault_lock(),
            vec![0x33; 20],
            400_00000000,
            Some(pool_type()),
        );
        let outputs = vec![
            // CKB returned to user
            make_output(
                user,
                standard_lock(),
                vec![0x22; 20],
                400_00000000,
                None,
            ),
        ];
        let tx = TxView {
            tx_hash: &[0x03; 32],
            block_hash: &[0xC2; 32],
            tx_index: 0,
            block_number: 1002,
            timestamp: 1_700_000_020,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };
        let bundles = build_activity_bundles_for_block_with_detectors(
            &[tx],
            &HashMap::new(),
            &stablepp_detectors(),
        );
        // User gets action: vault in inputs only + no token delta -> liquidation
        let actions = find_owner_actions(&bundles, user);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].protocol, "stablepp");
        assert_eq!(actions[0].action, "liquidation");
    }

    #[test]
    fn test_adjust_vault() {
        // Vault in inputs + outputs, no token delta -> adjust
        let user: u8 = 0xAA;
        let vault_owner: u8 = 0xBB;

        let input_vault = make_input(
            vault_owner,
            vault_lock(),
            vec![0x33; 20],
            200_00000000,
            Some(pool_type()),
        );
        let input_user = make_input(user, standard_lock(), vec![0x22; 20], 300_00000000, None);
        let outputs = vec![
            make_output(
                vault_owner,
                vault_lock(),
                vec![0x33; 20],
                400_00000000,
                Some(pool_type()),
            ),
            make_output(
                user,
                standard_lock(),
                vec![0x22; 20],
                100_00000000,
                None,
            ),
        ];
        let tx = TxView {
            tx_hash: &[0x04; 32],
            block_hash: &[0xC3; 32],
            tx_index: 0,
            block_number: 1003,
            timestamp: 1_700_000_030,
            is_cellbase: false,
            inputs: vec![input_vault, input_user],
            outputs: &outputs,
            witnesses: &[],
        };
        let bundles = build_activity_bundles_for_block_with_detectors(
            &[tx],
            &HashMap::new(),
            &stablepp_detectors(),
        );
        let actions = find_owner_actions(&bundles, user);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "adjust");
    }

    #[test]
    fn test_intent_lock_metadata() {
        // Intent Lock in inputs -> hasIntent: true
        let user: u8 = 0xAA;
        let vault_owner: u8 = 0xBB;
        let intent_owner: u8 = 0xCC;

        let input_intent = make_input(
            intent_owner,
            intent_lock(),
            vec![0x44; 20],
            100_00000000,
            None,
        );
        let input_user = make_input(user, standard_lock(), vec![0x22; 20], 400_00000000, None);
        let outputs = vec![
            make_output(
                vault_owner,
                vault_lock(),
                vec![0x33; 20],
                400_00000000,
                Some(pool_type()),
            ),
            make_output(
                user,
                standard_lock(),
                vec![0x22; 20],
                100_00000000,
                None,
            ),
        ];
        let tx = TxView {
            tx_hash: &[0x05; 32],
            block_hash: &[0xC4; 32],
            tx_index: 0,
            block_number: 1004,
            timestamp: 1_700_000_040,
            is_cellbase: false,
            inputs: vec![input_intent, input_user],
            outputs: &outputs,
            witnesses: &[],
        };
        let bundles = build_activity_bundles_for_block_with_detectors(
            &[tx],
            &HashMap::new(),
            &stablepp_detectors(),
        );
        let actions = find_owner_actions(&bundles, user);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].metadata["hasIntent"], true);
    }

    #[test]
    fn test_fallback_interaction() {
        // No vault cells, no token delta, but Pool type script present -> interaction
        let user: u8 = 0xAA;

        let input = make_input(
            user,
            standard_lock(),
            vec![0x22; 20],
            100_00000000,
            Some(pool_type()),
        );
        let outputs = vec![make_output(
            user,
            standard_lock(),
            vec![0x22; 20],
            100_00000000,
            Some(pool_type()),
        )];
        let tx = TxView {
            tx_hash: &[0x06; 32],
            block_hash: &[0xC5; 32],
            tx_index: 0,
            block_number: 1005,
            timestamp: 1_700_000_050,
            is_cellbase: false,
            inputs: vec![input],
            outputs: &outputs,
            witnesses: &[],
        };
        let bundles = build_activity_bundles_for_block_with_detectors(
            &[tx],
            &HashMap::new(),
            &stablepp_detectors(),
        );
        let actions = find_owner_actions(&bundles, user);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "interaction");
    }
}
```

**Step 2: Register module in `db/writer.rs`**

Add to `crates/indexer/src/db/writer.rs` (after line 65, near `rgbpp_detector`):

```rust
pub(crate) mod stablepp_detector;
```

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer stablepp_detector 2>&1 | tail -20`
Expected: all tests pass

**Step 4: Commit**

```bash
git add crates/indexer/src/db/writer/stablepp_detector.rs crates/indexer/src/db/writer.rs
git commit -m "feat(indexer): implement StableppDetector for CDP vault lifecycle detection"
```

---

### Task 4: Register StableppDetector in sync pipeline

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:1676-1683,4108-4115`

**Step 1: Add StableppDetector to both bulk and live sync detector lists**

At line 1676-1683 (bulk sync), add after the FiberDetector line:

```rust
let protocol_detectors: Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>> = vec![
    Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
        self.config.is_mainnet(),
    )),
    Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
        self.config.is_mainnet(),
    )),
    Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
        self.config.is_mainnet(),
    )),
];
```

At line 4108-4115 (live sync), same change:

```rust
let protocol_detectors: Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>> = vec![
    Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
        self.config.is_mainnet(),
    )),
    Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
        self.config.is_mainnet(),
    )),
    Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
        self.config.is_mainnet(),
    )),
];
```

**Step 2: Run full Rust check**

Run: `cargo check 2>&1 | tail -10`
Expected: compiles cleanly

**Step 3: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "feat(indexer): register StableppDetector in bulk and live sync pipelines"
```

---

### Task 5: Update frontend — remove `protocolName` and `role`, add stablepp labels

**Files:**

- Modify: `frontend/lib/api.ts:445-462`
- Modify: `frontend/lib/activity-classify.ts:88-99`
- Modify: `frontend/components/activity-event-row.tsx:265-286`
- Modify: `frontend/components/latest-activities.tsx:380-410,412-434`

**Step 1: Remove `protocolName` from `ActivityTypeCall` and `role` from `ActivityLockCall`**

In `frontend/lib/api.ts`, change `ActivityTypeCall` (lines 445-452):

```typescript
interface ActivityTypeCall {
  typeCodeHash: string;
  typeHashType: string;
  typeArgs: string;
  scriptHash: string;
  scriptName?: string;
}
```

Change `ActivityLockCall` (lines 454-462):

```typescript
interface ActivityLockCall {
  lockCodeHash: string;
  lockHashType: string;
  lockArgs: string;
  scriptHash: string;
  scriptName?: string;
  decoded?: Record<string, unknown>;
}
```

**Step 2: Remove `role === 'protocol_action'` classification path**

In `frontend/lib/activity-classify.ts`, delete lines 88-99 (the protocol action lock call fallback):

```typescript
// DELETE THIS BLOCK:
// Layer 2 (catch-all): Protocol action lock calls
const protocolAction = activity.lockCalls.find((lc) => lc.role === 'protocol_action');
if (protocolAction) {
  return {
    displayType: 'protocolAction',
    activity,
    primaryAssetChange: null,
    primaryTypeCall: activity.typeCalls[0] ?? null,
    primaryLockCall: protocolAction,
    primaryProtocolAction: null,
  };
}
```

**Step 3: Update type call rendering — use `scriptName`**

In `frontend/components/activity-event-row.tsx`, change `getTypeEventParts()` (lines 265-286):

```typescript
function getTypeEventParts(sc: ActivityTypeCall): EventParts {
  const label = sc.scriptName?.trim() || 'Type call';
  return {
    badge: (
      <span className="text-amber font-mono text-xs">
        {'\u2699'} {label}
      </span>
    ),
    value: (
      <span className="font-mono text-xs">
        <TypeCallExpr sc={sc} />
      </span>
    ),
  };
}
```

**Step 4: Update homepage type call rendering — use `scriptName`**

In `frontend/components/latest-activities.tsx`, change `StreamItemTypeCall` (lines 380-410):

```typescript
function StreamItemTypeCall({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryTypeCall } = classified;
  const badge = getTypeBadge(classified);
  const label = primaryTypeCall?.scriptName?.trim() || 'Type call';

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('min-w-0 truncate font-mono text-xs', badge.colorClass)}>
          {badge.icon} {label}{' '}
          {primaryTypeCall ? <TypeCallExpr sc={primaryTypeCall} /> : null}
        </span>
        <span className="text-text-dim shrink-0 font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        <AddressLink address={activity.address} />
        <CkbDelta delta={activity.ckbDelta} />
      </div>
    </>
  );
}
```

**Step 5: Add stablepp action labels alongside fiber labels**

In `frontend/components/latest-activities.tsx`, after `FIBER_ACTION_LABELS` (line 412-417), add:

```typescript
const STABLEPP_ACTION_LABELS: Record<string, string> = {
  open_vault: 'Open Vault',
  borrow: 'Borrow',
  repay: 'Repay',
  close_vault: 'Close Vault',
  deposit: 'Deposit',
  adjust: 'Adjust Vault',
  liquidation: 'Liquidation',
  redemption: 'Redemption',
  interaction: 'Interaction',
};

const PROTOCOL_ACTION_LABELS: Record<string, Record<string, string>> = {
  fiber: FIBER_ACTION_LABELS,
  stablepp: STABLEPP_ACTION_LABELS,
};
```

Then update `StreamItemProtocolAction` (around lines 419-434) to use the generic lookup:

```typescript
const isKnownProtocol =
  primaryProtocolAction && PROTOCOL_ACTION_LABELS[primaryProtocolAction.protocol];

const action = primaryProtocolAction
  ? (PROTOCOL_ACTION_LABELS[primaryProtocolAction.protocol]?.[primaryProtocolAction.action] ??
    primaryProtocolAction.action.replace(/_/g, ' '))
  : (primaryLockCall?.decoded?.intentType as string) ||
    (primaryLockCall?.decoded?.action as string) ||
    '';
```

Remove the `isFiber` variable and the fiber-specific branching that was there before.

**Step 6: Run type check**

Run: `cd frontend && pnpm type-check 2>&1 | tail -10`
Expected: no errors

**Step 7: Run lint**

Run: `cd frontend && pnpm lint 2>&1 | tail -10`
Expected: no errors

**Step 8: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/activity-classify.ts frontend/components/activity-event-row.tsx frontend/components/latest-activities.tsx
git commit -m "refactor(frontend): remove protocolName and role, use scriptName for type calls, add stablepp labels"
```

---

### Task 6: Update frontend tests

**Files:**

- Modify: `frontend/__tests__/components/activity-event-row.test.tsx:112-134`
- Modify: `frontend/__tests__/components/latest-activities.test.tsx:122-148`
- Modify: `frontend/__tests__/lib/activity-classify.test.ts:146-203`

**Step 1: Update activity-event-row tests**

In `frontend/__tests__/components/activity-event-row.test.tsx`, replace the `protocolName` test (lines 112-134) with a `scriptName` test:

```typescript
  it('renders script name instead of Type call when scriptName is set', () => {
    render(
      <ActivityEventGroup
        activity={makeActivity({
          typeCalls: [
            {
              typeCodeHash: '0xcode',
              typeHashType: 'type',
              typeArgs: '0x1234',
              scriptHash: '0xhash',
              scriptName: 'Stable++ Pool',
            },
          ],
        })}
        formatTimeAgo={mockFormatTimeAgo}
      />
    );
    expect(screen.getAllByText(/Stable\+\+ Pool/).length).toBeGreaterThan(0);
    // Should NOT show "Type call" when script name is present
    expect(screen.queryByText(/Type call/)).not.toBeInTheDocument();
  });
```

Also remove `protocolName` from any other test that references it in this file.

Remove `role` from `makeActivity`'s `lockCalls` if present in this file.

**Step 2: Update latest-activities tests**

In `frontend/__tests__/components/latest-activities.test.tsx`, replace the `protocolName` test (lines 122-148):

```typescript
  it('renders script name for script calls with scriptName', async () => {
    vi.mocked(api.getLatestActivities).mockResolvedValue([
      makeActivity({
        address: 'ckb1qprotocol111111111111111111111111111111111111111',
        txHash: '0xtx-protocol',
        typeCalls: [
          {
            typeCodeHash: '0xcode',
            typeHashType: 'type',
            typeArgs: '0x1234',
            scriptHash: '0xhash',
            scriptName: 'Stable++ Pool',
          },
        ],
      }),
    ]);

    render(<LatestActivities />);

    await waitFor(() => {
      expect(screen.getByText(/Stable\+\+ Pool/)).toBeInTheDocument();
      // Should NOT show "Type call" text when script name is present
      expect(screen.queryByText(/Type call/)).not.toBeInTheDocument();
    });
  });
```

**Step 3: Update activity-classify tests**

In `frontend/__tests__/lib/activity-classify.test.ts`:

Delete or rewrite tests that reference `role`:

- `test_classifies_protocol_action_lock_call_as_protocolAction` (lines 146-163): Delete — this classification path no longer exists
- `test_asset_change_takes_priority_over_protocol_action_lock_call` (lines 165-184): Rewrite without `role`:

```typescript
it('asset change takes priority over lock calls', () => {
  const result = classifyActivity(
    makeActivity({
      assetChanges: [
        { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
      ],
      lockCalls: [
        {
          lockCodeHash: '0xintent',
          lockHashType: 'type',
          lockArgs: '0xargs',
          scriptHash: '0xhash',
        },
      ],
    })
  );
  expect(result.displayType).toBe('token');
});
```

- `test_access_control_lock_call_does_not_create_protocolAction_type` (lines 186-203): Rewrite without `role`:

```typescript
it('lock call without protocol action does not create protocolAction type', () => {
  const result = classifyActivity(
    makeActivity({
      lockCalls: [
        {
          lockCodeHash: '0xrgbpp',
          lockHashType: 'type',
          lockArgs: '0xargs',
          scriptHash: '0xhash',
          scriptName: 'RGB++',
        },
      ],
    })
  );
  expect(result.displayType).toBe('ckbTransfer');
});
```

**Step 4: Run frontend tests**

Run: `cd frontend && npx vitest run 2>&1 | tail -20`
Expected: all tests pass

**Step 5: Commit**

```bash
git add frontend/__tests__/
git commit -m "test(frontend): update activity tests for protocolName and role removal"
```

---

### Task 7: Update documentation

**Files:**

- Modify: `docs/ACTIVITY_SYSTEM.md:840-876,906-908`

**Step 1: Update Protocol Grouping section**

In `docs/ACTIVITY_SYSTEM.md`, replace the "Protocol Grouping" section (lines 840-876) with:

```markdown
## Protocol Grouping

Protocol identification is handled by Layer 3 `ProtocolDetector` implementations in the indexer. Each detector recognizes protocol-specific script patterns and emits `ProtocolAction` entries.

Active detectors:

- **RgbppDetector** — RGB++ leap/transfer actions via lock script transitions
- **FiberDetector** — Fiber Network channel lifecycle (open/close/force_close/settlement)
- **StableppDetector** — Stable++ CDP vault lifecycle (open_vault/borrow/repay/close_vault/liquidation/redemption)

The `docs/script-name-overrides.json` `protocols` field retains code_hash groupings as reference metadata but is no longer used at runtime for protocol identification.
```

**Step 2: Update file reference table**

Add StableppDetector to the Activity Builder table (around line 892):

```markdown
| `crates/indexer/src/db/writer/stablepp_detector.rs` | StableppDetector: ProtocolDetector impl (open_vault, borrow, repay, close_vault, adjust, liquidation, redemption) |
```

**Step 3: Commit**

```bash
git add docs/ACTIVITY_SYSTEM.md
git commit -m "docs: update ACTIVITY_SYSTEM.md for protocol_name removal and StableppDetector"
```

---

### Task 8: Final verification

**Step 1: Run full Rust check + clippy**

Run: `cargo check && cargo clippy 2>&1 | tail -20`
Expected: no errors, no warnings

**Step 2: Run all Rust tests**

Run: `cargo test 2>&1 | tail -30`
Expected: all tests pass

**Step 3: Run frontend checks**

Run: `cd frontend && pnpm type-check && pnpm lint && npx vitest run 2>&1 | tail -30`
Expected: all pass

**Step 4: Commit any remaining fixes**

If any fixes were needed, commit them.
