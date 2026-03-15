# ckbadger-store Column Families (52 total: 51 domain + 1 append-only)

ckbadger runs two logical RocksDB stores (both backed by `ckbadger-store`):

- **Domain store** (`[store].domain_data_path`, 51 CFs) — canonical chain view, all mutable state including activities, addr_txs, live/consumed cell markers, indexes, stats, and aggregates. May perform create/update/delete as required by chain progression and reorg handling.
- **Append-only store** (`[store].append_only_data_path`, 1 CF: `cells`) — immutable cell payloads, content-addressed by outpoint. Write-once, never updated or deleted.

The indexer opens both stores read-write; the API opens both in secondary (read-only) mode. Cell reads are cross-store: live/consumed markers in domain, cell payloads in append-only.

## Column Families

| Column Family                    | Key                                                   | Value                   | Purpose                                                                                   |
| -------------------------------- | ----------------------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------------- |
| `cells` **(append-only store)**  | tx_hash + output_index (34B)                          | LiveCellInfo            | Immutable cell payload store (write-once, content-addressed)                              |
| `live_cells`                     | tx_hash + output_index (34B)                          | empty                   | Live UTXO pointer set                                                                     |
| `consumed_cells`                 | tx_hash + output_index (34B)                          | ConsumedCellMeta        | Consumed pointer + consume metadata                                                       |
| `reorg_undo_log_by_block`        | block + seq                                           | UndoLogEntry            | Unified rollback undo-log journal                                                         |
| `block_headers`                  | block_number (8B)                                     | CachedBlockHeader       | Block header + DAO field cache                                                            |
| `block_hash_index`               | block_hash (32B)                                      | block_number (8B)       | Reverse lookup: hash -> number                                                            |
| `cell_by_lock`                   | lock_script_hash + outpoint                           | empty                   | Cell index by lock script                                                                 |
| `cell_by_type`                   | type_script_hash + outpoint                           | empty                   | Cell index by type script                                                                 |
| `cell_by_lock_code`              | lock_code_hash + outpoint                             | empty                   | Cell index by lock code_hash                                                              |
| `cell_by_type_code`              | type_code_hash + outpoint                             | empty                   | Cell index by type code_hash                                                              |
| `cell_by_data_hash`              | blake2b(cell_data) + outpoint                         | empty                   | Cell index by data hash (code cell resolution)                                            |
| `tx_index`                       | block_number + tx_index                               | tx_hash                 | Transaction ordering index                                                                |
| `tx_hash_map`                    | tx_hash (32B)                                         | block_number + tx_index | Reverse lookup: tx_hash -> position                                                       |
| `addr_balance`                   | lock_script_hash (32B)                                | AddressBalance          | Address balance and cell counts                                                           |
| `addr_txs`                       | lock_hash + block + tx_index                          | empty                   | Address transaction history index                                                         |
| `dao_deposits`                   | tx_hash + output_index (34B)                          | DaoDepositCacheEntry    | DAO deposit lifecycle cache                                                               |
| `dao_by_withdraw_tx`             | withdraw_outpoint (34B)                               | deposit outpoint        | Reverse lookup: withdraw outpoint -> deposit                                              |
| `dao_by_block`                   | block_desc (8B BE) + outpoint (34B)                   | empty                   | DAO index ordered by deposit block DESC                                                   |
| `dao_by_lock_block`              | lock_hash (32B) + block_desc (8B BE) + outpoint (34B) | empty                   | DAO index by lock + deposit block DESC                                                    |
| `dao_by_status_block`            | status (2B BE) + block_desc (8B BE) + outpoint (34B)  | empty                   | DAO index by status + deposit block DESC                                                  |
| `tokens`                         | type_script_hash (32B)                                | TokenInfo               | UDT token metadata                                                                        |
| `token_holders`                  | type_hash + lock_hash                                 | balance                 | Token holder balances                                                                     |
| `token_holders_by_balance`       | type_hash + balance_desc + lock_hash                  | empty                   | Token holders ranked by balance DESC                                                      |
| `addr_tokens_by_balance`         | lock_hash + balance_desc + type_hash                  | empty                   | Address token balances ranked by balance DESC                                             |
| `token_transfers`                | type_hash + block + tx_index                          | TransferInfo            | Token transfer records                                                                    |
| `spore_data`                     | spore_id (32B)                                        | SporeData               | Spore NFT metadata                                                                        |
| `spore_by_cluster`               | cluster_id + spore_id                                 | empty                   | Spore index by cluster                                                                    |
| `object_data`                    | object_id                                             | ObjectData              | Unified Object metadata (mNFT, etc.)                                                      |
| `object_by_collection`           | collection_id + object_id                             | empty                   | Object index by collection                                                                |
| `identity_data`                  | identity_id (32B)                                     | IdentityEntry           | Identity metadata (.bit, did:ckb)                                                         |
| `object_collection_agg`          | collection_id                                         | ObjectCollectionAgg     | Object collection aggregate stats                                                         |
| `object_collection_activities`   | collection_id + block + tx                            | ActivityRecord          | Pre-computed Object collection activity feed                                              |
| `identity_by_collection`         | collection_id + identity_id                           | empty                   | Identity index by collection                                                              |
| `identity_agg`                   | collection_id (sentinel 32B)                          | IdentityCollectionAgg   | Identity collection aggregate stats (domain)                                              |
| `identity_collection_activities` | collection_id + block + tx                            | ActivityRecord          | Pre-computed Identity collection activity feed (domain)                                   |
| `stats_identity`                 | collection_id + lock_hash                             | i64 (owner count)       | Per-owner identity counts by collection                                                   |
| `activities`                     | block_num_desc + tx_idx_desc + tx_hash (44B)          | TxActivityBundle        | Per-tx activity bundle (all owner deltas, includes protocol_actions per owner)            |
| `pending_proposals`              | proposal_id (10B hex string)                          | CachedProposal (JSON)   | Ephemeral pending proposal cache (live sync only)                                         |
| `fiber_channels`                 | channel_id (32B blake2b)                              | FiberChannel            | Fiber Network channel registry                                                            |
| `fiber_channel_by_commitment`    | commitment_hash                                       | channel_id (32B)        | Fiber channel index by commitment                                                         |
| `fiber_channel_by_funding_args`  | funding_lock_args                                     | channel_id (32B)        | Fiber channel index by funding args                                                       |
| `addr_fiber_channels`            | lock_hash (32B) + channel_id (32B)                    | empty                   | Address-to-Fiber-channels index                                                           |
| `cluster_agg`                    | cluster_id                                            | ClusterAgg              | Spore cluster aggregate stats                                                             |
| `script_info`                    | code_hash (32B)                                       | ScriptInfo              | Known script metadata                                                                     |
| `stats_chain`                    | prefixed keys                                         | chain chart snapshots   | Daily/hourly/epoch/miner/block stats (DailyActivityStats includes protocol_action_counts) |
| `stats_dao`                      | prefixed keys                                         | DAO snapshots           | DAO daily snapshots                                                                       |
| `stats_hodl`                     | prefixed keys                                         | HODL/chart snapshots    | HODL waves, cell distribution, address cohorts                                            |
| `stats_script`                   | prefixed keys                                         | ScriptDailyDelta        | Script daily deltas                                                                       |
| `stats_token`                    | prefixed keys                                         | token rollups + deltas  | Token transfer/hourly/daily stats                                                         |
| `stats_spore`                    | prefixed keys                                         | spore rollups/indexes   | Spore/cluster daily + owner/index stats                                                   |
| `stats_object`                   | prefixed keys                                         | object rollups/indexes  | Object/mNFT daily + hourly + indexes                                                      |
| `sync_meta`                      | fixed keys                                            | SyncStatus/ReorgEvent   | Sync progress, deep-fork, reorg metadata                                                  |

