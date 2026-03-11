# UX/UI Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Redesign ckbadger frontend with Chinese traditional color palette, focused dashboard layout, full-immersion terminal atmosphere, and entity-link navigation.

**Architecture:** Pure frontend styling overhaul. No backend changes. Replace Tailwind color tokens → update CSS variables → update component classes → restructure homepage layout. Design doc: `docs/plans/2026-03-11-ux-ui-redesign.md`.

**Tech Stack:** Tailwind CSS 3.4, React 19, TypeScript, JetBrains Mono

---

## Phase 1: Foundation (color tokens + CSS variables + effects)

### Task 1: Replace Tailwind color tokens

**Files:**

- Modify: `frontend/tailwind.config.ts`

**Step 1: Replace the color system**

Replace all color definitions in tailwind.config.ts with the Chinese traditional palette. The key changes:

- Remove old neon colors (phosphor green `#44ee77`, rose `#ff4477`, sky `#44bbff`, mint `#00ffaa`, violet `#aa77ff`)
- Add new Chinese traditional colors: jade `#2edba3`, rouge `#e8555a`, aqua `#68ccf0`, gold `#f2c55c`, lavender `#b8a9e8`, amber `#d4883a`
- Update background tokens to match design doc (void, bg, surface, elevated)
- Update text tokens (text-bright, text, text-dim, text-ghost)
- Update semantic mappings (interactive → aqua, positive → jade, negative → rouge, warning → gold)
- Update boxShadow glow values for each new color
- Keep all animations, spacing, and font config unchanged

Reference the design doc color tables for exact hex values. Map the old token names to new ones:

- `interactive` / `interactive-hover` → aqua `#68ccf0` / jade `#2edba3`
- `positive` / `positive-bright` → jade `#2edba3` / `#3ef0b8`
- `negative` / `negative-bright` → rouge `#e8555a` / `#f06668`
- `warning` → gold `#f2c55c`
- `info` → aqua `#68ccf0`
- `emphasis-dim` → gold-dim `#d0a840`
- Keep accent- prefix for chart palette: accent-1 through accent-12

**Step 2: Verify build compiles**

Run: `cd frontend && pnpm type-check`
Expected: PASS (no type errors, just class name changes)

**Step 3: Commit**

```
feat(frontend): replace color tokens with Chinese traditional palette
```

---

### Task 2: Update CSS variables and global effects

**Files:**

- Modify: `frontend/app/globals.css`

**Step 1: Update CSS custom properties**

In the `:root` / `@layer base` section, update all `--color-*` variables to match the new Tailwind tokens. Ensure the CSS variables stay in sync with tailwind.config.ts values.

**Step 2: Add atmosphere effect utilities**

Add these new utility classes (append to globals.css):

