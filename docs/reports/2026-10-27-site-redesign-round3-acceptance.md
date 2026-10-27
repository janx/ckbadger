# Site redesign round 3 — D4 acceptance

## Goal

Complete the approved Inventory, Token, Object, Collection, m-NFT and four identity
item pages. This continues the accepted D3 Address round at `cce36584` on
`integration/site-redesign-round3`. Scripts/Cells (D5), DAO/Charts (D6), network
topics (D7) and final error-page artwork (D8) remain subsequent rounds.

## Principle Alignment

- Domain pages expose their canonical cell, transaction and protocol-byte evidence.
  Physical lock ownership stays separate from protocol-named owner/manager rights.
- The planned changes are read-time API/frontend work. An independently found
  legacy Cluster parser defect was also corrected at its shared source;
  existing affected metadata may require a re-sync. No schema migration,
  backfill, purge or user-service restart is performed.
- Complete scopes and exact integer amounts precede pagination. Daily/weekly
  series declare the accepted indexed chain-time calendar and exclude unfinished
  periods; no full-header scan or statistical approximation is introduced.
- Decoder byte descriptions and cached VM trait results are separate evidence.
  The cache does not record the original decoder binary hash. Unknown executable
  dialects and incomplete decoding remain explicit.

## Result

D4 implementation and acceptance are complete at code revision `fa13050a` on
`integration/site-redesign-round3`. The scope and evidence boundaries below are
part of that acceptance.

### Behavior

- Inventory shares one canonical asset membership/composition calculation with
  summary warmup, exposes all three categories and retains complete totals. The
  summary has its own pinned anchor; list responses use an independent warmup
  snapshot and do not claim a shared cross-response view.
- Token pages join exact supply events, complete-day capacity/supply, complete-week
  transfers, whole-supply holder distributions and full activity/transaction feeds.
  Issuer and maximum-supply claims require actual script evidence.
- Items expose every indexed canonical cell version and the original transaction
  data/scripts, with protocol field ranges and retained refresh errors. m-NFT
  configuration uses the protocol's eight prohibition bits; zero permits an
  operation. State bit 0 means claimed and bit 1 means locked.
- Collection genetics keep same-name multi-valued traits, exact whole-collection
  frequencies and full-scope filters. DOB byte maps use verified executable
  dialects, including the real Nervape3X binary executed in regression tests.
- Identity evidence distinguishes current and historical documents/records,
  witness commitments, account expiry and physical ownership. DID CBOR retains
  non-map values, tags, byte strings and integers outside JavaScript's safe range.

### Real mainnet evidence

The isolated APIs use separate RocksDB secondary directories. The user's services on
8100, 8101 and 8114 were not restarted, and their primary stores were not written.
Raw-cell comparisons use bounded read-only node RPC calls on 8114; product
endpoints read the configured local node store.

- At `933902e3`, **66 asset checks passed**, including all **35 historical cells**
  of six selected items: Spore 2, m-NFT 3, .bit 11, .bit Cell 2, DID 2 and .cell 15.
  Exactly 35 transaction and 35 header RPCs independently checked original bytes,
  scripts, occupied capacity, creation headers and consumption inputs. Semantic
  protocol decoding is additionally covered by owning parser regressions.
- The legacy Nervape collection closes at 2,777 live objects and 2,611 holders
  across gallery, holder distribution and genetics partitions. Complete weekly
  buckets are contiguous. The DID PDS grouping closes at 114 live identities.
- Four initial probe errors are preserved: an explicit cache-warmup 503 and three
  requests with an invalid DID collection alias. Waiting for cache readiness and
  using the existing `did_ckb` alias made those four checks pass. No product
  fallback or API alias was added to make the probe pass.
