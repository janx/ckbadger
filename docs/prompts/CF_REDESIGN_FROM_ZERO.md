# CF Redesign From Zero (API-Driven)

## Goal

- Rebuild `indexer/writer/CF` around one core rule: **single canonical cell payload + pointer indexes**.
- Optimize for bulk sync write throughput and shortest rebuild time.
- Keep API read semantics stable while moving storage internals to low-amplification layout.

## API Read Requirements (Grouped)

| API Domain      | Read Pattern                                                         | Required Data                                                     |
| --------------- | -------------------------------------------------------------------- | ----------------------------------------------------------------- |
| Blocks          | by number/hash, recent list, tx list in block                        | block header cache, tx index                                      |
| Transactions    | by hash, detail, lifecycle                                           | tx location, live/consumed cell lookup                            |
| Cells/Addresses | cell by outpoint, address live cells, addr tx history, addr balances | live set + cell payload + addr indexes                            |
| Tokens          | token meta, holders, transfers, activities, occupation chart         | token/meta CF + token holder index + stats                        |
| Scripts         | script usage and occupied capacity                                   | lock/type code indexes + script info + stats                      |
| Spore/NFT       | item, collection, holders, activities, charts                        | nft/spore CFs + collection indexes + activities                   |
| DAO             | deposit lifecycle, charts, issuance                                  | dao deposit CF + withdraw reverse index + issuance/stat snapshots |
| Statistics      | network/chart endpoints                                              | daily/hourly/epoch/script/token/nft/spore deltas and snapshots    |
| Search          | hash/address/script/token/spore resolution                           | hash indexes + entity meta CFs                                    |

## Ground-Zero CF Layout

### Layer 0: Canonical State + Pointer Indexes

| CF                  | Key                                 | Value               | Write Owner | Notes                                       |
| ------------------- | ----------------------------------- | ------------------- | ----------- | ------------------------------------------- |
| `cells`             | `outpoint`                          | `LiveCellInfo`      | indexer     | **Single source of truth for cell payload** |
| `live_cells`        | `outpoint`                          | empty               | indexer     | UTXO pointer set                            |
| `consumed_cells`    | `outpoint`                          | `ConsumedCellMeta`  | indexer     | consumed pointer + consume metadata         |
| `cell_by_lock`      | `lock_hash + block + outpoint`      | empty               | indexer     | live index                                  |
| `cell_by_type`      | `type_hash + block + outpoint`      | empty               | indexer     | live index                                  |
| `cell_by_lock_code` | `lock_code_hash + block + outpoint` | empty               | indexer     | live index                                  |
| `cell_by_type_code` | `type_code_hash + block + outpoint` | empty               | indexer     | live index                                  |
| `block_headers`     | `block_number`                      | `CachedBlockHeader` | indexer     | fast block/dao/timestamp lookup             |
| `block_hash_index`  | `block_hash`                        | `block_number`      | indexer     | reverse block lookup                        |
| `tx_index`          | `block + tx_idx`                    | `TxIndexEntry`      | indexer     | ordered tx lookup                           |
| `tx_hash_map`       | `tx_hash`                           | `block + tx_idx`    | indexer     | reverse tx lookup                           |

### Layer 1: Entity Metadata / Domain State

- `tokens`, `token_holders`, `token_transfers`
- `spore_data`, `spore_by_cluster`
- `nft_data`, `nft_by_collection`, `cluster_agg`, `nft_collection_agg`
- `dao_deposits`, `dao_by_withdraw_tx`, `block_issuance`
- `script_info`

### Layer 2: Time-Series / Activity / Aggregates

- `activities`, `nft_collection_activities`, `addr_txs`, `addr_balance`, `addr_daily_stats`
- `stats` (prefix namespaces: daily/hourly/epoch/script/token/spore/nft/dao/hodl/etc.)
- `sync_meta`

## Indexer Write Model

### Per output cell

1. `cells[outpoint] = LiveCellInfo` (append-only canonical payload)
2. `live_cells[outpoint] = empty`
3. write all live indexes (`cell_by_*`)

### Per input (consumption)

1. `consumed_cells[outpoint] = ConsumedCellMeta { consumed_at_block, consumed_by_tx }`
2. delete `live_cells[outpoint]`
3. delete live indexes (`cell_by_*`)

### Reorg

- Never delete canonical history payload from `cells`.
- Rollback only toggles pointer sets/indexes:
  - remove `live_cells` for cells created after rollback point
  - remove `consumed_cells` for consumptions after rollback point
  - restore `live_cells` pointer when cell existed before rollback point

## API Read Model

- `get_cell(outpoint)`:
  - check `live_cells` marker
  - read payload from `cells`
- `get_consumed_cell_info(outpoint)`:
  - read `consumed_cells` meta
  - read payload from `cells`
  - compose runtime `ConsumedCellInfo`
- list/scan endpoints (address/script/token charts):
  - scan pointer/index CFs
  - resolve payload from `cells`

## Why This Improves Bulk Sync

1. Removes duplicated full-cell payload in consumed path.
2. Converts hot-path `live/consumed` CF values to tiny records (marker/meta).
3. Shrinks write amplification during input-heavy blocks.
4. Keeps rebuild deterministic: if derived data is wrong, drop DB and replay from genesis.

## Migration Status

- Implemented in this refactor:
  - Added `cells` CF.
  - Migrated writer core cell lifecycle to canonical payload + marker/meta model.
  - Updated rollback and API/store read paths to resolve from `cells`.
- Remaining phase (optional next step):
  - Split `stats` super-CF into dedicated CFs for very hot prefixes if compaction pressure warrants.