```css
/* === Atmosphere: Full Immersion (no particles) === */

/* CRT Scan Lines - always present */
.crt-scanlines {
  position: fixed;
  inset: 0;
  background: repeating-linear-gradient(
    0deg,
    transparent 0px,
    transparent 1px,
    rgba(0, 0, 0, 0.07) 1px,
    rgba(0, 0, 0, 0.07) 3px
  );
  pointer-events: none;
  z-index: 9999;
}

/* Vignette overlay */
.crt-vignette {
  position: fixed;
  inset: 0;
  background: radial-gradient(ellipse at center, transparent 20%, rgba(0, 0, 0, 0.55) 100%);
  pointer-events: none;
  z-index: 9998;
}

/* Ambient glow - 3 positioned blurred radials */
.ambient-glow {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: -1;
}
.ambient-glow::before {
  content: '';
  position: absolute;
  inset: 0;
  background:
    radial-gradient(ellipse 350px 250px at 25% 15%, rgba(46, 219, 163, 0.07) 0%, transparent 70%),
    radial-gradient(ellipse 300px 200px at 75% 50%, rgba(104, 204, 240, 0.05) 0%, transparent 70%),
    radial-gradient(ellipse 200px 200px at 50% 85%, rgba(242, 197, 92, 0.05) 0%, transparent 70%);
}

/* Neon edge reflection for cards */
.neon-edge-top {
  position: relative;
}
.neon-edge-top::after {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 1px;
  background: linear-gradient(
    90deg,
    transparent,
    var(--neon-color, rgba(46, 219, 163, 0.4)),
    transparent
  );
  pointer-events: none;
}

/* Left color bar variants */
.neon-bar-left {
  position: relative;
}
.neon-bar-left::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  width: 3px;
  height: 100%;
  background: var(--bar-color, #2edba3);
  box-shadow:
    0 0 20px var(--bar-color, #2edba3),
    0 0 40px var(--bar-glow, rgba(46, 219, 163, 0.25));
}

/* Glow tier utilities */
.glow-neon {
  text-shadow:
    0 0 10px var(--glow-color),
    0 0 30px var(--glow-color-mid, rgba(46, 219, 163, 0.25)),
    0 0 60px var(--glow-color-far, rgba(46, 219, 163, 0.15));
}
.glow-soft {
  text-shadow:
    0 0 20px var(--glow-color-mid, rgba(46, 219, 163, 0.25)),
    0 0 40px var(--glow-color-far, rgba(46, 219, 163, 0.15));
}

/* Neon flicker animation for hero values */
@keyframes neon-flicker {
  0%,
  90%,
  100% {
    opacity: 1;
  }
  92% {
    opacity: 0.85;
  }
  94% {
    opacity: 1;
  }
  96% {
    opacity: 0.9;
  }
}
.animate-neon-flicker {
  animation: neon-flicker 8s ease-in-out infinite;
}

/* Live dot with pulse */
.live-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #2edba3;
  box-shadow:
    0 0 8px #2edba3,
    0 0 16px rgba(46, 219, 163, 0.25);
  animation: live-pulse 1.5s ease-in-out infinite;
}
@keyframes live-pulse {
  0%,
  100% {
    box-shadow: 0 0 4px #2edba3;
  }
  50% {
    box-shadow:
      0 0 10px #2edba3,
      0 0 20px rgba(46, 219, 163, 0.25);
  }
}
```

**Step 3: Remove or update conflicting old utilities**

Review existing `.terminal-card`, `.terminal-border-glow`, `.indicator-light`, `.crt-screen` classes. Update their color references from old tokens to new ones. Remove duplicate functionality where the new utilities replace them.

**Step 4: Verify build**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 5: Commit**

```
feat(frontend): add atmosphere effects and update CSS variables
```

---

### Task 3: Update chart color palette

**Files:**

- Modify: `frontend/lib/chart-colors.ts`

**Step 1: Replace chart colors**

```typescript
// Chinese Traditional palette for charts
export const CHART_PRIMARY_COLOR = '#2edba3'; // 翠玉 jade
export const CHART_SECONDARY_COLOR = '#e8555a'; // 胭脂 rouge
export const CHART_TERTIARY_COLOR = '#68ccf0'; // 缥碧 aqua

export const CHART_PALETTE = [
  '#2edba3', // jade
  '#e8555a', // rouge
  '#68ccf0', // aqua
  '#f2c55c', // gold
  '#b8a9e8', // lavender
  '#d4883a', // amber
  '#1fb88a', // jade-dim
  '#c04048', // rouge-dim
  '#4aa8d0', // aqua-dim
  '#d0a840', // gold-dim
  '#9888c8', // lavender-dim
  '#b07028', // amber-dim
];

export const CHART_GRID_COLOR = '#1a1f30';
export const CHART_AXIS_COLOR = '#343c50';
export const CHART_TOOLTIP_BG = '#10131c';
export const CHART_HOVER_BG = '#161a25';
```

**Step 2: Verify no type errors**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 3: Commit**

```
feat(frontend): update chart colors to Chinese traditional palette
```

---

## Phase 2: Core Components

### Task 4: Update TerminalPanel with neon edge reflections

**Files:**

- Modify: `frontend/components/ui/terminal-panel.tsx`

**Step 1: Update color references**

