# UI Color Guidelines

The frontend has two themes drawn from one palette: **dark** (midnight — ink and silk, neon
tubes; the theme the site was designed in and the default) and **light** (the same ink on paper by
day). The reader chooses in the header's theme menu (Dark · Light · System); the choice is kept in
`localStorage` under `ckbadger.theme` and applied as `data-theme` on `<html>` before the first
paint (`index.html`, `lib/theme.ts`).

## One source of truth

`frontend/lib/palette.ts` holds every colour of both themes by name (`PALETTE.dark`,
`PALETTE.light`) and the non-colour finish per theme (`MATERIALS`: glass fills, the sky, the
scanline ink, `glow`). `tailwind.config.ts` emits both as CSS custom properties — `--c-<key>` (the
colour), `--rgb-<key>` (its channels, for Tailwind's opacity modifiers) and `--m-<key>` (a
material) — on `:root` for dark and under `:root[data-theme='light']` for light.

Page code never writes a hex (`__tests__/lib/theme-tokens-gate.test.ts` fails the build if it
does). A colour is one of:

- a Tailwind class (`text-text-dim`, `bg-jade/8`, `border-base-border`) — every token in
  `tailwind.config.ts` resolves through the variables;
- a named constant from `lib/design-tokens.ts` (`CLASS_COLORS.dao`, `CELL_COLORS.lock`,
  `FORK_COLORS.grid` …), each a `color()` reference, for SVG attributes and inline styles;
- `color(key)`, `alpha(key, a)`, `glowAlpha(key, a)` or `material(key)` from `lib/palette.ts`;
- `fade(cssColor, a)` to tint a colour you only hold as a reference (a `color-mix`);
- for a 2D canvas or a WebGL shader, which cannot read a variable: `useThemePalette()` for the
  concrete hex of the theme in force (`hexAlpha(hex, a)` for an rgba).

Halos and glows are alphas multiplied by `--m-glow` (1 by night, 0 by day): write them with
`glowAlpha`, or in CSS as `rgb(var(--rgb-jade) / calc(0.45 * var(--m-glow)))`. A rule whose glow
colour comes from a component variable is switched off by day in `app/globals.css` instead.

## Contrast

`__tests__/lib/palette.test.ts` pins every text-grade ink and accent to ≥ 4.5:1 on every ground in
both themes, hover and secondary inks to ≥ 3:1, and the midnight values to the ones the site was
designed in. Change a value there and the test says what it broke.

## Text hierarchy

Use the following hierarchy in `frontend/app` and `frontend/components`:

1. `Primary Data` (numbers/hashes/status that users scan first)

- `text-text-bright`, `text-emphasis`, `text-interactive`

2. `Secondary Context` (labels, section metadata, minor values)

- `text-text`

3. `Helper/Delimiter` (placeholders, separators, helper copy)

- `text-text-dim`

## Named accent colors (Chinese traditional)

| Token      | Name | Midnight  | Daylight  | Usage                    |
| ---------- | ---- | --------- | --------- | ------------------------ |
| `jade`     | 翠玉 | `#2edba3` | `#0a7254` | Active, interactive, nav |
| `rouge`    | 胭脂 | `#e8555a` | `#b5303a` | Error, failure           |
| `aqua`     | 缥碧 | `#68ccf0` | `#0d6b96` | Informational, links     |
| `gold`     | 琥珀 | `#f2c55c` | `#7f5c10` | Warning, emphasis values |
| `lavender` | 紫藤 | `#b8a9e8` | `#6a55b5` | Identity, special        |
| `amber`    | 琥珀 | `#d4883a` | `#9c520e` | Burnt orange accent      |

The grounds step the other way by day: the page is paper (`base-bg`), a surface is white
(`base-surface`), and `base-elevated` is a grey tint (a hover, a raised panel) rather than a
lighter step.

## Semantic colors

| Token      | Resolves to | Usage              |
| ---------- | ----------- | ------------------ |
| `positive` | jade        | Success, confirmed |
| `negative` | rouge       | Error, failure     |
| `warning`  | gold        | Warning, pending   |
| `info`     | aqua        | Informational      |

## Guardrails

- Do not use standard Tailwind color classes (text-white, bg-gray-\*, etc.) -- use semantic tokens.
- Do not write a hex or an `rgba(...)` of a palette colour anywhere but `lib/palette.ts`.
- Prefer semantic colors for charts from `frontend/lib/chart-colors.ts`.
- For new chart legends, keep the same semantic mapping:
  - primary series -> jade (`CHART_PRIMARY_COLOR`)
  - the following series -> the jade ramp (`jadeRamp(n)`)

## Review checklist

- New placeholder/separator text uses `text-text-dim`
- New helper or instruction text uses `text-text-dim` or `text-text`
- Primary numbers are not rendered in dim tones
- Chart color choices come from project palette constants
- No raw Tailwind color classes (text-white, bg-gray-\*, etc.)
- A new colour is added to both themes in `lib/palette.ts` and read through a reference
- A glow is `glowAlpha` / `--m-glow`-scaled, so it goes out by day
- Checked in both themes (the header menu, or `localStorage.setItem('ckbadger.theme', 'light')`)
