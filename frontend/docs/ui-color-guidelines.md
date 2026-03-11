# UI Color Guidelines

This document defines the frontend text color hierarchy for the ink-and-silk midnight theme in `ckbadger`.

## Primary Palette

- Primary signal: `text-emphasis` (#2edba3)
- Interactive signal: `text-interactive` (#68ccf0)
- Warning signal: `text-warning` (#f2c55c)
- Bright foreground: `text-text-bright` (#dee2ec)
- Default foreground: `text-text` (#a0a8be)
- Dim foreground: `text-text-dim` (#606880)

## Text Hierarchy

Use the following hierarchy in `frontend/app` and `frontend/components`:

1. `Primary Data` (numbers/hashes/status that users scan first)

- `text-text-bright`, `text-emphasis`, `text-interactive`

2. `Secondary Context` (labels, section metadata, minor values)

- `text-text`

3. `Helper/Delimiter` (placeholders, separators, helper copy)

- `text-text-dim`

## Named Accent Colors (Chinese Traditional)

| Token      | Name | Hex       | Usage                    |
| ---------- | ---- | --------- | ------------------------ |
| `jade`     | 翠玉 | `#2edba3` | Active, interactive, nav |
| `rouge`    | 胭脂 | `#e8555a` | Error, failure           |
| `aqua`     | 缥碧 | `#68ccf0` | Informational, links     |
| `gold`     | 琥珀 | `#f2c55c` | Warning, emphasis values |
| `lavender` | 紫藤 | `#b8a9e8` | Identity, special        |
| `amber`    | 琥珀 | `#d4883a` | Burnt orange accent      |

## Semantic Colors

| Token      | Hex       | Usage              |
| ---------- | --------- | ------------------ |
| `positive` | `#2edba3` | Success, confirmed |
| `negative` | `#e8555a` | Error, failure     |
| `warning`  | `#f2c55c` | Warning, pending   |
| `info`     | `#68ccf0` | Informational      |

## Guardrails

- Do not use standard Tailwind color classes (text-white, bg-gray-\*, etc.) -- use semantic tokens.
- Prefer semantic colors for charts from `frontend/lib/chart-colors.ts`.
- For new chart legends, keep the same semantic mapping:
  - primary series -> jade (`CHART_PRIMARY_COLOR`)
  - secondary series -> rouge (`CHART_SECONDARY_COLOR`)

## Review Checklist

- New placeholder/separator text uses `text-text-dim`
- New helper or instruction text uses `text-text-dim` or `text-text`
- Primary numbers are not rendered in dim tones
- Chart color choices come from project palette constants
- No raw Tailwind color classes (text-white, bg-gray-\*, etc.)
