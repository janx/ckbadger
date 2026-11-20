# Testing Systems

ckbadger has three testing systems, each answering a different question:

| System     | Question                                     | Runs against                    |
| ---------- | -------------------------------------------- | ------------------------------- |
| **verify** | Is the indexed data correct?                 | API backed by a completed index |
| **bench**  | How fast is each endpoint?                   | API + frontend                  |
| **stress** | How much load can it handle before breaking? | API + frontend                  |

`verify` needs the target network's API backed by a completed index; the indexer need not remain
running. `bench` and `stress` also exercise the frontend. None modify the database.

## Verify

Data integrity verification. Calls the API and optionally the official CKB explorer to confirm indexed data matches the chain.

### Quick Start

```bash
ckbadger verify --depth fast              # 10 checks, seconds
ckbadger verify --depth sampling          # 79 checks, minutes
ckbadger verify --list-checks             # List all checks
```

When `-C` points at an orchestrator root, `verify` runs every `[[network]]` in
declaration order. Point `-C` at a network subdirectory (for example,
`ckbadger -C work/testnet verify`) to verify only that network.

### Check Tiers

| Tier                  | Checks | Runtime | What it validates                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --------------------- | ------ | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Fast** (F1-F10)     | 10     | seconds | API reachable, sync complete, genesis block, tip block, deep fork clear, DAO statistics sane, genesis-baseline burnt invariant, data-hash index files code cells only, every genesis Type ID binary's impossible pairs named by none (scripts named by their pair), the store's layout is this build's (a skipped re-sync)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| **Sampling** (S1-S43) | 43     | minutes | Block hash roundtrip, parent chain, address balance, chart validations (tx count, cells, supply, block time, epoch, HODL wave, knowledge composition, APC, inflation), supply invariants, RPC compare, tokens, spores, NFTs, holder consistency, DAO status index vs deposits, participant rows consistency, entity capacity history vs chain, `.cell` ring integrity, `.cell` id-is-label-hash, `.cell` owner index, `.cell` records hash parity, DAO lifecycle vs the node's live DAO cells, global activity bucket index vs per-block classification, address cellbase counts vs block rewards, token live cell counts vs the type index, collection daily activity counts vs activity rows, derived DAO depositor rows vs the DAO lifecycle, derived cluster activity counts vs activity rows, the derived `.bit` → `.bit Cell` index vs `.bit Cell` items, each complete day's activity kinds and cellbases vs its transactions, each recent activity's senders vs its inputs' locks, each script pair's live cells (the lookup) vs the cells the index lists by that pair, each recent activity's recognised type codes vs its cells' , `live_code_cells_by_type` vs the node's live code cells (both ways on a sample) |
| **Explorer** (X1-X26) | 26     | minutes | Compare last 30 days against official CKB explorer API (tx count, DAO deposit, hash rate, difficulty, knowledge size, uncle rate, cell counts, supply, circulation, compensation, mining reward, normalized treasury)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |

Explorer `burnt` and `treasury_amount` are not compared raw: the official explorer leaves phase-1
frozen DAO interest in treasury while also including it in deposit compensation. The verifier
subtracts ckbadger's exact lifecycle-derived phase-1 amount from the Explorer legacy partition.
`/charts/secondary-issuance` exposes that materialized amount as
`phase1CompensationShannons`; it is not inferred from the treasury value being checked. Mainnet's
daily checks use absolute comparisons. Testnet's daily checks compare normalized changes because
its Explorer statistics retain historical constant baseline gaps; the NervosDAO point check uses
the normalized latest completed day. No tolerance is widened. See POSTMORTEM DAO-029 and DAO-031.

Explorer X26 compares its cumulative `total_depositors_count` with the separate completed-day
`/dao/charts/ever-depositors` series, which reads the indexed count of locks that have ever
deposited. The `/dao/charts/total-deposit` secondary series remains **current** depositors and
must not be compared with Explorer's cumulative count. X26 retains its 20% bound for Explorer's
repeated counting of returning depositors (POSTMORTEM DAO-020).

The secondary-issuance Sampling check verifies exact **completed-day partition conservation**:
compensation + miner issuance + burnt treasury must equal that day's independent
`protocolTotalShannons`, and frozen phase-1 compensation cannot exceed the compensation share.
It does not compare a historical chart point with the newer live `/dao/statistics` singleton or
independently establish each category's allocation. The Fast and Sampling burnt checks compare
matching completed dates in exact shannons against the genesis burnt invariant; the total-supply
monotonicity check also sums exact shannons. These chart checks depend on the API's indexed
monotone chain-time calendar for complete-day boundaries and blockless-day carry; they do not
independently verify that calendar.

`--depth fast` runs Fast tier only. `--depth sampling` runs all three tiers (Fast + Sampling +
Explorer). By default, `ckbadger verify` resolves each network's API, RPC, official explorer and
cache directory from its work directory. A run can select checks with `--checks`, adjust
deterministic sampling with `--sample-count` and `--seed`, and override API/RPC/explorer URLs or
the cache directory when `-C` names a single-network work directory. Endpoint overrides are
rejected when `-C` names an orchestrator root with multiple networks.

`data_hash_index_only_code_cells` (F8, needs `--rpc-url`) reads the bounded raw export
`GET /verify/data-hash-rows?data_hash=…`. A `cell_by_data_hash` row exists iff its cell is a code
cell (`keys::is_code_cell`: non-empty data that starts with the ELF magic, the one rule every
ELF test uses; a Type ID cell holding state is not code; shared by the live writer and the bulk
build; a rollback restores the rows from the block
journal, which the rollback property suite `crates/indexer/tests/rollback_property.rs` pins with a
code cell created and consumed above the fork). The check asks for the hash of empty data, which
must name no row, and, for every output of the node's genesis block that carries data (the system
binaries, their dep group, the secp256k1 data), for the rows under the hash of its data: a code
cell must be among them, any other cell must not. The export itself answers 500 for a row that
names neither a live nor a consumed cell. A store built before C-2 filed every cell with data, and
one built before the fix round filed Type ID cells holding state; both fail until re-synced.

