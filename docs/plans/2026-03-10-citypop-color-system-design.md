# Citypop Midnight — Color System Design

**Theme**: 80s Japan, citypop, midnight, terminal (softened)
**Approach**: Full replacement of current color system. Multi-chromatic palette (Option D) with simple role mapping (Option C) and softened CRT effects (Option B).

## Design Decisions

- **Pink = emphasis** (replaces lime). Primary data highlight color.
- **Teal = interactive** (replaces cyan). Links, buttons, hover states.
- **Orange + violet = chart/palette only**. Not assigned to semantic UI roles.
- **CRT effects softened**. Less "hacker terminal", more "late-night FM radio booth".

## 1. Base & Surface Colors

Warmer midnight with subtle purple undertone (shift from cold navy).

| Token           | Old       | New       | Note                           |
| --------------- | --------- | --------- | ------------------------------ |
| `base.bg`       | `#0d0f18` | `#0c0a12` | Warmer midnight, slight purple |
| `base.surface`  | `#12151e` | `#110e1a` | Card backgrounds               |
| `base.elevated` | `#181c27` | `#181424` | Hover/elevated panels          |
| `base.border`   | `#1f2430` | `#1e1a2a` | Purple-tinted borders          |

## 2. Text Hierarchy

Rose-tinted cream replaces blue-grey.

| Token            | Old       | New       |
| ---------------- | --------- | --------- |
| `text.primary`   | `#fffaf3` | `#f0e6ea` |
| `text.secondary` | `#c8c2b8` | `#c0b0b8` |
| `text.muted`     | `#6b6860` | `#706068` |
| `text.dim`       | `#4a4740` | `#453d42` |

## 3. Emphasis & Interactive

Core accent swap.

| Token                 | Old              | New                      |
| --------------------- | ---------------- | ------------------------ |
| `emphasis.DEFAULT`    | `#8ce00a` (lime) | `#ff6b9d` (citypop pink) |
| `emphasis.dim`        | `#6ba808`        | `#d4547e`                |
| `emphasis.bright`     | `#abe05a`        | `#ff8fb8`                |
| `emphasis.glow`       | `#8ce00a30`      | `#ff6b9d30`              |
| `interactive.DEFAULT` | `#00d7eb` (cyan) | `#4dd0c8` (teal)         |
| `interactive.hover`   | `#67ffef`        | `#78edd8`                |
| `interactive.muted`   | `#00d7eb40`      | `#4dd0c840`              |
| `interactive.dim`     | `#009aa8`        | `#38a89e`                |

## 4. Semantic Colors

| Token             | Old       | New       | Note                    |
| ----------------- | --------- | --------- | ----------------------- |
| `positive`        | `#8ce00a` | `#4dd0c8` | Teal (green-ish = good) |
| `positive.dim`    | —         | `#38a89e` |                         |
| `positive.bright` | —         | `#78edd8` |                         |
| `negative`        | `#ff000f` | `#ff4060` | Warmer red              |
| `negative.dim`    | `#cc000c` | `#cc3350` |                         |
| `negative.bright` | `#ff273f` | `#ff6080` |                         |
| `warning`         | `#ffb900` | `#ff8c42` | Citypop orange          |
| `warning.dim`     | `#cc8c00` | `#cc7035` |                         |
| `warning.bright`  | `#ffd141` | `#ffb070` |                         |
| `info`            | `#008df8` | `#64b5f6` | Softer blue             |
| `info.dim`        | `#006bc0` | `#4a90c8` |                         |
| `info.bright`     | `#0092ff` | `#90ccff` |                         |

## 5. Chart Palette (12 colors)

```
#ff6b9d  — pink (primary series)
#ff8c42  — orange
#b07cff  — violet
#4dd0c8  — teal
#64b5f6  — sky blue
#ffe066  — warm yellow
#ff4081  — hot pink
#78edd8  — bright teal
#d4a0ff  — light violet
#ffb070  — light coral
#80cbc4  — muted teal
#706068  — muted
```

Chart UI colors:

```
CHART_GRID_COLOR    = #1e1a2a  (border)
CHART_AXIS_COLOR    = #453d42  (dim text)
CHART_TOOLTIP_BG    = #110e1a  (surface)
CHART_TOOLTIP_BORDER = #1e1a2a (border)
CHART_HOVER_COLOR   = #453d42  (dim)
```

## 6. Glow & Shadow Effects

Wider spread, lower intensity — warm neon ambience.

| Effect             | Old                                     | New                                     |
| ------------------ | --------------------------------------- | --------------------------------------- |
| `glow`             | `0 0 4px #8ce00a25, 0 0 10px #8ce00a15` | `0 0 6px #ff6b9d20, 0 0 14px #ff6b9d10` |
| `glow-strong`      | `0 0 3px #8ce00a50, 0 0 8px #8ce00a25`  | `0 0 5px #ff6b9d35, 0 0 12px #ff6b9d18` |
| `glow-inset`       | `inset 0 1px 4px #8ce00a10`             | `inset 0 1px 6px #ff6b9d08`             |
| `interactive-glow` | `0 0 4px #00d7eb30, 0 0 10px #00d7eb18` | `0 0 6px #4dd0c820, 0 0 14px #4dd0c810` |

## 7. Terminal Effects — Softened

| Effect     | Change                                                                              |
| ---------- | ----------------------------------------------------------------------------------- |
| Scanlines  | Reduce opacity to ~40% of current                                                   |
| Flicker    | Narrow range: 0.96-1.0 → 0.98-1.0                                                   |
| Glow pulse | Brightness 1.0-1.1 → 1.0-1.05, cycle 3s → 4s                                        |
| Glitch     | Keep but reduce intensity — subtle nod                                              |
| New        | Very subtle warm gradient overlay on body (dark purple → dark rose, barely visible) |

## 8. CSS Variables (globals.css)

```css
--foreground: #f0e6ea;
--background: #0c0a12;
--color-surface: #110e1a;
--color-elevated: #181424;
--color-border: #1e1a2a;
--color-emphasis: #ff6b9d;
--color-emphasis-dim: #d4547e;
--color-interactive: #4dd0c8;
--color-interactive-hover: #78edd8;
```

## 9. Files to Modify

| File                                   | Changes                                             |
| -------------------------------------- | --------------------------------------------------- |
| `frontend/tailwind.config.ts`          | All color tokens, box shadows                       |
| `frontend/app/globals.css`             | CSS variables, utility classes, keyframe animations |
| `frontend/lib/chart-colors.ts`         | Chart palette + UI colors                           |
| `frontend/docs/ui-color-guidelines.md` | Update guidelines to match new palette              |

## 10. Scope Notes

- No structural changes — same CSS variable architecture, same Tailwind token names.
- No component logic changes — only color values and effect intensities.
- `color-preview.html` will be deleted after implementation.
