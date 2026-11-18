# Responsive Layout

_Date: 2026-11-01 · Status: Principles adopted · Scope: every page of the frontend_

ckbadger is read on a 1280 px desktop, in a narrow desktop window or portrait tablet (640–767 px,
no touch assumed) and on a phone (390 px reference, 360 px minimum). The same information is shown
at every width; only its arrangement changes. This document holds the rules; the per-page audit
and the execution plan that applied them live under `docs/superpowers/` (local, not tracked).

## Reference widths

Content width inside the 16 px page gutter:

| Viewport | Content width | Note                                          |
| -------- | ------------- | --------------------------------------------- |
| 1280     | 1232          | desktop reference                             |
| 640–767  | 608           | the Tailwind `container` locks at 640 px here |
| 390      | 358           | phone reference                               |
| 360      | 328           | phone minimum                                 |

The 640–767 band has one content width to design for. Below 640 the container is fluid.

## Principles

1. **Content adapts to the width; it never overflows it.** The header, the vitals strip and the
   first screen of a page are laid out to fit the width they are given. Horizontal scrolling is
   reserved for regions that are lists by nature (tables, raw data, long hashes); those scroll
   inside their own box with the page's thin scrollbar, never the platform bar. Nothing in a sticky
   header scrolls sideways, and a visualization that is the page's subject never scrolls inside
   itself.
2. **A narrow form shows every item the wide form shows.** Hierarchy comes from arrangement and
   typography, never from dropping information (the same rule as the activity feed). A strip that
   is one row on a desktop becomes a few designed rows on a phone, grouped by meaning — not an
   organically wrapped bag of items, and not a row to pan sideways.
3. **Fixed-geometry visualizations re-flow; they never scale.** A canvas drawn on a pixel grid
   (the transaction pipeline: a 15 px cell on a 17 px pitch, 99 px plates) keeps its units at full
   size. In less width it shows fewer whole units; when the row form cannot hold its first unit the
   canvas changes orientation or arrangement (the pipeline stands on end as a vertical timeline on
   a phone), keeping its materials and encodings. Scaling the canvas makes its text unreadable; a
   half-cut unit shows a broken object; folding a row into two rows breaks the flow it depicts.
4. **The measured width decides, not the breakpoint.** A component that lays itself out from
   numbers (the pipeline, the charts, a table with column minimum widths) measures its container
   and picks the arrangement from what fits. CSS breakpoints are for CSS-only layouts (the strip,
   grids of cards).
5. **One visual language across widths.** The same label/value pairs, the same materials, the
   same colours, type sizes at most one step smaller — so a reader recognises the page on a phone.

## Rules for the shared pieces

- **Header search field**: the long placeholder `search block / tx / address / cell` (285.6 px at
  the 14 px body step, `lib/type-metrics.ts`) only when the field's measured text box holds it
  whole, `search` otherwise, chosen before paint (principle 4; a field 351.6 px wide from `md`:
  the header gives about 169 px at 768–1023, 368.6 px at 1024 with one network but less beside the
  network switcher, 460.8 px at 1280). The `/` key is drawn from `md`.
- **Vitals strip** (every page, under the header): one wrapping row from `md` up, the same on
  every page. Below `md` the home page keeps the whole strip in three groups on their own lines —
  block number with the live mark, the epoch with its bar, `index / length` and the time left, the
  three mining figures. Every other page has one line below `md`: the block number, the epoch with
  its bar, and the live mark at the end (its dot alone below 640; the word stays for a screen
  reader, and the status line's mode block says it). The mining figures and the epoch's position
  and time left are the home page's below `md` — a ruled exception to principle 2 (UI review
  2026-11-14, PA-14): with the 52 px header and the 26 px status line the three lines took 212 of
  an 844 px first screen on every page. At 360 the line was 280.6 of 328 px at the 11 / 13 px
  steps (readability R4 moved them to 12 / 14 px: about 8 % wider, re-measured at the gate); page
  content starts at about y = 111 at 390 (53 px header, 42 px strip, its 16 px margin).
- **Status line** (every page; `components/layout/status-line.tsx`): the terminal session's last
  row, fixed at the foot of the window, 26 px on the elevated ground under a 1 px line; `body`
  keeps 26 px of bottom padding so no page ends under it. From `md` up it is one row — the mode
  block (`LIVE` / `SYNCING` / `OFFLINE`), the network, the tip, the release, the key hints, the
  clock — and it must fit at 768 px (measured: 753 of 753 px with 26 px to spare, so a release
  string five or more characters longer than `0.8.2` would clip the clock). Below `md` it is one
  row of the mode, the network, the tip and the clock; the release and the hints are the wide
  form's only (a touch screen has no keys). From `md` the release is in the status line and the
  clock's zone in the `?` help; below `md` the footer's last row says both — designed, not
  dropped. Never two rows, never a sideways scroller, never over the page's last prompt.
