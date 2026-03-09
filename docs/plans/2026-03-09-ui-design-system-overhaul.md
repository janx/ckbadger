# UI/UX Design System Overhaul — Argonaut-Inspired Terminal Aesthetic

**Date**: 2026-03-09
**Status**: Approved

## Goal

Overhaul the ckbadger frontend design system to create a cohesive midnight terminal hacking atmosphere with strict color role assignments, increased information density, and Argonaut color scheme inspiration.

## Problems with Current Design

1. **Color role overload**: `terminal-green` serves 5+ roles (interactive, emphasis, accent, heading, trend) — muddies visual communication
2. **Clinical tone**: Pure white text + blue-gray slates feel sterile, not "midnight hacking"
3. **Dashboard spacing**: Padding/gaps optimized for comfort, not information density
4. **Inconsistent link colors**: Clickable elements alternate between green, amber, and white with no clear pattern

## Design: Role-Based Color System (4 Layers)

### Layer 1: Atmosphere (~78%)

The "midnight" feel. Navy-tinted backgrounds with warm off-white text hierarchy.

| Token            | Hex       | CSS Variable             | Usage                           |
| ---------------- | --------- | ------------------------ | ------------------------------- |
| `base-bg`        | `#0d0f18` | `--color-base-bg`        | Page background                 |
| `surface`        | `#12151e` | `--color-surface`        | Cards, panels                   |
| `elevated`       | `#181c27` | `--color-elevated`       | Hover states, elevated surfaces |
| `border`         | `#1f2430` | `--color-border`         | Dividers, borders               |
| `text-primary`   | `#fffaf3` | `--color-text-primary`   | Headings, key data (warm white) |
| `text-secondary` | `#c8c2b8` | `--color-text-secondary` | Body text (warm gray)           |
| `text-muted`     | `#6b6860` | `--color-text-muted`     | Labels, metadata                |

### Layer 2: Interactive (~12%)

Everything clickable. Cyan = "you can interact with this."

| Token               | Hex         | CSS Variable                | Usage                               |
| ------------------- | ----------- | --------------------------- | ----------------------------------- |
| `interactive`       | `#00d7eb`   | `--color-interactive`       | Links, clickable hashes, active nav |
| `interactive-hover` | `#67ffef`   | `--color-interactive-hover` | Hover on links                      |
| `interactive-muted` | `#00d7eb40` | `--color-interactive-muted` | Subtle interactive backgrounds      |

### Layer 3: Emphasis (~7%)

Important values and data highlights. Lime green = "pay attention to this number."

| Token           | Hex         | CSS Variable            | Usage                                  |
| --------------- | ----------- | ----------------------- | -------------------------------------- |
| `emphasis`      | `#8ce00a`   | `--color-emphasis`      | Block numbers, CKB amounts, key values |
| `emphasis-dim`  | `#6ba808`   | `--color-emphasis-dim`  | Secondary emphasis, borders            |
| `emphasis-glow` | `#8ce00a30` | `--color-emphasis-glow` | Glow effects                           |

### Layer 4: Semantic (~3%)

Status signals only. Never decorative.

| Token      | Hex       | CSS Variable       | Usage                                     |
| ---------- | --------- | ------------------ | ----------------------------------------- |
| `positive` | `#8ce00a` | `--color-positive` | Up trends, success (shares with emphasis) |
| `negative` | `#ff000f` | `--color-negative` | Down trends, errors                       |
| `warning`  | `#ffb900` | `--color-warning`  | Warnings, pending                         |
| `info`     | `#008df8` | `--color-info`     | Informational badges                      |

## Spacing & Density Changes

Compress toward "htop" density. Less dead space, more data per screen.

| Element        | Current         | New               |
| -------------- | --------------- | ----------------- |
| Page container | `py-8`          | `py-4`            |
| Section gaps   | `mt-6` / `mt-8` | `mt-4` / `mt-5`   |
| Panel padding  | `p-4` / `p-6`   | `p-3` / `p-4`     |
| Row padding    | `py-3`          | `py-2`            |
| Panel header   | `px-4 py-3`     | `px-3 py-2`       |
| Grid gaps      | `gap-6`         | `gap-3` / `gap-4` |
| Page header mb | `mb-8`          | `mb-4`            |
| Data field py  | `py-3`          | `py-2`            |

## CRT/Glow Effects

Keep atmosphere, reduce intensity:

- Scanline overlay: reduce opacity to ~0.015
- Glow shadows: lime green, ~30% less spread
- Row-scan hover: keep as-is
- Logo neon flicker: keep as-is

## Component Changes

### Color Role Mapping

| Element                   | Current Color                        | New Color                   | New Role    |
| ------------------------- | ------------------------------------ | --------------------------- | ----------- |
| Clickable hash links      | `text-amber` / `text-terminal-green` | `text-interactive` (cyan)   | Interactive |
| Block numbers (as values) | `text-terminal-green`                | `text-emphasis` (lime)      | Emphasis    |
| CKB capacity amounts      | `text-white`                         | `text-emphasis` (lime)      | Emphasis    |
| Navigation active         | green border                         | cyan border                 | Interactive |
| "View all" links          | `text-terminal-green`                | `text-interactive` (cyan)   | Interactive |
| Timestamps                | `text-slate-400/500`                 | `text-muted` (warm)         | Atmosphere  |
| Labels                    | `text-slate-500`                     | `text-muted` (warm)         | Atmosphere  |
| Headings                  | `text-white`                         | `text-primary` (warm white) | Atmosphere  |

### Activity Badges

| Type     | Current                              | New                            |
| -------- | ------------------------------------ | ------------------------------ |
| Received | `bg-emerald-900/50 text-emerald-300` | `bg-emphasis/10 text-emphasis` |
| Sent     | `bg-red-900/50 text-red-300`         | `bg-negative/10 text-negative` |
| Coinbase | `bg-purple-900/50 text-purple-300`   | `bg-info/10 text-info`         |
| Self     | `bg-slate-800 text-slate-400`        | `bg-elevated text-muted`       |
| Pending  | `bg-amber-900/50 text-amber`         | `bg-warning/10 text-warning`   |

### Charts

- Primary series: emphasis lime
- Secondary series: interactive cyan
- Grid lines: `border` color
- Axis text: `text-muted`
- Tooltips: `bg-surface` + `border-border`

## Implementation Strategy

1. Define CSS variables + Tailwind tokens in config (foundation)
2. Update `globals.css` base styles and effects
3. Update core UI components (terminal-panel, stat-block, data-field, page-header)
4. Update layout components (header, footer)
5. Update page-level components (homepage sections, detail pages)
6. Update chart components
7. Visual review and fine-tuning

## Files Affected

- `frontend/tailwind.config.ts` — color tokens, shadows, animations
- `frontend/app/globals.css` — CSS variables, base styles, effects
- `frontend/components/ui/*.tsx` — all 26 UI components
- `frontend/components/layout/*.tsx` — header, footer, logo
- `frontend/components/home-*.tsx` — homepage sections
- `frontend/components/latest-*.tsx` — activity feeds
- `frontend/app/**/*client-page.tsx` — all page components
- `frontend/components/charts/*.tsx` — chart wrappers
