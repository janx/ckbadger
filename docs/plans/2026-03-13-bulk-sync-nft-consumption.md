# Bulk Sync NFT Consumption Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable spore and mNFT consumption processing during bulk sync, fixing permanently incorrect `is_live`, `live_count`, `holders_count`, and owner aggregates after bulk sync.

**Architecture:** Extend the parser-stage `run_nft_precompute()` to identify consumed spore/mNFT cells from `input_cell_info.type_code_hash` + `type_args` (same pattern as DotBit). Pass pre-identified consumption events to T6a (spore) and T6b (mNFT) writer threads via `PreParsedNftData`. Handle same-batch create-then-consume ordering with the same interleaving logic used for DotBit.

**Tech Stack:** Rust, RocksDB, existing parser/writer infrastructure

**Requires re-sync from genesis after implementation.**

---

### Task 1: Add Consumption Event Types

**Files:**

- Modify: `crates/indexer/src/sync/types.rs:33-60`

**Step 1: Add `SporeConsumptionEvent` and `MnftConsumptionEvent` structs**

```rust
pub(crate) struct SporeConsumptionEvent {
    pub(crate) spore_id: Vec<u8>,
    pub(crate) block_number: i64,
    pub(crate) consuming_tx_hash: [u8; 32],
    pub(crate) tx_global_index: usize,
}

pub(crate) struct MnftConsumptionEvent {
    pub(crate) token_id: Vec<u8>,
    pub(crate) block_number: i64,
    pub(crate) consuming_tx_hash: [u8; 32],
    pub(crate) tx_global_index: usize,
}
```

**Step 2: Extend `PreParsedNftData`**

Add two new fields after `consumed_dotbit`:

```rust
pub(crate) struct PreParsedNftData {
    pub(crate) mnft_issuers: Vec<(usize, ParsedMnftIssuer)>,
    pub(crate) mnft_classes: Vec<(usize, usize, ParsedMnftClass)>,
    pub(crate) mnft_tokens: Vec<(usize, usize, ParsedMnftToken)>,
    pub(crate) dotbit_accounts: Vec<(usize, ParsedDotbitAccountOutput)>,
    pub(crate) consumed_dotbit: Vec<DotbitConsumptionEvent>,
    pub(crate) consumed_spore: Vec<SporeConsumptionEvent>,      // NEW
    pub(crate) consumed_mnft: Vec<MnftConsumptionEvent>,        // NEW
    pub(crate) dotbit_tx_actions: HashMap<usize, String>,
}
```

**Step 3: Update `PreParsedNftData` construction sites**

Search for all places that construct `PreParsedNftData` and add the two new fields initialized to `Vec::new()` (will be populated in Task 3).

**Step 4: Run `cargo check` to verify compilation**

Run: `cargo check`
Expected: PASS (new fields initialized to empty)

**Step 5: Commit**

```
feat: add spore/mNFT consumption event types for bulk sync
```

---

### Task 2: Add Ordering Helpers for Spore/mNFT

**Files:**

- Modify: `crates/indexer/src/sync/nft_helpers.rs`
- Test: inline `#[cfg(test)]` module in same file

**Step 1: Write failing tests for ordering helpers**

Add tests to the existing `#[cfg(test)]` module in `nft_helpers.rs`:

```rust
#[test]
fn test_should_consume_spore_no_recreate() {
    // No later create in batch → should consume
    assert!(should_consume_spore(None, 10));
}

#[test]
fn test_should_consume_spore_recreated_after() {
    // Created again after consume → should NOT consume (transfer, not burn)
    assert!(!should_consume_spore(Some(12), 10));
}

#[test]
fn test_should_consume_spore_recreated_before() {
    // Created before consume → should consume
    assert!(should_consume_spore(Some(8), 10));
}

#[test]
fn test_should_consume_mnft_no_recreate() {
    assert!(should_consume_mnft_token(None, 10));
}

#[test]
fn test_should_consume_mnft_recreated_after() {
    assert!(!should_consume_mnft_token(Some(12), 10));
}

#[test]
fn test_should_consume_mnft_recreated_before() {
    assert!(should_consume_mnft_token(Some(8), 10));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-indexer should_consume_spore`
Expected: FAIL -- functions don't exist

**Step 3: Implement ordering helpers**

Add to `nft_helpers.rs`, following the same pattern as `should_consume_dotbit_account`:

```rust
/// Returns true if the spore should be consumed in this batch.
/// If the spore was re-created later in the same batch (transfer), skip consumption.
pub(crate) fn should_consume_spore(
    latest_create_tx_index: Option<usize>,
    consume_tx_index: usize,
) -> bool {
    match latest_create_tx_index {
        Some(last_create) => last_create < consume_tx_index,
        None => true,
    }
}

/// Returns true if the mNFT token should be consumed in this batch.
/// If the token was re-created later in the same batch (transfer), skip consumption.
pub(crate) fn should_consume_mnft_token(
    latest_create_tx_index: Option<usize>,
    consume_tx_index: usize,
) -> bool {
    match latest_create_tx_index {
        Some(last_create) => last_create < consume_tx_index,
        None => true,
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-indexer should_consume_spore -- --nocapture && cargo test -p ckbadger-indexer should_consume_mnft_token -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add spore/mNFT same-batch ordering helpers
```

---

### Task 3: Extend `run_nft_precompute` to Identify Spore/mNFT Consumptions

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs:140-290` (`run_nft_precompute`)

**Step 1: Add spore/mNFT code hash imports**

At the top of the function or in the imports, make the spore and mNFT code hash constants available. The function already has access to `input_cell_info` and `batch_cell_infos` which contain `type_code_hash` and `type_args`.

Import or reference:

- `SporeParser::is_spore_type_script` (from `parser/spore.rs`)
- `MnftParser::is_mnft_token_code_hash` or the raw code hash constants from `parser/mnft.rs`

**Step 2: Add tracking maps for same-batch create-consume ordering**

At the beginning of `run_nft_precompute`, add:

```rust
// Track latest creation tx_global_index per spore_id and mnft token_id
// for same-batch consume-after-create detection
let mut batch_spore_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();
let mut batch_mnft_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();
```

**Step 3: In Pass 1 (output scanning), track spore/mNFT creation tx_global_index**

In the existing output scanning loop (the one that already processes mNFT and DotBit outputs), add spore tracking. For each output cell, check if it's a spore type script and record:

```rust
// Inside the output scanning loop, after existing mNFT/DotBit processing:
if let Some(ref type_code_hash) = cell.type_code_hash {
    // Track spore creations for same-batch ordering
    if SporeParser::is_spore_type_script_bytes(type_code_hash) {
        if let Some(ref type_args) = cell.type_args {
            if !type_args.is_empty() {
                batch_spore_latest_create.insert(type_args.clone(), tx_global_index);
            }
        }
    }
    // Track mNFT token creations for same-batch ordering
    if MnftParser::is_mnft_token_code_hash_bytes(type_code_hash) {
        if let Some(ref type_args) = cell.type_args {
            if type_args.len() >= 28 {
                batch_mnft_latest_create.insert(type_args.clone(), tx_global_index);
            }
        }
    }
}
```

Note: You may need to add `is_spore_type_script_bytes` and `is_mnft_token_code_hash_bytes` methods that take `&[u8]` (the parsed bytes) rather than hex strings. Check if these already exist on the parser structs or add them. The spore parser has `is_spore_type_script(code_hash: &str)` — you'll need a bytes variant. Use `LazyLock` byte constants like the ones added in the earlier fix for UDT.

**Step 4: In Pass 2 (input scanning), identify spore/mNFT consumptions**

After the existing DotBit consumption identification, add spore/mNFT identification. The pattern mirrors DotBit exactly — check `type_code_hash` on input cells:

```rust
// After DotBit consumption identification in Pass 2:
let mut consumed_spore = Vec::new();
let mut consumed_mnft = Vec::new();

for (tx_global_index, parsed_block, tx_data) in all_txs_iter {
    let block_number = parsed_block.block_number;
    let consuming_tx_hash: [u8; 32] = tx_data.hash.clone().try_into()
        .map_err(|_| anyhow::anyhow!("tx hash not 32 bytes"))?;

    for input in &tx_data.inputs {
        let key = (input.previous_output_tx_hash.clone(), input.previous_output_index);

        // Look up input cell info (from DB prefetch or batch outputs)
        let cell_info = input_cell_info.get(&key)
            .or_else(|| batch_cell_infos.get(&key));

        if let Some(cell_info) = cell_info {
            if let Some(ref type_code_hash) = cell_info.type_code_hash {
                // Check spore consumption
                if SporeParser::is_spore_type_script_bytes(type_code_hash) {
                    if let Some(ref type_args) = cell_info.type_args {
                        if !type_args.is_empty() {
                            consumed_spore.push(SporeConsumptionEvent {
                                spore_id: type_args.clone(),
                                block_number,
                                consuming_tx_hash,
                                tx_global_index,
                            });
                        }
                    }
                }
                // Check mNFT token consumption
                if MnftParser::is_mnft_token_code_hash_bytes(type_code_hash) {
                    if let Some(ref type_args) = cell_info.type_args {
                        if type_args.len() >= 28 {
                            consumed_mnft.push(MnftConsumptionEvent {
                                token_id: type_args.clone(),
                                block_number,
                                consuming_tx_hash,
                                tx_global_index,
                            });
                        }
                    }
                }
            }
        }
    }
}
```

**Step 5: Populate the new fields in the return value**

Update the `PreParsedNftData` construction at the end of `run_nft_precompute`:

```rust
Ok(PreParsedNftData {
    mnft_issuers,
    mnft_classes,
    mnft_tokens,
    dotbit_accounts,
    consumed_dotbit,
    consumed_spore,  // NEW
    consumed_mnft,   // NEW
    dotbit_tx_actions,
})
```

**Step 6: Add byte-level code hash matching methods if needed**

If `SporeParser::is_spore_type_script_bytes(&[u8]) -> bool` and `MnftParser::is_mnft_token_code_hash_bytes(&[u8]) -> bool` don't exist, add them in `parser/spore.rs` and `parser/mnft.rs`:

```rust
// In parser/spore.rs, add LazyLock byte constants and a bytes method:
use std::sync::LazyLock;

