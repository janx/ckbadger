# ckbadger HTTP API Reference (skeleton)

This is an auto-generated skeleton of every endpoint exposed by `ckbadger-api`,
derived from the `routes()` functions in `crates/api/src/routes/*.rs` and the
nesting in `crates/api/src/lib.rs:384` (`.nest("/api/v1", routes::api_routes())`).

**Source of truth.** The Rust code is authoritative. Each handler's `Query<T>`,
`Path<T>`, and `Json<T>` types — together with the `Serialize` response struct
— define the exact request and response shape. Look there for field-level
details that aren't reproduced here.

## Conventions

- **Base path.** All endpoints in this document are nested under `/api/v1`.
- **Path syntax.** Axum 0.8: path params use `{id}` form.
- **Casing.** Response structs use `#[serde(rename_all = "camelCase")]`. Query
  params decode from snake_case (`lock_script_hash`, `type_script_hash`, etc.).
- **Error envelope.** Handlers return `ApiResult<T>` —
  `ApiError::{not_found, bad_request, internal, warmup_pending}` map to
  appropriate HTTP statuses.
- **Pagination.** Listing endpoints return
  `CursorPaginatedResponse<T> = { data: Vec<T>, total, limit, nextCursor? }`.
  The cursor is opaque; pass the previous response's `nextCursor` back as
  `?cursor=`. Address-cell cursors encode `script_hash + block_num + tx_hash +
output_index`; activity cursors encode `block_num:tx_idx:seq`.
- **Address inputs.** Any `lock_script_hash` parameter accepts either a CKB
  address (auto-decoded) or a 0x-prefixed lock hash. `type_script_hash` accepts
  only the 0x-prefixed 32-byte hash.
- **Non-JSON responses.** A few spore endpoints return raw binary or SVG; they
  are flagged in the inventory below.
- **WebSocket.** Real-time subscriptions live under `crates/api/src/ws/` and
  are not part of this REST inventory.
- **Page-level routes.** `/llms.txt`, `/llms-full.txt`, `/capabilities`, and
  `.md`/`.raw` page suffixes are served by the frontend layer (see
  `frontend/public/llms.txt`), not by `/api/v1`.

## Modules

`api_routes()` (`crates/api/src/routes/mod.rs:25`) merges 17 modules.
`tx_lookup.rs` is a helper imported by `transactions.rs` and is not mounted.

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

---

### assets (crates/api/src/routes/assets.rs)

| Method | Path                                                             | Handler                                | Purpose                                                                               |
| ------ | ---------------------------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------- |
| GET    | `/api/v1/assets`                                                 | `list_assets`                          | List all assets (tokens + objects + identities), cache-backed with search/sort/cursor |
| GET    | `/api/v1/assets/objects/items/{object_id}`                       | `get_object_item_detail`               | Get mNFT token item detail (class, issuer, lifecycle, composition)                    |
| GET    | `/api/v1/assets/objects/items/{object_id}/activities`            | `list_mnft_item_activities`            | List per-item mNFT activities (cursor-paginated, optional `action`)                   |
| GET    | `/api/v1/assets/objects/{collection_id}`                         | `get_object_collection`                | Get object/Spore collection detail (counts, composition, owned capacity)              |
| GET    | `/api/v1/assets/objects/{collection_id}/items`                   | `list_object_collection_items`         | List items in an object collection (search/status/cursor)                             |
| GET    | `/api/v1/assets/objects/{collection_id}/holders`                 | `list_object_collection_holders`       | List ranked holders of an object collection (cursor)                                  |
| GET    | `/api/v1/assets/objects/{collection_id}/activities`              | `list_object_collection_activities`    | List collection-level activities (cursor, optional `action`)                          |
| GET    | `/api/v1/assets/objects/{collection_id}/charts/capacity-history` | `get_object_collection_capacity_chart` | Stacked-area capacity history chart (date range)                                      |

**Params**

