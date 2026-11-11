# UI Color Guidelines

The frontend has two themes drawn from one palette: **night**, "rubbing" (`dark`, the default — a
dark warm-ink ground, moon-white text, celadon phosphor, cinnabar marks, old gold) and **day**,
"Song print" (`light` — the same inks on paper). The values come from the design boards
(`docs/superpowers/design/2026-11-10-rubbing-song/src/lib.js`, `THEMES.dark` / `THEMES.light`).
The reader chooses in the header's theme menu (Dark · Light · System); the choice is kept in
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
both themes, hover and secondary inks and the cinnabar `mark` to ≥ 3:1, and the night values to the
rubbing values of the boards (`RUBBING`). Change a value there and the test says what it broke.
Where a board value fell short of its grade it was moved in OKLCH lightness only (hue and chroma
kept) until it cleared — see the derivation below.

## Text hierarchy

Use the following hierarchy in `frontend/app` and `frontend/components`:

1. `Primary Data` (numbers/hashes/status that users scan first)

- `text-text-bright`, `text-emphasis`, `text-interactive`

2. `Secondary Context` (labels, section metadata, minor values)

- `text-text`

3. `Helper/Delimiter` (placeholders, separators, helper copy)

- `text-text-dim`

## Named accent colors (rubbing × Song print)

The token names are the roles the pages were written against; the values are the boards' inks.

