# 青绿暖白 Light Theme Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the "Citypop Midnight" dark theme with a traditional Chinese cyan-green (青绿) light theme on warm white (#faf6f0) background, preserving terminal-style CRT effects.

**Architecture:** Three-layer approach: (1) swap token foundation files (tailwind config, CSS vars, chart colors), (2) update layout and components with hardcoded colors, (3) audit all files using standard Tailwind color classes. All semantic Tailwind classes (text-emphasis, bg-base-surface, etc.) auto-update when tokens change.

**Tech Stack:** Tailwind CSS 3.4, CSS custom properties, React/TypeScript components

**Design spec:** `docs/plans/2026-03-10-qinglv-light-theme-design.md`

---

### Task 1: Swap Tailwind color tokens

**Files:**

- Modify: `frontend/tailwind.config.ts`

**Step 1: Run existing tests to establish baseline**

Run: `cd frontend && npx vitest run 2>&1 | tail -5`
Expected: All tests pass

**Step 2: Replace color tokens and box shadows**

Replace the entire `colors` object (lines 8-52) and `boxShadow` object (lines 61-66) in `frontend/tailwind.config.ts`:

```typescript
colors: {
  // 青绿暖白 palette (Qinglv Light)
  base: {
    bg: '#faf6f0',       // 宣纸白
    surface: '#f3ede5',  // 素纸
    elevated: '#efe8de', // 绢白
    border: '#e0d8cc',   // 麻色
  },
  text: {
    primary: '#2a2520',
    secondary: '#7a7068',
    muted: '#b0a898',
    dim: '#d0c8bc',
  },
  interactive: {
    DEFAULT: '#3aaa80',
    hover: '#2cc878',
    muted: '#3aaa8020',
    dim: '#2d8a68',
  },
  emphasis: {
    DEFAULT: '#1e7a6a',
    dim: '#166858',
    glow: '#1e7a6a30',
    bright: '#28a088',
  },
  positive: {
    DEFAULT: '#4a8c5c', // 竹青
    dim: '#3a7048',
  },
  negative: {
    DEFAULT: '#c04040', // 朱红
    dim: '#a03535',
    bright: '#d45050',
  },
  warning: {
    DEFAULT: '#b88420', // 琥珀
    dim: '#9a6e1a',
    bright: '#d4a030',
  },
  info: {
    DEFAULT: '#3a6ea0', // 靛青
    dim: '#2e5a84',
    bright: '#4a88c0',
  },
},
```

Replace boxShadow:

```typescript
boxShadow: {
  glow: '0 0 6px #1e7a6a18, 0 0 14px #1e7a6a10',
  'glow-strong': '0 0 5px #1e7a6a28, 0 0 12px #1e7a6a18',
  'glow-inset': 'inset 0 1px 6px #1e7a6a08',
  'interactive-glow': '0 0 6px #3aaa8020, 0 0 14px #3aaa8010',
},
```

Also update the comment on line 9 from `// Citypop Midnight palette` to `// 青绿暖白 palette (Qinglv Light)`.

Also update the comment on line 120 from `// Japanese poster inspired spacing rhythm` to just remove the comment or leave it.

**Step 3: Verify build**

Run: `cd frontend && npx tsc --noEmit 2>&1 | tail -5`
Expected: No errors

**Step 4: Commit**

```bash
git add frontend/tailwind.config.ts
git commit -m "feat(theme): swap Tailwind color tokens to 青绿暖白 palette"
```

---

### Task 2: Update CSS variables and globals.css

**Files:**

- Modify: `frontend/app/globals.css`

**Step 1: Replace CSS custom properties (lines 5-17)**

Replace `:root` block:

```css
:root {
  --foreground: #2a2520;
  --background: #faf6f0;
  --color-surface: #f3ede5;
  --color-elevated: #efe8de;
  --color-border: #e0d8cc;
  --color-emphasis: #1e7a6a;
  --color-emphasis-dim: #166858;
  --color-interactive: #3aaa80;
  --color-interactive-hover: #2cc878;
  --font-mono: 'JetBrains Mono', 'SFMono-Regular', 'Roboto Mono', ui-monospace, monospace;
  --font-display: 'Space Grotesk', 'Inter', system-ui, sans-serif;
}
```

**Step 2: Update body background noise blend (lines 19-26)**

