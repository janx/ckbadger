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
scanline ink, the scan of a fill, `glow`). `tailwind.config.ts` emits both as CSS custom
properties — `--c-<key>` (the colour), `--rgb-<key>` (its channels, for Tailwind's opacity
modifiers) and `--m-<key>` (a material) — on `:root` for dark and under
`:root[data-theme='light']` for light.

Page code never writes a hex (`__tests__/lib/theme-tokens-gate.test.ts` fails the build if it
does). A colour is one of:

- a Tailwind class (`text-text-dim`, `bg-jade/8`, `border-base-border`) — every token in
  `tailwind.config.ts` resolves through the variables;
- a named constant from `lib/design-tokens.ts` (`CLASS_COLORS.dao`, `CELL_COLORS.lock`,
  `FORK_COLORS.grid` …), each a `color()` reference, for SVG attributes and inline styles;
- `color(key)`, `alpha(key, a)`, `glowAlpha(key, a)` or `material(key)` from `lib/palette.ts`;
- `fade(cssColor, a)` to tint a colour you only hold as a reference (a `color-mix`);
- for a 2D canvas or a WebGL shader, which cannot read a variable: `useThemePalette()` for the
  concrete hex of the theme in force (`hexAlpha(hex, a)` for an rgba). The SVG scan of a chart's
  fills reads its material the same way (`useScanFill()`): a mask is referenced by an id of its
  own chart, which a variable cannot carry.

Halos and glows are alphas multiplied by `--m-glow` (1 by night, 0 by day): write them with
`glowAlpha`, or in CSS as `rgb(var(--rgb-jade) / calc(0.45 * var(--m-glow)))`. A rule whose glow
colour comes from a component variable is switched off by day in `app/globals.css` instead.

## Contrast

**By night the text grade is perceptual** (readability plan 2026-11-18, ruling R2): an ink that
carries a word is graded by its APCA lightness contrast on the ground it is read on,
`apcaLc(ink, ground)` in `lib/palette.ts` (APCA-W3 0.0.98G-4g; the grade is the absolute value).
The floors, on the page (`base-bg`), the surface and the well, and on the elevated tint:

| Ink                                                                                                      | Page, surface, well | Elevated |
| -------------------------------------------------------------------------------------------------------- | ------------------- | -------- |
| the reading ink `text`                                                                                   | Lc ≥ 75             | ≥ 72     |
| the quiet ink `text-dim`, every accent and its `-dim` step, the classes, the cell parts, the ELF regions | Lc ≥ 60             | ≥ 58     |
| the marks: `ink-muted`, `ink-quiet`, `text-ghost` (never a word)                                         | Lc < 60             | < 60     |

and the bright, the reading and the quiet ink stand at least 10 Lc apart in that order; the marks
rise ghost, quiet, muted; an accent's `-dim` step reads under it and its `-bright` step over it.
Why: WCAG's ratio treats a light ink on a dark ground as it treats the same ratio the other way
round, and the eye does not; at the same ratio a light ink on a dark ground reads weaker. The night
inks tuned to 4.5:1 read at Lc 59 (the reading ink) and 38 (the quiet ink) where their day
counterparts read at 83 and 70, so by night the ladder was moved up to the floors above (the
boards' ruling of 2026-11-18: reading ink 76, quiet ink 61, accents and classes 70 on the page). **WCAG's 4.5:1 stays the second floor**: every text-grade ink and accent at ≥ 4.5:1 on every
ground in both themes, hover and secondary inks and the cinnabar `mark` at ≥ 3:1. By day the
perceptual floors hold as a pin the day values already clear (the day keeps its values); two day
pairs are held as known exceptions until the day is re-graded: the muted ink reads over Lc 60 on the
paper, the sheet and the well (64 / 70 / 71), and the gold's quiet step a hair over the gold (69.3
against 69.1).

Structure by night: a row rule, the hair and a bar's empty track stand off the page ground by
1.25:1 and a frame (`base-border`) by 1.40:1, as the day's rule does on paper (below).