Replace all old color class names (e.g., `text-terminal-green`, `text-amber`, `border-base-border`) with new token names from the Tailwind config. Add neon-edge-top class to panel headers. Add optional `accentColor` prop for the left bar glow (jade/aqua/gold/rouge/lavender).

**Step 2: Add LiveDot to TerminalPanelHeader**

When the `indicator` prop is "active", render a `<span className="live-dot" />` instead of the old indicator light.

**Step 3: Verify build + existing tests pass**

Run: `cd frontend && pnpm type-check && npx vitest run`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): update TerminalPanel with neon edge effects
```

---

### Task 5: Update StatBlock with glow tiers

**Files:**

- Modify: `frontend/components/ui/stat-block.tsx`

**Step 1: Update color variants**

Replace the color prop options from `green/amber/white` to `jade/aqua/gold/rouge/lavender/default`. Map each to the corresponding new text color class and glow tier:

- jade: `text-jade glow-neon` with jade CSS variables
- aqua: `text-aqua glow-neon` with aqua CSS variables
- gold: `text-gold glow-neon` with gold CSS variables
- rouge: `text-rouge glow-neon` with rouge CSS variables
- default: `text-text-bright` (no glow)

Add `glowTier` prop: 'neon' | 'soft' | 'none' (default: 'none').
When glowTier is set, apply the corresponding `.glow-neon` or `.glow-soft` utility with inline CSS variables for the color.

**Step 2: Update trend indicator colors**

- up → jade
- down → rouge
- flat → text-dim

**Step 3: Update MiniStat color variants similarly**

**Step 4: Verify build + tests**

Run: `cd frontend && pnpm type-check && npx vitest run`
Expected: PASS

**Step 5: Commit**

```
feat(frontend): update StatBlock with glow tier system
```

---

### Task 6: Create EntityLink component pattern

**Files:**

- Modify: `frontend/components/ui/address.tsx`
- Modify: `frontend/components/ui/hash.tsx`

**Step 1: Update Address component**

Change the link color from current to aqua default, jade on hover:

- Default: `text-aqua`
- Hover: `hover:text-jade hover:glow-soft` (with jade glow CSS vars)
- Transition: `transition-colors`

**Step 2: Update Hash component**

Same aqua → jade hover pattern. Ensure the `HexCode` component uses aqua for the hash display.

**Step 3: Verify build + tests**

Run: `cd frontend && pnpm type-check && npx vitest run`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): apply EntityLink pattern to address and hash components
```

---

## Phase 3: Layout

### Task 7: Update header and navigation

**Files:**

- Modify: `frontend/components/layout/header.tsx`
- Modify: `frontend/components/layout/logo.tsx`

**Step 1: Update header styling**

- Set header height to 42px
- Background: `bg` token
- Nav link colors: text-dim default → jade when active
- Ensure search bar is prominent in center

**Step 2: Update logo**

- Color: jade (replacing current green/amber)
- Add live-dot next to logo

**Step 3: Verify build**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): update header and logo with jade accent
```

---

### Task 8: Update footer/status bar

**Files:**

- Modify: `frontend/components/layout/site-footer.tsx`

**Step 1: Update to status bar style**

- 26px height, surface background
- Show sync tip, epoch, version in monospace dim text
- Jade live-dot for connected status

**Step 2: Commit**

```
feat(frontend): restyle footer as terminal status bar
```

---

### Task 9: Update search bar and command palette

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/command-palette.tsx`

**Step 1: Update search-bar colors**

Replace all old color references. Input prompt: jade `>`. Result highlights: aqua for entities. Selected: elevated background + jade prefix.

**Step 2: Update command-palette colors**

Result type badges use semantic colors: Block → aqua, Tx → jade, Address → text-bright, Token → gold, Script → lavender. Section headers: text-dim. Active item: surface background + jade indicator.

