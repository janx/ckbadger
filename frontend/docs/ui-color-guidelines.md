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

4. `Padding and decoration` (never a word, never a figure)

- `text-text-ghost`; for marks and rules also the quiet slate (`ink-quiet`) and the muted ink
  (`ink-muted`), both under text grade

### The ghost ink

The ghost (`text-text-ghost`, about 1.9:1 by night and 2:1 by day) is below every text grade on
purpose: it is for the **padding zeros** of a number (the trailing zeros that keep eight decimals
aligned, a leading zero) and for **pure decoration** (the `#` of a comment line, a separator, an
arrow between two tiles, a rule). It never carries information. A section heading's note, a hint
("tap or hover …"), a caption, a footnote, a field's name, a count, "occupied ±N" and a tick are
information: they are the dim ink. A link inside a note names no ink of its own: it keeps the
note's dim and takes the page's link hover (`a:hover`).

The rule is about what an ink is used for, not its name: the quiet slate (`ink-quiet`, 2.3–2.7:1
by night, 2.7–3.3:1 by day) and the muted ink (`ink-muted`, pinned at 3:1 as a mark's ink) are
under text grade too. Like the ghost they draw marks — a rule, a tick, a swatch, a frame, the
separator between two names (↔, →) — and never a word or a figure: an offset, a date, a day label,
a caption, a framing byte's hex, "vs", a dash that stands for "none" are the dim ink. Where a quiet
ink used to set two kinds of bytes apart (framing from fields, padding from sections), the dim ink
against the other's own tone does it, both readable. The gate refuses all three on words.

A value that **is** zero is a datum, not padding: "fee 0 CKB", a zero balance, a tier with 0
objects, an empty filter's "0" are the dim ink. Only the padding zeros of a non-zero number are
ghost (`ExactCkb` marks its datum zero `data-zero`, its padding `data-ghost`).

An ink is never thinned by an alpha modifier (`text-text-dim/60`, `text-aqua/70`): a quieter ink is
a named key at 4.5:1 (`jade-dim`, `aqua-dim`, `lavender-dim`, `gold-dim`, `text-dim`).
`__tests__/lib/ink-gate.test.ts` fails on the ghost, an alpha-thinned ink or an element faded by
`opacity-*` below 1 outside its allowlist, each site there with its reason (padding zeros,
decoration, a linked highlight while the reader points, a mark with no word, a disabled control).

The same rules hold for SVG text and inline styles. A word in SVG takes its ink as a `fill` at full
strength: never a `fillOpacity`, `opacity` or `strokeOpacity` below 1 on a `<text>` / `<tspan>` or
on a group that holds one, never a style `opacity` below 1 on an element that holds words, and
never the ghost as its `fill` or `color` (`var(--c-text-ghost)`, `color('text-ghost')`) — a chart's
tick label, a legend and a caption are figures and words, so the dim ink. A shape's alpha (a bar,
a dot, an area, a line) is its strength, not an ink, and stays. The gate reads the JSX as well as
the classes: an element faded or painted in the ghost by an attribute or a style fails it when it
holds words (SVG text, or JSX text among its children), outside the allowlist of pointer states.

A thing that steps back with time steps back by its marks, never by its words: an older block in
the pipeline keeps its plate and its number, "uncle" and "+N" at full strength in their own inks,
and its marks step back by their fill, mixed toward the ground they sit on (the squares and the
cellbase toward their cut, the dots toward the plate), never by an opacity.

### Set back

A row or a tile that is context, not the subject (a cellbase row in the transactions list, the
cells a transaction structure does not follow on the cell page, a neighbour's row in an identity's
feed) is **set back**: the quiet variant, the class `quiet` (`app/globals.css`), which draws every
word in it — its key, its figures, its links — in the dim ink. Nothing is faded with opacity: at
55 % the hash of a cellbase row fell to 3.6:1 by day, its reward to 2.1:1, and its stone turned
pink, an ink the seals do not have. A seal on a set-back row keeps **its own state's ink at full
strength** (its SVG is outside the rule), because a seal's ink is its state: a cellbase is in
force, so its stone stays the pressed cinnabar; a consumed cell's stone is the dead ink whether
set back or not. The padding zeros inside keep the ghost.

