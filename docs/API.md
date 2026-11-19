# ckbadger HTTP API Reference (skeleton)

This is an auto-generated skeleton of every endpoint exposed by `ckbadger-api`,
derived from the `routes()` functions in `crates/api/src/routes/*.rs` and the
nesting in `crates/api/src/lib.rs:453` (`.nest("/api/v1", routes::api_routes())`).

**Source of truth.** The Rust code is authoritative. Each handler's `Query<T>`,
`Path<T>`, and `Json<T>` types — together with the `Serialize` response struct
— define the exact request and response shape. Look there for field-level
details that aren't reproduced here.

## Conventions

- **Canonical backend path.** All inventory paths below are nested under `/api/v1` on a direct
  per-network API port, for example `http://127.0.0.1:8101/api/v1`.
- **Shared-frontend path.** In orchestrator mode, use
  `/api/{network}/v1` on the shared frontend origin, for example
  `http://127.0.0.1:8100/api/testnet/v1`. The proxy removes `{network}` before forwarding to the
  selected backend. WebSockets similarly use direct `/ws` or shared `/ws/{network}`.
- **Path syntax.** Axum 0.8: path params use `{id}` form.
- **Casing.** Response structs use `#[serde(rename_all = "camelCase")]`. Query
  params decode from snake_case (`lock_script_hash`, `type_script_hash`, etc.).
- **Error envelope.** Handlers return `ApiResult<T>` — the `ApiError` constructors (`not_found`,
  `bad_request`, `internal`, `initializing`, `warmup_pending`, `service_unavailable`, …) map to
  appropriate HTTP statuses. `service_unavailable` (`503`) means the answer depends on a state
  that passes and the request can be retried (see `/transactions/{hash}/detail`).
- **Pagination.** Listing endpoints return
  `CursorPaginatedResponse<T> = { data: Vec<T>, total?, limit, hasMore, nextCursor }`.
  The cursor is opaque; pass the previous response's `nextCursor` back as
  `?cursor=`. Address-cell cursors encode `script_hash + block_num + tx_hash +
output_index`; activity cursors encode `block_num:tx_idx`; spore/cluster list cursors
  encode `block_num:0x<32-byte-id>` over a `(block DESC, id ASC)` order.
- **Address inputs.** Any `lock_script_hash` parameter accepts either a CKB
  address (auto-decoded) or a 0x-prefixed lock hash. `type_script_hash` accepts
  only the 0x-prefixed 32-byte hash.
- **Non-JSON responses.** A few spore endpoints return raw binary or SVG; they
  are flagged in the inventory below.
- **WebSocket.** Real-time subscriptions live under `crates/api/src/ws/` and are not part of this
  REST inventory.
- **Page-level routes.** `/llms.txt`, `/llms-full.txt`, `/capabilities`, and
  network-scoped `.md`/`.raw` page suffixes are served by the frontend layer (see
  `frontend/public/llms.txt`), not by either API base path.

## Modules