- `ListParams` — `limit`, `type` (token|object|identity), `standard`, `cursor`, `search`, `sort_key`, `sort_direction`, `composition_tier`
- `ObjectItemsParams` — `limit`, `cursor`, `search`, `status`
- `MnftItemActivitiesParams` — `limit`, `cursor`, `action`
- `CollectionHoldersParams` — `limit`, `cursor`
- `CollectionActivitiesParams` — `limit`, `cursor`, `action`
- `ChartRangeParams` — `from`, `to`
- `Path<String>` for `object_id`, `collection_id`

**Responses**

- `CursorPaginatedResponse<AssetResponse>` — id, type, standard, name, holders/transfers counts, owned capacity
- `MnftItemDetailResponse` — nft_id, class, issuer, lifecycle, composition
- `CursorPaginatedResponse<MnftItemActivityResponse>` — per-item activity entries
- `NftCollectionDetailResponse` — counts, composition, class/issuer summary
- `CursorPaginatedResponse<CollectionItemResponse>` — items
- `CursorPaginatedResponse<CollectionHolderResponse>` — holders with item counts
- `CursorPaginatedResponse<CollectionActivityResponse>` — collection activities
- `StackedAreaChartResponse` — `data` + `series` (defined in `statistics.rs`)

---

### identities (crates/api/src/routes/identities.rs)

| Method | Path                                                              | Handler                               | Purpose                                                          |
| ------ | ----------------------------------------------------------------- | ------------------------------------- | ---------------------------------------------------------------- |
| GET    | `/api/v1/assets/identities/dotbit/items/{identity_id}`            | `get_dotbit_item_detail`              | Get .bit account item detail (expired_at, registered_at, status) |
| GET    | `/api/v1/assets/identities/dotbit/items/{identity_id}/activities` | `list_dotbit_item_activities`         | List .bit item activities (cursor, optional `action`)            |
| GET    | `/api/v1/assets/identities/did/items/{identity_id}`               | `get_did_ckb_item_detail`             | Get did:ckb item detail                                          |
| GET    | `/api/v1/assets/identities/did/items/{identity_id}/activities`    | `list_did_ckb_item_activities`        | List did:ckb item activities (cursor, optional `action`)         |
| GET    | `/api/v1/assets/identities/{collection_id}`                       | `get_identity_collection`             | Get identity collection aggregate (dotbit/did:ckb only)          |
| GET    | `/api/v1/assets/identities/{collection_id}/holders`               | `list_identity_collection_holders`    | List ranked holders of an identity collection                    |
| GET    | `/api/v1/assets/identities/{collection_id}/activities`            | `list_identity_collection_activities` | List identity collection activities (cursor, optional `action`)  |
| GET    | `/api/v1/assets/identities/{collection_id}/items`                 | `list_identity_collection_items`      | List identity items in a collection (search/status/cursor)       |

**Params**

- `Path<String>` for `identity_id` / `collection_id`
- `IdentityCollectionHoldersParams` — `limit`, `cursor`
- `MnftItemActivitiesParams` (re-used from assets.rs) — `limit`, `cursor`, `action`
- `CollectionActivitiesParams` (from assets.rs) — `limit`, `cursor`, `action`
- `ObjectItemsParams` (from assets.rs) — `limit`, `cursor`, `search`, `status`

**Responses**

- `IdentityCollectionDetailResponse` — collection_id, standard, name, counts, owned capacity/knowledge
- `CollectionHolderResponse` (from assets.rs)
- `CollectionActivityResponse` (from assets.rs)
- `CollectionItemResponse` (from assets.rs) — used for item detail and item list
- `CursorPaginatedResponse<MnftItemActivityResponse>` — per-item activities

---

### blocks (crates/api/src/routes/blocks.rs)

| Method | Path                            | Handler               | Purpose                                                              |
| ------ | ------------------------------- | --------------------- | -------------------------------------------------------------------- |
| GET    | `/api/v1/blocks`                | `list_blocks`         | List blocks (cursor-paginated, DESC)                                 |
| GET    | `/api/v1/blocks/{id}`           | `get_block`           | Get block by number or 0x-prefixed hash (with miner/reward/hardfork) |
| GET    | `/api/v1/blocks/{id}/fee-stats` | `get_block_fee_stats` | Per-block fee-rate stats (avg/min/max) and cycles                    |
| GET    | `/api/v1/blocks/{id}/proposals` | `get_block_proposals` | List block proposals with linkage to committed transactions          |