`script_names_by_pair` (F9, needs `--rpc-url`) holds the one rule of a script's name: a script is
named by the code its pair (code hash, hash type) runs (lane rd/api-lock-names). For every output
of the node's genesis block deployed under Type ID whose data is code, it asks
`POST /scripts/lookup` for all eight pairs on the binary's two hashes. Four of them are no script —
the Type ID hash by `data`, `data1` or `data2` (a Type ID hash is no code's data hash) and the
code's data hash by `type` (it is no type script's hash) — so they run no code and the lookup must
answer each "Unknown" (or not at all). Four run the binary: where the lookup names the Type ID by
`type`, it must answer the data hash by `data`, `data1` and `data2` with the same name, with no
hash type beside the name but the pair's own (`nameBy`). Every pair must be answered as itself (its
`hashType`), no pair that was not asked, and at least one binary must be named by its Type ID: a
lookup that answers `{}` names nothing to hold to the rule and fails (it passed before the fix
round, saying the eight pairs were "answered as themselves"). What it cannot see: a genesis binary
the registry does not register is named by nothing, and the check then holds only its four
impossible pairs; and it reads the lookup's names, not the store's layout (a store from before the
lane is refused by the indexer and the API at start, and named by the next check).

`store_layout_matches_build` (F10) holds the store the API serves to this build's layout
(`ckbadger_store::STORE_LAYOUT`): `/statistics/network` states the domain store's `store_layout`
stamp, which the indexer writes on a fresh store before its first write. A store an older build
wrote carries no stamp (`storeLayout: null`), an API from before the stamp states none (no
`storeLayout`), and a store of another layout states its own: each fails, naming both layouts and
"delete RocksDB and re-sync from genesis". The indexer and `ckbadger label-import` refuse such a
store before they write anything, and the API refuses it at open; this check is what
`verify --depth fast` says when the re-sync was skipped while old services still run.

`script_reference_live_cells_consistency` (S41) holds a script's figures to its own cells. For
every code hash in `/stats/activity-summary-24h` `scriptCounts`, by each of its four hash types, the
lookup's `liveCellsCount` of the pair (its `script_reference_info` row, as lock plus as type) must
equal the live cells `/cells/by-script` lists by that pair as lock and as type, walked to the end;
past 1,000 rows of one kind the walk stops and the figure is held only to the rows walked (the
result line names those pairs). For such a pair the check proves only that `liveCellsCount` is at
least the rows walked: it cannot see an overcount, and the by-script route's `total` is the same
per-pair counter the lookup reads (`script_reference_info`), so there is no independent exact
figure to compare with short of walking every row. A difference is a finding only when the API's view stood at one
block before and after the reads; one never read at one block, after re-reads for up to the settle
budget, leaves the check inconclusive. A store built before the lane counted every hash type of a
code hash in one row and answered (X, data) with the cells of (X, type) as well.

`address_balance_spot_check` is deliberately bounded. It deterministically
selects at most 10 recently active addresses across live-cell-count bands, then
checks every live cell of each selected address exactly. Candidates with more
than 1,000 live cells or 10,000 transactions are excluded; the complete sample
is capped at 5,000 live cells and 50,000 transactions. For each selected
address, both of these invariants must hold:

```text
stored balance          == sum(capacity of all live cells)
stored liveCellsCount   == number of live cells returned
```

If the live-cell endpoint returns more cells than its stored count declares,
the check fails at the first proving page instead of expanding into an
unbounded scan.

> **Scope:** `verify` covers only chain-derived data (the domain + append-only stores). The
> **network store** (`net_nodes` / `net_stats` / `net_crawl`, written by the opt-in `ckbadger-crawler`) is
> **outside** all 77 checks — it holds observational, non-chain p2p-crawler data that is
> non-deterministic and not subject to chain-integrity invariants, so none of these checks apply
> to it.

### CLI Reference

```
ckbadger verify [OPTIONS]

OPTIONS:
  --depth <DEPTH>          fast or sampling [default: fast]
  --list-checks            List all checks and exit
  --checks <NAMES>         Run only these checks (comma-separated)
  --no-explorer            Skip official explorer comparison checks
  --format <FORMAT>        text or json [default: text]
  --sample-count <N>       Samples for the sampling tier [default: 1000]
  --seed <N>               Deterministic sampling seed [default: 42]
  --tolerance <F>          Max explorer deviation, fraction [default: 0.001]
  --entity <KIND:ID>       Verify this entity exactly (repeatable), e.g. token:0x…
  --entity-max-rpc <N>     RPC requests the chain-derived checks may spend, per network [default: 1000000]
  --entity-max-records <N> History records they may fold in, per network [default: 2000000]
  --entity-budget-seconds <N>  Wall-clock seconds they may spend, per network [default: 600]
  --api-url <URL>          Override the ckbadger API base URL
  --rpc-url <URL>          Override the CKB RPC URL
  --explorer-url <URL>     Override the explorer API URL
  --cache-dir <DIR>        Override the explorer response cache directory
```

`--checks` fails on an unknown name, and on a name whose tier is above `--depth`,
rather than reporting an all-green run over zero checks. The four endpoint
overrides describe one network, so they are rejected when an orchestrator root
selects multiple networks — run them against `-C <workdir>/<network>`.

### Check Statuses and Exit Codes

A check reports one of six statuses. The distinction the old pass/fail pair
could not express is between _evidence of an inconsistency_ and _absence of
evidence_: a check that never ran, could not reach the data, or was given
nothing to sample proves nothing, and is never rendered green.

| Status          | Meaning                                                                                 | Exit contribution |
| --------------- | --------------------------------------------------------------------------------------- | ----------------- |
| `pass`          | The check's whole declared scope was verified and agreed                                | 0                 |
| `fail`          | A difference was proven                                                                 | 1                 |
| `inconclusive`  | Anchor moved, budget exhausted, adapter unsupported, or the source was incomplete       | 2                 |
| `error`         | RPC/response schema, a cursor that did not advance, a bad parameter, or a local failure | 2                 |
| `skipped`       | The operator narrowed the scope (`--checks`, `--no-explorer`, no `--rpc-url`)           | 0                 |
| `notApplicable` | Independently proven that the network holds no such object                              | 0                 |

