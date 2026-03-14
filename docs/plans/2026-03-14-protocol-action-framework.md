# Protocol Action Framework Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a general-purpose protocol action detection framework to the activity system, with RGB++ as the first detector implementation.

**Architecture:** A `ProtocolDetector` trait receives all Layer 2 signals (asset changes, type calls, lock calls) plus the full transaction view, and emits `Vec<ProtocolAction>`. Detectors run after all existing classification, and their output is stored alongside existing activity fields. RGB++ detector compares input/output lock scripts on cells sharing the same type_script identity to determine leap/transfer actions.

**Tech Stack:** Rust (store types, indexer, API), TypeScript/React (frontend types, classification, display)

**Design doc:** `docs/plans/2026-03-14-protocol-action-framework-design.md`

---

### Task 1: Add ProtocolAction type to store

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:1006-1037`

**Step 1: Add the ProtocolAction struct**

After `LockCallEntry` (line 1020), add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolAction {
    /// Protocol identifier: "rgbpp", "utxoswap", "fiber", etc.
    pub protocol: String,
    /// Action name: "leap_to_ckb", "leap_to_btc", "transfer", etc.
    pub action: String,
    /// Protocol-specific decoded metadata.
    pub metadata: serde_json::Value,
}
```

**Step 2: Add protocol_actions field to ActivityEntry**

After `lock_calls` field (line 1003), add:

```rust
#[serde(default)]
pub protocol_actions: Vec<ProtocolAction>,
```

**Step 3: Add protocol_actions field to OwnerActivityDelta**

After `lock_calls` field (line 1035), add:

```rust
#[serde(default)]
pub protocol_actions: Vec<ProtocolAction>,
```

**Step 4: Add protocol_action_counts to DailyActivityStats**

After `script_counts` field (line 1140), add:

```rust
/// Per-protocol action counts: "rgbpp:leap_to_ckb" -> count
#[serde(default)]
pub protocol_action_counts: HashMap<String, u32>,
```

**Step 5: Add serde_json dependency to ckbadger-store if not already present**

Check `crates/ckbadger-store/Cargo.toml` for `serde_json`. Add if missing.

**Step 6: Run cargo check**

Run: `cargo check -p ckbadger-store`
Expected: Compilation errors in files that construct `ActivityEntry` / `OwnerActivityDelta` without the new field.

**Step 7: Fix all construction sites**

Add `protocol_actions: vec![]` or `protocol_actions: Vec::new()` to every place that constructs `ActivityEntry` or `OwnerActivityDelta`:

- `crates/indexer/src/db/writer/activities.rs` — `OwnerActivityDelta` construction (~line 503) and `ActivityEntry` construction in `flatten_tx_activity_bundle` (~line 544)
- `crates/ckbadger-store/src/activity_ops.rs` — `resolve_owner_activity_entry` if it constructs `ActivityEntry`
- `crates/api/tests/api_integration.rs` — test helpers that construct bundles
- `crates/indexer/tests/reorg_handling.rs` — test helpers

**Step 8: Run cargo check again**

Run: `cargo check`
Expected: PASS (all crates compile)

**Step 9: Run tests**

Run: `cargo test --lib`
Expected: PASS

**Step 10: Commit**

```
feat(store): add ProtocolAction type and protocol_actions field to activity types
```

---

### Task 2: Add ProtocolDetector trait and integrate into activity builder

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:144-536`

**Step 1: Write test for protocol detector integration**

Add test at the bottom of the `#[cfg(test)]` module in `activities.rs`:

```rust
#[test]
fn test_protocol_detector_emits_actions() {
    use ckbadger_store::types::ProtocolAction;

    struct TestDetector;
    impl ProtocolDetector for TestDetector {
        fn protocol_name(&self) -> &str { "test_proto" }
        fn detect(
            &self,
            _tx: &TxView<'_>,
            _owner_lock_hash: &[u8],
            _accum: &OwnerAccum,
            _asset_changes: &[AssetChange],
            _type_calls: &[TypeCallEntry],
            _lock_calls: &[LockCallEntry],
        ) -> Vec<ProtocolAction> {
            vec![ProtocolAction {
                protocol: "test_proto".into(),
                action: "test_action".into(),
                metadata: serde_json::json!({"key": "value"}),
            }]
        }
    }

    let tx = TxView {
        tx_hash: &[0x01; 32],
        block_hash: &[0x02; 32],
        tx_index: 0,
        block_number: 100,
        timestamp: 1_700_000_000,
        is_cellbase: false,
        inputs: vec![],
        outputs: &[],
        witnesses: &[],
    };

    let token_info_cache = HashMap::new();
    let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(TestDetector)];
    let bundles = build_activity_bundles_for_block_with_detectors(
        &[tx], &token_info_cache, &detectors,
    );
    // No owners = no protocol_actions (detectors run per-owner)
    assert!(bundles[0].owners.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer test_protocol_detector_emits_actions`
