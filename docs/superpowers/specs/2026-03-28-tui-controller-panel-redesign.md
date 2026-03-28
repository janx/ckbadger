# TUI Controller Panel Redesign

## Goal

Align the TUI sync tab's controller panel and overlap column with the new wall-clock band + build/IO overlap algorithm. The current display centers on `overlap %` (a metric the controller no longer targets). The redesign makes the controller's actual decision signals — wall clock, build vs IO, and the resulting action — visible at a glance.

## Scope

Two functions in `crates/tui/src/ui.rs`:
- `controller_panel_lines()` (~300 lines) — the bottom controller diagnostics panel
- `draw_overlap_column()` (~200 lines) — the right-side dual-lane timeline + sparklines

No IPC or data pipeline changes. All required data (`controller_recv_ema`, `controller_build_ema`, `controller_wait_ema`) already exists in `BulkBuildProgressData`.

## Design

### Controller Panel

#### Compact mode (2 lines)

```
wall 2.1s [██████░░░░] build 1.8s io 0.3s  250K (+12%)  fill 78%
[BUILD] thr 8 bg 6  L0 12  fl 1/4
```

**Line 1 — sizing:**
- `wall X.Xs` — wall clock EMA (recv + build + flush), the P1 signal. Colored by band: green if ∈ [1s, 3s], red if outside.
- `[██████░░░░]` — 10-char bar showing wall clock position within [1s, 3s]. Filled chars = position in band. Green in-band, red out-of-band. If wall < 1s, bar pinned left (all empty). If wall > 3s, bar pinned right (all filled).
- `build X.Xs  io X.Xs` — build EMA and IO EMA (recv + flush) side by side. This is the P2 signal.
- `250K (+12%)` — target_cells budget with delta. Same as before but more compact (drop "cells" label).
- `fill X%` — actual vs target cell ratio. Same as before.

**Line 2 — I/O resources:**
- `[BOTTLENECK]` badge — same as before (FETCH/BUILD/FLUSH, colored).
- `thr X bg X` — fetch_threads and bg_jobs. Same as before.
- `L0 X` — L0 EMA. Same as before (red if > 40).
- `fl X/X` — flush channel pending/capacity. Same as before.

#### Detail mode (5 lines)

```
── sizing ── wall 2.1s [██████░░░░] ─────
  build 1.8s  io 0.3s (recv 87% flush 13%)  → grow
  budget 250K cells (+12%)  148k blk  1.7c/b  fill 78%
── i/o ── [BUILD] ───────────────────────
  thr 8 (+1) bg 6 (=)  L0 12  flush 1/4
```

**Line 1 — sizing header:**
- Replace `overlap X%` with `wall X.Xs [band bar]`.

**Line 2 — build vs IO:**
- `build X.Xs  io X.Xs` — the two quantities the controller compares.
- `(recv X% flush X%)` — waste composition breakdown, inlined after IO. Only shown when waste > 1ms.
- `→ grow` / `→ shrink` / `→ hold` — controller decision label. Derived from budget delta: `|delta| < 0.5% → hold`, `delta > 0 → grow`, `delta < 0 → shrink`. Colored: green for grow, amber for shrink, dim for hold.

**Line 3 — budget:** Same content as before (target_cells, block count, density, fill ratio).

**Line 4 — I/O header:** Replace `waste X.Xs  recv X%  flush X%` with just the bottleneck badge. Waste composition moved to line 2.

**Line 5 — I/O knobs:** Same as before (threads with delta, bg_jobs with delta, L0, flush channel).

### Overlap Column

#### Header

Current: `Batch #42  2100ms  eff 85%`

New: `Batch #42  wall 2.1s  build>io`

- Replace raw `Xms` with `wall X.Xs` (consistent with controller panel).
- Replace `eff X%` with dominance indicator: `build>io` (green) when build > IO, `io>build` (amber/red) when IO ≥ build.
- Wall clock value colored by band position (same logic as controller panel).

#### Dual-lane timeline rows

Current labels: `CPU` and `I/O`.

New label: `Build` replaces `CPU`. `I/O` stays.

The bar rendering logic (filled blocks for build, dim blocks for wait, fetch/flush positioning) is unchanged — it accurately visualizes timing regardless of what the controller does with the data.

#### Sparklines

No changes. `Build`, `FetW`, `FluW` sparklines remain as-is.

### Band Bar Rendering

The band bar is a 10-character visual showing where wall clock sits within [1s, 3s]:

```rust
// Map wall_clock into [0, 10] within the [1s, 3s] range.
let position = ((wall_clock - 1000.0) / 2000.0 * 10.0).round().clamp(0.0, 10.0) as usize;
let filled = position;
let empty = 10 - filled;
let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));
```

Color:
- `wall_clock < 1000.0` → red (below band, bar shows `[░░░░░░░░░░]`)
- `wall_clock > 3000.0` → red (above band, bar shows `[██████████]`)
- Otherwise → green (in band)

### Wall Clock Color

Used in both controller panel and overlap column header:

```rust
let wall_color = if wall_clock >= 1000.0 && wall_clock <= 3000.0 {
    TERMINAL_GREEN
} else {
    ERROR_RED
};
```

### Decision Label

Shown in detail mode line 2. Derived from budget delta percentage:

```rust
let (decision_label, decision_color) = if delta_pct > 0.5 {
    ("→ grow", TERMINAL_GREEN)
} else if delta_pct < -0.5 {
    ("→ shrink", AMBER)
} else {
    ("→ hold", SLATE_500)
};
```

When no delta is available (first batch), show nothing.

## Files Changed

| File | Change |
|------|--------|
| `crates/tui/src/ui.rs` | Rewrite `controller_panel_lines()` and `draw_overlap_column()` header/label |

## Testing

The TUI rendering functions are pure (take data, return Lines/render to Frame). Manual visual verification by running `ckbadger tui` during a bulk sync. No unit tests for rendering layout (consistent with existing codebase — no TUI rendering tests exist).
