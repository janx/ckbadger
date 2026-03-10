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