Expected: FAIL — `ProtocolDetector` trait and `build_activity_bundles_for_block_with_detectors` don't exist.

**Step 3: Add TxView witnesses field**

Add `pub witnesses: &'a [String]` to `TxView` struct (after `outputs` field, ~line 171).

**Step 4: Define the ProtocolDetector trait**

After the `TxView` struct, add:

```rust
/// Detects protocol-level actions by analyzing cross-layer signals.
pub(crate) trait ProtocolDetector {
    fn protocol_name(&self) -> &str;

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        accum: &OwnerAccum,
        asset_changes: &[AssetChange],
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction>;
}
```

**Step 5: Add build_activity_bundles_for_block_with_detectors**

Refactor `build_activity_bundles_for_block` to delegate to a new function that accepts detectors:

```rust
pub fn build_activity_bundles_for_block(
    txs: &[TxView<'_>],
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<TxActivityBundle> {
    build_activity_bundles_for_block_with_detectors(txs, token_info_cache, &[])
}

pub fn build_activity_bundles_for_block_with_detectors(
    txs: &[TxView<'_>],
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
    detectors: &[Box<dyn ProtocolDetector>],
) -> Vec<TxActivityBundle> {
    let hashes = code_hashes();
    txs.iter()
        .map(|tx| build_tx_activity_bundle(tx, hashes, token_info_cache, detectors))
        .collect()
}
```

**Step 6: Thread detectors into build_tx_activity_bundle**

Add `detectors: &[Box<dyn ProtocolDetector>]` parameter to `build_tx_activity_bundle`. After building `asset_changes`, `type_calls`, and `lock_calls` (after ~line 501), run detectors:

```rust
let type_calls_slice: Vec<TypeCallEntry> = type_calls.clone().unwrap_or_default();
let lock_calls_slice: Vec<LockCallEntry> = lock_calls.clone().unwrap_or_default();

let protocol_actions: Vec<ProtocolAction> = detectors
    .iter()
    .flat_map(|d| d.detect(tx, lock_hash, accum, &asset_changes, &type_calls_slice, &lock_calls_slice))
    .collect();
```

Add `protocol_actions` to the `OwnerActivityDelta` construction.

**Step 7: Update TxView construction sites to include witnesses**

In `crates/indexer/src/sync/batch.rs`, add `witnesses: &td.witnesses,` to both `TxView` construction sites (~lines 2848 and 4114).

**Step 8: Run cargo check**

Run: `cargo check`
Expected: PASS

**Step 9: Run the test**

Run: `cargo test -p ckbadger-indexer test_protocol_detector_emits_actions`
Expected: PASS

**Step 10: Run full test suite**

Run: `cargo test --lib`
Expected: PASS

**Step 11: Commit**

```
feat(indexer): add ProtocolDetector trait and integrate into activity builder
```

---

### Task 3: Implement RgbppDetector

**Files:**

- Create: `crates/indexer/src/db/writer/rgbpp_detector.rs`
- Modify: `crates/indexer/src/db/writer/mod.rs`
- Modify: `crates/indexer/src/db/writer/activities.rs` (tests)

**Step 1: Write tests for RGB++ detection**

Add to `activities.rs` test module (these test the detector via the public API):

