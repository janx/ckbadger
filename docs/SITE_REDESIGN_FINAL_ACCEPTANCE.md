# Site redesign — final integration acceptance

## Goal

Finish the automated integration work after D1–D8: run the full mainnet verifier,
exercise the complete site through its shipped Rust frontend proxy, fix actual
cross-page regressions, and prepare an accepted release artifact. Three
`gpt-6-sol` lanes own implementation and review; root integrates and verifies.

## Principle Alignment

- Start from `docs/prompts/WORLD_VIEW.md` and `INFORMATION_DESIGN.md`.
- Preserve the accepted layered entity-history check: exact selected histories
  and totals, with the indexed monotone calendar declared as a dependency.
  Do not replace this with a full-header scan or reduce the requested scope.
- Use the real frontend HTTP/WebSocket proxy and runtime configuration for the
  integration browser run. Expected HTTP 404 responses are distinct from
  JavaScript exceptions. Deliberate accessible ellipses and screen-reader-only
  chart captions are distinct from visible text accidentally clipped.
- Fix response contracts at their owner; do not hide malformed responses with
  empty-array defaults or fake pagination.
- No schema change, primary-store purge or new chain replay is part of these
  read-time/frontend corrections.

## Result

Full mainnet verification passed **64/64**, with `scopeComplete=true`, in
858.6 seconds. Run ID: `20261028T035358Z-c207e907`; the report is
`work/mainnet/perf/verify/20261028T035358Z-c207e907/report.json`.
The final implementation is
`b0c6bbabc509083974f63663f8b4a48d8551451f`; its frozen release executable has
SHA-256 `057b7bac7f29111743a741aa20636f02ada541d778d31ee8ad7c86bc39219759`.

### Integrated gates

- Rust formatting, workspace check and strict all-target Clippy passed.
- The full Rust workspace passed **4,774 tests in 75 suites**, zero failures.
  Three existing opt-in native-VM tests remain unrun; no new test was ignored.
- Frontend type checking, lint, production build and **1,725 tests in 218 files**
  passed.
- All **27** exported backend producer fixtures match their frontend consumers,
  including the newly separated cumulative DAO depositor chart.
- The final release passed **78/78 browser checks**: 32 routes at both 1440 and
  390 px, two intermediate-width legend checks (768/1024), ten redirects and two
  navigation interactions. Empty and nonempty Fiber addresses, D4/D5 details,
  exact typed-404 behavior, visible text, page overflow, footer count and browser
  exceptions were checked. A separate real `/ws/mainnet` connection received
  three frames, without interception.

The final S25 check passed all eight selected entities at anchor **20,855,366**
in **400.571 seconds**, processing **1,379,946 history records** and **334,088
RPC requests**. The budgets remain 600 seconds, 2,000,000 records and 1,000,000
RPC requests. Every entity is complete with zero differences, including the
final anchor recheck. Its calendar remains explicitly `indexed_chain_time` with
`calendarIndependentlyVerified=false`; the amount proof is not an independent
proof of every historical date bucket. X26 passed all 30 compared days with its
unchanged external-comparison tolerance.

The first run immediately after API startup recorded three documented
`warmup_pending` 503s for asset lists (75/78). It is retained as
`browser-sol/smoke-cold-start-results.json`. The API log confirms cache warming;
after the three actual asset endpoints became ready, the unchanged strict
browser assertions passed 78/78. The task-owned readiness probe now includes
those endpoints, not only `/blocks`.

### Integration corrections

The first full-site pass found an address-page crash that isolated page mocks
had hidden. `GET /addresses/{lockHash}/fiber/channels` returns a JSON array;
the frontend typed it as a cursor page and read `.data.length`. The API client
and page now consume the actual array, remove unsupported cursor controls,
request at most 100 entries and state that bound. Empty-array and producer-backed
nonempty-array regressions reproduce the old crash and verify the corrected
channel links. A focused audit found the five other address subresource response
shapes consistent with their owning Rust endpoints.

The initial browser pass also revealed mobile pipeline legend clipping. Its
shared footer had a fixed single-line height and hidden overflow. The correction
lets it wrap at every width. DOM measurements with the bundled font confirmed
all ten labels visible at 390, 768, 1024 and 1440 px.

