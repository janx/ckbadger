# Cluster v1 description parser correction

## Goal

Correct an upstream parser defect exposed while implementing D4's raw Cluster Cell endpoint. This is a separate, explicitly authorized correction outside the original read-time-only API scope.

A two-field Molecule Cluster table starts its first field at byte 12. The old shared parser treated bytes 12–16 as a third-field offset whenever the payload was at least 16 bytes long. In v1 those bytes contain the name length, so the description was discarded. This was not missing chain evidence.

## Principle Alignment

- Fix the owning shared parser, with no API-specific decoder, fallback, backfill, or automatic database repair.
- The official immutable schemas distinguish [two-field ClusterData](https://github.com/sporeprotocol/spore-contract/blob/9a6010e43ffd09ac9e6fe7ee389cc915e3e4f999/lib/types/schemas/spore_v1.mol) from [three-field ClusterDataV2](https://github.com/sporeprotocol/spore-contract/blob/9a6010e43ffd09ac9e6fe7ee389cc915e3e4f999/lib/types/schemas/spore_v2.mol). The first Molecule field offset, not payload length, identifies the header layout.
- Both live `sync/batch.rs` → `parse_clusters` and bulk `bulk_build/facts.rs` → `parse_cluster_parsed_cell` reach the same `parse_cluster_data` function. The API also reuses that parser.
- Logical store impact: corrected name/description projections flow through existing indexer **domain** writers. No storage method, schema, or persistent write path is added. `CF_CELLS` append-only bytes and all network-store state are unchanged. Parsing borrows the original bytes; the bulk-entry regression also asserts its input bytes remain identical.

## Result

The parser selects the complete table size as the description end for a two-field header, and uses the third offset for three-field or compatible extended headers. Existing three-field tests remain unchanged. New regressions cover normal, empty, and Unicode v1 fields through both actual parser entry points, plus the API's exact raw-cell endpoint.

- Baseline: 25 parser tests passed.
- RED: both new parser regressions and the API regression reproduced the lost description.
- GREEN: 27 parser tests, 35 asset-history endpoint tests, and 28 Spore endpoint tests passed.
- Normal commit hooks validate formatting and strict workspace Clippy.
- Re-sync required: **yes for already-indexed two-field Cluster metadata and dependent DOB results**. Replaying with the corrected indexer is the supported repair. The read-only raw endpoint immediately interprets original bytes correctly, but does not repair persisted metadata or cached decode state.
- This task did not clear, rewrite, or re-sync any user database and did not start or restart user services.

A bounded real-chain inspection found the selected mainnet Nervape sample is three-field and unaffected: block 12,808,955, outpoint `0xa11e0db807fb728bf48bebd9796ef14cb16de96e41b347aa4f02cd7860cc1cf8:0`, first offset 16, 5,135 data bytes. This does **not** establish that all other locally indexed clusters are unaffected; no full-store or full-chain scan was performed.

### Focused performance observation

A scratch harness extracted the exact old/new shared data-parser bodies, compiled both into one optimized binary, and alternated AB/BA order for eight rounds of 100,000 parses each. It used the real 5,135-byte three-field sample and checked equivalent output before timing. Setup and input loading were outside timing; no RocksDB, RPC, or synchronization was measured.

| Parser | Mean per parse | Median per parse |    Round range |
| ------ | -------------: | ---------------: | -------------: |
| Before |       2.109 µs |         2.116 µs | 2.045–2.170 µs |
| After  |       2.129 µs |         2.101 µs | 2.016–2.350 µs |

The mean changed +0.95% and median −0.71%. These short measurements ran on the shared development host alongside other task work, without exclusive CPU affinity. They provide a bounded observation of the unchanged three-field parser, not a whole-indexer throughput guarantee. Two-field work now includes the previously omitted description and is not semantically equivalent to the broken baseline. No new bulk-sync benchmark was run.

### Artifacts

Owner worktree `.lane-results/` preserves `cluster-parser-baseline.log`, `cluster-parser-v1-red.log`, `cluster-protocol-v1-red.log`, `cluster-parser-v1-green.log`, and `cluster-protocol-v1-green.log`; the scratch benchmark source, input, CSV and JSON summary use the `cluster-parser-bench` prefix. Official schemas are preserved under `.lane-results/upstream/`. The independent real-chain sample is in root `.worktrees/site-redesign-round3-results/d4-legacy-cluster-{creation-rpc,molecule-evidence}.json`.
