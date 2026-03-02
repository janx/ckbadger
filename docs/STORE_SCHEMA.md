# ckbadger-store Column Families (37 total)

ckbadger runs two logical RocksDB stores (both backed by `ckbadger-store`):

- **Domain store** (`CKBADGER_DOMAIN_DATA_PATH`) — mutable canonical/query state
- **Append-only store** (`CKBADGER_APPEND_ONLY_DATA_PATH`) — immutable history/index archives

The indexer opens both stores read-write; the API opens both in secondary (read-only) mode.

## Column Families

| Column Family               | Key                          | Value                   | Purpose                                  |
| --------------------------- | ---------------------------- | ----------------------- | ---------------------------------------- |
| `cells`                     | tx_hash + output_index (34B) | LiveCellInfo            | Canonical append-only cell payload store |
| `live_cells`                | tx_hash + output_index (34B) | empty                   | Live UTXO pointer set                    |
| `consumed_cells`            | tx_hash + output_index (34B) | ConsumedCellMeta        | Consumed pointer + consume metadata      |
| `block_headers`             | block_number (8B)            | CachedBlockHeader       | Block header + DAO field cache           |
| `block_hash_index`          | block_hash (32B)             | block_number (8B)       | Reverse lookup: hash -> number           |
| `cell_by_lock`              | lock_script_hash + outpoint  | empty                   | Cell index by lock script                |
| `cell_by_type`              | type_script_hash + outpoint  | empty                   | Cell index by type script                |
| `cell_by_lock_code`         | lock_code_hash + outpoint    | empty                   | Cell index by lock code_hash             |
| `cell_by_type_code`         | type_code_hash + outpoint    | empty                   | Cell index by type code_hash             |
| `tx_index`                  | block_number + tx_index      | tx_hash                 | Transaction ordering index               |
| `tx_hash_map`               | tx_hash (32B)                | block_number + tx_index | Reverse lookup: tx_hash -> position      |
| `addr_balance`              | lock_script_hash (32B)       | AddressBalance          | Address balance and cell counts          |
| `addr_txs`                  | lock_hash + block + tx_index | empty                   | Address transaction history index        |
| `addr_daily_stats`          | lock_hash + date             | AddressDailyStats       | Per-address daily aggregates             |
| `dao_deposits`              | tx_hash + output_index (34B) | DaoDepositCacheEntry    | DAO deposit lifecycle cache              |
| `dao_by_withdraw_tx`        | withdraw_tx_hash (32B)       | deposit outpoint        | Reverse lookup: withdraw -> deposit      |
| `block_issuance`            | block_number (8B)            | BlockIssuance           | Per-block issuance data                  |
| `tokens`                    | type_script_hash (32B)       | TokenInfo               | UDT token metadata                       |
| `token_holders`             | type_hash + lock_hash        | balance                 | Token holder balances                    |
| `token_transfers`           | type_hash + block + tx_index | TransferInfo            | Token transfer records                   |
| `spore_data`                | spore_id (32B)               | SporeData               | Spore NFT metadata                       |
| `spore_by_cluster`          | cluster_id + spore_id        | empty                   | Spore index by cluster                   |
| `nft_data`                  | nft_id                       | NftData                 | Unified NFT metadata (.bit, mNFT, etc.)  |
| `nft_by_collection`         | collection_id + nft_id       | empty                   | NFT index by collection                  |
| `nft_collection_agg`        | collection_id                | NftCollectionAgg        | NFT collection aggregate stats           |
| `nft_collection_activities` | collection_id + block + tx   | ActivityRecord          | Pre-computed collection activity feed    |
| `activities`                | addr/token/entity + block+tx | ActivityRecord          | Unified activity feed                    |
| `cluster_agg`               | cluster_id                   | ClusterAgg              | Spore cluster aggregate stats            |
| `script_info`               | code_hash (32B)              | ScriptInfo              | Known script metadata                    |
| `stats_chain`               | prefixed keys                | chain chart snapshots   | Daily/hourly/epoch/miner/block stats     |
| `stats_dao`                 | prefixed keys                | DAO snapshots           | DAO daily snapshots                      |
| `stats_hodl`                | prefixed keys                | HODL snapshots          | HODL wave timelines                      |
| `stats_script`              | prefixed keys                | ScriptDailyDelta        | Script daily deltas                      |
| `stats_token`               | prefixed keys                | token rollups + deltas  | Token transfer/hourly/daily stats        |
| `stats_spore`               | prefixed keys                | spore rollups/indexes   | Spore/cluster daily + owner/index stats  |
| `stats_nft`                 | prefixed keys                | nft rollups/indexes     | NFT/mNFT/.bit daily + hourly + indexes   |
| `sync_meta`                 | fixed keys                   | SyncStatus/ReorgEvent   | Sync progress, deep-fork, reorg metadata |

## Key Design

- `CkbadgerStore::open(path)` — primary read-write mode for indexer and maintenance CLI commands (domain + append-only)
- `CkbadgerStore::open_secondary(primary_path, secondary_path)` — read-only mode for API (domain + append-only)
- All store operations are synchronous (RocksDB reads are fast)

## Memory Considerations

| Machine RAM | Expected Usage |
| ----------- | -------------- |
| >= 32GB     | ~22GB peak     |
| < 32GB      | ~8GB peak      |

## Environment Variables

| Parameter                        | Default                             | Description                        |
| -------------------------------- | ----------------------------------- | ---------------------------------- |
| `CKBADGER_DOMAIN_DATA_PATH`      | `./data/ckbadger-store`             | Domain RocksDB data directory      |
| `CKBADGER_APPEND_ONLY_DATA_PATH` | `./data/ckbadger-store-append-only` | Append-only RocksDB data directory |

```bash
# Default: uses ./data/ckbadger-store
cargo run -p ckbadger-indexer

# Custom paths
CKBADGER_DOMAIN_DATA_PATH=/ssd/ckbadger-store \
CKBADGER_APPEND_ONLY_DATA_PATH=/ssd/ckbadger-store-append-only \
cargo run -p ckbadger-indexer
```