Three rules decide which of these a check may report:

- **Only a 404 is evidence of absence.** A check whose subject may legitimately
  not exist on a network (a protocol with no deployment there) reads it
  through `api_get_or_not_found`: an HTTP 404 on that resource is
  `notApplicable`. Every other API failure — a 5xx, a transport error, a body
  that does not decode — is `error`, never a pass over zero items.
- **Uncovered scope is `inconclusive`.** Anything the operator asked for that
  the check cannot verify — an `--entity` family without an adapter, an
  exhausted budget — keeps the check from `pass`, whatever the rest found.
- **An empty sample verifies nothing.** A check that sizes its sample from
  `--sample-count` declares it (`requires_sampling`), and `--sample-count 0`
  makes it `error` instead of a pass over an empty selection.

The process exit code merges every check of every selected network:
**`fail` (1) > `error`/`inconclusive` (2) > `pass` (0)**. A skipped check no
longer counts as a pass; it instead sets `scopeComplete: false`, which is
reported separately from `status` — a run can be `fail` _and_ incomplete at the
same time, and that is not softened in either direction.

Every selected network is verified and reports, even when an earlier one has
already failed. `--format json` writes exactly one parseable document to stdout
(progress and diagnostics go to stderr):

```json
{
  "schemaVersion": 1,
  "runId": "20260923T101500Z-abcdef12",
  "status": "fail",
  "scopeComplete": false,
  "networks": [ { "network": "mainnet", "status": "fail", "scopeComplete": false, "checks": [ … ] } ]
}
```

Each network's report is also persisted to
`<network workdir>/perf/verify/<run-id>/report.json` — written to a temp file
and renamed, so an interrupted run leaves no partial file that could be read as
complete evidence.

### Asset List vs Detail Counts (S21, S22)

`object_asset_collection_consistency` and `asset_top_holders_address_consistency` compare an
asset's `/assets` row with its own endpoints (detail, item page, holder page). The two sides are
not read at the same block by construction: `/assets` answers from the snapshot the API rebuilds
when the tip moves (it then rests 30 s), and names the block that snapshot was built at in
`anchor`; every other endpoint reads the store view of its request. A row and a detail are
comparable only at one block, so a count difference is classified by where it was read:

| Reading                                                                                                              | Result                                                           |
| -------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| the row and the live endpoints agree                                                                                 | pass                                                             |
| they differ, and a re-read agrees (the snapshot caught up with the block the detail was read at)                     | pass; `N asset(s) agreed on a re-read` on the result line        |
| they differ with the list `anchor` equal to the API's view (`/assets/summary` `anchor`) before and after the re-read | fail; the finding names the block (`every read at block N`)      |
| they differ and no reading within 100 s (per check run) stood at one block, or the snapshot's `anchor` is `null`     | inconclusive; the reason names the asset, the counts, the blocks |

`asset_top_holders_address_consistency` then compares each top holder's address detail with the
address's transaction and live-cell totals. Those are all reads of the request's own view: the
detail's memo is keyed by the tip block (`docs/API.md` → `transactionsCount` / `total`), so it
cannot answer a block behind the totals.