```rust
#[test]
fn test_rgbpp_leap_to_ckb() {
    // Input: cell with rgbpp lock + xUDT type script
    // Output: same type script but standard CKB lock
    // Expected: ProtocolAction { protocol: "rgbpp", action: "leap_to_ckb" }

    let rgbpp_lock_code_hash = parse_hex_to_bytes(
        crate::parser::rgbpp::RGBPP_LOCK_CODE_HASH_MAINNET
    );
    let standard_lock = vec![0x9b; 32]; // secp256k1
    let type_code_hash = vec![0xAA; 32];
    let type_args = vec![0xBB; 32];

    // rgbpp lock args: out_index(4) + btc_txid(32)
    let mut rgbpp_args = vec![0u8; 36];
    rgbpp_args[0..4].copy_from_slice(&2u32.to_le_bytes());
    for i in 0..32 { rgbpp_args[4 + i] = (i + 1) as u8; }

    let input = InputCellView {
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: rgbpp_lock_code_hash.clone(),
        lock_hash_type: 1,
        lock_args: rgbpp_args,
        capacity: 200_00000000,
        occupied_capacity: 100_00000000,
        type_code_hash: Some(type_code_hash.clone()),
        type_hash_type: Some(1),
        type_script_hash: Some(vec![0xCC; 32]),
        type_args: Some(type_args.clone()),
        udt_amount: Some(1000),
        data: vec![0; 16],
        is_dao_withdraw_request: false,
    };

    let output = crate::parser::cell::ParsedCell {
        lock_script_hash: vec![0x22; 32],
        lock_code_hash: standard_lock,
        lock_hash_type: 1,
        lock_args: vec![0x33; 20],
        capacity: 200_00000000,
        type_code_hash: Some(type_code_hash),
        type_hash_type: Some(1),
        type_script_hash: Some(vec![0xCC; 32]),
        type_args: Some(type_args),
        data: vec![0; 16],
        data_size: 16,
        udt_amount: Some(1000),
    };

    let outputs = vec![output];
    let tx = TxView {
        tx_hash: &[0x01; 32],
        block_hash: &[0x02; 32],
        tx_index: 0,
        block_number: 100,
        timestamp: 1_700_000_000,
        is_cellbase: false,
        inputs: vec![input],
        outputs: &outputs,
        witnesses: &[],
    };

    let token_info_cache = HashMap::new();
    let detector = super::rgbpp_detector::RgbppDetector::new(true);
    let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(detector)];
    let bundles = build_activity_bundles_for_block_with_detectors(
        &[tx], &token_info_cache, &detectors,
    );

    // Check that at least one owner has an rgbpp protocol action
    let all_actions: Vec<&ProtocolAction> = bundles[0].owners.iter()
        .flat_map(|o| &o.protocol_actions)
        .filter(|a| a.protocol == "rgbpp")
        .collect();
    assert!(!all_actions.is_empty(), "expected rgbpp protocol action");
    assert_eq!(all_actions[0].action, "leap_to_ckb");
    assert!(all_actions[0].metadata.get("btcTxid").is_some());
}

#[test]
fn test_rgbpp_transfer() {
    // Input: rgbpp lock + type script
    // Output: rgbpp lock (different BTC UTXO) + same type script
    // Expected: ProtocolAction { protocol: "rgbpp", action: "transfer" }

    let rgbpp_lock_code_hash = parse_hex_to_bytes(
        crate::parser::rgbpp::RGBPP_LOCK_CODE_HASH_MAINNET
    );
    let type_code_hash = vec![0xAA; 32];
    let type_args = vec![0xBB; 32];

    let mut input_args = vec![0u8; 36];
    input_args[0..4].copy_from_slice(&0u32.to_le_bytes());
    for i in 0..32 { input_args[4 + i] = (i + 1) as u8; }

    let mut output_args = vec![0u8; 36];
    output_args[0..4].copy_from_slice(&1u32.to_le_bytes());
    for i in 0..32 { output_args[4 + i] = (i + 10) as u8; }

    let input = InputCellView {
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: rgbpp_lock_code_hash.clone(),
        lock_hash_type: 1,
        lock_args: input_args,
        capacity: 200_00000000,
        occupied_capacity: 100_00000000,
        type_code_hash: Some(type_code_hash.clone()),
        type_hash_type: Some(1),
        type_script_hash: Some(vec![0xCC; 32]),
        type_args: Some(type_args.clone()),
        udt_amount: Some(1000),
        data: vec![0; 16],
        is_dao_withdraw_request: false,
    };

    let output = crate::parser::cell::ParsedCell {
        lock_script_hash: vec![0x11; 32], // same owner (same lock_hash)
        lock_code_hash: rgbpp_lock_code_hash,
        lock_hash_type: 1,
        lock_args: output_args,
        capacity: 200_00000000,
        type_code_hash: Some(type_code_hash),
        type_hash_type: Some(1),
        type_script_hash: Some(vec![0xCC; 32]),
        type_args: Some(type_args),
        data: vec![0; 16],
        data_size: 16,
        udt_amount: Some(1000),
    };

    let outputs = vec![output];
    let tx = TxView {
        tx_hash: &[0x01; 32],
        block_hash: &[0x02; 32],
        tx_index: 0,
        block_number: 100,
        timestamp: 1_700_000_000,
        is_cellbase: false,
        inputs: vec![input],
        outputs: &outputs,
        witnesses: &[],
    };

    let token_info_cache = HashMap::new();
    let detector = super::rgbpp_detector::RgbppDetector::new(true);
    let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(detector)];
    let bundles = build_activity_bundles_for_block_with_detectors(
        &[tx], &token_info_cache, &detectors,
    );

    let all_actions: Vec<&ProtocolAction> = bundles[0].owners.iter()
        .flat_map(|o| &o.protocol_actions)
        .filter(|a| a.protocol == "rgbpp")
        .collect();
    assert!(!all_actions.is_empty(), "expected rgbpp protocol action");
    assert_eq!(all_actions[0].action, "transfer");
}

#[test]
fn test_no_rgbpp_action_for_standard_locks() {
    // Input and output both have standard CKB locks, no type script
    // Expected: no protocol actions
    let tx = TxView {
        tx_hash: &[0x01; 32],
        block_hash: &[0x02; 32],
        tx_index: 0,
        block_number: 100,
        timestamp: 1_700_000_000,
        is_cellbase: false,
        inputs: vec![],
        outputs: &[],
        witnesses: &[],
    };

    let token_info_cache = HashMap::new();
    let detector = super::rgbpp_detector::RgbppDetector::new(true);
    let detectors: Vec<Box<dyn ProtocolDetector>> = vec![Box::new(detector)];
    let bundles = build_activity_bundles_for_block_with_detectors(
        &[tx], &token_info_cache, &detectors,
    );
    let all_actions: Vec<&ProtocolAction> = bundles[0].owners.iter()
        .flat_map(|o| &o.protocol_actions)
        .collect();
    assert!(all_actions.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-indexer test_rgbpp_leap_to_ckb`
