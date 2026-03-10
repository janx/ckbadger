# Design: 青绿暖白设计系统 (Qinglv Light Theme)

Date: 2026-03-10

## Goal

Replace the current "Citypop Midnight" dark theme with a light theme rooted in traditional Chinese cyan-green (青绿) colors. Create a bright, warm, daytime-comfortable aesthetic while preserving the terminal-style core.

## Design Decisions

1. **Light background**: Warm white / 宣纸质感 (`#faf6f0`)
2. **CRT effects**: All retained (scanlines, glow, flicker) — readapted for light backgrounds
3. **Color roles**: Green (绿) = interactive/primary, Cyan (青) = emphasis/secondary
4. **Semantic colors**: Traditional Chinese color names (朱红, 竹青, 琥珀, 靛青)
5. **Chart palette**: 12 traditional Chinese colors (full spectrum)
6. **Fonts**: Unchanged (JetBrains Mono + Space Grotesk)

## Color Specification

### Background Layers

| Token      | Usage                   | Hex       | Name   |
| ---------- | ----------------------- | --------- | ------ |
| `base`     | Page background         | `#faf6f0` | 宣纸白 |
| `surface`  | Panels/cards            | `#f3ede5` | 素纸   |
| `elevated` | Higher elevation panels | `#efe8de` | 绢白   |
| `border`   | Borders/dividers        | `#e0d8cc` | 麻色   |

### Text Hierarchy

| Token            | Usage                  | Hex       |
| ---------------- | ---------------------- | --------- |
| `text-primary`   | Main body text         | `#2a2520` |
| `text-secondary` | Secondary text         | `#7a7068` |
| `text-muted`     | Disabled/faded         | `#b0a898` |
| `text-dim`       | Very faint placeholder | `#d0c8bc` |

### Interactive (Green Primary)

| Token               | Hex         | Usage                                 |
| ------------------- | ----------- | ------------------------------------- |
| `interactive`       | `#3aaa80`   | Links, buttons, active tabs           |
| `interactive-hover` | `#2cc878`   | Hover state                           |
| `interactive-muted` | `#3aaa8020` | Interactive element backgrounds       |
| `interactive-dim`   | `#2d8a68`   | Pressed state / secondary interactive |

### Emphasis (Cyan Secondary)

| Token             | Hex         | Usage                             |
| ----------------- | ----------- | --------------------------------- |
| `emphasis`        | `#1e7a6a`   | Data highlights, important values |
| `emphasis-dim`    | `#166858`   | Darker variant                    |
| `emphasis-glow`   | `#1e7a6a30` | Light glow/shadow                 |
| `emphasis-bright` | `#28a088`   | Brighter variant                  |

### Semantic Colors (Traditional Chinese)

| Token          | Name | Hex       | Usage              |
| -------------- | ---- | --------- | ------------------ |
| `positive`     | 竹青 | `#4a8c5c` | Success, confirmed |
| `positive-dim` | —    | `#3a7048` | Darker variant     |
| `negative`     | 朱红 | `#c04040` | Error, failure     |
| `negative-dim` | —    | `#a03535` | Darker variant     |
| `warning`      | 琥珀 | `#b88420` | Warning, pending   |
| `warning-dim`  | —    | `#9a6e1a` | Darker variant     |
| `info`         | 靛青 | `#3a6ea0` | Informational      |
| `info-dim`     | —    | `#2e5a84` | Darker variant     |

### Chart Palette (12 Traditional Colors)

| Index | Name | Hex       |
| ----- | ---- | --------- |
| 0     | 竹青 | `#4a8c5c` |
| 1     | 石绿 | `#1e7a6a` |
| 2     | 靛蓝 | `#3a6ea0` |
| 3     | 琥珀 | `#b88420` |
| 4     | 朱砂 | `#c04040` |
| 5     | 藤黄 | `#d4a828` |
| 6     | 胭脂 | `#b84060` |
| 7     | 紫檀 | `#7a5090` |
| 8     | 月白 | `#68a8b8` |
| 9     | 赭石 | `#a06830` |
| 10    | 黛色 | `#4a5868` |
| 11    | 豆绿 | `#8ab870` |

### Box Shadows (Light Theme Glow)

| Token              | Value                                   | Usage                    |
| ------------------ | --------------------------------------- | ------------------------ |
| `glow`             | `0 0 6px #1e7a6a18, 0 0 14px #1e7a6a10` | Subtle emphasis glow     |
| `glow-strong`      | `0 0 5px #1e7a6a28, 0 0 12px #1e7a6a18` | Strong emphasis glow     |
| `glow-inset`       | `inset 0 0 8px #1e7a6a10`               | Inset border glow        |
| `interactive-glow` | `0 0 8px #3aaa8025`                     | Interactive element glow |

## CRT Effects Adaptation

All CRT effects preserved but tuned for light backgrounds:

- **Scanlines**: `#2a252008` (extremely subtle on warm white — 宣纸纹理)
- **Neon glow** → **Soft cyan shadow**: `0 0 8px #1e7a6a25`
- **Flicker animation**: Reduced amplitude (opacity 96%-100%) to avoid eye strain on light bg
- **Indicator light**: Green pulse using `#3aaa80` with `box-shadow: 0 0 6px #3aaa8080`
- **Scan line animation**: Retained at 8s loop, color `#1e7a6a10`
- **Row scan hover**: Horizontal gradient sweep using `#3aaa8010`

## Files to Change

| File                                        | Change                                                               |
| ------------------------------------------- | -------------------------------------------------------------------- |
| `frontend/tailwind.config.ts`               | Replace all color tokens, shadows, animations                        |
| `frontend/app/globals.css`                  | Update CSS variables, scanline colors, glow colors, background noise |
| `frontend/lib/chart-colors.ts`              | Replace 12-color palette                                             |
| `frontend/app/layout.tsx`                   | Remove `class="dark"`, update html/body classes                      |
| `frontend/components/ui/terminal-panel.tsx` | Update scanline/glow classes for light theme                         |
| `frontend/components/ui/stat-block.tsx`     | Update color variants                                                |
| `frontend/components/ui/badge.tsx`          | Update variant colors                                                |
| `frontend/components/ui/progress-bar.tsx`   | Update color logic                                                   |
| `frontend/components/ui/line-chart.tsx`     | Update default colors                                                |
| All page components                         | Audit for hardcoded dark-theme colors                                |
