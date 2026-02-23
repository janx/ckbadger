# UI Color Guidelines

This document defines the frontend text color hierarchy for dark surfaces in `ckbadger`.

## Primary Palette

- Primary signal: `text-terminal-green`
- Secondary signal: `text-amber`
- Main foreground: `text-white` / `text-slate-300`
- Muted foreground: `text-slate-400`
- Tertiary/helper foreground: `text-slate-500`

## Text Hierarchy

Use the following hierarchy in `frontend/app` and `frontend/components`:

1. `Primary Data` (numbers/hashes/status that users scan first)

- `text-white`, `text-terminal-green`, `text-amber`

2. `Secondary Context` (labels, section metadata, minor values)

- `text-slate-400`

3. `Helper/Delimiter` (placeholders, separators, helper copy)

- `text-slate-500`

## Guardrails

- Do not use `text-slate-600` in user-facing views under `frontend/app` and `frontend/components`.
- Prefer semantic colors for charts from `frontend/lib/chart-colors.ts`.
- For new chart legends, keep the same semantic mapping:
- primary series -> `terminal green`
- secondary series -> `amber`

## Review Checklist

- New placeholder/separator text uses `text-slate-500`
- New helper or instruction text uses `text-slate-500` or `text-slate-400`
- Primary numbers are not rendered in slate tones
- Chart color choices come from project palette constants