Expected: FAIL — `rgbpp_detector` module doesn't exist.

**Step 3: Create rgbpp_detector.rs**

Create `crates/indexer/src/db/writer/rgbpp_detector.rs`:

```rust
//! RGB++ protocol action detector.
//!
//! Detects RGB++ actions by comparing lock scripts on cells that share
//! the same type_script identity (type_code_hash + type_args) across
//! inputs and outputs.

use std::collections::HashMap;

use ckbadger_store::types::{LockCallEntry, ProtocolAction, TypeCallEntry};

use super::activities::{InputCellView, OwnerAccum, ProtocolDetector, TxView};
use crate::parser::rgbpp::RgbppParser;

/// Cells grouped by type_script identity for lock transition analysis.
#[derive(Debug, Clone)]
struct TypedCell {
    lock_code_hash: Vec<u8>,
    lock_args: Vec<u8>,
}

/// Key for grouping cells by type_script identity.
type TypeIdentity = (Vec<u8>, Vec<u8>); // (type_code_hash, type_args)

pub struct RgbppDetector {
    is_mainnet: bool,
}

impl RgbppDetector {
    pub fn new(is_mainnet: bool) -> Self {
        Self { is_mainnet }
    }

    fn is_rgbpp_or_btc_time_lock(&self, code_hash: &[u8]) -> bool {
        RgbppParser::is_rgbpp_lock_code_hash(code_hash, self.is_mainnet)
            || RgbppParser::is_btc_time_lock_code_hash(code_hash, self.is_mainnet)
    }

    fn is_rgbpp_lock(&self, code_hash: &[u8]) -> bool {
        RgbppParser::is_rgbpp_lock_code_hash(code_hash, self.is_mainnet)
    }

    fn is_btc_time_lock(&self, code_hash: &[u8]) -> bool {
        RgbppParser::is_btc_time_lock_code_hash(code_hash, self.is_mainnet)
    }

    fn extract_btc_metadata(&self, lock_code_hash: &[u8], lock_args: &[u8]) -> serde_json::Value {
        if self.is_rgbpp_lock(lock_code_hash) {
            if let Some(parsed) = RgbppParser::parse_rgbpp_lock_args(lock_args) {
                return serde_json::json!({
                    "btcTxid": parsed.btc_txid,
                    "outIndex": parsed.out_index,
                });
            }
        } else if self.is_btc_time_lock(lock_code_hash) {
            if let Some(txid) = RgbppParser::extract_btc_txid_from_btc_time_lock_args(lock_args) {
                return serde_json::json!({
                    "btcTxid": txid,
                });
            }
        }
        serde_json::json!({})
    }
}

impl ProtocolDetector for RgbppDetector {
    fn protocol_name(&self) -> &str {
        "rgbpp"
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        _owner_lock_hash: &[u8],
        _accum: &OwnerAccum,
        _asset_changes: &[ckbadger_store::types::AssetChange],
        _type_calls: &[TypeCallEntry],
        _lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction> {
        // Collect typed cells from inputs grouped by type identity
        let mut input_typed: HashMap<TypeIdentity, Vec<TypedCell>> = HashMap::new();
        for input in &tx.inputs {
            if let (Some(ref tc), Some(ref ta)) = (&input.type_code_hash, &input.type_args) {
                let key = (tc.clone(), ta.clone());
                input_typed.entry(key).or_default().push(TypedCell {
                    lock_code_hash: input.lock_code_hash.clone(),
                    lock_args: input.lock_args.clone(),
                });
            }
        }

        // Collect typed cells from outputs grouped by type identity
        let mut output_typed: HashMap<TypeIdentity, Vec<TypedCell>> = HashMap::new();
        for output in tx.outputs {
            if let (Some(ref tc), Some(ref ta)) = (&output.type_code_hash, &output.type_args) {
                let key = (tc.clone(), ta.clone());
                output_typed.entry(key).or_default().push(TypedCell {
                    lock_code_hash: output.lock_code_hash.clone(),
                    lock_args: output.lock_args.clone(),
                });
            }
        }

        let mut actions = Vec::new();

        // Collect all type identities
        let mut all_type_ids: Vec<&TypeIdentity> = input_typed.keys().collect();
        for key in output_typed.keys() {
            if !input_typed.contains_key(key) {
                all_type_ids.push(key);
            }
        }

        for type_id in all_type_ids {
            let inputs = input_typed.get(type_id);
            let outputs = output_typed.get(type_id);

            let any_input_rgbpp = inputs.map_or(false, |cells| {
                cells.iter().any(|c| self.is_rgbpp_or_btc_time_lock(&c.lock_code_hash))
            });
            let any_output_rgbpp = outputs.map_or(false, |cells| {
                cells.iter().any(|c| self.is_rgbpp_or_btc_time_lock(&c.lock_code_hash))
            });

            if !any_input_rgbpp && !any_output_rgbpp {
                continue;
            }

            // Determine action based on lock transitions
            let input_cells = inputs.map(|v| v.as_slice()).unwrap_or(&[]);
            let output_cells = outputs.map(|v| v.as_slice()).unwrap_or(&[]);

            let (action, metadata_cell) = if !input_cells.is_empty() && !output_cells.is_empty() {
                // Both input and output exist for this type identity
                let in_rgbpp = input_cells.iter().find(|c| self.is_rgbpp_or_btc_time_lock(&c.lock_code_hash));
                let out_rgbpp = output_cells.iter().find(|c| self.is_rgbpp_or_btc_time_lock(&c.lock_code_hash));

                match (in_rgbpp, out_rgbpp) {
                    (Some(ic), Some(oc)) => {
                        if self.is_btc_time_lock(&oc.lock_code_hash) {
                            ("btc_time_locked", Some(oc))
                        } else {
                            ("transfer", Some(oc))
                        }
                    }
                    (Some(ic), None) => ("leap_to_ckb", Some(ic)),
                    (None, Some(oc)) => ("leap_to_btc", Some(oc)),
                    (None, None) => continue,
                }
            } else if input_cells.is_empty() {
                // Output only — receive
                let oc = output_cells.iter().find(|c| self.is_rgbpp_or_btc_time_lock(&c.lock_code_hash));
                match oc {
                    Some(c) => ("receive", Some(c)),
                    None => continue,
                }
            } else {
                // Input only with rgbpp lock, no matching output type identity
                let ic = input_cells.iter().find(|c| self.is_rgbpp_or_btc_time_lock(&c.lock_code_hash));
                match ic {
                    Some(c) => ("leap_to_ckb", Some(c)),
                    None => continue,
                }
            };

            let metadata = metadata_cell
                .map(|c| self.extract_btc_metadata(&c.lock_code_hash, &c.lock_args))
                .unwrap_or_else(|| serde_json::json!({}));

            actions.push(ProtocolAction {
                protocol: "rgbpp".into(),
                action: action.into(),
                metadata,
            });
        }

        actions
    }
}
```