`api_routes()` (`crates/api/src/routes/mod.rs`) merges 19 modules.
Four files in `routes/` are helpers rather than mounted modules: `tx_lookup.rs`
(imported by `transactions.rs`), `proposal_window.rs` (the shared
proposal-window scan used by `blocks.rs` and `graph.rs`), `address_presence.rs` (the one
"does this address exist" rule, chain view plus tx pool, used by `search.rs`) and
`address_pool.rs` (an address's tx-pool segment: the page-one pool rows and `pool` summary of the
activity and transaction lists, and the address detail's `pendingSummary`).

---

### activities (crates/api/src/routes/activities.rs)

| Method | Path                                  | Handler                  | Purpose                                                                                  |
| ------ | ------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------- |
| GET    | `/api/v1/addresses/{addr}/activities` | `get_address_activities` | List a single address's transaction activities (cursor-paginated, optional `filter`)     |
| GET    | `/api/v1/activities`                  | `get_global_activities`  | List global recent activities across all addresses (cursor-paginated, optional `filter`) |
| GET    | `/api/v1/activities/latest`           | `get_latest_activities`  | Return the latest N activities (no cursor; capped 1-64)                                  |

**Params**

- `Path(addr): Path<String>` — CKB address or hex lock_hash
- `ActivityParams` — `limit`, `cursor`, `filter` (used by `get_address_activities` and `get_global_activities`)
- `LatestActivityParams` — `limit` only

**Responses**

- `CursorPaginatedResponse<ActivityResponse>` — per-address activity (tx + Layer1/2/3 deltas, participants)
- `CursorPaginatedResponse<GlobalActivityResponse>` — tx-level global activity
- `Vec<GlobalActivityResponse>` — plain vector for latest

**The scripts an activity names** (both shapes; the feeds fold consecutive rows by what was done
and these codes, never their args)

- `itemTypes: [{ typeCodeHash, typeHashType, scriptName }] | null` — the type scripts the
  transaction's items and its Nervos DAO step carry, each code (code hash and hash type) once, in
  code-hash order: the type script of every token, object and identity any participant gained or
  lost, and the Nervos DAO's when it took a DAO step; `scriptName` is the registry's name. Each is
  what the indexer classified, read from ckbadger's own stores in the request's view, never a
  match on bytes a transaction carries (anyone can add an output whose args equal an item's id):
  - a token: its row (`CF_TOKENS`), the script itself;
  - a spore, a did:ckb, a .bit Cell, a `.cell` name: the first cell the indexer recorded for the
    item (`object_outpoint_by_id`); a .bit account: the first cell recorded for its account id;
  - an m-NFT token: the outputs of its creating transaction the indexer recorded as the token
    (`object_outpoints`, MNFT_TOKEN);
  - a Spore cluster, for which no cell is recorded: the outputs of its creating transaction the
    classifier would take as the cluster (a code the protocol registry names `Cluster`, the
    cluster's id as args).

  `null` on a pool row (a pending row never folds), and on a committed row whose item's script the
  stores cannot say (no row, no recorded cell, a recorded payload without a type, two codes for one
  item): `itemTypesUnread` then says which item and why. That part fails alone; the page answers,
  and a feed folds such a row with no other.

- `itemTypesUnread: string | null` — see `itemTypes`.
- `typeCalls` — the type scripts the indexer does not recognise (an item's type never is one),
  with their args.
- `lockCalls` — the non-standard locks attached to the transaction's OUTPUTS, with their args.
  An output's lock does not run: these are not the locks the transaction called. The locks that
  ran, its inputs', are not stored per activity yet; they wait for the indexer's per-participant
  input flag (a re-sync).

**Participants**

A participant is any party the transaction affects, not only a holder of one of its cells. Both
response shapes describe a party the same way:

```json
{
  "address": "ckb1q...",
  "lockHash": "0x...",
  "lockHashPrefix": null,
  "roles": []
}
```

- A party that held a cell carries `address` + `lockHash`, and `lockHashPrefix` is `null`.
  `address` is `null` only when no lock script is known for `lockHash` (a tx-pool row resolves
  the lock from the pool transaction's own cells first) — never a hex string posing as an
  address. A stored lock script that cannot be encoded is a `500` naming the lock hash.
- A party a protocol NAMED by the first 20 bytes of its lock hash always carries
  `lockHashPrefix`. `address` and `lockHash` are filled in when exactly one known lock script
  starts with that prefix, and are `null` when none does — an unresolved party is reported as its
  prefix, never as a fabricated address. Two matching lock scripts is a `500`, not a guess.
- `roles` lists what the protocol said the party did (`owner_from`, `owner_to`, `manager_to`);
  empty for a plain cell participant.
- A named party that held no cell has `ckbDelta` and `usedDelta` of exactly `"0"` — that zero is
  what happened, not a missing value — and its `addr_txs` row reports `txType: "named"`.
- `ActivityResponse.participants` are the OTHER parties; `ActivityResponse.roles` are this
  participant's own roles.

**Item deltas**

Each item a transaction moves is one `ItemDeltaResponse`, tagged by `kind`:

```json
[
  { "kind": "token", "typeScriptHash": "0x...", "delta": "-1000", "symbol": "...", "decimals": 8 },
  { "kind": "object", "objectId": "0x...", "delta": 1 },
  { "kind": "identity", "identityId": "0x...", "delta": 1, "standard": "dotcell" }
]
```

- `standard` is required on every identity delta and is the standard's wire value:
  `dotbit | bit_cell | did_ckb | dotcell`. It is what the indexer recorded when it emitted the
  delta (`ItemKind::Identity(standard)`), not a store lookup, so a pending registration of a name
  the store does not hold yet carries it too. `.cell`, `.bit` and short did:ckb ids are all 20
  bytes, so the id alone cannot say which item page it belongs to.

---

### assets (crates/api/src/routes/assets.rs)

| Method | Path                                                             | Handler                                | Purpose                                                                               |
| ------ | ---------------------------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------- |
| GET    | `/api/v1/assets`                                                 | `list_assets`                          | List all assets (tokens + objects + identities), cache-backed with search/sort/cursor |
| GET    | `/api/v1/assets/summary`                                         | `asset_summary::get`                   | Exact catalog counts and composition in one pinned canonical view                     |
| GET    | `/api/v1/assets/objects/items/{object_id}`                       | `get_object_item_detail`               | Get mNFT token item detail (class, issuer, lifecycle, composition)                    |
| GET    | `/api/v1/assets/objects/items/{object_id}/activities`            | `list_mnft_item_activities`            | List per-item mNFT activities (cursor-paginated, optional `action`)                   |
| GET    | `/api/v1/assets/objects/{collection_id}`                         | `get_object_collection`                | Get object/Spore collection detail (counts, composition, owned capacity)              |
| GET    | `/api/v1/assets/objects/{collection_id}/items`                   | `list_object_collection_items`         | List items in an object collection (search/status/cursor)                             |
| GET    | `/api/v1/assets/objects/{collection_id}/holders`                 | `list_object_collection_holders`       | List ranked holders of an object collection (cursor)                                  |
| GET    | `/api/v1/assets/objects/{collection_id}/activities`              | `list_object_collection_activities`    | List collection-level activities (cursor, optional `action`)                          |
| GET    | `/api/v1/assets/objects/{collection_id}/charts/capacity-history` | `get_object_collection_capacity_chart` | Stacked-area capacity history chart (date range)                                      |

`/assets` answers from the in-memory snapshot the warmup rebuilds when the tip moves, not from the store view of the request, and says so: beside the page it returns `anchor: {blockNumber,blockHash} | null`, the block that snapshot was built at. A row's counts (`holdersCount`, `totalSupply`, `liveCount`, …) are that block's; an asset's own endpoints read the store and can already answer a block later, so a row is comparable with its detail only when the detail was read at `anchor`. `anchor` is `null` when the tip moved while the snapshot was built (its rows then span more than one block), when the token and object snapshots of an unfiltered page stand at different blocks, and on a store with no block.

`/assets/summary` returns `source: "canonical_asset_indexes"`, `anchor: {blockNumber,blockHash} | null`, `counts: {token,object,identity}`, `objectStandards: [{standard,collectionsCount,liveCount}]`, `objectComposition: [{tier,collectionsCount,liveCount}]`, `fullyOnchainObjectsCount`, and `pendingItemsCount`. Composition rows classify whole collections; their `liveCount` counts members of those collections, not individual-item tiers. `pendingItemsCount` independently counts every live pending Spore, including members of mixed collections and the existing Sole Spores sentinel collection. A fully pending collection has tier `decode_pending`; mixed collections retain their normal aggregate classification. The same owning catalog supplies warmup membership/classification and the summary; no page sampling is used.

The summary's request pins its canonical view. `/assets` remains a warmup snapshot, so separate list and summary requests do not claim a shared anchor. Summary reads token metadata, checks only enough canonical holder rows to establish positive membership for unlabelled tokens, scans collection aggregates and live Spore pending evidence; it does not run all token capacity/history/holder-sum scans. `composition_tier` additionally accepts `decode_pending`; filtering precedes pagination and `total` covers the entire matching snapshot.

Collection detail's `activitiesCount` counts canonical collection transactions, not individual item actions or transferred items. Spore detail and Inventory reuse the existing canonical-history counter (five-minute in-memory TTL); mNFT and identity collections use their owning persisted counters. Separate responses and the independently refreshed Inventory snapshot do not promise the same tip. A known Spore cluster with no indexed members, including the virtual Sole Spores collection, has a valid empty detail/items/holders/history/feed surface even before its first member is minted. A member index with a missing aggregate is an invariant error, not an empty collection; an unknown ID remains 404.

**Params**

- `ListParams` — `limit`, `type` (token|object|identity), `standard`, `cursor`, `search`, `sort_key`, `sort_direction`, `composition_tier`
- `ObjectItemsParams` — `limit`, `cursor`, `search`, `status`
- `MnftItemActivitiesParams` — `limit`, `cursor`, `action`
- `CollectionHoldersParams` — `limit`, `cursor`
- `CollectionActivitiesParams` — `limit`, `cursor`, `action`
- `ChartRangeParams` — `from`, `to`
- `Path<String>` for `object_id`, `collection_id`. `collection_id` is a hex id or, for an
  identity collection, one of the identity aliases listed under identities below — so the
  `.cell` collection page's chart is `/assets/objects/dotcell/charts/capacity-history`

**Responses**

- `CursorPaginatedResponse<AssetResponse>` — id, type, standard, name, holders/transfers counts, owned capacity
- `MnftItemDetailResponse` — nft_id, class, issuer, lifecycle, composition
- `CursorPaginatedResponse<MnftItemActivityResponse>` — per-item activity entries
- `NftCollectionDetailResponse` — counts, composition, class/issuer summary
- `CursorPaginatedResponse<CollectionItemResponse>` — items
- `RankedPage<CollectionHolderResponse>` — holders with item counts, a cursor page with `firstRank`
- `CursorPaginatedResponse<CollectionActivityResponse>` — collection activities
- `StackedAreaChartResponse` — `data` + `series` (defined in `statistics.rs`). Each day's `values` carry `occupiedCapacity`, `freeCapacity` and `capacity`, the whole its cells hold, all three stated by the route (`held_point` in `asset_calendar.rs`, the free part by the one helper `utils::assets::free_part`, which refuses an occupied part under zero or above the whole, the day failing with its date and its operands), so a page sums nothing; the series are `occupiedCapacity` ("Occupied") and `freeCapacity` ("Free"), from one helper (`held_series`). They were `used` and `unused` (part B, the third read's S4: one name for a held point's parts on every route, as the token history and the cell versions name them).

---

### identities (crates/api/src/routes/identities.rs)

| Method | Path                                                                | Handler                               | Purpose                                                                                  |
| ------ | ------------------------------------------------------------------- | ------------------------------------- | ---------------------------------------------------------------------------------------- |
| GET    | `/api/v1/assets/identities/dotbit/items/{identity_id}`              | `get_dotbit_item_detail`              | Get .bit AccountCell item detail                                                         |
| GET    | `/api/v1/assets/identities/dotbit/items/{identity_id}/activities`   | `list_dotbit_item_activities`         | List .bit AccountCell activities                                                         |
| GET    | `/api/v1/assets/identities/did/items/{identity_id}`                 | `get_did_ckb_item_detail`             | Get did:ckb item detail                                                                  |
| GET    | `/api/v1/assets/identities/did/items/{identity_id}/activities`      | `list_did_ckb_item_activities`        | List did:ckb item activities                                                             |
| GET    | `/api/v1/assets/identities/bit-cell/items/{identity_id}`            | `get_bit_cell_item_detail`            | Get independent .bit Cell identity detail                                                |
| GET    | `/api/v1/assets/identities/bit-cell/items/{identity_id}/activities` | `list_bit_cell_item_activities`       | List .bit Cell identity activities                                                       |
| GET    | `/api/v1/assets/identities/dotcell/ring`                            | `get_dotcell_ring`                    | Get the `.cell` uniqueness ring root (namespace, root outpoint, first id, live count)    |
| GET    | `/api/v1/assets/identities/dotcell/items/{id_or_name}`              | `get_dotcell_item_detail`             | Get a `.cell` name by 20-byte id, `alice` or `alice.cell`                                |
| GET    | `/api/v1/assets/identities/dotcell/items/{id_or_name}/activities`   | `list_dotcell_item_activities`        | List a `.cell` name's activities                                                         |
| GET    | `/api/v1/assets/identities/dotcell/items/{id_or_name}/children`     | `list_dotcell_item_children`          | List a `.cell` name's live sub-names after the detail's first page (cursor-paginated)    |
| GET    | `/api/v1/assets/identities/{collection_id}`                         | `get_identity_collection`             | Get .bit AccountCell, .bit Cell, did:ckb or `.cell` collection aggregate (aliases below) |
| GET    | `/api/v1/assets/identities/{collection_id}/holders`                 | `list_identity_collection_holders`    | List ranked holders of an identity collection                                            |
| GET    | `/api/v1/assets/identities/{collection_id}/activities`              | `list_identity_collection_activities` | List identity collection activities (cursor, optional `action`)                          |
| GET    | `/api/v1/assets/identities/{collection_id}/items`                   | `list_identity_collection_items`      | List identity items in a collection (search/status/cursor)                               |

**Params**

- `Path<String>` for `identity_id` / `collection_id`. A `collection_id` is a hex sentinel id or
  one of the aliases in the store's one identity table (`ckbadger_store::types::identity_aliases`,
  case-insensitive): `dotbit` / `.bit`, `bit_cell` / `bit-cell` / `.bit-cell`, `did:ckb` /
  `did_ckb`, `dotcell` / `.cell`. There is no bare `cell` alias.
- `id_or_name` for a `.cell` name: the 20-byte id (`0x` + 40 hex) or the label, case-insensitive,
  with or without `.cell` (`alice`, `ALICE.cell`)
- `DotCellChildrenParams` — `limit` (default 50, max 200), `cursor` (the last child's 20-byte
  name id, as returned in `nextCursor` / `childrenNextCursor`)
- `IdentityCollectionHoldersParams` — `limit`, `cursor`
- `MnftItemActivitiesParams` (re-used from assets.rs) — `limit`, `cursor`, `action`
- `CollectionActivitiesParams` (from assets.rs) — `limit`, `cursor`, `action`
- `ObjectItemsParams` (from assets.rs) — `limit`, `cursor`, `search`, `status`

**Responses**

- `IdentityCollectionDetailResponse` — collection_id, standard, name, counts, owned capacity/knowledge
- `RankedPage<CollectionHolderResponse>` (from assets.rs) — a cursor page with `firstRank`
- `CollectionActivityResponse` (from assets.rs)
- `CollectionItemResponse` (from assets.rs) — used for item detail and item list
- `CursorPaginatedResponse<MnftItemActivityResponse>` — per-item activities
- `DotCellRingResponse` — `namespaceArgs`, `rootOutPoint`, `firstId`, `liveCount`; `404` on a
  network with no `.cell` ring root indexed
- `DotCellItemResponse` — the name, its owner/manager/sale/records, `parent`, and the **first
  page** of its live sub-names: `children` (at most 50), `childrenHasMore`, `childrenNextCursor`
- `PartyRef` (a `.cell` name's owner, manager and sale seller) — `hashPrefix`, `lockHash`,
  `address`, and `lockScriptName`, the registry's name for the party's lock code hash, the one
  name every route gives a lock's registry name (it was `scriptName`, the word a type call and a
  code cell keep)
- `CursorPaginatedResponse<DotCellNameRef>` — `…/children`: `identityId`, `label`, `name`

**`.cell` sub-names**

The parent→child index lists every sub-name ever registered under a parent, recycled ones
included; liveness is decided on read, so `children` and `…/children` serve live sub-names only,
in id order. The detail carries the first 50; `…/children?cursor=<childrenNextCursor>` pages
through the rest with the same `nextCursor` contract as every list. A child row that is missing
or not a `.cell` identity is a `500` naming the parent and the child.

---

### blocks (crates/api/src/routes/blocks.rs)

| Method | Path                                 | Handler               | Purpose                                                              |
| ------ | ------------------------------------ | --------------------- | -------------------------------------------------------------------- |
| GET    | `/api/v1/blocks`                     | `list_blocks`         | List blocks (cursor-paginated, DESC)                                 |
| GET    | `/api/v1/blocks/{id}`                | `get_block`           | Get block by number or 0x-prefixed hash (with miner/reward/hardfork) |
| GET    | `/api/v1/blocks/{id}/fee-stats`      | `get_block_fee_stats` | Per-block fee-rate stats (avg/min/max) and cycles                    |
| GET    | `/api/v1/blocks/{id}/proposals`      | `get_block_proposals` | List block proposals with linkage to committed transactions          |
| GET    | `/api/v1/blocks/{id}/uncles/{index}` | `get_block_uncle`     | Full header of the uncle at `index` in canonical block `{id}`        |

**Params**

- `ListParams` — `limit`, `cursor` (i64 block-number cursor)
- `Path(id): Path<String>` — block number string or hex hash
- `Path((id, index))` — the including canonical block (as above) and the uncle's position in its
  uncle list

**Responses**

- `CursorPaginatedResponse<BlockResponse>` — block metadata + miner/hardfork activation
- `BlockResponse` — single block
- `BlockFeeStatsResponse` — total_size, total_cycles, fee-rate stats, `cycles_pending`
- `UncleDetailResponse` — the uncle's header as the including block embeds it (`number`, `hash`,
  `parentHash`, `timestamp`, `epoch`/`epochNumber`/`epochIndex`/`epochLength`, `compactTarget`,
  `difficulty`, `nonce`, `version`, `transactionsRoot`, `proposalsHash`, `extraHash`,
  `dao: {raw, c, ar, s, u}`, `proposalIds`) plus `includedBy: {number, hash, index}` and
  `canonicalAtHeight: {number, hash}` (the canonical block that won the uncle's height). Uncles are
  not canonical blocks, so `/blocks/{uncle hash}` stays 404; an unknown index or a non-canonical
  including block is 404
- `Vec<BlockProposal>` — `proposalIndex`, `proposalId`, `committedTxHash`, `committedBlockNumber`.
  Commitments are resolved through the shared NC-Max commit-window scan (`+2..=+10`, first matching
  tx in chain order wins — the same `resolve_committed_txs` path `/graph/proposals` uses). A
  proposal not committed within the window blocks that exist (e.g. tip-adjacent windows) reports
  both fields as `null`.

---

### transactions (crates/api/src/routes/transactions.rs)

| Method | Path                                           | Handler                      | Purpose                                                                 |
| ------ | ---------------------------------------------- | ---------------------------- | ----------------------------------------------------------------------- |
| GET    | `/api/v1/transactions`                         | `list_transactions`          | List transactions (latest first, or filtered by `block_number`)         |
| GET    | `/api/v1/transactions/{hash}`                  | `get_transaction`            | Get transaction summary by hash                                         |
| GET    | `/api/v1/transactions/{hash}/detail`           | `get_transaction_detail`     | Get full transaction (inputs/outputs/witnesses with scripts)            |
| GET    | `/api/v1/transactions/{hash}/cell-deps`        | `get_cell_deps`              | List the transaction's cell_deps (read from CKB RocksDB)                |
| GET    | `/api/v1/transactions/{hash}/cycles`           | `get_cycles_status`          | Get cycles calculation status (Done/Calculating/Queued/Failed/NotFound) |
| GET    | `/api/v1/transactions/{hash}/lifecycle`        | `get_transaction_lifecycle`  | Tx lifecycle (Pending\|Committed) + proposal block + commitment window  |
| POST   | `/api/v1/transactions/{hash}/calculate-cycles` | `trigger_cycles_calculation` | Enqueue and (briefly) wait for cycles calculation                       |

**Params**

- `ListParams` — `limit`, `block_number`, `cursor`, `activity_type`: one of `user`, `ckb`,
  `script`, `cellbase`, `dao`, `token`, `object`, `identity` (none or `all`: every transaction).
  `user` is every committed transaction but the cellbase, newest first, read from the
  `activities` rows (which hold no cellbase) with their `tx_index` entries; the Transactions page
  opens on it. A filtered list states no `total`. `/transactions/pool` takes the same values;
  there `user` names every pool transaction (none is a cellbase), a partly read one too.
- `Path(hash): Path<String>` — tx hash (with or without `0x`)

**Responses**

- `CursorPaginatedResponse<TransactionResponse>` / `TransactionResponse` — hash, block_number, fee, cycles
- `TransactionDetailResponse` — full detail with `inputs`, `outputs`, witnesses, fee rate, confirmations
- `Vec<CellDepResponse>` — out_point + dep_type. Fail-fast: `500` when the CKB RocksDB reader is
  unavailable (the endpoint's only data source), `400` for a pending/proposed mempool tx, `404`
  when the tx exists nowhere — never a silent `200 []` for those states
- `CyclesStatusResponse` (from `crate::cycles`) — status, cycles, error
- `TransactionLifecycleResponse` — phase, proposal_id, `proposed_in`/`committed_in`, commitment_distance, `commitment_window: CommitmentWindow`

**`/transactions/{hash}/detail` for a transaction the store has not indexed**

- A transaction the node holds in its pool (`status` `pending` / `proposed`) or has committed
  before this process's store indexed it (`status` `committed`, `poolStatus`
  `committed_awaiting_index`) is served provisionally from the node: `poolStatus` and
  `interpretation` are set, `timestamp` / `confirmations` / `index` are absent, and a committed one
  always carries `blockNumber` and `blockHash`.
- Each input resolves through one order: an output of a transaction the tx-pool mirror's snapshot
  holds, otherwise `outputs[index]` of the parent from the node's `get_transaction`. A spent input
  of a committed transaction resolves the same way as a chained pool spend.
- `fee` has one source: Σ inputs + Σ exact Nervos DAO compensation of any withdraw-request inputs
  − Σ outputs, over fully resolved inputs. The node's pool fee is not read.
- An input whose parent transaction the node does not know yet is a `503`
  `service_unavailable` naming the outpoints ("retry") — never a response with a partial fee or
  unresolved inputs.

---

### cells (crates/api/src/routes/cells.rs)

| Method | Path                                             | Handler                      | Purpose                                                                                                                                                                         |
| ------ | ------------------------------------------------ | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/cells/live-summary`                     | `get_live_cell_summary`      | Exact O(1) live-cell class/data counts at a verifiable canonical tip                                                                                                            |
| GET    | `/api/v1/cells/live`                             | `list_live_cells`            | List live cells with optional lock/type/type_code_hash filters (cursor)                                                                                                         |
| GET    | `/api/v1/cells/by-script`                        | `list_cells_by_script`       | List cells matching a script reference (`code_hash`, `hash_type`, `script_kind=lock\|type\|both`)                                                                               |
| GET    | `/api/v1/cells/{tx_hash}/{output_index}`         | `get_cell`                   | Get cell detail (live or consumed) including data/dep_group/code_cell_of/DAO info                                                                                               |
| GET    | `/api/v1/addresses/top`                          | `get_top_addresses`          | Top addresses by balance (warmup cache)                                                                                                                                         |
| GET    | `/api/v1/addresses/active`                       | `get_active_addresses`       | Most-active addresses in last N `days` (warmup cache)                                                                                                                           |
| GET    | `/api/v1/addresses/{addr}`                       | `get_address`                | Address summary (balance, used_capacity, live_cells_count, lock_script info)                                                                                                    |
| GET    | `/api/v1/addresses/{addr}/transactions`          | `get_address_transactions`   | List recent transactions for an address (cursor)                                                                                                                                |
| GET    | `/api/v1/addresses/prefix/{prefix}/transactions` | `get_prefix_transactions`    | List the transactions a protocol NAMED a 20-byte lock-hash prefix in without it holding a cell (cursor) — the `addr_txs_by_prefix` half of an address's history, read as itself |
| GET    | `/api/v1/addresses/{addr}/tokens`                | `get_address_tokens`         | List token balances for an address (cursor)                                                                                                                                     |
| GET    | `/api/v1/addresses/{addr}/holdings`              | `address_holdings::holdings` | Exact current physical holdings and six-class live-cell capacity totals                                                                                                         |
| GET    | `/api/v1/addresses/{addr}/cells`                 | `address_holdings::cells`    | Current physical cells, optionally filtered by class before cursor pagination                                                                                                   |
| GET    | `/api/v1/addresses/{addr}/dotcell-names`         | `get_address_dotcell_names`  | List the `.cell` names this address owns (cursor). A name's owner is the first 20 bytes of its owner's lock hash, so this is a prefix seek on the address's own lock hash       |

**Params**

- `ListCellsParams` — `limit`, `lock_script_hash`, `type_script_hash`, `type_code_hash`, `cursor`
- `ListCellsByScriptParams` — `limit`, `code_hash`, `hash_type`, `script_kind` (default `both`), `cursor`
- `TopAddressesParams` — `limit` (default 100)
- `Path(prefix)` for `/addresses/prefix/{prefix}/transactions` — 40 hex characters, optional `0x`
  (the first 20 bytes of a lock hash); `AddressTxParams` — `limit` (default 50, max 100), `cursor`
  (`block_num:tx_idx`, from `nextCursor`)

**`transactionsCount` / `total`**

`transactionsCount` on `/addresses/{addr}` and `total` on `/addresses/{addr}/transactions` are the
same number: the address's cell participations (`addr_balance.txs_count`) plus the transactions a
protocol named it in without it holding a cell (`addr_prefix_stats`). They are added in one store
helper (`address_tx_count`) that every consumer calls, so the count can never disagree with the
merged transaction list beneath it. Pool rows stay out of both and are reported in `pool`.

`/addresses/{addr}` memoizes its chain view (balance, counts, lock script) per tip block: the
memo's key carries the tip's number and hash, as read in the request's pinned view, so the detail
never answers a block behind the address's transaction and live-cell totals, and a reorg to
another block at the same height is a miss. (Until 2026-11-15 the key named only the address and
the entry lived 30 s, so after a new block the detail's `transactionsCount` and `liveCellsCount`
could trail `total` on `/transactions` and `/cells/live` by that block.)

**`/cells/live-summary` response and consistency**

```json
{
  "tip": { "block": 12345678, "hash": "0x..." },
  "liveCells": 8421937,
  "classes": {
    "dao": 182341,
    "typedNonDao": 2639044,
    "plain": 5600552
  },
  "dataBearing": 2810388
}
```

- `dao`, `typedNonDao`, and `plain` are mutually exclusive and their checked sum is
  `liveCells`. `dataBearing` is orthogonal and counts cells whose data is non-empty.
- The indexer updates the fixed-size record from birth/spend transitions. Normal sync publishes it
  in the same domain-store batch as the canonical block header and sync status; bulk sync publishes
  it only after canonical history is finalized, atomically with the final sync status. The request
  performs only a constant number of point reads; it never scans live cells.
- During initial bulk sync and while a reorg/deep-fork reconciliation has withdrawn the current
  record, the endpoint returns `503` with `error: "initializing"`. Corrupt/inconsistent records
  fail with `500`; no zero/default result is synthesized.
- Consumers such as cknerv should verify `tip.hash` against the local CKB node's
  `get_block_hash(tip.block)` before accepting the counts.

**`/cells/by-script` behavior**

- Rows are the live cells of exactly one script reference form `(code_hash, hash_type)`, read
  directly from that form's contiguous range in `cell_by_lock_code` / `cell_by_type_code`.
- `cursor` is opaque and form-scoped. For `script_kind=lock|type` it is the hex-encoded 75-byte
  cell-by-code index key of the last returned row; for `script_kind=both` it is phase-composite —
  `lock:<hex>` or `type:<hex>`. Replaying a cursor against a different `code_hash`/`hash_type` form,
  or a bare key in `both` mode, is a `400`.
- `script_kind=both` enumerates the deduplicated union across pages: the lock form is exhausted
  first, then type-form rows whose lock script is not already the same form. Each cell appears
  exactly once.
- `total` is the per-form reference counter for `script_kind=lock|type`. For `script_kind=both` it
  is **omitted**: the deduplicated union count is not available from the per-form counters, and
  `lock + type` would double-count cells matching on both sides.
- Each row carries `matchedScriptKind` (`lock` | `type`) — which index enumerated it, and the phase
  a client would use when building a `both` cursor from that row.
- `ActiveAddressesParams` — `limit` (default 100), `days` (default 7)
- `AddressTxParams` — `limit`, `cursor`
- `AddressTokensParams` — `limit`, `cursor`
- `Path<String>` (`addr`) and `Path<(String, i32)>` (`tx_hash`, `output_index`)

**Responses**

- `CursorPaginatedResponse<CellResponse>` — cell summary
- `CellDetailResponse` — full cell detail with `lock`/`type_script` (`ScriptResponse`), `data`, `data_analysis`, `dep_group_items`, `code_cell_of: Vec<CodeCellScript>`, `dao_info: Option<DaoInfo>`, `used_capacity_breakdown: OccupiedCapacityBreakdown`, and the cell's class as the transaction detail serves it:
  - `typeCellType: string` — always present: the cell's class by its type script
    (`transaction_structure::cell_type`), `plain | sudt | xudt | <registry slug> | typed` — the
    same string the transaction detail serves as this cell's `cellType`, as an output and as an
    input (save the genesis Satoshi output, whose `cellType` there is the burn marker). `cellType`
    on this endpoint stays the genesis burn marker: `genesis_special_burn` on that output, absent
    on every other cell.
  - `display: TransactionCellDisplay | null` — the token / object / identity / file display the
    transaction detail serves for this cell (`transaction_cells::from_parts`), `null` when its
    type script has none.
- `AddressResponse` — lock_script_hash, balance, used_capacity, lock_script (`ScriptResponse`) + `lock_script_info: LockScriptInfo` (`deprecated` reflects the stored script record, e.g. an address on a superseded ACP deployment reports `true`).
  `address` is always the canonical lowercase RFC-0021 encoding of the lock script on the serving
  network — never the raw input echoed back. `pendingSummary` is described below
- `CursorPaginatedResponse<PrefixTransactionResponse>` — `txHash`, `blockNumber`, `txIndex`,
  `capacityChange`, `txType` (`named`), `tags` (participant tag bitmask)

**`pendingSummary` on `/addresses/{addr}`**

```json
{ "pendingSummary": { "txCount": 2, "capacityDelta": "-10000000000" } }
```

- Always present. `capacityDelta` is the address's signed net capacity change in shannons, as a
  decimal string, over its page-one tx-pool segment: the pool transactions involving it that are
  not indexed yet, at most 200.
- Independent of any list filter, and computed per request — never served from the detail's
  per-block memo.
- `null` when the tx-pool mirror is disabled or unhealthy: there is no pool view to summarise,
  and "0 pending" would be a guess. A healthy mirror with nothing pending gives
  `{ "txCount": 0, "capacityDelta": "0" }`.

**Address inputs (`{addr}`, all address endpoints)**

- One shared parser (`parse_address_to_script`): RFC-0021 full format (`0x00`) only, Bech32m
  checksum enforced (the legacy Bech32 checksum is a `400` naming the reason), `hash_type` must be
  `0x00|0x01|0x02|0x04`.
- Case follows the bech32 spec: all-lowercase and ALL-UPPERCASE both accepted, mixed case rejected.
- A 64-hex-char lock script hash (optional `0x`) is accepted everywhere an address is.
- `Vec<TopAddressResponse>` / `Vec<ActiveAddressResponse>`
- `CursorPaginatedResponse<AddressTransactionResponse>` — tx_type, capacity_change, script_labels
- `CursorPaginatedResponse<AddressTokenResponse>` — per-address token balance

**Current address holdings (Address v3)**

`GET /addresses/{addr}/holdings` streams the complete existing owner live-cell
index in one pinned read view. It returns:

- `lockScriptHash`, `balance`, `occupiedCapacity` (every live cell's),
  `daoCellsCapacity`, `otherCellsOccupiedCapacity`, `otherCellsFreeCapacity`,
  `liveCellsCount`.
- `classes`: fixed keys `dao`, `token`, `object`, `identity`, `script`, `plain`;
  each contains `cellCount`, `capacity`, `occupiedCapacity`.
- `holdings`: `dao` counts live DAO cells (both phases), `token` counts distinct
  types with a positive balance, `object` and `identity` count current items.
  A zero-amount token cell contributes capacity and a token cell count, but no
  positive-balance token holding; this agrees with `/addresses/{addr}/tokens`.
- `depositCellsCount`, `withdrawalRequestCellsCount`, `daoUnclaimedCompensation`,
  `estimatedApc` (string, or null without current DAO cells).
- `objectComposition`: fixed counters `pureCkb`, `btcCkb`,
  `decentralizedMixture`, `centralizedMixture`, `unknown`, `decodePending`.
- `objectCollections`: `{id: string|null, standard: string, name: string|null,
itemCount: number, compositionCounts: <the same six counters>}`. These are
  whole-owner totals, independent of the currently displayed item page.

All capacities/compensation are exact decimal **shannon strings**. The balance
is stated in three disjoint parts, each true by its name, which make it by their
definition (the balance is the classes' capacity summed):
`balance = daoCellsCapacity + otherCellsOccupiedCapacity + otherCellsFreeCapacity`.
The check the route makes is of each live cell, in its scan: a cell whose
capacity is below its occupied bytes fails the request, named by its out point
(part B, the third read's N1).
`daoCellsCapacity` is the whole capacity of the address's Nervos DAO cells
(deposits and withdrawal requests, the `dao` class); `otherCellsOccupiedCapacity`
is what pays for the bytes of its other cells; `otherCellsFreeCapacity` is the
rest of its other cells, the CKB they hold beyond what their bytes occupy. No
part is "spendable": a lock, a `since` or a cellbase's maturity decides that,
and the route reads none of them. (`freeCapacity` and `daoFreeCapacity`, the
other cells' free part and the DAO cells' free part, are gone.) DAO holdings follow the current cell
lock, including a withdrawal request received from another depositor. The older
`/dao/deposits/{lock_hash}` and `/dao/summary/{lock_hash}` describe the original
depositor's lifecycle history and must not stand in for current holdings.

`GET /addresses/{addr}/cells` accepts `class=dao|token|object|identity|script|plain`
(omit for all), `limit` (default 50, range 1–100), and an opaque `cursor`. Cursors
are bound to the owner and class; changing either requires starting a new page.
The response is `CursorPaginatedResponse<AddressCell>`, with an exact `total` of
all matching current cells. Each row has these required keys:

```typescript
interface AddressCell {
  txHash: string;
  outputIndex: number;
  capacity: string;
  occupiedCapacity: string;
  lockScriptHash: string;
  typeScriptHash: string | null;
  typeCodeHash: string | null;
  typeHashType: string | null; // exact data/type/data1/data2, never inferred
  dataSize: number;
  createdAtBlock: number;
  class: 'dao' | 'token' | 'object' | 'identity' | 'script' | 'plain';
  typeCellType: string; // plain | sudt | xudt | <registry slug> | typed: the transaction detail's cellType
  display: TransactionCellDisplay | null;
  expiresAt: number | null; // identity Unix seconds, where the protocol has expiry
  heldSinceBlock: number | null; // identity continuous physical ownership
  dao: DaoDepositResponse | null; // original deposit evidence for this current cell
}
```

`class` groups the page's filters and totals; `typeCellType` is the cell's class
by its type script (`transaction_structure::cell_type`), the same value the cell
detail serves as `typeCellType` and the transaction detail as `cellType`.
`display` reuses the transaction-cell descriptor. Current mNFT holdings may use
the current class name/composition, while historical transaction tiles retain
their immutable-evidence rules. `createdAtBlock` is the current outpoint creation
height; identity `heldSinceBlock` additionally follows canonical same-owner
consume/recreate links. Transfers and disconnected burn/remint lifecycles reset
that interval. For a DAO request, the row outpoint/lock is the current cell and
`dao.txHash`/`dao.lockScriptHash` identify the original deposit/depositor.

Both endpoints use O(owner live cells) reads with streaming aggregation and no
scan ceiling. The page retains at most `limit + 1` cells. The summary retains
distinct item/token identities and collection counters, not every cell payload,
and reads no raw node transactions. Only returned cell descriptors requiring
raw bytes read the configured node RocksDB; absence is an explicit error, with
no RPC fallback. Identity page rows also read that item's existing reverse
outpoint history. Broken index targets, inconsistent balance aggregates, or
contradictory item evidence are errors, not partial totals. No store is written.

These physical counts deliberately exclude protocol-named `.cell` rights held
through an owner/manager lock-hash prefix. Keep `/addresses/{addr}/dotcell-names`
as a separate named-rights view; do not add its cells/capacity to this partition.

Address transaction rows expose required `cycles: number|null` and
`cyclesStatus: 'complete'|'pending'|'failed'`. Committed cellbase/genesis rows
have zero complete execution; pending/failed persisted sentinels are never
exposed as measured cycles. Pool rows use the node's exact cycles and complete
status.

---

### statistics (crates/api/src/routes/statistics.rs)

| Method | Path                                          | Handler                                  | Purpose                                                                                                    |
| ------ | --------------------------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/statistics/network`                  | `get_network_stats`                      | Network stats (tip, hash rate, difficulty, TPS, DAO totals, sync/deep-fork status)                         |
| GET    | `/api/v1/statistics/tx-stats`                 | `get_tx_stats`                           | Hourly + daily transaction-count timeseries (24h + 14d)                                                    |
| GET    | `/api/v1/statistics/recent-blocks`            | `get_recent_blocks`                      | Recent 24h blocks (timestamp + tx count)                                                                   |
| GET    | `/api/v1/charts/transaction-count`            | `get_transaction_count_chart`            | Daily transaction count chart                                                                              |
| GET    | `/api/v1/charts/cell-count`                   | `get_cell_count_chart`                   | Cell count chart (all/live/dead stacked area)                                                              |
| GET    | `/api/v1/charts/knowledge-size`               | `get_knowledge_size_chart`               | Common-knowledge-size chart with utilization%                                                              |
| GET    | `/api/v1/charts/common-knowledge-composition` | `get_common_knowledge_composition_chart` | Stacked area: transfer / DAO / UDT / NFT-Spore / other contracts                                           |
| GET    | `/api/v1/charts/capacity-turnover-ratio`      | `get_capacity_turnover_ratio_chart`      | Daily + weekly capacity turnover chart                                                                     |
| GET    | `/api/v1/charts/cell-size-distribution`       | `get_cell_size_distribution_chart`       | Cell-size histogram chart                                                                                  |
| GET    | `/api/v1/charts/address-cohort-retention`     | `get_address_cohort_retention_chart`     | Address cohort retention chart                                                                             |
| GET    | `/api/v1/charts/most-utilized-scripts`        | `get_most_utilized_scripts_chart`        | Top scripts share charts (used + capacity)                                                                 |
| GET    | `/api/v1/charts/most-utilized-assets`         | `get_most_utilized_assets_chart`         | Top assets share charts (used + capacity)                                                                  |
| GET    | `/api/v1/charts/block-time-distribution`      | `get_block_time_distribution_chart`      | Block time distribution histogram (last 42 epochs or fewer) + its window                                   |
| GET    | `/api/v1/charts/epoch-time-distribution`      | `get_epoch_time_distribution_chart`      | Epoch duration histogram                                                                                   |
| GET    | `/api/v1/charts/epoch-time-length`            | `get_epoch_time_length_chart`            | Per-epoch length (whole ms) + blocks                                                                       |
| GET    | `/api/v1/charts/average-block-time`           | `get_average_block_time_chart`           | Daily average block time (whole ms)                                                                        |
| GET    | `/api/v1/charts/hash-rate`                    | `get_hash_rate_chart`                    | Daily hash-rate chart                                                                                      |
| GET    | `/api/v1/charts/difficulty`                   | `get_difficulty_chart`                   | Daily difficulty chart                                                                                     |
| GET    | `/api/v1/charts/uncle-rate`                   | `get_uncle_rate_chart`                   | Daily uncle-rate chart                                                                                     |
| GET    | `/api/v1/charts/miner-address-distribution`   | `get_miner_address_distribution_chart`   | Miner distribution for the last 7 complete UTC+8 days                                                      |
| GET    | `/api/v1/charts/total-supply`                 | `get_total_supply_chart`                 | Total supply (circulating / DAO-locked / burnt) stacked area                                               |
| GET    | `/api/v1/charts/nominal-apc`                  | `get_nominal_apc_chart`                  | Synthesized nominal APC curve (no DB read)                                                                 |
| GET    | `/api/v1/charts/secondary-issuance`           | `get_secondary_issuance_chart`           | Secondary issuance breakdown plus exact protocol-total and frozen phase-1 shannon metadata                 |
| GET    | `/api/v1/charts/inflation-rate`               | `get_inflation_rate_chart`               | Exact trailing-365-day nominal & real inflation from DAO snapshots                                         |
| GET    | `/api/v1/charts/hodl-wave`                    | `get_hodl_wave_chart`                    | HODL-wave bands (under 1 day … 3 years or more) + holder_count                                             |
| GET    | `/api/v1/stats/daily-activities`              | `get_daily_activity_stats`               | Per-day activity breakdown by tx-type and script (param: `days`, default 30)                               |
| GET    | `/api/v1/stats/activity-summary-24h`          | `get_activity_summary_24h`               | Rolling 24h activity summary aggregated from hourly buckets                                                |
| GET    | `/api/v1/statistics/asset-ecosystem`          | `get_asset_ecosystem`                    | Top tokens + capacity-category breakdown (tokens/objects/DAO/other, shares of total live capacity `C - S`) |

**Params**

- `DailyActivityStatsParams` — `days` (only used by `get_daily_activity_stats`)
- Most other handlers take only `State<Arc<AppState>>` (no query)

**Responses**

- `NetworkStats` — tip block, hash rate, difficulty, epoch, TPS, sync_status, deep_fork_status, hero DAO metrics
- `TxStatsResponse` — current_hour/day + `hourly_data`/`daily_data: Vec<TxStatsDataPoint>`
- `RecentBlocksResponse` — `blocks: Vec<RecentBlockItem>`
- `ChartResponse` — `data: Vec<ChartDataPoint>`, `title`, axis labels (from `crate::response`)
- `StackedAreaChartResponse` — `data: Vec<StackedAreaDataPoint>`, `series: Vec<StackedAreaSeries>`, `title`
- `MostUtilizedScriptsChartResponse` / `MostUtilizedAssetsChartResponse` — pair of stacked-area share charts
- `MinerDistributionResponse` — `data: Vec<MinerDistributionDataPoint>`, explicit
  `windowDays`/`fromDate`/`toDate`; each row exposes the canonical
  `minerLockHash` and an optional resolvable CKB `address`
- `Vec<DailyActivityStatsResponse>` / `ActivitySummary24hResponse` — per-day or 24h activity counts + `script_counts: Vec<ScriptCountEntry>`
- `CellSizeDistributionChartResponse` — the cell-size histogram (`ChartResponse`, flattened) and its two wholes
- `BlockTimeDistributionChartResponse` — the block-interval histogram (`ChartResponse`, flattened), its window (`epochs`, `intervals`, `beyondRange`) and each bucket's count (`bucketCounts`)
- `AssetEcosystemResponse` — `top_tokens: Vec<TopTokenEntry>` + `capacity_breakdown: Vec<CapacityCategory>`

**What a chart page states (lane D1)**

A chart page draws at most five bands of a stack and states every share of a
whole the route sends; it never sums the bands itself. These fields carry the
bands it draws and the wholes it divides by, read-only on the store:

- `GET /charts/hodl-wave` — `bands: [{ key, label, members }]`, the five bands a
  page draws, oldest first: `gt3y`, `1y3y`, `6m1y`, `1m6m` (members `3m6m`,
  `1m3m`) and `lt1m` (members `1w1m`, `1d1w`, `24h`). Each point's `values`
  carries the merged `1m6m` and `lt1m` beside the eight bands, each computed
  from the bands' exact capacities. Every percentage of this route, the eight
  and the two merged, is cut at two decimals, never rounded
  (`format_ratio_cut`): two of three CKB is `"66.66"`. The eight `series` are
  named in the bands' words: `3 years or more`, `1 to 3 years`,
  `6 to 12 months`, `3 to 6 months`, `1 to 3 months`, `1 week to 1 month`,
  `1 day to 1 week`, `under 1 day` (they were `> 3y` … `24h`).
- `GET /charts/most-utilized-scripts` and `/charts/most-utilized-assets` — each
  of `usedShare` and `capacityShare` has `bands`: the five largest ranks, then
  `rest` (label `Others`, members every rank after the fifth and `others`) when
  there are more than five. Each point's `values.rest` (shannons, present with
  that band) is the day's total less the five drawn; `values.drawnTotal`
  (shannons) is the ranks drawn as their own bands summed, the five largest or
  every named rank where there are fewer (what "the five largest hold" states;
  `values.others` is then the rest); `values.total` (shannons) is the day's
  total, every rank and `others`, the whole the page's shares divide by. The
  series come in the route's order, largest first, a tie broken by the other
  metric and then the key; the page keeps it.
- `GET /charts/common-knowledge-composition` — each point's
  `values.knowledgeSize` (CKB): the day's knowledge size, which the five kinds
  partition.
- `GET /charts/total-supply` — five bands that make C, each point's
  `values.totalIssuance` (CKB), the total issuance in the day's DAO field:
  `outsideDao` ("Outside the Nervos DAO": the protocol circulation less every
  live DAO cell, `circulating − protocol_deposited`), `daoDeposited`
  ("Deposited in the Nervos DAO": the status-0 deposits, the amount the DAO page
  calls deposited), `daoWithdrawing` ("Withdrawing from the Nervos DAO": the
  principal of the withdrawal requests, `protocol_deposited − total_deposited`),
  `unclaimedCompensation` (DAO compensation accrued and not yet claimed, inside
  S) and `burnt` (genesis burnt and the treasury part of S). They sum to C to
  the shannon and none is negative; the route checks both per row and fails
  with the date and every operand where either does not hold, and refuses a day
  whose snapshot has no `protocol_deposited` ("delete RocksDB and re-sync from
  genesis"). `values.circulating` is the protocol circulation,
  `C − genesis burnt − S` (`outsideDao + daoDeposited + daoWithdrawing`), the
  amount the site calls circulating everywhere; it is not a band. (Part A2's
  four bands keyed the first `circulatingNotDeposited`, the withdrawal requests
  in it; lane D1's keyed it `outsideDao` with the same meaning.)
  `daoDeposited + daoWithdrawing` is `protocol_deposited`, the capacity of
  every live DAO cell (deposits and withdrawal requests) at the day's end: the
  amount `/statistics/ckbytes-allocation`'s `daoCapacity` states at its block
  (the home page's "in all DAO cells"; the indexer sums the same live DAO
  entries' capacity for both), the two figures one amount at two moments
  (part B5).
- `GET /charts/secondary-issuance` — one whole, each point's
  `protocolTotalShannons`: the secondary issuance since genesis,
  `miner + S + claimed`, in shannons. Its three destinations make it: `compensation` (DAO
  compensation accrued, claimed or not), `mining` and `burnt` (the treasury).
  The route checks the partition per day and fails with the date and the
  operands where it does not hold. `phase1CompensationShannons` is the frozen
  phase-1 compensation, a part of the unclaimed compensation, for developers.
  (`componentsTotalShannons` is gone: by the DAO partition it was the same
  figure.)
- `GET /charts/cell-size-distribution` — `liveCells` (the cells of every
  bucket) and `occupiedCapacity` (the CKB they occupy), counted from the same
  snapshot as the buckets.
- `GET /charts/epoch-time-distribution` — a bucket is one minute, its `date`
  the minute in hours at two decimals, cut (181 minutes is `"3.01"`); no two
  minutes share a label. `GET /charts/block-time-distribution` — each bucket's
  `date` is its exact 0.1 s edge and its `value`, the share of blocks, is cut
  at three decimals. A share is of every interval of the window: `intervals`
  counts them all, `beyondRange` the ones more than 50 s after their parent,
  counted in `intervals` and drawn in no bucket, and `epochs` is the complete
  epochs the window holds (42, or fewer on a younger chain).
- `GET /charts/epoch-time-length` — each epoch's `value` is its duration in
  whole ms (an epoch of 14,418,000 ms read `"4.01"` h when it was hours rounded
  at two decimals); `target.hours` is cut. `GET /charts/average-block-time` —
  each day's `value` is the day's average in whole ms, as the store keeps it
  (`DailyStats::avg_block_time_ms`). The page cuts both for display and counts
  "over the target" in integers.
- `GET /charts/uncle-rate` (a fraction, six decimals), `/charts/capacity-turnover-ratio`
  (a percentage, six decimals), `/charts/address-cohort-retention` (a
  percentage, six decimals) and `/dao/charts/circulation-ratio` (a percentage,
  four decimals) cut their shares at their precision, never round. An API older
  than this cut sent the same shapes rounded half up, so a page cannot tell one
  from the other and draws it (part B8: accepted, the frontend and the API ship
  together).
- `GET /stats/daily-activities` — each day's `kindOccurrences`: the eight kinds
  the charts read (transfers, the three DAO actions, tokens, objects,
  identities, script calls) summed. Kinds overlap, an action counting in each
  kind it is, so this is not a count of distinct activities; coinbase and
  unknown transactions are not in it. `daoCount` is the day's three DAO
  actions (deposits, withdraw requests, completed withdraws) summed, the DAO
  band of Activity by type. `userTransactionCount` is the day's user
  transactions: its committed transactions (its `DailyStats` row) less its
  cellbases (`coinbaseCount`), the figure the Transactions page's lede and
  "all" chip, the Activities page's lede and the home's daily bars state; a
  day without its chain row, or with more cellbases than transactions, is a
  500 naming the date.
- `GET /stats/activity-summary-24h` — the rolling window's `kindOccurrences`
  and `daoCount`, counted as the daily rows count them (one helper each in the
  route); the home page's activity shares divide by `kindOccurrences` and its
  DAO share is `daoCount`. `scriptCounts` is ranked by calls, ties by code hash;
  `scriptCalls` is every call it counts (a transaction calling two scripts
  counts twice, so it is not `scriptCallCount`), and `scriptRest`
  (`{ scripts, calls } | null`) the scripts after the five largest: the home
  card's script shares and its "others" row, summed nowhere in the page.

**What a page states outside the chart pages (lane exactness, part A)**

A count, a rank or a whole that a page states is the route's figure, read in
the same view as its rows; the page sums nothing. Read-only on the stores (the
network store too), no re-sync:

- `GET /charts/miner-address-distribution` — `restBlocks`: the window's blocks
  found by every miner after the five largest, the ranking card's "Others".
- `GET /charts/uncle-rate` — `counts: [{ date, uncles, blocks }]`, one per
  point of `data` in its order: each day's exact operands, so "over the target"
  is counted as uncles × `orphanRateTarget.denominator` > blocks ×
  `orphanRateTarget.numerator`; `value` stays the share, cut.
- `GET /tokens/{type_hash}/holders`, `/assets/objects/{collection_id}/holders`,
  `/assets/identities/{collection_id}/holders` and
  `/spore/clusters/{cluster_id}/holders` — `firstRank`: the 1-based rank of the
  page's first row in the request's read view (a token's: the holders that
  sort before the cursor in `CF_TOKEN_HOLDERS_BY_BALANCE`, counted, plus one; a
  collection's: the rows before the cursor's place in the ranking plus one, the
  ranking cached by the view's tip block). A cursor is a place in the order
  (count or balance descending, lock ascending), not a row: a page starts at the
  first row after it, whether or not its holder still stands there (a holder
  that moved or left no longer makes the next page a 400). The holders pages
  rank each row by it (the page's `firstRank` plus the row's place) and count
  the pager's rows from it (part B5); they refuse a response without it, as an
  API older than the page.
- `GET /transactions/pool` — `totals.waiting`: the transactions waiting in the
  pool, pending and proposed (not `awaitingIndex`). The pool tab states its
  pending, proposed and awaiting counts apart and sums none of them, so no page
  reads `totals.waiting` (part B5).
- `GET /forks/overview` — `canonicalBlocksInWindow`: the window's canonical
  blocks, what its uncle rate (`unclesInWindow` of them) divides by.
- `GET /scripts` — `roleCounts: { all, locks, types, both, neither }`: the
  script families by role; `locks` and `types` each count a family used as both
  (`both`), `neither` the families with no role, so the four make the whole
  (`locks + types - both + neither = all`, checked in the route; the list's
  first sentence says each).
- `GET /dao/statistics` — `depositCellsCount` (deposits),
  `withdrawalRequestCellsCount` (withdrawal requests) and `liveCellsCount`
  (their total, the live DAO cells); `secondaryIssuance` (shannons): the
  miners' share, the depositors' compensation and the burnt share, which make
  it. The legacy `activeDeposits`, the same live-cell total under an untrue
  name (a withdrawal request is no active deposit), is no longer sent (part B5);
  the route still refuses a stored row whose legacy count is not the deposits and
  the requests. The home page's "Active deposits" reads `depositCellsCount`.
- `GET /charts/block-time-distribution` — `bucketCounts`: each bucket's
  intervals, one per point of `data`, so a tie or the most common interval is
  read from counts, not from the cut shares.
- `GET /tokens/{type_hash}/charts/capacity-history` — each point states
  `capacity` beside `occupiedCapacity` and `freeCapacity`, as the other "CKB held"
  histories do.
- `GET /network/map` — a located node whose GeoIP record has no country is
  `Unknown`, as the distributions name it (it was `""`).
- Cut at the places the pages show, never rounded half up (part A2, S5), the
  shapes unchanged: `/inventory`'s `hMultiplier` (two places),
  `/statistics/network`'s `avgBlockTime` (two) and `tps`,
  `transactionsPerMinute`, `transactionsPerDay` (two, one, none),
  `/statistics/asset-ecosystem`'s category `percentage` (two), `/dao/calculator`'s
  realized APC (two) and `/dao/evidence`'s percentages (two and four). The
  inventory writes `hMultiplier` as it is, its whole part grouped, and refuses a
  value not at two places (part B5: it cut the route's figure a second time).
- `/statistics/network`'s `hashRate` and `difficulty` (part B, the third read's
  S3): in the unit the exact value reaches (no lowered threshold), cut at two
  places; the difficulty chart's day is the day's average cut to the integer.
  `estimatedEpochTime` stays the nearest second of an estimate (the remaining
  blocks at the window's average interval), which the page writes after "≈".
- Fee rates (part B4): every rate the routes serve at two places is cut there,
  never rounded half up — `/mempool/transactions`' `feeRate`,
  `/mempool/blocks`' `feeRateRange` and `medianFeeRate`, the pending
  proposals' `feeRate`, `/blocks/{id}/fee-stats`' `avgFeeRate`, `minFeeRate`
  and `maxFeeRate` (one function, `utils::exact_fee`); the whole-number rates
  (`tx_fee_rate`, shannons per KB) were already floored.
- `GET …/cell-versions` (spore objects, object items, identity items) — each
  version's `freeCapacity`, `capacity − occupiedCapacity`, stated so a page
  subtracts nothing (part B5). The token history's days, the "CKB held"
  histories and the cell versions take their free part from one helper
  (`utils::assets::free_part`), which refuses an occupied part under zero or
  above the whole.
- Activities' Nervos DAO actions (part B6) — each `protocolActions[]` entry of
  protocol `dao` carries, written by the indexer with the action (the route
  passes the metadata through; its type, free JSON, guarantees no key):
  `capacity` (shannons, an exact decimal string) and `lockHash`, the lock hash
  (hex, no `0x`) of the cell that took the step (a deposit's output deposit
  cell, a withdrawal request's output request cell, a completed withdrawal's
  input request cell); a `withdraw_request` also `depositBlock`, a
  `withdraw_complete` also `compensation` (shannons). A page says whose step
  each is from `lockHash`, matching a participant's `lockHash`, and refuses an
  action whose owner is missing, malformed or no participant.
- `GET /network/distributions` — `hosting` (`organisations`, `rest`,
  `restOrganisations`): the verified nodes by hosting organisation (an
  organisation's several ASNs as one, `Unknown` without an ASN), the five
  largest by nodes (ties by name) and the rest's nodes and organisations; they
  sum to `verifiedRetained`, checked. `releases: [{ release, count, builds }]`:
  the client versions by release (the label before its build), each release's
  nodes and builds, largest first.

---

### graph (crates/api/src/routes/graph.rs)

| Method | Path                                          | Handler              | Purpose                                                                  |
| ------ | --------------------------------------------- | -------------------- | ------------------------------------------------------------------------ |
| GET    | `/api/v1/graph/cell/{tx_hash}/{output_index}` | `get_cell_graph`     | Build cell-centric graph (creating tx, inputs at depth>1, consumer)      |
| GET    | `/api/v1/graph/transaction/{hash}`            | `get_tx_graph`       | Build tx-centric graph (inputs, outputs with live/dead status)           |
| GET    | `/api/v1/graph/proposals/{block_number}`      | `get_proposal_graph` | Build proposal-commitment graph for a block (window w_close=2, w_far=10) |

**Params**

- `GraphParams` — `depth` (default 2, clamped 1-5; cell graph only)
- `Path<(String, i32)>` for cell, `Path<String>` for tx hash, `Path<i64>` for block_number

**Responses**

- `GraphResponse` — `nodes: Vec<GraphNode>`, `links: Vec<GraphLink>` (each with id, node_type/link_type, JSON `data`)
- `ProposalGraphResponse` — same + `metadata: ProposalGraphMetadata` (source_block, total_proposals, committed_count, commitment_window)

---

### hardforks (crates/api/src/routes/hardforks.rs)

| Method | Path                | Handler          | Purpose                                                 |
| ------ | ------------------- | ---------------- | ------------------------------------------------------- |
| GET    | `/api/v1/hardforks` | `list_hardforks` | List CKB hardforks for a network with activation status |

**Params**

- `HardforksQuery` — `network` (defaults to `state.ckb_network`)

**Responses**

- `HardforkTimelineResponse` — `network`, `tip_epoch`, `tip_block`, `events: Vec<HardforkEventResponse>` (with `resources: Vec<HardforkResourceResponse>`)

---

### search (crates/api/src/routes/search.rs)

| Method | Path             | Handler  | Purpose                                                                                                                                                                                                                  |
| ------ | ---------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| GET    | `/api/v1/search` | `search` | Universal search across blocks/tx/address/cell/script/token/spore/cluster/asset (supports `scope:term` prefixes). A `.cell` name (`alice` or `alice.cell`) hashes straight to its id and returns an exact `identity` hit |

**Params**

- `SearchParams` — `q: String`

**Responses**

- `SearchResponse` — `results: Vec<SearchResult>`, `query`, `normalized_query`, `ambiguous`

---

### tokens (crates/api/src/routes/tokens.rs)

| Method | Path                                                 | Handler                        | Purpose                                                                          |
| ------ | ---------------------------------------------------- | ------------------------------ | -------------------------------------------------------------------------------- |
| GET    | `/api/v1/tokens`                                     | `list_tokens`                  | List tokens from warmup cache (search/standard/cursor)                           |
| GET    | `/api/v1/tokens/{type_hash}`                         | `get_token`                    | Get token detail + holders/transfers/24h aggregates + owned capacity             |
| GET    | `/api/v1/tokens/{type_hash}/holders`                 | `get_token_holders`            | List ranked token holders (cursor)                                               |
| GET    | `/api/v1/tokens/{type_hash}/transfers`               | `get_token_transfers`          | List token transfers (cursor)                                                    |
| GET    | `/api/v1/tokens/{type_hash}/activities`              | `get_token_activities`         | List token activities (cursor)                                                   |
| GET    | `/api/v1/tokens/{type_hash}/charts/capacity-history` | `get_token_capacity_chart`     | Stacked-area capacity history chart (date range)                                 |
| GET    | `/api/v1/tokens/{type_hash}/history`                 | `token_history::history`       | Complete-day supply/capacity, complete-week transfer counts, every supply change |
| GET    | `/api/v1/tokens/{type_hash}/distribution`            | `token_history::distribution`  | Whole positive-holder distribution and top-one/next-nine/rest amounts            |
| GET    | `/api/v1/tokens/{type_hash}/activity-feed`           | `token_history::activity_feed` | Full cross-user activities, one row per canonical transaction                    |
| GET    | `/api/v1/tokens/{type_hash}/transactions`            | `token_history::transactions`  | The same deduplicated scope as canonical transaction rows                        |

**Params**

- `ListParams` — `limit`, `standard`, `cursor`, `search`
- `HolderParams` — `limit`, `cursor`
- `TransferParams` — `limit`, `cursor`
- `ActivityParams` — `limit`, `cursor`
- `ChartRangeParams` — `from`, `to`
- `Path(type_hash): Path<String>`

**Responses**

- `CursorPaginatedResponse<TokenResponse>` / `TokenResponse` — token info + standard, supply, holders/transfers
- `TokenHoldersResponse` — flat cursor page plus `totalSupply`, nullable `anchor` and `firstRank`
- `CursorPaginatedResponse<TokenTransferResponse>` — tx_hash, from/to, amount, is_mint/is_burn
- `CursorPaginatedResponse<TokenActivityResponse>` — tx + actions + `transfers: Vec<TokenTransferDetail>`
- `StackedAreaChartResponse` — occupied and free capacity series (`occupiedCapacity`, `freeCapacity`)

**D4 exact read contracts**

- Detail and list `cellsCount` is the exact physical live type-index count, including zero-amount cells. Detail includes `sameSymbolTokens: [{typeScriptHash, holdersCount}]`; matching a symbol is never identity. Detail `issuer: {lockHash, address, lockKind} | null` is emitted only when standard sUDT/plain xUDT arguments prove owner-lock authority. Arbitrary compatible scripts and xUDT owner-mode/extension flags do not prove this. `maximumSupplyStatus` remains `unknown` for unproven constraints. Stored metadata provenance is not recorded and must not be inferred from equality with a bundled label.
- Holder pages retain `data/total/limit/hasMore/nextCursor`, adding `totalSupply` and `anchor: {blockNumber,blockHash} | null` from the same request view. Each holder adds nullable `lockKind`; percentages use this page's denominator, never another request's cached total.
- History returns `source: "indexed_chain_time"`, `completeThrough` (last complete UTC+8 date), canonical `anchor`, `daily: [{date,supply,capacity,occupiedCapacity,freeCapacity}]` (`freeCapacity` is `capacity - occupiedCapacity`, stated so a page subtracts nothing; a day whose occupied part exceeds its capacity is refused), `weekly: [{weekStart,transfersCount}]`, `supplyEvents`, `mintCount`, `burnCount`, and `transfersCount`. Supply/amount/capacity are exact decimal strings; capacity units are shannons. Daily rows include quiet dates with unchanged state. Weeks start Monday and only finished weeks are returned. Counters count indexed transfer records (including mint/burn legs); they are not unique-transaction counts. All supply events through the anchor are returned, oldest first, with `txHash, blockNumber, txIndex, transferSequence, timestamp` (milliseconds), `kind: mint|burn, amount, supplyAfter, lockHash, address, lockKind`. The lock is the mint recipient or burn sender. The final supply must equal current holder balances; inconsistent or missing evidence fails contextually.
- Distribution returns `holdersCount,totalSupply,largestHolderBalance,nextNineBalance,remainingBalance,decimals,unit,buckets`. Bucket boundaries are raw integer strings, lower inclusive/upper exclusive; the last upper bound is null. Known decimals produce token-unit ranges below 1, 1–10, 10–100, 100–1,000, 1,000–10,000, 10,000–100,000, and at least 100,000. Unknown decimals explicitly use raw units. Counts and balances cover every positive holder, including values above u128.
- New feeds accept `limit` (1–100) and `cursor=block:realTxIndex`, return exact deduplicated `total`, and share the existing `GlobalActivityResponse` / `TransactionResponse` projections. Every participant remains present. Token amount moved can be read as the sum of positive deltas for that token in the complete activity; transaction `value` continues to mean CKB moved. Old `/activities` remains the transfer-pair grouping API with its independent transfer-sequence cursor.
- Read complexity: history and feeds scan that token's transfer prefix, distribution scans its holder prefix, live count scans its physical type-cell prefix. History joins relevant headers only and trusts their indexed chain time; it never scans the full chain's headers or substitutes RPC. No new persistent writes.

Owner authority follows [RFC 0025](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md) and [RFC 0052](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0052-extensible-udt/0052-extensible-udt.md), rather than arbitrary compatible-script argument guesses.

---

### dao (crates/api/src/routes/dao.rs)

| Method | Path                                   | Handler                       | Purpose                                                                                       |
| ------ | -------------------------------------- | ----------------------------- | --------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/dao/deposits`                 | `list_deposits`               | List all DAO deposits (cursor, optional `status` 0/1/2)                                       |
| GET    | `/api/v1/dao/deposits/{lock_hash}`     | `get_deposits_by_address`     | List DAO deposits for a lock_hash (cursor)                                                    |
| GET    | `/api/v1/dao/summary/{lock_hash}`      | `get_address_dao_summary`     | Per-address DAO summary (locked, unclaimed compensation, APC)                                 |
| GET    | `/api/v1/dao/statistics`               | `get_statistics`              | Global DAO statistics (totals, depositors, compensation, 24h deltas)                          |
| GET    | `/api/v1/dao/top-depositors`           | `get_top_depositors`          | Top DAO depositors leaderboard                                                                |
| GET    | `/api/v1/dao/calculator`               | `calculate_compensation`      | Compute expected DAO compensation for `capacity` between `deposit_block` and `withdraw_block` |
| GET    | `/api/v1/dao/charts/total-deposit`     | `get_total_deposit_chart`     | Cumulative total deposit chart with depositors series                                         |
| GET    | `/api/v1/dao/charts/daily-deposit`     | `get_daily_deposit_chart`     | Daily deposit chart                                                                           |
| GET    | `/api/v1/dao/charts/daily-depositors`  | `get_daily_depositors_chart`  | Daily depositor-address count chart                                                           |
| GET    | `/api/v1/dao/charts/circulation-ratio` | `get_circulation_ratio_chart` | DAO-deposit/circulating-supply ratio chart                                                    |

**Params**

- `ListParams` — `limit`, `status` (i16), `cursor`
- `CalculatorParams` — `capacity` (String), `deposit_block` (i64), `withdraw_block` (Option<i64>)
- `Path(lock_hash): Path<String>`

**Responses**

- `CursorPaginatedResponse<DaoDepositResponse>`
- `AddressDaoSummaryResponse` — counts + locked/compensation totals + APC
- `DaoStatisticsResponse` — global totals + 24h deltas + treasury breakdown
- `DaoTopDepositorsResponse` — `depositors: Vec<DaoTopDepositorResponse>`
- `CalculatorResponse` — capacity, estimated compensation, APC string
- `ChartResponse` — chart data points

---

### fiber (crates/api/src/routes/fiber.rs)

| Method | Path                                      | Handler                | Purpose                                                        |
| ------ | ----------------------------------------- | ---------------------- | -------------------------------------------------------------- |
| GET    | `/api/v1/fiber/channels`                  | `list_channels`        | List Fiber channels (cursor, optional `state` filter)          |
| GET    | `/api/v1/fiber/channels/{channel_id}`     | `get_channel`          | Get a Fiber channel + lifecycle timeline                       |
| GET    | `/api/v1/addresses/{addr}/fiber/channels` | `get_address_channels` | List Fiber channels for a CKB address                          |
| GET    | `/api/v1/fiber/stats`                     | `get_stats`            | Total/open channel counts and locked capacity (iterates store) |

**Params**

- `ListChannelsParams` — `limit`, `cursor`, `state` (`open` / `closed` / `cooperativelyClosed` / `force_closed` / `forceClosed` / `settled`)
- `AddressChannelsParams` — `limit`
- `Path<String>` for `channel_id` / `addr`

**Responses**

- `CursorPaginatedResponse<FiberChannelResponse>` — channel_id, state, capacity, UDT info, participants, funding/closing tx info
- `FiberChannelDetailResponse` — channel + `timeline: Vec<FiberTimelineEvent>`
- `Vec<FiberChannelResponse>` for address listing
- `FiberStatsResponse` — totals

**Field semantics**

- `capacity` — the funding cell's CKB capacity in shannons, for every channel. `totalCapacityLocked`
  in `FiberStatsResponse` sums this over open channels and is therefore always a CKB figure, never
  a UDT amount.
- `udtTypeHash` / `udtAmount` — set together when the funding cell carries a type script: the type
  script hash identifying the token, and the token amount held by the funding cell (decimal string,
  `u128`). Both are `null` for plain-CKB channels. A UDT-funded channel still reports its CKB
  `capacity` alongside these.
- `participants` — the funding transaction's non-fiber owners. The funding and commitment locks
  that implement the channel are never listed as participants.
- `delayEpoch` / `commitmentOutputIndex` — the revocation delay encoded in the commitment lock args
  and the index of the commitment output, both taken from the commitment cell of the most recent
  force-close or commitment-revocation event. `null` while a channel is open or cooperatively
  closed (no commitment cell exists).

---

### spore (crates/api/src/routes/spore.rs)

| Method | Path                                                          | Handler                      | Purpose                                                                                        |
| ------ | ------------------------------------------------------------- | ---------------------------- | ---------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/spore/clusters`                                      | `list_clusters`              | List Spore clusters from warmup cache (cursor `block:0x<id>`)                                  |
| GET    | `/api/v1/spore/clusters/{cluster_id}`                         | `get_cluster`                | Get cluster detail (supports `sole-spores` alias)                                              |
| GET    | `/api/v1/spore/clusters/{cluster_id}/charts/capacity-history` | `get_cluster_capacity_chart` | Cluster capacity history (stacked area, date range)                                            |
| GET    | `/api/v1/spore/clusters/{cluster_id}/holders`                 | `get_cluster_holders`        | List ranked cluster holders (cursor)                                                           |
| GET    | `/api/v1/spore/clusters/{cluster_id}/activities`              | `get_cluster_activities`     | List cluster activities (cursor, optional `action` mint/transfer/burn)                         |
| GET    | `/api/v1/spore/clusters/{cluster_id}/spores`                  | `get_spores_by_cluster`      | List spores in a cluster (cursor `block:0x<id>`, from in-memory cache)                         |
| GET    | `/api/v1/spore/objects`                                       | `list_spores`                | List all live spores (cursor `block:0x<id>`, from in-memory cache)                             |
| GET    | `/api/v1/spore/objects/{spore_id}`                            | `get_spore`                  | Get spore detail (with cumulative owned capacity from daily deltas)                            |
| GET    | `/api/v1/spore/objects/{spore_id}/activities`                 | `list_spore_item_activities` | Per-spore activities (cursor, optional `action`)                                               |
| GET    | `/api/v1/spore/objects/{spore_id}/decode`                     | `decode_spore`               | DOB-decoded traits + media (with on-the-fly SVG render URL when applicable)                    |
| GET    | `/api/v1/spore/objects/{spore_id}/media/{hash}`               | `serve_media`                | **Binary** content-addressed media blob (raw bytes, CSP/no-sniff headers)                      |
| GET    | `/api/v1/spore/objects/{spore_id}/render`                     | `render_spore_svg`           | **Raw SVG** rendered on-the-fly from decoded traits or cluster DOB1 patterns (`image/svg+xml`) |
| GET    | `/api/v1/spore/objects/{spore_id}/charts/capacity-history`    | `get_spore_capacity_chart`   | Single-spore capacity history (stacked area, date range)                                       |
| GET    | `/api/v1/spore/owner/{lock_hash}`                             | `get_spores_by_owner`        | List spores owned by a lock_hash (cursor `block:0x<id>`)                                       |

**Params**

- `ListParams` — `limit`, `cursor` (i64 block-number cursor)
- `ChartRangeParams` — `from`, `to`
- `ClusterHoldersParams` — `limit`, `cursor`
- `ClusterActivitiesParams` — `limit`, `cursor`, `action`
- `MnftItemActivitiesParams` (from assets.rs) — per-spore item activities
- `Path<String>` for `cluster_id` / `spore_id` / `lock_hash`; `Path<(String, String)>` for `serve_media` `(spore_id, hash)`

**Responses**

- `CursorPaginatedResponse<ClusterResponse>` / `ClusterResponse` — cluster_id, name, counts, composition (`ClusterCompositionResponse`)
- `CursorPaginatedResponse<SporeResponse>` / `SporeResponse` — spore_id, content_type/size, owner, owned capacity, `media_profile`
- `RankedPage<ClusterHolderResponse>` (a cursor page with `firstRank`) / `CursorPaginatedResponse<ClusterActivityResponse>`
- `CursorPaginatedResponse<MnftItemActivityResponse>` for per-spore activities
- `SporeDobDecodeResponse` — `status` (`decoded` \| `failed` \| `pending`), traits, `media: Vec<DecodedMediaResponse>`, `issues` (carries the failure reason when `failed`)
- `axum::Response` for `serve_media` (any media MIME) and `render_spore_svg` (`image/svg+xml`) — not JSON
- `StackedAreaChartResponse` for capacity history charts. Each day's `values` carry `occupiedCapacity`, `freeCapacity` and `capacity`, the whole its cells hold, all three stated by the route (`held_point` in `asset_calendar.rs`, the free part by the one helper `utils::assets::free_part`, which refuses an occupied part under zero or above the whole, the day failing with its date and its operands), so a page sums nothing; the series are `occupiedCapacity` ("Occupied") and `freeCapacity` ("Free"), from one helper (`held_series`). They were `used` and `unused` (part B, the third read's S4: one name for a held point's parts on every route, as the token history and the cell versions name them).

---

### mempool (crates/api/src/routes/mempool.rs)

| Method | Path                                | Handler                    | Purpose                                                                        |
| ------ | ----------------------------------- | -------------------------- | ------------------------------------------------------------------------------ |
| GET    | `/api/v1/mempool/info`              | `get_mempool_info`         | Mempool size/cycles/min-fee-rate (proxies CKB `tx_pool_info`)                  |
| GET    | `/api/v1/mempool/transactions`      | `get_mempool_transactions` | List up to 500 mempool txs sorted by fee_rate desc (proxies `get_raw_tx_pool`) |
| GET    | `/api/v1/mempool/blocks`            | `get_mempool_blocks`       | Predicted pending blocks packed from mempool by fee_rate (size/cycles limits)  |
| GET    | `/api/v1/mempool/pending-proposals` | `get_pending_proposals`    | List unexpired pending proposals tracked locally                               |

**Params**

- All handlers take only `State<Arc<AppState>>` (no query/path params)

**Responses**

- `MempoolInfo` — pending/proposed/orphan counts, totals, tip info
- `MempoolTransactionsResponse` — `transactions: Vec<MempoolTransaction>`, `total`
- `MempoolBlocksResponse` — `pending_blocks: Vec<MempoolBlock>` + totals (with `fee_rate_range: FeeRateRange`)
- `ckbadger_common::PendingProposalsResponse` — `proposals: Vec<PendingProposal>`, `tip_block_number`, `total_count`

---

### network (crates/api/src/routes/network.rs)

These endpoints read the optional, non-chain network store written solely by
`ckbadger-crawler`; all handlers remain secondary/read-only. Participation evidence,
configured-node session direction, and crawler dial results are orthogonal. The API does not
claim global network completeness or infer NAT, firewall, “home node”, offline, or other remote
causes.

| Method | Path                              | Handler         | Purpose                                                              |
| ------ | --------------------------------- | --------------- | -------------------------------------------------------------------- |
| GET    | `/api/v1/network/summary`         | `summary`       | Checked completed evidence projections plus separate active progress |
| GET    | `/api/v1/network/distributions`   | `distributions` | Histograms scoped explicitly to retained verified peer records       |
| GET    | `/api/v1/network/history`         | `history`       | Verified/reachable counts and distribution shares by hour/day        |
| GET    | `/api/v1/network/peers`           | `peers`         | Unified observed-peer list with optional crawler-verified metadata   |
| GET    | `/api/v1/network/peers/{peer_id}` | `peer_by_id`    | Advertisements, sessions, dial probes, and verification evidence     |

**Params**

- `HistoryQuery` — required `metric` (`verifiedPeers`, `reachablePeers`, `versionShare`,
  `countryShare`) and `granularity` (`hour`, `day`), plus optional inclusive Unix-second `from`
  and `to`
- `PeersQuery` — optional `cursor`, `limit` (default 50, valid range 1–500), exact `state`
  (`reachable`, `verifiedUnavailable`, `advertisedUnverified`, `foreignNetwork`, or
  `noCompletedObservation`), typed address `observation` (`dialRequestFailed`,
  `noAuthenticatedSessionBeforeDeadline`,
  `authenticatedSessionWithoutIdentifyBeforeDeadline`, `malformedIdentify`, `foreignNetwork`, or
  `sameNetworkIdentified`), `country`, and exact `version`

`state` filters `crawlerDialState` only. It does not filter or reinterpret the independent
`participation` or `sessionInitiators` facts.

Unknown `state` or `observation` values return `400` before any network-store read. History bucket
timestamps use checked multiplication; an unrepresentable persisted bucket is an explicit `500`
store invariant failure rather than a wrapped timestamp.

- `Path(peer_id): Path<String>` — raw peer ID encoded as hexadecimal

**Responses**

- `NetworkSummary` — `enabled`, `hasData`, optional completed `lastRound`, and optional separate
  `activeRound`. The completed summary includes nullable `localObserver` longitudinal evidence
  (`peerId`, first/latest observation time and round, count, latest client/active/address/protocol/
  connection metadata) and exact current-round `directSessionObservations` split into
  `observerInitiated` and `peerInitiated`. `lastRound` also exposes checked `candidatePeers`,
  `verifiedRetainedPeers`, `reachablePeers`, `verifiedUnavailablePeers`, `exhaustedCandidates`,
  `foreignPeers`, `addressAttempts`, and `nonSuccessfulAddressAttempts`, plus the persisted peer outcome matrix,
  address-result histogram, Discovery counters, malformed-address count, and newly verified count.
  `activeRound` reports exact checkpoint progress without replacing completed evidence.
- `NetworkDistributions` — `verifiedRetained`, `sameNetworkReachable`,
  `verifiedUnavailable`, and sorted version/country/ASN/protocol buckets over verified records only
- `NetworkHistory` — requested metric/granularity and ascending points; daily data excludes the
  incomplete current day
- `NetworkPeersPage` — unified observed-peer `items` and optional `nextCursor`, sorted by
  `latestPositiveObservedAt` descending. That timestamp is the checked maximum of retained alias
  advertisement time, target-centric advertisement evidence, direct-session evidence, and
  crawler-identified `lastSeen`. Each item keeps `crawlerDialState` separate from the three exact
  `participation` booleans (`discoveryAdvertised`, `directSessionObserved`,
  `crawlerIdentified`) and the distinct `sessionInitiators` values. It also exposes nullable
  `primaryAddr`, `lastAdvertisedAt`, and `lastDialObservedAt`; a direct-only peer does not fabricate
  those fields from its session connection. Candidate-only version/country/ASN/RTT fields are
  `null` as well.
- `PeerDetailResponse` —
  `observationVantage="configuredLocalCkbRpcObserverAndThisCrawler"`, the same orthogonal
  `crawlerDialState`/`participation`/`sessionInitiators`, nullable `firstDiscoveredAt` and
  `lastAdvertisedAt`, exact `latestPositiveObservedAt`, retained aliases (including nullable
  per-alias `lastVerifiedAt`), immutable `lastCompleted` typed dial evidence, optional separately
  labeled `active` evidence, and optional retained `verified` crawler record.
- `PeerDetailResponse.directSessions[]` — target-centric positive session evidence with
  `observerPeerId`, `initiator`, first/latest observation time and round, exact
  `observationCount`, latest `clientVersion`, `sessionAddresses`, `connectedDurationMs`, nullable
  `lastPingDurationMs`, and `{id, version}` protocol rows. `sessionAddresses` are never dial
  aliases.
- `PeerDetailResponse.advertisers[]` — durable target-centric advertiser references. Each entry
  exposes `advertiserPeerId`, `alias`, `firstObservedAt`, `lastObservedAt`,
  `firstObservedRound`, `lastObservedRound`, and exact `observationCount`; it is not reconstructed
  by inverting verified peers' latest `knownPeers`.

`DiscoveryEvidenceResponse` exposes `validNodesMessages`, `validResponseMessages`,
`validAnnounceMessages`, `malformedMessages`, `unexpectedMessages`,
`normalizedAdvertisedAddresses`, and `rejectedAdvertisedAddresses`. Checked persisted evidence
requires response plus announce counts to equal the valid-Nodes total. Valid response and announce
addresses are a positive union, not last-message-wins state.

The old `/network/nodes` routes and ambiguous fields such as `totalKnown` and
`unreachablePeers` are intentionally removed. `advertisedUnverified` means all retained dial
aliases were exhausted without a retained same-network crawler verification; it is not a global
liveness claim and does not negate direct-session participation evidence. Session direction is
named from the configured local CKB observer's vantage: `observerInitiated` corresponds to RPC
`is_outbound=true`, while `peerInitiated` corresponds to `is_outbound=false`. RPC session
addresses are connection facts only and are never crawler dial aliases.

The crawler accepts local-node session observations only after `get_block_hash(0)` matches the
configured network's exact genesis hash. A mismatched RPC node is an error, not foreign-network
peer evidence.

When the crawler/store is disabled, summary and aggregate/list endpoints return an honest
disabled or empty state. A point lookup still returns not found.

---

### scripts (crates/api/src/routes/scripts.rs)

| Method | Path                                             | Handler                                          | Purpose                                                                                             |
| ------ | ------------------------------------------------ | ------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/scripts`                                | `list_scripts`                                   | List script families from warmup cache (search/sort/cursor)                                         |
| POST   | `/api/v1/scripts/lookup`                         | `lookup_scripts`                                 | Bulk-resolve up to 100 code_hashes (optional `tx_hash` per-tx resolution)                           |
| GET    | `/api/v1/scripts/code-cell`                      | `get_code_cell`                                  | Resolve a code-cell outpoint for a `code_hash`                                                      |
| GET    | `/api/v1/scripts/code-cells`                     | `get_code_cells`                                 | List all code cells (live+consumed) for a `code_hash`, with ambiguity info                          |
| GET    | `/api/v1/scripts/charts/capacity-history`        | `get_script_capacity_history_chart_by_code_hash` | Capacity history chart for a script identified by `code_hash` (range + `script_kind`)               |
| GET    | `/api/v1/scripts/{name}`                         | `get_script`                                     | Get script family detail with all versions                                                          |
| GET    | `/api/v1/scripts/{name}/usage`                   | `get_script_usage`                               | Aggregate cell/capacity usage for a script family, broken down by deployment                        |
| GET    | `/api/v1/scripts/{name}/charts/capacity-history` | `get_script_capacity_history_chart`              | Capacity history chart for a script by `{name}` (optional `code_hash` filter, range, `script_kind`) |

**Params**

- `ListParams` — `limit`, `cursor`, `network`, `decoder_type` (unused), `search`, `sort_key`, `sort_direction`
- `LookupScriptsRequest` (POST body via `Json<>`) — `code_hashes: Vec<String>`, optional `tx_hash`
- `CodeCellQuery` — `code_hash`, optional `hash_type` (`data` \| `type` \| `data1` \| `data2`). With
  `hash_type` the lookup resolves the exact observed reference form (a data-family form only
  resolves when a code cell with that data hash exists on chain); without it the whole deployment
  family is resolved. An unrecognized `hash_type` is a 400.
- `ScriptCapacityHistoryQuery` — `code_hash` (Option), `script_kind`, `from`, `to` (used with `{name}` path)
- `ScriptCapacityHistoryByCodeHashQuery` — required `code_hash`, plus `script_kind`/`from`/`to`
- `Path(name): Path<String>`

**Responses**

- `CursorPaginatedResponse<ScriptFamilyListItemResponse>` — script family listing (from `crate::response`)
- `HashMap<String, ScriptLookupInfo>` — keyed by reference hash; each entry has resolution_state (`resolved`/`ambiguous`) and optional `ambiguity: ScriptResolutionAmbiguityResponse`.
  Every deployment-scoped field describes **the queried reference**, never a sibling deployment
  that happens to share the same bytecode: `liveCellsCount`/`ownedCapacitySum`/`ownedKnowledgeSum`
  and `scriptKind` come from the queried reference's own usage rollup (zero when the reference has
  no indexed usage), and `codeCellTxHash`/`codeCellOutputIndex`/`codeCellsLiveCount`/`codeCellsTotal`
  cover only the code cells deployed under that reference. `codeHash` remains the resolved bytecode
  version hash, which two deployments of one binary legitimately share. `deprecated` is reported for
  ambiguous references too, from the reference's own record.
- `CodeCellResponse` — `tx_hash`, `output_index` (option of either)
- `CodeCellsResponse` — `code_cells: Vec<CodeCellEntry>`, counts, optional `resolved_version_hash`, optional ambiguity
- `ScriptFamilyDetailResponse` — family + `versions` (from `crate::response`)
- `ScriptUsageResponse` — totals + `by_deployment: Vec<DeploymentUsage>`
- `StackedAreaChartResponse` (from statistics) for capacity history charts. Each day's `values` carry `occupiedCapacity`, `freeCapacity` and `capacity`, the whole its cells hold, all three stated by the route (`held_point` in `asset_calendar.rs`, the free part by the one helper `utils::assets::free_part`, which refuses an occupied part under zero or above the whole, the day failing with its date and its operands), so a page sums nothing; the series are `occupiedCapacity` ("Occupied") and `freeCapacity` ("Free"), from one helper (`held_series`). They were `used` and `unused` (part B, the third read's S4: one name for a held point's parts on every route, as the token history and the cell versions name them).

---

### forks (crates/api/src/routes/forks.rs)

| Method | Path                   | Handler            | Purpose                                                                             |
| ------ | ---------------------- | ------------------ | ----------------------------------------------------------------------------------- |
| GET    | `/api/v1/forks`        | `list_forks`       | List reorg/deep-fork events (currently derived from sync status; returns at most 1) |
| GET    | `/api/v1/forks/recent` | `get_recent_reorg` | Combined recent reorg + deep-fork status                                            |
| GET    | `/api/v1/forks/{id}`   | `get_fork_detail`  | Reorg detail by id (only id=1 returns a deep-fork event; orphaned lists are empty)  |

**Params**

- `ListForksParams` — optional `limit`
- `Path(id): Path<i32>`

**Responses**

- `CursorPaginatedResponse<ReorgEventResponse>` — id, fork point, old/new tip, depth, event_type
- `ReorgDetailResponse` — `event` + `orphaned_blocks: Vec<OrphanedBlockResponse>` + `orphaned_transactions: Vec<OrphanedTransactionResponse>`
- `RecentReorgResponse` — `has_recent_reorg`, `reorg` (option), `deep_fork: DeepForkStatusResponse`

---

### verify (crates/api/src/routes/verify.rs)

| Method | Path                               | Handler                    | Purpose                                                               |
| ------ | ---------------------------------- | -------------------------- | --------------------------------------------------------------------- |
| POST   | `/api/v1/verify/entity-statistics` | `export_entity_statistics` | Bounded, read-only export of raw entity daily deltas for the verifier |

A verification-only endpoint, not part of the public explorer surface. Every
read happens inside the request's single read-view pin, so the anchor and the
rows it labels come from one view; it reads no warmup cache, calls no RPC, and
writes nothing.

**Params**

- `EntityStatisticsRequest` (JSON body) — `entities: [{kind, id}]` (≤ 16, `kind`
  must be `token` in this delivery), optional
  `expectedAnchor: {blockNumber, blockHash}`, optional `maxDailyRows`
  (≤ 8192, default 8192)

**Responses**

- `EntityStatisticsResponse`
  - `anchor` — `{blockNumber, blockHash}` of the store's sync tip
  - `state` — `{bulkSessionInProgress, liveCellSummaryInitialized, deepForkDetected, entityStatsUndoContract, hourlyRetention}`,
    all read under the same pin as the rows they qualify.
    - `entityStatsUndoContract` — `{version, coverageFloorBlock, updatedAtBlock}`,
      or `null` when the write path has published no contract.
      `coverageFloorBlock` is the lowest block the block journal can still roll
      back to; `updatedAtBlock` is the committed tip when that floor was last
      advanced, so a stale floor is distinguishable from a current one.
    - `hourlyRetention` — one answer **per hourly family**, keyed by family
      name: `{"token": …, "mnft": …}`. There is deliberately no cross-family
      verdict; one family being settled says nothing about the other. Each
      value is either `"unknown"` (the store holds no retention row for that
      family — absence of evidence, never a zero boundary) or
      `{authoritative, policyVersion, executedCutoffHour, roundInProgressCutoffHour, cursor, roundStartedAt, roundCompletedAt}`.
      `authoritative` is `true` only when the last recorded round ran to the
      end of that family (`cursor` cleared _and_ `roundCompletedAt` set);
      `executedCutoffHour` is a retention boundary a reader may trust only
      then, because an interrupted round has deleted just the keys before
      `cursor`. `roundInProgressCutoffHour` is diagnostic — never a boundary —
      and `cursor` is the hex-encoded resume key of an in-flight round.
      Values are published as stored: a family whose first round never finished
      reports the sentinel the store holds, with `authoritative: false` saying
      not to read it as an hour.
  - `complete` — false whenever a write-path phase is in flight or any entity
    could not be exported in full
  - `anchorMismatch` — present only when `expectedAnchor` did not match:
    `{expected, actual}`, with nothing exported
  - `entities[]` — `{kind, id, present, rowCount, typeScript, complete, currentCapacity, currentKnowledge, currentError, daily}`,
    where `typeScript` is `{codeHash, hashType, args}` read from the same pin
    (so the verifier can build a chain query without asking an endpoint that
    computes aggregates), and `daily[]` is
    `{date, capacityDelta, knowledgeDelta}` with every value an exact decimal
    string in shannons. `present`/`rowCount`/`current*` are `null` when the
    state withheld the numbers. `currentCapacity`/`currentKnowledge` accumulate
    _every_ stored row, not just the returned page; a token has no separately
    stored current value, so these are the same checked accumulation the public
    token endpoint performs. `currentError` carries why that accumulation
    failed (a corrupt row series), with the daily rows still exported so the
    offending day can be named.

**Errors**

- `400` — empty selection, more than 16 entities, `maxDailyRows` outside
  `1..=8192`, an unsupported `kind`, or a malformed id
- `503 initializing` — the indexer has not published a sync tip yet

An `expectedAnchor` the store has moved past is not an error: the chain moved,
so the response is `200` with `complete: false`, nothing exported, and
`anchorMismatch` carrying both anchors so a client can re-pin.

A request over a hard cap is refused rather than silently narrowed; a legal
request that cannot be exported in full returns `complete: false` instead of a
truncated list.

---

**Total endpoints: 135** across 19 modules. Confirm against
`crates/api/src/routes/*.rs` for any field-level question; this skeleton is
intentionally name-and-purpose only.
