# On-Chain Token Info Extraction + Explorer API Import

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-extract token name/symbol/decimals from xUDT Unique Cells during sync, and import published token metadata from the official CKB explorer API.

**Architecture:** Extend the existing Unique Cell parsing in `token_helpers.rs` to return name/symbol/decimal (currently only returns total_supply). Write this info to CF_TOKENS during both live-sync and bulk-build, with label_import TOML data taking priority via unconditional overwrite at startup. A one-time Python script imports published tokens from the official explorer API as TOML files.

**Tech Stack:** Rust (indexer), Python 3 (import script)

**Spec:** `docs/superpowers/specs/2026-03-26-onchain-token-info-extraction-design.md`

---

### Task 1: New parser `parse_unique_cell_token_info`

**Files:**
- Modify: `crates/indexer/src/sync/token_helpers.rs:333-394`

- [ ] **Step 1: Add `UniqueTokenInfo` struct and `parse_unique_cell_token_info` function**

Add after the existing constants (line 46):

```rust
/// Token metadata extracted from an xUDT Unique Cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UniqueTokenInfo {
    pub decimal: u8,
    pub name: String,
    pub symbol: String,
    pub total_supply: Option<i128>,
}

/// Parse all fields from a Unique Cell's data:
/// `[1B decimal][1B name_len][name UTF-8][1B symbol_len][symbol UTF-8][tag-value pairs...]`
///
/// Returns `None` if data is malformed or name/symbol are not valid UTF-8 (fail fast).
pub(crate) fn parse_unique_cell_token_info(data: &[u8]) -> Option<UniqueTokenInfo> {
    if data.len() < 3 {
        return None;
    }

    let mut index = 0usize;
    let decimal = *data.get(index)?;
    index += 1;

    let name_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + name_len + 1 {
        return None;
    }
    let name = std::str::from_utf8(data.get(index..index + name_len)?).ok()?.to_string();
    index += name_len;

    let symbol_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + symbol_len {
        return None;
    }
    let symbol = std::str::from_utf8(data.get(index..index + symbol_len)?).ok()?.to_string();
    index += symbol_len;

    let mut total_supply = None;
    while index + 8 <= data.len() {
        let tag = u32::from_le_bytes(data[index..index + 4].try_into().ok()?);
        index += 4;
        let data_len = u32::from_le_bytes(data[index..index + 4].try_into().ok()?) as usize;
        index += 4;
        if data.len() < index + data_len {
            return None;
        }
        let value = &data[index..index + data_len];
        if tag == TOKEN_INFO_TAG_TOTAL_SUPPLY && data_len == TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN {
            let raw = u128::from_le_bytes(value.try_into().ok()?);
            if raw <= i128::MAX as u128 {
                total_supply = Some(raw as i128);
            }
        }
        index += data_len;
    }

    Some(UniqueTokenInfo {
        decimal,
        name,
        symbol,
        total_supply,
    })
}
```

- [ ] **Step 2: Rewrite `parse_token_info_total_supply` as thin wrapper**

Replace the existing `parse_token_info_total_supply` function (lines 333-375) with:

```rust
pub(crate) fn parse_token_info_total_supply(data: &[u8]) -> Option<i128> {
    parse_unique_cell_token_info(data)?.total_supply
}
```

Note: `collect_unique_cell_total_supply_by_type_args` is intentionally left unchanged — it calls `parse_token_info_total_supply` which now delegates to the new parser.

- [ ] **Step 3: Write unit tests for `parse_unique_cell_token_info`**

Add in the `#[cfg(test)]` module:

