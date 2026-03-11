# UX/UI Design System — Midnight Terminal with Chinese Traditional Colors

**Date:** 2026-03-11
**Status:** Design approved
**Supersedes:** 2026-03-10-midnight-citypop-design.md, 2026-03-09-ui-design-system-overhaul.md

## Goal

Redesign the ckbadger frontend UX/UI system with three objectives:

1. **Information hierarchy** — highlight the most important CKB blockchain state at a glance
2. **Data-driven navigation** — let users wander through information by clicking entities (addresses, hashes, blocks)
3. **1980s Tokyo rooftop atmosphere** — terminal + neon + dashboard feel: "midnight, alone on a skyscraper, surrounded by glowing screens"

## Design Decisions

| Dimension  | Choice                           | Description                                                               |
| ---------- | -------------------------------- | ------------------------------------------------------------------------- |
| Density    | Focused Dashboard                | Hero stat cards + 2-column content panels                                 |
| Navigation | Command palette + entity links   | Minimal chrome, data itself is the navigation                             |
| Atmosphere | Full immersion (no particles)    | Scan lines, vignette, neon glow, edge reflections, subtle flicker         |
| Color      | Chinese traditional on dark base | 翠玉 jade, 胭脂 rouge, 缥碧 aqua, 藤黄 gamboge, 雪青 lavender, 琥珀 amber |

## Color System

### Backgrounds (cold midnight base, unchanged)

| Token           | Hex       | Usage                             |
| --------------- | --------- | --------------------------------- |
| `void`          | `#08090e` | `<body>`, deepest background      |
| `bg`            | `#0c0e15` | Primary panels, stat cells, cards |
| `surface`       | `#10131c` | Hover states, secondary fills     |
| `elevated`      | `#161a25` | Selected states, overlays         |
| `border`        | `#222840` | Strong borders, search overlay    |
| `border-subtle` | `#1a1f30` | Panel borders, row dividers       |

### Text (4 tiers, unchanged)

| Token         | Hex       | Usage                            |
| ------------- | --------- | -------------------------------- |
| `text-bright` | `#dee2ec` | Stat values, page titles         |
| `text`        | `#a0a8be` | Body text, field values          |
| `text-dim`    | `#606880` | Labels, timestamps, inactive nav |
| `text-ghost`  | `#343c50` | Axis labels, hints, separators   |

### Accent Colors — Chinese Traditional (中国传统色)

| Token      | Name | Hex       | Dim       | Glow        | Semantic Role                                           |
| ---------- | ---- | --------- | --------- | ----------- | ------------------------------------------------------- |
| `jade`     | 翠玉 | `#2edba3` | `#1fb88a` | `#2edba325` | Primary interactive, positive delta, transfers, success |
| `rouge`    | 胭脂 | `#e8555a` | `#c04048` | `#e8555a25` | Negative delta, consumed cells, errors                  |
| `aqua`     | 缥碧 | `#68ccf0` | `#4aa8d0` | `#68ccf025` | Links, hashes, block numbers, info                      |
| `gold`     | 藤黄 | `#f2c55c` | `#d0a840` | `#f2c55c25` | DAO locked, pending, warnings, section prompts          |
| `lavender` | 雪青 | `#b8a9e8` | `#9888c8` | `#b8a9e825` | NFT/Spore, creative actions, identities                 |
| `amber`    | 琥珀 | `#d4883a` | `#b07028` | `#d4883a25` | Pending state, mempool, withdraw-request                |

### Chart Palette (12 colors for multi-series)

```
jade, rouge, aqua, gold, lavender, amber,
jade-dim, rouge-dim, aqua-dim, gold-dim, lavender-dim, amber-dim
```

### Semantic Mapping

| Semantic           | Token | Usage                                       |
| ------------------ | ----- | ------------------------------------------- |
| `ok` / `positive`  | jade  | Up trends, success states, transfer badges  |
| `err` / `negative` | rouge | Down trends, errors, negative amounts       |
| `warn`             | gold  | Warnings, pending operations                |
| `info`             | aqua  | Informational, block numbers, hash links    |
| `interactive`      | aqua  | Clickable elements, links (→ jade on hover) |
| `emphasis`         | gold  | Key values, CKB amounts, active tabs        |

