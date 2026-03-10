# Citypop Midnight Color System — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current lime/cyan terminal color system with a citypop midnight palette (pink emphasis, teal interactive, multi-chromatic charts, softened CRT effects).

**Architecture:** Pure color/effect value swap across 4 files. No structural changes — same CSS variable names, same Tailwind token structure, same component APIs.

**Tech Stack:** Tailwind CSS, CSS custom properties, TypeScript constants

**Design doc:** `docs/plans/2026-03-10-citypop-color-system-design.md`

---

### Task 1: Update Tailwind color tokens

**Files:**

- Modify: `frontend/tailwind.config.ts`

**Step 1: Replace all color values and box shadows**

Replace the entire `colors` object and `boxShadow` object with:

```typescript
colors: {
  // Citypop Midnight palette
  base: {
    bg: '#0c0a12',
    surface: '#110e1a',
    elevated: '#181424',
    border: '#1e1a2a',
  },
  text: {
    primary: '#f0e6ea',
    secondary: '#c0b0b8',
    muted: '#706068',
    dim: '#453d42',
  },
  interactive: {
    DEFAULT: '#4dd0c8',
    hover: '#78edd8',
    muted: '#4dd0c840',
    dim: '#38a89e',
  },
  emphasis: {
    DEFAULT: '#ff6b9d',
    dim: '#d4547e',
    glow: '#ff6b9d30',
    bright: '#ff8fb8',
  },
  positive: {
    DEFAULT: '#4dd0c8',
    dim: '#38a89e',
  },
  negative: {
    DEFAULT: '#ff4060',
    dim: '#cc3350',
    bright: '#ff6080',
  },
  warning: {
    DEFAULT: '#ff8c42',
    dim: '#cc7035',
    bright: '#ffb070',
  },
  info: {
    DEFAULT: '#64b5f6',
    dim: '#4a90c8',
    bright: '#90ccff',
  },
},
```

```typescript
boxShadow: {
  glow: '0 0 6px #ff6b9d20, 0 0 14px #ff6b9d10',
  'glow-strong': '0 0 5px #ff6b9d35, 0 0 12px #ff6b9d18',
  'glow-inset': 'inset 0 1px 6px #ff6b9d08',
  'interactive-glow': '0 0 6px #4dd0c820, 0 0 14px #4dd0c810',
},
```

**Step 2: Soften animation keyframes**

Replace `terminal-flicker` keyframes:

```typescript
'terminal-flicker': {
  '0%, 100%': { opacity: '1' },
  '50%': { opacity: '0.99' },
  '25%, 75%': { opacity: '0.98' },
},
```

Replace `terminal-glow-pulse` animation timing and keyframes:

```typescript
// In animation:
'terminal-glow-pulse': 'terminal-glow-pulse 4s ease-in-out infinite',

// In keyframes:
'terminal-glow-pulse': {
  '0%, 100%': { filter: 'brightness(1)' },
  '50%': { filter: 'brightness(1.05)' },
},
```

Soften `glitch` keyframes (reduce displacement):

```typescript
glitch: {
  '0%': { transform: 'translate(0)', opacity: '1' },
  '20%': { transform: 'translate(-1px, 1px)', opacity: '0.9' },
  '40%': { transform: 'translate(1px, -1px)', opacity: '0.95' },
  '60%': { transform: 'translate(-0.5px, 0.5px)', opacity: '0.9' },
  '80%': { transform: 'translate(0.5px, -0.5px)', opacity: '0.95' },
  '100%': { transform: 'translate(0)', opacity: '1' },
},
```

**Step 3: Run type check**

Run: `cd frontend && pnpm type-check`
Expected: PASS (no type changes, only values)

**Step 4: Commit**

```bash
git add frontend/tailwind.config.ts
git commit -m "style: replace tailwind color tokens with citypop midnight palette"
```

---

### Task 2: Update CSS custom properties and utility classes

**Files:**

- Modify: `frontend/app/globals.css`

**Step 1: Replace `:root` variables**

```css
:root {
  --foreground: #f0e6ea;
  --background: #0c0a12;
  --color-surface: #110e1a;
  --color-elevated: #181424;
  --color-border: #1e1a2a;
  --color-emphasis: #ff6b9d;
  --color-emphasis-dim: #d4547e;
  --color-interactive: #4dd0c8;
  --color-interactive-hover: #78edd8;
  --font-mono: 'JetBrains Mono', 'SFMono-Regular', 'Roboto Mono', ui-monospace, monospace;
  --font-display: 'Space Grotesk', 'Inter', system-ui, sans-serif;
}
```

**Step 2: Replace all hardcoded color values in utility classes**

All occurrences of the old colors need updating. The key replacements:

| Old value                 | New value                  | Context                      |
| ------------------------- | -------------------------- | ---------------------------- |
| `#8ce00a`                 | `#ff6b9d`                  | emphasis/terminal-text color |
| `rgba(140, 224, 10, ...)` | `rgba(255, 107, 157, ...)` | All emphasis rgba variants   |
| `#6ba808`                 | `#d4547e`                  | emphasis-dim                 |
| `rgba(107, 168, 8, ...)`  | `rgba(212, 84, 126, ...)`  | emphasis-dim rgba            |
| `#ffb900`                 | `#ff8c42`                  | warning/amber                |
| `#1f2430`                 | `#1e1a2a`                  | border color                 |
| `#12151e`                 | `#110e1a`                  | surface color                |

Specific classes to update:

`.terminal-text`: color `#ff6b9d`, text-shadow `0 0 6px #ff6b9d30`