**Step 4: Register the module**

In `crates/indexer/src/db/writer/mod.rs`, add:

```rust
pub(crate) mod rgbpp_detector;
```

**Step 5: Run cargo check**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 6: Run RGB++ tests**

Run: `cargo test -p ckbadger-indexer test_rgbpp_`
Expected: PASS

**Step 7: Run full test suite**

Run: `cargo test --lib`
Expected: PASS

**Step 8: Commit**

```
feat(indexer): implement RgbppDetector for protocol action framework
```

---

### Task 4: Wire RgbppDetector into sync pipeline

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` (~lines 2834-2863, 4099-4128)

**Step 1: Import and instantiate RgbppDetector in batch.rs**

At the TxView construction site (~line 2834 and ~line 4099), the detector needs to be created and passed. Find where `build_activity_bundles_for_block` is called and replace with `build_activity_bundles_for_block_with_detectors`.

Create the detector once per batch using the config's `is_mainnet` flag. Pass it to the activity builder.

**Step 2: Run cargo check**

Run: `cargo check`
Expected: PASS

**Step 3: Run full test suite**

Run: `cargo test --lib`
Expected: PASS

**Step 4: Commit**

```
feat(indexer): wire RgbppDetector into sync pipeline
```

---

### Task 5: Add protocol:\* filter to activity queries

**Files:**

- Modify: `crates/ckbadger-store/src/activity_ops.rs:301-331`
- Modify: `crates/api/src/routes/activities.rs:494-507`

**Step 1: Write test for protocol filter**

Add test to `crates/ckbadger-store/src/activity_ops.rs` test module:

```rust
#[test]
fn test_matches_activity_filter_protocol() {
    let mut entry = ActivityEntry {
        tx_hash: vec![1; 32],
        block_hash: vec![2; 32],
        block_number: 100,
        tx_index: 0,
        timestamp: 1_700_000_000,
        ckb_delta: 0,
        used_delta: 0,
        is_cellbase: false,
        has_type_script: false,
        asset_changes: vec![],
        type_calls: None,
        lock_calls: None,
        protocol_actions: vec![ProtocolAction {
            protocol: "rgbpp".into(),
            action: "leap_to_ckb".into(),
            metadata: serde_json::json!({}),
        }],
        peers: vec![],
    };

    assert!(CkbadgerStore::matches_activity_filter(&entry, Some("protocol:rgbpp")));
    assert!(!CkbadgerStore::matches_activity_filter(&entry, Some("protocol:fiber")));
    assert!(CkbadgerStore::matches_activity_filter(&entry, None));

    entry.protocol_actions = vec![];
    assert!(!CkbadgerStore::matches_activity_filter(&entry, Some("protocol:rgbpp")));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-store test_matches_activity_filter_protocol`
Expected: FAIL

**Step 3: Add protocol filter to matches_activity_filter**

In `activity_ops.rs`, before `Some(_) => false` (~line 329), add:

```rust
Some(f) if f.starts_with("protocol:") => {
    let protocol_name = &f["protocol:".len()..];
    entry.protocol_actions.iter().any(|a| a.protocol == protocol_name)
}
```

**Step 4: Update API filter validation**

In `crates/api/src/routes/activities.rs`, update `validate_activity_filter` (~line 494):

```rust
fn validate_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if !matches!(
            value,
            "all" | "ckb" | "token" | "nft" | "dao" | "type_call" | "lock_call"
        ) && !value.starts_with("protocol:")
        {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, nft, dao, type_call, lock_call, protocol:<name>",
                value
            )));
        }
    }
    Ok(())
}
```

**Step 5: Run tests**

Run: `cargo test -p ckbadger-store test_matches_activity_filter_protocol`
Expected: PASS

Run: `cargo test -p ckbadger-api test_validate_activity_filter`
Expected: PASS

**Step 6: Commit**

```
feat(store,api): add protocol:* filter for activity queries
```

---

### Task 6: Add protocol_actions to API response

**Files:**

- Modify: `crates/api/src/routes/activities.rs:38-52, 113-128, 418-487`

**Step 1: Add ProtocolActionResponse struct**

After `LockCallResponse` (~line 111), add:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolActionResponse {
    pub protocol: String,
    pub action: String,
    pub metadata: serde_json::Value,
}
```