static SPORE_CODE_HASHES_BYTES: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    vec![
        parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2),
        parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_DID),
        parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V2),
        parse_hex_to_bytes(SPORE_CODE_HASH_TESTNET_V1),
    ]
});

pub fn is_spore_type_script_bytes(code_hash: &[u8]) -> bool {
    SPORE_CODE_HASHES_BYTES.iter().any(|h| h.as_slice() == code_hash)
}
```

```rust
// In parser/mnft.rs:
static MNFT_TOKEN_CODE_HASH_BYTES: LazyLock<Vec<u8>> = LazyLock::new(|| {
    parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH)
});

pub fn is_mnft_token_code_hash_bytes(code_hash: &[u8]) -> bool {
    code_hash == MNFT_TOKEN_CODE_HASH_BYTES.as_slice()
}
```

**Step 7: Run `cargo check` and `cargo test -p ckbadger-indexer --lib`**

Expected: PASS

**Step 8: Commit**

```
feat: identify spore/mNFT consumptions in parser precompute stage
```

---

### Task 4: Process Spore Consumption in T6a (Bulk Sync)

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` (T6a thread, ~lines 2142-2265)

**Step 1: Add consumption phase to T6a**

After the existing spore creation phase in T6a, add a consumption phase. Follow the DotBit consumption pattern from T6b.

```rust
// After spore creation loop in T6a, add:

// Phase 2: Spore consumption
for event in &pre_parsed_nft_data.consumed_spore {
    let should_consume = should_consume_spore(
        batch_spore_latest_create.get(&event.spore_id).copied(),
        event.tx_global_index,
    );
    if should_consume {
        if let Some(collection_id) = writer.consume_spore(
            &event.spore_id,
            event.block_number,
            &event.consuming_tx_hash,
            &mut batch,
            &mut spore_state,
        )? {
            // Record consumption activity if needed
            // (follow existing activity recording pattern in T6a)
        }
    }
}
```

**Note:** You need to build `batch_spore_latest_create` in T6a. During the creation loop, track:

```rust
let mut batch_spore_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();

// Inside the creation loop, after insert_spore_cell:
batch_spore_latest_create
    .entry(spore_id.clone())
    .and_modify(|idx| *idx = (*idx).max(tx_global_index))
    .or_insert(tx_global_index);
```

Wait — the `batch_spore_latest_create` should already come from `run_nft_precompute` (Task 3 Step 3) so it can be passed via `PreParsedNftData`. Actually, it's simpler to just build it in T6a from the creation data that's already being iterated. T6a already iterates all spore creations, so adding a HashMap entry is trivial.

**Step 2: Import the ordering helper**

Add import at the top of batch.rs or in the T6a closure:

```rust
use crate::sync::nft_helpers::should_consume_spore;
```

**Step 3: Run `cargo check`**

Expected: PASS

**Step 4: Commit**

```
feat: process spore consumption in T6a during bulk sync
```

---

### Task 5: Process mNFT Consumption in T6b (Bulk Sync)

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` (T6b thread, ~lines 2267-2467)

**Step 1: Add consumption phase to T6b for mNFT**

After the existing mNFT token creation phase in T6b (and after the DotBit consumption phase), add mNFT consumption:

```rust
// After DotBit consumption in T6b, add:

// Phase 3: mNFT token consumption
let mut batch_mnft_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();
// Build from the mNFT tokens already processed above
for (tx_global_index, _output_index, token) in &pre_parsed_nft_data.mnft_tokens {
    batch_mnft_latest_create
        .entry(token.token_id.clone())
        .and_modify(|idx| *idx = (*idx).max(*tx_global_index))
        .or_insert(*tx_global_index);
}

