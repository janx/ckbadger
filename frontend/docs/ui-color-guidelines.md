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
