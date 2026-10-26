# Site redesign bulk performance verification — 2026-10-26

## Goal

Validate the approved round-one indexer write changes with fresh, isolated mainnet bulk runs from genesis to block 2,000,000 and 12,000,000, including complete batches in the dense 6.65M–12M region. A critical-path regression beyond measured run noise blocks acceptance.

## Principle Alignment

- All measured stores are disposable scratch directories under `.worktrees/site-redesign-perf-results`; the user's stores and running services were not modified. The existing CKB node store was opened read-only.
- Changes retain the existing per-transaction rows and avoid new bulk VM work or script transaction indexes. Type ID tracking is restricted to native Type ID code cells. Family attribution is computed once at final/live rollup, using the same exact reference aggregation as the API.
- Immutable cell payloads remain append-only. Changed canonical metadata, usage, actions and statistics belong to the domain store. The native cycles worker is lazy and is outside the measured bulk path.
- Timings are exact sums of recorded batch measurements. No interpolation, extrapolation or sampled persistent aggregates are used.

## Method and provenance

The baseline is `2c292b73`. The pre-refactor B1 candidate contains the persisted changes through `82eb1432`, Type ID retirement/reactivation fix `0ec5a283`, shared family membership/usage helper `e121815c`, family rollup fix `4c77f5fa`, the mainnet xUDT registration correction `c1505674`, and explicit registration/lineage corrections `d791b4c5` and `e136cfcc`. Fork reconciliation, lazy cycles and read-only/frontend changes are outside this bulk benchmark. That scratch candidate is `e3d15fc068e7913f1271374ddea47684d6b5344b`; the later integrated owner refactor and diagnostic have separate provenance below. Exact revisions, local patch text and SHA-256 hashes for the later instrumented binaries are retained in `benchmark-source.json`. The first two diagnostic 2M binaries predate per-flush instrumentation; their original hashes were not recorded. Final acceptance uses hash-verified binaries.

| Instrumented binary       | SHA-256                                                            |
| ------------------------- | ------------------------------------------------------------------ |
| Baseline                  | `cbb19aa3df59992ca178ca6c825a7b0b972f4196803e11ad96089bb9d994f425` |
| Pre-refactor B1 candidate | `1892b0d516cb5c05483e15ad91ca2d0074e2dc99ea105551a996a52c9465fbe6` |

Both runs use AMD Ryzen AI 9 HX 370 (12 cores/24 threads), 98,126,496 KiB host RAM, a 32 GiB store and bulk memory budget, the same mainnet source, target RocksDB direct reads, and disabled crawler. CKB source reads retain their existing buffered-IO behavior. `hardware.json` and each run's `config.toml` preserve the environment.

Scratch-only patches cap the source tip at the desired endpoint plus the existing 1,000-block bulk threshold, and emit one exact timing record per completed flush. These patches are not merged into production. Every completed comparison run starts with fresh scratch stores and exits after final snapshot materialization at the exact requested endpoint.

The analyzer derives each batch's inclusive first block as `end_block - blocks + 1` and asserts contiguity. It joins exact flush records by batch index. The existing batch `flush_ms` field is a last-completed watch snapshot and is never summed. Initial 2M runs have only the final prepare/commit totals rounded to 0.1 seconds; later runs have exact per-flush durations. Reducer branches and flush overlap with other work, so their phase totals must not be added together to reconstruct wall time.

## Retained diagnostic and failed runs

All earlier runs remain available; failed or interrupted snapshots are excluded from accepted comparisons.

| Run                              | Outcome             | Process wall (s) | Peak RSS (KiB) | Interpretation                                                                                         |
| -------------------------------- | ------------------- | ---------------: | -------------: | ------------------------------------------------------------------------------------------------------ |
| Initial baseline 0–2M            | Complete            |          109.607 |      7,463,088 | Diagnostic baseline; no exact per-flush instrumentation                                                |
| Initial candidate 0–2M           | Complete            |          119.749 |      7,336,796 | Build/flush decreased; extra prefetch wait dominated wall increase                                     |
| Original baseline 0–12M          | Complete            |        1,305.460 |      9,533,664 | External compilation load; retained but replaced by a fresh baseline for final pairing                 |
| Initial candidate 0–12M          | Failed              |        1,016.095 |     10,386,260 | Legal ELF→non-ELF retirement rejected at block 11,993,376; owning transition fixed                     |
| Retirement-fixed candidate 0–12M | Interrupted         |          755.395 |      9,378,872 | Stopped at 9,665,755 after family usage defect was identified; superseded                              |
| Family-rollup candidate 0–12M    | Failed finalization |        1,315.648 |      9,063,524 | Reached 12M; arbitrary copied Type ID deployments incorrectly treated as permanent family registration |

The final baseline and candidate each emit the same 14 `.bit` sentinel warnings, with identical transaction hashes, output indexes and 176-byte payload sizes. Read-only RPC inspection identified zero account IDs, empty args and sentinel expiry values; these were not treated as new parser failures. `final-warning-comparison.json` and `baseline-dotbit-warning-check-all.json` retain the evidence.