```rust
#[test]
fn test_parse_unique_cell_token_info_with_all_fields() {
    let data = build_token_info_data(42_000);
    let info = parse_unique_cell_token_info(&data).unwrap();
    assert_eq!(info.decimal, 8);
    assert_eq!(info.name, "Token");
    assert_eq!(info.symbol, "TKN");
    assert_eq!(info.total_supply, Some(42_000));
}

#[test]
fn test_parse_unique_cell_token_info_without_tags() {
    let mut data = Vec::new();
    data.push(18); // decimal
    data.push(4);  // name len
    data.extend_from_slice(b"Test");
    data.push(2);  // symbol len
    data.extend_from_slice(b"TS");
    let info = parse_unique_cell_token_info(&data).unwrap();
    assert_eq!(info.decimal, 18);
    assert_eq!(info.name, "Test");
    assert_eq!(info.symbol, "TS");
    assert_eq!(info.total_supply, None);
}

#[test]
fn test_parse_unique_cell_token_info_empty_name_symbol() {
    let mut data = Vec::new();
    data.push(0); // decimal
    data.push(0); // name len (empty)
    data.push(0); // symbol len (empty)
    let info = parse_unique_cell_token_info(&data).unwrap();
    assert_eq!(info.name, "");
    assert_eq!(info.symbol, "");
}

#[test]
fn test_parse_unique_cell_token_info_truncated_data() {
    assert!(parse_unique_cell_token_info(&[]).is_none());
    assert!(parse_unique_cell_token_info(&[8]).is_none());
    assert!(parse_unique_cell_token_info(&[8, 5]).is_none()); // name_len=5 but no name bytes
}

#[test]
fn test_parse_unique_cell_token_info_invalid_utf8() {
    let mut data = Vec::new();
    data.push(8);
    data.push(2);
    data.extend_from_slice(&[0xFF, 0xFE]); // invalid UTF-8
    data.push(2);
    data.extend_from_slice(b"OK");
    assert!(parse_unique_cell_token_info(&data).is_none());
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer parse_unique_cell_token_info`
Expected: All 5 new tests PASS. Existing `parse_token_info_total_supply` tests also PASS (wrapper delegates correctly).

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/sync/token_helpers.rs
git commit -m "feat(indexer): add parse_unique_cell_token_info for name/symbol/decimal extraction"
```

---

### Task 2: New collector `collect_unique_cell_token_info`

**Files:**
- Modify: `crates/indexer/src/sync/token_helpers.rs:377-394`

- [ ] **Step 1: Add `collect_unique_cell_token_info` function**

Add after `collect_unique_cell_total_supply_by_type_args`:

```rust
/// Collect full token info from all Unique Cell outputs in the given cells.
/// Returns a map from Unique Cell type_args (20 bytes) to parsed token info.
pub(crate) fn collect_unique_cell_token_info(
    cells: &[crate::parser::cell::ParsedCell],
) -> HashMap<Vec<u8>, UniqueTokenInfo> {
    let mut infos = HashMap::new();
    for cell in cells {
        let Some(type_args) = cell.type_args.as_ref() else {
            continue;
        };
        if type_args.len() != UNIQUE_TYPE_ARGS_LEN {
            continue;
        }
        let Some(info) = parse_unique_cell_token_info(&cell.data) else {
            continue;
        };
        infos.insert(type_args.clone(), info);
    }
    infos
}
```

- [ ] **Step 2: Add `collect_token_onchain_info` to resolve Unique Cell → token type_hash**

Add a new function that parallels `collect_token_max_supply_observations` but collects name/symbol/decimal:

```rust
/// Resolve Unique Cell token info to xUDT token type_hashes.
/// For each xUDT output cell with extension scripts pointing to a Unique Cell,
/// maps the xUDT's type_script_hash to the Unique Cell's token info.
pub(crate) fn collect_token_onchain_info(
    all_tx_data: &[TxData],
) -> HashMap<Vec<u8>, UniqueTokenInfo> {
    let mut result = HashMap::new();

    for tx_data in all_tx_data {
        let unique_infos = collect_unique_cell_token_info(&tx_data.cells);
        if unique_infos.is_empty() {
            continue;
        }

        for cell in &tx_data.cells {
            let Some(type_code_hash) = cell.type_code_hash.as_ref() else {
                continue;
            };
            let Some(type_hash_type) = cell.type_hash_type else {
                continue;
            };
            if !matches!(
                crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type),
                Some(crate::parser::udt::UdtStandard::Xudt)
            ) {
                continue;
            }

            let Some(type_args) = cell.type_args.as_ref() else {
                continue;
            };
            let Some(token_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };

            let Some(extension_scripts) =
                extract_xudt_extension_scripts(type_args, &tx_data.witnesses)
            else {
                continue;
            };

            for extension in extension_scripts {
                if extension.args.len() != UNIQUE_TYPE_ARGS_LEN {
                    continue;
                }
                if let Some(info) = unique_infos.get(&extension.args) {
                    result.insert(token_type_hash.clone(), info.clone());
                }
            }
        }
    }

    result
}
```

- [ ] **Step 3: Write test for `collect_token_onchain_info`**

```rust
#[test]
fn test_collect_token_onchain_info_resolves_unique_cell_to_xudt() {
    let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
    let token_type_hash = [0x91; 32];
    let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
    let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

    let unique_cell = dummy_unique_token_info_cell(unique_type_args, 42_000);
    let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
    let tx = dummy_tx_data(
        [0xF0; 32],
        false,
        vec![],
        vec![unique_cell, xudt_cell],
        vec![],
        vec![],
    );

    let infos = collect_token_onchain_info(&[tx]);
    let info = infos.get(token_type_hash.as_slice()).unwrap();
    assert_eq!(info.name, "Token");
    assert_eq!(info.symbol, "TKN");
    assert_eq!(info.decimal, 8);
    assert_eq!(info.total_supply, Some(42_000));
}

