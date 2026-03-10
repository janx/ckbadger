# Midnight City Pop Terminal — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reskin the ckbadger frontend from light Qinglv palette to dark midnight city pop terminal aesthetic.

**Architecture:** Pure token/class replacement — no structural changes, no new components, no API changes. Replace Tailwind color tokens, CSS variables, chart constants, and component Tailwind classes. All changes are visual only.

**Tech Stack:** Tailwind CSS, CSS custom properties, React/TypeScript (class changes only)

**Design Reference:** `docs/plans/2026-03-10-midnight-citypop-design.md` + `frontend/public/mood-previews.html`

---

## Validation Strategy

This is a visual reskin — no behavioral tests. Validate each task with:

1. `cd frontend && pnpm type-check` — no TypeScript errors
2. `cd frontend && pnpm lint` — no ESLint errors
3. Visual inspection via `pnpm dev` at key checkpoints

---

### Task 1: Foundation — tailwind.config.ts

**Files:**

- Modify: `frontend/tailwind.config.ts`

**Step 1: Replace the entire color palette**

Replace the `colors` object (lines 8-52) with the midnight city pop tokens:

```typescript
colors: {
  // Midnight City Pop palette
  base: {
    bg: '#0c0e15',
    surface: '#10131c',
    elevated: '#161a25',
    border: '#1a1f30',
  },
  text: {
    primary: '#dee2ec',
    secondary: '#a0a8be',
    muted: '#606880',
    dim: '#343c50',
  },
  interactive: {
    DEFAULT: '#f0b866',
    hover: '#f8ca80',
    muted: '#f0b86620',
    dim: '#c89440',
  },
  emphasis: {
    DEFAULT: '#f0b866',
    dim: '#c89440',
    glow: '#f0b86630',
    bright: '#f8ca80',
  },
  positive: {
    DEFAULT: '#5ce0b8',
    dim: '#3cb898',
  },
  negative: {
    DEFAULT: '#e86080',
    dim: '#c04860',
    bright: '#f07898',
  },
  warning: {
    DEFAULT: '#f0b866',
    dim: '#c89440',
    bright: '#f8ca80',
  },
  info: {
    DEFAULT: '#6ab0e8',
    dim: '#4a88c0',
    bright: '#82c0f0',
  },
  // City pop accent colors (new)
  amber: {
    DEFAULT: '#f0b866',
    dim: '#c89440',
  },
  rose: {
    DEFAULT: '#e87ea0',
    dim: '#c0608a',
  },
  sky: {
    DEFAULT: '#6ab0e8',
    dim: '#4a88c0',
  },
  mint: {
    DEFAULT: '#5ce0b8',
    dim: '#3cb898',
  },
  violet: {
    DEFAULT: '#b08af0',
    dim: '#8a68c8',
  },
},
```

**Step 2: Replace box shadows**

Replace `boxShadow` (lines 61-66):

```typescript
boxShadow: {
  glow: '0 0 8px #f0b86618, 0 0 16px #f0b86610',
  'glow-strong': '0 0 6px #f0b86628, 0 0 14px #f0b86618',
  'glow-inset': 'inset 0 1px 8px #f0b86608',
  'interactive-glow': '0 0 8px #f0b86620, 0 0 16px #f0b86610',
},
```

**Step 3: Remove display font family**

Replace `fontFamily` (lines 54-57) — pure monospace only:

```typescript
fontFamily: {
  mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
  display: ['var(--font-mono)', 'ui-monospace', 'monospace'],
},
```

**Step 4: Verify**

Run: `cd frontend && pnpm type-check`
Expected: PASS (no type changes)

**Step 5: Commit**

```
feat(frontend): replace tailwind color tokens with midnight city pop palette
```

---

### Task 2: Foundation — globals.css

**Files:**

- Modify: `frontend/app/globals.css` (449 lines)

**Step 1: Replace CSS variables (:root block, lines 5-17)**