A missing `anchor` key on `/assets` is an error: the check does not classify against a list that
does not say what it read. Contract violations that no re-read can change (a wrong `assetType`,
an id that is not the asset's, a page without `total`) are findings at once. Neither check needs
RPC.

### `.cell` (DotCell) Names (S27-S30)

Four Sampling checks, store-vs-store except the last, which reads cells and
witnesses back from the node. Applicability is decided by the collection route:
a 404 from `GET assets/identities/dotcell` makes each check `notApplicable` (no
`.cell` deployment on this network); any other failure of that read is `error`.
`dotcell_records_hash_parity` samples `--sample-count` names (at most 20), and
`dotcell_owner_index_consistency` reads each resolvable owner's whole
`/dotcell-names` list, page by page, before comparing it with the counter.

| Check                             | What it proves                                                                                                                                                     |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `dotcell_ring_integrity`          | Walking `next` from the ring root visits every live name exactly once, in strictly ascending id order, ending at the zero id; the walk's length equals `liveCount` |
| `dotcell_id_is_label_hash`        | Every name's id is `blake2b(label)[..20]` — the id is derived, never stored, so a mismatch means the label and the key disagree                                    |
| `dotcell_owner_index_consistency` | Per-owner counts sum to `liveCount`, the holder rows equal `holdersCount`, and each resolvable owner's `/dotcell-names` list matches its counter                   |
| `dotcell_records_hash_parity`     | For a sample of live names: the node's cell data `[1..33]` equals the stored `recordsHash`, and the witness at the cell's own output index decodes to it           |

`identity_collection_holder_consistency` deliberately does **not** cover `.cell`.
It asserts that an owner holds at least as many live cells as identity items,
which is true when each identity sits in a cell under the owner's own lock.
Every `.cell` name sits under the protocol's Account Lock instead, so that
premise does not hold; `dotcell_owner_index_consistency` is its replacement.

### Global Activity Bucket Index (S32)

`global_activity_bucket_index_consistency` proves `CF_TX_ACTIONS_BY_BUCKET`
(the index behind every filtered `/activities` and `/transactions` listing)
against the classification it encodes. It takes the blocks of the
`--sample-count` most recent activities (at most 20 distinct blocks) and, for
each block and each of the seven buckets, compares two API reads:

| Read                                                       | Source                                                                            |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `activities?blockNumber=B&filter=<bucket>`                 | every canonical transaction of block `B`, classified from its own `TxActions` row |
| `activities?filter=<bucket>&cursor=<B+1>:0`, until below B | the bucket index rows of block `B`, each loaded back with its `TxActions` row     |

A transaction only in the first has no bucket row; one only in the second is a
bucket row for a transaction that does not classify there. Both are findings
naming the block, bucket, transaction index and hash. A bucket row whose
`TxActions` row is missing, or that disagrees with its row's classification,
makes the bucket listing itself fail with that position, which the check
reports as an error. It needs no RPC.

### Address Cellbase Counts (S33)

`address_cellbase_count_consistency` proves `AddressBalance.cellbase_txs_count`
— what the unfiltered `/addresses/{addr}/activities` `total` subtracts from the
address's `transactionsCount` — against the block headers. It samples up to 20
blocks (`--sample-count`, deterministic with `--seed`) from `1..=tip`; a block
whose reward is not yet paid is skipped. For each other block:

| Assertion                                                                                                                                         | Applies to                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| the miner (`minerAddress`) holds the paying cellbase (`miningRewardTxHash` at `rewardPaidAtBlock`) as a cellbase row of its own transaction index | every sampled miner                                     |
| `1 <= transactionsCount - total <= transactionsCount`                                                                                             | every sampled miner                                     |
| committed transaction rows == `transactionsCount`, and `transactionsCount - total` == cellbase rows                                               | miners with at most 10,000 transactions (paged in full) |

A miner above the bound is never paged in full. A failing sample is re-read
once; if it still fails and the tip moved meanwhile, it is reported as skipped
rather than as a finding. It needs no RPC.

### Token Live Cell Counts (S34)

`token_live_cells_consistency` proves the persisted token live cell count —
`cellsCount` on `/tokens` and `/tokens/{hash}`, the running sum of the token's
`TokenDailyDelta.live_cells_delta` rows — against the type index. The API reads
only the daily rows; this check is where they meet the cells. For up to 20
tokens of the first `/tokens` page (`--sample-count`), in list order:

| Assertion                                                                                  | Finding names |
| ------------------------------------------------------------------------------------------ | ------------- |
| the detail's `cellsCount` equals the list row's                                            | the token     |
| every row `cells/live?type_script_hash=<token>` lists carries the token's type script hash | the cell      |
| the number of those rows (all pages) equals `cellsCount`                                   | the token     |

A token above 25,000 cells, or one that would take the run past 60,000 listed
cells, is not paged; the pass detail names it as not sampled. It needs no RPC.

### Collection Daily Activity Counts (S35)

`collection_daily_activity_consistency` proves `collection_activity_daily` — what the insights
`weeklyActivity` folds — against the collection activity rows. For up to 10 collections of the
first `/assets?type=object` and `/assets?type=identity` pages (`--sample-count`), in list order:

| Assertion                                                                                                                                                                                                                            | Finding names               |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------- |
| every completed week of `assets/objects/{id}/insights` equals the collection's `assets/objects/{id}/activities` rows (all pages) folded by `CollectionActivityDailyCounts::of_actions` into the UTC+8 week of each row's `chainTime` | the collection and the week |
| the rows listed equal the collection's `activitiesCount` (re-read from the detail after the walk, so a tip that moved is not a finding)                                                                                              | the collection              |

Rows after `completeThrough` (the tip's incomplete week) are on neither side. A collection above
5,000 rows, or one that would take the run past 20,000 listed rows, is not paged; the pass detail
names it as not sampled. It needs no RPC.

### Derived DAO Depositor Aggregate (S36)

`dao_depositor_aggregate_consistency` proves the indexer-derived `dao_live_agg` (round 3 L1)
against the DAO lifecycle rows it is derived from, through the API:

| Assertion                                                                                                                                                                                          | Finding names               |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| one walk of every `/dao/depositors` page, all ranked at one anchor tip `T`, sums to `/dao/evidence` `phase0` at `T` and has `concentration.currentDepositors` rows (global row = Σ depositor rows) | `dao/evidence@T`            |
| ranks are `1..=total` and `total` equals the rows listed                                                                                                                                           | the depositor / the listing |
| for up to 20 sampled depositors (`--sample-count`; the largest and the smallest), the row equals that lock's `/dao/deposits/{lock}` rows that were phase-0 at `T`                                  | the depositor               |

Each lifecycle row's own transition blocks give its state at `T`, so a tip that moves during the
per-lock walks is not a finding. If the tip moves during each of three `/dao/depositors` walks
the check is `inconclusive`. It needs no RPC: about 165 depositor pages plus the sampled locks'
deposit pages on mainnet.

### Derived Cluster Activity Counts (S37)

`cluster_activities_count_consistency` proves the indexer-derived
`object_collection_activity_count` (round 3 L2) — what a Spore cluster's `activitiesCount` is —
against the rows it counts. For up to 10 Spore clusters of the first `/assets?type=object` page
(`--sample-count`), in list order, `/spore/clusters/{id}` `activitiesCount` must equal the rows of
`/spore/clusters/{id}/activities` (all pages); a difference the detail does not confirm after the
walk (the tip moved) is not a finding. A cluster above 5,000 rows, or one that would take the run
past 20,000 listed rows, is named as not sampled. It needs no RPC. S35 additionally compares the
asset list's `activitiesCount` with the listed rows for the same first page.

### Derived `.bit` → `.bit Cell` Index (S38)

`bit_cell_account_index_consistency` proves the indexer-derived `bit_cell_by_account` (round 3
L3) both ways for up to 20 `.bit Cell` items of the first `/assets/identities/bit_cell/items`
page (`--sample-count`): the `.bit` identity a `.bit Cell` names (`blake2b(name)[..20]`) lists it
in `bitCells` exactly when the `.bit Cell` is live, and every `.bit Cell` that `.bit` identity lists
is live and names that account. A `.bit Cell` naming an account with no `.bit` identity on this
chain is reported in the pass detail, not as a finding. It needs no RPC.

### Daily Activity Kinds vs Transactions (S39)

`daily_activity_kinds_partition_transactions` proves the one-kind rule of the daily activity rows
(F-6, POSTMORTEM STATS-019) against a count written by other code: for every complete day of
`/stats/daily-activities?days=0`, the sum of its `kindCounts` (each non-cellbase transaction in
exactly one kind) plus its `coinbaseCount` must equal the day's transactions in
`/charts/transaction-count`, which the chain stats writer counts from each block's transaction
list (`DailyStats.transactions_count`). A transaction counted in no kind, or in two, is a finding
naming the day, the kinds and the gap; so is a day the blocks carry transactions on that has no
activity row, or the reverse. The activity rows are read before and after the transaction counts,
and a day whose row moved between the two reads (a reorg across its end) is not compared; with
no day compared the run is inconclusive. The cellbase count comes from the activity rows
themselves (the API sends no independent per-day block count), so a transaction misfiled as a
cellbase is outside this check. It needs no RPC and reads no `--sample-count`.

### Senders vs Input Locks (S40)

`participant_sent_matches_input_locks` proves the indexer's senders bit (`TAG_INPUT`, the API's
`sent`) against the transaction's inputs as ckbadger's own cells hold them: for up to 50 recent
activities (`/activities`, `--sample-count`), each `/transactions/{hash}` input's lock script is
hashed here, and every party of the transaction's own activity (`/transactions/{hash}/activity`,
which draws every party; a feed row draws at most six) must state `sent` exactly when its lock is
one of those input locks; every input's lock must be a party that sent. The feed row's own
statement of the bit, the tags of every party it names in `partyIds` (the bit a folded run counts
its senders by), must say what `sent` says. A party that sends with no input of its lock, holds
one without sending, an input lock that is no party, or a feed row naming a party with the bit
`sent` denies (or not naming it) is a finding naming the transaction and the lock. A store synced
before the bit existed reads as "nobody sent" and fails on every activity. It needs no RPC.