### DAO Secondary Index Notes

- `dao_by_block`: key = `i64::MAX - deposit_block` (big-endian) + deposit outpoint, supports global DAO deposit pagination in newest-first order.
- `dao_by_lock_block`: key = `lock_script_hash(32B)` + block_desc + outpoint, supports per-address DAO deposit pagination.
- `dao_by_status_block`: key = `status(i16 BE)` + block_desc + outpoint, supports status-filtered DAO queries (`deposited/withdrawing/withdrawn`).

### stats_hodl Key Prefixes

The `stats_hodl` CF uses single-byte prefixes to multiplex different snapshot types. Key format: `prefix(1B) + date_string`.

| Prefix | Constant                         | Value Type            | Description                                         |
| ------ | -------------------------------- | --------------------- | --------------------------------------------------- |
| `0x0B` | `STATS_PREFIX_HODL_WAVE`         | HODL wave snapshot    | Daily HODL wave age-band distribution               |
| `0x21` | `STATS_PREFIX_CELL_DISTRIBUTION` | DailyCellDistribution | Daily cell distribution (age bands + size buckets)  |
| `0x22` | `STATS_PREFIX_ADDR_COHORT`       | DailyAddressCohort    | Daily address cohort retention (new/returning/lost) |

Cell distribution and address cohort snapshots are materialized by the indexer during sync (one snapshot per day boundary). The API reads these directly instead of scanning live cells.

## Key Design

- `CkbadgerStore::open_domain(path)` / `open_append_only(path)` — primary read-write mode for indexer and maintenance commands (split domain + append-only)
- `CkbadgerStore::open_domain_secondary(primary_path, secondary_path)` / `open_append_only_secondary(primary_path, secondary_path)` — read-only mode for API/TUI (split secondary stores)
- All store operations are synchronous (RocksDB reads are fast)

## Memory Considerations

| Machine RAM | Expected Usage |
| ----------- | -------------- |
| >= 32GB     | ~22GB peak     |
| < 32GB      | ~8GB peak      |

## Config Keys

| Parameter                       | Default            | Description                        |
| ------------------------------- | ------------------ | ---------------------------------- |
| `[store].domain_data_path`      | `data/domain`      | Domain RocksDB data directory      |
| `[store].append_only_data_path` | `data/append-only` | Append-only RocksDB data directory |

```bash
[store]
domain_data_path = "/ssd/ckbadger-store"
append_only_data_path = "/ssd/ckbadger-store-append-only"
```
