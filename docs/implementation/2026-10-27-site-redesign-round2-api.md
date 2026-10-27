# Round 2 API: D3 Address v3

## Goal

Expose exact current address holdings and six-class live-cell totals required by
Address v3 (gaps 7/8), and normalize address transaction execution status.

## Principle Alignment

- Read-time only: enumerate the existing owner live-cell index within the HTTP
  request's pinned view. No schema, indexer, domain, append-only, or network writes.
- The same classifier supplies whole-owner totals and filtered cell pages. Use
  the shared protocol registry and transaction cell display decoder, including
  actual node RocksDB bytes where required. No private protocol hash list or RPC
  fallback. Missing index targets/payload/metadata and contradictory counts fail
  with owner/outpoint context.
- Scan every owner row for totals; page filtering happens before the limit.
  There is no arbitrary scan ceiling or inferred total from the current page.
- Capacity is decimal shannons. The partition is exact occupied capacity + DAO
  free capacity + other free capacity. The DAO component includes both live
  deposit and withdrawal-request cells, whose counts are also separate.
- Holdings use appropriate units: DAO cells, distinct positive-balance token type instances,
  object items, identity items. Objects include Spore/mNFT; identities include
  .bit, .bit Cell, did:ckb, and physical .cell cells. Collection infrastructure
  is an ordinary script cell, not an object item.
- Protocol-named .cell owner/manager rights remain the existing dotcell-names
  endpoint/UI section. They are distinct from physical lock ownership and add
  neither cells nor capacity to this partition.
- Current mNFT collection metadata may describe current holdings; historical
  transaction tiles retain their existing immutable-evidence restriction.

## Implemented contract

- `GET /addresses/{addr}/holdings`: exact six-class totals and distinct holdings,
  object composition and per-collection counts, balance/occupied/free/DAO free,
  separate DAO phase counts. Composition counters preserve all six stored tiers.
- `GET /addresses/{addr}/cells?class=...&limit=...&cursor=...`: existing cell index
  order, optional class (`dao`, `token`, `object`, `identity`, `script`, `plain`),
  exact occupied capacity, shared display descriptor, identity expiry, and the
  current outpoint creation height. Identity `heldSinceBlock` follows exact
  canonical consume/recreate links: self-updates extend ownership, transfers
  and disconnected burn/remint lifecycles reset it. Cursor is owner- and class-bound.
- Existing token balances supply their paged holdings rows; zero-amount token
  cells still contribute class capacity/counts but not a positive-balance kind.
  Current DAO rows use the new class page, including withdrawal requests received
  from another depositor. Original-depositor lifetime DAO lists are not holdings.
  Unclaimed compensation uses the existing canonical DAO helper and the same
  pinned tip. Existing
  transactions/activities supply history; Address v3 adds no daily capacity chart.
- Address transactions reuse committed transaction cycle normalization, exposing
  nullable cycles plus explicit complete/pending/failed status.

## Result

Re-sync required: **no**. No persistent write path changed in any logical store.
Both endpoints are uncached and use the existing request pin. The summary streams
O(owner live cells), retaining distinct item/token keys and collection counters,
not cell payloads/descriptors. Pages retain at most limit+1 cells; only emitted
descriptors read raw node bytes. Identity emitted rows additionally read their
item-specific indexed canonical history. Clusterless Spore sentinel grouping is
exposed as no on-chain collection, never looked up as fabricated cluster metadata.

Validation (all paths relative to this lane's `.lane-results/`):

- Existing endpoint baseline: `d3-baseline-locked.log`, 35 cells + 5 transaction-cell tests passed.
- Initial RED: `d3-red.log`, seven failures (missing routes and cycles status).
- Zero-balance kind RED: `d3-zero-red.log`, incorrectly returned 2 kinds instead of 1.
- Clusterless Spore RED: `d3-sole-spore-red.log`, incorrectly required a real collection for the sentinel.
- Final affected suites: `d3-relevant-final.log`, **83 passed** (18 address holdings,
  35 cells, 25 DAO, 5 transaction-cell tests; includes shared test helpers).
- Command: `TMPDIR=/home/f0rk/projects/ckbadger/.worktrees/t CARGO_BUILD_JOBS=2
cargo test --offline --locked -p ckbadger-api --test api_address_holdings
--test api_cells --test api_transaction_cells --test api_dao`.
- `d3-green-expanded.log`: 2,059-cell sparse-owner summary took **44.73 ms**
  in a debug fixture, with an empty node data fixture. This is a bounded fixture
  measurement, not a mainnet performance claim. No before endpoint existed.
- Actual router JSON exports `address-cells.json` and `address-holdings.json`
  supplied to the frontend agent for producer/consumer contract fixtures.

Formatting and strict Clippy run through normal commit hooks. Root owns integrated
workspace gates, final UI/board acceptance, and optional read-only real-address
verification. A separate lane debug CLI build will support that verification.

Build note: worktrees do not receive ignored Cargo.lock automatically. The first
offline attempt resolved newer cached dependencies and was stopped; copying the
root lockfile restored the exact integrated dependency graph and baseline passed.
