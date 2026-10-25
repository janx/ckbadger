# DAO refresh and snapshot performance acceptance

The exact DAO lifecycle values remained identical on a fixed, nonempty
synthetic dataset. A refresh regression in the data-exactness candidate was
traced to reading the entire DAO deposit table twice: once for tip compensation
and again for the 24-hour historical baseline. The optimization computes both
observations during one scan, using the existing exact per-deposit compensation
function for each observation. It does not change persisted data or the
completed-day snapshot path.

## Revisions and scope

- Original project baseline: `fce23b8d`.
- Data-exactness candidate before this optimization: `d3c4dbb0`.
- Optimized candidate: the commit containing this report.
- The measured refresh call is `BatchWriter::refresh_latest_dao_statistics`.
- The measured snapshot call is
  `BatchWriter::apply_exact_completed_dao_snapshot`.
- The measured snapshot batch additionally calls
  `BatchWriter::update_dao_daily_snapshot` and
  `StoreBatch::commit` against a temporary RocksDB database. It excludes
  unrelated CF writes and transaction parsing from a full live-sync batch.

Before optimization, refresh read status-0 and status-1 index rows and their
DAO entries, then made one full `dao_deposits` scan for tip compensation and a
second full scan for the rolling baseline. After optimization, the status
index reads are unchanged and one full primary-table scan computes both exact
observations. The fixture has two distinct lifecycle header keys; it does
not model the cost of thousands of distinct header reads in a real chain.
In the [8,192-entry phase trace](2026-10-25-dao-performance-phases.log),
the pre-optimization candidate spent about 46–57 ms on the status scan,
63–74 ms on exact tip compensation, 67–80 ms on the separate rolling
baseline scan, and less than 0.5 ms on persistence.

## Reproducible fixture and checks

The manual [fixture injector](../../../scripts/dao_perf_fixture.py) adds a
temporary test to `statistics.rs` in each **isolated worktree**. The test is
removed before committing product code. It seeds 512 or 8,192 unique DAO
outpoints, 128 owner lock hashes, deposit block 50, request block 80,
completion block 90, tip block 100, deposit AR 1, observation AR 2, 200 CKB
capacity and 102 CKB occupied capacity per entry. The mixed dataset uses
25% status 0, 25% status 1, and 50% status 2; completed entries carry the
exact 98 CKB compensation. The test checks live capacity, free capacity,
claimed and unclaimed compensation, and the persisted completed-day
snapshot on every iteration. At 8,192 mixed entries each revision produced
the same `capacity=81920000000000`,
`free=40140800000000`, `unclaimed=40140800000000`, and
`claimed=40140800000000` shannons.

For each candidate binary: compile outside the timed section with
`cargo test --locked -p ckbadger-indexer --lib test_dao_perf_nonempty_fixture --offline -- --nocapture`.
For the original baseline, generate its historical lockfile locally with
the same command without `--locked`.
Use [the comparison runner](../../../scripts/dao_perf_compare.py) on the
resulting test binaries. It alternates revisions in ABBA order for each
entry count, uses three warmups plus 12 timed calls per run, records every
sample and host load, and rejects numeric output differences. Example:

```bash
python scripts/dao_perf_fixture.py crates/indexer/src/db/writer/statistics.rs current
python scripts/dao_perf_compare.py \
  --before-worktree /path/to/preopt-worktree \
  --after-worktree /path/to/optimized-worktree \
  --before-binary /path/to/preopt-test-binary \
  --after-binary /path/to/optimized-test-binary \
  --output /tmp/dao-perf.jsonl --mixed --iterations 12 --entries 512 8192
```

For `fce23b8d`, pass `baseline` instead of `current` to the fixture
injector: the original refresh owns the tip daily row, whereas the
data-exactness candidate requires that row to be exact before refresh.
The explicit seed difference is outside every timed section. The old
baseline fixture lacks the additional batch measurement; the comparison
runner accepts that.

## Measurements

Host: AMD Ryzen AI 9 HX 370, 24 logical CPUs, 93 GiB RAM; Rust 1.99.0.
Build: Cargo `test` profile, `opt-level=0`, debuginfo enabled. No service or
existing work database was read or modified. Values below are the medians
of each 12-sample run, in milliseconds. Per-sample ranges and test output
are retained in the linked JSONL files.

| Comparison | Entries | Revision | Refresh runs | Snapshot runs | Snapshot batch runs |
| --- | ---: | --- | ---: | ---: | ---: |
| Same candidate, load 4.3–4.7 | 512 | Before optimization | 11.49 / 11.54 | 4.06 / 4.02 | 3.63 / 4.06 |
| Same candidate, load 4.3–4.7 | 512 | After optimization | 6.76 / 7.41 | 3.58 / 3.80 | 3.59 / 3.87 |
| Same candidate, load 4.3–4.7 | 8,192 | Before optimization | 164.62 / 145.91 | 63.83 / 52.09 | 61.73 / 52.08 |
| Same candidate, load 4.3–4.7 | 8,192 | After optimization | 141.34 / 87.63 | 80.10 / 56.77 | 62.37 / 50.15 |
| Original baseline vs final, load 2.5–2.7 | 8,192 | `fce23b8d` | 112.69 / 140.69 | 57.79 / 95.97 | — |
| Original baseline vs final, load 2.5–2.7 | 8,192 | After optimization | 85.56 / 109.06 | 44.97 / 58.14 | 46.51 / 74.18 |

The same-candidate mixed 8,192-entry comparison has an overall refresh
median of 162.53 ms before versus 101.30 ms after (37.7% lower).
The snapshot batch path stayed in a similar range. Individual samples vary
with shared-host contention and RocksDB cache state; these figures are
synthetic microbenchmarks, not a mainnet sync-throughput estimate. The
active-only fixture had larger variance and is not used to claim a
performance improvement.

Raw logs: [same-candidate snapshot and batch](2026-10-25-dao-performance-paired-batch-mixed.jsonl),
[same-candidate snapshot only](2026-10-25-dao-performance-paired-mixed.jsonl),
and [original baseline versus final](2026-10-25-dao-performance-baseline-final-mixed.jsonl).
The [active-only paired log](2026-10-25-dao-performance-paired-active.jsonl)
shows its larger run-to-run variance.
The first log contains all 288 timed calls across eight runs, including
minimum and maximum samples; no outliers were removed.

Validation: DAO store tests 22/22, indexer statistics tests 31/31,
`cargo fmt --all -- --check`, and `git diff --check` passed. The
combined-observation store test compares both results with the original
independent exact views using active and completed entries. The temporary
fixture checks all four totals for both revisions at both sizes.

Store boundary: the optimization reads the domain DAO table and retains
the existing domain stats writes. It does not touch the append-only or
network stores. No re-sync is required for this optimization.