The final registration correction was validated against all 471 exact native Type ID ELF deployments observed through 12M, including six deployments of the unregistered cross-family copy. These regressions run through the real bundled importer and family rollup. Each failure was fixed in its owning computation; no failed scratch store was patched or reused.

## Final measured runs

The fixed run order was candidate 12M → baseline 12M → candidate 2M → baseline 2M. Each completed successfully. A same-binary candidate 12M repeat was then added to test reproducibility; it also completed successfully.

| Run                 | Process wall (s) | CPU user + system (s) | Peak RSS (KiB) | Batches | Finalize (s) |
| ------------------- | ---------------: | --------------------: | -------------: | ------: | -----------: |
| after-0-12m         |        1,194.259 |            11,421.685 |      8,317,004 |     535 |       34.049 |
| before-0-12m-repeat |        1,146.494 |             9,689.969 |     10,045,284 |     595 |       56.424 |
| after-0-2m-repeat   |           55.635 |               676.994 |      6,068,600 |      23 |        4.886 |
| before-0-2m-repeat  |           47.950 |               743.669 |      5,770,480 |      19 |        4.158 |
| after-0-12m-repeat  |        1,163.751 |            11,723.074 |      9,327,308 |     522 |       44.031 |

All completed 12M runs process exactly 12,000,001 blocks, 27,359,903 transactions, 52,274,211 output cells and 50,417,541 inputs. Both 2M runs process exactly 2,000,001 blocks, 2,258,093 transactions, 2,552,868 output cells and 2,398,627 inputs.

Candidate wall time is 4.17% higher in the 12M pair and 16.03% higher in the short pair. Those values are retained rather than replaced by a CPU-only metric. In the short pair, candidate CPU time, build, facts, resolve, address reduction and flush are lower while prefetch wait increases from 13.312 s to 25.793 s. This is evidence that wall time alone does not identify a code cost on this host.

`address_reduce` is an existing metric name, but its timer spans the join of address/cell distribution and the other five owner reducers. It is **not** a pure address-only control. Earlier static reasoning based on unchanged address source cannot exclude cost in ScriptOwner or ObjectOwner; this was identified before acceptance and led to the controlled owner diagnostic below.

### Full-window phase totals

| Phase (s)         | Baseline 12M | Candidate 12M | Baseline 2M | Candidate 2M |
| ----------------- | -----------: | ------------: | ----------: | -----------: |
| build             |      520.001 |       570.026 |      25.152 |       20.085 |
| facts             |      184.905 |       202.430 |       7.719 |        4.966 |
| resolve           |       74.337 |        77.100 |       4.217 |        2.583 |
| reduce            |      240.163 |       268.572 |      12.191 |       11.941 |
| history           |      151.811 |       170.695 |       6.792 |        8.545 |
| address_reduce    |      188.799 |       213.334 |      10.485 |        8.096 |
| activity_stats    |       48.499 |        56.391 |       3.727 |        3.067 |
| interner_gc       |        5.018 |         4.925 |       0.060 |        0.045 |
| memory_accounting |       46.485 |        38.700 |       0.021 |        0.018 |
| prefetch_recv     |      478.170 |       506.878 |      13.312 |       25.793 |
| flush_wait        |        5.283 |         5.951 |       0.000 |        0.000 |
| prepare           |      102.293 |       126.356 |       6.935 |        5.391 |
| commit            |      122.599 |       124.068 |       5.562 |        4.153 |
| flush             |      224.893 |       250.425 |      12.497 |        9.544 |

### Exact dense comparison

The primary dense comparison covers **6,659,251–12,000,000 inclusive**, using only complete batches. All three completed 12M runs have this exact boundary. Their denominators are identical: **5,340,750 blocks, 16,811,801 transactions, 35,586,249 output cells and 34,367,393 inputs**. The newer baseline has 422 batches and the candidate 405; the adaptive batch controller remains enabled.

| Phase (s)         | Original baseline | Paired baseline | Candidate | Baseline repeat change | Candidate vs paired baseline |
| ----------------- | ----------------: | --------------: | --------: | ---------------------: | ---------------------------: |
| build             |           268.425 |         340.947 |   357.227 |                +27.02% |                       +4.77% |
| facts             |           101.404 |         127.922 |   140.469 |                +26.15% |                       +9.81% |
| resolve           |            35.392 |          49.332 |    48.801 |                +39.39% |                       -1.08% |
| reduce            |           120.303 |         149.567 |   153.823 |                +24.33% |                       +2.85% |
| history           |            58.898 |          90.456 |    90.863 |                +53.58% |                       +0.45% |
| address_reduce    |           104.293 |         119.795 |   126.992 |                +14.86% |                       +6.01% |
| activity_stats    |            25.697 |          29.858 |    31.645 |                +16.19% |                       +5.98% |
| interner_gc       |             3.100 |           3.858 |     3.669 |                +24.43% |                       -4.89% |
| memory_accounting |            37.270 |          43.337 |    37.216 |                +16.28% |                      -14.12% |
| prefetch_recv     |           473.429 |         359.229 |   385.233 |                -24.12% |                       +7.24% |
| prepare           |            50.191 |          63.683 |    75.312 |                +26.88% |                      +18.26% |
| commit            |            42.999 |          55.092 |    62.585 |                +28.13% |                      +13.60% |
| flush             |            93.190 |         118.776 |   137.897 |                +27.46% |                      +16.10% |