**Params**

- `ListParams` — `limit`, `cursor` (i64 block-number cursor)
- `Path(id): Path<String>` — block number string or hex hash

**Responses**

- `CursorPaginatedResponse<BlockResponse>` — block metadata + miner/hardfork activation
- `BlockResponse` — single block
- `BlockFeeStatsResponse` — total_size, total_cycles, fee-rate stats, `cycles_pending`
- `Vec<BlockProposal>` — proposal_id + committed_tx info

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

- `ListParams` — `limit`, `block_number`, `cursor`
- `Path(hash): Path<String>` — tx hash (with or without `0x`)

**Responses**

- `CursorPaginatedResponse<TransactionResponse>` / `TransactionResponse` — hash, block_number, fee, cycles
- `TransactionDetailResponse` — full detail with `inputs`, `outputs`, witnesses, fee rate, confirmations
- `Vec<CellDepResponse>` — out_point + dep_type
- `CyclesStatusResponse` (from `crate::cycles`) — status, cycles, error
- `TransactionLifecycleResponse` — phase, proposal_id, `proposed_in`/`committed_in`, commitment_distance, `commitment_window: CommitmentWindow`

---

### cells (crates/api/src/routes/cells.rs)

| Method | Path                                     | Handler                    | Purpose                                                                                           |
| ------ | ---------------------------------------- | -------------------------- | ------------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/cells/live`                     | `list_live_cells`          | List live cells with optional lock/type/type_code_hash filters (cursor)                           |
| GET    | `/api/v1/cells/by-script`                | `list_cells_by_script`     | List cells matching a script reference (`code_hash`, `hash_type`, `script_kind=lock\|type\|both`) |
| GET    | `/api/v1/cells/{tx_hash}/{output_index}` | `get_cell`                 | Get cell detail (live or consumed) including data/dep_group/code_cell_of/DAO info                 |
| GET    | `/api/v1/addresses/top`                  | `get_top_addresses`        | Top addresses by balance (warmup cache)                                                           |
| GET    | `/api/v1/addresses/active`               | `get_active_addresses`     | Most-active addresses in last N `days` (warmup cache)                                             |
| GET    | `/api/v1/addresses/{addr}`               | `get_address`              | Address summary (balance, used_capacity, live_cells_count, lock_script info)                      |
| GET    | `/api/v1/addresses/{addr}/transactions`  | `get_address_transactions` | List recent transactions for an address (cursor)                                                  |
| GET    | `/api/v1/addresses/{addr}/tokens`        | `get_address_tokens`       | List token balances for an address (cursor)                                                       |

**Params**

- `ListCellsParams` — `limit`, `lock_script_hash`, `type_script_hash`, `type_code_hash`, `cursor`
- `ListCellsByScriptParams` — `limit`, `code_hash`, `hash_type`, `script_kind` (default `both`), `cursor`
- `TopAddressesParams` — `limit` (default 100)
- `ActiveAddressesParams` — `limit` (default 100), `days` (default 7)
- `AddressTxParams` — `limit`, `cursor`
- `AddressTokensParams` — `limit`, `cursor`
- `Path<String>` (`addr`) and `Path<(String, i32)>` (`tx_hash`, `output_index`)

**Responses**

- `CursorPaginatedResponse<CellResponse>` — cell summary
- `CellDetailResponse` — full cell detail with `lock`/`type_script` (`ScriptResponse`), `data`, `data_analysis`, `dep_group_items`, `code_cell_of: Vec<CodeCellScript>`, `dao_info: Option<DaoInfo>`, `used_capacity_breakdown: OccupiedCapacityBreakdown`
- `AddressResponse` — lock_script_hash, balance, used_capacity, lock_script (`ScriptResponse`) + `lock_script_info: LockScriptInfo`
- `Vec<TopAddressResponse>` / `Vec<ActiveAddressResponse>`
- `CursorPaginatedResponse<AddressTransactionResponse>` — tx_type, capacity_change, script_labels
- `CursorPaginatedResponse<AddressTokenResponse>` — per-address token balance

---

### statistics (crates/api/src/routes/statistics.rs)

| Method | Path                                          | Handler                                  | Purpose                                                                            |
| ------ | --------------------------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------- |
| GET    | `/api/v1/statistics/network`                  | `get_network_stats`                      | Network stats (tip, hash rate, difficulty, TPS, DAO totals, sync/deep-fork status) |
| GET    | `/api/v1/statistics/tx-stats`                 | `get_tx_stats`                           | Hourly + daily transaction-count timeseries (24h + 14d)                            |
| GET    | `/api/v1/statistics/recent-blocks`            | `get_recent_blocks`                      | Recent 24h blocks (timestamp + tx count)                                           |
| GET    | `/api/v1/charts/transaction-count`            | `get_transaction_count_chart`            | Daily transaction count chart                                                      |
| GET    | `/api/v1/charts/cell-count`                   | `get_cell_count_chart`                   | Cell count chart (all/live/dead stacked area)                                      |
| GET    | `/api/v1/charts/knowledge-size`               | `get_knowledge_size_chart`               | Common-knowledge-size chart with utilization%                                      |
| GET    | `/api/v1/charts/common-knowledge-composition` | `get_common_knowledge_composition_chart` | Stacked area: transfer / DAO / UDT / NFT-Spore / other contracts                   |
| GET    | `/api/v1/charts/capacity-turnover-ratio`      | `get_capacity_turnover_ratio_chart`      | Daily + weekly capacity turnover chart                                             |
| GET    | `/api/v1/charts/cell-size-distribution`       | `get_cell_size_distribution_chart`       | Cell-size histogram chart                                                          |
| GET    | `/api/v1/charts/address-cohort-retention`     | `get_address_cohort_retention_chart`     | Address cohort retention chart                                                     |
| GET    | `/api/v1/charts/most-utilized-scripts`        | `get_most_utilized_scripts_chart`        | Top scripts share charts (used + capacity)                                         |
| GET    | `/api/v1/charts/most-utilized-assets`         | `get_most_utilized_assets_chart`         | Top assets share charts (used + capacity)                                          |
| GET    | `/api/v1/charts/block-time-distribution`      | `get_block_time_distribution_chart`      | Block time distribution histogram (last 42 epochs)                                 |
| GET    | `/api/v1/charts/epoch-time-distribution`      | `get_epoch_time_distribution_chart`      | Epoch duration histogram                                                           |
| GET    | `/api/v1/charts/epoch-time-length`            | `get_epoch_time_length_chart`            | Per-epoch length (hours) + blocks                                                  |
| GET    | `/api/v1/charts/average-block-time`           | `get_average_block_time_chart`           | Daily average block time                                                           |
| GET    | `/api/v1/charts/hash-rate`                    | `get_hash_rate_chart`                    | Daily hash-rate chart                                                              |
| GET    | `/api/v1/charts/difficulty`                   | `get_difficulty_chart`                   | Daily difficulty chart                                                             |
| GET    | `/api/v1/charts/uncle-rate`                   | `get_uncle_rate_chart`                   | Daily uncle-rate chart                                                             |
| GET    | `/api/v1/charts/miner-address-distribution`   | `get_miner_address_distribution_chart`   | Top miner address distribution                                                     |
| GET    | `/api/v1/charts/total-supply`                 | `get_total_supply_chart`                 | Total supply (circulating / DAO-locked / burnt) stacked area                       |
| GET    | `/api/v1/charts/nominal-apc`                  | `get_nominal_apc_chart`                  | Synthesized nominal APC curve (no DB read)                                         |
| GET    | `/api/v1/charts/secondary-issuance`           | `get_secondary_issuance_chart`           | Secondary issuance breakdown (compensation/mining/burnt)                           |
| GET    | `/api/v1/charts/inflation-rate`               | `get_inflation_rate_chart`               | Synthesized nominal & real inflation curves (no DB read)                           |
| GET    | `/api/v1/charts/hodl-wave`                    | `get_hodl_wave_chart`                    | HODL-wave bands (24h…>3y) + holder_count                                           |
| GET    | `/api/v1/stats/daily-activities`              | `get_daily_activity_stats`               | Per-day activity breakdown by tx-type and script (param: `days`, default 30)       |
| GET    | `/api/v1/stats/activity-summary-24h`          | `get_activity_summary_24h`               | Rolling 24h activity summary aggregated from hourly buckets                        |
| GET    | `/api/v1/statistics/asset-ecosystem`          | `get_asset_ecosystem`                    | Top tokens + capacity-category breakdown (tokens/objects/DAO/other)                |

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
- `MinerDistributionResponse` — `data: Vec<MinerDistributionDataPoint>`
- `Vec<DailyActivityStatsResponse>` / `ActivitySummary24hResponse` — per-day or 24h activity counts + `script_counts: Vec<ScriptCountEntry>`
- `AssetEcosystemResponse` — `top_tokens: Vec<TopTokenEntry>` + `capacity_breakdown: Vec<CapacityCategory>`

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

| Method | Path             | Handler  | Purpose                                                                                                          |
| ------ | ---------------- | -------- | ---------------------------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/search` | `search` | Universal search across blocks/tx/address/cell/script/token/spore/cluster/asset (supports `scope:term` prefixes) |

