# On-Chain Token Info Extraction + Explorer API Import

**Date**: 2026-03-26
**Status**: Draft

## Problem

Token display metadata (name, symbol, decimals) in ckbadger comes exclusively from manually maintained TOML files in `docs/metadata/tokens/`. This has two gaps:

1. **Coverage**: Tokens with on-chain info cells (xUDT Unique Cell) are not auto-detected. New tokens require manual TOML additions.
2. **Lag**: The official CKB explorer has token info (via self-service submission and on-chain auto-detection) that ckbadger lacks until someone manually adds a TOML file.

## Solution

Two complementary paths:

- **Path 1 (CKB Native)**: Extract name/symbol/decimal from xUDT Unique Cells during indexer sync, write to CF_TOKENS when TOML labels are absent.
- **Path 2 (One-time script)**: Import published token metadata from the official CKB explorer API into `docs/metadata/tokens/` TOML files, covering tokens without on-chain info cells.

### Out of Scope

- Omiga inscription info cell parsing: format has no public specification; covered by Path 2 (API import) instead.
- icon_url / description import from explorer API: quality uncontrollable, not chain data.

## Design

### Path 1: xUDT Unique Cell Token Info Extraction

#### Data Format (xUDT Unique Cell)

```
[1 byte: decimal]
[1 byte: name_len] [name_len bytes: name (UTF-8)]
[1 byte: symbol_len] [symbol_len bytes: symbol (UTF-8)]
[tag-value pairs...]
  [4 bytes LE: tag] [4 bytes LE: data_len] [data_len bytes: value]
  TAG 1 = total_supply (16 bytes, u128 LE)
```

Unique Cells are identified by type_args length == 20 bytes with the Unique Cell code_hash (`0x2c8c11c985da60b0a330c61a85507416d6382c130ba67f0c47ab071e00aec628` mainnet).

#### New Parser

Replace `parse_token_info_total_supply(data) -> Option<i128>` with:

```rust
pub(crate) struct UniqueTokenInfo {
    pub decimal: u8,
    pub name: String,      // may be empty
    pub symbol: String,     // may be empty
    pub total_supply: Option<i128>,
}

pub(crate) fn parse_unique_cell_token_info(data: &[u8]) -> Option<UniqueTokenInfo>
```

Parses the same byte layout but extracts all fields. The old function becomes a thin wrapper or is inlined at call sites.

**Location**: `crates/indexer/src/sync/token_helpers.rs`

#### New Collector

```rust
pub(crate) fn collect_unique_cell_token_info(
    cells: &[ParsedCell],
) -> HashMap<Vec<u8>, UniqueTokenInfo>   // keyed by type_args (20 bytes)
```

Parallel to existing `collect_unique_cell_total_supply_by_type_args`, but returns full info structs.

#### Write Path (UDT Writer)

In the UDT writer's batch processing, after collecting unique cell token info for the batch:

1. For each xUDT cell in the batch, resolve its extension scripts to find associated Unique Cell type_args.
2. Look up the Unique Cell info from the collector.
3. Read existing `TokenInfo` from CF_TOKENS.
4. **Merge rule**: If `existing.name.is_some()`, skip (TOML label takes priority). Otherwise, set name/symbol/decimals from the Unique Cell data.
5. Write merged `TokenInfo` back to CF_TOKENS.

This runs inside the existing token update path in `crates/indexer/src/db/writer/udt.rs`.

#### Merge Priority (high to low)

| Priority | Source | When |
|----------|--------|------|
| 1 | TOML label (`label_import`) | Startup, one-time |
| 2 | On-chain Unique Cell | Sync time, conditional |
| 3 | Empty | Default |

A TOML file always wins. To correct wrong on-chain info, add a TOML entry.

### Path 2: Explorer API Import Script

#### Script

`scripts/import_explorer_tokens.py` — standalone Python script, not compiled into binary.

#### Process

1. Paginate through `GET https://mainnet-api.explorer.nervos.org/api/v1/udts?page={n}&page_size=100` with `Accept: application/vnd.api+json` header.
2. Filter: `published == true` AND `symbol` non-empty AND `full_name` non-empty.
3. For each token, compute a filename: `{symbol_lowercase}-{args_hex_prefix_8chars}.toml`.
4. Check if `docs/metadata/tokens/` already has a TOML file with matching `(code_hash, hash_type, args)`. If yes, skip.
5. Generate TOML:
   ```toml
   name = "{full_name}"
   symbol = "{symbol}"
   decimals = {decimal}
   standard = "{udt_type}"  # xudt, sudt, etc.

   [mainnet]
   code_hash = "{code_hash}"
   hash_type = "{hash_type}"
   args = "{args}"
   ```
6. Write to `docs/metadata/tokens/{filename}`.

#### What is NOT imported

- `icon_file`, `description`, `email`, `operator_website` — off-chain community data, quality varies.
- Tokens with `published: false` — no curated metadata.
- Tokens with empty symbol or name — no display value.

#### Deduplication

The script scans all existing TOML files first, builds a set of `(code_hash, hash_type, args)` tuples, and skips any API token that already has a matching entry. This prevents overwriting manually curated files.

## Re-sync

Path 1 requires a re-sync from genesis to populate historical Unique Cell info. Not urgent — Path 2 (API import) provides immediate coverage for the gap.

## Testing

### Path 1 Tests

- **`parse_unique_cell_token_info`** unit tests (in `token_helpers.rs`):
  - Valid data with name, symbol, decimal, and total_supply tag
  - Valid data with no tags (total_supply = None)
  - Truncated data (returns None)
  - Empty name/symbol (returns empty strings, not None)
  - Non-UTF-8 name/symbol bytes (returns None or lossy? — fail fast: return None)
- **UDT writer merge logic** test:
  - Token with TOML label: Unique Cell data does NOT overwrite
  - Token without TOML label: Unique Cell data fills name/symbol/decimals
  - Token with partial TOML (e.g., name only): behavior TBD — recommend all-or-nothing from TOML

### Path 2 Tests

- Manual verification: run script, inspect generated TOML files, diff against existing metadata.
- No automated tests needed for a one-time import script.

## Files Changed

| File | Change |
|------|--------|
| `crates/indexer/src/sync/token_helpers.rs` | New `UniqueTokenInfo` struct, `parse_unique_cell_token_info`, `collect_unique_cell_token_info`; deprecate/inline `parse_token_info_total_supply` |
| `crates/indexer/src/db/writer/udt.rs` | Add merge logic: read existing TokenInfo, conditionally write Unique Cell name/symbol/decimals |
| `scripts/import_explorer_tokens.py` | New file: one-time API import script |
| `docs/metadata/tokens/*.toml` | New files generated by import script |