The SVG noise texture needs to work on light background. Update body:

```css
body {
  color: var(--foreground);
  background: var(--background);
  min-height: 100vh;
  background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E");
  background-blend-mode: soft-light;
  background-size: 128px 128px;
}
```

Change `overlay` → `soft-light` (softer noise on light background).

**Step 3: Update terminal-text utility classes (lines 38-46)**

```css
.terminal-text {
  color: #1e7a6a;
  text-shadow: 0 0 4px #1e7a6a30;
}

.terminal-text-dim {
  color: #166858;
  text-shadow: 0 0 3px #16685820;
}
```

**Step 4: Update CRT screen effect (lines 48-75)**

Update `.crt-screen::before` scanline color for light bg:

```css
.crt-screen::before {
  content: '';
  position: absolute;
  inset: 0;
  background: repeating-linear-gradient(
    0deg,
    rgba(42, 37, 32, 0.04) 0px,
    rgba(42, 37, 32, 0.04) 1px,
    transparent 1px,
    transparent 2px
  );
  pointer-events: none;
  z-index: 10;
}
```

Update `.crt-screen::after` vignette for light bg:

```css
.crt-screen::after {
  content: '';
  position: absolute;
  inset: 0;
  background: radial-gradient(ellipse at center, transparent 0%, rgba(42, 37, 32, 0.06) 100%);
  pointer-events: none;
  z-index: 10;
}
```

**Step 5: Update scanline-overlay (line 83)**

```css
.scanline-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 4px;
  background: linear-gradient(to bottom, transparent, rgba(30, 122, 106, 0.008), transparent);
  animation: scan-line 8s linear infinite;
  pointer-events: none;
  z-index: 9999;
}
```

**Step 6: Update row-scan hover effect (lines 132-137)**

```css
.row-scan::after {
  content: '';
  position: absolute;
  left: 0;
  top: 0;
  width: 100%;
  height: 100%;
  background: linear-gradient(
    90deg,
    transparent 0%,
    rgba(58, 170, 128, 0.04) 50%,
    transparent 100%
  );
  transform: translateX(-100%);
  transition: transform 0.5s ease-out;
  pointer-events: none;
}
```

**Step 7: Update indicator-light (lines 148-162)**

```css
.indicator-light {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #3aaa80;
  box-shadow:
    0 0 4px #3aaa8080,
    0 0 8px #3aaa8040;
  animation: glow-pulse 1.5s ease-in-out infinite;
}

.indicator-light-amber {
  background: #b88420;
  box-shadow: 0 0 6px #b88420;
}
```

**Step 8: Update logo-image filter (lines 168-175)**

```css
.logo-image {
  filter: drop-shadow(0 0 12px rgba(30, 122, 106, 0.3));
  transform-origin: top left;
}

.logo-image:hover {
  filter: drop-shadow(0 0 20px rgba(30, 122, 106, 0.5));
}
```

**Step 9: Update neon-flicker-anim keyframes (lines 199-270)**

Replace ALL `rgba(255, 107, 157, ...)` occurrences in the `@keyframes neon-flicker-anim` with `rgba(30, 122, 106, ...)` (emphasis color). This is a bulk find-replace within this keyframe block. Every instance of `255, 107, 157` → `30, 122, 106`.

**Step 10: Update terminal-card component styles (lines 362-418)**

```css
@layer components {
  .terminal-card {
    @apply relative overflow-hidden rounded-lg border;
    border-color: #e0d8cc;
    background: #f3ede5;
    box-shadow:
      inset 0 0 30px rgba(30, 122, 106, 0.02),
      0 0 10px rgba(30, 122, 106, 0.04);
  }

  .terminal-card::before {
    content: '';
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(
      0deg,
      rgba(42, 37, 32, 0.03) 0px,
      rgba(42, 37, 32, 0.03) 1px,
      transparent 1px,
      transparent 2px
    );
    pointer-events: none;
    z-index: 1;
  }

  .terminal-card-header {
    @apply relative z-10 border-b px-4 py-3;
    border-color: #e0d8cc;
    background: linear-gradient(to bottom, rgba(30, 122, 106, 0.03), transparent);
  }

  .terminal-card-content {
    @apply relative z-10 p-4;
  }

  .terminal-border-glow {
    position: relative;
  }

  .terminal-border-glow::after {
    content: '';
    position: absolute;
    inset: -1px;
    border-radius: inherit;
    padding: 1px;
    background: linear-gradient(
      135deg,
      rgba(30, 122, 106, 0.15),
      transparent 50%,
      rgba(30, 122, 106, 0.06)
    );
    mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    mask-composite: exclude;
    pointer-events: none;
  }
}
```

