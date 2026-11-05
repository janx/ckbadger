# ckbadger-store Column Families (75 total: 71 domain + 1 append-only + 3 network)

ckbadger runs three logical RocksDB store classes (all backed by `ckbadger-store`):

- **Domain store** (`[store].domain_data_path`, 71 CFs) — canonical chain view, all mutable state including activities, addr_txs, live/consumed cell markers, indexes, stats, and aggregates. May perform create/update/delete as required by chain progression and reorg handling.
- **Append-only store** (`[store].append_only_data_path`, 1 CF: `cells`) — immutable cell payloads, content-addressed by outpoint. Write-once, never updated or deleted.
- **Network store** (`[store].network_data_path`, 3 CFs: `net_nodes`, `net_stats`, `net_crawl`) —
  crawler p2p probes, configured-local-node session observations, and durable in-progress crawl
  state: non-chain, non-deterministic, TTL-retained. Written solely by the opt-in
  `ckbadger-crawler` service; it is the **only store class EXEMPT from rebuild-from-genesis**. See
  the [Network Store](#network-store) section below.

The indexer opens the two chain stores (domain + append-only) read-write and the API opens them secondary (read-only). The network store follows the same sole-writer + secondary-reader model: the crawler opens it read-write (sole writer), read consumers (API) open it secondary (read-only). Cell reads are cross-store: live/consumed markers in domain, cell payloads in append-only.

`block_headers` rows in the domain store require `CachedBlockHeader.chain_time_ms`, the running maximum of raw header timestamps from genesis through that block. The raw `timestamp` remains available for chain payloads and display. All indexed day/hour buckets, completed-bucket reads, and chain-time searches use `chain_time_ms`. Old serialized headers have no compatible default and must be rebuilt from genesis; mixing old headers with new statistics is invalid. Daily block statistics persist exact `difficulty_sum: u128` rather than a rounded floating-point average. Re-sync rebuilds these domain rows and aggregates; it does not mutate `cells` in the append-only store or the crawler's network store.

`dao_deposits` also requires `DaoDepositCacheEntry.deposit_chain_time_ms`, the deposit
block's canonical running-maximum time. Live and bulk indexer writers persist it at creation;
DAO refresh reads this field rather than reloading every deposit header. The raw
`deposit_timestamp` remains available for display. This domain-store serialization change
requires re-sync from genesis; there is no default, migration, or append-only payload mutation.

`cells` values are the `LiveCellInfo` v2 layout (2026-11-05), in this field order:
`capacity` i64, `lock_code_hash` `[u8; 32]`, `lock_hash_type` i16, `lock_args` bytes,
`type_code_hash` `Option<[u8; 32]>`, `type_hash_type` `Option<i16>`, `type_args` `Option<bytes>`,
`data_size` i32, `udt_amount` `Option<u128>`, `data_hash` `Option<[u8; 32]>` (a plain secp256k1
cell encodes in 101 bytes). They are written only by `LiveCellInfo::encode` and read only by
`LiveCellInfo::decode` (bincode `DefaultOptions`: varint integers, little endian, trailing bytes
rejected; the type implements no serde traits, so no second codec exists). Nothing derivable from
these fields is stored: `lock_script_hash()` / `type_script_hash()` derive the script hashes with
`ckbadger_common::script_hash`, and `occupied_capacity()` derives the occupied capacity from the
args and data lengths with `ckbadger_common::dao::occupied_capacity_shannons`, the routine the
DAO parser, the live writer and bulk facts also call. Index keys that embed a script hash are unchanged: the live
writer takes them from the parser, which uses that same routine; bulk takes them from ckb-types
`calc_script_hash()` over the raw script bytes (`binary_facts.rs`), which bulk requires to be the
canonical Molecule encoding of those fields (anything else fails with its block, tx index and
output index), so both give the same bytes. A type script is stored whole or not at all and the
data size is never negative: `encode` refuses anything else, and `decode` rejects it, as well as a
short, padded or v1 payload and one that still stores the occupied capacity, with an error
carrying its byte length. The CF is lz4-compressed (universal compaction). Changing this layout
requires re-sync from genesis; there is no migration.

## Column Families

| Column Family                    | Key                                                                           | Value                                                                          | Purpose                                                                                                                                                                                                                                                    |
| -------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cells` **(append-only store)**  | tx_hash + output_index (34B)                                                  | LiveCellInfo v2 (see `cells` payload above)                                    | Immutable cell payload store (write-once, content-addressed)                                                                                                                                                                                               |
| `live_cells`                     | tx_hash + output_index (34B)                                                  | empty                                                                          | Live UTXO pointer set                                                                                                                                                                                                                                      |
| `consumed_cells`                 | tx_hash + output_index (34B)                                                  | ConsumedCellMeta                                                               | Consumed pointer + consume metadata                                                                                                                                                                                                                        |
| `reorg_undo_log_by_block`        | block (8B BE) + seq (8B BE)                                                   | journal entry: cf name, key, previous value                                    | The block journal: each live block's previous values of the `Journal`-class keys it wrote (see [below](#reorg_undo_log_by_block-the-block-journal))                                                                                                        |
| `block_headers`                  | block_number (8B)                                                             | CachedBlockHeader                                                              | Block header + DAO field cache                                                                                                                                                                                                                             |
| `block_hash_index`               | block_hash (32B)                                                              | block_number (8B)                                                              | Reverse lookup: hash -> number                                                                                                                                                                                                                             |
| `cell_by_lock`                   | lock_script_hash + outpoint                                                   | empty                                                                          | Cell index by lock script                                                                                                                                                                                                                                  |
| `cell_by_type`                   | type_script_hash + outpoint                                                   | empty                                                                          | Cell index by type script                                                                                                                                                                                                                                  |
| `cell_by_lock_code`              | lock_code_hash (32B) + hash_type (1B) + block (8B BE) + outpoint              | empty                                                                          | Cell index by lock script reference form `(code_hash, hash_type)`                                                                                                                                                                                          |
| `cell_by_type_code`              | type_code_hash (32B) + hash_type (1B) + block (8B BE) + outpoint              | empty                                                                          | Cell index by type script reference form `(code_hash, hash_type)`                                                                                                                                                                                          |
| `cell_by_data_hash`              | blake2b(cell_data) + outpoint                                                 | empty                                                                          | Cell index by data hash (code cell resolution)                                                                                                                                                                                                             |
| `tx_index`                       | block_number + tx_index                                                       | TxIndexEntry { tx_hash: [u8; 32], fee, size, counts, cycles, tags, timestamp } | Domain transaction order and canonical hash pointer; written by the indexer in live and bulk paths                                                                                                                                                         |
| `tx_hash_map`                    | tx_hash (32B)                                                                 | block_number + tx_index                                                        | Reverse lookup: tx_hash -> position                                                                                                                                                                                                                        |
| `addr_balance`                   | lock_script_hash (32B)                                                        | AddressBalance                                                                 | Address balance, cell counts, `txs_count` and `cellbase_txs_count` (the cellbase txs among them; activity total = `address_tx_count - cellbase_txs_count`)                                                                                                 |
| `addr_txs`                       | lock_hash + block + tx_index + tx_hash                                        | AddrTxValue                                                                    | Address transaction thin index with capacity change, tx flags, and participant activity tags                                                                                                                                                               |
| `addr_txs_by_prefix`             | lock_hash_prefix(20) + block_num_desc(8) + tx_idx_desc(4) + tx_hash(32) = 64B | AddrTxValue                                                                    | Same thin index for a party a protocol NAMED by its 20-byte lock-hash prefix and that holds no cell in the transaction                                                                                                                                     |
| `addr_prefix_stats`              | lock_hash_prefix(20)                                                          | AddrPrefixStats                                                                | How many transactions named this prefix without it holding a cell; added to `addr_balance.txs_count` by `address_tx_count`                                                                                                                                 |
| `dao_deposits`                   | tx_hash + output_index (34B)                                                  | DaoDepositCacheEntry                                                           | DAO lifecycle plus original capacity, exact occupied capacity, deposit/request ARs, and claimed compensation                                                                                                                                               |
| `dao_by_withdraw_tx`             | withdraw_outpoint (34B)                                                       | deposit outpoint                                                               | Reverse lookup: withdraw outpoint -> deposit                                                                                                                                                                                                               |
| `dao_by_block`                   | block_desc (8B BE) + outpoint (34B)                                           | empty                                                                          | DAO index ordered by deposit block DESC                                                                                                                                                                                                                    |
| `dao_by_lock_block`              | lock_hash (32B) + block_desc (8B BE) + outpoint (34B)                         | empty                                                                          | DAO index by lock + deposit block DESC                                                                                                                                                                                                                     |
| `dao_by_status_block`            | status (2B BE) + block_desc (8B BE) + outpoint (34B)                          | empty                                                                          | DAO index by status + deposit block DESC                                                                                                                                                                                                                   |
| `tokens`                         | type_script_hash (32B)                                                        | TokenInfo                                                                      | UDT metadata only; total supply and holder count derive from `token_holders`                                                                                                                                                                               |
| `token_holders`                  | type_hash (32B) + lock_hash (32B)                                             | TokenBalance (32B unsigned BE)                                                 | Exact aggregate holder balances; values may exceed a single cell's u128 amount                                                                                                                                                                             |
| `token_holders_by_balance`       | type_hash (32B) + complemented balance BE (32B) + lock_hash (32B)             | empty                                                                          | 96B key; token holders ranked by balance DESC, lock hash ASC                                                                                                                                                                                               |
| `addr_tokens_by_balance`         | lock_hash (32B) + complemented balance BE (32B) + type_hash (32B)             | empty                                                                          | 96B key; address token balances ranked by balance DESC, type hash ASC                                                                                                                                                                                      |
| `token_transfers`                | type_hash + block + tx_index                                                  | TransferInfo                                                                   | Token transfer records                                                                                                                                                                                                                                     |
| `spore_data`                     | spore_id (32B)                                                                | SporeData                                                                      | Spore NFT metadata                                                                                                                                                                                                                                         |
| `spore_by_cluster`               | cluster_id + spore_id                                                         | empty                                                                          | Spore index by cluster                                                                                                                                                                                                                                     |
| `mnft_data`                      | object_id                                                                     | ObjectEntry                                                                    | mNFT metadata (issuer/class/token)                                                                                                                                                                                                                         |
| `mnft_by_collection`             | collection_id + object_id                                                     | empty                                                                          | mNFT index by collection                                                                                                                                                                                                                                   |
| `identity_data`                  | identity_id (20B AccountCell / `.cell` name; 32B .bit Cell/did:ckb)           | IdentityEntry                                                                  | Identity metadata with separate standards and lifecycles for .bit AccountCell, .bit Cell, did:ckb and `.cell` names                                                                                                                                        |
| `mnft_collection_agg`            | collection_id                                                                 | MnftCollectionAggregate                                                        | mNFT collection aggregate stats                                                                                                                                                                                                                            |
| `object_collection_activities`   | collection_id + block + tx                                                    | ObjectCollectionActivityEntry                                                  | Pre-computed object collection activity feed                                                                                                                                                                                                               |
| `identity_by_collection`         | collection_id + identity_id                                                   | empty                                                                          | Identity index by collection. A `.cell` sub-name is indexed twice: once under the `.cell` sentinel, once under its parent's padded 20-byte id, which is what lists a name's children (see [below](#identity_by_collection-cell-parentchild-rows))          |
| `identity_agg`                   | collection_id (sentinel 32B)                                                  | IdentityCollectionAgg                                                          | Per-standard identity aggregates; .bit AccountCell and .bit Cell use different sentinels                                                                                                                                                                   |
| `identity_collection_activities` | collection_id + block + tx                                                    | ObjectCollectionActivityEntry                                                  | Pre-computed identity collection activity feed (domain)                                                                                                                                                                                                    |
| `collection_activity_daily`      | collection_id (32B padded) + day YYYYMMDD (4B BE) = 36B                       | CollectionActivityDailyCounts                                                  | A collection's mint / transfer / burn transactions per UTC+8 day: the insights weekly calendar folds these rows; see the note below                                                                                                                        |
| `stats_identity`                 | collection_id + owner segment (32B)                                           | i64 (owner count)                                                              | Per-owner identity counts by collection. The owner segment is a lock hash for every standard but `.cell`, whose chain-level owner is a 20-byte prefix written as an explicit `owner20 ‖ 0^12` and decoded back by collection — never served as a lock hash |
| `dotcell_name_by_owner`          | owner_hash20 (20B) + name_id (20B) = 40B                                      | empty                                                                          | The `.cell` names a 20-byte owner prefix holds: address pages, holder counts, and the sale-state join                                                                                                                                                      |
| `dotcell_ring`                   | namespace_args (20B)                                                          | DotCellRingRoot                                                                | One row per `.cell` namespace: the uniqueness ring's root cell, so verify can walk the ring without scanning. A network runs exactly one namespace                                                                                                         |
| `activities`                     | block_num_desc + tx_idx_desc + tx_hash (44B)                                  | TxActions                                                                      | One canonical per-tx activity record; TX-level protocol/type/lock actions stored once plus sorted participant deltas                                                                                                                                       |
| `tx_actions_by_bucket`           | bucket(1) + block_num_desc(8) + tx_idx_desc(4) = 13B                          | empty                                                                          | Global activity bucket index: one row per non-cellbase `activities` row, keyed by `classify_global_bucket` (`Dao=1 … Ckb=7`). Serves filtered `/activities` and `/transactions` pages without scanning; see the note below                                 |
| `pending_proposals`              | proposal_id (10B hex string)                                                  | CachedProposal (JSON)                                                          | Ephemeral pending proposal cache (live sync only); rollback class `Never`: written out of band from mempool snapshots, a proposal of an orphaned block stays until committed or expired                                                                                                                                                                                                          |
| `fiber_channels`                 | channel_id (32B blake2b of funding outpoint)                                  | FiberChannel                                                                   | Fiber Network channel registry; funding lock args are descriptive and are not unique                                                                                                                                                                       |
| `fiber_channel_by_commitment`    | commitment_hash                                                               | channel_id (32B)                                                               | Fiber channel index by commitment                                                                                                                                                                                                                          |
| `addr_fiber_channels`            | lock_hash (32B) + channel_id (32B)                                            | empty                                                                          | Address-to-Fiber-channels index                                                                                                                                                                                                                            |
| `cluster_agg`                    | cluster_id                                                                    | ClusterAgg                                                                     | Spore cluster aggregate stats                                                                                                                                                                                                                              |
| `script_info`                    | code_hash (32B)                                                               | ScriptInfo                                                                     | Legacy/compatibility script metadata keyed by bare hash                                                                                                                                                                                                    |
| `stats_chain`                    | prefixed keys                                                                 | chain chart snapshots                                                          | Daily/hourly/epoch/miner/block stats (DailyActivityStats includes protocol_action_counts)                                                                                                                                                                  |
| `stats_dao`                      | prefixed keys                                                                 | DAO snapshots                                                                  | DAO daily snapshots (including exact unclaimed and frozen phase-1 compensation), plus latest/top summaries; sealed aggregates in bulk build                                                                                                                |
| `stats_hodl`                     | prefixed keys                                                                 | HODL/chart snapshots                                                           | HODL waves, cell distribution, address cohorts                                                                                                                                                                                                             |
| `hodl_capacity_by_date`          | creation day `YYYYMMDD` (8B ASCII, UTC+8)                                     | i128 LE (16B), > 0                                                             | HODL wave tracker: live capacity per cell creation day (a day with no live capacity has no row); scalars in `sync_meta` `hodl_cursor`. Written per block that moves it (B-4)                                                                               |
| `cell_dist_cohort_by_month`      | cohort month `YYYY-MM` (7B ASCII)                                             | (used, balance): 2 × i128 LE (32B), >= 0                                       | Cell-distribution tracker: address cohort accumulator by first-seen month; bucket totals in `sync_meta` `cell_dist_cursor`. Written per block that moves it (B-4)                                                                                          |
| `stats_script`                   | prefixed keys                                                                 | ScriptDailyDelta                                                               | Script daily deltas (per `code_hash` + `hash_type` + lock/type + day; sealed in bulk build)                                                                                                                                                                |
| `stats_token`                    | prefixed keys                                                                 | token rollups + deltas                                                         | Token transfer totals, hourly buckets, and daily deltas (sealed in bulk build)                                                                                                                                                                             |
| `stats_spore`                    | prefixed keys                                                                 | spore rollups/indexes                                                          | Spore/cluster daily + owner/index stats                                                                                                                                                                                                                    |
| `stats_mnft`                     | prefixed keys                                                                 | mNFT rollups/indexes                                                           | mNFT daily + hourly + owner/index stats                                                                                                                                                                                                                    |
| `script_versions`                | version_hash                                                                  | ScriptVersionInfo                                                              | Canonical script code version rows keyed by `H(script_code)`                                                                                                                                                                                               |
| `script_versions_by_label`       | label_len + label_key + version_hash                                          | empty                                                                          | Label-to-version index for named script family lookups                                                                                                                                                                                                     |
| `script_families`                | family_id (string)                                                            | ScriptFamilyInfo                                                               | Script family metadata (groups related script versions)                                                                                                                                                                                                    |
| `script_versions_by_family`      | family_id + version_hash                                                      | empty                                                                          | Script versions indexed by family                                                                                                                                                                                                                          |
| `script_reference_info`          | reference_hash + hash_type (33B)                                              | ScriptReferenceInfo                                                            | Script reference aggregate stats (cell/capacity counts per lock/type)                                                                                                                                                                                      |
| `script_reference_to_version`    | reference_hash + hash_type (33B)                                              | version_hash                                                                   | Script reference to version mapping                                                                                                                                                                                                                        |
| `script_family_by_name`          | family_name (string)                                                          | family_id                                                                      | Reverse lookup: family name -> family ID                                                                                                                                                                                                                   |
| `sync_meta`                      | fixed keys                                                                    | Typed records / JSON monitoring bytes                                          | Tip/status/runtime/progress/memory, reorg/deep-fork state, bulk session marker, background tasks, network identity, and genesis economic baseline                                                                                                          |
| `dob_decoded`                    | spore_id (32B)                                                                | DecodeOutcome (Decoded(DobDecodedEntry) \| Failed(DobDecodeFailure))           | Cached CKB-VM DOB decode outcome (bulk-disabled, populated after sync catches up to tip). Failed is written only for deterministic failures; transient RPC failures are not persisted.                                                                     |
| `lock_scripts`                   | lock_hash (32B)                                                               | LockScriptEntry                                                                | Lock script components by hash (survives cell consumption for address resolution)                                                                                                                                                                          |
| `dao_live_agg`                   | kind(1B) + kind-specific key (see [Derived Aggregates](#derived-aggregates))  | signed components, 16B LE each                                                 | **Derived** (bulk-disabled): live NervosDAO cells by depositor lock, deposit epoch and (deposit, request) epoch, plus one global row                                                                                                                       |
| `object_collection_activity_count` | collection_id (32B, padded)                                                 | `[count]` (16B LE)                                                             | **Derived** (bulk-disabled): `object_collection_activities` rows per collection — the Spore cluster `activitiesCount`                                                                                                                                      |
| `bit_cell_by_account`            | account_id (20B) + `.bit Cell` identity id (32B)                              | `[1]` (16B LE)                                                                 | **Derived** (bulk-disabled): live `.bit Cell` items naming each `.bit` account                                                                                                                                                                            |
| `tx_group_cycles`               | block_number + tx_index (12B, the canonical slot)                             | TxGroupCyclesRecord (Complete(TransactionCycles) \| Failed)                   | Lazy script-group cycles of one transaction, written by the on-demand cycles worker after the block committed (bulk-disabled); `RangeByBlock`: removed with its block on rollback |
| `net_nodes` **(network store)**  | peer_id (raw bytes)                                                           | NodeRecord                                                                     | TTL-retained same-network crawler-Identify records, latest dialability, and exact Discovery evidence                                                                                                                                                       |
| `net_stats` **(network store)**  | `0x00` singleton, or metric(1B)+gran(1B)+bucket(8B BE)                        | LatestStatus / HistoryPoint                                                    | Checked completed dial/session/Discovery aggregates plus time-bucketed verified/reachable/share history                                                                                                                                                    |
| `net_crawl` **(network store)**  | `0x00` singleton, or `0x01` + peer_id                                         | ActiveCrawl / CrawlCandidate                                                   | Durable logical-round state: dial aliases, target-centric advertisements, direct sessions, active probes, and stable completed evidence                                                                                                                    |

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

### `identity_by_collection`: `.cell` parent→child rows

Beside the `(sentinel, identity_id)` row every identity has, a `.cell` sub-name gets a second row
keyed `pad_id_32(parent_id) ‖ child_id` — the parent's 20-byte name id zero-padded to the
32-byte collection slot, then the child's 20-byte id; the value is empty.

- The index lists **every** sub-name ever registered under the parent, recycled
  (`is_live = false`) ones included — the same "ever registered" semantics as the sentinel rows.
  Liveness is decided on read: the API's children listing filters `is_live` and pages by child
  id.
- The forward writers file the row when the sub-name is first inserted (live
  `db/writer/dotcell.rs`, bulk `sync/bulk_build/owners/object.rs`). The live writer writes it in
  its block's scope, so the block journal removes it again when that block is rolled back.

### `activities` value: item deltas

`TxActions` carries one `ItemDelta { item_id, kind, magnitude, negative }` per item a transaction
moves. `kind` is `ItemKind::{Token, Object, Identity(IdentityStandard)}`: an identity delta keeps
the standard the builder emitted it for, because `.cell`, `.bit` and short did:ckb ids are all 20
bytes and the id alone cannot say which standard it belongs to. `ItemKind::tag()` is the one
kind → participant-tag mapping. The value is bincode, so this is a layout change covered by a
re-sync, not migrated.

`TxActions.protocol_actions` carries one `dotbit` action per `.bit` account a DAS transaction's
`ActionData` is about (`db/writer/dotbit_detector.rs`): `{ protocol: "dotbit", action: <ActionData
action verbatim>, metadata: { accountId: "0x<20-byte id>" } }`. Subjects are the accounts only created
or only consumed (registration, recycle) or, when every account is on both sides, all of them (renew,
edit, transfer, …); ring neighbours whose `next` was rewritten are not subjects.

### `tx_actions_by_bucket`: the global activity filter index

`ckbadger_store::classify_global_bucket(&TxActions)` is the one classification behind every
global activity filter (`dao`, `token`, `object`, `identity`, `protocol`, `script`, `ckb`); its
enum discriminant is the key's first byte. Precedence: item-action protocols claim their
class (`dao` actions → Dao, `dotbit` and `dotcell` actions → Identity), then any other protocol action →
Protocol, then the participants' aggregate tags (DAO, token, object, identity), then type/lock
script calls → Script, else Ckb. Cellbase has no row (it has no `activities` row either).

- **Write.** `StoreBatch::put_tx_actions` puts the bucket row in the same batch as the
  `activities` row (live sync), and bulk build pushes it beside the `activities` row in
  `build_history_rows_for_block` (Class A). Both derive the key through `tx_actions_bucket_key`.
- **Read.** `list_tx_actions_page_by_bucket` seeks the bucket from the cursor and loads each
  row's `TxActions` by its 12-byte position prefix. A row without `TxActions`, or one whose
  `TxActions` classifies elsewhere, is an error naming the position. Rows are canonical by
  construction, so the read does no per-row canonicity lookups.
- **Rollback.** The row is written in its block's scope, so the block journal records its
  creation and a rollback deletes it (`Journal` class: unlike `activities`, its key opens with the
  bucket, not the block number). Pinned by the rollback property suite
  (`crates/indexer/tests/rollback_property.rs`).
- **Verify.** `global_activity_bucket_index_consistency` (S32, `docs/TESTING.md`).

### `collection_activity_daily`: a collection's activity per day

One row per (collection, UTC+8 stats day of the block's chain time) on which the collection's
activity rows (`object_collection_activities` / `identity_collection_activities`, the same
32-byte padded collection id) carry a mint, transfer or burn. Value: `CollectionActivityDailyCounts
{ mint_transactions, transfer_transactions, burn_transactions }` (bincode, 24 B). Spore clusters
(and the sole-Spores sentinel), m-NFT classes and the four identity sentinels alike.

- **Rule.** `CollectionActivityDailyCounts::of_actions`: each activity row adds 1 to each of Mint /
  Transfer / Burn its `actions` contain (a row can add to more than one); Recycle / Renew / Update
  add nothing, and a day whose rows add nothing has no row. `CollectionActivityDailyDeltas::add_row`
  folds a row into its `(collection, day)`; both sync paths and the API test fixtures call it.
- **Write.** Live: every site that writes a collection activity row folds it in the same place
  (`ObjectCollectionActivityAccumulator::flush` / `flush_identity`, `resolve_dotbit_tx_activity`,
  the `.cell` row in the activity loop), per block (`CollectionActivityDailyBlockDeltas`);
  `stage_collection_activity_daily` then reads each touched `(collection, day)` once through the
  batch (`StoreBatch::get_current_many`, one `multi_get`) and writes it in the scope of every block
  that adds to it, with its value at the end of that block, into the batch's atomic domain commit.
  Bulk: `build_history_rows_for_block` folds each row it pushes into the block's day;
  `ActivityStatsAccumulator` keeps the open days and writes a day once, when the chain-time
  watermark passes its UTC+8 end (Class C, like `ACTIVITY_DAILY`), the rest at finalize.
- **Rollback.** The block journal writes each row back to its value at the end of the target
  block. Pinned by the rollback property suite (`crates/indexer/tests/rollback_property.rs`).
- **Read.** The insights `weeklyActivity` (`completed_week_calendar`) folds the collection's rows
  into Monday weeks: O(days), no activity, transaction or header read. `/assets/objects/{id}/activities`
  rows carry `chainTime`, the clock their day is taken from.
- **Verify.** `collection_daily_activity_consistency` (S35, `docs/TESTING.md`).

### `addr_txs` value: a closed transaction type

`AddrTxValue { capacity_change, flags, tags }` (both `addr_txs` and `addr_txs_by_prefix`) stores
`flags` as `AddrTxType`, a `#[repr(u8)]` enum on the same single byte as before: `0` received,
`1` sent, `2` internal, `3` transfer, `4` named (a party a protocol named that holds no cell). A
byte outside `0..=4` fails when the row is decoded, naming the byte, and every reader propagates
that error with its lock/block/tx context — a corrupt byte is never rendered as some default
label. The stored bytes of every valid row are unchanged.

**`addr_balance.cellbase_txs_count`** (W5) counts the cellbase transactions among `txs_count`.
Both forward paths increment it through `AddressBalance::cellbase_tx_increment` — bulk inside the
address reducer's existing row update, live in the parser's `fold_tx_address_deltas` — and a
rollback restores it with the rest of the row from the block journal. Cellbase rows are the only
`addr_txs` rows without a `CF_TX_ACTIONS` row, so `CkbadgerStore::address_activity_count`
(`address_tx_count - cellbase_txs_count`) is the one definition of an address's activity count,
served as `total` on the unfiltered `/addresses/{addr}/activities`. The `AddressBalance` bincode
layout changed and requires a genesis re-sync.

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

### `tx_group_cycles`: lazy VM results

Key = the transaction's canonical slot, `block_number (8B BE) ‖ tx_index (4B BE)` — the
`tx_index` key. The value is `TxGroupCyclesRecord::Complete(TransactionCycles { block_hash,
cycles, groups })` or `Failed { block_hash, failure: ReplayFailure }`. Failure evidence is
fixed-size: dependency resolution, invalid chain data, lock/type group plus script hash, no groups,
or cycles overflow. Raw diagnostics are logged, never persisted or exposed by the public API. RPC
transport/body errors, RPC errors, uncommitted transactions and commit-block mismatches are
retryable: they leave both the group record and scalar cycles untouched. Open transaction pages
retry every two seconds while the node is unavailable and recover when it returns.

Only the indexer's on-demand cycles worker writes these records, atomically with
`tx_index.cycles`, after checking canonical identity; bulk emits no such row. Cellbase/genesis
transactions reject group records; a Complete record cannot regress to Failed. Failed may become
Failed or Complete. Readers resolve the transaction's canonical slot through `tx_hash_map` /
`tx_index` and read that key, then validate group sum and uniqueness. The CF is `RangeByBlock`: a
rollback range-deletes the rolled-back blocks' slots with `tx_index`, so a transaction re-included
by the winning branch has no record until the worker computes it again (until B-5c the record
lived in `sync_meta` under `tx_group_cycles:<tx_hash>`, which the journal rollback left behind).

### DAO Secondary Index Notes

- `dao_by_block`: key = `i64::MAX - deposit_block` (big-endian) + deposit outpoint, supports global DAO deposit pagination in newest-first order.
- `dao_by_lock_block`: key = `lock_script_hash(32B)` + block_desc + outpoint, supports per-address DAO deposit pagination.
- `dao_by_status_block`: key = `status(i16 BE)` + block_desc + outpoint, supports status-filtered DAO queries (`deposited/withdrawing/withdrawn`).

### `reorg_undo_log_by_block`: the block journal

Every row is one block journal entry (`docs/prompts/REORG_HANDLING.md` → Rollback Mechanisms): the
value of a `Journal`-class key (`CfSpec::rollback_class`, `CF_SPECS`) at the end of the block before
the one that first wrote it in a live batch.

- **Key** = `block_number(8B BE i64) + seq(8B BE u64)`, with `seq = (0x0006 << 48) | local`:
  `UndoSeqScope::Journal` (`crates/ckbadger-store/src/keys.rs`, the only scope; a row with other
  scope bits is an error on every read) in the high 16 bits, and `local` counting the block's
  entries from 0.
- **Value** = `[cf name length u8][cf name][key length u16 BE][key][tag u8]`, the tag 0 (absent)
  or 1 (present), followed when present by `[previous value length u32 BE][previous value]`.
  One encode (`encode_journal_value`, used by `BlockOverlay::finish`) and one decode
  (`decode_journal_value`, used by the rollback), both in
  `crates/ckbadger-store/src/block_overlay.rs`; a CF name over 255 bytes, a key over 65,535 or a
  value over `u32` is refused on encode, and a short row, trailing bytes, an unknown tag or a CF
  name that is not UTF-8 is an error on decode, naming the offset. An absent previous value means
  the key did not exist, and a rollback deletes it.

One entry per `(block, key)`, written by the batch's `BlockOverlay` in the same atomic commit as
the block; a block that leaves a key unchanged journals nothing for it. `rollback_to_block`
(`reorg_ops.rs`) range-deletes the rolled-back blocks' `RangeByBlock` rows (this CF among them) and
writes their entries back from the top block down, each block's in reverse sequence order. Every
`Journal`-class row is restored this way, counters and aggregates included (`addr_balance` with its
`txs_count` / `cellbase_txs_count`, `addr_prefix_stats`, `collection_activity_daily`, the eight
per-entity daily / hourly stats families, entity rows with their owner-index and outpoint
reverse-index rows, token and DAO lifecycle rows): nothing is reversed from the rows a rollback
deletes or rebuilt from the rows that survive. The DAO tip summary (`stats_dao` latest / top rows
and the still-open UTC+8 day's `DAO_DAILY_SNAPSHOT` row) is written outside the journal and
recomputed after every tip change; a sealed day's snapshot is journaled at its sealer block.

**Bounded window.** Every live commit advances the coverage floor to
`committed_tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS` (1000 blocks, `crates/indexer/src/sync/batch.rs`)
and deletes every journal row of the blocks it leaves behind with one range delete, staged into the
same batch as the blocks, so the floor and the deletions it describes are never separately durable.
`committed_tip` is the durable tip the batch builds on (the block before its first block), because
the prune can only see committed rows; a batch longer than the window leaves its own below-floor
rows to the next batch, whose tip has moved past them. The floor is monotonic. Bulk build records
no journal at all.

**Size.** Each live batch logs `Live batch journal staged` at `debug` with `journal_entries`,
`journal_bytes` and `journal_keys_written`. Most entries are creations of insert-only index rows
(no previous value); an aggregate row journals its previous value once per block that changes it.
On the test fixture chains an entry averages ~110 bytes, 4.8–8.0 KB per block.

### Protocol history and Type ID lineage

`.bit` AccountCell outputs whose data names no account — the DAS root AccountCell (all-zero
account id; it is re-created on mainnet) or data too short to name one — get no
`DOTBIT_ACCOUNT_OUTPOINT` row. An account whose name the DAS witness does not carry is still an
account (indexed under its id), because the activity builder counts identities from the data
alone. They get an empty
`STATS_PREFIX_DOTBIT_UNINDEXED_OUTPOINT` row (domain `stats_mnft`, key prefix `0x25` + outpoint)
instead, written by live sync in its block's scope (journaled) and by bulk build from the cell's
missing protocol facts. Consuming an AccountCell that has neither row is a lost index row and
stops the live batch; consuming one with the marker moves no `.bit` identity on either sync path,
exactly as its creation moved none.

Identity output history rows keep the same keys in domain `stats_spore` (did:ckb / .bit Cell)
and `stats_mnft` (.bit AccountCell). Their value is now one `AssetAction` byte: mint 0, transfer 1,
burn 2, recycle 3, renew 4, update 5. Bulk and live use the same prior-owner/expiry classifier;
transaction-wide .bit actions cannot turn a recreated neighbour into a mint or transfer.

Domain `ScriptVersionInfo.registered_references` preserves all `(reference_hash, hash_type)`
forms explicitly imported for a binary, including multiple Type ID references. Chain observations
never add registrations or replace the imported preferred canonical reference. The first metadata
entry for that binary is the preferred display reference; the complete registration set is sorted
and deduplicated independently of that preference.
`ScriptVersionInfo.type_id_deployments` separately stores exact `(type_script_hash, block_number)`
observations of ELF code. Only registered Type ID lineage propagates permanent family membership
to newly observed binaries. An unregistered Type ID may deploy binaries from different families;
its active binary determines its current family without creating a permanent registration.
Bulk excludes non-executable state cells; live changes to version metadata, family/label indexes
and reference mapping are journaled per block. Retirement clears the executable mapping while
preserving deployment history and the registered family's reference usage.

Startup label import owns imported registrations, not chain deployment history. A binary listed
only in another network's metadata can still have been deployed on this network. Excluded-network
cleanup preserves an existing binary family, label, indexes and chain-derived display reference
when this store has Type ID deployment observations for that binary; it removes revoked imported
anchors without inventing replacement registrations or family attribution. This also applies
after the final imported anchor is revoked. Unobserved foreign-only labels are still removed.
Retaining a known binary's family does not grant an unregistered Type ID permanent membership.
Active registrations and persisted observations are validated before import writes; contradictory
family attribution fails with reference, binary and block context instead of being repaired.

### `sync_meta` Fixed Keys

`sync_meta` belongs to the **domain store** and is written only by the indexer. Its fixed-key
namespace includes:

- canonical progress/state: `sync_status` (bincode `SyncChainStatus`: tip number / hash and the
  running transaction / cell totals, journaled per block) and `sync_runtime` (bincode
  `SyncRuntimeStatus`: last sync time, rate, run start, bulk completion, deep-fork state, written
  outside the journal); both decode exactly, and `get_sync_status` composes them into `SyncStatus`.
  Telemetry: `runtime_status`, `sync_progress`, `memory_stats`, and `background_tasks`
- every `sync_meta` key written outside the block journal is listed, with its owner and reason,
  in `OUTSIDE_JOURNAL` (`crates/ckbadger-store/src/store.rs`; `docs/prompts/REORG_HANDLING.md` →
  Outside the journal)
- rollback state: the reorg event records (`reorg:*`); the deep-fork state is in `sync_runtime`
- bulk-build state: the session-in-progress marker
- materialization tracker cursors: `hodl_cursor` (bincode `HodlTrackerCursor`: holder count,
  open snapshot day, last processed block) and `cell_dist_cursor` (bincode
  `CellDistTrackerCursor`: per-size-bucket live cell counts and occupied capacities, open snapshot
  day, last processed block). With the `hodl_capacity_by_date` and `cell_dist_cohort_by_month`
  rows they are the trackers' whole persisted state; the live writer writes, in each block's
  scope, the rows the block moved plus both cursors, so the block journal restores them, and bulk
  finalize writes them all once. A block's day (a consumed cell's creation day, an address's
  first-seen month) is `stats_day` of its stored header's `chain_time_ms`; no block→day map is
  persisted. The legacy whole-state blobs `hodl_tracker` / `cell_dist_tracker` are converted to
  these rows once at indexer start and deleted (development-only conversion, removed after the
  store refactor's re-sync)
- chain identity: `network_identity`, persisted at first sync and validated on later starts
- exact economics: `genesis_baseline` (`GenesisBaseline { total_issuance, burnt,
virtual_occupied }`), derived from block 0 and used by supply, APC, and knowledge-size paths
- exact live-cell inventory: `live_cell_summary:initialized` (presence marker) and
  `live_cell_summary:current`, a fixed-width 72-byte `LiveCellSummary` (`tip block/hash` + four
  `u64` counters). The live writer writes `current` at every block of a batch, in that block's
  scope, so the block journal restores the target block's summary on rollback; no per-block
  history is kept.
- entity-stats rollback coverage: `entity_stats_undo_contract`
- hourly retention evidence: `hourly_retention_state:<family>`
- derived aggregate state: `derive:<aggregate>` (bincode `DeriveMeta`, see
  [Derived Aggregates](#derived-aggregates))
- lazy VM results are not in `sync_meta`: see [`tx_group_cycles`](#tx_group_cycles-lazy-vm-results).
- fork reconciliation: `reorg_reconcile_pending` names the automatic event waiting for
  canonical replay. Once the winning tip is replayed, the indexer verifies abandoned-tip
  ancestry and atomically adds full event-key `reverted_by_event` / `reverses_events` links.
  Historical detection fields remain unchanged, and these observation links are not undo state.
  Before reorg, startup cleanup or partial-batch cleanup rolls any block back, the indexer
  atomically sets the pending event's `reconcile_interrupted` flag and deletes the marker in
  domain `CF_SYNC_META`. A surviving marker proves no subsequent rollback began, so the retained
  fork-point check remains a strict invariant. Interruption and `superseded_by_tip_hash` may
  coexist; deep events always have interruption false. A crash after rollback but before the
  new event is written can still lose that new observation; this change does not journal it.
  Every event that rolled blocks back records `first_orphan_hash` (`Option<[u8; 32]>`): the first
  block the switch dropped (height `fork_point + 1`), read by `execute_reorg` from the domain
  store's own headers before the rollback, after proving `fork_point_hash -> first orphan -> … ->
old_tip_hash` links; `None` exactly for a deep detection. It is the event's D7 address, so the API
  resolves every fork row without the node store. Reconcile and interruption rewrites carry it
  unchanged. The `ReorgEvent` bincode layout changed (again, W5) and requires a genesis re-sync.

#### `entity_stats_undo_contract`

Value is `EntityStatsUndoContract { version, coverage_floor_block, updated_at_block }`.
`coverage_floor_block` is the lowest block a shallow reorg can still be undone to — block journal
rows at or below it have been pruned; `updated_at_block` is the committed tip when that floor was
last advanced, so a stale floor is distinguishable from a current one. The contract describes
which journal rows exist, so it is written outside the journal
(`StoreBatch::put_entity_stats_undo_contract` stages it straight into the commit; the block overlay
refuses the key and the rollback refuses a journal row that names it): rolling back the block that
advanced the floor leaves the floor where the prune put it.

Who writes it, and when:

- **Bulk completion** writes it once, with `coverage_floor_block` = the **handoff tip**, the last
  block bulk actually wrote — never the chain tip bulk was racing. The two differ by up to
  `bulk_sync_threshold`, and a floor above what the store holds would claim coverage over blocks
  that were never written. See `docs/prompts/BULK_SYNC.md` rule 12.
- **Live sync** rewrites it in any commit that advances the floor, in the same batch as the
  blocks. A fresh store that never ran bulk gets its first contract at its first live commit.

All three rollback entry points — live reorg, startup cleanup, and partial-batch cleanup — refuse
a target below `coverage_floor_block` with a rebuild-required error rather than rolling back
rows they cannot restore. A store with a tip but no contract is refused at startup: it was
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
- `round_in_progress_cutoff_hour` and `cursor` describe an in-flight round: below that cutoff,
  deletions have happened just up to `cursor`. Readers must not treat them as a boundary.
- Deletions and the state row are staged into the same block batch by the writer, so the
  persisted boundary always matches what was actually deleted.
- A round's cutoff is the lower of two bounds: the read contract (`now − 48 h`) and the rollback
  contract (the hour of the header at `committed_tip − ENTITY_STATS_UNDO_RETAIN_BLOCKS`, so no
  hourly key a retained journal entry could restore is deleted), and never below
  `executed_cutoff_hour`. `committed_tip` is the block before the live batch's first block — the
  store's durable sync tip, checked equal to it — never the batch's own uncommitted last block:
  a batch longer than the undo window would otherwise read a header it has not committed yet and
  report store corruption (POSTMORTEM IDX-012).
- The periodic retention request is cleared only after the batch that served it commits; a
  failed batch leaves it armed for the retry.
- A round pins its cutoff when it starts: the first step writes `round_in_progress_cutoff_hour`,
  later steps read it back instead of recomputing from a clock that has moved, and completion
  advances `executed_cutoff_hour` to exactly that pinned value. A chain shorter than the undo
  window has no block old enough to bound the rollback contract, so the step is skipped (with its
  reason logged) rather than persisting an `i64::MIN` cutoff as authoritative.

This row is the evidence that a missing hourly bucket is legitimate retention rather than
corruption. A reader that cannot see it must report `unknown`, never "zero".
`SPORE_HOURLY` has **no retention policy and no row** — its buckets are never expired. Identity
(`.bit` sentinel) rows under `OBJECT_HOURLY` are excluded for the same reason; only mNFT classes
expire, resolved through `cf_mnft_collection_agg` via the padded 32-byte collection id.

The live-cell summary is mutable canonical state, so it belongs to the domain store. Normal sync
updates it at every block, in the same atomic batch as block headers and `sync_status`, and the
block journal records it per block; bulk-build keeps only the four global counters and the last
block-end summary in memory, then publishes it with the final status. A rollback writes the target
block's summary back from the journal. Neither API reads nor recovery scan `live_cells`/`cells`, and
`CF_CELLS` is never written by this feature.

Missing or conflicting identity/baseline state is an invariant failure. API readers must not
invent a replacement value or write this CF.

### Bulk-Build Sealed Aggregate Note

During fresh-db bulk sync, the indexer writes `stats_dao`, `stats_script`, and `stats_token`
inline as Class C sealed aggregates:

- `stats_dao` stores DAO daily snapshots keyed by date, then refreshes the latest/top summary rows
  after sync tip metadata is finalized. The latest row (`DaoLatestStatistics`) records the block it
  was computed at by number and hash (`tip_block_number`, `tip_block_hash`, both from the sync-tip
  header its single writer `refresh_latest_dao_statistics` reads, on bulk and live alike);
  `/dao/statistics` serves them as `anchor` and fails when that hash is no longer the canonical
  header at that height. The bincode layout changed (W5) and requires a genesis re-sync.
- `stats_script` stores `ScriptDailyDelta` rows keyed by
  `code_hash + hash_type + kind(lock/type) + YYYYMMDD` — the hash_type byte keeps
  references that share code_hash bytes but differ in hash_type (data/type/data1/data2)
  on independent daily timelines.
- `tokens` rows (`TokenInfo`) carry metadata provenance (IV-26). `metadata_source` (`Label` or
  `Chain`) names which source wrote the stored `name` / `symbol` / `decimals`: `None` exactly when
  all three are `None`, `Label` while any label-written field survives. `chain_metadata` (a
  `ChainTokenMetadata` with `name`, `symbol`, `decimals` and `binding`, `XudtExtension` or
  `IssuanceCooccurrence`) keeps the bound Unique Cell's own values even when the label kept the
  stored fields. The one rule is `ckbadger-store` `token_metadata.rs`: label import calls
  `TokenInfo::apply_label`; the live writer and the bulk snapshot writer both call
  `TokenInfo::merge_chain_metadata` through `udt::apply_onchain_token_info` (an extension binding
  overwrites and replaces the chain values; a co-occurrence fills gaps only and never displaces an
  earlier binding); the API reads `checked_metadata_source`, `chain_metadata_conflict` and
  `claimed_symbols`.
- `stats_token` stores total transfer counters, hourly transfer buckets, and `TokenDailyDelta`
  rows keyed by token `type_script_hash`. A token's daily capacity/knowledge history counts every
  cell whose type script is that sUDT/xUDT instance, whatever its data holds (an owner-mode cell
  too short for a u128 amount included); the amount facets — holders, supply, transfer counts,
  `TOKEN_HOURLY` and the token row — need an amount. Bulk (via `CellSemanticTag::{Sudt, Xudt}`)
  and live (`pipeline::token_daily_member`) apply the same rule (POSTMORTEM STATS-011).
  `TokenDailyDelta` (`owned_capacity_delta`, `owned_knowledge_delta`, `tx_count`, `minted`,
  `burned`, `mint_count`, `burn_count`, `transfer_tx_count`, `live_cells_delta`; 136 bytes): `tx_count` is the number of transactions that day that
  created or consumed at least one such cell, one per transaction however many of its cells moved
  (live: `pipeline::add_token_daily_tx` → `TokenDailyChanges::add_tx`; bulk: `TokenOwner::apply_tx`).
  `transfer_tx_count` is the number of those transactions with at least one `token_transfers`
  record of the token (the token tabs' scope; their `total` is its sum), and `minted` / `burned`
  (`TokenBalance`, 32-byte big-endian each, exact past `i128`) and `mint_count` / `burn_count` sum
  that day's mint / burn transfer records (`UdtParser::build_transfers_from_cells`, the records the
  Transactions tab shows). Both are folded once per (transaction, token) by
  `TokenDailyDelta::add_transfer_tx` on both paths (live: the writer folds the records it persists
  into the parser's contributions before `update_token_daily_deltas_batch`; bulk:
  `TokenOwner::apply_tx`'s transfer loop); the day's signed supply change is `minted - burned`
  (`TokenDailyDelta::apply_supply`). `live_cells_delta` is the day's net live cells of the token
  (every member cell created `+1`, consumed `-1`, the same membership as the capacity deltas): live
  `pipeline::add_token_daily_tx` folds it from the parser's per-cell members
  (`token_daily_created` / `token_daily_consumed`), bulk `TokenOwner::record_daily_delta` on the
  day entry it already updates for capacity. Its running sum (`token_live_cells_from_daily`) is the
  token's live cell count, the one `cellsCount` `/tokens` and `/tokens/{hash}` serve — neither
  reads the token's cells; `ckbadger verify` (`token_live_cells_consistency`) compares it with the
  live cells behind `cell_by_type`. A running count below zero is an error naming the token and
  the day. A day with a transaction keeps its row even when its capacity
  nets to zero, and `find_first_invalid_token_daily_delta` rejects a row with `tx_count == 0`, a
  running supply that a day's burns would take below zero, a record count without its amount (or the
  reverse), and a `transfer_tx_count` above `tx_count` or zero beside mint / burn records. A rollback restores the whole row from the block journal. The script,
  cluster, spore and object daily rows keep the 32-byte pair codec (`apply_daily_pair`); each codec
  rejects the other's width.

### Derived Aggregates

Persisted aggregates the **indexer derives from rows it has already written**, after bulk sync
hands off to live sync, and then maintains on the live path only (round 3, lane L). They cost bulk
sync nothing: the three CFs are `BulkDisabled` (the bulk materializer refuses them), the rebuild
runs on the live pipeline's writer task, and live maintenance runs inside `write_parsed_batch`
only for non-bulk batches. They need no re-sync: an existing store derives them on first start.

One definition per aggregate lives in `crates/ckbadger-store/src/derive_ops.rs`
(`DerivedAggregateId { name, cf_name, version }`, key layouts, value codec); the indexer computes
the rows (`crates/indexer/src/derive/`), the API reads them.

**Signed sums.** Every aggregate is a sum over source rows: each row contributes a fixed vector of
`i128` components to one or more keys; a key's value is the sum (16 bytes little-endian per
component); an all-zero sum has no row. The rebuild folds the source rows of one RocksDB
**snapshot** taken at the sync tip `H` when the derivation starts; every live block above `H`
adds `f(new) − f(old)` for each source row it changes. Intermediate values may be negative (a row
present at `H` and changed above it is subtracted before the rebuild reaches it); each
aggregate's row invariant is checked when the rebuild completes and on every later live write.

**State** — `sync_meta` `derive:<aggregate>` → `DeriveMeta { version, state, run, started_at_ms,
finished_at_ms }`, with `state` one of:

| State                                                     | Meaning                                                                                                                    |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `NotStarted`                                              | rows absent or garbage; not maintained, not served                                                                         |
| `Running { derived_from_tip: H, phase, cursor, rows_done }` | rebuild in flight over the snapshot at `H`; live deltas above `H` applied. `phase` `Folding`: source rows being folded (rows may be negative); `Validating`: source exhausted, rows exact, being checked in chunks and live writes checked; `cursor` is progress only |
| `Complete { derived_from_tip: H, completed_at_tip: C, … }` | rows equal a from-scratch derivation at the sync tip; exact since block `C` (the tip when the last chunk committed)        |
| `Failed { error }`                                        | written only by an indexer that is stopping on an invariant violation of this aggregate (rebuild or live write), so the TUI / API can say why; reset at the next start |

Transitions: on indexer start (before the first live batch) a missing meta, a `version` other
than the current one, `Running` (its snapshot died with the process) or `Failed` → rows cleared,
`NotStarted`; `Complete` is kept (and must have `C` ≤ the sync tip). Once sync is within
`bulk_sync_threshold` of the tip, the scheduler starts the first not-complete aggregate in
registry order: rows cleared + `Running{H, Folding}` in one write, then the snapshot. Each chunk
folds at most 20,000 source rows / 4,096 distinct keys / ~150 ms. When the source is exhausted
the phase becomes `Validating`: the rows are checked against the aggregate's invariant in chunks
of the same size (live writes are checked from then on, since every row is exact), and after the
last row the state becomes `Complete{H, C}`. A rollback resets
(`NotStarted` + clear + deletion of its journal entries above the target, committed **before** the
journal rollback) a `Running` derivation whenever it removes a block, and a `Complete` one when its
target is below `C`; otherwise the block journal restores the rows exactly. Aggregates that are
not maintained have their journal entries above the target dropped the same way. **Fail fast:** a live delta that cannot be computed or a row that
breaks its invariant fails the live batch as a pre-commit invariant (nothing of the batch is
committed) and a rebuild-chunk or validation error propagates out of the scheduler — either way
the indexer stops, after writing the aggregate's `Failed` marker. There is no degraded
continuation. Bumping an aggregate's `version` in `derive_ops.rs` re-derives it on the next
start.

**`dao_live_agg`** (L1, J1). Source rows: the `dao_deposits` lifecycle entries with status 0 (a
live deposit cell) and 1 (a live withdraw-request cell), walked through `dao_by_status_block`. The
epochs come from the canonical headers of the deposit and request blocks.

| Key                                                | Value                                                 | Contributed by                 |
| -------------------------------------------------- | ----------------------------------------------------- | ------------------------------ |
| `0x00`                                             | `[phase0 capacity, phase0 cells, phase1 capacity, phase1 cells]` | every status-0 / status-1 row |
| `0x01` + lock_hash (32B)                           | `[phase0 capacity, phase0 cells]` of that depositor   | status-0 rows of that lock     |
| `0x02` + deposit epoch (16B)                       | `[phase0 capacity, phase0 cells]`                     | status-0 rows deposited there  |
| `0x03` + deposit epoch (16B) + request epoch (16B) | `[phase1 capacity, phase1 cells]`                     | status-1 rows                  |

An epoch is `number (u64 BE) + index (u32 BE) + length (u32 BE)`. Capacities are exact shannons.
`/dao/evidence` buckets the `0x02` rows into its 30 nominal days and splits the `0x03` rows into
ready / waiting against the tip epoch; `/dao/depositors` ranks the `0x01` rows. The rebuild
re-checks each stored row against its headers (lifecycle markers, deposit/request AR, deposit
time); live sync folds `LiveDaoBatch`'s transitions (creation, phase 1, phase 2) per block.

**`object_collection_activity_count`** (L2, J5). Key = collection_id (32B, padded as in
`object_collection_activities`) → `[rows]`: the number of `object_collection_activities` rows
with that prefix (Spore clusters, the Sole Spores collection, and m-NFT classes alike; the API
reads it for Spore collections, whose `cluster_agg` row is bulk-written and stays byte-identical).
The rebuild counts the CF; live sync adds one per row `ObjectCollectionActivityAccumulator::flush`
writes; a rollback restores the rows and the count from the block journal.

**`bit_cell_by_account`** (L3, J3). Key = account_id (20B) + `.bit Cell` identity id (32B) →
`[1]` for every live `.bit Cell` whose data names that `.bit` account; a prefix scan on the
account lists them. Source rows: `identity_data` entries of standard `.bit Cell`. Live sync folds
the entry transitions `insert_bit_cell` and the identity branch of `consume_spore` report: a mint
adds the row, a consume removes it, and a `.bit Cell` re-created with data naming another account
moves between accounts (`f(new) − f(old)`); the completion check requires every value to be
exactly `[1]`.

**Writers.** The indexer only: the scheduler (rebuild chunks, meta), `apply_live_batch` (live
deltas, each row written per block in the live batch and journaled with it), the block journal
(rollback). The API reads the
meta and the rows in the request's pinned view and serves an aggregate only when its meta is
`Complete` at the version it reads (`crates/api/src/derived.rs`). Until then `/dao/evidence` and
`/dao/depositors` — whose whole response is the aggregate — answer `503 derive_pending`, and every
other field an aggregate feeds (a cluster's `activitiesCount`, a feed's `total`, a `.bit`
account's `bitCells`) is `null` (unknown) in an otherwise normal response.
Progress is mirrored to `background_tasks` as `derive:<aggregate>` (TUI, `/pipeline`).

### `stats_chain` activity address sets

`ACTIVITY_DAILY` (`0x1D`) / `ACTIVITY_HOURLY` (`0x1E`) rows carry `unique_address_count`; the
addresses behind it are kept per bucket in two forms (`Lock` participants only):

| Prefix | Constant                   | Key suffix                           | Value                                    | Written by                                                                                  |
| ------ | -------------------------- | ------------------------------------ | ---------------------------------------- | ------------------------------------------------------------------------------------------- |
| `0x23` | `ACTIVITY_DAILY_ADDR_SET`  | `%Y%m%d` (UTC+8)                     | sorted, concatenated 32-byte lock hashes | bulk build, once per bucket; live sync never rewrites it                                    |
| `0x24` | `ACTIVITY_HOURLY_ADDR_SET` | `%Y%m%d%H` (UTC+8)                   | sorted, concatenated 32-byte lock hashes | bulk build, once per bucket; live sync never rewrites it                                    |
| `0x26` | `ACTIVITY_DAILY_ADDR_ROW`  | `%Y%m%d` (UTC+8) + lock hash (32B)   | empty                                    | live sync: one row per address it saw first in the bucket, in the block's scope (journaled) |
| `0x27` | `ACTIVITY_HOURLY_ADDR_ROW` | `%Y%m%d%H` (UTC+8) + lock hash (32B) | empty                                    | live sync: one row per address it saw first in the bucket, in the block's scope (journaled) |

An address is new to a bucket when it is in neither the bucket's blob nor an earlier row; each new
one gets its row and raises the bucket's `unique_address_count` by one
(`BatchWriter::update_activity_bucket_stats`, per block). So a live block journals one small entry
per new address and its bucket row, never the bucket-wide set. A bucket's addresses are the blob
plus its rows (`CkbadgerStore::get_activity_bucket_addrs`, which fails on an address in both); a
blob without its bucket's stats row is an error.

### stats_hodl Key Prefixes

The `stats_hodl` CF uses single-byte prefixes to multiplex different snapshot types. Key format: `prefix(1B) + date_string`.

| Prefix | Constant                         | Value Type            | Description                                         |
| ------ | -------------------------------- | --------------------- | --------------------------------------------------- |
| `0x0B` | `STATS_PREFIX_HODL_WAVE`         | HODL wave snapshot    | Daily HODL wave age-band distribution               |
| `0x21` | `STATS_PREFIX_CELL_DISTRIBUTION` | DailyCellDistribution | Daily cell distribution (age bands + size buckets)  |
| `0x22` | `STATS_PREFIX_ADDR_COHORT`       | DailyAddressCohort    | Daily address cohort retention (new/returning/lost) |

Cell distribution and address cohort snapshots are materialized by the indexer during sync (one snapshot per day boundary). The API reads these directly instead of scanning live cells.

## Network Store

The **network store** (`[store].network_data_path`, default `data/network`, CFs `net_nodes` + `net_stats` + `net_crawl`) is a distinct third RocksDB store class holding whole-network CKB L1 crawler observations, configured-local-node session observations, and resumable crawl state. This remains exactly 3 network CFs and 75 CFs overall; the richer evidence model adds no CF. Unlike the two chain stores it is:

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
GeoIP latitude and longitude are independently optional; absent MMDB coordinates are not `(0, 0)`.
An ASN record without a valid number is absent, not `AS0`. GeoIP database decode/read failures
abort that crawler round before `net_nodes` publication; ordinary IP misses remain absent.
`NodeRecord` uses bincode, so the optional-coordinate schema requires deleting the existing
network store and re-crawling after this change. Domain and append-only chain stores are unaffected.
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
  long-poll releases its pin before waiting — its contract is to observe the _next_ view;
  `/pipeline` releases it before waiting, at most 3 s, for the store to reach the node's pool tip,
  pins a fresh view for each attempt, and builds the snapshot under the attempt that ended the
  wait), plus background full-store scans (cache warmup), which interleave external
  I/O with minutes-long scans and accept drift rather than freeze catch-up.
- WebSocket block broadcasts pin only the bounded synchronous local payload build
  after the RPC tip request. The pin keeps block headers and canonical tx-index
  hashes in one catch-up view and is released before the next ticker/RPC await.
- Both sides are bounded: a read scope arriving after a catch-up queues yields to it for 100ms and
  then pins anyway, and a catch-up held up by a read scope logs the stall every 5s.
- Never hold a read view across `CkbadgerStore::refresh` on the same thread — that self-deadlocks.
  Catch-up runs on its own thread (`spawn_blocking` in the API), never inside a pinned scope.

### Serving only caught-up views (`crates/api/src/store_readiness.rs`)

A freshly opened secondary builds each column family's view at that CF's own latest complete
point, so before its first catch-up two CFs the indexer commits together can disagree (round 2:
`/tokens` answered 500 for three minutes after a restart because `CF_STATS_MNFT` lagged the
collection aggregate CF and a correct warmup invariant fired on healthy data). The API's store
refresh loop is therefore also its readiness authority, recorded in one `StoreReadiness` on
`AppState` and enforced by one route layer over every route that reads a secondary:

- **Startup rule.** The loop's first round runs as soon as the router is built and repeats every
  second until one round has caught up every secondary the process reads (append-only, domain,
  the CKB node DB reader, and the network store when present). Until then those routes answer
  `503 store_catching_up` naming the attempt and the failing store's error, the cache warmup and
  every other background reader (chart warmup, cache loops, WebSocket broadcasters, tx-pool
  mirror) wait, and each failed attempt logs at `error!`.
- **3-strike rule.** After that, three consecutive failed catch-ups of any one secondary turn those
  routes into the same 503 with that secondary's last error. Failures one and two log at `warn!`
  and keep serving the previous (coherent, stale) view; the third and every further one log at
  `error!`; that secondary's next success clears it and logs at `info!`.
- A network-store secondary attached after startup is caught up once before it is published to
  readers, so it never serves its freshly opened view either.
- Not gated: routes that read only the node's RPC (`/mempool/info`, `/mempool/transactions`,
  `/mempool/blocks`) and the `/ws` upgrade (its broadcasters start after the first catch-up).
- Warmup failures are unchanged: a named invariant still fails the warmup (`500` with the
  invariant text) and the loop retries it.

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
**whole** database, so the smallest per-CF buffer decides how often all 71 CFs flush together.

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