### The fraction inks

The digits after an amount's decimal point are a step quieter than its integer, in a named ink of
the amount's sign at body contrast — never the sign's ink at an opacity (78 % put the cinnabar at
3.7:1 by night and the celadon and the neutral at about 3.2–3.7:1 by day). `AMOUNT_INK` (beside
`ExactCkb` in `components/activity-event-row.tsx`) is the one table every amount renderer reads:

| Sign     | Integer         | Fraction digits  |
| -------- | --------------- | ---------------- |
| positive | `text-positive` | `text-jade-dim`  |
| negative | `text-negative` | `text-rouge-dim` |
| neutral  | `text-text`     | `text-text-dim`  |

SVG text reads the same table: each sign's `fractionKey` (`jade-dim`, `rouge-dim`, `text-dim`) is
the palette key of its fraction class, drawn as `fill={color(AMOUNT_INK[sign].fractionKey)}` on
the fraction's `<tspan>` (the DAO history's weekly end label), never the sign's ink at
`fillOpacity`.

A display figure's decimals (the DAO deposit, a Fiber channel's capacity, a script's CKB) take
their own dim step at the hero size; their padding zeros are the ghost.

## Named accent colors (rubbing × Song print)

The token names are the roles the pages were written against; the values are the boards' inks.

| Token      | Ink                      | Night     | Day       | Usage                                                                      |
| ---------- | ------------------------ | --------- | --------- | -------------------------------------------------------------------------- |
| `jade`     | celadon 青瓷             | `#93c1b6` | `#2f6b63` | Active, interactive, nav, the data colour, phosphor                        |
| `mark`     | cinnabar 朱砂            | `#c2452d` | `#b5331f` | Prompt squares, the seals' ink in force, the search square — UI grade only |
| `rouge`    | cinnabar text 朱砂       | `#d9634a` | `#a8392a` | Error, failure (cinnabar as text)                                          |
| `gold`     | old gold 古金            | `#c9a45c` | `#80600f` | Warning, DAO, emphasis values                                              |
| `aqua`     | indigo 靛青              | `#7fa3c9` | `#2e5f8f` | Informational, links                                                       |
| `lavender` | lotus 藕荷               | `#a797c4` | `#5e4d9a` | Special; a chart hue                                                       |
| `identity` | rouge 胭脂 (the boards') | `#c97f93` | `#9c3b5f` | The identity class (`CLASS_COLORS.identities`)                             |
| `token`    | azurite 石青             | `#6f9ad6` | `#2a5a9e` | The token class (`CLASS_COLORS.tokens`)                                    |
| `object`   | lotus violet             | `#a58ad6` | `#5e45a8` | The object class (`CLASS_COLORS.objects`)                                  |
| `copper`   | ochre 赭石               | `#cc8a5e` | `#93522b` | Bitcoin: RGB++, the BTC tiers (`BITCOIN_COLORS.copper`)                    |
| `amber`    | moss 苔绿                | `#a3bf7a` | `#46701f` | A chart hue (one-to-one for the old burnt orange)                          |

`mark` and `rouge` are both cinnabar: `mark` is the pigment as a shape (≥ 3:1, never body text),
`rouge` the same red lifted (by night) or deepened (by day) to body-text contrast.

A seal (the seals by kind: `lib/seal/`, `components/ui/seal.tsx`) is pressed in one of three inks,
and the ink is its state: `mark` for what is in force (a block, a committed transaction, a live
cell, a script); `text-dim`, the dead ink, for a consumed cell or an uncle; and for what is not yet
committed (a pool transaction and the cells it would create) a dashed stone in `mark`, its field at
half strength. A row stone's text is cut in `base-bg`, the page ground. `Seal` passes no other
paint, and the library refuses one that is not a palette reference or a hex (`assertPaint`).

The grounds step the other way by day: the page is paper (`base-bg`), a surface is a whiter sheet
(`base-surface`), the well is the whitest (`base-void`, under an inset), and `base-elevated` is a
darker tint (a hover, a raised panel) rather than a lighter step.

### Rules and tracks on ruled paper

By day the page is ruled paper: the day ink at 4.5 % in 1 px of every 3, over everything
(`scanline-ink`). On it the hair (`base-border-subtle`) and the elevated tint are 1.09:1 and
1.03:1 against a ruled line, so a row rule drawn in them is one more ruled line and a bar's empty
track is not there. Two keys of their own carry them: **`row-rule`** for every 1 px horizontal
rule between the rows of a list or a table (under its head or its tabs, above a block it closes;
`border-row-rule`, `divide-row-rule`) and **`bar-track`** for a bar's empty track and a cell's
free capacity (`bg-bar-track`, `FREE_CAPACITY_COLOR`). By day each clears both the paper between
rules and a ruled line by 1.25:1; by night they are the hair and the elevated tint, unchanged.
The ground decides its rule: inside `.tier2-ground` the two keys resolve to **`row-rule-deep`**
and **`bar-track-deep`** (the class re-points `--c-` / `--rgb-row-rule` and `-bar-track`), which
clear the darker tier-2 ground and a ruled line on it by 1.25:1 by day; no class changes. The
frames, chips and the page header's edges keep the hair; a chart's gridlines and base line are rules (`CHART_GRID_COLOR`, `FORK_COLORS.grid` / `.axis`: the row-rule key). A **tile** is the surface under its own
1 px border (`slate-other`, the tile outline): its fill alone is about 1.03:1 on the tier-2 ground
by day.

### Words on a fill

A word inside a filled shape (a bar segment, an icicle box, a banner) is read on the fill, not on
the page: its ink is the one `inkOnFill(theme, key, alpha)` (`lib/palette.ts`) picks for that
fill's own luminance over its ground in the theme in force — the well (`base-void`) or the bright
ink (`text-bright`), whichever reads stronger — never an ink chosen by the segment's rank or by
hand per node, and never the dim ink or an opacity on a fill: every word on one fill is in that
one ink. `__tests__/lib/fill-ink.test.ts`, `segment-ink.test.tsx` and `icicle-ink.test.tsx` pin
every labelled fill at 4.5:1 in both themes. A gradient fill under one label (the allele gel's
bands, the plate-wide ink at the band's strength over the plate the gel sits on) is read at every
colour under the word, and the label takes the ink that reads best on the worst of them
(`inkOnColours`); `allele-band-ink.test.tsx` pins it, and lists the bands where neither ink reaches
4.5:1.

A pipeline block's plate (`pipeline-plate`) is a ground of its own, and its words are chosen for
it: the number in `text-bright`, "uncle" and every "+N" in `text`, one step up from the dim
(`text-dim` is 4.1 / 4.0:1 on the plate), so each reads at 4.5:1 on the plate at every age.

A block is joined to its parent by one strap through their heads (down through their stones in
the narrow column): in the plate's tone across the gap between them, and gilt (`pipeline-gilt`)
across the plates and the joints inside the proposal window, the block's stone standing on it.
The gilt is a ground for words as the plate is: `text-bright` and `text` read at 4.5:1 on it in
both themes and `text-dim` is not used on it, so a block number inside the window stays in
`text-bright` and no gold word stands on the gilt (`gold` is 4.2:1 on it by night, its bright
step 3.8:1 by day); it is never faded and never dimmed.
`palette.test.ts` pins the inks on the plate and on the gilt, and `pipeline-panel.test.tsx`
computes every word the canvas writes against what it stands on.

### The display figure

A topic page's one display figure (40 px, `text-display`) is **gold** on the DAO page (the DAO's
own colour, the only gold text in tier 1), **celadon** (`jade`) when it is a live count — the
network's reachable nodes, Fiber's open channels — and the **bright ink** (`text-bright`)
otherwise (forks, scripts, a script family). Its decimals are a dim step at the hero size and its
padding zeros the ghost.

## How every key is derived

Source values are the boards' `THEMES` (lib.js). "mix A B t" is an sRGB mix, t of A; "L ±x" and
"C ×y" are OKLCH lightness and chroma moves with the hue kept; "≥ 4.5" / "≥ 3" means moved in
lightness only (lighter by night, darker by day) until it clears that ratio on all four grounds.

| Role                                                    | Source                                  | Rule (night / day)                                                                                                                                                                                                                 |
| ------------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `base-void` `-bg` `-surface` `-elevated`                | well, bg, surface, elevated             | the board values: `#0f0d0b #15120f #1b1714 #241f1a` / `#fbf9f4 #f3eee4 #faf7f0 #e9e2d4`                                                                                                                                            |
| `base-border`, `base-border-subtle`                     | line, hair                              | `#2a231d #221c17` / `#d4cab8 #e3dbcc`                                                                                                                                                                                              |
| `row-rule`, `bar-track`                                 | hair, elevated                          | the hair and the elevated tint by night (`#221c17 #241f1a`); by day L −0.042 / −0.063 until each clears the paper and a ruled line (`#e9e5db`) by 1.25: `#d5cdbf` (1.36 / 1.25), `#d4cdc0` (1.37 / 1.26)                           |
| `row-rule-deep`, `bar-track-deep`                       | row-rule, bar-track                     | the same by night (1.14 / 1.18 on the night tier-2 ground `#100e0b`); by day L −0.030 until each clears the tier-2 ground (`#eae3d6`) and a ruled line on it (`#e1dacd`) by 1.25: `#cbc3b5` (1.37 / 1.26), `#cac3b6` (1.37 / 1.26) |
| `text-bright`, `text`, `text-ghost`                     | moon, text, ghost                       | the board values                                                                                                                                                                                                                   |
| `text-dim`                                              | dim `#8a8073` / `#6f6659` (4.21 / 4.38) | ≥ 4.5 → `#908679` (4.57) / `#6c6356` (4.58)                                                                                                                                                                                        |
| `ink-quiet`, `ink-muted`                                | ghost → dim                             | mix dim ghost 0.32 / 0.62 (muted ≥ 3); day 0.45 / 0.83 (muted ≥ 3)                                                                                                                                                                 |
| `neon-white`                                            | celadon                                 | 12 % celadon in white / celadon L −0.15, C ×0.75 (the deep celadon ink)                                                                                                                                                            |
| `jade`, `jade-dim`, `jade-bright`                       | celadon, celadonDim, celadonBright      | board values by night; day dim `#5a948a` (2.70) ≥ 4.5 → `#346e65` (4.57); day bright = jade L −0.07 `#185750` (6.47)\*                                                                                                             |
| `aqua` `gold` `rouge` `lavender` `amber`                | indigo, gold, cinnabarText, lotus, moss | board values, ≥ 4.5: day gold `#8a6a1e` (3.92) → `#80600f` (4.53), day moss `#4f7a2a` (3.93) → `#46701f` (4.53)                                                                                                                    |
| `*-dim`                                                 | the accent                              | L −0.10, C ×0.8 / L +0.05, C ×0.7; then ≥ 4.5 (text grade) or ≥ 3 (`amber-dim`)                                                                                                                                                    |
| `*-bright`                                              | the accent                              | L +0.08 (lighter by night) / L −0.07 (more ink by day; gold C ×0.85)                                                                                                                                                               |
| `rouge-dim`                                             | the dim step                            | ≥ 4.5, text grade like every other `*-dim` (it is the negative fraction ink): `#aa503c` L +0.094 → `#ca6c57` (4.51) / `#a6594c` L −0.033 → `#9b4f43` (4.53)                                                                        |
| `mark`                                                  | cinnabar                                | the board value (3.25 / 4.71, UI grade)                                                                                                                                                                                            |
| `token`, `token-bright`                                 | azurite 石青 `#6f9ad6` / `#2a5a9e`      | ≥ 4.5; bright L +0.08 / −0.07                                                                                                                                                                                                      |
| `object`                                                | a lotus violet `#a58ad6` / `#5e45a8`    | ≥ 4.5                                                                                                                                                                                                                              |
| `identity`                                              | the boards' rouge `#c97f93` / `#9c3b5f` | ≥ 4.5 (already)                                                                                                                                                                                                                    |
| `copper`                                                | ochre 赭石 `#cc8a5e` / `#93522b`        | ≥ 4.5                                                                                                                                                                                                                              |
| `slate-ckb`, `slate-other`                              | ghost → dim; line2                      | mix dim ghost 0.12 (day 0.18); line2 `#342c25` / `#c4b9a5`                                                                                                                                                                         |
| `cell-capacity` `-lock` `-type` `-data`, `data-framing` | moon, indigo, moss, celadon + indigo    | capacity = text-bright; lock indigo L +0.02 / −0.01; type moss L +0.02 / −0.03, C ×0.95; data mix celadon indigo 0.5; framing = data L −0.08, C ×0.8 / L +0.04, C ×0.8 — all ≥ 4.5                                                 |
| `board-link`                                            | indigo + celadon                        | mix indigo celadon 0.6, ≥ 4.5                                                                                                                                                                                                      |
| `tile`, `tile-current`, `grid-faint`                    | the grounds, celadon                    | mix surface bg 0.5 (day elevated bg); celadon 18 % on surface (day 15 %); mix hair bg 0.5                                                                                                                                          |
| `flow-ckb`, `flow-fee`                                  | text → dim → ghost                      | mix text dim 0.4 (day 0.3), ≥ 3; mix dim ghost 0.55 (day 0.45)                                                                                                                                                                     |
| `ramp-1` … `ramp-5`                                     | celadon into `base-bg`                  | mix jade bg at 0.80, 0.64, 0.50, 0.38, 0.27 (strictly weaker, pinned)                                                                                                                                                              |
| `pipeline-pending` `-proposed` `-committed`             | indigo, gold, celadon                   | hue and chroma kept at OKLCH L 0.66 / 0.53                                                                                                                                                                                         |
| `pipeline-drop-neutral`                                 | text                                    | C ×0.5 at the fills' L (0.66 / 0.53)                                                                                                                                                                                               |
| `pipeline-well`                                         | well                                    | the well / mix elevated bg 0.5                                                                                                                                                                                                     |
| `pipeline-step-to`                                      | white / ink                             | `#f6f2ea` / text-bright: what a stage hue is mixed toward as its cycles rise (the cellbase a quarter of the way), lighter by night, inkier by day; the steps used to mix toward white                                              |
| `pipeline-plate`, `pipeline-cut`                        | the boards                              | the block's plate `#2f2821` / `#ddd4c3`; the cut a transaction is set in `#13100d` / `#f8f4ec`                                                                                                                                     |
| `pipeline-gilt`                                         | the boards                              | the strap across the plates and joints of the proposal window `#564013` / `#c8b47f`; a ground for `text-bright` and `text` (≥ 4.5), never faded or dimmed                                                                          |
| `pipeline-dot-waiting` `-committed` `-expired`          | gold, celadon; the quiet dot            | the proposal dots on the plate, waiting and committed ≥ 3 on it: `#ba9a5a #63897f #48413a` / `#866619 #3d7870 #b9b3a8`                                                                                                             |
| `search-*`, `stream-*`                                  | the well, celadon                       | panel mix well black 0.8 / `#fffdf8`; selected, hover = celadon 8 %, 3 % on the well (10 %, 4 % on the surface); stream mix well black 0.7, 0.88 / tile, mix elevated line 0.75                                                    |
| `row-tag-accent-edge`, `reward-secondary`               | celadon into bg                         | 36 %; 58 % (day 50 %)                                                                                                                                                                                                              |
| `code-fixed-fill`, `code-unspendable-fill`              | the inks                                | mix text dim 0.3, mix dim ghost 0.4 / mix dim ghost 0.8, mix ghost line 0.75                                                                                                                                                       |
| `elf-*`                                                 | framing, celadon, moss, gold, lotus     | header = framing; program headers = header L +0.07 by night, L −0.07 by day; rodata mix celadon moss 0.55; eh-frame rodata C ×0.4; data gold L +0.06 C ×0.75; meta lotus L +0.06 C ×0.8 (day: the day accents, lighter)            |
| `elf-*`, text grade                                     | the row above                           | ≥ 4.5 also on the tier-2 ground (night clears): day program headers `#2d5665` (3.28 → 6.19), rodata `#3b7047` (4.40 → 4.53), eh-frame `#576a5a` (3.96 → 4.51), data `#7c6126` (4.15 → 4.53), meta `#695c9a` (3.81 → 4.53)          |
| `gel-*`                                                 | copper, celadon                         | overlay warm white / ink; btc copper pale, copper, jade-bright (day jade-dim); pure neon-white, jade-bright, jade (day jade-dim, jade, jade-bright)                                                                                |
| `tier-pure-ink-lit`                                     | celadon                                 | jade-bright                                                                                                                                                                                                                        |
| `badge-*`, `bitcoin-*`                                  | celadon, copper                         | inner = celadon 7 % / copper 6 % on the well; jade pale / light / mid = neon-white / jade-bright / their mix (day jade-dim family); bitcoin light copper 30 % in warm white, dark copper L −0.16                                   |
| `life-bg-*`                                             | celadon, gold on the well               | 4 % / 3 % (day 6 % / 5 % on bg)                                                                                                                                                                                                    |
| `sky-*`, `well-bg`, `selection-ink`                     | bg, surface, void                       | top mix surface bg 0.35, mid = bg, foot mix void bg 0.4 (day: elevated 30 % at the foot); well-bg = the well; selection `#f6f2ea` / ink                                                                                            |
| `crystal-*`, `mount-*`                                  | celadon, ochre                          | artwork, the same in both themes: a celadon crystal (`#b7d9d0`, `#93c1b6` stepped) on an ochre mount (`#cc8a5e` stepped)                                                                                                           |

\* The boards' day celadonBright `#8fb9b0` is 1.67:1 on the day elevated ground, below UI grade; by
day "bright" is more ink, not more light, as for every other accent.

Materials (`MATERIALS`): `scanline-ink` black 22 % / the day ink `#1f1b17` at 4.5 % (CRT
scanlines by night, ruled paper by day, 1 px in every 3); `vignette` black 18 % / ink 3 % (half
the midnight strength); `grain-opacity` 0.02 / 0.03; `glow` 1 / 0; `glass-fill` the surface,
opaque, and `glass-sheen` / `-top-edge` / `-bottom-edge` / `-ring` transparent (a frame is a tile:
the surface under its own 1 px border); `well-inset` black 60 % / ink 8 % (the boards' `inset`),
`well-lip` transparent; `tier2-ground` the well at 90 % / elevated at 90 %; `header-strip` the
surface, opaque, in both themes (the vitals strip is the page's own ground drawn solid, so the page
never shows through it); `seal-press` 0.16 / 0.08, the opacity of the newest block's stone (its page seal) pressed behind the
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
- Prefer semantic colors for chart series from `frontend/lib/design-tokens.ts`, as the shared
  `Chart` (`frontend/components/ui/chart.tsx`) takes them; its grid is `CHART_GRID_COLOR`
  (`frontend/lib/chart-colors.ts`), the row-rule key.
- For new chart legends, keep the same semantic mapping:
  - primary series -> jade (the jade ramp's first shade)
  - the following series -> the jade ramp (`jadeRamp(n)`)

## Review checklist

- New placeholder/separator text uses `text-text-dim`
- New helper or instruction text uses `text-text-dim` or `text-text`
- The ghost ink only on padding zeros and decoration; a zero that is a datum is dim
- Nothing set back with opacity: the `quiet` class, the seal at full strength in its state's ink
- No ink thinned with `/<n>`; fraction digits from `AMOUNT_INK`
- No SVG word thinned by `fillOpacity` / `opacity`, none filled with the ghost; ticks are dim
- A row rule is `border-row-rule`, a bar's track `bg-bar-track`; a tile has its 1 px border
- Words on a fill take `inkOnFill`; a legend's label is text, its colour on the swatch only
- Primary numbers are not rendered in dim tones
- Chart color choices come from project palette constants
- No raw Tailwind color classes (text-white, bg-gray-\*, etc.)
- A new colour is added to both themes in `lib/palette.ts` and read through a reference
- A glow is `glowAlpha` / `--m-glow`-scaled, so it goes out by day
- Checked in both themes (the header menu, or `localStorage.setItem('ckbadger.theme', 'light')`)