```css
:root {
  --foreground: #dee2ec;
  --background: #08090e;
  --color-surface: #10131c;
  --color-elevated: #161a25;
  --color-border: #1a1f30;
  --color-emphasis: #f0b866;
  --color-emphasis-dim: #c89440;
  --color-interactive: #f0b866;
  --color-interactive-hover: #f8ca80;
  --font-mono: 'JetBrains Mono', 'SFMono-Regular', 'Roboto Mono', ui-monospace, monospace;
  --font-display: 'JetBrains Mono', 'SFMono-Regular', 'Roboto Mono', ui-monospace, monospace;
}
```

**Step 2: Replace body styles (lines 19-26)**

Remove noise texture SVG (light-theme artifact). Replace with clean dark background:

```css
body {
  color: var(--foreground);
  background: var(--background);
  min-height: 100vh;
}
```

**Step 3: Replace utility colors in @layer utilities (lines 28-200)**

Update hardcoded color values throughout:

- `.terminal-text`: color `#f0b866`, text-shadow `0 0 4px #f0b86630`
- `.terminal-text-dim`: color `#c89440`, text-shadow `0 0 3px #c8944020`
- `.crt-screen::before` scanline: change `rgba(42, 37, 32, 0.04)` to `rgba(0, 0, 0, 0.08)` for dark bg
- `.crt-screen::after` vignette: change to `rgba(0, 0, 0, 0.15)`
- `.scanline-overlay` gradient: change `rgba(30, 122, 106, 0.008)` to `rgba(240, 184, 102, 0.01)`
- `.hex-char:hover`: color `var(--color-emphasis)` (already variable, OK)
- `.row-scan::after` gradient: change `rgba(58, 170, 128, 0.04)` to `rgba(240, 184, 102, 0.06)`
- `.indicator-light`: background `#5ce0b8`, box-shadow `0 0 4px #5ce0b880, 0 0 8px #5ce0b840`
- `.indicator-light-amber`: background `#f0b866`, box-shadow `0 0 6px #f0b866`
- `.logo-glow`: change `rgba(30, 122, 106, ...)` to `rgba(240, 184, 102, ...)`
- `.logo-glow-flicker`, `.logo-flicker`, `.logo-cursor-flash`: keep animation names, colors updated via filter
- `.cycles-calculating-marquee`: change gradient colors to `#606880` and `#dee2ec`

**Step 4: Replace keyframe colors (lines 202-384)**

Update all `rgba(30, 122, 106, ...)` references in keyframes to `rgba(240, 184, 102, ...)` (amber).

**Step 5: Replace @layer base (lines 386-390)**

```css
@layer base {
  * {
    @apply border-base-border;
  }
}
```

(Same class, but now resolves to `#1a1f30` via new tailwind config)

**Step 6: Replace @layer components (lines 392-449)**

Update `.terminal-card`:

- border-color: `#1a1f30`
- background: `#0c0e15`
- box-shadow: `inset 0 0 30px rgba(240, 184, 102, 0.02), 0 0 10px rgba(240, 184, 102, 0.04)`
- scanline rgba: `rgba(0, 0, 0, 0.06)`

Update `.terminal-card-header`:

- border-color: `#1a1f30`
- gradient: `rgba(240, 184, 102, 0.03)`

Update `.terminal-border-glow::after`:

- gradient: `rgba(240, 184, 102, 0.15)` and `rgba(240, 184, 102, 0.06)`

**Step 7: Add new global effects after @layer components**

Append CRT scanline overlay + vignette + ambient glow as `body::before` and `body::after`:

```css
/* Midnight CRT effects */
body::after {
  content: '';
  position: fixed;
  inset: 0;
  background: repeating-linear-gradient(
    0deg,
    transparent 0px,
    transparent 2px,
    rgba(0, 0, 0, 0.08) 2px,
    rgba(0, 0, 0, 0.08) 4px
  );
  pointer-events: none;
  z-index: 9999;
  opacity: 0.35;
}

body::before {
  content: '';
  position: fixed;
  inset: 0;
  background: radial-gradient(ellipse at center, transparent 50%, rgba(0, 0, 0, 0.4) 100%);
  pointer-events: none;
  z-index: 9998;
}
```

**Step 8: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 9: Commit**