**Step 11: Update base layer border default (line 358)**

```css
@layer base {
  * {
    @apply border-base-border;
  }
}
```

Change `border-gray-800` to `border-base-border` (uses our semantic token).

**Step 12: Update cycles-calculating-marquee gradient (lines 186-195)**

```css
.cycles-calculating-marquee {
  display: inline-block;
  background-image: linear-gradient(90deg, #7a7068 0%, #2a2520 45%, #7a7068 100%);
  background-size: 220% 100%;
  background-position: 220% 0;
  background-clip: text;
  -webkit-background-clip: text;
  color: transparent;
  -webkit-text-fill-color: transparent;
  animation: cycles-marquee-shine 2.8s linear infinite;
}
```

**Step 13: Verify build**

Run: `cd frontend && npx tsc --noEmit 2>&1 | tail -5`
Expected: No errors

**Step 14: Commit**

```bash
git add frontend/app/globals.css
git commit -m "feat(theme): update CSS variables and CRT effects for light theme"
```

---

### Task 3: Update chart colors

**Files:**

- Modify: `frontend/lib/chart-colors.ts`

**Step 1: Replace entire file contents**

```typescript
export const CHART_PRIMARY_COLOR = '#4a8c5c'; // 竹青
export const CHART_SECONDARY_COLOR = '#1e7a6a'; // 石绿
export const CHART_TERTIARY_COLOR = '#b88420'; // 琥珀

export const CHART_PALETTE = [
  '#4a8c5c', // 竹青
  '#1e7a6a', // 石绿
  '#3a6ea0', // 靛蓝
  '#b88420', // 琥珀
  '#c04040', // 朱砂
  '#d4a828', // 藤黄
  '#b84060', // 胭脂
  '#7a5090', // 紫檀
  '#68a8b8', // 月白
  '#a06830', // 赭石
  '#4a5868', // 黛色
  '#8ab870', // 豆绿
] as const;

// Centralized chart UI colors
export const CHART_GRID_COLOR = '#e0d8cc';
export const CHART_AXIS_COLOR = '#b0a898';
export const CHART_TOOLTIP_BG = '#f3ede5';
export const CHART_TOOLTIP_BORDER = '#e0d8cc';
export const CHART_HOVER_COLOR = '#d0c8bc';

export function getChartPaletteColor(index: number): string {
  const paletteLength = CHART_PALETTE.length;
  const normalizedIndex = ((index % paletteLength) + paletteLength) % paletteLength;
  return CHART_PALETTE[normalizedIndex];
}
```

**Step 2: Commit**

```bash
git add frontend/lib/chart-colors.ts
git commit -m "feat(theme): replace chart palette with 12 traditional Chinese colors"
```

---

### Task 4: Update layout.tsx

**Files:**

- Modify: `frontend/app/layout.tsx`

**Step 1: Remove dark class from html tag**

Change line 12 from:

```tsx
<html lang="en" className="dark">
```

to:

```tsx
<html lang="en">
```