**Step 2: Add protocol_actions field to ActivityResponse and GlobalActivityResponse**

Add `pub protocol_actions: Vec<ProtocolActionResponse>` to both structs (after `lock_calls`).

**Step 3: Add conversion in build_activity_response and build_global_activity_response**

After `lock_calls` in both functions, add:

```rust
protocol_actions: entry.protocol_actions.iter().map(|a| ProtocolActionResponse {
    protocol: a.protocol.clone(),
    action: a.action.clone(),
    metadata: a.metadata.clone(),
}).collect(),
```

(For `build_global_activity_response`, use `item.entry.protocol_actions`.)

**Step 4: Run cargo check**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 5: Run API tests**

Run: `cargo test -p ckbadger-api`
Expected: PASS (existing tests should still pass — they construct bundles without protocol_actions, which defaults to empty vec)

**Step 6: Commit**

```
feat(api): add protocol_actions to activity response
```

---

### Task 7: Add protocol_action_counts to daily stats

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:35-116, 630-690`

**Step 1: Write test**

Add to statistics.rs test module:

```rust
#[test]
fn test_accumulate_protocol_action_counts() {
    let mut stats = DailyActivityStats::default();
    let entry = ActivityEntry {
        tx_hash: vec![1; 32],
        block_hash: vec![2; 32],
        block_number: 100,
        tx_index: 0,
        timestamp: 1_700_000_000,
        ckb_delta: 100,
        used_delta: 0,
        is_cellbase: false,
        has_type_script: true,
        asset_changes: vec![],
        type_calls: None,
        lock_calls: None,
        protocol_actions: vec![
            ProtocolAction {
                protocol: "rgbpp".into(),
                action: "leap_to_ckb".into(),
                metadata: serde_json::json!({}),
            },
        ],
        peers: vec![],
    };
    BatchWriter::accumulate_activity_stats(&entry, &[], &mut stats);
    assert_eq!(*stats.protocol_action_counts.get("rgbpp:leap_to_ckb").unwrap(), 1);
}
```

**Step 2: Run test to verify it fails**

Expected: FAIL

**Step 3: Add protocol_action_counts accumulation**

In `accumulate_activity_stats_inner`, add a `protocol_actions: &[ProtocolAction]` parameter. After script counting (~line 60), add:

```rust
for pa in protocol_actions {
    let key = format!("{}:{}", pa.protocol, pa.action);
    *stats.protocol_action_counts.entry(key).or_insert(0) += 1;
}
```

Update call sites (`accumulate_activity_stats` and `accumulate_owner_activity_stats`) to pass `&entry.protocol_actions` / `&owner.protocol_actions`.

**Step 4: Merge protocol_action_counts in update_daily_activity_stats**

In `update_daily_activity_stats` (~line 674), after script_counts merge:

```rust
for (key, count) in &accumulated.protocol_action_counts {
    *e.protocol_action_counts.entry(key.clone()).or_insert(0) += count;
}
```

Do the same in `update_hourly_activity_stats` if it exists.

**Step 5: Run test**

Expected: PASS

**Step 6: Run full test suite**

Run: `cargo test --lib`
Expected: PASS

**Step 7: Commit**

```
feat(indexer): accumulate protocol_action_counts in daily activity stats
```

---

### Task 8: Frontend types and classification

**Files:**

- Modify: `frontend/lib/api.ts:454-491`
- Modify: `frontend/lib/activity-classify.ts`
- Modify: `frontend/__tests__/lib/activity-classify.test.ts`

**Step 1: Add TypeScript types**

In `frontend/lib/api.ts`, after `ActivityLockCall` interface (~line 462), add:

```typescript
interface ActivityProtocolAction {
  protocol: string;
  action: string;
  metadata: Record<string, unknown>;
}
```

Add `protocolActions: ActivityProtocolAction[];` to both `Activity` and `GlobalActivity` interfaces.

Export `ActivityProtocolAction` in the existing export list.

**Step 2: Update ClassifiedActivity**

In `frontend/lib/activity-classify.ts`, add to `ClassifiedActivity`:

```typescript
primaryProtocolAction: ActivityProtocolAction | null;
```

Import `ActivityProtocolAction` from `@/lib/api`.

**Step 3: Update classifyActivity**

Insert before the existing asset change check (line 40):

```typescript
// 0. Protocol actions — highest level interpretation
if (activity.protocolActions.length > 0) {
  return {
    type: 'protocolAction',
    activity,
    primaryAssetChange: activity.assetChanges[0] ?? null,
    primaryTypeCall: activity.typeCalls[0] ?? null,
    primaryLockCall: activity.lockCalls[0] ?? null,
    primaryProtocolAction: activity.protocolActions[0],
  };
}
```

Add `primaryProtocolAction: null` to all other return paths.

**Step 4: Update makeActivity in test helper**

In `frontend/__tests__/lib/activity-classify.test.ts`, add to `makeActivity`:

```typescript
protocolActions: overrides.protocolActions ?? [],
```

**Step 5: Add test for protocol action classification**

```typescript
it('classifies protocol action as protocolAction', () => {
  const result = classifyActivity(
    makeActivity({
      protocolActions: [
        { protocol: 'rgbpp', action: 'leap_to_ckb', metadata: { btcTxid: 'abc123' } },
      ],
    })
  );
  expect(result.type).toBe('protocolAction');
  expect(result.primaryProtocolAction?.protocol).toBe('rgbpp');
  expect(result.primaryProtocolAction?.action).toBe('leap_to_ckb');
});

