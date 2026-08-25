# ckbadger-store Column Families (63 total: 59 domain + 1 append-only + 3 network)

ckbadger runs three logical RocksDB store classes (all backed by `ckbadger-store`):

- **Domain store** (`[store].domain_data_path`, 59 CFs) — canonical chain view, all mutable state including activities, addr_txs, live/consumed cell markers, indexes, stats, and aggregates. May perform create/update/delete as required by chain progression and reorg handling.
- **Append-only store** (`[store].append_only_data_path`, 1 CF: `cells`) — immutable cell payloads, content-addressed by outpoint. Write-once, never updated or deleted.
- **Network store** (`[store].network_data_path`, 3 CFs: `net_nodes`, `net_stats`, `net_crawl`) —
  crawler p2p probes, configured-local-node session observations, and durable in-progress crawl
  state: non-chain, non-deterministic, TTL-retained. Written solely by the opt-in
  `ckbadger-crawler` service; it is the **only store class EXEMPT from rebuild-from-genesis**. See
  the [Network Store](#network-store) section below.

The indexer opens the two chain stores (domain + append-only) read-write and the API opens them secondary (read-only). The network store follows the same sole-writer + secondary-reader model: the crawler opens it read-write (sole writer), read consumers (API) open it secondary (read-only). Cell reads are cross-store: live/consumed markers in domain, cell payloads in append-only.

## Column Families

| Column Family                    | Key                                                               | Value                                                                | Purpose                                                                                                                                                                                |
| -------------------------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cells` **(append-only store)**  | tx_hash + output_index (34B)                                      | LiveCellInfo                                                         | Immutable cell payload store (write-once, content-addressed)                                                                                                                           |
| `live_cells`                     | tx_hash + output_index (34B)                                      | empty                                                                | Live UTXO pointer set                                                                                                                                                                  |
| `consumed_cells`                 | tx_hash + output_index (34B)                                      | ConsumedCellMeta                                                     | Consumed pointer + consume metadata                                                                                                                                                    |
| `reorg_undo_log_by_block`        | block + seq                                                       | UndoLogEntry                                                         | Unified rollback undo-log journal                                                                                                                                                      |
| `block_headers`                  | block_number (8B)                                                 | CachedBlockHeader                                                    | Block header + DAO field cache                                                                                                                                                         |
| `block_hash_index`               | block_hash (32B)                                                  | block_number (8B)                                                    | Reverse lookup: hash -> number                                                                                                                                                         |
| `cell_by_lock`                   | lock_script_hash + outpoint                                       | empty                                                                | Cell index by lock script                                                                                                                                                              |
| `cell_by_type`                   | type_script_hash + outpoint                                       | empty                                                                | Cell index by type script                                                                                                                                                              |
| `cell_by_lock_code`              | lock_code_hash (32B) + hash_type (1B) + block (8B BE) + outpoint  | empty                                                                | Cell index by lock script reference form `(code_hash, hash_type)`                                                                                                                      |
| `cell_by_type_code`              | type_code_hash (32B) + hash_type (1B) + block (8B BE) + outpoint  | empty                                                                | Cell index by type script reference form `(code_hash, hash_type)`                                                                                                                      |
| `cell_by_data_hash`              | blake2b(cell_data) + outpoint                                     | empty                                                                | Cell index by data hash (code cell resolution)                                                                                                                                         |
| `tx_index`                       | block_number + tx_index                                           | tx_hash                                                              | Transaction ordering index                                                                                                                                                             |
| `tx_hash_map`                    | tx_hash (32B)                                                     | block_number + tx_index                                              | Reverse lookup: tx_hash -> position                                                                                                                                                    |
| `addr_balance`                   | lock_script_hash (32B)                                            | AddressBalance                                                       | Address balance and cell counts                                                                                                                                                        |
| `addr_txs`                       | lock_hash + block + tx_index + tx_hash                            | AddrTxValue                                                          | Address transaction thin index with capacity change, tx flags, and participant activity tags                                                                                           |
| `dao_deposits`                   | tx_hash + output_index (34B)                                      | DaoDepositCacheEntry                                                 | DAO lifecycle plus original capacity, exact occupied capacity, deposit/request ARs, and claimed compensation                                                                           |
| `dao_by_withdraw_tx`             | withdraw_outpoint (34B)                                           | deposit outpoint                                                     | Reverse lookup: withdraw outpoint -> deposit                                                                                                                                           |
| `dao_by_block`                   | block_desc (8B BE) + outpoint (34B)                               | empty                                                                | DAO index ordered by deposit block DESC                                                                                                                                                |
| `dao_by_lock_block`              | lock_hash (32B) + block_desc (8B BE) + outpoint (34B)             | empty                                                                | DAO index by lock + deposit block DESC                                                                                                                                                 |
| `dao_by_status_block`            | status (2B BE) + block_desc (8B BE) + outpoint (34B)              | empty                                                                | DAO index by status + deposit block DESC                                                                                                                                               |
| `tokens`                         | type_script_hash (32B)                                            | TokenInfo                                                            | UDT metadata only; total supply and holder count derive from `token_holders`                                                                                                           |
| `token_holders`                  | type_hash (32B) + lock_hash (32B)                                 | TokenBalance (32B unsigned BE)                                       | Exact aggregate holder balances; values may exceed a single cell's u128 amount                                                                                                         |
| `token_holders_by_balance`       | type_hash (32B) + complemented balance BE (32B) + lock_hash (32B) | empty                                                                | 96B key; token holders ranked by balance DESC, lock hash ASC                                                                                                                           |
| `addr_tokens_by_balance`         | lock_hash (32B) + complemented balance BE (32B) + type_hash (32B) | empty                                                                | 96B key; address token balances ranked by balance DESC, type hash ASC                                                                                                                  |
| `token_transfers`                | type_hash + block + tx_index                                      | TransferInfo                                                         | Token transfer records                                                                                                                                                                 |
| `spore_data`                     | spore_id (32B)                                                    | SporeData                                                            | Spore NFT metadata                                                                                                                                                                     |
| `spore_by_cluster`               | cluster_id + spore_id                                             | empty                                                                | Spore index by cluster                                                                                                                                                                 |
| `mnft_data`                      | object_id                                                         | ObjectEntry                                                          | mNFT metadata (issuer/class/token)                                                                                                                                                     |
| `mnft_by_collection`             | collection_id + object_id                                         | empty                                                                | mNFT index by collection                                                                                                                                                               |
| `identity_data`                  | identity_id (20B AccountCell; 32B .bit Cell/did:ckb)              | IdentityEntry                                                        | Identity metadata with separate standards and lifecycles for .bit AccountCell, .bit Cell, and did:ckb                                                                                  |
| `mnft_collection_agg`            | collection_id                                                     | MnftCollectionAggregate                                              | mNFT collection aggregate stats                                                                                                                                                        |
| `object_collection_activities`   | collection_id + block + tx                                        | ObjectCollectionActivityEntry                                        | Pre-computed object collection activity feed                                                                                                                                           |
| `identity_by_collection`         | collection_id + identity_id                                       | empty                                                                | Identity index by collection                                                                                                                                                           |
| `identity_agg`                   | collection_id (sentinel 32B)                                      | IdentityCollectionAgg                                                | Per-standard identity aggregates; .bit AccountCell and .bit Cell use different sentinels                                                                                               |
| `identity_collection_activities` | collection_id + block + tx                                        | ObjectCollectionActivityEntry                                        | Pre-computed identity collection activity feed (domain)                                                                                                                                |
| `stats_identity`                 | collection_id + lock_hash                                         | i64 (owner count)                                                    | Per-owner identity counts by collection                                                                                                                                                |
| `activities`                     | block_num_desc + tx_idx_desc + tx_hash (44B)                      | TxActions                                                            | One canonical per-tx activity record; TX-level protocol/type/lock actions stored once plus sorted participant deltas                                                                   |
| `pending_proposals`              | proposal_id (10B hex string)                                      | CachedProposal (JSON)                                                | Ephemeral pending proposal cache (live sync only)                                                                                                                                      |
| `fiber_channels`                 | channel_id (32B blake2b of funding outpoint)                      | FiberChannel                                                         | Fiber Network channel registry; funding lock args are descriptive and are not unique                                                                                                   |
| `fiber_channel_by_commitment`    | commitment_hash                                                   | channel_id (32B)                                                     | Fiber channel index by commitment                                                                                                                                                      |
| `addr_fiber_channels`            | lock_hash (32B) + channel_id (32B)                                | empty                                                                | Address-to-Fiber-channels index                                                                                                                                                        |
| `cluster_agg`                    | cluster_id                                                        | ClusterAgg                                                           | Spore cluster aggregate stats                                                                                                                                                          |
| `script_info`                    | code_hash (32B)                                                   | ScriptInfo                                                           | Legacy/compatibility script metadata keyed by bare hash                                                                                                                                |
| `stats_chain`                    | prefixed keys                                                     | chain chart snapshots                                                | Daily/hourly/epoch/miner/block stats (DailyActivityStats includes protocol_action_counts)                                                                                              |
| `stats_dao`                      | prefixed keys                                                     | DAO snapshots                                                        | DAO daily snapshots (including exact unclaimed and frozen phase-1 compensation), plus latest/top summaries; sealed aggregates in bulk build                                            |
| `stats_hodl`                     | prefixed keys                                                     | HODL/chart snapshots                                                 | HODL waves, cell distribution, address cohorts                                                                                                                                         |
| `stats_script`                   | prefixed keys                                                     | ScriptDailyDelta                                                     | Script daily deltas (per `code_hash` + `hash_type` + lock/type + day; sealed in bulk build)                                                                                            |
| `stats_token`                    | prefixed keys                                                     | token rollups + deltas                                               | Token transfer totals, hourly buckets, and daily deltas (sealed in bulk build)                                                                                                         |
| `stats_spore`                    | prefixed keys                                                     | spore rollups/indexes                                                | Spore/cluster daily + owner/index stats                                                                                                                                                |
| `stats_mnft`                     | prefixed keys                                                     | mNFT rollups/indexes                                                 | mNFT daily + hourly + owner/index stats                                                                                                                                                |
| `script_versions`                | version_hash                                                      | ScriptVersionInfo                                                    | Canonical script code version rows keyed by `H(script_code)`                                                                                                                           |
| `script_versions_by_label`       | label_len + label_key + version_hash                              | empty                                                                | Label-to-version index for named script family lookups                                                                                                                                 |
| `script_families`                | family_id (string)                                                | ScriptFamilyInfo                                                     | Script family metadata (groups related script versions)                                                                                                                                |
| `script_versions_by_family`      | family_id + version_hash                                          | empty                                                                | Script versions indexed by family                                                                                                                                                      |
| `script_reference_info`          | reference_hash + hash_type (33B)                                  | ScriptReferenceInfo                                                  | Script reference aggregate stats (cell/capacity counts per lock/type)                                                                                                                  |
| `script_reference_to_version`    | reference_hash + hash_type (33B)                                  | version_hash                                                         | Script reference to version mapping                                                                                                                                                    |
| `script_family_by_name`          | family_name (string)                                              | family_id                                                            | Reverse lookup: family name -> family ID                                                                                                                                               |
| `sync_meta`                      | fixed keys                                                        | Typed records / JSON monitoring bytes                                | Tip/status/runtime/progress/memory, reorg/deep-fork state, bulk session marker, background tasks, network identity, and genesis economic baseline                                      |
| `dob_decoded`                    | spore_id (32B)                                                    | DecodeOutcome (Decoded(DobDecodedEntry) \| Failed(DobDecodeFailure)) | Cached CKB-VM DOB decode outcome (bulk-disabled, populated after sync catches up to tip). Failed is written only for deterministic failures; transient RPC failures are not persisted. |
| `lock_scripts`                   | lock_hash (32B)                                                   | LockScriptEntry                                                      | Lock script components by hash (survives cell consumption for address resolution)                                                                                                      |
| `net_nodes` **(network store)**  | peer_id (raw bytes)                                               | NodeRecord                                                           | TTL-retained same-network crawler-Identify records, latest dialability, and exact Discovery evidence                                                                                   |
| `net_stats` **(network store)**  | `0x00` singleton, or metric(1B)+gran(1B)+bucket(8B BE)            | LatestStatus / HistoryPoint                                          | Checked completed dial/session/Discovery aggregates plus time-bucketed verified/reachable/share history                                                                                |
| `net_crawl` **(network store)**  | `0x00` singleton, or `0x01` + peer_id                             | ActiveCrawl / CrawlCandidate                                         | Durable logical-round state: dial aliases, target-centric advertisements, direct sessions, active probes, and stable completed evidence                                                |

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

The **network store** (`[store].network_data_path`, default `data/network`, CFs `net_nodes` + `net_stats` + `net_crawl`) is a distinct third RocksDB store class holding whole-network CKB L1 crawler observations, configured-local-node session observations, and resumable crawl state. This remains exactly 3 network CFs and 63 CFs overall; the richer evidence model adds no CF. Unlike the two chain stores it is:

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