**Step 2: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "feat(theme): remove dark class for light theme"
```

---

### Task 5: Update components with hardcoded colors

**Files:**

- Modify: `frontend/components/ui/hex-display.tsx` (lines 29, 34, 39, 44)
- Modify: `frontend/components/chain-wave/index.tsx` (lines 77-88)
- Modify: `frontend/components/mempool-blocks.tsx` (line ~322)
- Modify: `frontend/components/ui/progress-bar.tsx` (lines 36-39, 101-103)

**Step 1: Update hex-display.tsx glow colors**

Replace the `colorClasses` object (lines 25-46):

```typescript
const colorClasses = {
  green: {
    base: 'text-emphasis-dim',
    hover: 'text-emphasis',
    glow: '0 0 4px rgba(30, 122, 106, 0.5)',
  },
  amber: {
    base: 'text-warning-dim',
    hover: 'text-warning',
    glow: '0 0 4px rgba(184, 132, 32, 0.5)',
  },
  white: {
    base: 'text-text-muted',
    hover: 'text-text-primary',
    glow: '0 0 4px rgba(42, 37, 32, 0.3)',
  },
  accent: {
    base: 'text-emphasis',
    hover: 'text-emphasis',
    glow: '0 0 4px rgba(30, 122, 106, 0.5)',
  },
};
```

Note: `hover: 'text-white'` → `hover: 'text-text-primary'` (no raw white on light bg).

**Step 2: Update chain-wave/index.tsx stagePillClass and bubbleColor**

Replace `stagePillClass` (lines 76-80):

```typescript
function stagePillClass(stage: FlowStage): string {
  if (stage === 'mempool') return 'border-warning/30 bg-warning/10 text-warning';
  if (stage === 'proposed') return 'border-positive/30 bg-positive/10 text-positive';
  return 'border-emphasis/40 bg-emphasis/10 text-emphasis';
}
```

Replace `bubbleColor` (lines 82-89):

```typescript
function bubbleColor(feeScore: number, stage: FlowStage, missing: boolean): string {
  if (missing) return 'rgba(176, 168, 152, 0.6)';

  const alpha = 0.4 + feeScore * 0.4;
  if (stage === 'mempool') return `rgba(184, 132, 32, ${alpha})`;
  if (stage === 'proposed') return `rgba(74, 140, 92, ${alpha})`;
  return `rgba(30, 122, 106, ${alpha})`;
}
```

**Step 3: Update mempool-blocks.tsx hardcoded rgba**

Find the `rgba(140, 224, 10, ...)` reference (~line 322) and replace with `rgba(30, 122, 106, ...)`.

**Step 4: Update progress-bar.tsx**

Replace `barColors` (lines 35-40):

```typescript
const barColors = {
  green: 'bg-gradient-to-r from-emphasis-dim via-emphasis to-emphasis-bright',
  amber: 'bg-gradient-to-r from-warning-dim via-warning to-warning-bright',
  blue: 'bg-gradient-to-r from-info-dim via-info to-info-bright',
  red: 'bg-gradient-to-r from-negative-dim via-negative to-negative-bright',
};
```

Replace `getColorClass` in `UsageBar` (lines 100-104):

```typescript
const getColorClass = () => {
  if (percent < 33) return 'bg-positive/30';
  if (percent < 66) return 'bg-warning/30';
  return 'bg-negative/30';
};
```

Also on line 109 change `text-white` to `text-text-primary`.

**Step 5: Verify build**

Run: `cd frontend && npx tsc --noEmit 2>&1 | tail -5`
Expected: No errors

**Step 6: Commit**

```bash
git add frontend/components/ui/hex-display.tsx frontend/components/chain-wave/index.tsx frontend/components/mempool-blocks.tsx frontend/components/ui/progress-bar.tsx
git commit -m "feat(theme): update hardcoded colors in components for light theme"
```

---

### Task 6: Audit and fix standard Tailwind color classes

**Files:** 33 files using `text-white`, `text-gray-*`, `bg-green-*`, `bg-red-*`, `text-green-*`, `bg-blue-*`, `bg-yellow-*` etc.

**Step 1: Find all non-semantic Tailwind color usages**

Run: `cd frontend && grep -rn 'text-white\|text-gray-\|bg-gray-\|border-gray-\|text-green-\|bg-green-\|text-blue-\|bg-blue-\|text-red-\|bg-red-\|text-yellow-\|bg-yellow-' --include='*.tsx' --include='*.ts' app/ components/ | grep -v node_modules | grep -v __tests__`

Review each occurrence and replace:

- `text-white` → `text-text-primary` (prominent text on light bg)
- `text-gray-400/500` → `text-text-muted`
- `text-gray-300` → `text-text-secondary`
- `text-gray-600` → `text-text-secondary`
- `bg-gray-800/900` → `bg-base-elevated`
- `border-gray-700/800` → `border-base-border`
- `text-green-200/300/400` → `text-positive`
- `bg-green-500/10` → `bg-positive/10`
- `border-green-500/30` → `border-positive/30`
- `bg-red-500/30` → `bg-negative/30`
- `bg-yellow-500/30` → `bg-warning/30`
- `from-blue-900 via-blue-600 to-blue-400` → `from-info-dim via-info to-info-bright`
- `from-red-900 via-red-600 to-red-400` → `from-negative-dim via-negative to-negative-bright`

**Principle:** Replace ALL standard Tailwind color classes with semantic token classes. No raw color classes should remain in production code (test files are excluded from this audit).

**Step 2: Verify build after each batch**

Run: `cd frontend && npx tsc --noEmit`

**Step 3: Commit**

```bash
git add -A frontend/app/ frontend/components/
git commit -m "feat(theme): replace standard Tailwind colors with semantic tokens"
```

---

### Task 7: Update UI color guidelines doc

**Files:**

- Modify: `frontend/docs/ui-color-guidelines.md`

**Step 1: Rewrite guidelines for new palette**

```markdown
# UI Color Guidelines