**Step 3: Verify build**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): update search and command palette colors
```

---

## Phase 4: Homepage Restructure

### Task 10: Add atmosphere overlays to layout

**Files:**

- Modify: `frontend/app/layout.tsx`

**Step 1: Add CRT + vignette + ambient glow overlays**

Inside the body (after Providers, before content), add:

```tsx
<div className="crt-scanlines" />
<div className="crt-vignette" />
<div className="ambient-glow" />
```

These are fixed-position overlays that cover the entire viewport. They layer on top of (scanlines, vignette) and behind (ambient glow) the content.

**Step 2: Verify visual — run dev server**

Run: `cd frontend && pnpm dev`
Check: overlays visible, content still readable, no interaction blocking.

**Step 3: Commit**

```
feat(frontend): add CRT scanlines, vignette, and ambient glow to layout
```

---

### Task 11: Restructure homepage to focused dashboard

**Files:**

- Modify: `frontend/components/home-content.tsx`
- Modify: `frontend/components/stats-cards.tsx`

**Step 1: Redesign stats-cards as hero stat row**

Replace `StatsCards` 6-column grid with 4 hero stat cards:

1. Latest Block (jade accent, neon glow tier)
2. Hash Rate (aqua accent)
3. DAO Locked (gold accent)
4. Active Addresses (rouge accent)

Each card uses the NeonCard pattern: left color bar + top reflection + hero-sized number (28px, font-weight 700) + label + delta subtitle.

**Step 2: Restructure home-content layout**

Change the current multi-section vertical layout to focused dashboard:

- Hero stat row (4 cards) at top
- 2-column grid below: `LatestBlocks` (2fr) + `LatestActivities` (1fr)
- Move EpochProgress, MiniStatsCards, PipelinePreview, ActivityBreakdown, HomeCharts, LatestTransactions below the fold or into secondary sections

**Step 3: Verify build + dev server**

Run: `cd frontend && pnpm type-check && pnpm dev`
Check: homepage renders with new layout

**Step 4: Commit**

```
feat(frontend): restructure homepage as focused dashboard
```

---

### Task 12: Update LatestBlocks and LatestActivities

**Files:**

- Modify: `frontend/components/latest-blocks.tsx`
- Modify: `frontend/components/latest-activities.tsx`

**Step 1: Update LatestBlocks**

- Block numbers: aqua text with EntityLink pattern
- Hashes: text-dim, truncated
- Tx count: text-bright
- Timestamp: text-dim
- New block highlight: jade glow instead of amber

**Step 2: Update LatestActivities**

- Badge colors: transfer → jade border, DAO → gold border, NFT/Spore → lavender border, negative → rouge border
- CKB amounts: jade (positive) / rouge (negative)
- Addresses: aqua EntityLinks
- Tx hashes: aqua EntityLinks
- New activity highlight: jade glow

**Step 3: Verify build + tests**

Run: `cd frontend && pnpm type-check && npx vitest run`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): update blocks and activities with Chinese traditional colors
```

---

## Phase 5: Charts

### Task 13: Update chart components

**Files:**

- Modify: `frontend/components/ui/line-chart.tsx`
- Modify: `frontend/components/ui/stacked-area-chart.tsx`
- Modify: `frontend/components/ui/pie-chart.tsx`
- Modify: `frontend/components/ui/spark-chart.tsx`
- Modify: `frontend/components/ui/multi-series-line-chart.tsx`
- Modify: `frontend/components/charts/chart-page.tsx`

**Step 1: Update all chart components**

Replace hardcoded color values with imports from `chart-colors.ts`. Update:

- Primary stroke: jade `#2edba3`
- Fill gradients: jade at 15-20% opacity
- Grid lines: `#1a1f30`
- Axis text: `#343c50`
- Tooltip bg: `#10131c`
- Multi-series: use CHART_PALETTE for series colors
- Glow filter on chart lines: `filter: drop-shadow(0 0 4px rgba(46, 219, 163, 0.4))`

**Step 2: Update chart-page wrapper**

Dark background tokens, updated title colors.