Flush wait is 1.620 s, 0.010 s and 5.945 s respectively; a percentage against the near-zero paired baseline is not useful. The raw complete-batch dense windows are also retained in `final-completed-analysis.json`: the original and paired baseline start at 6,651,874, while the candidate starts at 6,659,251. Those wider windows have different transaction denominators and are not substituted for the exact comparison above.

Every candidate dense phase increase is smaller than the corresponding change between the two baselines. This is an observed range, not a confidence interval or proof that the cost is exactly zero. Full-window address reduction and activity statistics also increased despite little full-window baseline change, which is why the additional candidate repeat was requested.

### Additional candidate repeat

The same-binary A–B–A repeat finishes in 1,163.751 s, 1.51% above the paired baseline, but its build cost rises to 612.682 s and CPU time to 11,723.074 s. Prefetch wait falls to 414.213 s. Thus the earlier build/CPU increase is not disproved by the smaller wall-time difference.

The fourth run does not contain the 6,659,251 batch boundary. The reproducibility comparison therefore uses **6,746,382–12,000,000 inclusive**, the earliest shared complete-batch boundary at or above 6.65M across all four runs. It covers exactly **5,253,619 blocks, 16,533,183 transactions, 35,006,066 cells and 33,820,461 inputs** in each run.

| Phase (s)         | Original baseline | Paired baseline | First candidate | Repeated candidate |
| ----------------- | ----------------: | --------------: | --------------: | -----------------: |
| build             |           263.525 |         331.536 |         350.821 |            370.943 |
| facts             |            99.987 |         122.299 |         138.653 |            142.978 |
| resolve           |            34.949 |          48.573 |          47.760 |             54.133 |
| reduce            |           117.425 |         146.767 |         150.630 |            159.059 |
| history           |            56.858 |          88.917 |          88.649 |            104.654 |
| address_reduce    |           101.801 |         117.476 |         124.630 |            124.750 |
| activity_stats    |            25.260 |          29.314 |          30.953 |             29.843 |
| interner_gc       |             3.056 |           3.795 |           3.589 |              3.939 |
| memory_accounting |            37.095 |          42.757 |          37.039 |             39.110 |
| prefetch_recv     |           473.426 |         344.284 |         385.224 |            284.435 |
| flush_wait        |             0.007 |           0.010 |           5.662 |              0.007 |
| prepare           |            49.577 |          62.675 |          73.893 |             80.773 |
| commit            |            40.048 |          54.282 |          53.043 |             82.922 |
| flush             |            89.625 |         116.958 |         126.937 |            163.696 |

The repeated candidate's full-window phase totals, retained separately from the dense interval, are:

| Phase             | Seconds |
| ----------------- | ------: |
| build             | 612.682 |
| facts             | 211.108 |
| resolve           |  89.609 |
| reduce            | 287.286 |
| history           | 198.216 |
| address_reduce    | 217.563 |
| activity_stats    |  55.454 |
| interner_gc       |   5.428 |
| memory_accounting |  41.813 |
| prefetch_recv     | 414.213 |
| flush_wait        |   2.558 |
| prepare           | 142.542 |
| commit            | 158.514 |
| flush             | 301.056 |

The raw dense complete-batch windows remain available with their own denominators:

| Run                 | First–last block     |    Blocks | Transactions |      Cells |     Inputs |
| ------------------- | -------------------- | --------: | -----------: | ---------: | ---------: |
| before-0-12m        | 6,651,874–12,000,000 | 5,348,127 |   16,835,204 | 35,637,616 | 34,415,846 |
| before-0-12m-repeat | 6,651,874–12,000,000 | 5,348,127 |   16,835,204 | 35,637,616 | 34,415,846 |
| after-0-12m         | 6,659,251–12,000,000 | 5,340,750 |   16,811,801 | 35,586,249 | 34,367,393 |
| after-0-12m-repeat  | 6,651,874–12,000,000 | 5,348,127 |   16,835,204 | 35,637,616 | 34,415,846 |

At this stage the gate remained open: phase increases recurred, and the repeated flush increase exceeded some observed baseline phase changes. Controlled owner measurements identified a narrow repeated identity-allocation cost, followed by the tested refactor described below; that finding alone does not explain the whole-run phase changes or pass the gate.

### Host load and interpretation limits

The machine was not isolated from other projects. The original baseline overlapped external compilation. The first final candidate overlapped `soulbara-world` activity; the paired baseline later overlapped two external `clippy-driver` processes. The additional candidate repeat encountered a new `soulbara` workspace compilation shortly after starting. Their process IDs, working directories, command lines, UTC checkpoints and run-start process snapshots are preserved. No external process was stopped.

