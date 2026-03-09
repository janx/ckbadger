# UI/UX Design System Overhaul — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current multi-role terminal color system with a strict role-based Argonaut-inspired palette, increase information density, and refine CRT atmosphere effects.

**Architecture:** 4-layer color system (atmosphere/interactive/emphasis/semantic) implemented through Tailwind config tokens and CSS variables. All hardcoded hex values centralized. Spacing compressed ~25% toward terminal density.

**Tech Stack:** Tailwind CSS 3.4, CSS custom properties, React/TypeScript components

---

### Task 1: Foundation — Tailwind Config Color Tokens

**Files:**

- Modify: `frontend/tailwind.config.ts:8-56`

**Step 1: Replace color definitions**

Replace the entire `colors` object (lines 8-41) with the new Argonaut-derived role-based palette:

```typescript
colors: {
  // Atmosphere layer (~78%)
  base: {
    bg: '#0d0f18',
    surface: '#12151e',
    elevated: '#181c27',
    border: '#1f2430',
  },
  text: {
    primary: '#fffaf3',
    secondary: '#c8c2b8',
    muted: '#6b6860',
    dim: '#4a4740',
  },
  // Interactive layer (~12%)
  interactive: {
    DEFAULT: '#00d7eb',
    hover: '#67ffef',
    muted: '#00d7eb40',
    dim: '#009aa8',
  },
  // Emphasis layer (~7%)
  emphasis: {
    DEFAULT: '#8ce00a',
    dim: '#6ba808',
    glow: '#8ce00a30',
    bright: '#abe05a',
  },
  // Semantic layer (~3%)
  positive: {
    DEFAULT: '#8ce00a',
    dim: '#6ba808',
  },
  negative: {
    DEFAULT: '#ff000f',
    dim: '#cc000c',
    bright: '#ff273f',
  },
  warning: {
    DEFAULT: '#ffb900',
    dim: '#cc8c00',
    bright: '#ffd141',
  },
  info: {
    DEFAULT: '#008df8',
    dim: '#006bc0',
    bright: '#0092ff',
  },
},
```

**Step 2: Replace boxShadow definitions**

Replace boxShadow (lines 50-56) with lime-green glow variants:

```typescript
boxShadow: {
  'glow': '0 0 4px #8ce00a25, 0 0 10px #8ce00a15',
  'glow-strong': '0 0 3px #8ce00a50, 0 0 8px #8ce00a25',
  'glow-inset': 'inset 0 1px 4px #8ce00a10',
  'interactive-glow': '0 0 4px #00d7eb30, 0 0 10px #00d7eb18',
},
```

**Step 3: Verify build**

Run: `cd frontend && npx tailwindcss --help` (just confirm tailwind parses config)
Then: `pnpm build` — expect build warnings about removed color tokens but no crashes.

**Step 4: Commit**

```
feat(frontend): replace color tokens with Argonaut role-based palette
```

---

### Task 2: Foundation — CSS Variables and Base Styles

**Files:**

- Modify: `frontend/app/globals.css:5-24,36-46,48-87,148-181,359-411`

**Step 1: Replace CSS variables** (lines 5-15)

```css
:root {
  --foreground: #fffaf3;
  --background: #0d0f18;
  --color-surface: #12151e;
  --color-elevated: #181c27;
  --color-border: #1f2430;
  --color-emphasis: #8ce00a;
  --color-emphasis-dim: #6ba808;
  --color-interactive: #00d7eb;
  --color-interactive-hover: #67ffef;
  --font-mono: 'JetBrains Mono', 'SFMono-Regular', 'Roboto Mono', ui-monospace, monospace;
  --font-display: var(--font-mono);
}
```

**Step 2: Update body styles** (lines 17-24)

Change `background` to use `--background` (should already), ensure the fractal noise overlay is tinted navy not gray.

**Step 3: Update terminal-text classes** (lines 36-46)