- **Page end** (every page, the not-found and error pages too): the footer is the page's last
  quiet row, then the empty prompt (the cinnabar square and the cursor), the last thing before the
  status line. The footer's "keyboard shortcuts" is drawn from `md` only; below `md` the footer
  ends instead on a line of its own with the release and the clock's zone, as in
  `ckbadger 0.8.2 · asia/shanghai`: each unbreakable, the line breaking only between them (at 360
  the links take two lines, 256.0 and 263.2 of 328 px, and this one 216.0).
- **Transaction pipeline** (homepage): a block is a plate 99 px wide; one row down to a 589 px
  canvas (two whole plates); below that a vertical timeline (the mempool and proposed wells, the
  dashed next block and the plates as rows 44 px tall, taller only for a 3×3 or 4×4 square or a
  line of uncles, joined by one strap that runs down through their stones, gilt through the
  proposal window). Never scaled, never folded, a plate never narrower than 99 px.
- **Lists and tables**: a table is shown when the measured container holds every column's minimum
  width; otherwise each row is a designed phone row of two or three lines that keeps every column
  (no generic label/value dump). Twenty rows per page below `md`.
- **Analytic tables** that explain a page (balance changes, script groups, peer evidence) get a
  phone form of their own; they are never a sideways scroller in a first screen.
- **Scrollbars**: painted in the page's palette and thin inside the page (`app/globals.css`), so a
  region that legitimately scrolls shows no platform-grey bar.
- **Touch**: anything a hover reveals must also be reachable by a tap that pins a readout under the
  figure; hints say "tap or hover".
- **Non-breaking units**: an address with its lock label, a number with its unit, an outpoint with
  its index, a date, a shortened id, a hyphenated term in a label or a figure — never broken
  inside; a count breaks only between its number and its noun, where it is wider than its line.
  Running prose (paragraphs, explanatory list items) may break at a hyphen as ordinary
  typesetting; the rule covers units in labels, figures and rows, and a date or an id in prose. A
  full id in a page's head (a name plate, an asset's id under its title) stands on one line where
  the measured line holds it, else in equal lines, each one unit (a hash as `0x` and 32 digits,
  then the last 32 indented 2ch; an address in as few equal lines as hold it), never a scroller;
  full ids in the evidence tier and in tables keep their own-box scroller.
- **Formats** (`frontend/lib/shorten.ts`, `frontend/lib/format-unit.ts`; UI review 2026-11-14,
  U-09, CH-14, PA-22; readability 2026-11-18, R9): a shortened id has three sizes, counted in
  characters of the value as written (`0x` and `ckb1` included). A hash is `head` 12…8, `row`
  10…6 and `tight` 6…4. An address keeps its network and its identity: `ckb1…` and its last 8
  characters at `head` and `row` (`ckb1…4s93hhsj`, 13 characters), its last 6 at `tight`
  (`ckb1…93hhsj`, 11), never more of its head: the characters after `ckb1` are the address format
  and the lock's code hash, the same for every address of a lock (two default-lock addresses share
  their first 58 characters), so they tell no two rows apart. A lock is named beside the address
  (the row's lock label), never by the head of it. `head` is for an h1 and a title line only;
  prose (a lede, a sentence, a note) takes the `row` form, for a hash too, as do tables, feeds and
  tiles; `tight` only below `sm` (a phone row) and in an SVG or tile label under 200 px. A page
  writes one id in one form at `sm` and wider; a list with a table and a phone row takes the size
  of the form it is drawn in, and a phone row that is also drawn in a desktop column (a list whose
  table needs more than its column) or a tile under 200 px only on a phone asks
  `useTightBelowSm` (tight below `sm`, the row size from it). Any other hex id takes the hash's
  sizes; an out point is its hash and `:N`; a value no longer than its shortened form is written
  whole. The ellipsis is the one character `…`, never `...`. A renderer takes a size, never a
  length, and `head` and `tight` are chosen only where the gate lists them with their reason
  (`__tests__/lib/shorten-gate.test.ts`). A
  percentage has two decimals and its sign with no space (`24.79%`); a compact count or CKB amount
  takes a lowercase `k` for thousands, then `M`, `B`, `T`; a physical quantity keeps its SI prefix
  with its symbol (`PH/s`, `kB`); a figure writes the symbol (`s`, `h`, `d`, `B`), prose the word.
  A row of a live list (blocks, transactions, activities, the pool) says its age (`20s ago`) with
  the full date-time in its hover; a list that is historical by nature keeps the date.

## Check

Every page is checked at 1280, 640 and 390 px before it ships: no horizontal page scroll, no
sideways-scrolling header strip, no scaled-down visualization, no item missing from the narrow
form, no text under 12 px (readability R4: the meta step, `lib/type-scale.ts`; SVG text held to it
by `lib/chart-type.ts` and its gate), the first data row of a list inside the first phone screen. The audit
tooling (a Playwright script that measures page overflow, inner scrollers, clipped elements and
page height, and cuts full-page screenshots into readable segments) is kept with the audit under
`docs/superpowers/reviews/`.