```
feat(frontend): replace globals.css with midnight city pop theme
```

---

### Task 3: Foundation — chart-colors.ts + layout.tsx

**Files:**

- Modify: `frontend/lib/chart-colors.ts` (31 lines)
- Modify: `frontend/app/layout.tsx` (23 lines)

**Step 1: Replace chart-colors.ts entirely**

```typescript
export const CHART_PRIMARY_COLOR = '#f0b866'; // amber
export const CHART_SECONDARY_COLOR = '#e87ea0'; // rose
export const CHART_TERTIARY_COLOR = '#6ab0e8'; // sky

export const CHART_PALETTE = [
  '#f0b866', // amber
  '#e87ea0', // rose
  '#6ab0e8', // sky
  '#5ce0b8', // mint
  '#b08af0', // violet
  '#f8ca80', // amber-bright
  '#f07898', // rose-bright
  '#82c0f0', // sky-bright
  '#78f0d0', // mint-bright
  '#c8a0f8', // violet-bright
  '#c89440', // amber-dim
  '#c0608a', // rose-dim
] as const;

// Centralized chart UI colors (dark theme)
export const CHART_GRID_COLOR = '#1a1f30';
export const CHART_AXIS_COLOR = '#343c50';
export const CHART_TOOLTIP_BG = '#10131c';
export const CHART_TOOLTIP_BORDER = '#222840';
export const CHART_HOVER_COLOR = '#161a25';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
```

**Step 2: Update layout.tsx body class**

Change line 13 from:

```tsx
<body className="font-sans antialiased">
```

to:

```tsx
<body className="font-mono antialiased bg-[#08090e] text-text-secondary">
```

**Step 3: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): update chart colors and root layout for midnight theme
```

---

### Task 4: Layout — header.tsx + logo.tsx + site-footer.tsx

**Files:**

- Modify: `frontend/components/layout/header.tsx` (107 lines)
- Modify: `frontend/components/layout/logo.tsx` (38 lines)
- Modify: `frontend/components/layout/site-footer.tsx` (60 lines)

**Step 1: Update header.tsx**

Replace all Tailwind color classes. Key replacements:

- `bg-base-bg/85` → `bg-base-bg/95`
- `border-base-border/80` → `border-base-border`
- `backdrop-blur-md` → `backdrop-blur-sm` (less blur needed on dark)
- Active nav: `border-interactive/50 bg-interactive/12 text-interactive` → `border-amber/40 bg-amber/8 text-amber`
- Inactive nav: `text-text-secondary/85` → `text-text-muted`
- Hover nav: `hover:border-base-border/80 hover:bg-base-elevated/35` → `hover:text-text-secondary`
- Logo link shadow: remove `shadow-[inset_0_0_0_1px_...]`

**Step 2: Update logo.tsx**

Replace color classes:

- Dollar sign: `text-interactive` → `text-text-muted`
- "ckbadger" text: `text-emphasis` → `text-amber`
- Cursor block: update colors to amber

**Step 3: Update site-footer.tsx**

Replace color classes:

- Background: `from-base-elevated/75 via-base-bg to-base-bg` → `bg-base-surface`
- Text: `text-text-muted` stays (now resolves to `#606880`)
- Links: `text-interactive` → `text-amber`
- Border: stays `border-t` (resolves to new dark border)

**Step 4: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 5: Visual checkpoint**

Run: `pnpm dev`, check homepage for:

- Dark nav bar with amber logo and blinking cursor
- Dark footer
- Overall dark background

**Step 6: Commit**

```
feat(frontend): restyle header, logo, and footer for midnight theme
```

---

### Task 5: UI Components — terminal-panel.tsx + stat-block.tsx + data-field.tsx

**Files:**

- Modify: `frontend/components/ui/terminal-panel.tsx` (179 lines)
- Modify: `frontend/components/ui/stat-block.tsx` (176 lines)
- Modify: `frontend/components/ui/data-field.tsx` (174 lines)

**Step 1: Update terminal-panel.tsx**

Replace variant color classes. Key patterns:

- Default variant: `bg-base-surface border-base-border` (stays, new colors via tailwind)
- Elevated variant: `bg-base-elevated` (stays)
- Inset variant: `bg-base-bg shadow-glow-inset` (stays)
- Scanline overlay rgba values in inline styles: update from warm brown to cold dark
- `hover:shadow-glow` (stays, new glow via tailwind)
- Header gradient: `from-base-elevated/50 bg-gradient-to-r` → keep structure, colors auto-update
- Indicator: replace hardcoded `#3aaa80` with tailwind class `bg-mint` if inline, or verify it uses `.indicator-light` class

**Step 2: Update stat-block.tsx**

Replace color variant mappings:

- `text-emphasis` → `text-amber`
- `text-warning` → `text-amber` (same in new system)
- `text-text-primary` → `text-text-primary` (stays, resolves to `#dee2ec`)
- Trend colors: `text-positive` → `text-positive` (now mint), `text-negative` → `text-negative` (now `#e86080`)

**Step 3: Update data-field.tsx**

Replace:

- Label: `text-text-muted` stays (resolves to `#606880`)
- Value: `text-text-primary` stays (resolves to `#dee2ec`)
- Border: `border-base-border/50` stays
- Copy hover: `hover:text-interactive` → `hover:text-amber`

**Step 4: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 5: Commit**

```
feat(frontend): restyle terminal-panel, stat-block, data-field for midnight theme
```

---

### Task 6: UI Components — tabs.tsx + hex-display.tsx + hex-code.tsx + address.tsx + hash.tsx

**Files:**

- Modify: `frontend/components/ui/tabs.tsx` (90 lines)
- Modify: `frontend/components/ui/hex-display.tsx` (266 lines)
- Modify: `frontend/components/ui/hex-code.tsx` (63 lines)
- Modify: `frontend/components/ui/address.tsx` (39 lines)
- Modify: `frontend/components/ui/hash.tsx` (36 lines)

**Step 1: Update tabs.tsx**

- Active tab: `border-interactive text-interactive` → `border-amber text-amber`
- Inactive: `text-text-muted hover:text-text-secondary` stays (auto-resolves)

**Step 2: Update hex-display.tsx**

This is the most visual component. Update byte color logic:

- Base byte color: replace emphasis colors with cycling city pop palette
- Group colors: use sky, violet, amber, rose cycling
- Hover glow: `text-shadow: 0 0 6px currentColor`
- Background: ensure container uses dark surface colors

**Step 3: Update hex-code.tsx**

- Default text: `text-emphasis-dim` → `text-sky-dim` (for hash-like display)
- Bright variant: `text-emphasis` → `text-sky`
- Hover: `hover:text-emphasis` → `hover:text-amber`

**Step 4: Update address.tsx**

- Link color: `text-interactive` → `text-sky`
- Hover: `hover:underline` stays

**Step 5: Update hash.tsx**

- Pass through to hex-code, update any hardcoded color props

**Step 6: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 7: Commit**

```
feat(frontend): restyle tabs, hex display, address, hash for midnight theme
```

---

### Task 7: UI Components — charts (line-chart, pie-chart, stacked-area-chart, spark-chart, multi-series-line-chart, chart-card)

**Files:**

- Modify: `frontend/components/ui/line-chart.tsx` (705 lines)
- Modify: `frontend/components/ui/pie-chart.tsx` (156 lines)
- Modify: `frontend/components/ui/stacked-area-chart.tsx` (580 lines)
- Modify: `frontend/components/ui/spark-chart.tsx` (73 lines)
- Modify: `frontend/components/ui/multi-series-line-chart.tsx` (491 lines)
- Modify: `frontend/components/ui/chart-card.tsx` (178 lines)

**Step 1: Update chart-card.tsx**

- Background: `bg-base-surface` stays (auto dark)
- Header gradient: `from-base-elevated/50` stays
- Text: `text-text-secondary` stays
- Action hint: `text-interactive` → `text-amber`

**Step 2: Update line-chart.tsx**

Search for hardcoded color hex values and replace with imported constants from `chart-colors.ts`. Key areas:

- Canvas background fill
- Grid line stroke colors
- Axis text fill colors
- Tooltip background/border
- Primary/secondary line strokes (should already use CHART_PRIMARY_COLOR etc.)

**Step 3: Update stacked-area-chart.tsx**

Same pattern as line-chart — replace hardcoded colors with chart-colors.ts constants.

**Step 4: Update multi-series-line-chart.tsx**

Same pattern. Series colors should use `getChartPaletteColor()`.

**Step 5: Update pie-chart.tsx**

Verify it uses `getChartPaletteColor()`. Update legend text colors.

**Step 6: Update spark-chart.tsx**

Default stroke: should use `CHART_PRIMARY_COLOR` (now amber).

**Step 7: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 8: Commit**

```
feat(frontend): restyle all chart components for midnight theme
```

---

### Task 8: UI Components — remaining small components

**Files:**

- Modify: `frontend/components/ui/capacity.tsx` (36 lines)
- Modify: `frontend/components/ui/capacity-utilization.tsx` (76 lines)
- Modify: `frontend/components/ui/capacity-occupation-section.tsx` (73 lines)
- Modify: `frontend/components/ui/occupation-range-selector.tsx` (35 lines)
- Modify: `frontend/components/ui/terminal-number.tsx` (38 lines)
- Modify: `frontend/components/ui/progress-bar.tsx` (123 lines)
- Modify: `frontend/components/ui/cursor-pagination.tsx` (79 lines)
- Modify: `frontend/components/ui/pagination.tsx` (74 lines)
- Modify: `frontend/components/ui/script-view.tsx` (108 lines)
- Modify: `frontend/components/ui/page-header.tsx` (195 lines)
- Modify: `frontend/components/ui/link.tsx` (60 lines)

**Step 1: Update capacity + utilization components**

- Decimal/unit color: `text-emphasis-dim` → `text-amber-dim`
- Occupied bar: `bg-warning` → `bg-amber`
- Positive sign: `text-emphasis` → `text-mint`
- Negative sign: `text-negative` stays

**Step 2: Update occupation-range-selector.tsx**

- Active: `bg-emphasis-dim/30 text-emphasis border-emphasis-dim` → `bg-amber/15 text-amber border-amber-dim`
- Inactive: `text-text-muted hover:bg-base-elevated hover:text-text-primary` stays

**Step 3: Update terminal-number.tsx**

- Glow colors: `text-emphasis-dim` / `text-emphasis` → `text-amber-dim` / `text-amber`

**Step 4: Update progress-bar.tsx**

Replace gradient colors:

- Green: `from-emphasis-dim via-emphasis to-emphasis-bright` → `from-mint-dim via-mint to-mint`
- Amber: `from-warning-dim via-warning to-warning-bright` → `from-amber-dim via-amber to-amber`
- Blue: `from-info-dim via-info to-info-bright` → `from-sky-dim via-sky to-sky`
- Red: `from-negative-dim via-negative to-negative-bright` → `from-negative-dim via-negative to-negative`

**Step 5: Update cursor-pagination + pagination**

- Button: `hover:border-interactive hover:text-interactive` → `hover:border-amber hover:text-amber`
- Background: `bg-base-elevated text-text-secondary` stays

**Step 6: Update script-view.tsx**

- Badge: `bg-base-elevated/70 text-text-secondary` stays
- Hover: `hover:bg-base-elevated` stays
- Container: `bg-base-surface/50` stays

**Step 7: Update page-header.tsx**

- Title: `text-text-primary` stays (now `#dee2ec`)
- Hash button hover: `hover:border-base-border` stays
- Nav buttons: `hover:text-interactive hover:border-interactive-dim` → `hover:text-amber hover:border-amber-dim`

**Step 8: Update link.tsx**

Check for hardcoded color classes, replace `text-interactive` → `text-amber` if present.

**Step 9: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 10: Commit**

```
feat(frontend): restyle remaining UI components for midnight theme
```

---

### Task 9: Feature Components — home page

**Files:**