### Recognised Type Codes vs Cells (S42)

`activity_recognized_types_match_cells` proves an activity's recognised type codes (C7: the row's
`recognized_type_codes`, the API's `recognizedTypes`) against the transaction's cells as
ckbadger's own stores hold them: for up to 50 recent activities (`/activities`, `--sample-count`),
the type script of each `/transactions/{hash}` input and output is taken, and the codes the
activity builder recognises by their pair (`is_recognized_type`, its one classification through
the one registry a protocol's pair is read by) must be exactly the codes the row states, each
(code hash, hash type). A recognised code on a cell the row does
not state, a stated code no cell carries, or an input whose cell the store does not hold is a
finding naming the transaction and the code. A store synced before the list reads as rows in
which no recognised type ran and fails on every activity whose cells carry one. It needs no RPC.

### Live Code Cells by Type vs the Node (S43)

`live_code_cells_index_matches_chain` holds `live_code_cells_by_type` (resync fix round S3, the
index a generic type reference is settled from) to the node, both ways on a sample. The rows are
sampled from `--sample-count` random keys (at most 32), each the first row at or after its key
(`GET /verify/live-code-cells?from=`). Each sampled row must name a cell the node holds live
(`get_live_cell`), under a type script whose hash is the row's and which is not Type ID, whose
data hashes to the row's binary and is code (`keys::is_code_cell`): a row that fails one is a stale
row. For each sampled row's type script, every live code cell the node lists under it
(`get_cells`, up to 1,000 cells) must have its row (`GET /verify/live-code-cells?type_hash=`): a
cell without one is a missing row. A finding stands only when a second read of the type's rows
still shows it (a block indexed between the reads is not reported); a type whose cells or rows
exceed one walk is named as not compared. A type that lost every row is never sampled (the
check's blind spot). It needs RPC.

Its limit: it recognises codes through the builder's own `is_recognized_type`, so it shares the
builder's recognition rule. It catches a row that does not hold what the builder recognises (a
writer, a decode or a route fault), never a wrong recognition rule: a code the classifier wrongly
recognises, or wrongly does not, is wrong on both sides alike and passes. The rule itself is pinned
only by the builder's and the registry's own tests.

### DAO Lifecycle vs Live DAO Cells (S31)

`dao_live_cells_match_node` proves the DAO lifecycle cell by cell against the
node. Every live NervosDAO cell is exactly one lifecycle entry: a deposit
(status 0, keyed by its own outpoint) or a withdraw request (status 1, keyed
by the request outpoint). At one anchor block `H` — the tip
`/dao/statistics` reports — it requires, with zero tolerance:

| Compared                                       | Against the node's live DAO cells at `H`                                                              |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| the lifecycle's live cells (`/dao/deposits`)   | the same outpoints, each with the same kind and capacity (and, for a request, the same deposit block) |
| `/dao/statistics` `activeDeposits`             | deposits + withdraw requests                                                                          |
| `totalDeposited` / `pendingWithdrawalCapacity` | Σ capacity of the deposits / of the withdraw requests                                                 |
| `/cells/live-summary` `classes.dao`            | the live DAO-cell count at the summary's own tip                                                      |

Both sides are pinned to `H` exactly rather than raced. The unfiltered
`/dao/deposits` listing is ordered by deposit block, which a status change
never moves, so a walk started after `H` sees every deposit created at or
before it, and each row's own transition blocks give its state at `H`. On the
node, `get_cells` cells created after `H` are dropped (it reports each cell's
creation block) and cells consumed after `H` are re-added from the node's own
history (`get_transactions` over `(H, tip]`, each spent cell read from its
creating transaction). The source is qualified through `verify-source.toml`
like S25; a reorg below the walk's end is `inconclusive`, never `fail`.

It needs a reachable CKB RPC (from the network work directory or `--rpc-url`). It costs one walk
of every DAO deposit through the API (~920 pages on mainnet), one `get_cells` walk (~25 pages)
and a handful of transaction reads: about 80 s on mainnet, hence Sampling. It would have caught
POSTMORTEM DAO-032 on the first run after the catch-up that caused it; on the
store that incident left behind it reports exactly the three spent withdraw
requests and the +3 / +158,705.40 CKB statistics excess. The indexer enforces
the count half of the same invariant before every live commit (the lifecycle's
status-0/1 count must equal the live-cell summary's DAO class), so this check
is the per-outpoint, per-capacity and cross-source proof of it.

### Entity Capacity Verification with an Indexed Calendar (S25)

`entity_capacity_history_matches_recorded_calendar` recomputes entity capacity
amounts from the chain and compares the complete history using the index's
calendar, with **zero tolerance** —
`--tolerance` belongs to the explorer comparisons and is not inherited here.

- **Capacity amounts are independent.** The oracle enumerates the entity's full
  `[0, H]` history through the CKB node's own indexer (`get_transactions`,
  exact script match, ungrouped, cursor-paged), resolves every input's prevout,
  and recomputes capacity and occupied capacity from the cells. Amounts never
  come from the production writer, parser or `PROTOCOL_REGISTRY`.
- **Calendar dependency is explicit.** The typed export includes UTC+8 date
  intervals covering every block in `[0, H]`, read from persisted
  `chain_time_ms` under the same pin as the daily rows. The API finds day
  boundaries by binary search; the verifier does not fetch a header prefix.
  Every entity record is assigned to its interval, with gaps, overlaps and
  malformed dates rejected. This is an exact comparison **given the indexed
  calendar**, not an independent proof of its dates. If calendar and daily
  rows share a date error, this runtime check may pass. The manifest and pass
  detail retain that distinction; `calendarIndependentlyVerified` is false.
- **Clock rules have separate regression coverage.** Small complete-chain
  fixtures exercise the production monotone-clock calculation, including
  quiet blocks crossing midnight, backward raw timestamps, batch boundaries
  and live rollback/replay. These tests establish the algorithm's behavior
  on the fixtures; they do not certify every persisted mainnet header.
  Reading only entity-touch headers would miss a quiet block advancing the
  clock, so that is not used as an independent date oracle either.
- **Three facets, one scan.** Per-day deltas and running prefix totals are
  compared against the exported rows; the **current** facet compares the
  chain's _live cell set_ — outputs in `[0, H]` whose outpoint is never
  consumed in that range — against the current capacity/occupied the index
  itself reports. A token has no separately stored current value, so the index
  side of that facet is the same checked accumulation the public
  `/tokens/{hash}` endpoint performs, read under the export's pin; comparing it
  against an independently computed live set is what makes the facet a real
  diagnostic rather than a restatement of the daily rows. A failure names which
  facet, which component (capacity or knowledge) and which day differed, and
  offsetting per-day errors that leave the total correct still fail.
  The collector cross-checks its own two passes: the live set and the
  accumulated daily deltas are the same quantity computed two ways, and a
  disagreement between them fails the run rather than publishing either.
- **The source must qualify first.** `<network workdir>/verify-source.toml` is
  the operator's declaration of what the node's index covers (genesis, node
  version, index start block, filters). It is checked against the live node and
  the case's anchor; anything missing or contradictory is `inconclusive`, never
  a confident `fail`.
- **Identity from the export.** The type script the chain query needs comes
  from the export, not from `/tokens/{hash}`: that endpoint accumulates the
  daily rows and so fails exactly when they are the thing under suspicion. The
  export's script is accepted only once it hashes back to the requested id.
- **Membership.** A token's history is **every cell whose type script is the
  token's** (exact code hash, hash type and args), with no data-length filter:
  an owner-mode or short-data cell that carries no parsable amount still moves
  the token's capacity and occupied capacity. The live and the bulk writer
  apply this same rule to `TOKEN_DAILY`; amount-dependent facets (supply,
  holders, transfer counts, `TOKEN_HOURLY` and the token row itself) still
  require a parsable u128 amount on both paths and never gate capacity
  membership. The
  oracle states the rule independently rather than borrowing the writers'
  parser, so a writer that drifts from it fails here.
- **Selection.** `--entity token:<type_hash>` (repeatable) picks entities
  exactly; ids are canonicalized to `0x` + lowercase hex, and an id that is not
  whole bytes of hex is rejected. Without it, the check uses the known incident
  selectors plus the head of the API's token directory (`GET /tokens?limit=8`,
  read from its cursor envelope). A selector of a family this delivery does not
  cover is listed in the manifest as uncovered and makes the run
  `inconclusive`, whatever the other selectors found. Budgets (16 entities,
  2M records, 1M RPC requests, 600 s) apply to the whole run per network;
  exhausting one is `inconclusive`. The original 10k-request allowance could
  not finish even the first mainnet incident selector. Raise the limits with
  `--entity-max-rpc`, `--entity-max-records` and `--entity-budget-seconds`
  rather than narrowing scope until a run fits — the manifest records both the
  budget and the spend. Every node call is charged, source qualification and
  the post-walk anchor re-verification included, so the manifest's
  `rpcRequests` is exactly the number of requests the node received.
  The RPC workload follows the selected entities' histories, not the chain's
  total height. A large anchor alone no longer consumes the request budget.
  The calendar export is bounded to 8,192 nonempty date intervals and shares
  the export's 4 MiB budget; an incomplete calendar cannot yield a pass.
  Mainnet run `20261027T073216Z-bc0ff6c4` at anchor 20,847,084 completed the
  default eight entities: 1,379,939 history records and 334,086 RPC requests in
  188.3 s, with zero differences. Its indexed calendar had 2,538 date intervals;
  a separate one-entity export probe took 2.23 s and returned 183,343 bytes.
  This measured the layered contract above, not the former independent full-chain
  date proof. It was a selected-check run, not a rerun of all 64 checks.
  A subsequent full mainnet run, `20261027T084559Z-08e068e3`, passed **all 64
  checks** in 428.16 s with `scopeComplete: true` and no skips, errors, failures,
  or inconclusive checks. CLI and the isolated read-only API used the same
  `625e0476` release artifact; the entity check covered eight entities at anchor
  20,847,472 in 194.60 s. The calendar dependency is unchanged. The report is
  `work/mainnet/perf/verify/20261027T084559Z-08e068e3/report.json`.
