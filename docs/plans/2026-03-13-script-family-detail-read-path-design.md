# Script Family Detail Read Path Design

## Goal

- Make script detail pages consistent at the deployment-family level.
- Ensure unknown script families still resolve a code cell.
- Ensure capacity history charts render as continuous time series even when daily deltas are compressed.
- Explicitly exclude the previously discussed Transactions tab from this change.

## Principle Alignment

- CKB Native: resolve code cells from actual chain data when label metadata is incomplete; chart values remain derived from exact indexed daily deltas.
- Local First: no new persistent indexes or backfill workflows; fix read semantics on top of existing local stores.
- Agent Friendly: unify detail-page reads around one deployment-family interpretation instead of ad hoc per-endpoint behavior.

## Problem

Current script detail reads split along three partially independent paths:

1. Script usage/activity counts operate on raw involved `code_hash` values.
2. Code-cell resolution depends on deployment metadata already stored in `ScriptInfo`.
3. Capacity-history rendering only spans days that still have non-zero daily delta rows.

This produces inconsistent pages for unknown `data`-family scripts:

- the page can show active usage but no code cell,
- family-related hashes are not consistently treated as one deployment,
- a compressed history can collapse to one point even though the live state persists across many days.

## Approved Scope

- Aggregate script detail reads by deployment family, not by exact URL hash alone.
- Keep existing RocksDB responsibility boundaries unchanged.
- Do not add a Transactions tab.

## Design

### 1. Deployment-Family Resolution

All script-detail reads should start from the same family resolution step:

- use `merge_script_info_for_reference()` and `related_code_hashes_for_reference()`,
- treat type-hash and bytecode-hash references for one deployment as one family,
- keep the requested URL hash as the page identity, but resolve detail data from the family.

This family resolution already exists in parts of the API and should become the single read path for:

- code-cell lookup,
- capacity-history aggregation.

### 2. Code Cell Resolution

Code-cell lookup remains ordered by strongest chain-grounded signal:

1. If the family has a usable type reference, query the deployment cell by type hash from the domain store.
2. If stored `ScriptInfo` already carries a pre-resolved outpoint, use it.
3. If the family is a `data` / `data1` / `data2` reference and still unresolved, use `CkbChainReader::find_cell_by_data_hash()` against the direct CKB RocksDB reader.

Behavioral rules:

- type-ref lookup remains the primary path; the direct data-hash scan is only a fallback for unresolved bytecode families,
- unknown script families must no longer silently return an empty code cell when the chain reader can resolve the live deployment cell,
- if the direct CKB reader is unavailable and no other path resolves the code cell, return an internal error with the requested hash and family context instead of pretending the code cell is absent.

### 3. Capacity History Rendering

Storage remains compressed:

- `stats_script` keeps only non-zero daily deltas,
- zero-net days remain absent from RocksDB.

Read semantics change:

- family-related hashes are aggregated first,
- the chart start remains the first day with known family delta unless `from` is explicitly supplied,
- the chart end becomes the latest complete indexed UTC day, not merely the last day with a retained delta row,
- missing days in that range are expanded as zero-delta days, so cumulative used/unused values carry forward unchanged.

This preserves exactness:

- no interpolation,
- no estimation,
- no synthetic value other than “same cumulative state as previous day” on zero-delta days.

### 4. Latest Complete Indexed Day

The chart should stop at the latest complete indexed UTC day:

- derive the synced tip block from the domain store,
- load its timestamp from cached block headers,
- convert that timestamp to UTC day,
- if the tip day is still incomplete relative to the chart rule, stop at the previous UTC day.

The exact helper should be shared inside the scripts route module so the chart logic has one clear boundary for “latest complete indexed day”.

### 5. Frontend Impact

No new frontend interaction model is required.

Expected frontend-visible changes:

- unknown script family pages show a concrete code-cell link once the backend resolves it,
- capacity charts render multiple points across the valid date range even when only one stored delta day exists.

The existing page components can keep their current layout and query usage if the API response shape remains unchanged.

## Validation Plan

- Add regression tests for code-cell fallback from unresolved bytecode family to direct CKB data-hash lookup.
- Add regression tests for script capacity-history expansion from a single stored delta day to a continuous chart ending at the latest complete indexed day.
- Run targeted API Rust tests covering both helper logic and route behavior.

## Storage Boundary Check

- Domain vs append-only target confirmed: yes
- Append-only update/delete path check: pass
- New persistent write paths: none
- Re-sync required: no

## Result

- Script detail pages become family-consistent without schema changes.
- Unknown bytecode families can still expose deployment code cells.
- Compressed daily-delta storage remains compact while charts stay readable and correct.