#[test]
fn test_collect_token_onchain_info_empty_when_no_unique_cell() {
    let token_type_hash = [0x92; 32];
    let type_args = vec![0x01; 36]; // just owner_lock_hash + flags, no extensions
    let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
    let tx = dummy_tx_data([0xF1; 32], false, vec![], vec![xudt_cell], vec![], vec![]);

    let infos = collect_token_onchain_info(&[tx]);
    assert!(infos.is_empty());
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer collect_token_onchain_info`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/sync/token_helpers.rs
git commit -m "feat(indexer): add collect_token_onchain_info for Unique Cell → token resolution"
```

---

### Task 3: Live-sync writer — apply on-chain token info

**Files:**
- Modify: `crates/indexer/src/db/writer/udt.rs:240-270`
- Modify: `crates/indexer/src/sync/batch.rs:1886-1934`

- [ ] **Step 1: Add `onchain_token_info` parameter to `process_udt_transfers_batch_with_state`**

In `crates/indexer/src/db/writer/udt.rs`, update both function signatures:

```rust
pub fn process_udt_transfers_batch(
    &self,
    transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
    max_supply_observations: &HashMap<Vec<u8>, i128>,
    onchain_token_info: &HashMap<Vec<u8>, crate::sync::token_helpers::UniqueTokenInfo>,
    block_timestamps: &HashMap<i64, i64>,
    batch: &mut StoreBatch,
) -> Result<()> {
    let mut state = self.new_udt_batch_state();
    self.process_udt_transfers_batch_with_state(
        transfers,
        max_supply_observations,
        onchain_token_info,
        block_timestamps,
        batch,
        &mut state,
    )
}

pub(crate) fn process_udt_transfers_batch_with_state(
    &self,
    transfers: &[(&ParsedUdtTransfer, &[u8], i64)],
    max_supply_observations: &HashMap<Vec<u8>, i128>,
    onchain_token_info: &HashMap<Vec<u8>, crate::sync::token_helpers::UniqueTokenInfo>,
    block_timestamps: &HashMap<i64, i64>,
    batch: &mut StoreBatch,
    state: &mut UdtBatchState,
) -> Result<()> {
```

- [ ] **Step 2: Add on-chain info application logic in the writer**

After the existing `apply_observed_max_supply` call (around line 367), add on-chain info application:

```rust
// Apply on-chain token info from Unique Cells (name/symbol/decimals)
apply_onchain_token_info(type_hash, &mut updated, onchain_token_info);
```

Add the helper function:

```rust
fn apply_onchain_token_info(
    type_hash: &[u8],
    info: &mut TokenInfo,
    onchain_info: &HashMap<Vec<u8>, crate::sync::token_helpers::UniqueTokenInfo>,
) {
    let Some(onchain) = onchain_info.get(type_hash) else {
        return;
    };
    // Always write on-chain data. label_import unconditionally overwrites at startup,
    // so TOML priority is maintained by execution order, not conditional checks.
    if !onchain.name.is_empty() {
        info.name = Some(onchain.name.clone());
    }
    if !onchain.symbol.is_empty() {
        info.symbol = Some(onchain.symbol.clone());
    }
    info.decimals = Some(onchain.decimal as i32);
}
```

Also handle the empty-transfers path (lines 265-278). The existing code only iterates `max_supply_observations.keys()`. Add a separate loop for `onchain_token_info.keys()` to handle tokens that have Unique Cells but no transfers in this batch:

```rust
// Apply on-chain token info for tokens not touched by transfers
for type_hash in onchain_token_info.keys() {
    if let Some(mut info) = self.store.get_token(type_hash)? {
        apply_onchain_token_info(type_hash, &mut info, onchain_token_info);
        batch.put_token(type_hash, &info);
    }
}
```

Place this after the existing max_supply loop (lines 268-276).

- [ ] **Step 3: Update call site in `batch.rs`**

In `crates/indexer/src/sync/batch.rs` around line 1887, add collection and pass to writer:

```rust
let max_supply_observations = collect_token_max_supply_observations(&all_tx_data);
let onchain_token_info = collect_token_onchain_info(&all_tx_data);
```

Update the call at line 1928:

```rust
self.writer.process_udt_transfers_batch_with_state(
    &transfer_refs,
    &max_supply_observations,
    &onchain_token_info,
    &block_timestamps,
    &mut data_batch,
    &mut udt_state,
)?;
```

- [ ] **Step 4: Fix any other call sites of `process_udt_transfers_batch`**

Search for all callers of `process_udt_transfers_batch` and `process_udt_transfers_batch_with_state`. Pass `&HashMap::new()` for `onchain_token_info` in any test or secondary call sites.

Run: `cargo check -p ckbadger-indexer`
Expected: PASS (no compile errors)

- [ ] **Step 5: Run existing tests**

Run: `cargo test -p ckbadger-indexer`
Expected: All existing tests PASS (no regressions)

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/db/writer/udt.rs crates/indexer/src/sync/batch.rs
git commit -m "feat(indexer): apply on-chain token info from Unique Cells in live-sync writer"
```

---

### Task 4: Bulk-build reducer — collect and apply Unique Cell info

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/owners/token.rs:24-68, 70-131, 180-260`

- [ ] **Step 1: Add `unique_cell_info` and `token_onchain_info` fields to `TokenOwner`**

```rust
#[derive(Debug, Default)]
pub(crate) struct TokenOwner {
    tokens: FxHashMap<Vec<u8>, TokenAccum>,
    /// On-chain max_supply observations collected from omnilock supply info cells.
    max_supply_observations: FxHashMap<Vec<u8>, i128>,
    /// On-chain token info collected from xUDT Unique Cells (keyed by unique type_args, 20 bytes).
    unique_cell_info: FxHashMap<Vec<u8>, crate::sync::token_helpers::UniqueTokenInfo>,
    /// Resolved on-chain token info (keyed by token type_hash).
    token_onchain_info: FxHashMap<Vec<u8>, crate::sync::token_helpers::UniqueTokenInfo>,
}
```

Update `estimated_bytes` to include the new fields:

```rust
+ crate::sync::bulk_build::accounting::hash_map_bytes(
    &self.unique_cell_info,
    |k, v| {
        crate::sync::bulk_build::accounting::bytes_vec_bytes(k)
            + 1 + v.name.len() as u64 + v.symbol.len() as u64 + 17
    },
)
+ crate::sync::bulk_build::accounting::hash_map_bytes(
    &self.token_onchain_info,
    |k, v| {
        crate::sync::bulk_build::accounting::bytes_vec_bytes(k)
            + 1 + v.name.len() as u64 + v.symbol.len() as u64 + 17
    },
)
```

- [ ] **Step 2: Add `observe_unique_cell_from_output` method**

```rust
fn observe_unique_cell_from_output(&mut self, cell: &CellFacts, ctx: &ReducerContext<'_>) {
    let Some(type_args_id) = cell.type_args_id else {
        return;
    };
    let type_args = ctx.resolve_identity(type_args_id);
    if type_args.len() != crate::sync::token_helpers::UNIQUE_TYPE_ARGS_LEN {
        return;
    }
    let Some(info) = crate::sync::token_helpers::parse_unique_cell_token_info(&cell.data) else {
        return;
    };
    self.unique_cell_info.insert(type_args.to_vec(), info);
}
```

- [ ] **Step 3: Add `resolve_unique_cell_for_xudt` method**

After processing all outputs in a tx, for each xUDT output, check if its extension scripts (args-only, since witnesses aren't available in bulk-build) reference a known Unique Cell:

```rust
fn resolve_unique_cell_for_xudt(&mut self, cell: &CellFacts, ctx: &ReducerContext<'_>) {
    let Some(type_code_hash_id) = cell.type_code_hash_id else {
        return;
    };
    let Some(type_hash_type) = cell.type_hash_type else {
        return;
    };
    let type_code_hash = ctx.resolve_identity(type_code_hash_id);
    if !matches!(
        crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type as i16),
        Some(crate::parser::udt::UdtStandard::Xudt)
    ) {
        return;
    }
    let Some(type_args_id) = cell.type_args_id else {
        return;
    };
    let type_args = ctx.resolve_identity(type_args_id);
    let Some(type_hash_id) = cell.type_script_hash_id else {
        return;
    };
    let token_type_hash = ctx.resolve_identity(type_hash_id);

    // Pass empty witnesses — bulk-build has no witnesses, so only extension_in_args (0x1) is resolved.
    // extension_in_witness (0x2) returns None with empty slice, which is acceptable.
    let Some(extensions) =
        crate::sync::token_helpers::extract_xudt_extension_scripts(type_args, &[])
    else {
        return;
    };

    for ext in extensions {
        if ext.args.len() != crate::sync::token_helpers::UNIQUE_TYPE_ARGS_LEN {
            continue;
        }
        if let Some(info) = self.unique_cell_info.get(&ext.args) {
            self.token_onchain_info
                .insert(token_type_hash.to_vec(), info.clone());
        }
    }
}
```

- [ ] **Step 4: Call new methods in `apply_tx`**

In `apply_tx`, after the existing `observe_max_supply_from_output` loop (line 109-112), add:

```rust
// Collect unique cell info from outputs
for cell in tx.cells.iter() {
    self.observe_unique_cell_from_output(cell, ctx);
}
// Resolve unique cell → token associations
for cell in tx.cells.iter() {
    self.resolve_unique_cell_for_xudt(cell, ctx);
}
```

- [ ] **Step 5: Apply on-chain info in `materialize_final` and fix label preservation priority**

In `materialize_final`, after the max_supply application (line 200-203) and BEFORE the label preservation block (line 206), add:

```rust
// Apply on-chain token info from Unique Cells
if let Some(onchain) = self.token_onchain_info.get(type_hash) {
    if !onchain.name.is_empty() {
        info.name = Some(onchain.name.clone());
    }
    if !onchain.symbol.is_empty() {
        info.symbol = Some(onchain.symbol.clone());
    }
    info.decimals = Some(onchain.decimal as i32);
}
```

**CRITICAL:** The existing label preservation block (lines 206-225) uses `if info.name.is_none()` which would skip overwriting on-chain data with TOML data. Change the display fields to unconditionally overwrite from store when the store has values (TOML data was written to store by label_import at startup):

```rust
if let Some(existing) = existing_tokens.get(type_hash) {
    // Display fields: store values unconditionally win (includes TOML label data)
    if existing.name.is_some() {
        info.name = existing.name.clone();
    }
    if existing.symbol.is_some() {
        info.symbol = existing.symbol.clone();
    }
    if existing.decimals.is_some() {
        info.decimals = existing.decimals;
    }
    if existing.icon_url.is_some() {
        info.icon_url = existing.icon_url.clone();
    }
    if existing.description.is_some() {
        info.description = existing.description.clone();
    }
    // max_supply: only fill gaps (on-chain observations are canonical)
    if info.max_supply.is_none() {
        info.max_supply = existing.max_supply;
    }
}
```

This replaces the existing `if info.X.is_none()` checks for display fields with `if existing.X.is_some()` checks, ensuring TOML label data always wins over on-chain data.

- [ ] **Step 6: Run tests**

Run: `cargo test -p ckbadger-indexer`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/owners/token.rs crates/indexer/src/sync/token_helpers.rs
git commit -m "feat(indexer): collect Unique Cell token info in bulk-build reducer"
```

---

### Task 5: Explorer API import script

**Files:**
- Create: `scripts/import_explorer_tokens.py`

- [ ] **Step 1: Write the import script**

```python
#!/usr/bin/env python3
"""
One-time script to import published token metadata from the official CKB explorer API
into docs/metadata/tokens/ as TOML files.

Usage: python3 scripts/import_explorer_tokens.py [--dry-run]
"""

import json
import os
import re
import sys
import time
import urllib.request

EXPLORER_API = "https://mainnet-api.explorer.nervos.org"
METADATA_DIR = os.path.join(os.path.dirname(__file__), "..", "docs", "metadata", "tokens")
PAGE_SIZE = 100
ACCEPT_HEADER = "application/vnd.api+json"

# Map explorer udt_type to ckbadger standard
UDT_TYPE_MAP = {
    "sudt": "sudt",
    "xudt": "xudt",
    "xudt_compatible": "xudt",
    "omiga_inscription": "xudt",
}


def fetch_page(page: int, udt_type: str = "xudt") -> dict:
    url = f"{EXPLORER_API}/api/v1/udts?page={page}&page_size={PAGE_SIZE}&type_hash=&udt_type={udt_type}"
    req = urllib.request.Request(url, headers={"Accept": ACCEPT_HEADER})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode())


def load_existing_tokens() -> set:
    """Load (code_hash, hash_type, args) tuples from existing TOML files."""
    existing = set()
    if not os.path.isdir(METADATA_DIR):
        return existing
    for fname in os.listdir(METADATA_DIR):
        if not fname.endswith(".toml"):
            continue
        path = os.path.join(METADATA_DIR, fname)
        code_hash = hash_type = args = None
        in_mainnet = False
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line == "[mainnet]":
                    in_mainnet = True
                elif line.startswith("[") and line != "[mainnet]":
                    in_mainnet = False
                elif in_mainnet:
                    if line.startswith("code_hash"):
                        code_hash = line.split("=", 1)[1].strip().strip('"')
                    elif line.startswith("hash_type"):
                        hash_type = line.split("=", 1)[1].strip().strip('"')
                    elif line.startswith("args"):
                        args = line.split("=", 1)[1].strip().strip('"')
        if code_hash and hash_type and args:
            existing.add((code_hash, hash_type, args))
    return existing


def make_filename(symbol: str, args: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", symbol.lower()).strip("-")
    if not slug:
        slug = "unknown"
    args_prefix = args[2:10] if args.startswith("0x") else args[:8]
    return f"{slug}-{args_prefix}.toml"


def generate_toml(token: dict) -> str:
    attrs = token["attributes"]
    name = attrs.get("full_name") or ""
    symbol = attrs.get("symbol") or ""
    decimal = attrs.get("decimal") or "0"
    udt_type = attrs.get("udt_type") or "xudt"
    standard = UDT_TYPE_MAP.get(udt_type, "xudt")

    ts = attrs.get("type_script") or {}
    code_hash = ts.get("code_hash", "")
    hash_type = ts.get("hash_type", "")
    args = ts.get("args", "")

    lines = [
        f'name = "{name}"',
        f'symbol = "{symbol}"',
        f"decimals = {decimal}",
        f'standard = "{standard}"',
        "",
        "[mainnet]",
        f'code_hash = "{code_hash}"',
        f'hash_type = "{hash_type}"',
        f'args = "{args}"',
        "",
    ]
    return "\n".join(lines)


def main():
    dry_run = "--dry-run" in sys.argv
    existing = load_existing_tokens()
    print(f"Found {len(existing)} existing token TOML files")

    imported = 0
    skipped_existing = 0
    skipped_empty = 0

    for udt_type in ["xudt", "sudt"]:
        page = 1
        while True:
            print(f"Fetching {udt_type} page {page}...")
            try:
                data = fetch_page(page, udt_type)
            except Exception as e:
                print(f"  Error fetching page {page}: {e}")
                break

            tokens = data.get("data", [])
            if not tokens:
                break

            for token in tokens:
                attrs = token["attributes"]

                if not attrs.get("published"):
                    continue

                symbol = (attrs.get("symbol") or "").strip()
                name = (attrs.get("full_name") or "").strip()
                if not symbol or not name:
                    skipped_empty += 1
                    continue

                ts = attrs.get("type_script") or {}
                code_hash = ts.get("code_hash", "")
                hash_type = ts.get("hash_type", "")
                args = ts.get("args", "")

                if not code_hash or not args:
                    skipped_empty += 1
                    continue

                if (code_hash, hash_type, args) in existing:
                    skipped_existing += 1
                    continue

                filename = make_filename(symbol, args)
                filepath = os.path.join(METADATA_DIR, filename)

                # Avoid overwriting if filename collision
                if os.path.exists(filepath):
                    skipped_existing += 1
                    continue

                toml_content = generate_toml(token)
                if dry_run:
                    print(f"  [dry-run] Would create: {filename}")
                else:
                    with open(filepath, "w") as f:
                        f.write(toml_content)
                    print(f"  Created: {filename}")
                imported += 1
                existing.add((code_hash, hash_type, args))

            meta = data.get("meta", {})
            total_pages = meta.get("total_pages", 1)
            if page >= total_pages:
                break
            page += 1
            time.sleep(0.5)  # Rate limit

    print(f"\nDone: {imported} imported, {skipped_existing} skipped (existing), {skipped_empty} skipped (empty)")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Test with dry-run**

Run: `python3 scripts/import_explorer_tokens.py --dry-run`
Expected: Lists tokens that would be imported, no files created.

- [ ] **Step 3: Commit script**

```bash
git add scripts/import_explorer_tokens.py
git commit -m "feat: add one-time script to import token metadata from CKB explorer API"
```

---

### Task 6: Run import script and commit new TOML files

**Files:**
- Create: `docs/metadata/tokens/*.toml` (new files from import)

- [ ] **Step 1: Run the import script**

Run: `python3 scripts/import_explorer_tokens.py`
Expected: Creates new TOML files in `docs/metadata/tokens/`.

- [ ] **Step 2: Verify generated files**

Spot-check a few generated TOML files:
- Fields are present and correctly formatted
- `decimals` is a number, not a string
- `standard` is "xudt" or "sudt"
- `code_hash` starts with "0x"

- [ ] **Step 3: Build to verify TOML files compile**

Run: `cargo build -p ckbadger-indexer`
Expected: Build succeeds (TOML files are bundled at compile time via `build.rs`).

- [ ] **Step 4: Commit new TOML files**

```bash
git add docs/metadata/tokens/
git commit -m "feat: import published token metadata from official CKB explorer"
```