- Modify: `frontend/components/home-content.tsx`
- Modify: `frontend/components/stats-cards.tsx`
- Modify: `frontend/components/mini-stats-cards.tsx`
- Modify: `frontend/components/latest-blocks.tsx`
- Modify: `frontend/components/latest-transactions.tsx`
- Modify: `frontend/components/latest-activities.tsx`
- Modify: `frontend/components/activity-breakdown.tsx`
- Modify: `frontend/components/home-charts.tsx`
- Modify: `frontend/components/deep-fork-alert.tsx`

**Step 1: Update stats-cards.tsx**

- `terminal-card border-emphasis-dim` → `terminal-card border-base-border`
- `bg-emphasis` indicator → `bg-mint`
- `text-emphasis-dim` → `text-amber-dim`

**Step 2: Update mini-stats-cards.tsx**

- Bar colors: `bg-emphasis-dim` / `bg-emphasis` → `bg-amber-dim` / `bg-amber`
- Warning bars: `bg-warning-dim` / `bg-warning` stays (now amber anyway)

**Step 3: Update latest-blocks.tsx**

- Block number highlight color if hardcoded
- Uses TerminalPanel + TerminalRow (mostly auto-updates)

**Step 4: Update latest-activities.tsx**

Replace badge color classes:

- Coinbase: `bg-purple-900/50 text-purple-300 border-purple-700/50` → `bg-violet/10 text-violet border-violet-dim/50`
- Received: `bg-positive/10 text-positive border-positive/30` stays (mint via new tokens)
- Sent: `bg-negative/10 text-negative border-negative/30` stays
- Self: `bg-base-elevated text-text-muted border-base-border/50` stays
- Asset badges: update any `bg-base-elevated/80` inline colors

**Step 5: Update activity-breakdown.tsx**

Replace hardcoded pie chart hex colors:

- Transfer `#8ce00a` → `#5ce0b8` (mint)
- DAO Deposit `#00d7eb` → `#f0b866` (amber)
- DAO Withdraw `#ffb900` → `#c89440` (amber-dim)
- Token `#a78bfa` → `#e87ea0` (rose)
- Object `#f472b6` → `#b08af0` (violet)
- Identity `#2dd4bf` → `#6ab0e8` (sky)
- Script colors array: use city pop palette

**Step 6: Update home-charts.tsx**

Replace inline SVG stroke/fill colors with amber/rose from chart-colors.ts.

**Step 7: Update deep-fork-alert.tsx**

- `border-negative-bright bg-negative` stays (new red tones)
- `bg-negative-bright` buttons stay

**Step 8: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 9: Visual checkpoint**

Run `pnpm dev`, verify homepage:

- Dark background with CRT scanline overlay
- Amber stat values, mint/rose/violet badges
- Charts with warm amber strokes
- Blocks/activities feeds readable

**Step 10: Commit**

```
feat(frontend): restyle home page components for midnight theme
```

---

### Task 10: Feature Components — chain-wave + search + command-palette + error pages

**Files:**

