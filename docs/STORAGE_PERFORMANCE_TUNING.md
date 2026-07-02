# Storage Performance Tuning

Performance analysis and tuning guide for ckbadger's RocksDB storage on encrypted Linux filesystems.

## Context

ckbadger's bulk-sync path writes two RocksDB instances (domain + append-only); a third, opt-in network store (written separately by the crawler, not the indexer) is outside this tuning scope. During bulk sync, the indexer pipeline reads blocks from the CKB node's RocksDB and writes parsed data to ckbadger's RocksDB. Both read and write paths go through the host filesystem.

This document captures findings from benchmarking on a LUKS-encrypted btrfs root partition and provides tuning recommendations.

## Why btrfs COW Hurts RocksDB

btrfs COW (copy-on-write) means every write allocates new disk space rather than updating in place:

1. **Write amplification**: Each data write triggers metadata B-tree updates and checksum computation
2. **Fragmentation**: Repeated overwrites (RocksDB compaction) scatter data across the disk, degrading sequential read performance over time
3. **Compaction penalty**: RocksDB compaction rewrites large SST files; btrfs COW turns each rewrite into allocate-new + write + free-old, roughly doubling the I/O
4. **Small write overhead**: WAL writes and manifest updates pay disproportionate COW tax

## Impact on ckbadger Bulk Sync Pipeline

During bulk sync, all three pipeline stages interact with the filesystem through LUKS + btrfs:

```
Fetcher  --[read CKB RocksDB]--> LUKS decrypt --> btrfs read (COW fragmentation)
Parser   --[CPU-intensive]------> competes with dm-crypt kernel threads for CPU
Writer   --[write ckbadger DB]--> LUKS encrypt --> btrfs write (COW amplification)
Compact  --[background R+W]----> both encrypt + decrypt, heavy COW overhead
```

Note: During bulk sync, the Fetcher reads directly from the CKB node's local RocksDB via `CkbChainReader` (not network RPC). This means the read path also goes through LUKS + btrfs.

## Recommended Fix: Disable btrfs COW on Data Directories

The simplest and lowest-risk optimization. No partition changes needed.

### How

`chattr +C` disables COW for new files created in a directory. Existing files are unaffected, so the cleanest approach is to start from a fresh workdir and set `+C` on that workdir before any data directories are created:

```bash
# 1. Stop ckbadger
ckbadger stop

# 2. Create a fresh workdir
mkdir ./workdir-nocow

# 3. Disable COW on the workdir before ckbadger creates any files under it
chattr +C ./workdir-nocow

# 4. Verify (should show 'C' on the workdir)
lsattr -d ./workdir-nocow

# 5. Initialize and sync using the new workdir so data/ directories inherit No_COW
ckbadger init --workdir ./workdir-nocow
ckbadger run --workdir ./workdir-nocow
```

Any data directories created later under `./workdir-nocow` inherit the No_COW behavior for newly created files, which is what RocksDB needs. Do not try to retrofit `+C` onto existing non-empty RocksDB files.

Optionally use the same fresh-workdir pattern for the CKB node's data directory to speed up Fetcher reads.

### What You Lose (and Why It Doesn't Matter for ckbadger)

| Lost Feature            | Impact on ckbadger                                  |
| ----------------------- | --------------------------------------------------- |
| Filesystem checksums    | None. RocksDB has its own block checksums.          |
| Self-healing (RAID)     | None. Single-disk setup has no redundancy anyway.   |
| Snapshot efficiency     | None. ckbadger data is not snapshotted.             |
| Transparent compression | Minimal. RocksDB compresses its own SST files.      |
| Write atomicity (COW)   | None. RocksDB WAL provides its own write atomicity. |

## Alternative: Dedicated Unencrypted Partition

For maximum performance, create a separate unencrypted XFS partition by shrinking the existing LUKS volume. This eliminates both btrfs COW and LUKS overhead.

### When This Is Worth It

- The `chattr +C` approach doesn't provide sufficient improvement
- You want to also benchmark the LUKS overhead in isolation
- You need the absolute maximum I/O throughput for development iteration speed

## Recommendation

Start with `chattr +C`. It's zero-risk, takes 5 minutes, and addresses the biggest bottleneck (btrfs COW). Only pursue the partition approach if more speed is needed after measuring.