for event in &pre_parsed_nft_data.consumed_mnft {
    let should_consume = should_consume_mnft_token(
        batch_mnft_latest_create.get(&event.token_id).copied(),
        event.tx_global_index,
    );
    if should_consume {
        if let Some(collection_id) = writer.consume_mnft_token_with_state(
            &event.token_id,
            event.block_number,
            &event.consuming_tx_hash,
            &mut batch,
            &mut mnft_state,
        )? {
            // Record consumption activity if needed
        }
    }
}
```

**Step 2: Import the ordering helper**

```rust
use crate::sync::nft_helpers::should_consume_mnft_token;
```

**Step 3: Run `cargo check`**

Expected: PASS

**Step 4: Commit**

```
feat: process mNFT consumption in T6b during bulk sync
```

---

### Task 6: Remove Bulk Sync Guards from Live Sync Path

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs` (live sync NFT consumption, ~lines 3696-3912)

**Step 1: Remove the `if bulk_sync_active { Vec::new() }` guards**

The live sync path (Group C) has guards at lines 3750-3765 that skip spore/mNFT consumption during bulk sync. Now that bulk sync has its own consumption path via T6a/T6b, the live sync path should ONLY run when NOT in bulk sync. These guards should remain — they correctly prevent duplicate consumption since bulk sync uses the pipeline path (T6a/T6b), not the grouped path.

**Actually, no change needed here.** The bulk sync pipeline uses T6a/T6b (the threaded path), not the grouped path. The `bulk_sync_active` guard correctly prevents the grouped path from doing redundant consumption. Leave the guards in place.

**Step 2: Verify understanding**

Confirm that:

- Bulk sync → pipeline path → T6a/T6b (now with consumption from Tasks 4-5)
- Live sync → grouped path (existing consumption, already works)
- The two paths are mutually exclusive based on sync mode

**Step 3: Run full test suite**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: PASS

**Step 4: Commit (if any cleanup changes)**

```
refactor: verify bulk/live sync consumption paths are complete
```

---

### Task 7: Add Integration Test

**Files:**

- Modify: `crates/indexer/tests/` (add test file or extend existing)

**Step 1: Write integration test for spore consumption in bulk sync**

Create a test that:

1. Sets up a test store
2. Creates mock parsed blocks with spore creation + consumption
3. Runs the precompute function
4. Verifies `consumed_spore` events are populated
5. Verifies `consumed_mnft` events are populated

```rust
#[test]
fn test_precompute_identifies_spore_consumption() {
    // Create input_cell_info with a spore type_code_hash
    // Run run_nft_precompute
    // Assert consumed_spore has an entry with the correct spore_id
}

#[test]
fn test_precompute_identifies_mnft_consumption() {
    // Create input_cell_info with a mNFT token type_code_hash
    // Run run_nft_precompute
    // Assert consumed_mnft has an entry with the correct token_id
}

#[test]
fn test_precompute_same_batch_spore_transfer_not_consumed() {
    // Create output with spore_id at tx_global_index 5
    // Create input consuming same spore_id at tx_global_index 3
    // The spore was re-created AFTER the consume → should NOT be in consumed_spore
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p ckbadger-indexer test_precompute_identifies`
Expected: PASS

**Step 3: Commit**

```
test: add integration tests for bulk sync NFT consumption identification
```

---

### Task 8: Final Validation

**Step 1: Run full workspace checks**

```bash
cargo check && cargo clippy && cargo test --lib
```

Expected: All pass

**Step 2: Verify the data flow**

Trace the full path:

1. Parser stage: `run_nft_precompute` → identifies consumed spore/mNFT from `input_cell_info`
2. Pipeline passes `PreParsedNftData` to writer stage
3. T6a thread: processes `consumed_spore` via `consume_spore()`
4. T6b thread: processes `consumed_mnft` via `consume_mnft_token_with_state()`
5. Both apply same-batch ordering checks

**Step 3: Document the change**

Update `docs/STORE_SCHEMA.md` or add a note to `CLAUDE.md` if needed (e.g., remove any mention of spore/mNFT consumption being skipped in bulk sync).

**Step 4: Final commit**

```
docs: update documentation for bulk sync NFT consumption support
```

---

## Summary

| Task | Description                               | Risk                          |
| ---- | ----------------------------------------- | ----------------------------- |
| 1    | Add event types + extend PreParsedNftData | Low — additive                |
| 2    | Add ordering helpers                      | Low — pure functions          |
| 3    | Extend run_nft_precompute                 | Medium — core logic change    |
| 4    | Process spore consumption in T6a          | Medium — writer thread change |
| 5    | Process mNFT consumption in T6b           | Medium — writer thread change |
| 6    | Verify live sync guards are correct       | Low — verification only       |
| 7    | Integration tests                         | Low — test-only               |
| 8    | Final validation                          | Low — verification only       |

**After implementation:** Delete RocksDB and re-sync from genesis.
