# Code Cells Display Design

## Problem

A data-hash-type script's `code_hash` is the blake2b of the bytecode. Multiple cells can contain the same bytecode (same code deployed multiple times, or genesis + later deployments). Currently we resolve and display a single outpoint — users can't see how many code cells exist, which are live (usable as `cell_dep`), and when the code was first deployed.

## Design

### API Changes

**New endpoint:** `GET /api/v1/scripts/code-cells`

```
GET /api/v1/scripts/code-cells?code_hash=0x...&hash_type=data
```

Response:

```json
{
  "codeCells": [
    {
      "txHash": "0xaa...",
      "outputIndex": 0,
      "status": "live",
      "createdAtBlock": 0,
      "capacity": "16200000000"
    },
    {
      "txHash": "0xbb...",
      "outputIndex": 1,
      "status": "consumed",
      "createdAtBlock": 1234,
      "capacity": "16200000000"
    }
  ],
  "liveCount": 1,
  "totalCount": 2
}
```

**Extend existing responses:** Add `codeCellsLiveCount` and `codeCellsTotal` to `ScriptLookupInfo` (from `/scripts/lookup`) and `ScriptResponse` (from `/scripts`), so deployment tables can show counts without a separate call.

### Store Changes

New method `list_all_cells_by_data_hash` — returns ALL matching cells (live + consumed) from `CF_CELL_BY_DATA_HASH`. Unlike `find_any_cell_by_data_hash` (returns first match), this returns the full list. Similarly `list_all_cells_by_type` for type-ref scripts (though type_id uniqueness means 0-1 results).

### Frontend Changes

**`/script/:codeHash` page** — Deployment panel:

- Replace the single "Code Cell" column with an expandable "Code Cells" sub-section below the deployment row.
- Header: `Code Cells (1 live, 2 consumed)`.
- List each code cell: outpoint link, status badge (green=Live, gray=Consumed), created-at block link, capacity.
- Live cells first, then consumed, both sorted by creation block ascending.

**`/scripts/:name` page** — Deployments panel:

- Each deployment row: show outpoint of first live cell + badge like `(3 live)` or `(0 live, 2 consumed)`.
- Click to expand inline sub-table showing all code cells for that deployment.

### Data Flow

```
Frontend                          API                          Store
────────                          ───                          ─────
lookupScripts()          →   /scripts/lookup          →   merge_script_info + count query
  returns codeCellsLiveCount, codeCellsTotal

getCodeCells()           →   /scripts/code-cells      →   list_all_cells_by_data_hash
  returns CodeCell[]                                       OR list_all_cells_by_type
```

### Type-ref vs Data-ref Behavior

|              | type ref                               | data ref                                    |
| ------------ | -------------------------------------- | ------------------------------------------- |
| Store method | `list_all_cells_by_type` (0-1 results) | `list_all_cells_by_data_hash` (0-N results) |
| Display      | Single cell (current behavior)         | Full list                                   |

For type-ref scripts with 0-1 code cells, the UI gracefully degrades to current single-cell display.

### Not In Scope

- Cell data preview/hex in the code cells list (click through to `/cell/:id`)
- Sorting/filtering controls (small result set)
- Caching (small result set, fast query)