- **Coverage is auditable.** `<run-id>/manifest.json` records the anchor, the
  source profile, the budgets, what was spent, and every entity that was not
  fully covered and why. It also saves the indexed calendar and identifies
  the chain-derived amount source separately from the calendar dependency.

```toml
# work/mainnet/verify-source.toml
genesisHash = "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5"
nodeVersion = "0.210.0 (2592ddf 2026-09-17)"
indexerVersion = "0.210.0 (2592ddf 2026-09-17)"
buildStartBlock = 0
continuousFromGenesis = true
provenance = "Operator confirmed the running, self-operated CKB indexer was continuously built from genesis."
```

`nodeVersion` and `indexerVersion` must agree — the node and its in-process
indexer are one binary — and both are checked against the live node.
The versions above are an example, not a supported-version restriction. After
upgrading the same node, read `local_node_info.version` from its configured RPC
endpoint and update both fields to that exact string. Retain the original
history provenance only while the index, genesis and unfiltered coverage it
describes remain unchanged. A version mismatch is correctly `inconclusive`;
do not remove qualification or claim new history coverage to make it pass.
Qualifying the source does not increase the history budget. Incomplete entity
enumeration remains inconclusive; the calendar dependency does not permit
sampling records or skipping prevouts.

> **Coverage limits.** Only the **token** family has an adapter in this
> delivery. Script, Spore, cluster and object families, independent candidate
> discovery, and the hourly-bucket contract are not covered; requesting them is
> `inconclusive`, not a pass.

