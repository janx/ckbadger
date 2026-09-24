# ckbadger-store Column Families (67 total: 63 domain + 1 append-only + 3 network)

ckbadger runs three logical RocksDB store classes (all backed by `ckbadger-store`):

- **Domain store** (`[store].domain_data_path`, 63 CFs) — canonical chain view, all mutable state including activities, addr_txs, live/consumed cell markers, indexes, stats, and aggregates. May perform create/update/delete as required by chain progression and reorg handling.
- **Append-only store** (`[store].append_only_data_path`, 1 CF: `cells`) — immutable cell payloads, content-addressed by outpoint. Write-once, never updated or deleted.
- **Network store** (`[store].network_data_path`, 3 CFs: `net_nodes`, `net_stats`, `net_crawl`) —
  crawler p2p probes, configured-local-node session observations, and durable in-progress crawl
  state: non-chain, non-deterministic, TTL-retained. Written solely by the opt-in
  `ckbadger-crawler` service; it is the **only store class EXEMPT from rebuild-from-genesis**. See
  the [Network Store](#network-store) section below.

The indexer opens the two chain stores (domain + append-only) read-write and the API opens them secondary (read-only). The network store follows the same sole-writer + secondary-reader model: the crawler opens it read-write (sole writer), read consumers (API) open it secondary (read-only). Cell reads are cross-store: live/consumed markers in domain, cell payloads in append-only.

## Column Families

| Column Family                    | Key                                                                           | Value                                                                | Purpose                                                                                                                                                                                                                                                    |
| -------------------------------- | ----------------------------------------------------------------------------- | -------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cells` **(append-only store)**  | tx_hash + output_index (34B)                                                  | LiveCellInfo                                                         | Immutable cell payload store (write-once, content-addressed)                                                                                                                                                                                               |
| `live_cells`                     | tx_hash + output_index (34B)                                                  | empty                                                                | Live UTXO pointer set                                                                                                                                                                                                                                      |
| `consumed_cells`                 | tx_hash + output_index (34B)                                                  | ConsumedCellMeta                                                     | Consumed pointer + consume metadata                                                                                                                                                                                                                        |
| `reorg_undo_log_by_block`        | block + seq                                                                   | UndoLogEntry                                                         | Unified rollback undo-log journal                                                                                                                                                                                                                          |
| `block_headers`                  | block_number (8B)                                                             | CachedBlockHeader                                                    | Block header + DAO field cache                                                                                                                                                                                                                             |
| `block_hash_index`               | block_hash (32B)                                                              | block_number (8B)                                                    | Reverse lookup: hash -> number                                                                                                                                                                                                                             |
| `cell_by_lock`                   | lock_script_hash + outpoint                                                   | empty                                                                | Cell index by lock script                                                                                                                                                                                                                                  |
| `cell_by_type`                   | type_script_hash + outpoint                                                   | empty                                                                | Cell index by type script                                                                                                                                                                                                                                  |
| `cell_by_lock_code`              | lock_code_hash (32B) + hash_type (1B) + block (8B BE) + outpoint              | empty                                                                | Cell index by lock script reference form `(code_hash, hash_type)`                                                                                                                                                                                          |
| `cell_by_type_code`              | type_code_hash (32B) + hash_type (1B) + block (8B BE) + outpoint              | empty                                                                | Cell index by type script reference form `(code_hash, hash_type)`                                                                                                                                                                                          |
| `cell_by_data_hash`              | blake2b(cell_data) + outpoint                                                 | empty                                                                | Cell index by data hash (code cell resolution)                                                                                                                                                                                                             |
| `tx_index`                       | block_number + tx_index                                                       | tx_hash                                                              | Transaction ordering index                                                                                                                                                                                                                                 |
| `tx_hash_map`                    | tx_hash (32B)                                                                 | block_number + tx_index                                              | Reverse lookup: tx_hash -> position                                                                                                                                                                                                                        |
| `addr_balance`                   | lock_script_hash (32B)                                                        | AddressBalance                                                       | Address balance and cell counts                                                                                                                                                                                                                            |
| `addr_txs`                       | lock_hash + block + tx_index + tx_hash                                        | AddrTxValue                                                          | Address transaction thin index with capacity change, tx flags, and participant activity tags                                                                                                                                                               |
| `addr_txs_by_prefix`             | lock_hash_prefix(20) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32) = 64B | AddrTxValue                                                          | Same thin index for a party a protocol NAMED by its 20-byte lock-hash prefix and that holds no cell in the transaction                                                                                                                                     |
| `addr_prefix_stats`              | lock_hash_prefix(20)                                                          | AddrPrefixStats                                                      | How many transactions named this prefix without it holding a cell; added to `addr_balance.txs_count` by `address_tx_count`                                                                                                                                 |
| `dao_deposits`                   | tx_hash + output_index (34B)                                                  | DaoDepositCacheEntry                                                 | DAO lifecycle plus original capacity, exact occupied capacity, deposit/request ARs, and claimed compensation                                                                                                                                               |
| `dao_by_withdraw_tx`             | withdraw_outpoint (34B)                                                       | deposit outpoint                                                     | Reverse lookup: withdraw outpoint -> deposit                                                                                                                                                                                                               |
| `dao_by_block`                   | block_desc (8B BE) + outpoint (34B)                                           | empty                                                                | DAO index ordered by deposit block DESC                                                                                                                                                                                                                    |
| `dao_by_lock_block`              | lock_hash (32B) + block_desc (8B BE) + outpoint (34B)                         | empty                                                                | DAO index by lock + deposit block DESC                                                                                                                                                                                                                     |
| `dao_by_status_block`            | status (2B BE) + block_desc (8B BE) + outpoint (34B)                          | empty                                                                | DAO index by status + deposit block DESC                                                                                                                                                                                                                   |
| `tokens`                         | type_script_hash (32B)                                                        | TokenInfo                                                            | UDT metadata only; total supply and holder count derive from `token_holders`                                                                                                                                                                               |
| `token_holders`                  | type_hash (32B) + lock_hash (32B)                                             | TokenBalance (32B unsigned BE)                                       | Exact aggregate holder balances; values may exceed a single cell's u128 amount                                                                                                                                                                             |
| `token_holders_by_balance`       | type_hash (32B) + complemented balance BE (32B) + lock_hash (32B)             | empty                                                                | 96B key; token holders ranked by balance DESC, lock hash ASC                                                                                                                                                                                               |
| `addr_tokens_by_balance`         | lock_hash (32B) + complemented balance BE (32B) + type_hash (32B)             | empty                                                                | 96B key; address token balances ranked by balance DESC, type hash ASC                                                                                                                                                                                      |
| `token_transfers`                | type_hash + block + tx_index                                                  | TransferInfo                                                         | Token transfer records                                                                                                                                                                                                                                     |
| `spore_data`                     | spore_id (32B)                                                                | SporeData                                                            | Spore NFT metadata                                                                                                                                                                                                                                         |
| `spore_by_cluster`               | cluster_id + spore_id                                                         | empty                                                                | Spore index by cluster                                                                                                                                                                                                                                     |
| `mnft_data`                      | object_id                                                                     | ObjectEntry                                                          | mNFT metadata (issuer/class/token)                                                                                                                                                                                                                         |
| `mnft_by_collection`             | collection_id + object_id                                                     | empty                                                                | mNFT index by collection                                                                                                                                                                                                                                   |
| `identity_data`                  | identity_id (20B AccountCell / `.cell` name; 32B .bit Cell/did:ckb)           | IdentityEntry                                                        | Identity metadata with separate standards and lifecycles for .bit AccountCell, .bit Cell, did:ckb and `.cell` names                                                                                                                                        |
| `mnft_collection_agg`            | collection_id                                                                 | MnftCollectionAggregate                                              | mNFT collection aggregate stats                                                                                                                                                                                                                            |
| `object_collection_activities`   | collection_id + block + tx                                                    | ObjectCollectionActivityEntry                                        | Pre-computed object collection activity feed                                                                                                                                                                                                               |
| `identity_by_collection`         | collection_id + identity_id                                                   | empty                                                                | Identity index by collection. A `.cell` sub-name is indexed twice: once under the `.cell` sentinel, once under its parent's padded 20-byte id, which is what lists a name's children                                                                       |
| `identity_agg`                   | collection_id (sentinel 32B)                                                  | IdentityCollectionAgg                                                | Per-standard identity aggregates; .bit AccountCell and .bit Cell use different sentinels                                                                                                                                                                   |
| `identity_collection_activities` | collection_id + block + tx                                                    | ObjectCollectionActivityEntry                                        | Pre-computed identity collection activity feed (domain)                                                                                                                                                                                                    |
| `stats_identity`                 | collection_id + owner segment (32B)                                           | i64 (owner count)                                                    | Per-owner identity counts by collection. The owner segment is a lock hash for every standard but `.cell`, whose chain-level owner is a 20-byte prefix written as an explicit `owner20 ‖ 0^12` and decoded back by collection — never served as a lock hash |
| `dotcell_name_by_owner`          | owner_hash20 (20B) + name_id (20B) = 40B                                      | empty                                                                | The `.cell` names a 20-byte owner prefix holds: address pages, holder counts, and the sale-state join                                                                                                                                                      |
| `dotcell_ring`                   | namespace_args (20B)                                                          | DotCellRingRoot                                                      | One row per `.cell` namespace: the uniqueness ring's root cell, so verify can walk the ring without scanning. A network runs exactly one namespace                                                                                                         |
| `activities`                     | block_num_desc + tx_idx_desc + tx_hash (44B)                                  | TxActions                                                            | One canonical per-tx activity record; TX-level protocol/type/lock actions stored once plus sorted participant deltas                                                                                                                                       |
| `pending_proposals`              | proposal_id (10B hex string)                                                  | CachedProposal (JSON)                                                | Ephemeral pending proposal cache (live sync only)                                                                                                                                                                                                          |
| `fiber_channels`                 | channel_id (32B blake2b of funding outpoint)                                  | FiberChannel                                                         | Fiber Network channel registry; funding lock args are descriptive and are not unique                                                                                                                                                                       |
| `fiber_channel_by_commitment`    | commitment_hash                                                               | channel_id (32B)                                                     | Fiber channel index by commitment                                                                                                                                                                                                                          |
| `addr_fiber_channels`            | lock_hash (32B) + channel_id (32B)                                            | empty                                                                | Address-to-Fiber-channels index                                                                                                                                                                                                                            |
| `cluster_agg`                    | cluster_id                                                                    | ClusterAgg                                                           | Spore cluster aggregate stats                                                                                                                                                                                                                              |
| `script_info`                    | code_hash (32B)                                                               | ScriptInfo                                                           | Legacy/compatibility script metadata keyed by bare hash                                                                                                                                                                                                    |
| `stats_chain`                    | prefixed keys                                                                 | chain chart snapshots                                                | Daily/hourly/epoch/miner/block stats (DailyActivityStats includes protocol_action_counts)                                                                                                                                                                  |
| `stats_dao`                      | prefixed keys                                                                 | DAO snapshots                                                        | DAO daily snapshots (including exact unclaimed and frozen phase-1 compensation), plus latest/top summaries; sealed aggregates in bulk build                                                                                                                |
| `stats_hodl`                     | prefixed keys                                                                 | HODL/chart snapshots                                                 | HODL waves, cell distribution, address cohorts                                                                                                                                                                                                             |
| `stats_script`                   | prefixed keys                                                                 | ScriptDailyDelta                                                     | Script daily deltas (per `code_hash` + `hash_type` + lock/type + day; sealed in bulk build)                                                                                                                                                                |
| `stats_token`                    | prefixed keys                                                                 | token rollups + deltas                                               | Token transfer totals, hourly buckets, and daily deltas (sealed in bulk build)                                                                                                                                                                             |
| `stats_spore`                    | prefixed keys                                                                 | spore rollups/indexes                                                | Spore/cluster daily + owner/index stats                                                                                                                                                                                                                    |
| `stats_mnft`                     | prefixed keys                                                                 | mNFT rollups/indexes                                                 | mNFT daily + hourly + owner/index stats                                                                                                                                                                                                                    |
| `script_versions`                | version_hash                                                                  | ScriptVersionInfo                                                    | Canonical script code version rows keyed by `H(script_code)`                                                                                                                                                                                               |
| `script_versions_by_label`       | label_len + label_key + version_hash                                          | empty                                                                | Label-to-version index for named script family lookups                                                                                                                                                                                                     |
| `script_families`                | family_id (string)                                                            | ScriptFamilyInfo                                                     | Script family metadata (groups related script versions)                                                                                                                                                                                                    |
| `script_versions_by_family`      | family_id + version_hash                                                      | empty                                                                | Script versions indexed by family                                                                                                                                                                                                                          |
| `script_reference_info`          | reference_hash + hash_type (33B)                                              | ScriptReferenceInfo                                                  | Script reference aggregate stats (cell/capacity counts per lock/type)                                                                                                                                                                                      |
| `script_reference_to_version`    | reference_hash + hash_type (33B)                                              | version_hash                                                         | Script reference to version mapping                                                                                                                                                                                                                        |
| `script_family_by_name`          | family_name (string)                                                          | family_id                                                            | Reverse lookup: family name -> family ID                                                                                                                                                                                                                   |
| `sync_meta`                      | fixed keys                                                                    | Typed records / JSON monitoring bytes                                | Tip/status/runtime/progress/memory, reorg/deep-fork state, bulk session marker, background tasks, network identity, and genesis economic baseline                                                                                                          |
| `dob_decoded`                    | spore_id (32B)                                                                | DecodeOutcome (Decoded(DobDecodedEntry) \| Failed(DobDecodeFailure)) | Cached CKB-VM DOB decode outcome (bulk-disabled, populated after sync catches up to tip). Failed is written only for deterministic failures; transient RPC failures are not persisted.                                                                     |
| `lock_scripts`                   | lock_hash (32B)                                                               | LockScriptEntry                                                      | Lock script components by hash (survives cell consumption for address resolution)                                                                                                                                                                          |
| `net_nodes` **(network store)**  | peer_id (raw bytes)                                                           | NodeRecord                                                           | TTL-retained same-network crawler-Identify records, latest dialability, and exact Discovery evidence                                                                                                                                                       |
| `net_stats` **(network store)**  | `0x00` singleton, or metric(1B)+gran(1B)+bucket(8B BE)                        | LatestStatus / HistoryPoint                                          | Checked completed dial/session/Discovery aggregates plus time-bucketed verified/reachable/share history                                                                                                                                                    |
| `net_crawl` **(network store)**  | `0x00` singleton, or `0x01` + peer_id                                         | ActiveCrawl / CrawlCandidate                                         | Durable logical-round state: dial aliases, target-centric advertisements, direct sessions, active probes, and stable completed evidence                                                                                                                    |

### Cell-by-Code Index Note

`cell_by_lock_code` / `cell_by_type_code` keys carry the script's `hash_type` byte directly after
the 32-byte code hash (75-byte keys: `code_hash(32) + hash_type(1) + block(8 BE) + outpoint(34)`).
Runtime script reference identity is `(reference_hash, hash_type)`, not bare `code_hash`, so each
reference form occupies its own contiguous key range. A reader seeking one form
(`encode_cell_code_index_prefix`) reads exactly that form's rows — a sparse form under a dense code
hash costs its own row count, never the whole code-hash prefix, and pagination inside a form is
exact with no cross-form filtering.

The other cell indexes (`cell_by_lock`, `cell_by_type`, `cell_by_data_hash`) are keyed by a full
script hash or data hash, which already encodes `hash_type`, so they keep the 74-byte
`hash(32) + block(8 BE) + outpoint(34)` shape.

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

### `reorg_undo_log_by_block` Undo Scopes

Key = `block_number(8B BE i64) + seq(8B BE u64)`. The sequence number carries its scope in the
high bits: `seq = (scope << 48) | local`, where `local` counts entries within one block.

| Scope         | Value    | Records                                                                |
| ------------- | -------- | ---------------------------------------------------------------------- |
| `TxContext`   | `0x0001` | Per-tx input/output shape used to derive cell and consumption rollback |
| `DotBit`      | `0x0002` | `.bit` account/identity entity mutations                               |
| `Object`      | `0x0003` | Spore, cluster and mNFT entity mutations                               |
| `EntityStats` | `0x0004` | The eight per-entity daily/hourly stats buckets                        |

The `local` counter is **per block and shared by every writer in one committed batch**
(`SharedUndoSeq`). Spore, mNFT and `.bit` batch states used to own three private counters that all
started at 0 and all stamped `Object`, so a second object write in the same block computed the
identical undo key and silently overwrote the first entry's pre-image (POSTMORTEM IDX-008).

**`EntityStats` scope.** The eight per-entity stats prefixes — `SCRIPT_DAILY` (`stats_script`),
`TOKEN_DAILY` and `TOKEN_HOURLY` (`stats_token`), `CLUSTER_DAILY`, `SPORE_DAILY` and
`SPORE_HOURLY` (`stats_spore`), `OBJECT_DAILY` and `OBJECT_HOURLY` (`stats_mnft`) — are restored
**only** by undo replay. They are not in `STATS_REPLAY_CANDIDATE_PREFIXES` and the rollback cutoff
sweep never deletes them (POSTMORTEM STATS-010). Each entry is a
`KeyMutation { target_store: Domain, cf_name, key, previous_value }` holding that key's value at
the **end of the previous block**, recorded once per `(block, key)` by `EntityStatsOverlay`
(`crates/indexer/src/db/writer/entity_stats.rs`); `previous_value: None` means the row did not
exist and rollback deletes it.

**`addr_prefix_stats` owns no undo scope.** Rollback reverses it by the number of
`addr_txs_by_prefix` rows it deletes — the same shape `addr_balance.txs_count` is reversed from the
`addr_txs` rows it deletes — and then asserts the counter **equals** the surviving rows. An undo
pre-image cannot do this: a live batch spans many blocks, the pre-image is recorded on one of them,
and a fork point on a later block of the same batch would never replay it while still deleting the
rows written after it.

**Bounded window.** Only the `EntityStats` scope is pruned during normal sync. Every live commit
advances the coverage floor to `committed_tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS` (1000 blocks,
`crates/indexer/src/sync/batch.rs`) and deletes the `EntityStats` entries it leaves behind, staged
into the same batch as the blocks, so the floor and the deletions it describes are never
separately durable. The floor is monotonic. `TxContext`, `DotBit` and `Object` entries are never
pruned — they are removed only when replayed. Bulk build records no undo entries at all.

### `sync_meta` Fixed Keys

`sync_meta` belongs to the **domain store** and is written only by the indexer. Its fixed-key
namespace includes:

- canonical progress/state: `tip_block`, `sync_status`, `runtime_status`, `sync_progress`,
  `memory_stats`, and `background_tasks`
- rollback state: `rollback_cleanup_in_progress`, latest/history reorg records, and `deep_fork`
- bulk-build state: batch/session-in-progress markers
- materialization trackers: HODL and cell-distribution trackers
- chain identity: `network_identity`, persisted at first sync and validated on later starts
- exact economics: `genesis_baseline` (`GenesisBaseline { total_issuance, burnt,
virtual_occupied }`), derived from block 0 and used by supply, APC, and knowledge-size paths
- exact live-cell inventory: `live_cell_summary:initialized`, `live_cell_summary:current`, plus
  `live_cell_summary:history:<block_be_i64>`. Each value is a fixed-width 72-byte
  `LiveCellSummary` (`tip block/hash` + four `u64` counters). History retains up to 37 block-end
  snapshots (current plus the maximum 36-block automatic reorg depth).
- entity-stats rollback coverage: `entity_stats_undo_contract`
- hourly retention evidence: `hourly_retention_state:<family>`

#### `entity_stats_undo_contract`

Value is `EntityStatsUndoContract { version, coverage_floor_block, updated_at_block }`.
`coverage_floor_block` is the lowest block a shallow reorg can still be undone to — `EntityStats`
undo entries at or below it have been pruned; `updated_at_block` is the committed tip when that
floor was last advanced, so a stale floor is distinguishable from a current one.

Who writes it, and when:

- **Bulk completion** writes it once, with `coverage_floor_block` = the **handoff tip**, the last
  block bulk actually wrote — never the chain tip bulk was racing. The two differ by up to
  `bulk_sync_threshold`, and a floor above what the store holds would claim coverage over blocks
  that were never written. See `docs/prompts/BULK_SYNC.md` rule 12.
- **Live sync** rewrites it in any commit that advances the floor, in the same batch as the
  blocks. A fresh store that never ran bulk gets its first contract at its first live commit.

All three rollback entry points — live reorg, startup cleanup, and partial-batch cleanup — refuse
a target below `coverage_floor_block` with a rebuild-required error rather than rolling back
buckets they cannot restore. A store with a tip but no contract is refused at startup: it was
written by a build that deleted these buckets instead of undoing them, so there is no honest
migration.

#### `hourly_retention_state:<family>`

One row per hourly family that has a retention policy — `hourly_retention_state:token` and
`hourly_retention_state:mnft` — holding
`HourlyRetentionState { policy_version, family, executed_cutoff_hour, round_in_progress_cutoff_hour, cursor, round_started_at, round_completed_at }`.

- `executed_cutoff_hour` is the **only** trustworthy boundary: every bucket below it is gone,
  every bucket above it is either present or was never written. It advances only when a round
  reaches the end of its family, and is monotonic — a clock that goes backwards un-deletes
  nothing.
- `round_in_progress_cutoff_hour` and `cursor` describe an in-flight round and are diagnostic
  only: below that cutoff, deletions have happened just up to `cursor`.
- Deletions and the state row are staged into the same block batch by the writer, so the
  persisted boundary always matches what was actually deleted.

This row is the evidence that a missing hourly bucket is legitimate retention rather than
corruption. A reader that cannot see it must report `unknown`, never "zero".
`SPORE_HOURLY` has **no retention policy and no row** — its buckets are never expired. Identity
(`.bit` sentinel) rows under `OBJECT_HOURLY` are excluded for the same reason; only mNFT classes
expire, resolved through `cf_mnft_collection_agg` via the padded 32-byte collection id.

The live-cell summary is mutable canonical state, so it belongs to the domain store. Normal sync
updates it in the same atomic batch as block headers and `sync_status`; bulk-build keeps only the
four global counters and up to 37 snapshots in memory, then publishes them with the final status.
Reorg restores a retained target snapshot and deletes orphan snapshots. Neither API reads nor
recovery scan `live_cells`/`cells`, and `CF_CELLS` is never written by this feature.

Missing or conflicting identity/baseline state is an invariant failure. API readers must not
invent a replacement value or write this CF.

### Bulk-Build Sealed Aggregate Note

During fresh-db bulk sync, the indexer writes `stats_dao`, `stats_script`, and `stats_token`
inline as Class C sealed aggregates:

- `stats_dao` stores DAO daily snapshots keyed by date, then refreshes the latest/top summary rows
  after sync tip metadata is finalized.
- `stats_script` stores `ScriptDailyDelta` rows keyed by
  `code_hash + hash_type + kind(lock/type) + YYYYMMDD` — the hash_type byte keeps
  references that share code_hash bytes but differ in hash_type (data/type/data1/data2)
  on independent daily timelines.
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

The **network store** (`[store].network_data_path`, default `data/network`, CFs `net_nodes` + `net_stats` + `net_crawl`) is a distinct third RocksDB store class holding whole-network CKB L1 crawler observations, configured-local-node session observations, and resumable crawl state. This remains exactly 3 network CFs and 67 CFs overall; the richer evidence model adds no CF. Unlike the two chain stores it is:

- **Non-chain / non-deterministic** — contents are derived from live peer-to-peer observation
  (crawler Identify/Discovery probes and configured-node `local_node_info`/`get_peers` sessions),
  not from deterministic block replay.
- **TTL-retained** — node records and history rollups are pruned on a rolling retention window; it is not a permanent append log.
- **The only store class EXEMPT from rebuild-from-genesis** — it cannot be reconstructed by replaying the chain, so deleting/rebuilding chain data does not touch it.
- **Single-writer** — written exclusively by the opt-in `ckbadger-crawler` service (`ckbadger crawl`; enabled via `[crawler].enabled`, default `false`). The indexer never writes it. Read consumers (API) open it secondary (read-only), the same access model as the chain stores.

### `net_nodes`

Key = raw `peer_id` bytes → `NodeRecord`. A record exists only after an outbound crawler probe's
authenticated peer returns a valid Identify for the configured CKB network. Fields are
`own_addrs`, `client_version`, `flags`,
`protocols`, `first_seen`, `last_seen`, `last_reachable_at`, latest completed-round `reachable`,
optional `geo`/`asn`/`last_rtt_ms`, exact `DiscoveryEvidence`, and `known_peers` resolved from the
last Discovery observation. `known_peers` is source-centric address-book gossip, not a live edge;
the durable source for a detail response's advertisers is the target candidate described below.
`DiscoveryEvidence` separately counts all valid `Nodes` messages, regular responses, announces,
malformed/unexpected messages, normalized advertised addresses, and rejected addresses; checked
validation requires responses plus announces to equal total valid `Nodes` messages.

### `net_stats` key layout

- `0x00` (single reserved byte) → `LatestStatus` singleton — latest completed round id/times;
  `CompletedPeerOutcomes`; `AddressObservationHistogram`; aggregate `DiscoveryEvidence`;
  `malformed_addresses`; `new_verified_peers`; the longitudinal `local_observer`; and the current
  completed round's exact `direct_session_observations` split into `observer_initiated` and
  `peer_initiated`. Candidate, retained, reachable, unavailable, exhausted, foreign, and
  address-attempt totals are checked projections from the two dial matrices, not separately
  persisted counters.
- `metric(1B) + granularity(1B) + ts_bucket(8B big-endian)` → `HistoryPoint` — time-bucketed
  rollups. Metric ids are `VerifiedPeers=1`, `ReachablePeers=2`, `VersionShare=3`, and
  `CountryShare=4`; granularities are hour/day. Big-endian buckets preserve chronological key
  order. The numeric ids for the first two metrics are unchanged, but the serialized network
  schema and public names are intentionally breaking.

### `net_crawl` key layout

- `0x00` → `ActiveCrawl` singleton — current logical round id, start/checkpoint times, exact active
  address-observation histogram, independent `alias_freshness_cutoff` and
  `direct_session_freshness_cutoff`, staged `local_observer_observation`, sorted
  `direct_session_targets`, scheduling sequence, malformed-address count, and actionable blocked
  reason. Presence of the observer observation is the durable marker that the round sampled RPC
  exactly once.
- `0x01 + peer_id` → `CrawlCandidate` — retained `CrawlAddress` dial aliases, target-centric
  `AdvertisementEvidence`, current-round `staged_direct_sessions`, completed longitudinal
  `direct_sessions`, fairness sequence, optional resumable `ActiveCandidateProbe`, and optional
  immutable `CompletedCandidateEvidence` for the last completed round. Each address observation
  includes address, round/time, exact elapsed milliseconds, and typed `AddressProbeResult`.

`AdvertisementEvidence` is keyed canonically within the target candidate by
`(advertiser_peer_id, alias)`. It preserves exact first/latest positive-observation times,
first/latest completed rounds, and count. A later randomized Discovery payload's omission is not
negative evidence and does not erase the prior fact. Alias TTL expiry removes evidence referring
to that alias. This target-centric layout answers “who advertised this peer?” with one candidate
lookup and no new CF.

`DirectSessionEvidence` is target-centric and keyed canonically by
`(observer_peer_id, initiator)`. It preserves exact first/latest positive-observation times and
rounds, observation count, and latest client version, session addresses, connected/ping durations,
and protocol rows. `get_peers.is_outbound` is interpreted from the configured local CKB observer's
vantage: `true` means the observer initiated the session; `false` means the remote peer initiated
it. A session may have no reusable address and remains valid evidence. Addresses reported for an
RPC session describe that connection only (an inbound source port may be ephemeral), so they are
stored only as session evidence and are never promoted to `CrawlAddress` dial aliases.
Missing a peer from a later `get_peers` snapshot is not negative evidence. Only the independent
direct-session time cutoff expires a completed fact; neither advertisement time nor successful
crawler dialing refreshes it.

`LocalObserverEvidence` similarly preserves exact first/latest observation times and rounds,
observation count, and the latest `local_node_info` client version, active flag, advertised
addresses, supported protocols, and connection count. It describes the configured CKB observer,
not a crawler probe result.

A slice checkpoint atomically updates `ActiveCrawl` and changed candidates in `net_crawl`; new
advertisements and direct sessions remain staged, and the checkpoint does not erase durable prior
evidence or `last_completed`. Partial slices never modify the published `net_nodes` snapshot or
`net_stats` status. Once every schedulable dial candidate is terminal, the crawler moves active
probe evidence to `last_completed`, merges staged positive advertisement/direct-session
observations into the target candidates, and one RocksDB batch publishes candidate
updates/deletes, verified-node changes, checked status/history, and deletion of the active
singleton. Addressless direct-only candidates never acquire dial-probe state.

Before building that batch, `commit_crawl_round` validates the candidate evidence through the same
checked classification helpers used by the crawler. It rejects unknown/duplicate aliases,
outcome/result disagreement, duplicate peer deltas, new records without same-network evidence,
per-peer reachability drift, matrix/snapshot drift, Discovery drift, and overflow. Every current-
round candidate publication must also be the exact terminal `active` → `last_completed` transition
from its persisted checkpoint; the persisted active histogram, rebuilt candidate histogram, and
status histogram must agree. A store-owned checked alias index is the single path used by both the
crawler and commit validator to resolve staged Discovery addresses into sorted/deduplicated
`known_peers`, while separate target-centric validators reconstruct staged advertisements and
direct sessions and their canonical merges. Staged success uniquely fixes every published node
field except Geo/ASN; retained exhausted/foreign nodes may change only `reachable` to false.
Observation times must lie inside the
durable round clock and the successful address timestamp must equal the staged-success timestamp.
An inactive candidate keeps its previously published evidence through every partial checkpoint.
At the following completed-round commit, the crawler applies the exact alias/advertisement and
direct-session TTL transitions, then retains the candidate only while a verified node or positive
evidence remains; otherwise that same atomic commit deletes it. This keeps the previous completed
view inspectable until a replacement completed view is ready.
Participation, session-initiation direction, and crawler dialability remain orthogonal facts; the
store does not derive NAT/firewall status, “home node”, or global reachability from them. Before
accepting `local_node_info`/`get_peers`, the crawler verifies `get_block_hash(0)` against the exact
configured-network genesis hash and publishes nothing from a mismatched RPC node.

Readers therefore observe either the previous completed round or the next internally coherent
completed round. The crawler is the only writer of all three network CFs; the API remains a
read-only secondary. If the API starts before the store exists, it keeps an empty read-only slot and
retries the secondary open; crawler creation becomes visible without an API restart.

This serialized schema is not backward compatible. Recreate only the network primary and its API
secondary (default mainnet paths `work/mainnet/data/network` and
`work/mainnet/data/network-api-secondary`) and crawl again. Do not delete or re-sync the domain or
append-only chain stores.

## Key Design

- `addr_txs` is keyed `lock_hash(32) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32)` = 76B and
  `addr_txs_by_prefix` `lock_hash_prefix(20) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32)` =
  64B. Two fixed-width CFs rather than one tagged key: every length assert on the full-hash index
  stays exact, and `list_addr_txs_recent` is a plain descending merge of the two scans. A given
  (party, tx) pair reaches exactly one of them — the activity builder merges a named prefix into
  the cell owner it matches — so the merge has nothing to dedupe and treats a collision as the
  upstream bug it is.
- `CkbadgerStore::open_domain(path)` / `open_append_only(path)` — primary read-write mode for indexer and maintenance commands (split domain + append-only)
- `CkbadgerStore::open_domain_secondary(primary_path, secondary_path)` / `open_append_only_secondary(primary_path, secondary_path)` — read-only mode for API/TUI (split secondary stores)
- All store operations are synchronous (RocksDB reads are fast)

## Read Consistency (secondary readers)

A secondary cannot take snapshots (`GetSnapshot` fails with "snapshot not supported in secondary
mode"), and its view advances _only_ when `try_catch_up_with_primary()` runs. Without a read view,
any read that resolves an index row and then loads the row it points at spans two views when a
catch-up lands in between: the iterator, pinned at creation, still yields the pre-catch-up index
row while the point lookup already sees the post-catch-up entry.

`crates/ckbadger-store/src/read_view.rs` makes catch-up — the single mutation point of a reader's
view — exclusive with pinned read scopes, restoring per process the guarantee `snapshot()` gives on
a primary, which every multi-CF read path already assumes:

- `CkbadgerStore::refresh` runs inside a `CatchUpWindow`; `catch_up_in_window()` takes that window
  by reference, so "all secondaries advance together" is checked at compile time. Refresh order
  between the domain and append-only stores is therefore invisible to readers.
- The API pins one read view per HTTP request (innermost middleware), so a response can never mix
  two views.
- Deliberately **not** pinned: handlers that wait for the indexer to write new data (the cycles
  long-poll releases its pin before waiting — its contract is to observe the _next_ view), plus
  background full-store scans (cache warmup) and WebSocket broadcasters, which interleave external
  I/O with minutes-long scans and accept drift rather than freeze catch-up.
- Both sides are bounded: a read scope arriving after a catch-up queues yields to it for 100ms and
  then pins anyway, and a catch-up held up by a read scope logs the stall every 5s.
- Never hold a read view across `CkbadgerStore::refresh` on the same thread — that self-deadlocks.
  Catch-up runs on its own thread (`spawn_blocking` in the API), never inside a pinned scope.

## Memory Considerations

Memory sizing is per network rather than a fixed host-wide peak:

- `[store].memory_budget_gb`, when set, is an explicit per-network RocksDB budget and is never
  divided again.
- Without an override, ckbadger divides detected host RAM by the governing orchestrator's
  co-resident network count. A standalone single-network work directory has count 1.
- The domain and append-only RocksDB instances inside one process share one block cache and one
  WriteBufferManager. Store-local memtables/table readers are summed, while shared resources are
  counted once.
- `[indexer].bulk_memory_budget_gb` optionally caps whole-process `VmRSS + VmSwap` on Linux or
  process physical footprint on macOS during bulk build; otherwise the per-network RAM share is
  used.

### Live Write Buffers and Flush Frequency

`atomic_flush = 1` means any single CF hitting its write buffer switches memtables for the
**whole** database, so the smallest per-CF buffer decides how often all 63 CFs flush together.

- Per-CF write buffers come from the memory profile, in three tiers: `write_buffer_mega_bytes`
  (base 256 MB, clamped 64 MB-1 GB), `write_buffer_high_bytes` (base 128 MB, clamped 32-512 MB)
  and `write_buffer_low_bytes` (base 32 MB, clamped 8-128 MB), each scaled by
  `wbm_normal_bytes / 8 GB`. `live_cf_write_buffer(name, profile)` is the one function used by both
  the open-time CF options and the live profile restored by `apply_normal_compaction_options`, so
  a CF cannot be opened at one size and live-tuned to another. Live sync no longer pins the tiers
  to fixed 8/4/2 MB — that is what produced the 2026-09-22 flush storm (POSTMORTEM `IDX-007`).
- `max_write_buffer_number` is unchanged: 4 for the mega and high tiers, 2 for the low tier.
- The live WriteBufferManager cap is unchanged at **384 MB** (`LIVE_WBM_CAP_BYTES`, applied as
  `min(wbm_normal_bytes, 384 MB)`). Total memtable memory stays governed by that cap rather than by
  per-CF minimums, so raising the per-CF tiers does not raise the ceiling.
- `max_manifest_file_size` is 64 MB (`configured_options`). The MANIFEST is an append-only log of
  every version edit and RocksDB only rolls it once it exceeds this size; the 1 GB default never
  rolled. The option matters to the **primary only** — a secondary never writes a MANIFEST, it
  replays the primary's — so a secondary opens faster because the primary's MANIFEST is bounded,
  not because the option takes effect on the secondary.

### Append-Only Commit Probe

A non-bulk commit to the append-only store probes every put key for an existing value to enforce
replay idempotency. That probe is one `multi_get_cf` per 4,096-key chunk
(`APPEND_PROBE_CHUNK_KEYS` in `batch.rs`), not one `get_cf` per key — during the 2026-09-22
catch-up it was 14,000-17,000 point reads per batch, inside the commit window.

The append-only invariants are unchanged: duplicate keys within one batch fail before the probe
runs; an existing key with the same value is skipped; an existing key with a different value fails
the batch; deletes still go through `validate_append_delete_by_cf_name`. The chunking exists so
peak memory is bounded by the chunk rather than by the batch, because the probe materializes the
existing value of every key it asks for. Bulk sync is constrained to fresh-DB rebuilds and skips
the probe entirely.

### Flush and MANIFEST Diagnostics

`memory_stats()` reports two exact standing consequences of flush behaviour:

| Field             | Meaning                                                                                                                                                                                                                                                                        |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `sst_files_total` | SST files summed over L0-L6 of every CF. Recomputed at most every 30 s (`SST_FILE_COUNT_MAX_AGE`): the sweep is ~420 property reads per store under the DB mutex, and `memory_stats()` is sampled every 3 s during bulk sync.                                                  |
| `manifest_bytes`  | Size of the MANIFEST named by this store's `CURRENT` file; 0 when it cannot be read (a just-created directory, or a rotation between the two reads). `MemoryStatsData` additionally carries `domain_manifest_bytes`, since the domain MANIFEST is the one a secondary replays. |

Flush **round** counts are deliberately not exposed. RocksDB has no cumulative flush property
(`num-running-flushes` and `mem-table-flush-pending` are instantaneous values), so a sampled gauge
cannot see a millisecond-scale flush. Flush rounds are counted from `flush_started` in the RocksDB
LOG.

## Config Keys

| Parameter                         | Default            | Description                                                                    |
| --------------------------------- | ------------------ | ------------------------------------------------------------------------------ |
| `[store].domain_data_path`        | `data/domain`      | Domain RocksDB data directory                                                  |
| `[store].append_only_data_path`   | `data/append-only` | Append-only RocksDB data directory                                             |
| `[store].network_data_path`       | `data/network`     | Network-crawler RocksDB data directory (opt-in; written by `ckbadger-crawler`) |
| `[store].memory_budget_gb`        | auto               | Explicit per-network RocksDB RAM budget; otherwise divide detected host RAM    |
| `[indexer].bulk_memory_budget_gb` | auto               | Optional whole-indexer bulk-sync memory cap                                    |

```toml
[store]
domain_data_path = "/ssd/ckbadger-store"
append_only_data_path = "/ssd/ckbadger-store-append-only"
network_data_path = "/ssd/ckbadger-store-network"
# memory_budget_gb = 32

[indexer]
# bulk_memory_budget_gb = 32
```