**Params**

- `SearchParams` — `q: String`

**Responses**

- `SearchResponse` — `results: Vec<SearchResult>`, `query`, `normalized_query`, `ambiguous`

---

### tokens (crates/api/src/routes/tokens.rs)

| Method | Path                                                 | Handler                    | Purpose                                                              |
| ------ | ---------------------------------------------------- | -------------------------- | -------------------------------------------------------------------- |
| GET    | `/api/v1/tokens`                                     | `list_tokens`              | List tokens from warmup cache (search/standard/cursor)               |
| GET    | `/api/v1/tokens/{type_hash}`                         | `get_token`                | Get token detail + holders/transfers/24h aggregates + owned capacity |
| GET    | `/api/v1/tokens/{type_hash}/holders`                 | `get_token_holders`        | List ranked token holders (cursor)                                   |
| GET    | `/api/v1/tokens/{type_hash}/transfers`               | `get_token_transfers`      | List token transfers (cursor)                                        |
| GET    | `/api/v1/tokens/{type_hash}/activities`              | `get_token_activities`     | List token activities (cursor)                                       |
| GET    | `/api/v1/tokens/{type_hash}/charts/capacity-history` | `get_token_capacity_chart` | Stacked-area capacity history chart (date range)                     |

**Params**

- `ListParams` — `limit`, `standard`, `cursor`, `search`
- `HolderParams` — `limit`, `cursor`
- `TransferParams` — `limit`, `cursor`
- `ActivityParams` — `limit`, `cursor`
- `ChartRangeParams` — `from`, `to`
- `Path(type_hash): Path<String>`

