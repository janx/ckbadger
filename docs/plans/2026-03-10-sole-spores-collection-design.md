# Design: Sole Spores Sentinel Collection

**Date**: 2026-03-10
**Status**: Approved

## Problem

Clusterless Spores (those with `cluster_id = None`) have no collection page, no aggregates, no holder stats, and no activity feed. They are only visible in the global `/spore/nfts` list.

## Solution

Define a sentinel collection ID for clusterless Spores, following the existing Identity sentinel pattern (`DOTBIT_SENTINEL_COLLECTION`, `DID_CKB_SENTINEL_COLLECTION`). The indexer materializes all collection infrastructure; the API hardcodes display metadata.

## Sentinel Definition

```rust
pub const SOLE_SPORES_SENTINEL_COLLECTION: [u8; 32] = *b"sole_spores_collection__________";
```

Follows the existing convention of human-readable 32-byte ASCII constants padded with underscores.

## Indexer Changes

When a Spore has `cluster_id = None` (and is not `is_did`):

- Set `collection_id = SOLE_SPORES_SENTINEL_COLLECTION`
- Write `spore_by_cluster` index entry
- Update `ClusterAggregate` (total/live/owner counts)
- Update owner counts in `stats_spore` CF
- Write collection activity entries

Reorg rollback follows existing cluster rollback paths — the sentinel is just another cluster_id.

## API Changes

- Recognize `SOLE_SPORES_SENTINEL_COLLECTION` in cluster endpoints
- Return hardcoded metadata: name `"Sole Spores"`, description `"Spores not belonging to any cluster"`
- No fake `ObjectEntry` for the cluster itself
- Accept `"sole-spores"` as a URL-friendly alias (like Identity accepts `"dotbit"`)

## Frontend Changes

None needed. Sole Spores appears in cluster listing automatically; detail page works as-is.

## Storage Impact

- No new CFs — reuses existing `spore_by_cluster`, `cluster_agg`, `stats_spore`, collection activity CFs
- Requires DB rebuild (re-sync from genesis)

## What Doesn't Change

- Spores with a real `cluster_id` — unchanged
- `did:ckb` Spores — already handled by `DID_CKB_SENTINEL_COLLECTION`
- Cluster Cell parsing — unchanged

## Principle Alignment

- **CKB Native**: Makes clusterless Spores (a valid CKB concept) first-class citizens
- **Local First**: No external dependencies; sentinel is a compile-time constant
- **Agent Friendly**: Uniform collection API — no special-casing for "no cluster"