### Color Role Assignments

| Element                    | Color                              | Role        |
| -------------------------- | ---------------------------------- | ----------- |
| Clickable hash links       | aqua                               | Interactive |
| Block numbers (values)     | aqua                               | Info        |
| CKB capacity amounts       | jade (positive) / rouge (negative) | Semantic    |
| Navigation active          | jade border + text                 | Interactive |
| DAO amounts                | gold                               | Emphasis    |
| Activity badges: transfer  | jade border                        | Semantic    |
| Activity badges: DAO       | gold border                        | Emphasis    |
| Activity badges: NFT/Spore | lavender border                    | Creative    |
| Activity badges: negative  | rouge border                       | Semantic    |
| Timestamps, labels         | text-dim                           | Atmosphere  |
| Page titles                | text-bright                        | Atmosphere  |

## Typography

- **Font:** JetBrains Mono only
- **Hero stat values:** 28px, font-weight 700, tabular-nums
- **Stat values:** 20px, font-weight 500
- **Page titles:** 18px, font-weight 500
- **Body / table rows:** 13px
- **Labels / keys:** 11px
- **Badges:** 10px
- **Tabs / Nav links:** 13px

## Visual Effects (Full Immersion, No Particles)

### CRT Scan Lines

- `repeating-linear-gradient(0deg, transparent 0 1px, rgba(0,0,0,0.07) 1px 3px)`
- Fixed position, full viewport, z-index: 9999
- pointer-events: none

### Vignette

- `radial-gradient(ellipse at center, transparent 20%, rgba(0,0,0,0.55) 100%)`
- Fixed position, z-index: 9998

### Ambient Glow (context-colored)

- 3 blurred radial gradients positioned around viewport
- Colors shift per page context (jade near blocks, gold near DAO, aqua near charts)
- 8-12% opacity, z-index: -1

### Neon Edge Reflections

- Cards with left-edge color bar: 3px wide, `box-shadow: 0 0 20px color, 0 0 40px glow`
- Top neon reflection line: `linear-gradient(90deg, color 0%, transparent 50%)` at 30% opacity
- Panel top reflection: `linear-gradient(90deg, transparent, accent, transparent)` at 40% opacity

### Glow Hierarchy (3 tiers)

1. **Neon** (hero numbers): `text-shadow: 0 0 10px color, 0 0 30px glow, 0 0 60px faint`
2. **Soft** (card values, panel titles): `text-shadow: 0 0 20px glow, 0 0 40px faint`
3. **None** (body text, labels, details)

### Subtle Flicker

- Hero stat values: occasional neon flicker animation (8s cycle, 92-98% keyframes)
- Live-dot indicator: pulsing glow animation (1.5s cycle)
- No glitch, no particles

## Layout — Focused Dashboard

### Page Structure

```
┌──────────────────────────────────────────────────┐
│  [logo]  [search ─────────── ⌘K]  DAO Assets ... │ ← top nav, 42px
├──────────────────────────────────────────────────┤
│                                                    │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ │ ← hero stats (4 cards)
│  │  Block   │ │ HashRate│ │ DAO     │ │ Active  │ │
│  │14,892,347│ │ 548.2   │ │ 8.92B   │ │ 8,492   │ │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘ │
│                                                    │
│  ┌──────────────────────┐ ┌──────────────────────┐ │ ← 2-column content
│  │  Recent Blocks       │ │  Activity Feed       │ │    (2fr : 1fr)
│  │  #14,892,347  47 txs │ │  Transfer +1,250     │ │
│  │  #14,892,346  31 txs │ │  DAO Dep +50,000     │ │
│  │  ...                 │ │  Transfer -48,200     │ │
│  │  ┌─────────────────┐ │ │  Spore Mint +162     │ │
│  │  │  inline chart   │ │ │  ...                 │ │
│  │  └─────────────────┘ │ │                      │ │
│  └──────────────────────┘ └──────────────────────┘ │
│                                                    │
├──────────────────────────────────────────────────┤
│  [synced] tip: N | epoch: N | version             │ ← status bar, 26px
└──────────────────────────────────────────────────┘
```