- These are selected-entity checks, not another exhaustive chain verification.
  The subsequent focused probe at `7ee0dd43` passed **13 checks** with 37 API
  requests (including readiness polling) and only five node RPCs. It checks both
  legacy and 3X Cluster raw cells, current ownership, seven complete-day chart
  scopes, future empty ranges and wrong-namespace rejection. Created/live fields
  are checked as distinct, with total at least live; this focused probe does not
  independently enumerate every historical mint to prove the created total.
- The 3X TypeID decoder now resolves through its actual canonical live cell,
  independently confirmed by the node's live-cell response and original ELF
  data hash. A metadata-only decoder need not appear in the script-usage mapping;
  the decoder lookup uses one canonical cell path, not a mapping fallback.
  Its real `98b7af5e…` executable now supplies the six mapped genes.
- Both selected mainnet Cluster cells are three-field: legacy Nervape has 5,135
  data bytes and 5,104 description bytes; Nervape 3x has 431 and 397 respectively.
  Their raw node bytes, protocol decode and indexed metadata agree exactly.

### Separately found parser defect and re-sync boundary

The shared parser previously read a two-field Cluster name length as a third
field offset and lost the description. The owning parser is corrected, with
live/bulk entry-point and raw-API RED/GREEN regressions. No API repair, new writer
path or append-only mutation is added. See the
[parser correction report](2026-10-27-cluster-v1-parser-correction.md) for the
official schemas, tests and narrowly scoped parser timing observation.

Four read-only historical usage point lookups at indexed mainnet block
20,848,805 found no rows for the registered v1 reference `0x598d793d…c3b856080`
under data/type/data1/data2. This establishes absence of that exact reference's
indexed history, rather than inferring it from zero live cells. It does not
establish absence of every possible copied deployment or effects on another
network. Existing affected two-field metadata and dependent cached DOB results
must be rebuilt by replaying with the fixed indexer; this task leaves them intact.

### Token read performance

At baseline `dc28f385` and candidate `dff9c3e9`, the same real USDI scope contains 18,646
transfer rows, 18,278 unique transactions, 720 holders, 13 supply events and 676
complete days. The four feed response bodies are byte-for-byte equivalent after
JSON normalization. Canonical transaction joins now use bounded multi-get batches
and reuse loaded transactions rather than point-reading each join repeatedly.

| Request             |    Before |    After |
| ------------------- | --------: | -------: |
| History             |  7.0512 s | 5.7728 s |
| Activities page 1   | 10.5912 s | 4.6184 s |
| Activities page 2   | 10.0338 s | 4.5664 s |
| Transactions page 1 |  9.2283 s | 4.6877 s |
| Transactions page 2 |  9.5651 s | 6.2584 s |

This is a before/after observation, not a latency guarantee. One transaction
request still emitted a read-pin wait warning. Inventory times varied from about
0.43–0.47 s to 1.19–1.22 s with unchanged code; cache/environment variance is not
credited to the batching change. No new cache or persistent index was introduced.

### Acceptance-discovered regressions

Integrated frontend testing exposed a shared tick-rounding defect: tiny nonzero
steps were treated as zero, and removing that error exposed the old 12-decimal
precision limit. The shared tick path now handles token decimals through 255
without `toFixed`'s 100-digit limit. Small axis labels use scientific notation;
amounts and keyboard readouts retain their exact decimal strings.

The real legacy Nervape mobile page also exposed a long BTCFS trait value whose
flex item could not shrink. The owning value element now allows shrinkage and
wraps the full value while retaining its adjacent exact count. The initial
390-pixel page's 694-pixel scroll width is retained as the browser regression
before the final replay.

Real collection inspection found that the generic Spore collection projection
hardcoded activities to zero although the canonical feed and existing Spore
endpoint both reported 5,137. The generic detail now uses the shared canonical count. Source discovery must remain cheap for callers that do
not request a count. This existing count cache has a 300-second TTL and is not
bound to the current feed's tip; equality in the stationary acceptance scope is
not a strict cross-response snapshot guarantee. The same source-resolution
review found a legitimate newly created, empty Cluster returning 404 through the
new primary detail request. The shared existence resolver now serves valid empty
metadata-only and Sole collections, returns 404 for unknown IDs, and reports a
contextual error for collections with members but no aggregate. Four added
regressions cover all seven generic collection routes, canonical transaction
counts, duplicate actions and orphan branch rows. The owner reran 807 affected
API tests successfully; 28 freshly captured producer JSON records are unchanged.