The source block store uses buffered reads, so source cache state was not controlled. Batches adapt to build timing and IO overlap; different batch counts change scheduling, allocation and write grouping even for identical block/transaction denominators. Repeating a baseline is therefore necessary context for phase differences, but it does not make all changes automatically harmless. The phase totals overlap and cannot be summed into process wall time.

At 12M, peak ScriptOwner tracked memory falls from 81,973,644 to 4,307,282 bytes, consistent with excluding arbitrary typed payloads from code-lineage tracking. Total peak RSS falls from 10,045,284 to 8,317,004 KiB in the final pair. These memory improvements do not independently pass the timing gate. The candidate emits 301,694,792 streamed history rows versus 301,755,551 in the paired baseline and 10,849,440 final snapshot rows versus 10,848,669. Batch-dependent row grouping prevents interpreting those counts as a per-transaction performance gain; no new per-script transaction index is introduced.

## Controlled owner diagnosis and focused refactor

The host has heterogeneous cores: logical CPUs 0–3 (and SMT siblings 12–15) advertise a 5,158 MHz maximum, while CPUs 4–11 (and siblings 16–23) advertise 3,289 MHz. The original process-level harness had no CPU affinity. Dependency checks ruled out a different Rust build graph in the original B1 comparison: Cargo.lock, normal/build feature trees and the relevant CKB/hash/bincode/RocksDB/Rayon/hashbrown/jemalloc/Serde release archives were identical. The native VM dependency change was not present in those measured B1 binaries.

A scratch release binary compiled the baseline and candidate ScriptOwner/ObjectOwner bodies together. The fixture setup, seed application, state serialization/comparison, memory accounting and drop were outside timing; wall time and thread CPU time surrounded only `apply_tx`. CPU affinity was fixed to CPU 0, versions alternated in ABBA order, and round zero was excluded. The longer run used 200,000 transactions per case (50,001 for intentionally amplified ELF lineage), 12 measured rounds, and exact common-state equality assertions. It ran without RPC or RocksDB. Raw evidence is `owner-microbench/long-{run,measurements,summary}.json*`; binary SHA-256 is `52f9e08565380aded43793e0a3253eb93d68a29ee03ea7b19037f2e765568dc6`.

| Controlled case                                | Owner  | Baseline median CPU ns/tx | Candidate median CPU ns/tx | Difference ns/tx |
| ---------------------------------------------- | ------ | ------------------------: | -------------------------: | ---------------: |
| Ordinary typed cells                           | Script |                  1,496.09 |                   1,214.53 |          −281.56 |
| Non-ELF native Type ID                         | Script |                  1,615.46 |                   1,314.86 |          −300.60 |
| ELF native Type ID                             | Script |                  1,622.13 |                   1,999.40 |          +377.26 |
| `.bit` consume/recreate, new owner             | Object |                  1,135.50 |                   1,396.51 |          +261.01 |
| `.bit` consume/recreate, same owner/new expiry | Object |                    904.57 |                   1,077.54 |          +172.96 |
| DID consume/recreate, new owner                | Object |                    955.45 |                   1,288.31 |          +332.87 |
| BitCell consume/recreate, new owner            | Object |                    825.92 |                   1,202.87 |          +376.95 |

All 12 identity-case paired CPU differences were positive. Plain ObjectOwner's median difference was only +4.12 ns/tx, with paired differences spanning negative and positive values; its individual loop duration was shorter than 100 ms, so it is a weak negative control. The fixture repeatedly recreates one identity/reference with growing outpoint maps, not a real many-identity working set. Its original interned lock hashes were synthetic values rather than hashes derived from their matching script arguments; the follow-up triple fixture corrects this and uses 32-byte native Type ID arguments. These are deterministic reducer fixtures, not consensus-valid transactions. `did_recreate` and `bitcell_recreate` mean same-transaction consume/output transfers, not burn followed by a later mint. The 50,001 synthetic ELF transitions greatly exceed the actual 471 observed through 12M and must not be extrapolated by percentage alone.

This isolated redundant work: full `IdentityEntry` clones for the per-item action snapshot and a second encoding/insertion of the same DID/BitCell reverse-history key. Commit `92684421` (integrated as `9c52d494`) replaces the temporary clone with copied owner/expiry/context, borrows transaction-local item keys, preserves the distinct stored and resolved `.bit` owner sources, and emits the final reverse action once. It preserves full live undo records, per-item classification and ordinary Spore/`.cell` empty reverse values. A malformed stored owner now fails with item/block/tx context. Before/after regression evidence and 1,799 passing indexer tests are in `owner-refactor-*.log`; workspace check and strict indexer all-target Clippy also pass.

The absolute identity cost is fractions of a microsecond per affected transaction. It cannot by itself explain tens of seconds of build time or thousands of process CPU seconds. This finding alone did not close performance acceptance; the real-chain shadow-owner diagnostic below was required. Its additional baseline CPU/memory make overall process wall time unsuitable as production-throughput evidence.

### Integrated three-version microbenchmark