`.terminal-text-dim`: color `#d4547e`, text-shadow `0 0 4px #d4547e20`

`.crt-screen::before` scanlines: reduce opacity from `0.15` to `0.06`

`.scanline-overlay`: change `rgba(140, 224, 10, 0.015)` to `rgba(255, 107, 157, 0.008)`

`.row-scan::after`: change `rgba(140, 224, 10, 0.05)` to `rgba(255, 107, 157, 0.03)`

`.indicator-light`: background `#ff6b9d`, box-shadow `0 0 4px #ff6b9d80, 0 0 8px #ff6b9d40`

`.indicator-light-amber`: background `#ff8c42`, box-shadow `0 0 6px #ff8c42`

`.logo-image`: `drop-shadow(0 0 12px rgba(255, 107, 157, 0.4))`, hover `drop-shadow(0 0 20px rgba(255, 107, 157, 0.6))`

`neon-flicker-anim` keyframes: replace all `rgba(140, 224, 10, ...)` with `rgba(255, 107, 157, ...)` (same alpha values)

`.terminal-card`: border-color `#1e1a2a`, background `#110e1a`, box-shadow inset `rgba(255, 107, 157, 0.02)` and outer `rgba(255, 107, 157, 0.04)` (reduced from 0.03/0.06)

`.terminal-card::before` scanlines: reduce from `rgba(0, 0, 0, 0.1)` to `rgba(0, 0, 0, 0.04)`

`.terminal-card-header`: border-color `#1e1a2a`, gradient `rgba(255, 107, 157, 0.02)`

`.terminal-border-glow::after`: `rgba(255, 107, 157, 0.2)` and `rgba(255, 107, 157, 0.08)` (reduced from 0.3/0.1)

**Step 3: Verify build**

Run: `cd frontend && pnpm build`
Expected: PASS

**Step 4: Commit**

```bash
git add frontend/app/globals.css
git commit -m "style: update CSS variables and utilities for citypop midnight"
```

---

### Task 3: Update chart colors

**Files:**

- Modify: `frontend/lib/chart-colors.ts`

**Step 1: Replace all color constants**

```typescript
export const CHART_PRIMARY_COLOR = '#ff6b9d'; // Citypop pink
export const CHART_SECONDARY_COLOR = '#4dd0c8'; // Teal
export const CHART_TERTIARY_COLOR = '#ff8c42'; // Citypop orange

export const CHART_PALETTE = [
  '#ff6b9d', // Citypop pink
  '#ff8c42', // Orange
  '#b07cff', // Violet
  '#4dd0c8', // Teal
  '#64b5f6', // Sky blue
  '#ffe066', // Warm yellow
  '#ff4081', // Hot pink
  '#78edd8', // Bright teal
  '#d4a0ff', // Light violet
  '#ffb070', // Light coral
  '#80cbc4', // Muted teal
  '#706068', // Muted
] as const;

// Centralized chart UI colors
export const CHART_GRID_COLOR = '#1e1a2a';
export const CHART_AXIS_COLOR = '#453d42';
export const CHART_TOOLTIP_BG = '#110e1a';
export const CHART_TOOLTIP_BORDER = '#1e1a2a';
export const CHART_HOVER_COLOR = '#453d42';
```

**Step 2: Run type check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 3: Commit**

```bash
git add frontend/lib/chart-colors.ts
git commit -m "style: update chart palette for citypop midnight"
```

---

### Task 4: Update UI color guidelines doc

**Files:**

- Modify: `frontend/docs/ui-color-guidelines.md`

**Step 1: Rewrite guidelines to match new palette**

```markdown
# UI Color Guidelines

This document defines the frontend text color hierarchy for dark surfaces in `ckbadger`.

## Primary Palette (Citypop Midnight)

- Primary signal: `text-emphasis` (citypop pink #ff6b9d)
- Secondary signal: `text-warning` (orange #ff8c42)
- Main foreground: `text-text-primary` (#f0e6ea)
- Muted foreground: `text-text-secondary` (#c0b0b8)
- Tertiary/helper foreground: `text-text-muted` (#706068)

## Text Hierarchy

Use the following hierarchy in `frontend/app` and `frontend/components`:

1. `Primary Data` (numbers/hashes/status that users scan first)

- `text-text-primary`, `text-emphasis`, `text-warning`

2. `Secondary Context` (labels, section metadata, minor values)

- `text-text-secondary`

3. `Helper/Delimiter` (placeholders, separators, helper copy)

- `text-text-muted`

## Guardrails

- Do not use `text-text-dim` in user-facing views under `frontend/app` and `frontend/components`.
- Prefer semantic colors for charts from `frontend/lib/chart-colors.ts`.
- For new chart legends, keep the same semantic mapping:
  - primary series -> citypop pink (`text-emphasis`)
  - secondary series -> orange (`text-warning`)

## Review Checklist

- New placeholder/separator text uses `text-text-muted`
- New helper or instruction text uses `text-text-muted` or `text-text-secondary`
- Primary numbers are not rendered in muted/dim tones
- Chart color choices come from project palette constants
```

**Step 2: Commit**

```bash
git add frontend/docs/ui-color-guidelines.md
git commit -m "docs: update color guidelines for citypop midnight palette"
```

---

### Task 5: Visual verification and cleanup

**Step 1: Run frontend lint + type check**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 2: Run frontend tests**

Run: `cd frontend && npx vitest run`
Expected: PASS (no color-dependent test assertions expected)

**Step 3: Delete preview file**

```bash
rm frontend/color-preview.html
```

**Step 4: Final commit**

```bash
git add -A
git commit -m "chore: remove color preview file"
```
