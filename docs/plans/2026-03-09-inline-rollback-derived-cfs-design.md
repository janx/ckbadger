# Inline Rollback for Derived CFs

## Problem

`rollback_to_block_with_tx_index_store` in `reorg_ops.rs` handles primary data (cells, headers, tx*index) inline, but punts on three derived CFs with full-table `rebuild*\*` functions that scan millions/billions of rows. This causes multi-minute startup stalls after unclean shutdown — even when nothing was actually rolled back (0 blocks, 0 txs, 0 cells affected).

## Approach

Accumulate deltas during existing cell and token_transfer rollback stages, then apply them to derived CFs in the same WriteBatch. This mirrors forward sync's `apply_address_balance_deltas` and `apply_script_usage_deltas` in `addresses.rs`.

## Design by CF

### 1. addr_balance (CF_ADDR_BALANCE)

During cell rollback stages (tx-context and fallback paths):

- Cell removed from live: `balance -= capacity, occupied -= occupied_capacity, live_cells_count -= 1`
- Cell restored to live: reverse signs

After cell processing, read current `AddressBalance` for touched lock_hashes, apply deltas, write back in same batch.

`txs_count`, `first_seen`, `last_activity` untouched — `addr_txs` is append-only and not modified during rollback.

### 2. script_info (CF_SCRIPT_INFO)

During cell rollback stages, for lock_code_hash and type_code_hash:

- Cell removed from live: `live_cells_count -= 1, live_capacity_sum -= capacity, live_occupied_capacity_sum -= occupied`
- Cell restored to live: reverse signs

`cells_count` (total) and non-live sums untouched — `cf_cells()` canonical entries are never deleted during rollback.

### 3. token_state (CF_TOKENS, CF_TOKEN_HOLDERS, CF_STATS_TOKEN)

During cell rollback stages, for cells with `udt_amount`:

- Cell removed from live: `(type_hash, lock_hash) holder_balance -= udt_amount`
- Cell restored to live: reverse signs

During token_transfer deletion (stage 8):

- Count deleted transfers per type_hash
- Track deleted hourly buckets per (type_hash, hour)

After stages, apply:

- Update `CF_TOKEN_HOLDERS` balances (delete if balance reaches 0)
- Update `TokenInfo.holders_count` and `TokenInfo.total_supply` from holder deltas
- Decrement `CF_STATS_TOKEN` transfer count and hourly entries

### 4. Removals

- `rebuild_addr_balances_from_live_cells_with_tx_index_store` from `address_ops.rs`
- `rebuild_script_infos_from_cells` from `stats_ops.rs`
- `rebuild_token_state_from_transfers` from `token_ops.rs`
- All calls in `reorg_ops.rs` post-commit rebuild block (~lines 1610-1647)
- Duplicate addr_balance rebuild call in `indexer.rs` (~line 939-951)

## Testing

- Existing rollback/reorg tests must pass
- Add rollback test verifying addr_balance, script_info, and token_holder correctness without rebuild calls