### Integration and browser evidence

At `599e4e61`, formatting, workspace check, strict all-target Clippy and the
full workspace tests passed: **4,681 tests across 73 suites**, zero failures.
Three existing opt-in native-VM tests were not run by the default command. The
new real DOB decoder regressions did execute. After the collection-only API
correction, `fa13050a` passed formatting, workspace check, strict all-target
Clippy and the complete API package: **1,316 tests across 34 suites**, no failures
or ignored tests. Other Rust implementation crates are unchanged from the full
workspace run; this targeted rerun does not claim a second full-workspace run.

At `c89ab16c`, frontend type checking, lint, **1,585 tests in 192 files** and the
production build passed. The frontend source tree is unchanged in the combined
`fa13050a` revision. At that combined revision, **96 fixture browser states**
passed at 1440/390 pixels, plus independent trait filter/reset, holder toggle and
DNA keyboard-focus checks. All **29 consumed producer fixtures** match their
captured API JSON. The examples use native seeded API fixtures and explicitly
synthetic presentation fixtures; they are not live-mainnet page proof. Capacity
calendar fixtures and collection membership fixtures exercise separate seeded
scopes and do not assert a shared chain anchor.

The final production-SPA replay at `fa13050a` passed **26 real mainnet page/width
combinations** (13 routes at 1440/390), with 160 API responses and no HTTP errors,
uncaught page errors, duplicate React keys, document overflow or clipped table,
definition-list or raw-text values. The collection's mobile document width is
now exactly 390 pixels; a separate screenshot confirms the full BTCFS identifier
wraps beside its count. The initial 694-pixel failure remains in the diagnostic
and failure logs. An earlier independent review of six representative desktop/mobile viewport
screenshots exposed the activity-count discrepancy; the corrected value was then
checked by the final five-surface probe and production page replay.

The SPA used its production build on task port 3147 and routed HTTP entity
requests to the isolated API on 18104. Runtime configuration selected mainnet;
WebSocket traffic was stubbed because this replay covers the changed HTTP entity
views, not the previously accepted realtime feed or supervisor startup. The API
used the final frozen binary for `fa13050a`, SHA-256
`cb6c2ed1a535bb9e35b3a42d6f2d63cce4d134296b8ab7e81a2d04666b233026`.

A five-request final mainnet probe confirmed the legacy collection's generic
detail, native detail, Inventory row, activity feed and transaction feed all
report **5,137 activities**; generic/native live counts both equal **2,777**.
These observations retain the cache/snapshot limits described above. All task
APIs/previews were stopped, ports 3145/3146/3147/18104 were confirmed closed,
and the two temporary root preview source files were removed. User services and
primary stores were not replaced, restarted, purged or rebuilt.

`d4-accepted-manifest.json` records the final evidence file hashes and production
index hash. This round does not claim another full 64-check chain verification;
that earlier accepted run remains separately recorded in `SITE_REDESIGN_EXECUTION.md`.

Ignored logs, fixed binaries, producer responses, screenshots and test metadata
are retained under `.worktrees/site-redesign-round3-results/`; lane reports live
under `docs/implementation/2026-10-27-site-redesign-round3-*.md`.

- Re-sync required: **no** for the D4 read-time changes; **yes** for existing
  metadata affected by the separate two-field parser defect. The bounded mainnet
  audit found no use of the registered affected v1 reference.
- Next: proceed to D5 Scripts/Cells using the accepted remaining-gap audit.
  D6 DAO/Charts, D7 network topics and D8 final page polish remain later rounds.