This document defines the frontend text color hierarchy for the 青绿暖白 (Qinglv Light) theme in `ckbadger`.

## Primary Palette (青绿暖白)

- Primary signal: `text-emphasis` (深青 #1e7a6a)
- Secondary signal: `text-interactive` (绿 #3aaa80)
- Warning signal: `text-warning` (琥珀 #b88420)
- Main foreground: `text-text-primary` (#2a2520)
- Muted foreground: `text-text-secondary` (#7a7068)
- Tertiary/helper foreground: `text-text-muted` (#b0a898)

## Text Hierarchy

Use the following hierarchy in `frontend/app` and `frontend/components`:

1. `Primary Data` (numbers/hashes/status that users scan first)

- `text-text-primary`, `text-emphasis`, `text-interactive`

2. `Secondary Context` (labels, section metadata, minor values)

- `text-text-secondary`

3. `Helper/Delimiter` (placeholders, separators, helper copy)

- `text-text-muted`

## Semantic Colors (Traditional Chinese Names)

| Token      | Name | Hex       | Usage              |
| ---------- | ---- | --------- | ------------------ |
| `positive` | 竹青 | `#4a8c5c` | Success, confirmed |
| `negative` | 朱红 | `#c04040` | Error, failure     |
| `warning`  | 琥珀 | `#b88420` | Warning, pending   |
| `info`     | 靛青 | `#3a6ea0` | Informational      |

## Guardrails

- Do not use `text-text-dim` in user-facing views under `frontend/app` and `frontend/components`.
- Do not use standard Tailwind color classes (text-white, bg-gray-\*, etc.) — use semantic tokens.
- Prefer semantic colors for charts from `frontend/lib/chart-colors.ts`.
- For new chart legends, keep the same semantic mapping:
  - primary series → 竹青 (`CHART_PRIMARY_COLOR`)
  - secondary series → 石绿 (`CHART_SECONDARY_COLOR`)

## Review Checklist

- New placeholder/separator text uses `text-text-muted`
- New helper or instruction text uses `text-text-muted` or `text-text-secondary`
- Primary numbers are not rendered in muted/dim tones
- Chart color choices come from project palette constants
- No raw Tailwind color classes (text-white, bg-gray-\*, etc.)
```

**Step 2: Commit**

```bash
git add frontend/docs/ui-color-guidelines.md
git commit -m "docs: update UI color guidelines for 青绿暖白 theme"
```

---

### Task 8: Run tests and visual verification

**Step 1: Run all frontend tests**

Run: `cd frontend && npx vitest run`
Expected: All tests pass (color changes shouldn't break behavioral tests)

**Step 2: Run type check**

Run: `cd frontend && pnpm type-check`
Expected: No errors

**Step 3: Run lint**

Run: `cd frontend && pnpm lint`
Expected: No errors

**Step 4: Visual verification**

Run: `cd frontend && pnpm dev`

Check in browser:

- [ ] Homepage loads with warm white background
- [ ] Scanlines visible as subtle texture
- [ ] Terminal panels have proper surface/elevated contrast
- [ ] Links and buttons show green (#3aaa80) interactive color
- [ ] Stat numbers show cyan (#1e7a6a) emphasis color
- [ ] Charts render with traditional color palette
- [ ] Badges (committed/pending) show correct semantic colors
- [ ] Indicator lights pulse in green
- [ ] Text is readable (proper contrast on light bg)
- [ ] Glow effects are subtle cyan shadows, not neon

**Step 5: Fix any visual issues found**

Address any remaining dark-theme artifacts discovered during visual review.

**Step 6: Final commit if needed**

```bash
git add -A frontend/
git commit -m "fix(theme): address visual issues from light theme review"
```