The follow-up binary uses integrated source `d9a37e7a` (`9c52d494` plus the CKBFS registry prerequisite), including the native VM dependency graph. Its SHA-256 is `489a8b6beb01ad42afb73146888ffa1d078fed5f97eeb43bdda58e7d9672346c`; the complete scratch patch has SHA-256 `47278935ab38e64346ffbb58932940ed08fcbb0fb39027c40f7bc6812a247dff`. The release profile remains LTO with one codegen unit. Source and build details are in `integrated-owner-profile/source-provenance.json`. Later D2 API/UI changes do not change the measured owner code; the only subsequent indexer-file diff is a test comment.

This binary contains baseline, pre-refactor and current ObjectOwner implementations. Their six order permutations are balanced over 12 measured rounds; ScriptOwner uses balanced two-way order. Setup, seeds, accounting, state comparison and destruction remain outside the timed loops, and CPU 0 is fixed. All common-state assertions passed, and **every pre-refactor/current emitted sealed and snapshot row matched exactly**.

The first run used 200,000 identity transactions, 800,000 negative-control transactions and 50,001 amplified ELF transitions per round. It completed in 96.970 seconds. Its negative ObjectOwner loops were only 83–89 ms, so a second run increased these to 300,000 / 1,200,000 / 75,001, completing in 164.195 seconds; negative ObjectOwner loops then had median durations of 125–180 ms. Neither run created a database or contacted RPC. Both raw runs are retained; no result was discarded to improve the comparison.

| Case / owner               | First run: current vs baseline paired CPU | Longer run: current vs baseline paired CPU | Longer run: current vs pre-refactor paired CPU |
| -------------------------- | ----------------------------------------: | -----------------------------------------: | ---------------------------------------------: |
| Plain / Script             |                                    +2.32% |                                     +2.63% |                                              — |
| Typed non-Type-ID / Script |                                   −17.14% |                                    −16.58% |                                              — |
| Non-ELF Type ID / Script   |                                   −16.15% |                                    −14.70% |                                              — |
| ELF Type ID / Script       |                                   +30.25% |                                    +34.47% |                                              — |
| Plain / Object             |                                    −6.12% |                                     −5.39% |                                         −6.69% |
| Typed non-Type-ID / Object |                                    −5.20% |                                     −4.34% |                                         −6.45% |
| Non-ELF Type ID / Object   |                                    −5.26% |                                     +0.73% |                                         −3.45% |
| `.bit` transfer / Object   |                                    −1.88% |                                     +0.15% |                                        −10.58% |
| `.bit` renew / Object      |                                    −1.17% |                                     −5.20% |                                         −8.87% |
| DID transfer / Object      |                                    +5.33% |                                     +9.74% |                                        −17.09% |
| BitCell transfer / Object  |                                    +4.23% |                                     +8.57% |                                        −19.64% |

These are medians of each round's paired percentage, not ratios of separately aggregated medians. In the longer run, DID/BitCell current-minus-baseline paired median costs were +64.97/+61.81 ns per transaction, while current-minus-pre-refactor costs were −152.02/−195.14 ns. The narrower snapshots and single row encoding remove the diagnosed redundant work; remaining per-item classification has a measurable synthetic cost. The amplified ELF case also pays for required lineage. The actual operation prevalence and combined owner cost must be measured on the real chain before interpreting those costs as a critical-path regression.

An external `soulbara` cargo process appeared during the longer run (recorded at 16:21:14 UTC). No external process was stopped; the fresh 12M start checkpoint at 16:28:05 UTC showed no cargo/rustc/clippy process. Absolute timings differ between rounds and runs, so the paired design and recorded order are essential. Raw micro measurements and summaries are `integrated-owner-profile/micro/long-triple*` and `long-triple-duration*`.

### Real-chain shadow-owner diagnostic

The diagnostic uses that same integrated binary and fresh stores. Each actual resolved batch is replayed into baseline/current ScriptOwner and ObjectOwner in alternating order on the same worker CPU, with affinity restored afterward. It records exact operation prevalence and true owner-loop wall/thread CPU; only the current indexer materializes the scratch stores. The 0–10,000 smoke completed with common-state equality and matching pair CPU affinity, but is functional validation only. The 0–12M run completed successfully in **438 batches**. Both versions processed exactly **12,000,001 blocks, 27,359,903 transactions, 52,274,211 outputs and 50,417,541 inputs**. All pairs stayed on the same CPU, and final common script/identity state matched. Its complete dense interval is **6,693,263–12,000,000**, with **5,306,738 blocks, 16,698,935 transactions, 35,355,066 outputs and 34,149,005 inputs**. This interval is shared exactly by the two in-process versions; it is not substituted for the earlier independently aligned dense windows.

| Window / owner | Baseline CPU (s) | Current CPU (s) | Signed CPU difference | Baseline wall (s) | Current wall (s) |
| -------------- | ---------------: | --------------: | --------------------: | ----------------: | ---------------: |
| Full / Script  |           82.797 |          74.940 |     −7.857 s / −9.49% |            88.954 |           80.475 |
| Full / Object  |           22.390 |          22.258 |     −0.132 s / −0.59% |            23.386 |           23.243 |
| Dense / Script |           50.918 |          45.638 |    −5.280 s / −10.37% |            54.801 |           49.162 |
| Dense / Object |           13.840 |          14.024 |     +0.184 s / +1.33% |            14.480 |           14.745 |

