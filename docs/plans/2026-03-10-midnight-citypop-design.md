# Midnight City Pop Terminal — UX/UI Design System

**Date:** 2026-03-10
**Scope:** Design tokens + component refinement (option B)
**Preview:** `frontend/public/mood-previews.html`

## Concept

Happy hacking at 2AM — dark terminal foundation with vibrant city pop accent colors. Information-dense, monospace-native, no decorative chrome. The feel of a dark room lit only by screen glow, with warm city pop colors bleeding through like distant city lights.

## Color System

### Backgrounds (cold midnight blue undertone)

| Token         | Hex       | Usage                                     |
| ------------- | --------- | ----------------------------------------- |
| void          | `#08090e` | `<body>`, deepest background              |
| bg            | `#0c0e15` | Primary panels, stat cells, cards         |
| surface       | `#10131c` | Hover states, hex blocks, secondary fills |
| elevated      | `#161a25` | Selected states, overlays                 |
| border        | `#222840` | Strong borders, search overlay            |
| border-subtle | `#1a1f30` | Panel borders, row dividers, most borders |

### Text (4 tiers)

| Token       | Hex       | Usage                                           |
| ----------- | --------- | ----------------------------------------------- |
| text-bright | `#dee2ec` | Stat values, page titles, selected text         |
| text        | `#a0a8be` | Body text, field values                         |
| text-dim    | `#606880` | Labels, timestamps, inactive nav, field keys    |
| text-ghost  | `#343c50` | Chart axis labels, breadcrumb separators, hints |

### City Pop Accents (5 vivid colors + dim variants)

| Token  | Hex       | Dim       | Usage                                                                  |
| ------ | --------- | --------- | ---------------------------------------------------------------------- |
| amber  | `#f0b866` | `#c89440` | Primary interactive, CKB amounts, cursor, active tabs, section prompts |
| rose   | `#e87ea0` | `#c0608a` | Token amounts, secondary links, DAO chart                              |
| sky    | `#6ab0e8` | `#4a88c0` | Block numbers, hash links, info color                                  |
| mint   | `#5ce0b8` | `#3cb898` | Transfer badges, success/ok states                                     |
| violet | `#b08af0` | `#8a68c8` | Coinbase badges, tertiary accent, hex display                          |

### Semantic

| Token | Hex       | Dim             |
| ----- | --------- | --------------- |
| ok    | `#5ce0b8` | `#3cb898`       |
| err   | `#e86080` | `#c04860`       |
| warn  | `#f0b866` | (same as amber) |

### Glow values

- amber-glow: `#f0b86625`
- rose-glow: `#e87ea025`
- sky-glow: `#6ab0e825`

## Typography

- **Font:** JetBrains Mono only (no display/sans-serif font)
- **Body:** 14px, line-height 1.7
- **Stat values:** 20px, font-weight 500
- **Page titles:** 18px, font-weight 500
- **Table rows:** 13px
- **Labels/keys:** 11-12px
- **Badges:** 10px
- **Tabs:** 13px
- **Nav links:** 13px
- **Status bar:** 11px
- **Chart labels:** 11px
- **Section dividers:** 12px
- **Hex display:** 13px, line-height 2.1

## Component Design

### Navigation Bar

- 42px tall, sticky top, `bg` background
- `$ckbadger` logo in amber with blinking block cursor
- Links at 13px, dim → text on hover, bright when active
- Search hint right-aligned with `kbd` shortcut badge

### Status Bar (new element)

- 26px tall, fixed bottom, `surface` background
- Vim/tmux style: `[dot] synced | tip: N | epoch: N | peers: N | version`
- Mint pulsing dot for sync status

### Section Dividers

- `# section-name` with amber prompt, hairline extending right
- 12px font, dim text for section name

### Stat Row

- Horizontal strip of cells separated by 1px `border-subtle` gaps
- No border-radius — sharp edges
- 14px padding, label 11px dim, value 20px bright, delta 11px ok/err

### Terminal Panel

- `bg` background, 1px `border-subtle` border
- Header: 8px 14px padding, 12px font, dot indicator + title + count
- Dot: 5px mint circle with glow shadow, pulsing animation

### Table Rows

- 7px 14px padding, 13px font
- 1px `border-subtle` bottom divider
- Hover: `surface` background + 2px amber inset left shadow
- Block numbers in sky, hashes in dim, times in dim, amounts in amber/rose

### Badges

- 10px font, 2px 6px padding, 1px border, no background fill
- transfer: mint text + mint-dim border
- dao: amber text + amber-dim border
- token: rose text + rose-dim border
- coinbase: violet text + violet-dim border

### Tabs

- 13px font, 8px 16px padding
- Inactive: dim text, transparent bottom border
- Active: amber text, amber-dim bottom border (1px)
- Hover: text color

### Data Fields

- Key-value rows, 7px 14px padding, 13px font
- Key: 150px fixed width, 12px dim text
- Value: text color, links in sky, highlights in amber, secondary in rose

### Hex Display

- 13px font, 2.1 line-height, `surface` background
- Multi-color byte cycling: sky (4n+1), violet (6n+1), amber (8n+1), rose (10n+1)
- Base bytes in dim, hover → bright + glow text-shadow

### Page Header

- 16px padding top/bottom, `border-subtle` bottom border
- Breadcrumb: 11px ghost, links in dim with hover
- Title: 18px bright, font-weight 500
- Subtitle: 12px dim

