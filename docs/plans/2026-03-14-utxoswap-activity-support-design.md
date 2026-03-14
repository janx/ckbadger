# UTXOSwap Activity Support Design

**Date**: 2026-03-14
**Status**: Approved
**Scope**: Indexer (parser + detector), API (lock args decoder), Script labels

## Problem

UTXOSwap is a DEX on CKB using an intent-based architecture. Users create intent cells (locked with the UTXOSwap intent lock) expressing swap/liquidity orders, which a sequencer aggregates and settles on-chain. Currently, UTXOSwap interactions appear as generic "CKB Transfer" or unrecognized lock calls with no protocol context.

## Background

UTXOSwap and Stable++ are independent projects from separate teams. They both use intent-based architecture but have different scripts and different purposes (DEX vs CDP).

### UTXOSwap Scripts

Source: UTXOSwap Sequencer API (`/api/v1/sequencer/configurations`) and [utxoswap-sdk-js](https://github.com/UTXOSwap/utxoswap-sdk-js).

| Script          | Mainnet code_hash                                                    | Testnet code_hash                                                    | hash_type    |
| --------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | ------------ |
| **Intent Lock** | `0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e` | `0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101` | type         |
| Pool Type       | `0xc70a8b00526419826023bcf196852eecdc87406cdff7366234f6387265413c98` | `0x5b9228b156fc20c2f091ce0ebd366aac6a2510fff150c6664f065edff59f8735` | type         |
| LP Type         | `0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95` | `0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb` | data1 / type |
| Proxy Lock      | `0x393df3359e33f85010cd65a3c4a4268f72d95ec6b049781a916c680b31ea9a88` | `0x75ac906998b047602967d7f89505bb9817e405b89f868111ded51d672f9e260e` | type         |

Only the intent lock code_hash is needed for the detector. Pool/LP/Proxy are not detection targets (LP tokens are already xudt_compatible; pool type appears as TypeCallEntry with script name from labels).

### Intent Args Structure

**Swap / AddLiquidity / RemoveLiquidity / SwapExactOutput / ClaimProtocolLiquidity** (90 bytes):

| Offset | Size | Field             | Type                                         |
| ------ | ---- | ----------------- | -------------------------------------------- |
| 0      | 20B  | `owner_lock_hash` | First 20 bytes of blake2b(owner lock script) |
| 20     | 20B  | `pool_type_hash`  | First 20 bytes of pool type script hash      |
| 40     | 8B   | `tx_fee`          | u64 LE                                       |
| 48     | 8B   | `expire_batch_id` | u64 LE                                       |
| 56     | 1B   | `intent_type`     | u8 enum (see below)                          |
| 57     | 1B   | `asset_in_index`  | u8 (0 or 1, assetX or assetY)                |
| 58     | 16B  | `amount_in`       | u128 LE                                      |
| 74     | 16B  | `amount_out_min`  | u128 LE                                      |

**CreatePool** (154 bytes):

| Offset | Size | Field            | Type                 |
| ------ | ---- | ---------------- | -------------------- |
| 0–55   | 56B  | (same header)    |                      |
| 56     | 1B   | `intent_type`    | 0 (CreatePool)       |
| 57     | 1B   | `total_fee_rate` | u8                   |
| 58     | 32B  | `asset_x`        | type_hash of token X |
| 90     | 32B  | `asset_y`        | type_hash of token Y |
| 122    | 16B  | `amount_x`       | u128 LE              |
| 138    | 16B  | `amount_y`       | u128 LE              |

**Intent Type enum**:

| Value | Name                    |
| ----- | ----------------------- |
| 0     | CreatePool              |
| 1     | AddLiquidity            |
| 2     | RemoveLiquidity         |
| 3     | SwapExactInputForOutput |
| 4     | SwapInputForExactOutput |
| 5     | ClaimProtocolLiquidity  |

## Design

### Detection Scope

All 6 intent types, both creation (submitted) and settlement (settled) phases:

| Phase      | Trigger                                             | Action suffix |
| ---------- | --------------------------------------------------- | ------------- |
| Creation   | Intent lock on tx **output**                        | `*_submitted` |
| Settlement | Intent lock on tx **input** (consumed by sequencer) | `*_settled`   |

12 total action names: `create_pool_submitted`, `create_pool_settled`, `add_liquidity_submitted`, `add_liquidity_settled`, `remove_liquidity_submitted`, `remove_liquidity_settled`, `swap_exact_input_submitted`, `swap_exact_input_settled`, `swap_exact_output_submitted`, `swap_exact_output_settled`, `claim_protocol_liquidity_submitted`, `claim_protocol_liquidity_settled`.

### Owner Attribution

- **Submitted**: attributed to owners with `ckb_delta < 0` (CKB flowing out to fund the intent cell).
- **Settled**: the intent lock args contain `owner_lock_hash` (first 20 bytes of blake2b). Prefix-match against the first 20 bytes of each owner's lock_hash in the `accum` map. 160-bit prefix gives negligible collision risk with typical <10 owners per tx.

### Metadata

Semantic subset (skip internal fields `tx_fee`, `expire_batch_id`):

```json
{
  "intentType": "SwapExactInputForOutput",
  "poolTypeHash": "0xa3f2...",
  "amountIn": "1000000000000",
  "amountOutMin": "500000000",
  "assetInIndex": 0
}
```

For CreatePool, additional fields: `"assetX"`, `"assetY"`, `"amountX"`, `"amountY"`, `"totalFeeRate"`.

### Parser Module

New file: `crates/indexer/src/parser/utxoswap.rs`

Follows `stablepp.rs` pattern:

```rust
// Code hash constants (mainnet + testnet)
const INTENT_LOCK_MAINNET: &str = "0x3547c9aa...";
const INTENT_LOCK_TESTNET: &str = "0x4e9c30c8...";

// Intent type enum
pub enum IntentType {
    CreatePool = 0,
    AddLiquidity = 1,
    RemoveLiquidity = 2,
    SwapExactInputForOutput = 3,
    SwapInputForExactOutput = 4,
    ClaimProtocolLiquidity = 5,
}

// Parsed intent args
pub struct ParsedIntentArgs {
    pub owner_lock_hash: [u8; 20],
    pub pool_type_hash: [u8; 20],
    pub intent_type: IntentType,
    pub asset_in_index: u8,
    pub amount_in: u128,
    pub amount_out_min: u128,
    pub create_pool_extra: Option<CreatePoolExtra>,
}

pub struct CreatePoolExtra {
    pub total_fee_rate: u8,
    pub asset_x: [u8; 32],
    pub asset_y: [u8; 32],
    pub amount_x: u128,
    pub amount_y: u128,
}

// Public API
pub fn is_intent_lock(code_hash: &[u8]) -> bool
pub fn parse_intent_args(args: &[u8]) -> Option<ParsedIntentArgs>
```

Parsing validates minimum length (90 for non-CreatePool, 154 for CreatePool). Returns `None` on malformed args.

### Detector

New file: `crates/indexer/src/db/writer/utxoswap_detector.rs`

```rust
pub(crate) struct UtxoSwapDetector { is_mainnet: bool }

impl ProtocolDetector for UtxoSwapDetector {
    fn protocol_name(&self) -> &str { "utxoswap" }

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        accum: &OwnerAccum,
        asset_changes: &[AssetChange],
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction> { ... }
}
```

Detection flow:

1. Scan tx outputs for intent lock → parse args → emit `{action}_submitted` for owners with `ckb_delta < 0`
2. Scan tx inputs for intent lock → parse args from input lock_args → emit `{action}_settled` for owners matching `owner_lock_hash` prefix (first 20 bytes)
3. Both events can coexist in one tx (distinct events, no dedup needed)

### Pipeline Registration

In `crates/indexer/src/sync/batch.rs`, add to both bulk and live sync detector lists:

```rust
Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
    self.config.is_mainnet(),
)),
```

### API Lock Args Decoder

In `crates/api/src/routes/activities.rs`, add `decode_utxoswap_intent` to `LOCK_ARGS_DECODERS`:

```rust
fn decode_utxoswap_intent(args: &[u8]) -> Option<serde_json::Value> {
    // Reuses parser::utxoswap::parse_intent_args
    // Returns semantic subset: intentType, poolTypeHash, amountIn, amountOutMin, assetInIndex
    // CreatePool adds: assetX, assetY, amountX, amountY, totalFeeRate
}
```

Registered for both mainnet and testnet intent lock code_hashes.

### Script Labels

Add `docs/token-labels/information/script/utxoswap-intent-lock/index.json`:

```json
{
  "name": "UTXOSwap Intent Lock",
  "deployments": {
    "mainnet": [{ "codeHash": "0x3547c9aa...", "hashType": "type" }],
    "testnet": [{ "codeHash": "0x4e9c30c8...", "hashType": "type" }]
  }
}
```

### Frontend

No changes needed. The protocol action framework handles rendering, classification, filtering (`protocol:utxoswap`), and daily stats automatically.

## Files Changed

| Layer    | File                                                                   | Change                                                                  |
| -------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Parser   | `crates/indexer/src/parser/utxoswap.rs`                                | **New**: code hash constants, `is_intent_lock()`, `parse_intent_args()` |
| Parser   | `crates/indexer/src/parser/mod.rs`                                     | Add `pub mod utxoswap`                                                  |
| Detector | `crates/indexer/src/db/writer/utxoswap_detector.rs`                    | **New**: `UtxoSwapDetector` impl                                        |
| Detector | `crates/indexer/src/db/writer/mod.rs`                                  | Add `pub(crate) mod utxoswap_detector`                                  |
| Pipeline | `crates/indexer/src/sync/batch.rs`                                     | Register `UtxoSwapDetector` in bulk + live detector lists               |
| API      | `crates/api/src/routes/activities.rs`                                  | Add `decode_utxoswap_intent` to `LOCK_ARGS_DECODERS`                    |
| Labels   | `docs/token-labels/information/script/utxoswap-intent-lock/index.json` | **New**: intent lock script label                                       |

## Tests

### Parser (`utxoswap.rs`)

- `test_is_intent_lock_mainnet` / `test_is_intent_lock_testnet` — positive
- `test_is_intent_lock_rejects_other` — negative
- `test_parse_swap_intent_args` — 90-byte swap args → correct fields
- `test_parse_create_pool_intent_args` — 154-byte create pool → correct extra fields
- `test_parse_intent_args_too_short` — <90 bytes → None
- `test_parse_intent_args_unknown_type` — invalid intent_type byte → None
- `test_all_hashes_32_bytes` / `test_hashes_distinct`

### Detector (`utxoswap_detector.rs`)

- `test_utxoswap_detector_protocol_name` — returns `"utxoswap"`
- `test_no_intent_lock_returns_empty` — no intent lock in tx → empty
- `test_swap_submitted` — intent lock on output + owner ckb_delta < 0 → `swap_exact_input_submitted`
- `test_swap_settled` — intent lock on input + owner prefix match → `swap_exact_input_settled`
- `test_add_liquidity_submitted` — intent_type=1 → `add_liquidity_submitted`
- `test_create_pool_submitted` — intent_type=0 with 154-byte args → `create_pool_submitted`
- `test_no_action_for_standard_locks_only` — no intent lock → no protocol actions
- `test_settled_owner_prefix_match` — verify 20-byte prefix match works

### API (`activities.rs`)

- `test_decode_utxoswap_intent_swap` — valid 90-byte args → correct JSON
- `test_decode_utxoswap_intent_create_pool` — valid 154-byte args → includes extra fields
- `test_decode_utxoswap_intent_too_short` — <90 bytes → None
- `test_utxoswap_intent_locks_have_decoders` — both code_hashes registered

## Reindex

Yes. New protocol actions in serialized `ActivityEntry`. Delete DB and re-sync from genesis.
