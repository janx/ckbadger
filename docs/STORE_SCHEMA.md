# ckbadger-store Column Families (63 total: 60 domain + 1 append-only + 2 network)

ckbadger runs three logical RocksDB store classes (all backed by `ckbadger-store`):

- **Domain store** (`[store].domain_data_path`, 60 CFs) — canonical chain view, all mutable state including activities, addr_txs, live/consumed cell markers, indexes, stats, and aggregates. May perform create/update/delete as required by chain progression and reorg handling.
- **Append-only store** (`[store].append_only_data_path`, 1 CF: `cells`) — immutable cell payloads, content-addressed by outpoint. Write-once, never updated or deleted.
- **Network store** (`[store].network_data_path`, 2 CFs: `net_nodes`, `net_stats`) — whole-network p2p-crawler observations: non-chain, non-deterministic, TTL-retained. Written solely by the opt-in `ckbadger-crawler` service; it is the **only store class EXEMPT from rebuild-from-genesis**. See the [Network Store](#network-store) section below.

The indexer opens the two chain stores (domain + append-only) read-write and the API opens them secondary (read-only). The network store follows the same sole-writer + secondary-reader model: the crawler opens it read-write (sole writer), read consumers (API) open it secondary (read-only). Cell reads are cross-store: live/consumed markers in domain, cell payloads in append-only.

## Column Families

| Column Family                    | Key                                                    | Value                                                                | Purpose                                                                                                                                                                                |
| -------------------------------- | ------------------------------------------------------ | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cells` **(append-only store)**  | tx_hash + output_index (34B)                           | LiveCellInfo                                                         | Immutable cell payload store (write-once, content-addressed)                                                                                                                           |
| `live_cells`                     | tx_hash + output_index (34B)                           | empty                                                                | Live UTXO pointer set                                                                                                                                                                  |
| `consumed_cells`                 | tx_hash + output_index (34B)                           | ConsumedCellMeta                                                     | Consumed pointer + consume metadata                                                                                                                                                    |
| `reorg_undo_log_by_block`        | block + seq                                            | UndoLogEntry                                                         | Unified rollback undo-log journal                                                                                                                                                      |
| `block_headers`                  | block_number (8B)                                      | CachedBlockHeader                                                    | Block header + DAO field cache                                                                                                                                                         |
| `block_hash_index`               | block_hash (32B)                                       | block_number (8B)                                                    | Reverse lookup: hash -> number                                                                                                                                                         |
| `cell_by_lock`                   | lock_script_hash + outpoint                            | empty                                                                | Cell index by lock script                                                                                                                                                              |
| `cell_by_type`                   | type_script_hash + outpoint                            | empty                                                                | Cell index by type script                                                                                                                                                              |
| `cell_by_lock_code`              | lock_code_hash + outpoint                              | empty                                                                | Cell index by lock code_hash                                                                                                                                                           |
| `cell_by_type_code`              | type_code_hash + outpoint                              | empty                                                                | Cell index by type code_hash                                                                                                                                                           |
| `cell_by_data_hash`              | blake2b(cell_data) + outpoint                          | empty                                                                | Cell index by data hash (code cell resolution)                                                                                                                                         |
| `tx_index`                       | block_number + tx_index                                | tx_hash                                                              | Transaction ordering index                                                                                                                                                             |
| `tx_hash_map`                    | tx_hash (32B)                                          | block_number + tx_index                                              | Reverse lookup: tx_hash -> position                                                                                                                                                    |
| `addr_balance`                   | lock_script_hash (32B)                                 | AddressBalance                                                       | Address balance and cell counts                                                                                                                                                        |
| `addr_txs`                       | lock_hash + block + tx_index                           | empty                                                                | Address transaction history index                                                                                                                                                      |
| `dao_deposits`                   | tx_hash + output_index (34B)                           | DaoDepositCacheEntry                                                 | DAO deposit lifecycle cache                                                                                                                                                            |
| `dao_by_withdraw_tx`             | withdraw_outpoint (34B)                                | deposit outpoint                                                     | Reverse lookup: withdraw outpoint -> deposit                                                                                                                                           |
| `dao_by_block`                   | block_desc (8B BE) + outpoint (34B)                    | empty                                                                | DAO index ordered by deposit block DESC                                                                                                                                                |
| `dao_by_lock_block`              | lock_hash (32B) + block_desc (8B BE) + outpoint (34B)  | empty                                                                | DAO index by lock + deposit block DESC                                                                                                                                                 |
| `dao_by_status_block`            | status (2B BE) + block_desc (8B BE) + outpoint (34B)   | empty                                                                | DAO index by status + deposit block DESC                                                                                                                                               |
| `tokens`                         | type_script_hash (32B)                                 | TokenInfo                                                            | UDT token metadata                                                                                                                                                                     |
| `token_holders`                  | type_hash + lock_hash                                  | balance                                                              | Token holder balances                                                                                                                                                                  |
| `token_holders_by_balance`       | type_hash + balance_desc + lock_hash                   | empty                                                                | Token holders ranked by balance DESC                                                                                                                                                   |
| `addr_tokens_by_balance`         | lock_hash + balance_desc + type_hash                   | empty                                                                | Address token balances ranked by balance DESC                                                                                                                                          |
| `token_transfers`                | type_hash + block + tx_index                           | TransferInfo                                                         | Token transfer records                                                                                                                                                                 |
| `spore_data`                     | spore_id (32B)                                         | SporeData                                                            | Spore NFT metadata                                                                                                                                                                     |
| `spore_by_cluster`               | cluster_id + spore_id                                  | empty                                                                | Spore index by cluster                                                                                                                                                                 |
| `mnft_data`                      | object_id                                              | ObjectEntry                                                          | mNFT metadata (issuer/class/token)                                                                                                                                                     |
| `mnft_by_collection`             | collection_id + object_id                              | empty                                                                | mNFT index by collection                                                                                                                                                               |
| `identity_data`                  | identity_id (32B)                                      | IdentityEntry                                                        | Identity metadata (.bit, did:ckb)                                                                                                                                                      |
| `mnft_collection_agg`            | collection_id                                          | MnftCollectionAggregate                                              | mNFT collection aggregate stats                                                                                                                                                        |
| `object_collection_activities`   | collection_id + block + tx                             | ActivityRecord                                                       | Pre-computed Object collection activity feed                                                                                                                                           |
| `identity_by_collection`         | collection_id + identity_id                            | empty                                                                | Identity index by collection                                                                                                                                                           |
| `identity_agg`                   | collection_id (sentinel 32B)                           | IdentityCollectionAgg                                                | Identity collection aggregate stats (domain)                                                                                                                                           |
| `identity_collection_activities` | collection_id + block + tx                             | ActivityRecord                                                       | Pre-computed Identity collection activity feed (domain)                                                                                                                                |
| `stats_identity`                 | collection_id + lock_hash                              | i64 (owner count)                                                    | Per-owner identity counts by collection                                                                                                                                                |
| `activities`                     | block_num_desc + tx_idx_desc + tx_hash (44B)           | TxActivityBundle                                                     | Per-tx activity bundle (all owner deltas, includes protocol_actions per owner)                                                                                                         |
| `pending_proposals`              | proposal_id (10B hex string)                           | CachedProposal (JSON)                                                | Ephemeral pending proposal cache (live sync only)                                                                                                                                      |
| `fiber_channels`                 | channel_id (32B blake2b)                               | FiberChannel                                                         | Fiber Network channel registry                                                                                                                                                         |
| `fiber_channel_by_commitment`    | commitment_hash                                        | channel_id (32B)                                                     | Fiber channel index by commitment                                                                                                                                                      |
| `fiber_channel_by_funding_args`  | funding_lock_args                                      | channel_id (32B)                                                     | Fiber channel index by funding args                                                                                                                                                    |
| `addr_fiber_channels`            | lock_hash (32B) + channel_id (32B)                     | empty                                                                | Address-to-Fiber-channels index                                                                                                                                                        |
| `cluster_agg`                    | cluster_id                                             | ClusterAgg                                                           | Spore cluster aggregate stats                                                                                                                                                          |
| `script_info`                    | code_hash (32B)                                        | ScriptInfo                                                           | Legacy/compatibility script metadata keyed by bare hash                                                                                                                                |
| `stats_chain`                    | prefixed keys                                          | chain chart snapshots                                                | Daily/hourly/epoch/miner/block stats (DailyActivityStats includes protocol_action_counts)                                                                                              |
| `stats_dao`                      | prefixed keys                                          | DAO snapshots                                                        | DAO daily snapshots plus latest/top DAO summaries (sealed aggregates in bulk build)                                                                                                    |
| `stats_hodl`                     | prefixed keys                                          | HODL/chart snapshots                                                 | HODL waves, cell distribution, address cohorts                                                                                                                                         |
| `stats_script`                   | prefixed keys                                          | ScriptDailyDelta                                                     | Script daily deltas (per `code_hash` + lock/type + day; sealed in bulk build)                                                                                                          |
| `stats_token`                    | prefixed keys                                          | token rollups + deltas                                               | Token transfer totals, hourly buckets, and daily deltas (sealed in bulk build)                                                                                                         |
| `stats_spore`                    | prefixed keys                                          | spore rollups/indexes                                                | Spore/cluster daily + owner/index stats                                                                                                                                                |
| `stats_mnft`                     | prefixed keys                                          | mNFT rollups/indexes                                                 | mNFT daily + hourly + owner/index stats                                                                                                                                                |
| `script_versions`                | version_hash                                           | ScriptVersionInfo                                                    | Canonical script code version rows keyed by `H(script_code)`                                                                                                                           |
| `script_versions_by_label`       | label_len + label_key + version_hash                   | empty                                                                | Label-to-version index for named script family lookups                                                                                                                                 |
| `script_families`                | family_id (string)                                     | ScriptFamilyInfo                                                     | Script family metadata (groups related script versions)                                                                                                                                |
| `script_versions_by_family`      | family_id + version_hash                               | empty                                                                | Script versions indexed by family                                                                                                                                                      |
| `script_reference_info`          | reference_hash + hash_type (33B)                       | ScriptReferenceInfo                                                  | Script reference aggregate stats (cell/capacity counts per lock/type)                                                                                                                  |
| `script_reference_to_version`    | reference_hash + hash_type (33B)                       | version_hash                                                         | Script reference to version mapping                                                                                                                                                    |
| `script_family_by_name`          | family_name (string)                                   | family_id                                                            | Reverse lookup: family name -> family ID                                                                                                                                               |
| `sync_meta`                      | fixed keys                                             | SyncStatus/ReorgEvent                                                | Sync progress, deep-fork, reorg metadata                                                                                                                                               |
| `dob_decoded`                    | spore_id (32B)                                         | DecodeOutcome (Decoded(DobDecodedEntry) \| Failed(DobDecodeFailure)) | Cached CKB-VM DOB decode outcome (bulk-disabled, populated after sync catches up to tip). Failed is written only for deterministic failures; transient RPC failures are not persisted. |
| `lock_scripts`                   | lock_hash (32B)                                        | LockScriptEntry                                                      | Lock script components by hash (survives cell consumption for address resolution)                                                                                                      |
| `net_nodes` **(network store)**  | peer_id (raw bytes)                                    | NodeRecord                                                           | Per-peer crawler observation (own_addrs, client_version, flags, protocols, first/last_seen, last_reachable_at, reachable, geo, asn, last_rtt_ms, sampled known_peers)                  |
| `net_stats` **(network store)**  | `0x00` singleton, or metric(1B)+gran(1B)+bucket(8B BE) | LatestStatus / HistoryPoint                                          | Latest-round status singleton (key `0x00`) + time-bucketed history points keyed per metric × granularity                                                                               |

### Script Modeling Note

Script version and label metadata lives in `script_versions` and `script_versions_by_label` CFs,
written by `label_import`. Script resolution (reference -> version -> code cell instances) is
performed at API query time using the existing cell indexes (`cell_by_data_hash`, `cell_by_type`,
`cell_by_type_code`) rather than via dedicated indexer-maintained CFs.

`script_info` remains as a compatibility cache keyed only by bare `code_hash`. That legacy shape is
still useful for some read paths, but it is not a complete canonical model for CKB script resolution
because:

- runtime reference identity is `(reference_hash, hash_type)`, not bare `code_hash`
- `type` references are current-state dependent and may resolve differently across upgrades
- exact version attribution for historical execution must come from the transaction's actual
  `cell_deps`

See [docs/SCRIPTS_CODE_CELLS_AND_REFS.md](./SCRIPTS_CODE_CELLS_AND_REFS.md) for the terminology and
model that future script schema refactors should follow.

### DAO Secondary Index Notes

- `dao_by_block`: key = `i64::MAX - deposit_block` (big-endian) + deposit outpoint, supports global DAO deposit pagination in newest-first order.
- `dao_by_lock_block`: key = `lock_script_hash(32B)` + block_desc + outpoint, supports per-address DAO deposit pagination.
- `dao_by_status_block`: key = `status(i16 BE)` + block_desc + outpoint, supports status-filtered DAO queries (`deposited/withdrawing/withdrawn`).

### Bulk-Build Sealed Aggregate Note

During fresh-db bulk sync, the indexer writes `stats_dao`, `stats_script`, and `stats_token`
inline as Class C sealed aggregates:

- `stats_dao` stores DAO daily snapshots keyed by date, then refreshes the latest/top summary rows
  after sync tip metadata is finalized.
- `stats_script` stores `ScriptDailyDelta` rows keyed by `code_hash + kind(lock/type) + YYYYMMDD`.
- `stats_token` stores total transfer counters, hourly transfer buckets, and `TokenDailyDelta`
  rows keyed by token `type_script_hash`.

### stats_hodl Key Prefixes

The `stats_hodl` CF uses single-byte prefixes to multiplex different snapshot types. Key format: `prefix(1B) + date_string`.

| Prefix | Constant                         | Value Type            | Description                                         |
| ------ | -------------------------------- | --------------------- | --------------------------------------------------- |
| `0x0B` | `STATS_PREFIX_HODL_WAVE`         | HODL wave snapshot    | Daily HODL wave age-band distribution               |
| `0x21` | `STATS_PREFIX_CELL_DISTRIBUTION` | DailyCellDistribution | Daily cell distribution (age bands + size buckets)  |
| `0x22` | `STATS_PREFIX_ADDR_COHORT`       | DailyAddressCohort    | Daily address cohort retention (new/returning/lost) |

Cell distribution and address cohort snapshots are materialized by the indexer during sync (one snapshot per day boundary). The API reads these directly instead of scanning live cells.

## Network Store

The **network store** (`[store].network_data_path`, default `data/network`, CFs `net_nodes` + `net_stats`) is a distinct third RocksDB store class holding whole-network CKB L1 p2p-crawler observations. Unlike the two chain stores it is:

- **Non-chain / non-deterministic** — contents are derived from live peer-to-peer observation (reachability probes, discovery responses), not from deterministic block replay.
- **TTL-retained** — node records and history rollups are pruned on a rolling retention window; it is not a permanent append log.
- **The only store class EXEMPT from rebuild-from-genesis** — it cannot be reconstructed by replaying the chain, so deleting/rebuilding chain data does not touch it.
- **Single-writer** — written exclusively by the opt-in `ckbadger-crawler` service (`ckbadger crawl`; enabled via `[crawler].enabled`, default `false`). The indexer never writes it. Read consumers (API) open it secondary (read-only), the same access model as the chain stores.

### `net_nodes`

Key = raw `peer_id` bytes → `NodeRecord` (per-peer crawler view: `own_addrs`, `client_version`, `flags`, `protocols`, `first_seen`, `last_seen`, `last_reachable_at`, `reachable`, `geo`, `asn`, `last_rtt_ms`, and a per-round sample of `known_peers`).

### `net_stats` key layout

- `0x00` (single reserved byte) → `LatestStatus` singleton — summary of the latest completed crawl round.
- `metric(1B) + granularity(1B) + ts_bucket(8B big-endian)` → `HistoryPoint` — time-bucketed rollups. Metrics: total nodes, reachable nodes, version share, country share. Granularity: hour, day. The big-endian bucket keeps each `(metric, granularity)` series in chronological key order, so range scans and prunes are contiguous.

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

| Parameter                       | Default            | Description                                                                    |
| ------------------------------- | ------------------ | ------------------------------------------------------------------------------ |
| `[store].domain_data_path`      | `data/domain`      | Domain RocksDB data directory                                                  |
| `[store].append_only_data_path` | `data/append-only` | Append-only RocksDB data directory                                             |
| `[store].network_data_path`     | `data/network`     | Network-crawler RocksDB data directory (opt-in; written by `ckbadger-crawler`) |

```bash
[store]
domain_data_path = "/ssd/ckbadger-store"
append_only_data_path = "/ssd/ckbadger-store-append-only"
network_data_path = "/ssd/ckbadger-store-network"
```