### Hero Stat Cards

- 4-column grid at top of homepage
- Each card: colored left-edge bar + neon glow, hero number (tier 1 glow), label + delta
- Cards map to: Latest Block (jade), Hash Rate (aqua), DAO Locked (gold), Active Addresses (rouge)

### Content Panels (TerminalPanel)

- Header bar: live-dot + title + action tabs (right-aligned)
- Content: table rows or activity feed items
- Top neon reflection line
- Footer: cursor pagination or "view all" link

### Information Layer Mapping (per INFORMATION_DESIGN.md)

| Page         | Primary Layer             | Up to L2                  | Down to L0                 |
| ------------ | ------------------------- | ------------------------- | -------------------------- |
| Homepage     | L1 activities + L0 blocks | Hero stats (aggregations) | Click block → block detail |
| Address      | L1 activities             | Balance, token values     | Activity → Tx → Cells      |
| Token        | L1 holders, transfers     | Stats, charts             | Holder → Address → Cells   |
| DAO          | L1 deposits               | Statistics, APC           | Deposit → Tx → Cell        |
| Block detail | L0 raw                    | —                         | Tx list → Cell detail      |
| Charts       | L2 aggregations           | —                         | Click data point → blocks  |

## Navigation Model

### Top Navigation Bar

- 42px tall, sticky, `bg` background
- Logo: `$ckbadger` in jade with live-dot
- Search bar: expands into CommandPalette on focus / `⌘K`
- Nav items: DAO, Assets, Scripts, Charts, Blocks (13px, text-dim → jade active)

### Command Palette (`⌘K`)

- Full-width overlay, fuzzy search across all entity types
- Result type badges: Block (aqua), Tx (jade), Address (text-bright), Token (gold), Script (lavender)
- Keyboard navigation: arrow keys, enter to navigate
- This is the "fast travel" — equivalent of teleporting across the city

### Entity Links (the "wandering" mechanism)

- Every hash, address, block number, token symbol is a clickable `EntityLink`
- Default color: aqua, hover: jade with glow
- Click navigates to that entity's detail page
- This creates a closed exploration loop: any data point leads to more data
- Cross-layer traceability: stats → addresses → txs → cells → scripts → stats

### No Sidebar, No Breadcrumbs

- Location context comes from the page header (entity type + identifier)
- "Where am I" is always clear from the data you're looking at
- Navigation emerges from the data graph, not from menus

## Key Page Designs

### 1. Homepage Dashboard

- Hero row: 4 stat blocks (Latest Block, Hash Rate, DAO Locked, Active Addresses)
- Content grid: Recent Blocks (2fr, with inline chart) + Activity Feed (1fr)
- All block numbers, hashes, addresses are EntityLinks
- Layer mapping: Hero = L2, Blocks = L0, Activities = L1

### 2. Address Detail (entity detail archetype)

- Hero: address hash (full, copyable) + balance stat block (jade/rouge)
- Tabs: Activities (default, L1) | Tokens | DAO | Live Cells (L0)
- Each activity row links to tx → cells → scripts
- Reusable pattern for: Token detail, Block detail, Script detail

### 3. Blocks List (list archetype)

- TerminalPanel with sortable table
- Columns: block #, hash, tx count, time, miner — all EntityLinks
- Inline spark chart in header
- Reusable pattern for: Transactions, Tokens, Scripts, DAO deposits

## Component Primitives

