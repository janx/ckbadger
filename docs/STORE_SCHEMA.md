# ckbadger-store Column Families

Two physical RocksDB instances with distinct write semantics:

- **Default store** (`CKBADGER_DATA_PATH`) — mutable state: indices, aggregates, sync metadata. Supports insert, update, delete, and range-delete on reorg.
- **Append store** (`{CKBADGER_DATA_PATH}-append`) — immutable history: cells, tx/block metadata, activities, asset indices. Insert-only, never deleted.

The indexer opens both stores read-write; the API opens both in secondary (read-only) mode.

## Default Store (25 canonical CFs)

### Core State

| CF                        | Key                                         | Value                                           | Purpose                                            |
| ------------------------- | ------------------------------------------- | ----------------------------------------------- | -------------------------------------------------- |
| `live_cells`              | outpoint (34B)                              | empty                                           | Liveness marker for unspent cells                  |
| `live_cells_by_lock`      | lock_hash(32) + block_num(8) + outpoint(34) | empty                                           | Cell index by lock script hash                     |
| `live_cells_by_type`      | type_hash(32) + block_num(8) + outpoint(34) | empty                                           | Cell index by type script hash                     |
| `live_cells_by_lock_code` | code_hash(32) + block_num(8) + outpoint(34) | empty                                           | Cell index by lock code_hash                       |
| `live_cells_by_type_code` | code_hash(32) + block_num(8) + outpoint(34) | empty                                           | Cell index by type code_hash                       |
| `consumed_cells`          | outpoint (34B)                              | consumed_at_block(8) + consumed_by_tx(32) = 40B | Consumption metadata (cell data in append `cells`) |
| `tx_index`                | block_num(8) + tx_index(4)                  | tx_hash (32B)                                   | Block→tx ordering index                            |
| `block_index`             | block_number (8B)                           | block_hash (32B)                                | Canonical block number→hash mapping                |

### Assets

| CF                       | Key                                          | Value                              | Purpose                          |
| ------------------------ | -------------------------------------------- | ---------------------------------- | -------------------------------- |
| `asset_meta`             | script_hash (32B)                            | AssetMeta (bincode)                | FT/NFT asset metadata            |
| `nft_item_meta`          | nft_type(1) + nft_id(32)                     | NftItemMeta (bincode)              | Unified NFT item metadata        |
| `nft_outpoints`          | outpoint (34B)                               | nft_type(1) + nft_id(32) = 33B     | Outpoint→NFT reverse lookup      |
| `nft_item_by_collection` | nft_type(1) + collection_id(32) + nft_id(32) | empty                              | NFT index by collection          |
| `ft_outpoints`           | outpoint (34B)                               | ft_type(1) + script_hash(32) = 33B | Outpoint→FT reverse lookup       |
| `dao_deposits`           | outpoint (34B)                               | DaoDepositCacheEntry (bincode)     | DAO deposit lifecycle cache      |
| `dao_withdraw_index`     | withdraw_tx_hash (32B)                       | deposit outpoint (34B)             | Reverse lookup: withdraw→deposit |
| `block_issuance`         | block_number (8B)                            | BlockIssuance (bincode)            | Per-block secondary issuance     |

### Aggregates & Indices

| CF                          | Key                                            | Value                        | Purpose                                               |
| --------------------------- | ---------------------------------------------- | ---------------------------- | ----------------------------------------------------- |
| `addr_stats`                | lock_hash (32B)                                | AddrStats (bincode)          | Address balance, cell counts (all addresses)          |
| `ft_stats`                  | script_hash (32B)                              | FtStats (bincode)            | FT supply, holder count                               |
| `ft_holders`                | script_hash(32) + lock_hash(32)                | amount (16B u128)            | FT holder balances (hot tokens)                       |
| `nft_collection_stats`      | nft_type(1) + collection_id(32)                | NftCollectionStats (bincode) | Collection aggregate stats                            |
| `addr_txs`                  | lock_hash(32) + block_num(8) + tx_index(4)     | empty                        | Address transaction history                           |
| `addr_activities`           | lock_hash(32) + activity_id(14)                | empty                        | Address activity index                                |
| `nft_collection_activities` | collection_id(32) + activity_id(14)            | empty                        | NFT collection activity index                         |
| `ft_activities`             | ft_type(1) + script_hash(32) + activity_id(14) | empty                        | FT activity index                                     |
| `stats`                     | prefixed keys                                  | various                      | Daily/hourly/chart aggregates, sync meta, script info |

### Legacy CFs (kept for migration, unused in new code)

`block_headers`, `block_hash_index`, `tx_hash_map`, `spore_data`, `spore_by_cluster`, `nft_data`, `cluster_agg`, `nft_collection_agg`, `script_info`, `sync_meta`, `token_transfers`, `addr_daily_stats`

## Append Store (6 CFs)

| CF               | Key                                         | Value               | Purpose                                 |
| ---------------- | ------------------------------------------- | ------------------- | --------------------------------------- |
| `cells`          | outpoint (34B)                              | CellInfo (bincode)  | SSOT for all cell data. Never deleted.  |
| `tx_meta`        | tx_hash (32B)                               | TxMeta (bincode)    | Transaction metadata (block, fee, size) |
| `block_meta`     | block_hash (32B)                            | BlockMeta (bincode) | Block metadata (number, timestamp, DAO) |
| `nft_item_index` | nft_type(1) + nft_id(32) + outpoint(34)     | empty               | Historical NFT outpoint tracking        |
| `ft_index`       | ft_type(1) + script_hash(32) + outpoint(34) | empty               | Historical FT outpoint tracking         |
| `activities`     | block_num(8) + tx_index(4) + seq(2)         | Activity (bincode)  | Unified activity records                |

## Key Design

- `CkbadgerStore::open(path)` — read-write for indexer. Opens default at `path`, append at `{path}-append`.
- `CkbadgerStore::open_secondary(primary, secondary)` — read-only for API. Both stores opened in secondary mode.
- Default store: `get_cf()`, `iterator_cf()`, `multi_get_cf()`
- Append store: `append_get_cf()`, `append_iterator_cf()`, `append_multi_get_cf()`

## Reorg Strategy

1. **Append CFs**: No action. Uncle/reverted data stays as history.
2. **Block-number-keyed CFs** (`block_index`, `tx_index`, `block_issuance`, `addr_txs`): Range-delete from fork_point+1.
3. **Cell indices** (`live_cells`, `live_cells_by_*`): Derive from reverted tx outputs/inputs via `tx_index`.
4. **Asset indices** (`nft_outpoints`, `ft_outpoints`): Blind-delete for reverted cell outpoints.
5. **Aggregates** (`addr_stats`, `ft_stats`, `nft_collection_stats`): Recompute from live state.

## Memory

| Machine RAM | Expected Usage |
| ----------- | -------------- |
| >= 32GB     | ~22GB peak     |
| < 32GB      | ~8GB peak      |

## Environment Variables

| Parameter                    | Default                         | Description                                    |
| ---------------------------- | ------------------------------- | ---------------------------------------------- |
| `CKBADGER_DATA_PATH`         | `./data/ckbadger-store`         | Default store + append store (`{path}-append`) |
| `CKBADGER_DERIVED_DATA_PATH` | `./data/ckbadger-store-derived` | Derived store (legacy, may be removed)         |

```bash
# Default paths
cargo run -p ckbadger-indexer

# Custom paths
CKBADGER_DATA_PATH=/ssd/ckbadger-store cargo run -p ckbadger-indexer
```
