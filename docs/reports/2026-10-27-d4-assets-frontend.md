# D4 asset frontend implementation

## Goal

Implement the approved Inventory, Token, Object, Collection, m-NFT and four identity boards, including their capacity state, complete history, two activity layers and raw protocol evidence.

## Principle Alignment

- CKB chain evidence remains authoritative. Capacities and token amounts retain exact integer precision; missing metadata, pending decoding and failed reads remain explicit.
- The frontend consumes canonical read projections. It does not write stores, infer missing owners, reconstruct protocol bytes through fallback requests, or replace an incomplete aggregate with page-local sums.
- Human-facing identity and capacity are tier one. Raw cell bytes, script references and decoder rules follow the 72px evidence boundary. Entity titles use 26px, section labels 15px and metadata 11px.
- A collection's current cluster cell is proven by its canonical live type index response. Cluster-cell capacity is not added to member-cell capacity. Protocol-named identity owners/managers are distinct from physical lock owners.

## Result

- Inventory has separate token/object/identity tables, exact canonical summary scope, explicit independently refreshed cached-list semantics and cursor pagination.
- Token detail shows exact supply/capacity, issuer proof when available, same-symbol collisions, balance distribution, same-response holder denominators, complete-day history, complete-week transfers and exact supply events.
- Object detail shows media dependency tier, link-only Bitcoin media, DNA/current decoder mapping, independently cached VM traits, complete collection variant counts, canonical cell versions and whole-life Activities / Transactions.
- Collection detail has exact created/live/burned counts, member capacity, a shared capacity/weekly-transfer axis and range controls, complete allele frequencies, server-filtered gallery pagination, current cluster cell evidence and holders.
- m-NFT class and item views expose actual configured permissions, separate claimed/locked state, issuer/class/token identity bytes and link-only renderer declarations. The m-NFT item slice was implemented by the root agent.
- .bit, .bit Cell, did:ckb and .cell show canonical per-version capacity and raw byte annotations alongside friendly protocol fields. DID lossless CBOR values are preserved; current linked .bit account evidence is explicitly current rather than a historical snapshot. .cell named rights, binary records, TTLs, sale state, ring links and full sub-name pagination remain visible.
- All new views preserve previously fetched data with a visible refresh error. Unknown cycles are not displayed as zero. Item activity URL cursors and navigation resets remain covered.
- The former collection-only Holders tab is now an always-visible holders section beside independent Activities / Transactions. Former page-local Spore content/search filtering is replaced by whole-collection server trait filtering; it no longer implies a complete search based on one page.

### Evidence and limitations

- Fixtures in `frontend/__tests__/msw/fixtures/item-producer.ts` are unchanged JSON values captured from real API handlers using isolated native CKB fixtures. They are not live mainnet evidence.
- Cluster detail/current-cell, full member gallery and object/genetics captures share the eight-member fixture. The three capacity-history captures are separate calendar fixtures; their balances must not be represented as the same read scope as that collection.
- Browser preview code and screenshots are task-only artifacts under `.worktrees/site-redesign-round3-results/`. The root agent performs the independent 1440px/390px final replay and live API acceptance. Preview scenarios do not prove live-store completeness.
- Stored token metadata provenance is not persisted. The UI explicitly says it is unavailable. A cached DOB VM result does not record the original decoder binary hash; current decoder rules and cached expressions are separately labeled. No new persistence was introduced to manufacture this evidence.
- Incomplete collection decoding exposes coverage but no partial whole-collection trait frequencies. Unsupported current decoder dialects retain DNA/raw rules and explicit unavailable mapping.
- Re-sync required: **no**. No store/schema/indexer write path is changed by this frontend lane.

### Validation

- Existing D4 baseline: 111 tests passed before page refactors.
- Inventory/Token slice: full frontend suite 1,506 tests passed before subsequent item slices.
- Item/collection/page/API targeted combination: 161 tests passed.
- Regression-first evidence: non-map/lossless DID CBOR, section type scale, shared cell-version heading, same-name multiple trait values and occupied/free semantic colors.
- Final lane type-check, ESLint and production SPA/agent-renderer builds passed. The full frontend suite passed 1,566 tests across 192 files with four workers; the initial default-worker run exposed two asynchronous test waits, which were corrected without changing product behavior.
- Immutable browser replay is recorded separately by the root agent after integration. Logs live under `.worktrees/site-redesign-round3-results/`; no live-pass count is inferred from fixtures.

Next: integrate final read-only API commits, replay the immutable combined frontend against final producer captures, and continue D5–D8 under the original plan.