| Component        | Description                                                   |
| ---------------- | ------------------------------------------------------------- |
| `NeonCard`       | Surface with colored left-edge glow + top reflection line     |
| `StatBlock`      | Label + hero number + delta subtitle, configurable glow tier  |
| `TerminalPanel`  | Bordered panel with header bar, live-dot, action tabs, footer |
| `EntityLink`     | Clickable hash/address/block#, aqua → jade hover with glow    |
| `CommandPalette` | `⌘K` overlay, fuzzy search, type-ahead results                |
| `ActivityRow`    | Type badge + amount + detail + EntityLinks                    |
| `BadgeTag`       | 10px bordered badge, color by activity type                   |
| `LiveDot`        | Pulsing glow dot for real-time indicators                     |

## What Changes vs. Current

### Changes

- Color palette: neon cyberpunk → Chinese traditional colors
- Glow effects: uniform → 3-tier hierarchy (neon / soft / none)
- Atmosphere: scan lines + vignette + ambient glow + neon edge reflections (no particles)
- Navigation: top nav emphasis → command palette + entity links emphasis
- Homepage: restructure around hero stats + 2-column focused layout
- Information hierarchy: explicit layer mapping (L0/L1/L2) per page

### What Does NOT Change

- All page routes and information architecture
- All component APIs and props interfaces
- Data fetching (TanStack Query patterns)
- Responsive breakpoints
- Backend API endpoints
- Font (JetBrains Mono)
- Dark-only theme (no light mode)

## Implementation Scope

### Foundation (must be done first)

1. `frontend/tailwind.config.ts` — Replace color tokens with Chinese traditional palette
2. `frontend/app/globals.css` — CSS variables, scan lines, vignette, glow utilities, neon edges
3. `frontend/lib/chart-colors.ts` — New chart palette

### Core Components

4. `frontend/components/ui/terminal-panel.tsx` — Neon edge reflections, live-dot
5. `frontend/components/ui/stat-block.tsx` — Glow tier system, hero sizing
6. `frontend/components/ui/page-header.tsx` — Simplified, entity-context focused
7. `frontend/components/ui/tabs.tsx` — jade active, gold emphasis variant
8. `frontend/components/ui/hex-display.tsx` — Chinese color cycling
9. `frontend/components/ui/cursor-pagination.tsx` — Token swap
10. `frontend/components/ui/address.tsx` — EntityLink pattern (aqua → jade hover)
11. `frontend/components/ui/hash.tsx` — EntityLink pattern

### Layout

12. `frontend/components/layout/header.tsx` — 42px, search bar, nav items
13. `frontend/components/layout/logo.tsx` — Jade color, live-dot
14. `frontend/components/layout/site-footer.tsx` — Status bar style
15. `frontend/components/search-bar.tsx` — Command palette integration
16. `frontend/components/command-palette.tsx` — Fuzzy search, type badges

### Homepage

17. `frontend/components/stats-cards.tsx` — Hero stat cards with glow
18. `frontend/components/latest-blocks.tsx` — Aqua block numbers, EntityLinks
19. `frontend/components/latest-activities.tsx` — Badge colors, EntityLinks
20. `frontend/components/home-content.tsx` — 2-column focused layout
21. `frontend/components/home-charts.tsx` — New chart colors

### Charts

22. `frontend/components/ui/line-chart.tsx` — New stroke/fill colors
23. `frontend/components/ui/stacked-area-chart.tsx` — New gradient colors
24. `frontend/components/ui/pie-chart.tsx` — New palette
25. `frontend/components/ui/spark-chart.tsx` — jade default stroke
26. `frontend/components/ui/multi-series-line-chart.tsx` — New series colors
27. `frontend/components/charts/chart-page.tsx` — Dark background tokens

### Remaining Pages & Components

28-44. All remaining page components and UI components — token swap to new palette

## Validation

- Visual: screenshot comparison before/after on homepage, address detail, blocks list
- Functional: all EntityLinks navigate correctly, CommandPalette search works
- Atmosphere: scan lines visible, vignette visible, glow hierarchy distinguishable
- Color: each accent color appears in its designated semantic role only
- Cross-layer: every L2 stat links down to L1, every L1 links down to L0