**Step 3: Verify build**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): update chart components with Chinese traditional palette
```

---

## Phase 6: Remaining Pages (color token sweep)

### Task 14: Update all remaining page components

**Files:**

- All `frontend/app/*/client-page.tsx` files
- All remaining `frontend/components/*.tsx` files
- `frontend/components/ui/tabs.tsx`
- `frontend/components/ui/hex-display.tsx`
- `frontend/components/ui/cursor-pagination.tsx`
- `frontend/components/ui/capacity.tsx`
- `frontend/components/ui/capacity-utilization.tsx`
- `frontend/components/ui/page-header.tsx`
- `frontend/components/ui/progress-bar.tsx`
- `frontend/components/ui/occupation-range-selector.tsx`
- `frontend/components/ui/script-view.tsx`
- `frontend/components/ui/terminal-number.tsx`
- `frontend/components/chain-wave/*.tsx`
- `frontend/components/error-boundary.tsx`
- `frontend/components/not-found-page.tsx`

**Step 1: Bulk search-and-replace old color tokens**

Systematic replacement across all files:

- `text-terminal-green` / `text-interactive` → appropriate new token (jade, aqua, or gold based on role)
- `text-amber` / `amber-dim` → `text-gold` / `gold-dim`
- `text-mint` → `text-jade`
- `bg-mint` → `bg-jade`
- `text-rose` → `text-rouge`
- `border-mint` → `border-jade`
- Old glow shadow classes → new glow utilities

This is a mechanical token swap. Use the Color Role Assignments table from the design doc to decide which old color maps to which new semantic role.

**Step 2: Update tabs.tsx**

Active tab: jade text + jade-dim bottom border. Inactive: text-dim.

**Step 3: Update hex-display.tsx**

Multi-color byte cycling using Chinese traditional palette: aqua (4n+1), lavender (6n+1), gold (8n+1), rouge (10n+1).

**Step 4: Update terminal-number.tsx**

Glow intensity variants use jade glow values instead of amber.

**Step 5: Verify build + all tests**

Run: `cd frontend && pnpm type-check && pnpm lint && npx vitest run`
Expected: ALL PASS

**Step 6: Commit**

```
feat(frontend): complete color token migration across all components
```

---

## Phase 7: Validation

### Task 15: Visual validation and cleanup

**Step 1: Run full test suite**

Run: `cd frontend && npx vitest run`
Expected: ALL PASS

**Step 2: Run type-check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: ALL PASS

**Step 3: Format all files**

Run: `pnpm format`

**Step 4: Dev server visual review**

Run: `cd frontend && pnpm dev`

Check each key page visually:

- Homepage: hero stats with jade/aqua/gold/rouge, 2-column layout, scan lines, vignette
- Address detail: aqua EntityLinks, jade/rouge amounts, tabbed sections
- Blocks list: aqua block numbers, table with row hover
- DAO page: gold accent throughout
- Charts: jade/rouge/aqua chart lines, dark backgrounds
- Search/CommandPalette: correct type badges, jade active states
- Mobile: responsive layout intact

**Step 5: Fix any visual issues found**

**Step 6: Final commit**

```
chore(frontend): visual cleanup and formatting
```

---

## Summary

| Phase              | Tasks | Files     | Description                                              |
| ------------------ | ----- | --------- | -------------------------------------------------------- |
| 1: Foundation      | 1-3   | 3 files   | Color tokens, CSS variables, chart palette               |
| 2: Core Components | 4-6   | 4 files   | TerminalPanel, StatBlock, EntityLink                     |
| 3: Layout          | 7-9   | 5 files   | Header, footer, search, command palette                  |
| 4: Homepage        | 10-12 | 5 files   | Atmosphere overlays, dashboard layout, blocks/activities |
| 5: Charts          | 13    | 6 files   | All chart components                                     |
| 6: Token Sweep     | 14    | ~30 files | Remaining component color migration                      |
| 7: Validation      | 15    | 0 files   | Testing, visual review, cleanup                          |

**Total: 15 tasks, ~50 files touched**

**Key verification commands:**

```bash
cd frontend && pnpm type-check       # TypeScript
cd frontend && pnpm lint             # ESLint
cd frontend && npx vitest run        # Tests
cd frontend && pnpm dev              # Visual review
pnpm format                          # Prettier
```
