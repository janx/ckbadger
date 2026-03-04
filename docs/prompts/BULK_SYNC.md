# Bulk Sync Rules

This document is the single source of truth for ckbadger bulk sync behavior.

## Scope

Bulk sync is the high-throughput index building path used with a fresh store.

## Mandatory Rules

1. **Single-shot rebuild only**  
   Bulk sync must either finish end-to-end or fail fast.

2. **Fresh DB only**  
   Bulk sync is only for rebuild from genesis on a fresh RocksDB state.  
   Resuming bulk sync from partially synced RocksDB is explicitly unsupported.

3. **No partial-state recovery flow in bulk mode**  
   During bulk sync, do not auto-cleanup partial state and continue.  
   Do not add defer + refill/rebuild patterns to recover correctness.

4. **All data must be inline**  
   All data required by ckbadger must be calculated and written inline on the canonical block path. No backfill/rebuild after bulk sync is allowed.

5. **Failure handling**  
   If bulk sync fails, fix the indexer/store logic first, then delete RocksDB and re-sync from genesis.

6. **No reorg handling in bulk stage**  
   During bulk sync, reorg detection, fork-point search, rollback, and deep-fork handling must not run.  
   Reorg handling is a live-sync-only path.

7. **Assume optimal path for speed**  
   Bulk sync must optimize for the happy path and maximum throughput.  
   Use optimistic execution instead of defensive recovery branches in the hot path.

8. **Abnormal state policy**  
   If bulk sync encounters abnormal state or invariant violations, fail fast and rebuild from genesis.  
   Do not add complex in-place repair flows for bulk mode.

## Design Implications

- Keep bulk sync logic simple and write-throughput oriented.
- Reorg-specific correction logic belongs to bounded reorg handling paths, not bulk rebuild paths.
- Avoid adding bulk-mode logic that assumes resuming from half-built state.