**Responses**

- `CursorPaginatedResponse<TokenResponse>` / `TokenResponse` — token info + standard, supply, holders/transfers
- `CursorPaginatedResponse<TokenHolderResponse>`
- `CursorPaginatedResponse<TokenTransferResponse>` — tx_hash, from/to, amount, is_mint/is_burn
- `CursorPaginatedResponse<TokenActivityResponse>` — tx + actions + `transfers: Vec<TokenTransferDetail>`
- `StackedAreaChartResponse` — used/unused capacity series

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

---

### spore (crates/api/src/routes/spore.rs)

| Method | Path                                                          | Handler                      | Purpose                                                                                        |
| ------ | ------------------------------------------------------------- | ---------------------------- | ---------------------------------------------------------------------------------------------- |
| GET    | `/api/v1/spore/clusters`                                      | `list_clusters`              | List Spore clusters from warmup cache (cursor by block)                                        |
| GET    | `/api/v1/spore/clusters/{cluster_id}`                         | `get_cluster`                | Get cluster detail (supports `sole-spores` alias)                                              |
| GET    | `/api/v1/spore/clusters/{cluster_id}/charts/capacity-history` | `get_cluster_capacity_chart` | Cluster capacity history (stacked area, date range)                                            |
| GET    | `/api/v1/spore/clusters/{cluster_id}/holders`                 | `get_cluster_holders`        | List ranked cluster holders (cursor)                                                           |
| GET    | `/api/v1/spore/clusters/{cluster_id}/activities`              | `get_cluster_activities`     | List cluster activities (cursor, optional `action` mint/transfer/burn)                         |
| GET    | `/api/v1/spore/clusters/{cluster_id}/spores`                  | `get_spores_by_cluster`      | List spores in a cluster (cursor by block, from in-memory cache)                               |
| GET    | `/api/v1/spore/objects`                                       | `list_spores`                | List all live spores (cursor by block, from in-memory cache)                                   |
| GET    | `/api/v1/spore/objects/{spore_id}`                            | `get_spore`                  | Get spore detail (with cumulative owned capacity from daily deltas)                            |
| GET    | `/api/v1/spore/objects/{spore_id}/activities`                 | `list_spore_item_activities` | Per-spore activities (cursor, optional `action`)                                               |
| GET    | `/api/v1/spore/objects/{spore_id}/decode`                     | `decode_spore`               | DOB-decoded traits + media (with on-the-fly SVG render URL when applicable)                    |
| GET    | `/api/v1/spore/objects/{spore_id}/media/{hash}`               | `serve_media`                | **Binary** content-addressed media blob (raw bytes, CSP/no-sniff headers)                      |
| GET    | `/api/v1/spore/objects/{spore_id}/render`                     | `render_spore_svg`           | **Raw SVG** rendered on-the-fly from decoded traits or cluster DOB1 patterns (`image/svg+xml`) |
| GET    | `/api/v1/spore/objects/{spore_id}/charts/capacity-history`    | `get_spore_capacity_chart`   | Single-spore capacity history (stacked area, date range)                                       |
| GET    | `/api/v1/spore/owner/{lock_hash}`                             | `get_spores_by_owner`        | List spores owned by a lock_hash (cursor by block)                                             |

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
- `CursorPaginatedResponse<ClusterHolderResponse>` / `CursorPaginatedResponse<ClusterActivityResponse>`
- `CursorPaginatedResponse<MnftItemActivityResponse>` for per-spore activities
- `SporeDobDecodeResponse` — `status` (`decoded` \| `failed` \| `pending`), traits, `media: Vec<DecodedMediaResponse>`, `issues` (carries the failure reason when `failed`)
- `axum::Response` for `serve_media` (any media MIME) and `render_spore_svg` (`image/svg+xml`) — not JSON
- `StackedAreaChartResponse` for capacity history charts

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
- `CodeCellQuery` — `code_hash`, `hash_type` (unused alias)
- `ScriptCapacityHistoryQuery` — `code_hash` (Option), `script_kind`, `from`, `to` (used with `{name}` path)
- `ScriptCapacityHistoryByCodeHashQuery` — required `code_hash`, plus `script_kind`/`from`/`to`
- `Path(name): Path<String>`

**Responses**

- `CursorPaginatedResponse<ScriptFamilyListItemResponse>` — script family listing (from `crate::response`)
- `HashMap<String, ScriptLookupInfo>` — keyed by reference hash; each entry has resolution_state (`resolved`/`ambiguous`) and optional `ambiguity: ScriptResolutionAmbiguityResponse`
- `CodeCellResponse` — `tx_hash`, `output_index` (option of either)
- `CodeCellsResponse` — `code_cells: Vec<CodeCellEntry>`, counts, optional `resolved_version_hash`, optional ambiguity
- `ScriptFamilyDetailResponse` — family + `versions` (from `crate::response`)
- `ScriptUsageResponse` — totals + `by_deployment: Vec<DeploymentUsage>`
- `StackedAreaChartResponse` (from statistics) for capacity history charts

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

**Total endpoints: 103** across 17 modules. Confirm against
`crates/api/src/routes/*.rs` for any field-level question; this skeleton is
intentionally name-and-purpose only.