it('protocol action takes priority over asset changes', () => {
  const result = classifyActivity(
    makeActivity({
      protocolActions: [{ protocol: 'rgbpp', action: 'transfer', metadata: {} }],
      assetChanges: [
        { type: 'token', typeScriptHash: '0xt', delta: '100', symbol: 'X', decimals: 8 },
      ],
    })
  );
  expect(result.type).toBe('protocolAction');
  expect(result.primaryAssetChange?.type).toBe('token');
});
```

**Step 6: Run frontend tests**

Run: `cd frontend && npx vitest run __tests__/lib/activity-classify.test.ts`
Expected: PASS

**Step 7: Commit**

```
feat(frontend): add protocolActions type and classification priority
```

---

### Task 9: Frontend display — protocol action event rows

**Files:**

- Modify: `frontend/components/activity-event-row.tsx:344-355`
- Modify: `frontend/__tests__/components/activity-event-row.test.tsx`

**Step 1: Add getProtocolActionEventParts**

In `activity-event-row.tsx`, after `getLockEventParts` (~line 305), add:

```typescript
function formatProtocolAction(action: string): string {
  return action.replace(/_/g, ' ');
}

function getProtocolActionEventParts(pa: ActivityProtocolAction): EventParts {
  const label = `${pa.protocol} \u00B7 ${formatProtocolAction(pa.action)}`;
  const btcTxid = pa.metadata?.btcTxid as string | undefined;

  return {
    badge: (
      <span className="text-orange font-mono text-xs">
        {'\u2B21'} {label}
      </span>
    ),
    value: btcTxid ? (
      <span className="text-text-dim font-mono text-xs">
        btc:{truncateHash(btcTxid, 8, 6)}
      </span>
    ) : null,
  };
}
```

Import `ActivityProtocolAction` from `@/lib/api`.

**Step 2: Insert protocol action rows into event list**

In `ActivityEventGroup` (~line 344), after the events array init and before `activity.assetChanges.forEach`, add:

```typescript
activity.protocolActions?.forEach((pa) => {
  events.push(getProtocolActionEventParts(pa));
});
```

**Step 3: Add lock call deduplication**

Replace the `activity.lockCalls.forEach` block with:

```typescript
const protocolNames = new Set((activity.protocolActions ?? []).map((pa) => pa.protocol));
activity.lockCalls.forEach((lc) => {
  const decodedProtocol = lc.decoded?.protocol as string | undefined;
  if (decodedProtocol && protocolNames.has(decodedProtocol)) return;
  events.push(getLockEventParts(lc));
});
```

**Step 4: Add test**

In `frontend/__tests__/components/activity-event-row.test.tsx`, add test for protocol action rendering (verify the component renders without errors when `protocolActions` is present).

**Step 5: Run tests**

Run: `cd frontend && npx vitest run`
Expected: PASS

**Step 6: Run type check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 7: Commit**

```
feat(frontend): render protocol action event rows with lock call deduplication
```

---

### Task 10: Add protocol filter to address page

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: Find existing filter tabs**

Locate where `lock_call` filter is defined in the address page filter list.

**Step 2: Add protocol:rgbpp filter option**

Add `{ label: 'RGB++', value: 'protocol:rgbpp' }` to the filter options array.

**Step 3: Run type check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): add RGB++ filter to address activity page
```

---

### Task 11: Final verification

**Step 1: Run full Rust test suite**

Run: `cargo test`
Expected: PASS

**Step 2: Run full frontend test suite**

Run: `cd frontend && npx vitest run`
Expected: PASS

**Step 3: Run pre-commit checks**

Run: `cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 4: Commit (if any formatting changes)**

```
chore: format
```
