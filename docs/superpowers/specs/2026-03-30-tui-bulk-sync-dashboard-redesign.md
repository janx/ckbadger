# TUI Bulk Sync Dashboard Redesign

## Goal

Replace the current 3-column "Sync Diagnostics" + 3-column "Controller/Watchers/Jobs" sections with a unified 2-column dashboard that simultaneously answers: "how fast vs best run?", "what's the bottleneck?", and "is hardware saturated?"

## Architecture

The bottom ~21 rows of the Sync tab (currently split between `draw_sync_diagnostics` and `draw_background_tasks`) become a single `draw_bulk_sync_dashboard` function with a consistent 2-column layout:

- **Left column (~55%)**: Real-time state — numbers, badges, bars, channel fill gauges. Content width is naturally bounded (numbers don't stretch).
- **Right column (~45%)**: Sparkline trends — these benefit from extra width on large terminals, showing more history.

Five row groups stacked vertically: Pipeline, Throughput, Controller, Resources, Build Stages (toggle). Each group spans both columns. Watchers/Jobs collapse to a single row shown only when non-empty.

## Scope

- Only the bulk-build mode of the Sync tab bottom section.
- Pipeline mode (live sync) and idle mode keep their current layout unchanged.
- No changes to the top half (status row + charts).
- No new data fields needed from the indexer — all metrics already exist in `BulkBuildProgressData`.

## Layout

### Standard mode (~21+ rows available)

```
─── Pipeline ─────────────────────────────┬──────────────────────────────────
 prefetch [████░ 48/64]→ build 1.94s      │ build  ▃▄▅▆▇▅▃▄▅▆▇▅▃▄▅▆▇▅▃▄
 → prepare [█░ 1/2]→ commit → disk 64%   │ flush  ▅▆▇▆▅▆▇█▇▆▅▆▇▆▅▆▇█▇▆
─── Throughput ───────────────────────────┼──────────────────────────────────
 12.0K blk/s  31K tx/s  43K blk/bat      │ blk/s  ▂▃▅▆▇▇▆▅▃▅▆▇▇▆▅▃▅▆▇▇
 us/cell: build 8.9  flush 18.7          │ us/cel ▇▆▅▅▄▃▃▂▂▃▄▅▇▆▅▅▄▃▃▂
 stall 3/min (69 total)  best: 6.6/13.4  │ stall  ░░░░░█░░░░░██░░░░░░░█
─── Controller ───────────────────────────┼──────────────────────────────────
 [BUILD] 2-5s ██████░░ 1.94s → GROW +5%  │ target ▁▂▃▄▅▆▇▇▇▇▇▇▁▂▃▄▅▆▇▇
 200K cells  fill 85%  thr 8  bg 4  L0 27│ L0     ▃▄▅▆▇▆▅▄▃▂▃▄▃▄▅▆▇▆▅▄
 waste: recv 35% | flush 65%             │ waste% ▇▆▅▅▄▃▃▂▂▃▅▇▆▅▅▄▃▃▂▂
─── Resources ────────────────────────────┼──────────────────────────────────
 CPU 63% (15/24)  Mem 64% (61G/96G)      │ CPU    ▅▅▆▆▅▅▆▇▆▅▅▆▅▅▆▆▅▅▆▇
 Disk 64% avg  181M/s  qd 17  await 0.2  │ disk%  ▅▆▇▇▇▆▅▆▇█▇▆▅▆▇▇▇▆▅▆
```

### Compact mode (~10-12 rows available)

Drop Resources group. Collapse Pipeline and Throughput to fewer lines. Sparklines narrow but still present:

```
─── Pipeline ────────────────────────────┬────────────────────────────
 prefetch[48/64]→ build 1.94s →prep→cmit│ build ▃▄▅▆▇▅  flush ▅▆▇▆▅
─── Throughput ──────────────────────────┼────────────────────────────
 12K blk/s  us/cell 8.9/18.7  stall 3/m│ blk/s ▂▃▅▆▇▇  us/c ▇▆▅▅▄
─── Controller ──────────────────────────┼────────────────────────────
 [BUILD] ██████░░ 1.94s GROW +5% L0 27  │ L0 ▃▄▅▆▇▆▅  target ▁▂▃▄▅
 200K thr 8 bg 4 waste:recv 35%|fl 65%  │ waste ▇▆▅▅▄▃  disk ▅▆▇▇▇
```

### Ultra-compact (~6 rows, minimum viable)

Two dense rows, no sparklines:

```
 prefetch→ build 1.94s →prep→cmit  12K blk/s  us/cell 8.9/18.7  stall 3/min
 [BUILD] GROW +5% 200K thr 8 bg 4 L0 27 fl 1/2  CPU 63% Disk 64% Mem 64%
```

## Row Group Details

### Pipeline (2-3 rows)

Shows the data flow: `prefetch → build → prepare → commit → disk`.

**Left column:**
- Row 1: `prefetch [████░ P/C]→ build Xs [reduce 61% addr 51%]`
  - Prefetch channel fill gauge with pending/capacity
  - Build time (the central number, colored green if in 2-5s band, red otherwise)
  - Top 2 build sub-phases as percentage of build time (inline, gray) — only when build is the bottleneck
- Row 2: `→ prepare [█░ P/C]→ commit → disk D%`
  - Prepare-commit channel fill gauge (the new pipeline stage)
  - Disk utilization percentage (colored: green <70%, amber 70-90%, red >90%)

**Right column:**
- Row 1: `build` sparkline (green) — build_ms history
- Row 2: `flush` sparkline (cyan) — flush_ms history (now = commit-only time)

The inline sub-phase display (`[reduce 61% addr 51%]`) replaces the old 6-row vertical stage list. Press `e` to toggle a full expansion that temporarily replaces the Resources group with a detailed stage breakdown:

```
─── Build Stages (e to close) ────────────────────────────────────────────────
 facts ██░ 175s 20%  resolve █░ 86s 10%  reduce ████░ 520s 61%
 addr  ████░ 438s 51%  history ███░ 328s 39%  activity █░ 94s 11%
```

### Throughput (2-3 rows)

Answers "faster or slower than best run?"

**Left column:**
- Row 1: `12.0K blk/s  31K tx/s  43K blk/bat`
  - Block rate, tx rate, blocks per batch
- Row 2: `us/cell: build 8.9  flush 18.7`
  - Per-cell cost — THE key efficiency metric. Computed as build_ms*1000/cells_created and flush_ms*1000/cells_created.
  - Color: green if ≤ best baseline, amber if 1-1.5x baseline, red if >1.5x baseline
- Row 3 (if room): `stall 3/min (69 total)  best: 6.6 / 13.4`
  - Stall rate (batches > 2x avg per minute)
  - Best-run baseline numbers for comparison

**Right column:**
- `blk/s` sparkline — throughput trend
- `us/cel` sparkline — per-cell cost trend (inverted color: green=low=good)
- `stall` sparkline — stall event markers

**Note on "best baseline"**: The best-run values (6.6 us/cell build, 13.4 us/cell flush) are hardcoded constants for now, derived from perf analysis. A future enhancement could auto-detect the best run from `trend.jsonl` but that's out of scope.

### Controller (2-3 rows)

Shows bottleneck classification and adaptive response.

**Left column:**
- Row 1: `[BUILD] 2-5s ██████░░ 1.94s → GROW +5%`
  - Bottleneck badge: `[FETCH]` amber / `[BUILD]` green / `[FLUSH]` red — bold, colored background
  - Build-time band bar (2-5s target range, green=in-band)
  - Current build EMA with sizing decision and delta
- Row 2: `200K cells  fill 85%  thr 8  bg 4  L0 27`
  - Target cells budget
  - Fill ratio (actual/target)
  - Fetch threads, background jobs, L0 file count (red if >40)
- Row 3 (if room): `waste: recv 35% | flush 65%`
  - Waste composition — which idle time dominates

**Right column:**
- `target` sparkline — target_cells trend (shows controller adaptation)
- `L0` sparkline — L0 file count trend (shows compaction pressure)
- `waste%` sparkline — total waste ratio trend

### Resources (2 rows)

Hardware utilization at a glance. Hidden in compact mode, replaced by Build Stages when `e` is pressed.

**Left column:**
- Row 1: `CPU 63% (15/24)  Mem 64% (61G/96G)  imm 3`
  - CPU: load average / core count as utilization percentage
  - Memory: used/total
  - Immutable memtables count (RocksDB flush pressure indicator)
- Row 2: `Disk 64% avg  181M/s  qd 17  await 0.2ms`
  - Disk utilization, write throughput, queue depth, average await

**Right column:**
- `CPU` sparkline
- `disk%` sparkline

### Watchers/Jobs (conditional, 0 or 2-3 rows)

Only shown when `app.background_tasks` has visible entries. During bulk sync this is almost always empty. When shown, renders as a simple table row at the very bottom, below the dashboard groups:

```
─── Background ────────────────────────────────────────────────────────────────
 verify-fast: running 12s  ⬤  label-import: done 0.3s ✓
```

Single row for up to 3-4 tasks. If more, expand to a table like today.

## Sparkline History Buffers

New `VecDeque` history fields in `App` state, updated each refresh cycle from `BulkBuildProgressData`:

| Buffer | Source field | Description |
|---|---|---|
| `build_cpu_ms_history` | Already exists | Build time per batch |
| `flush_wait_ms_history` | Already exists | Flush blocking time |
| `fetch_wait_ms_history` | Already exists | Prefetch blocking time |
| `blk_per_sec_history` | `rate_realtime` from SyncStatusRow | Throughput trend |
| `us_per_cell_build_history` | `build_ms * 1000 / cells_created` | Build efficiency |
| `us_per_cell_flush_history` | `flush_ms * 1000 / cells_created` | Flush efficiency |
| `target_cells_history` | `target_cells` | Controller target trend |
| `l0_history` | `controller_l0_ema` | Compaction pressure |
| `waste_pct_history` | `(recv_ema + wait_ema) / (build_ema + recv_ema + wait_ema) * 100` | Pipeline waste |
| `cpu_pct_history` | `load_avg / cores * 100` (from sync status) | CPU utilization |
| `disk_util_history` | `disk_util_pct` | Disk utilization |
| `stall_history` | Derived: 1 if `build_ms > 2 * build_ema`, else 0 | Stall event markers |

History size: same as existing `RATE_HISTORY_SIZE` (3600 samples).

## Density Detection

Reuse existing `detect_layout_density()` function:

| Available rows | Mode | Groups shown |
|---|---|---|
| ≥18 | Standard | Pipeline(3) + Throughput(3) + Controller(3) + Resources(2) + separator lines(4) = 15 core, +watchers if present |
| 10-17 | Compact | Pipeline(2) + Throughput(2) + Controller(2) + separator(3) = 9 core |
| <10 | Ultra-compact | 2 dense lines, no sparklines |

## Keyboard Interactions

| Key | Current behavior | New behavior |
|---|---|---|
| `e` | Toggle build subphases | Toggle: Resources ↔ Build Stages detail |
| `v` | Cycle diagnostics view mode | Cycle: auto → compact → detail → ultra-compact |
| `c` | Toggle compact layout | Unchanged |

## Color Scheme

Reuse existing palette constants from `ui.rs`:

| Signal | Color | When |
|---|---|---|
| Healthy/growing | `TERMINAL_GREEN` | Build in band, growing, CPU active |
| Warning/secondary | `AMBER` | Shrinking, fetch bottleneck, moderate pressure |
| Flush/IO | `CYAN` | Flush metrics, commit time |
| Critical | `ERROR_RED` | L0 >40, flush queue full, disk >90%, stalls |
| Neutral | `SLATE_500` | Labels, inactive, secondary text |
| Section headers | `SLATE_700` | `─── Pipeline ───` separator lines |

## Implementation Boundary

**In scope:**
- New `draw_bulk_sync_dashboard()` replacing `draw_sync_diagnostics()` + `draw_background_tasks()` for bulk-build mode
- New sparkline history buffers in `App`
- Derived metrics: `us_per_cell_build`, `us_per_cell_flush`, `waste_pct`, `stall_rate`
- Compact/ultra-compact layout variants
- `e` key toggles Build Stages detail
- Existing tests updated for new function signatures

**Out of scope:**
- Pipeline mode (live sync) layout changes
- New data fields from indexer (all metrics already available)
- Auto-detecting best-run baseline from `trend.jsonl`
- Changes to the Sync tab top half (status row, charts)
- Changes to Overview or System tabs

## Non-bulk-sync Fallback

When `sync.bulk_build` is `None` (pipeline mode or idle), the bottom section continues to use the existing `draw_sync_diagnostics()` + `draw_background_tasks()` layout unchanged. The new dashboard only activates during bulk sync.