```bash
ckbadger verify -C work/mainnet --depth sampling --checks dao_status_index_matches_deposits
ckbadger verify -C work/mainnet --depth sampling --checks entity_capacity_history_matches_recorded_calendar
```

### Explorer Response Cache

Explorer checks cache HTTP responses in each network workdir's `.verify-cache/`, so co-resident
networks never share explorer data. Fresh cache (< 5 min) is reused. Stale cache is re-fetched; on
HTTP failure, stale data is used with a warning.

### Adding a Check

1. Choose tier: `api_checks.rs` (fast/sampling) or `explorer.rs` (explorer comparison)
2. Create struct implementing `Check` trait (name, description, tier, run)
3. Register in the module's `*_checks()` function
4. Convention: `F{N}` / `S{N}` / `X{N}` prefix in doc comment

### File Locations

| What                         | Where                                         |
| ---------------------------- | --------------------------------------------- |
| Public CLI/target resolver   | `crates/cli/src/main.rs`                      |
| Verification engine          | `crates/indexer/src/verify/mod.rs`            |
| Check trait & types          | `crates/indexer/src/verify/checks.rs`         |
| API checks (F+S)             | `crates/indexer/src/verify/api_checks.rs`     |
| Explorer checks (X)          | `crates/indexer/src/verify/explorer.rs`       |
| Chain-history oracle (S25)   | `crates/indexer/src/verify/entity_history.rs` |
| History source qualification | `crates/indexer/src/verify/source.rs`         |
| Coverage manifest            | `crates/indexer/src/verify/manifest.rs`       |
| Report rendering             | `crates/indexer/src/verify/report.rs`         |
| LCG sampler                  | `crates/indexer/src/verify/sampling.rs`       |
| Typed export endpoint        | `crates/api/src/routes/verify.rs`             |

---

## Bench

Per-endpoint baseline benchmarking. Runs each API endpoint N times and reports latency percentiles, error rates, and throughput. Used for regression detection.

### Quick Start

```bash
make bench                                         # All endpoints, report saved to test_outputs/bench/
make bench BENCH_ARGS="--module blocks"             # Only blocks endpoints
make bench BENCH_ARGS="--risk high"                 # Only high-risk endpoints
make bench-json                                     # JSON output to stdout
make bench-baseline                                 # Save baseline for regression comparison
```

### How It Works

1. **Discovery** -- probes the API to find real parameters (block numbers, tx hashes, lock hashes, token type hashes, etc.)
2. **Resolution** -- resolves each endpoint template into a concrete URL using discovered params
3. **Benchmark** -- runs each endpoint sequentially: warmup requests (discarded), then measured iterations
4. **Report** -- per-endpoint latency percentiles (p50/p95/p99), error rates, throughput

### Endpoint Classification

131 registered benchmark cases across 18 benchmark modules, each classified by:

Some REST routes appear more than once with different discovered high-volume parameters, so this
is intentionally not the 127-route HTTP inventory count in `docs/API.md`.

- **Risk Tier**: Low, Medium, High -- reflects expected query cost
- **Read Pattern**: KeyLookup, BatchLookup, PrefixScan, RangeScan, FullCfScan, CrossStore, RpcDependent, Cached, Aggregation

### CLI Reference

```
ckbadger-bench [bench] [OPTIONS]

OPTIONS:
  --api-url <URL>          API base URL [default: http://localhost:8101/api/v1]
  --frontend-url <URL>     Frontend URL [default: http://localhost:8100]
  --iterations <N>         Requests per endpoint [default: 10]
  --concurrency <N>        Concurrent requests [default: 1]
  --warmup <N>             Warmup requests, not measured [default: 2]
  --timeout-ms <MS>        Per-request timeout [default: 10000]
  --module <NAME>          Filter by module name
  --endpoint <PATH>        Filter by endpoint path template
  --risk <TIER>            Filter by risk tier: high, medium, low
  --json                   Output JSON instead of table
  --output <FILE>          Save JSON report to file
  --output-dir <DIR>       Auto-save timestamped JSON reports
  --compare <FILE>         Compare against baseline JSON file
  --discovery-only         Print discovered params and exit
```

### Regression Detection

```bash
# Save a baseline
make bench-baseline

# Later, compare against it
make bench BENCH_ARGS="--compare bench-baseline.json"
```

Thresholds: regression > 20% slower (p95), improvement > 10% faster.

### File Locations

| What                  | Where                                          |
| --------------------- | ---------------------------------------------- |
| CLI & main            | `crates/bench/src/main.rs`                     |
| Endpoint registry     | `crates/bench/src/registry.rs`                 |
| Execution engine      | `crates/bench/src/runner.rs`                   |
| Metrics (percentiles) | `crates/bench/src/metrics.rs`                  |
| Report & comparison   | `crates/bench/src/report.rs`                   |
| Discovery             | `crates/bench/src/discovery.rs`                |
| Endpoint definitions  | `crates/bench/src/endpoints/*.rs` (18 modules) |

---

## Stress

Concurrent load testing with staged ramp-up. Finds how much traffic the system can handle, what breaks first, and at what concurrency level.

### Quick Start