Per-batch signed differences remain noisy. Script has 140/438 positive CPU differences overall and 100/335 in the dense window; the corresponding Object counts are 205/438 and 164/335. Overall batch CPU deltas span −1,061.46 to +791.81 ms for Script and −277.85 to +128.73 ms for Object. Median per-batch percentage changes are −11.61% and −0.95%; dense medians are −14.00% and −0.22%. These ranges and medians describe observed batches, not confidence bounds or estimates of missing work. Every signed CPU/wall difference is retained in `owner-strata.json`.

Execution order is material: the first version can encounter colder shared input/cache state and overlap other branches differently. Alternation balances batch counts, but does not make every batch identical to its neighbor.

| Window / owner | Baseline-first batches / tx | Current CPU change, baseline first | Current-first batches / tx | Current CPU change, current first |
| -------------- | --------------------------- | ---------------------------------: | -------------------------- | --------------------------------: |
| Full / Script  | 219 / 13,876,238            |                            −23.09% | 219 / 13,483,665           |                            +6.43% |
| Full / Object  | 219 / 13,876,238            |                             −7.54% | 219 / 13,483,665           |                            +7.10% |
| Dense / Script | 167 / 8,325,007             |                            −25.90% | 168 / 8,373,928            |                            +8.11% |
| Dense / Object | 167 / 8,325,007             |                             −4.76% | 168 / 8,373,928            |                            +8.08% |

The result also differs by core class. CPUs 0–3/12–15 are the higher advertised-frequency group; this classification is not a measurement of actual clock speed.

| Window / owner | Higher-frequency group batches / CPU change | Lower-frequency group batches / CPU change |
| -------------- | ------------------------------------------- | ------------------------------------------ |
| Full / Script  | 144 / −11.24%                               | 294 / −8.82%                               |
| Full / Object  | 183 / −2.83%                                | 255 / +0.75%                               |
| Dense / Script | 117 / −14.34%                               | 218 / −8.52%                               |
| Dense / Object | 151 / −1.05%                                | 184 / +2.94%                               |

Exact observed object-protocol prevalence is:

| Protocol classification               | Full inputs | Full outputs | Dense inputs | Dense outputs |
| ------------------------------------- | ----------: | -----------: | -----------: | ------------: |
| No object protocol facts              |  49,615,880 |   51,281,608 |   33,535,122 |    34,662,578 |
| mNFT issuer                           |       6,501 |        8,491 |          653 |           719 |
| mNFT class                            |      20,847 |       26,658 |        4,921 |         5,574 |
| mNFT token                            |      78,634 |      124,625 |       36,481 |        41,242 |
| `.bit`                                |     695,679 |      832,829 |      571,828 |       644,953 |
| Cluster, Spore, DID, BitCell, `.cell` |           0 |            0 |            0 |             0 |

“No object protocol facts” includes cells belonging to other protocols; it does not mean all cells are untyped. Full typed input/output counts are 16,673,255/17,283,495. Native Type ID inputs/outputs are 1,042,679/1,042,797, including exactly **471 ELF outputs**; dense counts are 883,817/883,869 with 255 ELF outputs. The 471 count independently matches the preserved full deployment audit. DID/BitCell action cost and Spore pending classification are covered by fixtures and tests, **not by this historical 12M workload**.

For diagnostic scope only, the unpaired actual loops recorded CPU/wall seconds of address+cell distribution 54.994/60.253, token 58.595/59.997, DAO 83.721/92.217 and Fiber 22.671/23.275. They migrated between CPUs and have no baseline partner. The whole diagnostic recorded **1,043.519 process wall seconds, 8,464.356 process CPU seconds and 10,215,348 KiB peak RSS**, including an extra **105.186 seconds of baseline owner thread CPU**, additional baseline state, comparisons and instrumentation. Finalization took 38.600 seconds. This is evidence against treating the earlier approximately 2,000-second CPU increase as a stable mandatory cost, but is **not** a production throughput result. The extra baseline time is never subtracted to invent a production estimate; changed controller batches and cache state prevent that calculation.

The host remained shared. External `soulbara` cargo/rustc processes were recorded during the chain pass at 16:30:36, 16:35:49 and 16:40:07 UTC; no external process was stopped. Task agents remained quiet until the timed process fully exited and denominator/state assertions were saved.

### Remaining bulk-path audit and interpretation

The paired owners cover the changed per-cell Type ID and identity computation, but not every component of the pipeline. A source/call-path audit checked the remaining changes:

- CKB movement changes `DailyActivityStats::accumulate_from_tx_actions`, called in activity accumulation. It replaces the existing participant loop over absolute deltas with a single positive-delta loop and one final checked addition, separately for the existing hourly and daily accumulators. It introduces no allocation, extra participant traversal, persistent row or serialized field. Cellbase still returns early. Daily/hourly values intentionally change; their representation does not.
- DAO deposit/request splitting adds checked counters to the existing final `refresh_latest_dao_statistics` status-0/status-1 scan, and eight bytes to the one latest-statistics value. DaoOwner replay and per-cell transition logic are unchanged. Daily unique-depositor writing was already correct and received regression coverage.
- Imported labels, registered-reference reconciliation, family usage and executable lineage materialization run at startup/final rollup. `CoreOwners::materialize_all` invokes Script/Object sealed/final emission only during finalization. The 832,829 `.bit` action outputs add one byte to existing reverse rows there; they do not add a row per transaction to the batch history stream.
- Memory accounting retains its existing state traversal. The `.bit` reverse-key set becomes an action-valued map, so the existing per-entry accounting also includes the value; there is no new pass. The original completed runs measured ObjectOwner retained memory rising by 7,495,461 bytes, within the assessed 8 MB allowance, while ScriptOwner retained memory fell by 77,666,362 bytes. Accounting itself is outside the paired `apply_tx` timers and remains a stated limitation. The new retained `type_id_lineage` also adds an exact `bincode::serialized_size` traversal each batch (471 ELF observations through this endpoint); this is outside the build timer and is not claimed to be free.
- Facts, resolve, history construction, preparation and RocksDB commit implementations are unchanged. The CKB reader extension fix affects single-block `get_block`; bulk `get_blocks_batch` remains unchanged. The later CKBFS registry adds two known hashes with explicit existing `Plain` bulk semantics; parity tests cover it, and it was absent from the earlier slow B1 runs.
- Final rollup currently serializes lineage undo/preimages even though bulk snapshot materialization discards that undo result. Its new family-membership reference pass also clones the lookup key and family string for included references. These are real extra finalization costs, outside the paired owner loops; no speculative optimization was added without measurement. They do not directly explain recurring history/build phase differences.
- The shared media analyzer's added pending-tier selection is not paired against its old helper body, but this 12M interval contains zero Spore outputs, so it contributes no executed cost here. Lazy native VM/cycles and live fork reconciliation add no bulk work.

This audit does not prove that unchanged code has identical timing: memory layout, controller decisions, batch grouping, CPU frequency, host contention and RocksDB/cache state can change costs indirectly. In particular, the older prepare/commit increases remain recorded and were not independently paired in the diagnostic. The controlled evidence narrows the source attribution: it found and removed redundant identity work, shows no aggregate owner increase beyond observed order/core variation on the actual workload, and does not support a stable large mandatory CPU increase from the new write semantics. A strict causal end-to-end claim would require an isolated host and predeclared uniform CPU affinity/batch conditions; this report does not make that claim.

Copied baseline owners use the current shared helpers and dependencies, including Spore media analysis. The diagnostic isolates owner-body changes; it does not reproduce every behavior or cost of the old executable. The final common-state comparison covers script usage/reference info and identity entries/aggregates/owner counts, intentionally excluding new executable-lineage/action rows and Spore media differences. Separate unpaired address+cell-distribution, token, DAO and Fiber loop timings identify their actual scope; they are not paired negative controls.

## Post-timing isolated snapshot verification

After the four fixed timings completed, the final integrated CLI (`230fec15`, SHA-256 `a9d54d4d492a8d5c566883a055b3d1e47d01fe814c19045e751514aefbcf1a1d`) opened only the completed candidate scratch stores through a temporary secondary API on port 18191. The temporary API was stopped before the extra performance repeat.

The scratch config points `domain_data_path` and `append_only_data_path` at the completed `after-0-12m/data/` directories, uses the real node RPC on 8114, disables pool mirroring/crawling, and binds only `127.0.0.1:18191`. The exact verification invocation was:

```bash
/home/f0rk/projects/ckbadger/target/debug/ckbadger \
  -C /home/f0rk/projects/ckbadger/.worktrees/site-redesign-perf-results/completed-scratch-verification \
  verify --depth fast --format json --no-explorer \
  --api-url http://127.0.0.1:18191/api/v1
```

It produced the following actual results:

| Check                              | Result                           | Evidence                                                                                  |
| ---------------------------------- | -------------------------------- | ----------------------------------------------------------------------------------------- |
| `api_reachable`                    | Pass                             | Latest block 12,000,000                                                                   |
| `genesis_block`                    | Pass                             | Mainnet genesis hash `0x92b197aa1fba0f63…`                                                |
| `tip_block`                        | Pass                             | Block 12,000,000 exists with one transaction                                              |
| `deep_fork_clear`                  | Pass                             | No unresolved deep fork                                                                   |
| `dao_statistics_sane`              | Pass                             | Deposited 956,034,940,658,686,921 shannons; 31,709 active deposits; APC 2.31%             |
| `genesis_baseline_burnt_invariant` | Pass                             | 8.4B CKB on completed day 2024-01-20                                                      |
| `sync_complete`                    | Expected bounded-fixture failure | Scratch height 12,000,000, actual node height 20,840,590, lag 8,840,590; `isSyncing=true` |

