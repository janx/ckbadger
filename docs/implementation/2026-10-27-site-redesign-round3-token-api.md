# D4 Token, Inventory, and shared asset feeds

## Goal

- Complete D4 Token and Inventory read contracts and provide one shared canonical asset feed projection for the object/identity lane.

## Principle Alignment

- API remains read-only. No schema, domain/append-only writes, backfill, or node RPC fallback.
- Asset feeds receive canonical transaction hashes from each asset's existing history index. One shared helper validates canonical locations, deduplicates transactions, sorts by real block/transaction index, and invokes the existing activity/transaction response builders. Full participants and stored cycle status survive unchanged. Transfer sequence is never a transaction index.
- Token history streams the complete token transfer prefix and joins only its referenced canonical block headers. Persisted running-max chain time owns UTC+8 dates; quiet dates carry state, complete-day/complete-week series exclude unfinished buckets. Existing daily capacity deltas own capacity history. No full header scan or sampled estimates.
- Positive daily supply requires positive capacity and occupied capacity, including validation of the unfinished anchor day before excluding it from the chart. The writer deletes zero-net daily deltas, so same-day mint/burn with no remaining supply legitimately has an absent capacity row.
- Supply changes use exact mint/burn transfer amounts and checked 256-bit accumulation; current supply must agree with the canonical holder aggregate. Distribution scans all positive holder balances and returns exact raw boundaries/counts/amounts, including unknown-decimal units.
- Token live cell count scans the existing type index, validating each index key, live marker and immutable payload. Index corruption is an error, not a skipped row. Only detail and rendered token list rows need this read; no new warmup writer.
- Inventory membership/classification has one owning `AssetCatalog` projection shared by warmup and the pinned summary request. The summary reads canonical token metadata, only enough holder rows to establish positive membership, collection aggregates, and streamed Spore pending evidence; it avoids full holder sums/history hydration. The background warmup pins only this projection, then releases it before its longer existing work. The list still uses its independent cached snapshot, without inventing an anchor or refresh timestamp.
- Pending items include the writer-normalized Sole Spores sentinel collection. Pending is a subset of aggregate unknown; impossible tier sums and pending counts fail with collection context. Composition summary rows count entire collections classified by tier; pending item count is a separate exact item metric.
- Arbitrary compatible scripts do not prove issuer rights or an unlimited supply. Plain xUDT requires absent/zero flags; nonzero owner-mode bits do not identify an issuer lock. Known sUDT/plain xUDT authority follows the first 32 argument bytes and the upstream RFCs linked in `docs/API.md`.
- `TokenInfo` does not persist per-field label/on-chain provenance. Stored provenance remains explicitly unavailable; matching values against bundled labels would not establish provenance. Reliable provenance would require a separate owning writer/schema task.
- Ranked holder pages expose their own same-view total supply and anchor, so a frontend percentage does not combine independently refreshed requests. Symbol collisions return other exact type hashes and holder counts, never an assertion of identity.

## Validation

- Baseline `cargo test -p ckbadger-api --test api_tokens --test api_assets --offline --locked -j2` passed; raw log `.lane-results/d4-baseline.log`.
- RED evidence: new feed/history/distribution routes returned 404 (`d4-token-red.log`); live cell count was null (`d4-live-count-red.log`); compatible scripts inherited unsupported unlimited semantics (`d4-compatible-cap-red.log`); xUDT owner-mode flags passed the plain-owner test (`d4-xudt-flags-red.log`); Inventory summary returned 404 (`d4-inventory-red.log`); overlapping composition counts incorrectly returned 200 (`d4-tier-overlap-red.log`); missing capacity evidence incorrectly rendered zero capacity alongside positive supply (`d4-capacity-evidence-red.log`).
- Producer regressions use the real router, canonical index rows and immutable cell payloads. Exported `producer/token-producer.json` and `producer/inventory-producer.json` are consumed by the frontend contract tests. The first producer gate passed 10 Token and 4 Inventory tests (`d4-producer-green.log`); final expanded counts follow below.
- Two initial full-unit attempts exposed only lane environment issues: sandbox loopback socket denial, then missing ignored embedded `frontend/server-dist` artifacts. Authorized loopback tests and copied root build artifacts correct the environment; no assertions were weakened. Raw failed logs are retained.
- Full API crate: 1,261 tests passed, zero failed/ignored (`d4-api-full-green.log`). This precedes the final positive-supply/capacity invariant; the complete affected suites were rerun for that addition: assets 62, Inventory 7, Token redesign 13, tokens 21; all 103 passed (`d4-final-affected-green.log`).
- Bounded cost fixture: real summary route over 8,192 Spore rows and one collection, five debug runs 127.112 / 109.977 / 115.433 / 107.797 / 105.618 ms (median 109.977 ms), `d4-catalog-fixture-cost.log`. This measures the new read projection, not a mainnet performance guarantee. The pre-refactor token/assets tests passed (21/62); they provide semantic parity, not a performance baseline for a formerly nonexistent summary endpoint.
- Catalog time is O(token metadata + unlabelled-token holder rows up to first positive balance + Spore rows + collection aggregates), with one decoded Spore payload at a time and retained token/collection metadata. History/feed time is O(the token transfer prefix), distribution O(the holder prefix), and rendered live count O(the token cell prefix). No frontend-page sampling or whole-header scan.
- All raw lane evidence lives under `.worktrees/site-redesign-round3-token/.lane-results/` (not committed). Normal pre-commit hooks passed: workspace all-target strict Clippy, Rust formatting, and Prettier (`d4-commit-hooks.log`).

## Result

- Re-sync required: no.
- The root agent will integrate the shared feed with object/identity endpoints, then run isolated read-only mainnet checks and the combined frontend/Rust gates. Current stored metadata provenance remains unavailable; list and summary snapshots are explicitly independent.
- Lane source and affected gates are complete. No user services or primary chain databases were changed; tests used only isolated workspace fixtures.