The full verifier exposed a separate DAO metric mismatch: X26 compared the
Explorer's cumulative depositor series with the redesigned chart's current
depositors. A distinct read-only `/dao/charts/ever-depositors` endpoint exposes
the existing indexed `cumulative_depositors` through the shared completed-day
calendar. X26 reads that series; the user-facing total-deposit chart retains its
current-depositor line. The existing 20% external-comparison bound is unchanged
(Explorer re-counts returning depositors; see POSTMORTEM DAO-020). This external
comparison is not an exact-equality proof.

The public testing guide now describes the implemented verification selection,
sampling and single-network endpoint override options. Machine-readable site
documentation covers Scripts/Cells, DAO/Charts, Forks/Hardforks, typed search and
the bounded vessel sample. The latter explicitly distinguishes six transactions
per block from four outputs per transaction.

### Runtime provenance and limits

The primary instance initially published
`0.8.2+integration/site-redesign-round1@d3b770f5f892`. The debug preflight used
the frozen D8 executable at `15d92d55`; final acceptance used the frozen release
at `b0c6bbab`, with a task-owned API on 18109 and the actual Rust frontend service
on 3152. Its only configured network was mainnet, matching `work/ckbadger.toml`;
no live testnet acceptance is claimed. Chain stores were opened secondary. The
frontend used its real API/WebSocket proxy, not a Playwright route replacement.

The debug preflight (`20261028T025550Z-aad91cff`) completed in 1,191.6 seconds:
62 passed, X26 failed on the metric mismatch, and S25 was inconclusive on its
600-second wall-time budget. Its failed evidence is preserved separately under
`preflight-debug-15d92d55/`. S25 selected the same eight entities and used the
same verifier source as the prior passing release run. That release processed
1,379,939 records in 194.6 seconds, while this debug run processed 1,043,464
in 626.0 seconds and could not finish its final anchor check. The release/debug
comparison motivates a release re-run with unchanged scope and budgets; it does
not establish a data defect or justify removing the anchor check.

The initial full-scope command is:

```sh
ckbadger -C work/mainnet verify --depth sampling --format json \
  --api-url http://127.0.0.1:18109/api/v1
```

No checks, Explorer comparisons, entity scopes or budgets are reduced. Evidence
is retained in `.worktrees/site-redesign-final-results/`, including
`final-verify-run.json`, `final-verify-stdout.json`, `accepted-build.json`, both
`integrated-*-gates.json` files, runtime logs and `browser-sol/`.

After all 64 checks passed, the task-owned API/frontend were stopped and the
existing local instance was updated to the identical accepted release. The old
supervisor received SIGINT; its four children exited and its PID/socket files
were removed before binary replacement. The old executable and SHA were saved
in `primary-before-update-ckbadger` and `primary-update.json`. The new supervisor
uses the original `run -C work` configuration; both configuration file hashes
are unchanged. No store was deleted or rebuilt.

The served `/runtime-config.js` now reports
`0.8.2+integration/site-redesign-final@b0c6bbabc509` on
`http://127.0.0.1:8100`. All four services are running; the indexer resumed live
sync with zero blocks behind. Asset, address and script cache readiness checks
passed, and the updated instance passed **7/7 fast integrity checks**. Startup
and service lifecycle evidence is in `primary-update.json`,
`primary-fast-stdout.json` and
`work/run/logs/supervisor-site-redesign-final.log`. The old executable is a
binary rollback artifact, not a rollback of subsequent indexed chain state.

A separate post-update browser smoke passed the actual primary Home (390 px),
Fiber-linked Address (390 px) and DAO (1440 px). All three served the expected
build and loaded their JS/CSS assets without browser/API errors or overflow;
Home received six frames from `ws://127.0.0.1:8100/ws/mainnet`. These three checks
are recorded in `browser-sol/primary-smoke.json`, separately from the 78-check
acceptance run. Temporary browser processes were closed.

Re-sync required: **no for this round's corrections**. The separately documented
D4 two-field Cluster parser correction has its own narrowly scoped replay
boundary for affected metadata/cache entries. Earlier mainnet usage evidence
found no history for the exact registered affected v1 reference; it does not
prove every copied deployment or another network unaffected. This final check
must not be represented as a new genesis replay or as the user's visual sign-off.
Next: the user's whole-site visual review on the updated local instance.