### Charts

- `bg` background, 1px border
- 14px padding, 110px SVG height
- Stroke: amber or rose at full brightness, 1.5px width
- Fill gradient: accent color at 15-18% opacity → 0%
- Grid lines: `border` color at 0.5px
- End dot: accent color circle, 0.8 opacity
- Axis labels: 11px ghost

### Progress Bar

- 3px height, `border-subtle` track, amber fill

### Capacity Bar

- 8px height, `border-subtle` track
- Used segment: amber
- Free segment: `border` color

### Search Overlay

- `surface` background, `border` border
- Input row: 10px 14px padding, amber `>` prompt, 14px bright text
- Results: 8px 14px padding, 13px font
- Selected: elevated background, bright text, amber `>` prefix

## Visual Effects

### CRT Scanline

- Repeating linear gradient overlay (2px transparent, 2px rgba(0,0,0,0.08))
- Fixed position, covers full viewport, 0.35 opacity
- pointer-events: none, z-index: 9999

### Vignette

- Radial gradient: transparent center → rgba(0,0,0,0.4) at edges
- Fixed position, z-index: 9998

### Ambient Glow

- 3 fixed-position blurred circles (filter: blur(140px))
- Amber: top-right, 400px, 7% opacity
- Rose: bottom-left, 400px, 7% opacity
- Violet: center, 400px, 4% opacity
- z-index: -1, pointer-events: none

## What Does NOT Change

- All page routes and information architecture
- All component APIs and props interfaces
- Real-time data patterns (TanStack Query)
- Responsive breakpoints
- Data fetching logic
- No new component files

## Implementation Scope

### Files to modify

1. `frontend/tailwind.config.ts` — Replace all color tokens, shadows, animations
2. `frontend/app/globals.css` — Replace CSS variables, add scanline/vignette/glow utilities
3. `frontend/app/layout.tsx` — Apply void background, update font loading
4. `frontend/lib/chart-colors.ts` — New chart palette using amber/rose/sky/mint/violet
5. `frontend/components/ui/terminal-panel.tsx` — Update variant classes
6. `frontend/components/ui/stat-block.tsx` — Update size/color configs
7. `frontend/components/ui/data-field.tsx` — Update label/value classes
8. `frontend/components/ui/tabs.tsx` — Update active/inactive styles
9. `frontend/components/ui/hex-display.tsx` — Multi-color cycling, hover glow
10. `frontend/components/ui/capacity.tsx` — Amber highlight for values
11. `frontend/components/ui/capacity-utilization.tsx` — Amber used bar
12. `frontend/components/ui/page-header.tsx` — Font size, spacing updates
13. `frontend/components/ui/cursor-pagination.tsx` — Border/color token swap
14. `frontend/components/ui/progress-bar.tsx` — New gradient colors
15. `frontend/components/ui/chart-card.tsx` — Dark background tokens
16. `frontend/components/ui/line-chart.tsx` — New stroke/fill colors
17. `frontend/components/ui/pie-chart.tsx` — New palette colors
18. `frontend/components/ui/stacked-area-chart.tsx` — New gradient colors
19. `frontend/components/ui/spark-chart.tsx` — Amber default stroke
20. `frontend/components/ui/multi-series-line-chart.tsx` — New series colors
21. `frontend/components/ui/address.tsx` — Sky link color
22. `frontend/components/ui/hash.tsx` / `hex-code.tsx` — Sky/dim variants
23. `frontend/components/ui/terminal-number.tsx` — Amber glow
24. `frontend/components/ui/occupation-range-selector.tsx` — Active amber
25. `frontend/components/ui/script-view.tsx` — Dark token swap
26. `frontend/components/ui/image.tsx` — No change needed
27. `frontend/components/layout/header.tsx` — Nav bar restyle (42px, 13px links)
28. `frontend/components/layout/logo.tsx` — Amber color, cursor animation
29. `frontend/components/layout/site-footer.tsx` — Dark tokens, status bar style
30. `frontend/components/search-bar.tsx` — Dark tokens, amber prompt
31. `frontend/components/command-palette.tsx` — Dark overlay tokens
32. `frontend/components/stats-cards.tsx` — Dark tokens, mint indicator
33. `frontend/components/mini-stats-cards.tsx` — Dark tokens, amber/rose bars
34. `frontend/components/latest-blocks.tsx` — Sky block numbers, dark rows
35. `frontend/components/latest-activities.tsx` — Vibrant badge colors
36. `frontend/components/latest-transactions.tsx` — Dark tokens
37. `frontend/components/activity-breakdown.tsx` — New pie chart colors
38. `frontend/components/home-charts.tsx` — Amber/rose chart strokes
39. `frontend/components/home-content.tsx` — Layout spacing
40. `frontend/components/chain-wave/` — Stage colors (amber/mint/sky)
41. `frontend/components/error-boundary.tsx` — err color tokens
42. `frontend/components/not-found-page.tsx` — Dark tokens
43. `frontend/components/charts/chart-page.tsx` — Dark background
44. `frontend/components/charts/chart-calculation-note.tsx` — Dark tokens

### Files with no changes needed

- All `app/*/page.tsx` and `client-page.tsx` — no styling, just data wiring
- `frontend/lib/api.ts` — data layer
- `frontend/lib/dynamic-client.tsx` — loading utility
- Test files — no visual changes