| Token      | Ink                      | Night     | Day       | Usage                                                    |
| ---------- | ------------------------ | --------- | --------- | -------------------------------------------------------- |
| `jade`     | celadon 青瓷             | `#93c1b6` | `#2f6b63` | Active, interactive, nav, the data colour, phosphor      |
| `mark`     | cinnabar 朱砂            | `#c2452d` | `#b5331f` | Prompt squares, seals, the search square — UI grade only |
| `rouge`    | cinnabar text 朱砂       | `#d9634a` | `#a8392a` | Error, failure (cinnabar as text)                        |
| `gold`     | old gold 古金            | `#c9a45c` | `#80600f` | Warning, DAO, emphasis values                            |
| `aqua`     | indigo 靛青              | `#7fa3c9` | `#2e5f8f` | Informational, links                                     |
| `lavender` | lotus 藕荷               | `#a797c4` | `#5e4d9a` | Special; a chart hue                                     |
| `identity` | rouge 胭脂 (the boards') | `#c97f93` | `#9c3b5f` | The identity class (`CLASS_COLORS.identities`)           |
| `token`    | azurite 石青             | `#6f9ad6` | `#2a5a9e` | The token class (`CLASS_COLORS.tokens`)                  |
| `object`   | lotus violet             | `#a58ad6` | `#5e45a8` | The object class (`CLASS_COLORS.objects`)                |
| `copper`   | ochre 赭石               | `#cc8a5e` | `#93522b` | Bitcoin: RGB++, the BTC tiers (`BITCOIN_COLORS.copper`)  |
| `amber`    | moss 苔绿                | `#a3bf7a` | `#46701f` | A chart hue (one-to-one for the old burnt orange)        |

`mark` and `rouge` are both cinnabar: `mark` is the pigment as a shape (≥ 3:1, never body text),
`rouge` the same red lifted (by night) or deepened (by day) to body-text contrast.

The grounds step the other way by day: the page is paper (`base-bg`), a surface is a whiter sheet
(`base-surface`), the well is the whitest (`base-void`, under an inset), and `base-elevated` is a
darker tint (a hover, a raised panel) rather than a lighter step.

## How every key is derived

Source values are the boards' `THEMES` (lib.js). "mix A B t" is an sRGB mix, t of A; "L ±x" and
"C ×y" are OKLCH lightness and chroma moves with the hue kept; "≥ 4.5" / "≥ 3" means moved in
lightness only (lighter by night, darker by day) until it clears that ratio on all four grounds.

| Role                                                    | Source                                  | Rule (night / day)                                                                                                                                                                               |
| ------------------------------------------------------- | --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `base-void` `-bg` `-surface` `-elevated`                | well, bg, surface, elevated             | the board values: `#0f0d0b #15120f #1b1714 #241f1a` / `#fbf9f4 #f3eee4 #faf7f0 #e9e2d4`                                                                                                          |
| `base-border`, `base-border-subtle`                     | line, hair                              | `#2a231d #221c17` / `#d4cab8 #e3dbcc`                                                                                                                                                            |
| `text-bright`, `text`, `text-ghost`                     | moon, text, ghost                       | the board values                                                                                                                                                                                 |
| `text-dim`                                              | dim `#8a8073` / `#6f6659` (4.21 / 4.38) | ≥ 4.5 → `#908679` (4.57) / `#6c6356` (4.58)                                                                                                                                                      |
| `ink-quiet`, `ink-muted`                                | ghost → dim                             | mix dim ghost 0.32 / 0.62 (muted ≥ 3); day 0.45 / 0.83 (muted ≥ 3)                                                                                                                               |
| `neon-white`                                            | celadon                                 | 12 % celadon in white / celadon L −0.15, C ×0.75 (the deep celadon ink)                                                                                                                          |
| `jade`, `jade-dim`, `jade-bright`                       | celadon, celadonDim, celadonBright      | board values by night; day dim `#5a948a` (2.70) ≥ 4.5 → `#346e65` (4.57); day bright = jade L −0.07 `#185750` (6.47)\*                                                                           |
| `aqua` `gold` `rouge` `lavender` `amber`                | indigo, gold, cinnabarText, lotus, moss | board values, ≥ 4.5: day gold `#8a6a1e` (3.92) → `#80600f` (4.53), day moss `#4f7a2a` (3.93) → `#46701f` (4.53)                                                                                  |
| `*-dim`                                                 | the accent                              | L −0.10, C ×0.8 / L +0.05, C ×0.7; then ≥ 4.5 (text grade) or ≥ 3 (`rouge-dim`, `amber-dim`)                                                                                                     |
| `*-bright`                                              | the accent                              | L +0.08 (lighter by night) / L −0.07 (more ink by day; gold C ×0.85)                                                                                                                             |
| `mark`                                                  | cinnabar                                | the board value (3.25 / 4.71, UI grade)                                                                                                                                                          |
| `token`, `token-bright`                                 | azurite 石青 `#6f9ad6` / `#2a5a9e`      | ≥ 4.5; bright L +0.08 / −0.07                                                                                                                                                                    |
| `object`                                                | a lotus violet `#a58ad6` / `#5e45a8`    | ≥ 4.5                                                                                                                                                                                            |
| `identity`                                              | the boards' rouge `#c97f93` / `#9c3b5f` | ≥ 4.5 (already)                                                                                                                                                                                  |
| `copper`                                                | ochre 赭石 `#cc8a5e` / `#93522b`        | ≥ 4.5                                                                                                                                                                                            |
| `slate-ckb`, `slate-other`                              | ghost → dim; line2                      | mix dim ghost 0.12 (day 0.18); line2 `#342c25` / `#c4b9a5`                                                                                                                                       |
| `cell-capacity` `-lock` `-type` `-data`, `data-framing` | moon, indigo, moss, celadon + indigo    | capacity = text-bright; lock indigo L +0.02 / −0.01; type moss L +0.02 / −0.03, C ×0.95; data mix celadon indigo 0.5; framing = data L −0.08, C ×0.8 / L +0.04, C ×0.8 — all ≥ 4.5               |
| `board-link`                                            | indigo + celadon                        | mix indigo celadon 0.6, ≥ 4.5                                                                                                                                                                    |
| `tile`, `tile-current`, `grid-faint`                    | the grounds, celadon                    | mix surface bg 0.5 (day elevated bg); celadon 18 % on surface (day 15 %); mix hair bg 0.5                                                                                                        |
| `flow-ckb`, `flow-fee`                                  | text → dim → ghost                      | mix text dim 0.4 (day 0.3), ≥ 3; mix dim ghost 0.55 (day 0.45)                                                                                                                                   |
| `ramp-1` … `ramp-5`                                     | celadon into `base-bg`                  | mix jade bg at 0.80, 0.64, 0.50, 0.38, 0.27 (strictly weaker, pinned)                                                                                                                            |
| `pipeline-pending` `-proposed` `-committed`             | indigo, gold, celadon                   | hue and chroma kept at OKLCH L 0.66 / 0.53                                                                                                                                                       |
| `pipeline-drop-neutral`                                 | text                                    | C ×0.5 at the fills' L (0.66 / 0.53)                                                                                                                                                             |
| `pipeline-well`, `pipeline-step-to`                     | well; white / ink                       | the well / mix elevated bg 0.5; `#f6f2ea` / text-bright                                                                                                                                          |
| `search-*`, `stream-*`                                  | the well, celadon                       | panel mix well black 0.8 / `#fffdf8`; selected, hover = celadon 8 %, 3 % on the well (10 %, 4 % on the surface); stream mix well black 0.7, 0.88 / tile, mix elevated line 0.75                  |
| `row-tag-accent-edge`, `reward-secondary`               | celadon into bg                         | 36 %; 58 % (day 50 %)                                                                                                                                                                            |
| `code-fixed-fill`, `code-unspendable-fill`              | the inks                                | mix text dim 0.3, mix dim ghost 0.4 / mix dim ghost 0.8, mix ghost line 0.75                                                                                                                     |
| `elf-*`                                                 | framing, celadon, moss, gold, lotus     | header = framing; program headers framing L +0.07; rodata mix celadon moss 0.55; eh-frame rodata C ×0.4; data gold L +0.06 C ×0.75; meta lotus L +0.06 C ×0.8 (day: the day accents, lighter)    |
| `gel-*`                                                 | copper, celadon                         | overlay warm white / ink; band text the deepest well / surface; btc copper pale, copper, jade-bright (day jade-dim); pure neon-white, jade-bright, jade (day jade-dim, jade, jade-bright)        |
| `tier-btc-*`                                            | moss, gold                              | ink moss C ×0.75 ≥ 4.5, lit L +0.07 (day −0.06); edges moss 36 / 52 % on bg (45 / 62 %); wash gold 28 % (30 %)                                                                                   |
| `tier-pure-*`                                           | celadon                                 | ink jade L −0.04 ≥ 4.5 (day jade), lit jade-bright; edges 34 / 50 % (38 / 55 %); wash 16 % (20 %)                                                                                                |
| `tier-centralized-edge`                                 | cinnabar                                | 38 % on bg                                                                                                                                                                                       |
| `badge-*`, `bitcoin-*`                                  | celadon, copper                         | inner = celadon 7 % / copper 6 % on the well; jade pale / light / mid = neon-white / jade-bright / their mix (day jade-dim family); bitcoin light copper 30 % in warm white, dark copper L −0.16 |
| `gem-*`                                                 | gold, celadon                           | gold-bright, gold, jade, jade-bright (day gold, gold-bright, jade-dim, jade)                                                                                                                     |
| `life-bg-*`                                             | celadon, gold on the well               | 4 % / 3 % (day 6 % / 5 % on bg)                                                                                                                                                                  |
| `sky-*`, `well-bg`, `selection-ink`                     | bg, surface, void                       | top mix surface bg 0.35, mid = bg, foot mix void bg 0.4 (day: elevated 30 % at the foot); well-bg = the well; selection `#f6f2ea` / ink                                                          |
| `crystal-*`, `mount-*`                                  | celadon, ochre                          | artwork, the same in both themes: a celadon crystal (`#b7d9d0`, `#93c1b6` stepped) on an ochre mount (`#cc8a5e` stepped)                                                                         |

\* The boards' day celadonBright `#8fb9b0` is 1.67:1 on the day elevated ground, below UI grade; by
day "bright" is more ink, not more light, as for every other accent.

Materials (`MATERIALS`): `scanline-ink` black 22 % / the day ink `#1f1b17` at 4.5 % (CRT
scanlines by night, ruled paper by day, 1 px in every 3); `vignette` black 18 % / ink 3 % (half
the midnight strength); `grain-opacity` 0.02 / 0.03; `glow` 1 / 0; `glass-fill` the surface,
opaque, and `glass-sheen` / `-top-edge` / `-bottom-edge` / `-ring` transparent (a frame is a tile:
the surface under its own 1 px border); `well-inset` black 60 % / ink 8 % (the boards' `inset`),
`well-lip` transparent; `tier2-ground` the well at 90 % / elevated at 90 %; `header-strip` the
surface, opaque, in both themes (the vitals strip is the page's own ground drawn solid, so the page
never shows through it); `seal-press` 0.16 / 0.08, the opacity of the newest block's seal pressed behind the
404 verse (the most each theme allows with every ink of the verse and its facts text-grade across a
stroke). A halo is the boards' bloom: 6 px of celadon at 45 % (`0 0 6px rgb(var(--rgb-jade) /
calc(0.45 * var(--m-glow)))`), out by day.

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