- Modify: `frontend/components/chain-wave/index.tsx`
- Modify: `frontend/components/chain-wave/epoch-progress.tsx`
- Modify: `frontend/components/chain-wave/pipeline-preview.tsx`
- Modify: `frontend/components/chain-wave/packed-container.tsx` (if exists)
- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/command-palette.tsx`
- Modify: `frontend/components/error-boundary.tsx`
- Modify: `frontend/components/not-found-page.tsx`

**Step 1: Update chain-wave stage colors**

Replace stage pill colors:

- Mempool: `border-warning/30 bg-warning/10 text-warning` stays (amber via new tokens)
- Proposed: `border-positive/30 bg-positive/10 text-positive` stays (mint via new tokens)
- Committed: `border-emphasis/40 bg-emphasis/10 text-emphasis` → `border-amber/40 bg-amber/10 text-amber`

**Step 2: Update search-bar.tsx**

- Input wrapper: `bg-base-surface` stays
- Dropdown: `bg-base-surface` stays
- Result hover: `hover:bg-base-elevated/50` stays
- Selected: `bg-base-elevated/80` stays

**Step 3: Update command-palette.tsx**

- Overlay: `bg-black/50 backdrop-blur-sm` stays
- Panel: `bg-base-surface` stays
- Active item: `bg-base-elevated/50` stays

**Step 4: Update error-boundary.tsx**

- Container: `border-negative/30 bg-negative/10` stays
- Title: `text-negative` stays
- Button: `bg-negative hover:bg-negative-bright` stays

**Step 5: Update not-found-page.tsx**

- Background: `bg-base-bg` stays
- Title: `text-emphasis-dim` → `text-amber-dim`
- Button: `border-emphasis-dim bg-emphasis/10 text-emphasis` → `border-amber-dim bg-amber/10 text-amber`
- Overlay gradient: update rgba values for dark base

**Step 6: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 7: Commit**

```
feat(frontend): restyle chain-wave, search, error pages for midnight theme
```

---

### Task 11: Feature Components — charts pages + NFT components

**Files:**

- Modify: `frontend/components/charts/chart-page.tsx`
- Modify: `frontend/components/charts/chart-calculation-note.tsx`
- Modify: `frontend/components/nft/nft-collection-stat-cards.tsx`
- Modify: `frontend/components/nft/nft-activity-card.tsx`

**Step 1: Update chart-page.tsx**

- `bg-base-bg min-h-screen` stays
- Back link: `hover:text-emphasis` → `hover:text-amber`
- Warning box: `border-warning/30 bg-warning/10` stays

**Step 2: Update chart-calculation-note.tsx**

- Any `text-emphasis` or `text-interactive` → `text-amber`

**Step 3: Update NFT components**

- Stat card values: `text-warning` → stays (amber via new token)
- Title: `text-text-muted` stays
- Activity borders: `border-base-border` stays

**Step 4: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 5: Commit**

```
feat(frontend): restyle chart pages and NFT components for midnight theme
```

---

### Task 12: Grep sweep — catch remaining hardcoded light-theme colors

**Step 1: Search for old palette hex values**

```bash
cd frontend && grep -rn '#faf6f0\|#f3ede5\|#efe8de\|#e0d8cc\|#2a2520\|#6a6058\|#8a8078\|#a8a090\|#3aaa80\|#1e7a6a\|#166858\|#4a8c5c\|#c04040\|#b88420\|#3a6ea0' --include='*.tsx' --include='*.ts' --include='*.css' | grep -v node_modules | grep -v '.next'
```

Expected: No matches outside of `mood-previews.html`.

**Step 2: Search for old Tailwind class patterns**

```bash
cd frontend && grep -rn 'text-interactive\b\|border-interactive\b\|bg-interactive\b\|text-emphasis\b\|border-emphasis\b\|bg-emphasis\b' --include='*.tsx' | grep -v node_modules | grep -v '.next'
```

Review each match — some may be intentional (interactive/emphasis now map to amber in tailwind config). Fix any that should specifically be a different city pop color.

**Step 3: Fix any remaining issues**

**Step 4: Verify**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 5: Final visual checkpoint**

Run `pnpm dev` and check:

- [ ] Homepage: dark bg, amber stats, colorful badges, charts visible
- [ ] Block detail page: dark fields, sky block numbers, amber values
- [ ] Address page: sky address, amber balance, tabs work
- [ ] Charts page: dark cards, visible chart lines on dark background
- [ ] DAO page: dark theme consistent
- [ ] 404 page: dark theme
- [ ] Search: dark overlay, readable results
- [ ] Mobile: hamburger menu works, no white flashes

**Step 6: Commit**

```
feat(frontend): sweep and fix remaining light-theme color references
```

---

### Task 13: Cleanup — remove preview file + run full test suite

**Step 1: Remove mood preview**

Delete `frontend/public/mood-previews.html` (served its purpose as design reference).

**Step 2: Run full frontend checks**

```bash
cd frontend && pnpm type-check && pnpm lint && npx vitest run
```

Expected: All pass. Fix any failures from color-dependent test snapshots.

**Step 3: Run Prettier**

```bash
pnpm format
```

**Step 4: Final commit**

```
chore(frontend): remove design preview and format
```