`__tests__/lib/palette.test.ts` pins all of it, and the night values to the boards (`RUBBING`, the
boards' ruling of 2026-11-18). Change a value there and the test says what it broke. Where a value
fell short of its grade it was moved in OKLCH lightness only (hue kept, chroma kept unless the
colour leaves sRGB) until it cleared — see the derivation below.

## Text hierarchy

Three inks carry words, each by its role (readability plan 2026-11-18, ruling R3). The quiet ink is
not the default: a page set mostly in its weakest word ink has no hierarchy, its links, dates, table
values and explaining sentences as quiet as their labels.

- **bright** (`text-text-bright`; `text-emphasis`, `text-interactive` where an accent names it):
  the one figure or name a reader scans first in a block;
- **reading** (`text-text`): every other value, every link, every date and number, and every
  sentence that explains;
- **quiet** (`text-text-dim`): a label, a unit that repeats down a column, a separator, a note under
  a figure, and a set-back row (the `quiet` class, "Set back" below).

Under them, never a word nor a figure: the ghost for padding and decoration (`text-text-ghost`),
and for marks and rules the quiet slate (`ink-quiet`) and the muted ink (`ink-muted`), both under
text grade ("The ghost ink" below).

| What                     | For example                                                                                                                                          | Ink     |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| the figure scanned first | a page's h1, a hero or display figure, the height that names a block's row                                                                           | bright  |
| label                    | a `dt`, a column head, a figure's eyebrow, a kind word ("Block"), a prompt heading's args, "occupied" before its bytes, a lock's kind, a `RowTag`    | quiet   |
| value                    | a hash, an address, an out point, a name, a state ("live", "consumed"), a full id under its h1                                                       | reading |
| link                     | every link wherever it stands: a block height in a sentence, an out point in a tile, a menu's entry; it turns jade on hover                          | reading |
| date                     | "2025-01-07", "3 m ago"                                                                                                                              | reading |
| number                   | a count, a percentage, a size in bytes, a pager's "Showing 1–25 of 1,234" (a zero that is a datum stays quiet: "The ghost ink" below)                | reading |
| unit                     | the "CKB" after each amount, "B", a display figure's unit                                                                                            | quiet   |
| separator                | the "·" between the parts of a line, JSON's braces and commas, "→"                                                                                   | quiet   |
| note                     | a line under a figure that says how to read it or what it leaves out, a figure's sub-line, a hint ("tap or hover"), the word "selected" before a pin | quiet   |
| explaining sentence      | a lede, a hero's companion line, a help popover's body, a banner's detail, a paragraph that explains a mechanism, a readout's idle line              | reading |
| set-back row             | a cellbase row in the transactions list, a neighbour's row: the `quiet` class sets every word in it quiet                                            | quiet   |

A note and an explaining sentence differ by where they stand: a note qualifies the figure right
above it and is read with it; a sentence that explains stands on its own (the head's lede, a
section's paragraph, a banner). A link stays a link inside a quiet line: it takes the reading ink.

`__tests__/lib/ink-role-gate.test.ts` holds the rule where the source shows it: a link renderer
(`<a>`, `Address`, a `…Link`), a CKB or token amount that takes its parent's ink, a table cell (a
`<td>`, a `ListTable` column's class or the root of what its `render` returns) and a paragraph
(`<p>`) never carry `text-text-dim` outside a `quiet` subtree, by their own class or by the
nearest ink their file sets around them. The sites tolerated today are listed per file with their
count and a reason: the page lanes' work list, which only shrinks (a new site fails, and a site that
goes fails until its count is lowered). A paragraph that is a note under a figure stays quiet, its
count kept with that reason. `__tests__/utils/ink-role.ts` (`inkOf`) reads the ink a rendered word
is drawn in, for a component's own test of its roles.

### The ghost ink

The ghost (`text-text-ghost`, Lc 20 / about 3:1 by night and 2:1 by day) is below every text grade on
purpose: it is for the **padding zeros** of a number (the trailing zeros that keep eight decimals
aligned, a leading zero) and for **pure decoration** (the `#` of a comment line, a separator, an
arrow between two tiles, a rule). It never carries information. A section heading's note, a hint
("tap or hover …"), a caption, a footnote, a field's name, a count, "occupied ±N" and a tick are
information: they take a word's ink by their role ("Text hierarchy"), never the ghost. A link inside
a note is still a link: it takes the reading ink and the page's link hover (`a:hover`).

The rule is about what an ink is used for, not its name: the quiet slate (`ink-quiet`, Lc 28–30 /
3.6–4.4:1 by night, 2.7–3.3:1 by day) and the muted ink (`ink-muted`, Lc 43–45 / 5.4–6.6:1 by
night, pinned at 3:1 as a mark's ink) are under text grade too. Like the ghost they draw marks — a rule, a tick, a swatch, a frame, the
separator between two names (↔, →) — and never a word or a figure: an offset, a date, a day label,
a caption, a framing byte's hex, "vs", a dash that stands for "none" take a word's ink by their
role. Where a quiet
ink used to set two kinds of bytes apart (framing from fields, padding from sections), the dim ink
against the other's own tone does it, both readable. The gate refuses all three on words.

A value that **is** zero is a datum, not padding: "fee 0 CKB", a zero balance, a tier with 0
objects, an empty filter's "0" are the dim ink. Only the padding zeros of a non-zero number are
ghost (`CkbAmount` marks its datum zero `data-zero`, a column's padding `data-ghost`).

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

### Amounts

How a CKB or a token amount is written is one rule in one module (readability plan 2026-11-18,
ruling R5): the digits in `lib/amount-form.ts`, the renderers `CkbAmount` and `TokenAmount` in
`components/ui/ckb-amount.tsx`. Every renderer takes a `form` and has no default: a call site
names its context. Every form is computed in BigInt base units, never a float; a shortened figure
is the exact value **cut, never rounded** (999.995 is never 1,000.00: a rounded figure can show a
digit that is not on chain), carries the exact value and its unit in its `title`, and is never an
operand.

| Form      | What it writes                                                                                                                                                          | Example                                  |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| `exact`   | The value, its trailing zeros dropped, no point for a whole number                                                                                                      | `122,000` · `0.0003` · `559.5125321`     |
| `column`  | Exact, padded to the unit's decimals (8 for CKB), the padding zeros in the ghost, so the points of a column align                                                       | `750.00000000` · `0.00030000`            |
| `summary` | Whole from 1,000 up; at most two decimals from 1 to 1,000, trailing zeros dropped; below 1 the exact CKB value, or a token's first four significant digits; cut, titled | `8,131,306,075` · `999.99` · `0.0001234` |
| `compact` | Two decimals of the largest unit reached (k, M, B, T), two decimals below 1,000; cut, titled                                                                            | `9.02B` · `123.45k` · `999.99`           |

Where each form stands:

| Place                                                                                                                          | Form      |
| ------------------------------------------------------------------------------------------------------------------------------ | --------- |
| The amount a page is about (a cell's capacity, a transaction's fee in its balance)                                             | `exact`   |
| A chart's readout (hover, in-plot, legend, Last change): where a point's value is read, which nothing else on the page carries | `exact`   |
| A numeric table column of subject amounts, whose points align                                                                  | `column`  |
| A hero or display figure, a stat tile, a list column, a chart's head figure, a feed line, a phone row, prose                   | `summary` |
| A tile of the chart index or the home page, an axis                                                                            | `compact` |

One ink per amount: the integer and the decimals take the same ink — the sign's with
`tone="sign"`, the reading ink (`text-text`) with `tone="plain"`, the parent's with
`tone="inherit"` (a display figure in its class colour, a value inside a brighter line).
`AMOUNT_INK` (`components/ui/ckb-amount.tsx`) is that table, one ink per sign; a number's
significant digits are never quieter than its integer. The ghost is for a `column`'s padding zeros
only. SVG text writes an amount from `ckbAmountText` in the text's one fill, a shortened one with
its exact value in a `<title>` (the DAO history's weekly end label). A display figure is one run at
its size; its unit beside it is the body step in the dim ink. `__tests__/lib/amount-gate.test.ts`
fails on an amount split for display outside the module (`splitCkbAmount`, `splitTokenAmount`,
`formatCkbAmount`'s parts, read off the call or off a name bound to it). Its listed exceptions are
the home page's circulating and DAO deposited figures, set in two sizes by their board and exact
to the shannon; the home page's lane decides their form.

## Named accent colors (rubbing × Song print)

The token names are the roles the pages were written against; the values are the boards' inks.

| Token      | Ink                      | Night     | Day       | Usage                                                                      |
| ---------- | ------------------------ | --------- | --------- | -------------------------------------------------------------------------- |
| `jade`     | celadon 青瓷             | `#9ecdc2` | `#2f6b63` | Active, interactive, nav, the data colour, phosphor                        |
| `mark`     | cinnabar 朱砂            | `#c2452d` | `#b5331f` | Prompt squares, the seals' ink in force, the search square — UI grade only |
| `rouge`    | cinnabar text 朱砂       | `#ff9e88` | `#a8392a` | Error, failure (cinnabar as text)                                          |
| `gold`     | old gold 古金            | `#e4bf76` | `#80600f` | Warning, DAO, emphasis values                                              |
| `aqua`     | indigo 靛青              | `#a2c8ef` | `#2e5f8f` | Informational, links                                                       |
| `lavender` | lotus 藕荷               | `#cdbceb` | `#5e4d9a` | Special; a chart hue                                                       |
| `identity` | rouge 胭脂 (the boards') | `#fcaec2` | `#9c3b5f` | The identity class (`CLASS_COLORS.identities`)                             |
| `token`    | azurite 石青             | `#9ec6fe` | `#2a5a9e` | The token class (`CLASS_COLORS.tokens`)                                    |
| `object`   | lotus violet             | `#d0b8fe` | `#5e45a8` | The object class (`CLASS_COLORS.objects`)                                  |
| `copper`   | ochre 赭石               | `#fab487` | `#93522b` | Bitcoin: RGB++, the BTC tiers (`BITCOIN_COLORS.copper`)                    |
| `amber`    | moss 苔绿                | `#b1ce88` | `#46701f` | A chart hue (one-to-one for the old burnt orange)                          |

By night every accent and class reads at Lc 70 on the page ground (the boards' ruling of
2026-11-18; the rubbing's values of 2026-11-10 are their sources in the derivation table), lighter
and a little paler than the rubbing's.

`mark` and `rouge` are both cinnabar: `mark` is the pigment as a shape (≥ 3:1, never body text),
`rouge` the same red lifted (by night, to Lc 63: a light coral, as cinnabar cannot be both
saturated and readable on the night ground) or deepened (by day) to body-text contrast.

A seal (the seals by kind: `lib/seal/`, `components/ui/seal.tsx`) is pressed in one of three inks,
and the ink is its state: `mark` for what is in force (a block, a committed transaction, a live
cell, a script); `text-dim`, the dead ink, for a consumed cell or an uncle; and for what is not yet
committed (a pool transaction and the cells it would create) a dashed stone in `mark`, its field at
half strength. A row stone's text is cut in `base-bg`, the page ground. `Seal` passes no other
paint, and the library refuses one that is not a palette reference or a hex (`assertPaint`).
A list row's stone is small, as the home pipeline's (readability ruling R6, `ROW_SEAL`): 16 px
high for a block, a transaction and a script, 18 for a cell, so a column of rows keeps each
state's ink in less than half the red it had (a block's 16² against 26²); a page head keeps its
large seal.

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
rules and a ruled line by 1.25:1; by night the rule is the hair and the track a step of its own,
at 1.25 and 1.30:1 to the page ground (readability ruling R2), where the hair and the elevated
tint used to sit at 1.11 and 1.14:1.
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
(`text-dim` is 4.0:1 on the plate by day, and Lc 58 by night, under the word floor), so each reads
at 4.5:1 on the plate at every age (by night Lc 89 and 73).

A block is joined to its parent by one strap through their heads (down through their stones in
the narrow column): in the plate's tone across the gap between them, and gilt (`pipeline-gilt`)
across the plates and the joints inside the proposal window, the block's stone standing on it.
The gilt is a ground for words as the plate is: `text-bright` and `text` read at 4.5:1 on it in
both themes (by night Lc 82 and 66) and `text-dim` is not used on it, so a block number inside the
window stays in `text-bright` and no gold word stands on the gilt (gold on gold: `gold` is Lc 60
on it by night, its bright step 3.8:1 by day); it is never faded and never dimmed.
`palette.test.ts` pins the inks on the plate and on the gilt, and `pipeline-panel.test.tsx`
computes every word the canvas writes against what it stands on.

### The display figure

A topic page's one display figure (40 px, `text-display`) is **gold** on the DAO page (the DAO's
own colour, the only gold text in tier 1), **celadon** (`jade`) when it is a live count — the
network's reachable nodes, Fiber's open channels — and the **bright ink** (`text-bright`)
otherwise (forks, scripts, a script family). Its decimals are a dim step at the hero size and its
padding zeros the ghost.

## How every key is derived

Source values are the boards' `THEMES` (lib.js) of 2026-11-10. "mix A B t" is an sRGB mix, t of A
(rounded half up), from the keys' present values; "L ±x" and "C ×y" are OKLCH lightness and chroma
moves with the hue kept; "≥ 4.5" / "≥ 3" means moved in lightness only (lighter by night, darker by
day) until it clears that ratio on all four grounds. The night inks were re-graded by the boards'
ruling of 2026-11-18 (readability ruling R2): "Lc N" means moved in OKLCH lightness only from the
source (hue kept, chroma kept unless the colour leaves sRGB) until it reads at |Lc| N on the page
ground; "Lc N on every ground" until it reads at N on the page, the surface and the well and at
N − 2 on the elevated tint (75 and 72 for the reading ink); "a:1" alone means that WCAG ratio to
the page ground.

| Role                                                    | Source                                  | Rule (night / day)                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `base-void` `-bg` `-surface` `-elevated`                | well, bg, surface, elevated             | the well and the bg the board values `#0f0d0b #15120f`; by night the surface and the elevated tint at 1.08 / 1.17:1 `#1e1a17 #26211c` (the board's `#1b1714 #241f1a`) / `#fbf9f4 #f3eee4 #faf7f0 #e9e2d4`                                                                                                                                                                                      |
| `base-border`, `base-border-subtle`                     | line, hair                              | by night 1.40 / 1.25:1 `#362e28 #2d2621` (the board's `#2a231d #221c17`, 1.21 / 1.11) / `#d4cab8 #e3dbcc`                                                                                                                                                                                                                                                                                      |
| `row-rule`, `bar-track`                                 | hair, elevated                          | by night the hair (`#2d2621`, 1.25) and a step of its own at 1.30:1 (`#2f2924`); by day L −0.042 / −0.063 until each clears the paper and a ruled line (`#e9e5db`) by 1.25: `#d5cdbf` (1.36 / 1.25), `#d4cdc0` (1.37 / 1.26)                                                                                                                                                                   |
| `row-rule-deep`, `bar-track-deep`                       | row-rule, bar-track                     | the same by night (1.29 / 1.34 on the night tier-2 ground `#100e0b`); by day L −0.030 until each clears the tier-2 ground (`#eae3d6`) and a ruled line on it (`#e1dacd`) by 1.25: `#cbc3b5` (1.37 / 1.26), `#cac3b6` (1.37 / 1.26)                                                                                                                                                             |
| `text-bright`, `text`, `text-ghost`                     | moon, text, ghost                       | by night bright = moon L +0.015 `#ede7db` (Lc 92: the first step on its hue at which every labelled bar step reads 4.5:1, `fill-ink.test`), text Lc 75 on every ground `#d6cdbe` (76 on the page), ghost Lc 20 `#685f56`; by day the board values                                                                                                                                              |
| `text-dim`                                              | dim `#8a8073` / `#6f6659` (4.21 / 4.38) | Lc 60 on every ground → `#bdb3a4` (61 on the page; ≥ 4.5 had made it `#908679`, Lc 38) / ≥ 4.5 → `#6c6356` (4.58)                                                                                                                                                                                                                                                                              |
| `ink-quiet`, `ink-muted`                                | ghost → dim                             | by night Lc 30 / 45 from the mixes dim ghost 0.32 / 0.62 of 2026-11-10: `#7f766b #9e9589`; day mix dim ghost 0.45 / 0.83 (muted ≥ 3)                                                                                                                                                                                                                                                           |
| `neon-white`                                            | celadon                                 | 12 % celadon in white / celadon L −0.15, C ×0.75 (the deep celadon ink)                                                                                                                                                                                                                                                                                                                        |
| `jade`, `jade-dim`, `jade-bright`                       | celadon, celadonDim, celadonBright      | by night Lc 70, Lc 60 on every ground, Lc 80: `#9ecdc2 #90bdb2 #b9dcd2`; day dim `#5a948a` (2.70) ≥ 4.5 → `#346e65` (4.57); day bright = jade L −0.07 `#185750` (6.47)\*                                                                                                                                                                                                                       |
| `aqua` `gold` `rouge` `lavender` `amber`                | indigo, gold, cinnabarText, lotus, moss | by night Lc 70: `#a2c8ef #e4bf76 #cdbceb #b1ce88`; rouge, a signal, Lc 60 (C ×0.86 to stay in sRGB), then on in L to `#ff9e88` (Lc 63) to stay 2.1 Lc over its quiet step; by day board values, ≥ 4.5: day gold `#8a6a1e` (3.92) → `#80600f` (4.53), day moss `#4f7a2a` (3.93) → `#46701f` (4.53)                                                                                              |
| `*-dim`                                                 | the accent                              | L −0.10, C ×0.8, then by night Lc 60 on every ground: `#90bdb2 #99b7d6 #ceb077 #baaed2` (`amber-dim`, no word's ink, Lc 60 on the page: `#a4ba84`) / L +0.05, C ×0.7; then ≥ 4.5 (text grade) or ≥ 3 (`amber-dim`)                                                                                                                                                                             |
| `*-bright`                                              | the accent                              | by night Lc 80, the accent's + 10: `#b9dcd2 #b4d8fe #f6cf87 #bad7ff` (`rouge-bright` Lc 68: `#feac99`) / L −0.07 (more ink by day; gold C ×0.85)                                                                                                                                                                                                                                               |
| `rouge-dim`                                             | the dim step                            | text grade, like every other `*-dim` (a quiet cinnabar word; it was the negative fraction ink until R5): by night Lc 60 on every ground and under `rouge`: `#aa503c` → `#fc9982` (≥ 4.5 had made it `#ca6c57`, Lc 38) / `#a6594c` L −0.033 → `#9b4f43` (4.53)                                                                                                                                  |
| `mark`                                                  | cinnabar                                | the board value (3.18 / 4.71 at worst, UI grade; 3.25 by night on the elevated tint of 2026-11-10)                                                                                                                                                                                                                                                                                             |
| `token`, `token-bright`                                 | azurite 石青 `#6f9ad6` / `#2a5a9e`      | by night Lc 70 / 80: `#9ec6fe #bad7ff`; by day ≥ 4.5, bright L −0.07                                                                                                                                                                                                                                                                                                                           |
| `object`                                                | a lotus violet `#a58ad6` / `#5e45a8`    | by night Lc 70: `#d0b8fe` / ≥ 4.5                                                                                                                                                                                                                                                                                                                                                              |
| `identity`                                              | the boards' rouge `#c97f93` / `#9c3b5f` | by night Lc 70: `#fcaec2` / ≥ 4.5 (already)                                                                                                                                                                                                                                                                                                                                                    |
| `copper`                                                | ochre 赭石 `#cc8a5e` / `#93522b`        | by night Lc 70: `#fab487` / ≥ 4.5                                                                                                                                                                                                                                                                                                                                                              |
| `slate-ckb`, `slate-other`                              | ghost → dim; line2                      | slate-ckb, also the ground of words (the balance bar's free part, the current code-cell holder): by night the neutral of mix dim ghost 0.12 moved in lightness until text-bright reads on it at Lc 70 (`#6a6157`, 4.93:1; the mix drifted with the inks to `#72695f`, 4.37:1) / mix dim ghost 0.18; line2 by night at 1.60:1 `#3f3730` (the board's `#342c25`) / `#c4b9a5`                     |
| `cell-capacity` `-lock` `-type` `-data`, `data-framing` | moon, indigo, moss, celadon + indigo    | capacity = text-bright; lock indigo L +0.02 / −0.01; type moss L +0.02 / −0.03, C ×0.95; data mix celadon indigo 0.5; framing = data L −0.08, C ×0.8 / L +0.04, C ×0.8; by night of the inks of 2026-11-10, then lock, type and data Lc 70 and framing Lc 60 on every ground: `#a2c7f0 #b1cd8a #a1cad9 #97b9c4`; by day all ≥ 4.5                                                              |
| `board-link`                                            | indigo + celadon                        | mix indigo celadon 0.6 (by night of the inks of 2026-11-10, then Lc 70: `#a1cadc`), ≥ 4.5                                                                                                                                                                                                                                                                                                      |
| `tile`, `tile-current`, `grid-faint`                    | the grounds, celadon                    | mix surface bg 0.5 (day elevated bg); celadon 18 % on surface (day 15 %); by night 1.20:1 `#28231f` (day mix hair bg 0.5)                                                                                                                                                                                                                                                                      |
| `flow-ckb`, `flow-fee`                                  | text → dim → ghost                      | by night mix text dim 0.4 of 2026-11-10, then Lc 60 `#bab1a2` (day mix text dim 0.3, ≥ 3); mix dim ghost 0.55 (day 0.45)                                                                                                                                                                                                                                                                       |
| `ramp-1` … `ramp-5`                                     | celadon into `base-bg`                  | mix jade bg at 0.80, 0.64, 0.50, 0.38, 0.27 (strictly weaker, pinned)                                                                                                                                                                                                                                                                                                                          |
| `pipeline-pending` `-proposed` `-committed`             | indigo, gold, celadon                   | hue and chroma kept at OKLCH L 0.66 / 0.53 (of the inks of 2026-11-10: the pipeline keys did not move with the 2026-11-18 ruling)                                                                                                                                                                                                                                                              |
| `pipeline-drop-neutral`                                 | text                                    | C ×0.5 at the fills' L (0.66 / 0.53), of the text ink of 2026-11-10                                                                                                                                                                                                                                                                                                                            |
| `pipeline-well`                                         | well                                    | the well / mix elevated bg 0.5                                                                                                                                                                                                                                                                                                                                                                 |
| `pipeline-step-to`                                      | white / ink                             | `#f6f2ea` / text-bright: what a stage hue is mixed toward as its cycles rise (the cellbase a quarter of the way), lighter by night, inkier by day; the steps used to mix toward white                                                                                                                                                                                                          |
| `pipeline-plate`, `pipeline-cut`                        | the boards                              | the block's plate `#2f2821` / `#ddd4c3`; the cut a transaction is set in `#13100d` / `#f8f4ec`                                                                                                                                                                                                                                                                                                 |
| `pipeline-gilt`                                         | the boards                              | the strap across the plates and joints of the proposal window `#564013` / `#c8b47f`; a ground for `text-bright` and `text` (≥ 4.5), never faded or dimmed                                                                                                                                                                                                                                      |
| `pipeline-dot-waiting` `-committed` `-expired`          | gold, celadon; the quiet dot            | the proposal dots on the plate, waiting and committed ≥ 3 on it: `#ba9a5a #63897f #48413a` / `#866619 #3d7870 #b9b3a8`                                                                                                                                                                                                                                                                         |
| `search-*`, `stream-*`                                  | the well, celadon                       | panel mix well black 0.8 / `#fffdf8`; selected, hover = celadon 8 %, 3 % on the well (10 %, 4 % on the surface); stream mix well black 0.7, 0.88 / tile, mix elevated line 0.75                                                                                                                                                                                                                |
| `row-tag-accent-edge`, `reward-secondary`               | celadon into bg                         | 36 %; 58 % (day 50 %)                                                                                                                                                                                                                                                                                                                                                                          |
| `code-fixed-fill`, `code-unspendable-fill`              | the inks                                | fixed: mix text dim 0.3 / mix dim ghost 0.8; unspendable, set by the words the scripts bar writes on it: by night the neutral of mix dim ghost 0.4 moved in lightness until the fill-ink rule's ink on it is the bright ink and reads at Lc 70 (`#6a6155`, 4.94:1; the mix drifted with the inks to `#8a8175`, where neither ink held a word) / mix ghost line 0.75                            |
| `elf-*`                                                 | framing, celadon, moss, gold, lotus     | header = framing; program headers = header L +0.07 by night (`#abd0dc`, L +0.0704), L −0.07 by day; rodata mix celadon moss 0.55; eh-frame rodata C ×0.4; data gold L +0.06 C ×0.75; meta lotus L +0.06 C ×0.8; by night of the inks of 2026-11-10, each at Lc 60 or more on every ground (meta moved to `#baaed2`; rodata, eh-frame and data already cleared) (day: the day accents, lighter) |
| `elf-*`, text grade                                     | the row above                           | ≥ 4.5 also on the tier-2 ground (night clears): day program headers `#2d5665` (3.28 → 6.19), rodata `#3b7047` (4.40 → 4.53), eh-frame `#576a5a` (3.96 → 4.51), data `#7c6126` (4.15 → 4.53), meta `#695c9a` (3.81 → 4.53)                                                                                                                                                                      |
| `gel-*`                                                 | copper, celadon                         | overlay warm white / ink; btc copper pale, copper, jade-bright (day jade-dim); pure neon-white, jade-bright, jade (day jade-dim, jade, jade-bright); by night those of 2026-11-10 (`#f2f8f6 #b7d9d0 #93c1b6`): the artwork inks did not move with the 2026-11-18 ruling                                                                                                                        |
| `tier-pure-ink-lit`                                     | celadon                                 | jade-bright                                                                                                                                                                                                                                                                                                                                                                                    |
| `badge-*`, `bitcoin-*`                                  | celadon, copper                         | inner = celadon 7 % / copper 6 % on the well; jade pale / light / mid = neon-white / jade-bright / their mix (day jade-dim family); bitcoin light copper 30 % in warm white, dark copper L −0.16; by night of the celadon, neon-white and jade-bright of 2026-11-10 (not moved by the 2026-11-18 ruling)                                                                                       |
| `life-bg-*`                                             | celadon, gold on the well               | 4 % / 3 % (day 6 % / 5 % on bg)                                                                                                                                                                                                                                                                                                                                                                |
| `sky-*`, `well-bg`, `selection-ink`                     | bg, surface, void                       | top mix surface bg 0.35, mid = bg, foot mix void bg 0.4 (day: elevated 30 % at the foot); well-bg = the well; selection `#f6f2ea` / ink                                                                                                                                                                                                                                                        |
| `crystal-*`, `mount-*`                                  | celadon, ochre                          | artwork, the same in both themes: a celadon crystal (`#b7d9d0`, `#93c1b6` stepped) on an ochre mount (`#cc8a5e` stepped)                                                                                                                                                                                                                                                                       |

\* The boards' day celadonBright `#8fb9b0` is 1.67:1 on the day elevated ground, below UI grade; by
day "bright" is more ink, not more light, as for every other accent.

Materials (`MATERIALS`): `scanline-ink` transparent / the day ink `#1f1b17` at 4.5 % (by day the
ruled paper over the page, 1 px in every 3, landing on the paper and leaving the ink whole; by
night no layer lies over the text: on the dark ground the lines showed only where they crossed
the text, and cut its strokes); `fill-scan` the scan of a fill / `none` (by night the scan is a
property of a figure's fill: of every 3 CSS px from the fill's foot up, 1 px lets the ground
behind it through at 22 % — `FILL_SCAN`, the strength the window's layer had on a fill; by day
none); `vignette` black 18 % / ink 3 % (half the midnight strength); `grain-opacity` 0.02 / 0.03;
`glow` 1 / 0; `glass-fill` the surface, opaque, and `glass-sheen` / `-top-edge` / `-bottom-edge`
/ `-ring` transparent (a frame is a tile:
the surface under its own 1 px border); `well-inset` black 60 % / ink 8 % (the boards' `inset`),
`well-lip` transparent; `tier2-ground` the well at 90 % / elevated at 90 %; `header-strip` the
surface, opaque, in both themes (the vitals strip is the page's own ground drawn solid, so the page
never shows through it); `seal-press` 0.16 / 0.08, the opacity of the newest block's stone (its page seal) pressed on the
404 page, beside the verse or below it and clear of every line of text since readability R6 (the
values are the most each theme allowed while it stood behind the verse, every ink of the verse and
its facts text-grade across a stroke). A halo is the boards' bloom: 6 px of celadon at 45 % (`0 0 6px rgb(var(--rgb-jade) /
calc(0.45 * var(--m-glow)))`), out by day.

By night the scan is a property of a fill, never of a word. An HTML fill takes the class
`.scan-fill` (`app/globals.css`: `fill-scan` as its mask image, so the ground shows through the
hit rows); a chart's SVG areas and bars take the mask of `useScanFill()`
(`components/ui/scan-fill.tsx`), drawn in the svg's user space, so only on a chart drawn 1:1,
where 3 px stay 3 px on the screen. A fill that holds its own label (the address page's balance
bar) does not take it. A line, a tick, a label, a marker, a seal, the pipeline's plates and strap,
a tile and a word never take it; `__tests__/lib/scan-fill-gate.test.ts` holds that and lists the
elements that do.

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

- An ink by role ("Text hierarchy"): a label, a repeated unit, a separator, a note `text-text-dim`;
  a value, a link, a date, a number, an explaining sentence `text-text`; the figure scanned first
  `text-text-bright`
- The ghost ink only on padding zeros and decoration; a zero that is a datum is dim
- Nothing set back with opacity: the `quiet` class, the seal at full strength in its state's ink
- No ink thinned with `/<n>`; an amount in one ink (`AMOUNT_INK`), its digits never quieter than its integer
- An amount names its form (`exact`, `column`, `summary`, `compact`; "Amounts"); a shortened one is cut, titled with its exact value
- No SVG word thinned by `fillOpacity` / `opacity`, none filled with the ghost; ticks are dim
- A row rule is `border-row-rule`, a bar's track `bg-bar-track`; a tile has its 1 px border
- Words on a fill take `inkOnFill`; a legend's label is text, its colour on the swatch only
- Primary numbers are not rendered in dim tones
- Chart color choices come from project palette constants
- No raw Tailwind color classes (text-white, bg-gray-\*, etc.)
- A new colour is added to both themes in `lib/palette.ts` and read through a reference
- A glow is `glowAlpha` / `--m-glow`-scaled, so it goes out by day
- Checked in both themes (the header menu, or `localStorage.setItem('ckbadger.theme', 'light')`)