Replace phosphor green (#00ff41) references with emphasis lime (#8ce00a):

```css
.terminal-text {
  color: #8ce00a;
  text-shadow: 0 0 4px #8ce00a40;
}
.terminal-text-dim {
  color: #6ba808;
  text-shadow: 0 0 3px #6ba80830;
}
```

**Step 4: Reduce scanline overlay opacity** (lines 77-87)

Change the scanline `background` alpha from current to ~0.015 opacity. Keep animation.

**Step 5: Update indicator-light colors** (lines 148-165)

Replace green glow with emphasis lime. Keep amber variant.

```css
.indicator-light {
  background: #8ce00a;
  box-shadow:
    0 0 4px #8ce00a80,
    0 0 8px #8ce00a40;
}
```

**Step 6: Update .terminal-card** (lines 359-412)

Replace all phosphor green glow references with emphasis lime. Update border colors to use `--color-border`.

**Step 7: Update .terminal-border-glow** (lines 390-411)

Replace gradient colors from terminal-green to emphasis lime.

**Step 8: Commit**

```
feat(frontend): update CSS variables and effects for Argonaut palette
```

---

### Task 3: Foundation — Chart Colors Centralization

**Files:**

- Modify: `frontend/lib/chart-colors.ts`

**Step 1: Update chart color constants**

```typescript
export const CHART_PRIMARY_COLOR = '#8ce00a'; // Emphasis lime (was #00ff41)
export const CHART_SECONDARY_COLOR = '#00d7eb'; // Interactive cyan (was #ffb000)
export const CHART_TERTIARY_COLOR = '#ffb900'; // Warning amber (was #00c389)
```

Update CHART_PALETTE array to use new Argonaut-derived colors:

```typescript
export const CHART_PALETTE = [
  '#8ce00a', // Emphasis lime
  '#00d7eb', // Interactive cyan
  '#ffb900', // Warning amber
  '#008df8', // Info blue
  '#ff000f', // Negative red
  '#9a5feb', // Argonaut bright magenta
  '#67ffef', // Bright cyan
  '#abe05a', // Bright lime
  '#ffd141', // Bright amber
  '#0092ff', // Bright blue
  '#6c43a5', // Argonaut magenta
  '#6b6860', // Muted
];
```

Also add centralized chart UI colors:

```typescript
export const CHART_GRID_COLOR = '#1f2430'; // Matches base.border
export const CHART_AXIS_COLOR = '#4a4740'; // Matches text.dim
export const CHART_TOOLTIP_BG = '#12151e'; // Matches base.surface
export const CHART_TOOLTIP_BORDER = '#1f2430'; // Matches base.border
export const CHART_HOVER_COLOR = '#4a4740'; // Matches text.dim
```

**Step 2: Commit**

```
feat(frontend): centralize chart colors with Argonaut palette
```

---

### Task 4: Core UI — TerminalPanel Component

**Files:**

- Modify: `frontend/components/ui/terminal-panel.tsx`

**Step 1: Update TerminalPanel variants** (lines 22-24)

```
default:  bg-slate-900 border-slate-800     → bg-base-surface border-base-border
elevated: bg-slate-850 border-slate-700     → bg-base-elevated border-base-border
inset:    bg-slate-950 border-slate-800     → bg-base-bg border-base-border
```

**Step 2: Update hover glow** (line 32)

```
hover:shadow-terminal-glow → hover:shadow-glow
```

**Step 3: Update TerminalPanelHeader** (lines 74-82)

```
gap-3 px-4 py-3 → gap-3 px-3 py-2                     (density)
border-b border-slate-800 → border-b border-base-border
from-slate-850/50 → from-base-elevated/50              (gradient)
text-slate-400 → text-text-muted                       (text color)
```

**Step 4: Update TerminalPanelContent** (lines 107-109)

```
p-2 → p-2  (sm stays)
p-4 → p-3  (md compressed)
p-6 → p-4  (lg compressed)
```

**Step 5: Update TerminalPanelFooter** (lines 124-125)

```
px-4 py-3 → px-3 py-2
border-t border-slate-800 → border-t border-base-border
to-slate-850/30 → to-base-elevated/30
```

**Step 6: Update TerminalDivider** (line 144)

```
text-slate-500 → text-text-muted
```

**Step 7: Update TerminalRow** (lines 171-172)

```
border-b border-slate-800/50 px-4 py-3 → border-b border-base-border/50 px-3 py-2
hover:bg-slate-850/50 → hover:bg-base-elevated/50
```

**Step 8: Commit**

```
feat(frontend): update TerminalPanel to role-based colors and denser spacing
```

---

### Task 5: Core UI — StatBlock Component

**Files:**

- Modify: `frontend/components/ui/stat-block.tsx`

**Step 1: Update color variants** (lines 33-35)

```
green: text-terminal-green → text-emphasis
amber: text-amber → text-warning
white: text-white → text-text-primary
```

**Step 2: Update trend colors** (lines 54-56)

```
up: text-terminal-green → text-positive
down: text-red-400 → text-negative
neutral: text-slate-500 → text-text-muted
```

**Step 3: Update label color** (line 77)

```
text-slate-500 → text-text-muted
```

**Step 4: Update StatGrid gap** (line 125)

```
gap-6 → gap-4
```

**Step 5: Update StatDivider gradient** (lines 138, 148)

```
via-slate-700 → via-base-border
```

**Step 6: Update MiniStat colors** (lines 164-167)

```
green: text-terminal-green → text-emphasis
amber: text-amber → text-warning
white: text-white → text-text-primary
muted: text-slate-400 → text-text-secondary
```

Label: `text-slate-500 → text-text-muted`

**Step 7: Commit**

```
feat(frontend): update StatBlock to role-based colors
```

---

### Task 6: Core UI — PageHeader Component

**Files:**

- Modify: `frontend/components/ui/page-header.tsx`

**Step 1: Reduce bottom margin** (line 39)

```
mb-8 → mb-4
```

**Step 2: Update nav button colors** (line 45)

```
border-slate-800 text-slate-400 → border-base-border text-text-muted
hover:text-terminal-green hover:border-terminal-dark → hover:text-interactive hover:border-interactive-dim
```

**Step 3: Update title color** (line 54)

```
text-white → text-text-primary
```

**Step 4: Update subtitle** (line 57)

```
text-slate-500 → text-text-muted
```

**Step 5: Update hash display** (lines 78-86)

```
border-slate-800 bg-slate-900/50 → border-base-border bg-base-surface/50
text-slate-400 → text-text-secondary
text-terminal-green (copied) → text-emphasis
```

**Step 6: Update badge variants** (lines 104-110)

```
green:   bg-green-900/50 text-green-400 → bg-positive/10 text-positive
amber:   bg-amber-900/50 text-amber → bg-warning/10 text-warning
red:     bg-red-900/50 text-red-400 → bg-negative/10 text-negative
gray:    bg-slate-800 text-slate-400 → bg-base-elevated text-text-muted
neutral: bg-slate-800/70 text-slate-300 → bg-base-elevated/70 text-text-secondary
blue:    bg-slate-800/70 text-slate-300 → bg-info/10 text-info
purple:  bg-slate-800/70 text-slate-300 → bg-info/10 text-info-bright
```

**Step 7: Commit**

```
feat(frontend): update PageHeader to role-based colors and reduced spacing
```

---

### Task 7: Core UI — DataField Component

**Files:**

- Modify: `frontend/components/ui/data-field.tsx`

**Step 1: Update vertical layout** (lines 38-55)

```
text-slate-500 (label) → text-text-muted
text-white (value) → text-text-primary
hover:text-terminal-green → hover:text-interactive
text-terminal-green (copied) → text-emphasis
```

**Step 2: Update horizontal layout** (lines 64-90)

```
border-b border-slate-800/50 py-3 → border-b border-base-border/50 py-2
text-slate-500 (label) → text-text-muted
text-white (value) → text-text-primary
hover:text-terminal-green → hover:text-interactive
text-terminal-green (copied) → text-emphasis
text-slate-500 (icon) → text-text-muted
group-hover:text-terminal-green → group-hover:text-interactive
```

**Step 3: Update DataSection** (line 125)

```
border-b border-slate-800 → border-b border-base-border
```

**Step 4: Commit**

```
feat(frontend): update DataField to role-based colors and reduced padding
```

---

### Task 8: Core UI — ChartCard Component

**Files:**

- Modify: `frontend/components/ui/chart-card.tsx`

**Step 1: Update card container** (lines 29-31)

```
border-slate-800 bg-slate-900 → border-base-border bg-base-surface
hover:bg-slate-850 hover:border-slate-700 → hover:bg-base-elevated hover:border-base-border
```

**Step 2: Update header** (lines 35-38)

```
border-b border-slate-800 from-slate-850/50 → border-b border-base-border from-base-elevated/50
text-slate-300 → text-text-secondary
text-terminal-green (VIEW) → text-interactive
```

**Step 3: Update content** (lines 44-51)

```
p-4 → p-3
bg-slate-800 (skeleton) → bg-base-elevated
```

**Step 4: Update ChartSection** (lines 84-89)

```
mb-10 → mb-6
text-white → text-text-primary
bg-terminal-green (dot) → bg-emphasis
gap-6 → gap-4
```

**Step 5: Update FilterButtonGroup** (lines 146-155)

```
bg-terminal-green text-slate-950 (active) → bg-emphasis text-base-bg
bg-slate-800 text-slate-400 (inactive) → bg-base-elevated text-text-muted
hover:bg-slate-700 hover:text-slate-300 → hover:bg-base-border hover:text-text-secondary
```

**Step 6: Commit**

```
feat(frontend): update ChartCard to role-based colors and reduced spacing
```

---

### Task 9: Layout — Header

**Files:**

- Modify: `frontend/components/layout/header.tsx`

**Step 1: Update header container** (line 24)

```
border-slate-800/80 bg-slate-950/85 → border-base-border/80 bg-base-bg/85
```

**Step 2: Update active nav link** (lines 40-41)

```
border-terminal-green/50 bg-terminal-green/12 text-terminal-green
  → border-interactive/50 bg-interactive/12 text-interactive
shadow-[inset_0_0_0_1px_rgba(74,222,128,0.18)]
  → shadow-[inset_0_0_0_1px_rgba(0,215,235,0.18)]
```

**Step 3: Update inactive nav link** (line 42)

```
text-slate-300/85 → text-text-secondary/85
hover:border-slate-700/80 hover:bg-slate-800/35 hover:text-slate-100
  → hover:border-base-border/80 hover:bg-base-elevated/35 hover:text-text-primary
```

**Step 4: Update mobile menu button** (line 54)

```
text-slate-400 hover:text-terminal-green hover:bg-slate-800
  → text-text-muted hover:text-interactive hover:bg-base-elevated
```

**Step 5: Update mobile menu** (lines 81-91)

```
border-slate-800/80 bg-slate-950/95 → border-base-border/80 bg-base-bg/95
```

Mobile nav links follow same active/inactive pattern as desktop.

**Step 6: Commit**

```
feat(frontend): update header nav to interactive cyan color role
```

---

### Task 10: Layout — Footer

**Files:**

- Modify: `frontend/components/layout/site-footer.tsx`

**Step 1: Update footer colors** (lines 10-49)

```
border-slate-800/90 bg-slate-950/95 → border-base-border/90 bg-base-bg/95
border-slate-800/90 bg-slate-900/70 → border-base-border/90 bg-base-surface/70
bg-slate-800/80 → bg-base-border/80
bg-slate-950/95 → bg-base-bg/95
text-slate-500 → text-text-muted
text-slate-200 → text-text-primary
text-slate-300 → text-text-secondary
text-terminal-green → text-interactive
hover:text-green-300 → hover:text-interactive-hover
text-slate-400 → text-text-muted
hover:text-slate-200 → hover:text-text-primary
decoration-slate-700/80 → decoration-base-border/80
hover:decoration-terminal-dark → hover:decoration-interactive-dim
```

**Step 2: Commit**

```
feat(frontend): update footer to role-based colors
```

---

### Task 11: Homepage — Content Layout and Sections

**Files:**

- Modify: `frontend/components/home-content.tsx`
- Modify: `frontend/components/home-charts.tsx`
- Modify: `frontend/components/home-stats.tsx` (if exists, may be MiniStatsCards)

**Step 1: Update home-content.tsx spacing** (line 39, 42, 50, 62, 66, 70)

```
py-6 sm:py-8 → py-4 sm:py-6
mt-6 → mt-4  (all section gaps)
mt-8 → mt-5  (final grid gap)
gap-6 → gap-4 (grid gaps)
```

Update live indicator colors (lines 87-88):

```
bg-slate-900/90 → bg-base-surface/90
text-terminal-green border-terminal-dark/50 → text-emphasis border-emphasis-dim/50
```

**Step 2: Update home-charts.tsx**

```
gap-4 → gap-3 (grid)
border-slate-800 bg-slate-900 → border-base-border bg-base-surface
text-terminal-green → text-emphasis
text-amber → text-warning
```

Replace inline SVG stroke colors:

```
#334155 → use CHART_GRID_COLOR from chart-colors.ts
```

**Step 3: Commit**

```
feat(frontend): update homepage layout density and colors
```

---

### Task 12: Homepage — Latest Blocks and Transactions

**Files:**

- Modify: `frontend/components/latest-blocks.tsx`
- Modify: `frontend/components/latest-transactions.tsx`

**Step 1: Update latest-blocks.tsx**

Row highlight (lines 96-97):

```
bg-terminal-green/10 shadow-terminal-glow → bg-emphasis/10 shadow-glow
```

Text colors (lines 106-107):

```
text-slate-500 → text-text-muted
text-terminal-green → text-emphasis (block numbers as values)
```

Clickable block links: → `text-interactive` (if they are links)

Hardfork badge (line 117):

```
border-amber-900/60 bg-amber-900/30 text-amber-300 → border-warning-dim/60 bg-warning/10 text-warning
```

Header action link:

```
text-slate-500 hover:text-terminal-green → text-text-muted hover:text-interactive
```

**Step 2: Update latest-transactions.tsx**

Same pattern: replace `text-amber` with `text-interactive` for clickable tx hashes, `bg-amber/10 shadow-amber-glow` with `bg-interactive/10 shadow-interactive-glow` for highlights.

```
text-terminal-dim → text-emphasis-dim
text-amber-dim → text-warning-dim (or text-interactive-dim for links)
text-slate-500 → text-text-muted
hover:text-amber → hover:text-interactive
```

**Step 3: Commit**

```
feat(frontend): update latest blocks/transactions to role-based colors
```

---

### Task 13: Homepage — Latest Activities

**Files:**

- Modify: `frontend/components/latest-activities.tsx`

**Step 1: Update type badges** (lines 18-41)

```
Coinbase:  bg-purple-900/50 text-purple-300 border-purple-700/50  → bg-info/10 text-info border-info-dim/50
Received:  bg-emerald-900/50 text-emerald-300 border-emerald-700/50  → bg-positive/10 text-positive border-positive-dim/50
Sent:      bg-red-900/50 text-red-300 border-red-700/50  → bg-negative/10 text-negative border-negative-dim/50
Self:      bg-slate-800 text-slate-400 border-slate-700/50  → bg-base-elevated text-text-muted border-base-border/50
```

**Step 2: Update asset badges** (line 53)

```
border-slate-700/60 bg-slate-800/80 → border-base-border/60 bg-base-elevated/80
```

**Step 3: Update activity highlight** (lines 185-186)

```
bg-cyan-500/10 shadow-[0_0_8px_rgba(6,182,212,0.15)]
  → bg-interactive/10 shadow-interactive-glow
```

**Step 4: Update CKB delta colors** (line 255)

```
text-emerald-400 (positive) → text-positive
text-red-400 (negative) → text-negative
```

**Step 5: Update clickable elements**

Address links, tx hash links, block links → `text-interactive`

**Step 6: Commit**

```
feat(frontend): update latest activities to role-based colors
```

---

### Task 14: Chart Components — Inline Hex Replacements

**Files:**

- Modify: `frontend/components/ui/line-chart.tsx`
- Modify: `frontend/components/ui/multi-series-line-chart.tsx`
- Modify: `frontend/components/ui/stacked-area-chart.tsx`
- Modify: `frontend/components/ui/pie-chart.tsx`
- Modify: `frontend/components/ui/spark-chart.tsx`

**Step 1: Import chart UI colors**

In each chart component, import from chart-colors.ts:

```typescript
import {
  CHART_GRID_COLOR,
  CHART_TOOLTIP_BG,
  CHART_TOOLTIP_BORDER,
  CHART_HOVER_COLOR,
} from '@/lib/chart-colors';
```

**Step 2: Replace hardcoded hex values**

For all chart components:

```
#334155, #374151 (grid lines) → CHART_GRID_COLOR
#475569, #6b7280 (hover lines) → CHART_HOVER_COLOR
#0f172a, #111827 (tooltip bg) → CHART_TOOLTIP_BG
#334155 (tooltip border) → CHART_TOOLTIP_BORDER
#111827 (pie center) → CHART_TOOLTIP_BG
```

For line-chart.tsx marker color:

```
#f59e0b → keep as is (this is a specific marker color, or import from semantic)
```

**Step 3: Update spark-chart.tsx default color**

```
#10b981 → CHART_PRIMARY_COLOR (#8ce00a)
```

**Step 4: Commit**

```
feat(frontend): centralize chart hex colors through chart-colors.ts
```

---

### Task 15: Detail Pages — Block, Transaction, Address

**Files:**

- Modify: `frontend/app/blocks/[id]/client-page.tsx`
- Modify: `frontend/app/tx/[hash]/client-page.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: Block detail page**

```
min-h-screen bg-slate-950 → min-h-screen bg-base-bg
container mx-auto max-w-5xl px-4 py-8 → container mx-auto max-w-5xl px-4 py-4
mb-6 (panel spacing) → mb-4
border-amber-500/30 (hardfork) → border-warning/30
```

Tabs styling:

```
data-[state=active]:border-terminal-green data-[state=active]:text-terminal-green
  → data-[state=active]:border-interactive data-[state=active]:text-interactive
```

Loading skeleton:

```
bg-slate-800 → bg-base-elevated
```

**Step 2: Transaction detail page**

Update witness segment color palettes (lines 48-96) to use Argonaut colors:

```
green palette: use emphasis (#8ce00a variants)
cyan palette: use interactive (#00d7eb variants)
amber palette: use warning (#ffb900 variants)
fuchsia palette: use info (#008df8 / #9a5feb variants)
```

Graph loading:

```
border-slate-700/70 bg-slate-900/70 → border-base-border/70 bg-base-surface/70
```

All `text-terminal-green` → context-dependent (`text-emphasis` for values, `text-interactive` for links)
All `text-amber` → context-dependent
All `bg-slate-*` → corresponding `bg-base-*`
All `border-slate-*` → `border-base-*`
All `text-slate-*` → corresponding `text-text-*`

**Step 3: Address detail page**

Same systematic replacement as block detail. Additional:

- Activity filter buttons: follow FilterButtonGroup pattern from ChartCard
- Stat blocks: already handled by StatBlock component update
- Tab triggers: same as block detail

**Step 4: Commit**

```
feat(frontend): update detail pages to role-based colors and spacing
```

---

### Task 16: DAO and Charts Pages

**Files:**

- Modify: `frontend/app/dao/page.tsx`
- Modify: `frontend/app/charts/page.tsx`
- Modify: Chart subpages with hardcoded colors

**Step 1: DAO page**

```
text-terminal-green (stat values) → text-emphasis
container mx-auto px-4 py-8 → container mx-auto px-4 py-4
gap-8 md:grid-cols-2 → gap-4 md:grid-cols-2
```

Pie chart hex colors:

```
#00ff41 → CHART_PRIMARY_COLOR (#8ce00a)
#ffb000 → CHART_SECONDARY_COLOR (#00d7eb) or CHART_TERTIARY_COLOR
#3d4a5c → text.muted equivalent
```

Table styling:

```
border-slate-800 → border-base-border
hover:bg-slate-850/50 → hover:bg-base-elevated/50
```

**Step 2: Charts index page**

```
container mx-auto px-4 py-8 → container mx-auto px-4 py-4
```

Warning banner:

```
border-yellow-500/30 bg-yellow-500/10 → border-warning/30 bg-warning/10
```

Hardfork event markers:

```
#f59e0b (activated) → warning.DEFAULT (#ffb900)
#38bdf8 (upcoming) → interactive.DEFAULT (#00d7eb)
#00c389 (CKB teal) → emphasis.DEFAULT (#8ce00a)
```

**Step 3: Update chart subpages with hardcoded colors**

- `charts/hodl-wave/page.tsx`: `#ec4899` → keep or use info palette
- `charts/epoch-time-length/page.tsx`: `#f59e0b` → `#ffb900`, `#38bdf8` → `#00d7eb`
- `charts/knowledge-size/page.tsx`: `#f59e0b` → `#8ce00a`
- `charts/miner-address-distribution/page.tsx`: Replace COLORS array with CHART_PALETTE import

**Step 4: Commit**

```
feat(frontend): update DAO and charts pages to role-based colors
```

---

### Task 17: Remaining Pages — Sweep

**Files:**

- All remaining `client-page.tsx` files under `frontend/app/`

**Step 1: Systematic search-and-replace across all pages**

Use grep to find all remaining instances of old color tokens:

```bash
cd frontend && grep -rn 'text-terminal-green\|text-amber\|bg-slate-\|border-slate-\|text-slate-\|text-white\|shadow-terminal' --include='*.tsx' app/ components/
```

Apply the consistent mapping:

```
text-terminal-green → text-emphasis (values) or text-interactive (links)
text-amber → text-warning (values) or text-interactive (links)
text-white → text-text-primary
text-slate-300 → text-text-secondary
text-slate-400 → text-text-secondary (or text-text-muted for very dim)
text-slate-500 → text-text-muted
text-slate-600/700 → text-text-dim
bg-slate-950 → bg-base-bg
bg-slate-900 → bg-base-surface
bg-slate-850 → bg-base-elevated
bg-slate-800 → bg-base-border (or bg-base-elevated)
border-slate-800 → border-base-border
border-slate-700 → border-base-border
shadow-terminal-glow → shadow-glow
shadow-terminal-glow-strong → shadow-glow-strong
shadow-terminal-inset → shadow-glow-inset
shadow-amber-glow → shadow-interactive-glow
```

**Step 2: Reduce py-8 → py-4 and mt-6 → mt-4 across page containers**

**Step 3: Commit**

```
feat(frontend): sweep remaining pages for role-based color tokens
```

---

### Task 18: HexDisplay and Specialized Components

**Files:**

- Modify: `frontend/components/ui/hex-display.tsx`
- Modify: `frontend/components/ui/hash.tsx`
- Modify: `frontend/components/ui/address.tsx`
- Modify: `frontend/components/ui/capacity.tsx`
- Modify: `frontend/components/ui/progress-bar.tsx`
- Modify: `frontend/components/ui/tabs.tsx`
- Modify: `frontend/components/ui/pagination.tsx`
- Modify: `frontend/components/ui/cursor-pagination.tsx`

**Step 1: hex-display.tsx**

Replace color variants:

```
green → emphasis
amber → warning
white → text-primary
cyan → interactive
```

**Step 2: hash.tsx, address.tsx**

These are display components — update any `text-terminal-green` or `text-amber` to role-appropriate colors. Linked hashes → `text-interactive`.

**Step 3: capacity.tsx**

CKB amounts → `text-emphasis`

**Step 4: progress-bar.tsx**

```
green color → emphasis
```

**Step 5: tabs.tsx**

Active tab triggers:

```
text-terminal-green border-terminal-green → text-interactive border-interactive
```

**Step 6: pagination.tsx, cursor-pagination.tsx**

Active page / nav buttons:

```
text-terminal-green bg-terminal-green/15 → text-interactive bg-interactive/15
border-slate-800 → border-base-border
text-slate-400 → text-text-muted
hover:text-terminal-green → hover:text-interactive
```

**Step 7: Commit**

```
feat(frontend): update remaining UI components to role-based colors
```

---

### Task 19: Tailwind Config Cleanup — Remove Old Tokens

**Files:**

- Modify: `frontend/tailwind.config.ts`

**Step 1: Remove old color tokens**

After all components are migrated, remove the old unused color namespaces from tailwind config:

- `ckb` (primary, secondary, dark, darker)
- `terminal` (green, dim, dark, glow, bg, bg-light)
- `amber` (DEFAULT, bright, dim, dark)
- `slate` custom extensions (950, 900, 850, 800, 700, 600, 500)

Keep only the new role-based tokens.

**Step 2: Remove old boxShadow names**

Remove `terminal-glow`, `terminal-glow-strong`, `terminal-inset`, `amber-glow`, `amber-glow-strong`.

**Step 3: Build and verify**

Run: `cd frontend && pnpm build`
Expected: Clean build with no references to removed tokens.

Run: `cd frontend && pnpm lint && pnpm type-check`
Expected: All pass.

**Step 4: Commit**

```
refactor(frontend): remove deprecated color tokens from tailwind config
```

---

### Task 20: Visual Verification

**Step 1: Start dev server and verify all pages**

Run: `cd frontend && pnpm dev`

Check each page category:

- [ ] Homepage — stats, charts, latest blocks/txs/activities
- [ ] Block list and block detail
- [ ] Transaction detail (including witness viewer)
- [ ] Address detail (all tabs)
- [ ] DAO page with charts
- [ ] Charts index and individual chart pages
- [ ] Scripts, tokens, NFTs pages
- [ ] Header navigation (active states, hover)
- [ ] Footer
- [ ] Mobile responsive (narrow viewport)

Verify:

- [ ] No remaining phosphor green (#00ff41)
- [ ] Cyan used consistently for all interactive/link elements
- [ ] Lime green used for data emphasis only
- [ ] Warm off-white text, not pure white
- [ ] Navy-tinted backgrounds
- [ ] Tighter spacing throughout
- [ ] CRT effects present but subtle

**Step 2: Fix any visual issues found**

**Step 3: Final commit**

```
fix(frontend): visual polish from design system verification
```

---

## Summary

| Task | Scope                       | Commit                                             |
| ---- | --------------------------- | -------------------------------------------------- |
| 1    | Tailwind color tokens       | `feat: replace color tokens with Argonaut palette` |
| 2    | CSS variables and effects   | `feat: update CSS variables and effects`           |
| 3    | Chart colors centralization | `feat: centralize chart colors`                    |
| 4    | TerminalPanel               | `feat: update TerminalPanel`                       |
| 5    | StatBlock                   | `feat: update StatBlock`                           |
| 6    | PageHeader                  | `feat: update PageHeader`                          |
| 7    | DataField                   | `feat: update DataField`                           |
| 8    | ChartCard                   | `feat: update ChartCard`                           |
| 9    | Header nav                  | `feat: update header`                              |
| 10   | Footer                      | `feat: update footer`                              |
| 11   | Homepage layout             | `feat: update homepage`                            |
| 12   | Latest blocks/txs           | `feat: update latest blocks/txs`                   |
| 13   | Latest activities           | `feat: update latest activities`                   |
| 14   | Chart components            | `feat: centralize chart hex`                       |
| 15   | Detail pages                | `feat: update detail pages`                        |
| 16   | DAO and charts pages        | `feat: update DAO and charts`                      |
| 17   | Remaining pages sweep       | `feat: sweep remaining pages`                      |
| 18   | Specialized UI components   | `feat: update remaining UI components`             |
| 19   | Remove old tokens           | `refactor: remove deprecated tokens`               |
| 20   | Visual verification         | `fix: visual polish`                               |