```bash
make stress                                                    # Mixed scenario, auto-saved to test_outputs/stress/
make stress STRESS_ARGS="--scenario api --auto-ramp"           # Pure API throughput, auto-ramp until breakage
make stress STRESS_ARGS="--scenario heavy --stages 10,50,100"  # Heavy queries, fixed stages
make stress STRESS_ARGS="--remote-host 192.168.1.100"          # Test a remote instance
```

### How It Works

1. **Discovery** -- same as bench, probes API for real parameters
2. **Resolution** -- resolves endpoints into concrete URLs
3. **Staged ramp-up** -- for each stage, spawns N virtual users (VUs) as tokio tasks
4. **VU loop** -- each VU loops: pick weighted random endpoint, execute request, send sample to collector, optional think time
5. **Collection** -- dedicated collector task aggregates samples per stage, prints real-time status line
6. **Degradation detection** -- compares each stage against baseline (stage 1), detects soft degradation and hard failure
7. **Report** -- stage summary table, endpoint breakdown, read pattern summary

### Scenarios

| Scenario  | Endpoints                   | Weight strategy                                                                                              | Think time | Frontend |
| --------- | --------------------------- | ------------------------------------------------------------------------------------------------------------ | ---------- | -------- |
| **mixed** | All API + 7 frontend routes | By page type (homepage 25%, blocks 20%, transactions 20%, addresses 15%, assets 10%, other 10%, frontend 5%) | 50-200ms   | Yes      |
| **heavy** | Only High-risk              | By ReadPattern danger (FullCfScan 4, CrossStore 4, RangeScan 3, PrefixScan 2, other 1)                       | None       | No       |
| **api**   | All API                     | Uniform                                                                                                      | None       | No       |

Scenarios can be combined: `--scenario mixed,heavy,api` runs all three sequentially.

### Ramp-Up Modes

**Fixed stages** (default): `--stages 10,25,50,100,200,300` runs each VU count for `--stage-duration` seconds.

**Auto-ramp**: `--auto-ramp` starts at 10 VUs, doubles each stage (10, 20, 40, 80, 160, 320, ...), stops when hard failure is detected.

### Degradation Detection

Based on stage 1 (baseline) metrics:

| Signal           | Threshold             | Meaning                  |
| ---------------- | --------------------- | ------------------------ |
| Soft degradation | p95 > 2x baseline p95 | Latency deteriorating    |
| Errors emerging  | error rate > 1%       | Failures appearing       |
| Hard failure     | error rate > 10%      | Service effectively down |

Auto-ramp records soft degradation but continues. Stops at hard failure.

### Report Output

Three tables per scenario:

**Stage summary** -- per-stage RPS, latency percentiles, error rate, status:

```
  Stage   VUs    Dur(s)  RPS      p50      p95      p99      Err%     Status
  -----------------------------------------------------------------------
  1       10     30      120      12ms     25ms     40ms     0.0%     baseline
  2       25     30      285      14ms     30ms     55ms     0.0%     ok
  3       100    30      780      35ms     120ms    250ms    0.2%     ⚠ soft degradation
  4       300    18      620      200ms    2500ms   5000ms   15.2%    ✖ breaking point
```

**Endpoint breakdown** -- last stable stage vs breaking stage, per endpoint:

```
  Endpoint                        Pattern         Stable p95   Break p95    Verdict
  ----------------------------------------------------------------------------------
  /addresses/{addr}/activities    RangeScan       85ms         3200ms       ✖ first to break
  /tokens/{hash}/holders          FullCfScan      120ms        4500ms       ✖ critical
  /blocks/{id}                    KeyLookup       15ms         45ms         ok
```

**Read pattern summary** -- aggregated by ReadPattern:

```
  ReadPattern     Endpoints  Avg p95 @stable  Avg p95 @break  Degradation
  -----------------------------------------------------------------------
  FullCfScan            7          90ms             3000ms       33.3×  ✖
  CrossStore            5          80ms             2200ms       27.5×  ✖
  Cached               14           5ms               12ms        2.4×
```

Reports are auto-saved as JSON to `test_outputs/stress/` when run via `make stress`.

### CLI Reference

```
ckbadger-bench stress [OPTIONS]

OPTIONS:
  --scenario <SCENARIO>        mixed, heavy, api, or comma-separated [default: mixed]
  --stages <VU_LIST>           Comma-separated VU counts per stage [default: 10,25,50,100,200,300]
  --stage-duration <SECS>      Duration per stage in seconds [default: 30]
  --auto-ramp                  Auto-increase VUs until hard failure (ignores --stages)
  --think-time-ms <RANGE>      Think time range for mixed scenario [default: 50-200]
  --remote-host <HOST>         Remote ckbadger host (expands to api-url + frontend-url)
  --api-url <URL>              API base URL [default: http://localhost:8101/api/v1]
  --frontend-url <URL>         Frontend base URL [default: http://localhost:8100]
  --timeout-ms <MS>            Per-request timeout [default: 10000]
  --warmup-duration <SECS>     Warmup before stage 1 [default: 5]
  --json                       Output JSON to stdout
  --output <FILE>              Save JSON report to file
  --output-dir <DIR>           Auto-save timestamped JSON reports
```

`--remote-host` and `--api-url`/`--frontend-url` are mutually exclusive.

### File Locations

| What                 | Where                                  |
| -------------------- | -------------------------------------- |
| Stage scheduler      | `crates/bench/src/stress/mod.rs`       |
| Scenario definitions | `crates/bench/src/stress/scenario.rs`  |
| Sample collector     | `crates/bench/src/stress/collector.rs` |
| VU loop              | `crates/bench/src/stress/vu.rs`        |
| Report output        | `crates/bench/src/stress/report.rs`    |

---

## Makefile Targets

```bash
make verify                    # ckbadger verify --depth fast
make bench                     # All endpoints, auto-save to test_outputs/bench/
make bench-json                # JSON output to stdout
make bench-baseline            # Save baseline to bench-baseline.json
make stress                    # Mixed scenario, auto-save to test_outputs/stress/
```

Reports are saved to `test_outputs/` (gitignored).