The verifier exits 1 and reports overall failure because the deliberately bounded snapshot is not synced to the real node tip. That result is preserved; the node tip was not faked to make verification green. The other six checks passed individually. Full output is `completed-scratch-verification/fast-verify.json` and its corresponding verifier report.

The real native Type ID `0x5c33fc69bd72e895a63176147c6ab0bb5758d1c7a32e0914f99f9ec1bed90d41` provides a retirement/reactivation case. Its old ELF was created at 11,964,455, replaced by non-ELF data at 11,993,376, then replaced by a new ELF at 11,993,505. At the measured 12M endpoint, both explicit type-hash and automatic code-cell lookup correctly select the new ELF `0x2e1768…dbba11`; the old ELF `0x8e9ea9…c6f5b5` remains discoverable as consumed. The reference's own capacity-history endpoint retains four completed days. These are capacities of cells that reference that code hash, not the Type ID code cell's own capacity, and the chart is not used to infer current live counts.

This reference is unregistered and has no permanent family attribution. Regression fixtures cover its transient no-executable state and prove non-ELF payloads do not enter executable version lineage. The generic data-hash code-cell endpoint can still return an immutable consumed non-ELF cell, so that endpoint alone is not a version-registration assertion. Exact RPC payloads, output hashes and API results are retained in `completed-scratch-verification/retirement-reactivation-*.json`.

### Verification of the integrated diagnostic snapshot

After the timed diagnostic process exited, a preserved copy of the `9c52d494` CLI (SHA-256 `9d0aa953665649b65df9d0624268aa35bff32f43d0f16a14ff4e6a1b8b0f4254`) opened the newly completed `integrated-owner-profile/chain-0-12m` stores through secondary API port **18192**. Its config disables pool mirroring and crawler; no indexer was started. The invocation, from `/home/f0rk/projects/ckbadger`, was:

```bash
.worktrees/site-redesign-perf-results/integrated-owner-profile/verifier-cli-9c52d494 \
  -C .worktrees/site-redesign-perf-results/integrated-owner-profile/completed-scratch-verification \
  verify --depth fast --format json --no-explorer \
  --api-url http://127.0.0.1:18192/api/v1
```

Again, `api_reachable`, `genesis_block`, `tip_block`, `deep_fork_clear`, `dao_statistics_sane` and `genesis_baseline_burnt_invariant` all passed. The exact DAO totals match the earlier snapshot: 956,034,940,658,686,921 deposited shannons, 31,709 active deposits, 2.31% APC; genesis burnt remains 8.4B CKB on 2024-01-20. `sync_complete` failed as expected against real node height **20,841,165** with lag **8,841,165**, preserving verifier exit 1. The temporary API PID 2,868,775 was stopped with SIGINT and exited 130. `integrated-owner-profile/completed-scratch-verification/{provenance.json,fast-verify.json,api.log}` preserves the actual invocation, checks and lifecycle.

## Artifacts and validation

The final diagnostic evidence is under `integrated-owner-profile/`: complete `scratch-only.patch`, `source-provenance.json`, both long micro runs, host checkpoints, `chain-0-12m/{indexer.log,time.json,owner-analysis.json,owner-strata.json}`, exact denominator assertions and isolated verifier output. Both before/after owners use the same actual resolved batches; baseline owners never publish to RocksDB.

All earlier raw evidence is under `.worktrees/site-redesign-perf-results`: `benchmark-source.json`, `final-pair-provenance.json`, `final-paired-sequence.json`, `final-completed-analysis.json`, `exact-dense-three-run-comparison.json`, `exact-dense-four-run-comparison.json`, each run's `indexer.log`, `time.json`, config, performance samples and host checkpoints. Failed/interrupted runs remain separately named. No benchmark-only patch is merged.

Source `230fec15` passed formatting, workspace check, strict all-target workspace Clippy and 4,516 Rust tests (three existing opt-in tests excluded; the historical VM fixtures were explicitly run earlier). The frontend passed 1,445 tests, type-check, lint, production build and browser acceptance. The owner lane additionally reproduced the Type ID failures before fixing them and ran the 471-deployment real-chain regression. After the focused owner refactor, root `9c52d494` passed 4,518 Rust tests with the same three existing opt-in exclusions, formatting, workspace check and strict all-target Clippy. Later read/UI completion gates are recorded in the consolidated execution report.

## Result

The performance gate is accepted with the bounded conclusion **no measured, attributable critical-path regression beyond observed run/order/core variation** after the identity refactor, supported by the paired real-chain results and independent hot-path review. This is not a strict proof of identical production throughput. Earlier slower wall/CPU/prepare/commit results remain part of the record, and the diagnostic is not relabeled as a production benchmark. Additional accounting/finalization work and unpaired storage/cache/layout effects remain explicit limitations. No further uncontrolled 12M repeat was used to select a favorable outcome.

Re-sync required: **yes**, once the integrated redesign is accepted. The user performs the final re-sync; these measurements never rebuild the user's stores.
