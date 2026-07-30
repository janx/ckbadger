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

9. **Memory must be bounded by retained bytes**
   Prefetch and flush queues must apply backpressure using retained payload bytes, not only item
   counts or estimated block density. Final-snapshot materialization must stream through bounded
   write batches instead of constructing a second full copy of reducer state. If the process
   cannot continue within its whole-process memory budget, fail with RSS, swap, block, and owner
   context before the kernel OOM killer intervenes.

10. **Bulk-to-live handoff uses a fresh process**
    After all bulk rows and completion metadata are durably finalized, the indexer exits
    successfully. The supervisor immediately starts a new indexer process, which selects the
    normal near-tip pipeline from the persisted tip. The bulk reducer heap must not be retained
    into live sync.

11. **Only one co-resident network may bulk-sync at a time**
    In orchestrator mode, indexers are admitted in `[[network]]` declaration order. APIs,
    enabled crawlers, and the shared frontend may start immediately, and indexers already past
    the bulk threshold may continue near-tip/live sync, but the supervisor must not run two
    fresh-store bulk-build engines concurrently. This is a resource invariant, not a fallback:
    failure of one network stops sequenced admission instead of silently skipping to another.

## Design Implications

- Keep bulk sync logic simple and write-throughput oriented.
- Reorg-specific correction logic belongs to bounded reorg handling paths, not bulk rebuild paths.
- Avoid adding bulk-mode logic that assumes resuming from half-built state.
- Size automatic RocksDB and bulk budgets from the network's co-resident RAM share; explicit
  per-network overrides remain explicit and are never divided again.

## Implementation

The bulk-build engine (`crates/indexer/src/sync/bulk_build/`) implements these rules via an
in-memory build model where RocksDB is the final write-once artifact, not working memory.

See [docs/superpowers/specs/2026-03-17-bulk-sync-build-engine-design.md](../superpowers/specs/2026-03-17-bulk-sync-build-engine-design.md)
for the full design spec and [docs/INDEXER_PIPELINE.md](../INDEXER_PIPELINE.md) for runtime
architecture details.

### Key invariants

- **LiveCellOwner** is the authoritative in-memory live-cell set. All input resolution uses it
  directly — no DB reads for correctness-critical data. Canonical data hashes stay inline in the
  live-cell slot; the side maps contain only genuinely sparse protocol/UDT/DAO facts.
- **IdentityInterner** deduplicates lock/type/data hashes into `u32` IDs, keeping per-cell memory
  compact; lookup and ID tables share each interned byte payload. Live-cell creation/consumption
  maintains exact per-ID reference counts. Zero-live IDs are reclaimed only between batches after
  all frozen read views and batch-local facts have been released; slot reuse must first invalidate
  any per-ID write-dedup marker.
- **AddressOwner** keeps fixed-size lock/transaction hashes in memory and converts to the stable
  store representation only while emitting final rows.
- **Owner reducers** (address, script, token, DAO, object, fiber) consume resolved tx facts and
  maintain their domain state in memory until materialization.
- CF writes are classified by policy: Class A (append-only event rows streamed immediately),
  Class B (final snapshot written once after convergence), Class C (sealed aggregates flushed
  when time bucket closes), Class D (disabled during bulk sync).
- Activity Class C buckets seal only when their exact UTC+8 bucket end is at or below CKB median
  time past (37 headers including the current header, upper median). This bounds unique-address
  sets without allowing a later valid block to modify an already-written bucket.
- Class B owners and live-cell indexes emit sequentially through domain-store batches capped at
  32 MiB. `CF_CELLS` remains on the separate append-only history path and is never targeted by
  finalize materialization.
- The whole-process guard uses `VmRSS + VmSwap` on Linux and the process physical footprint
  (resident + compressed memory from `proc_pid_rusage`) on macOS. An unavailable or invalid
  platform measurement fails the batch checkpoint. `[indexer].bulk_memory_budget_gb` overrides
  the automatic per-network RAM share; a zero budget is invalid.
- On supported platforms the `ckbadger` process uses jemalloc for both Rust and unprefixed C
  allocation symbols, so RocksDB `WriteBatch` churn and Rust reducer churn have one allocator
  owner whose unused pages can be reclaimed consistently. Memory failures and performance
  samples must report the platform's available process/allocator measurements (RSS/swap and
  jemalloc allocated/active/resident/retained bytes on Linux; process physical footprint on
  macOS), plus separate domain/append-only memtable and table-reader observations.
- Domain and append-only RocksDB memtables, table readers, compaction backlog, SSTs, L0 files,
  and immutable memtables are store-local and must be summed. Their block cache and
  WriteBufferManager are process-wide shared resources and must be counted exactly once.
