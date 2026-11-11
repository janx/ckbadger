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
| 1280     | 1248          | desktop reference                             |
| 640–767  | 608           | the Tailwind `container` locks at 640 px here |
| 390      | 358           | phone reference                               |
| 360      | 326           | phone minimum                                 |

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
   (the transaction pipeline: 10 px cells, 89 px cards) keeps its units at full size. In less width
   it shows fewer whole units; when the row form cannot hold its first unit the canvas changes
   orientation or arrangement (the pipeline stands on end as a vertical timeline on a phone),
   keeping its materials and encodings. Scaling the canvas makes its text unreadable; a half-cut
   unit shows a broken object; folding a row into two rows breaks the flow it depicts.
4. **The measured width decides, not the breakpoint.** A component that lays itself out from
   numbers (the pipeline, the charts, a table with column minimum widths) measures its container
   and picks the arrangement from what fits. CSS breakpoints are for CSS-only layouts (the strip,
   grids of cards).
5. **One visual language across widths.** The same label/value pairs, the same materials, the
   same colours, type sizes at most one step smaller — so a reader recognises the page on a phone.

## Rules for the shared pieces

- **Vitals strip** (every page, under the header): one wrapping row from `md` up; below `md` three
  groups on their own lines — block number with the live mark, the epoch with a bar that takes the
  free width, the three mining figures.
- **Status line** (every page; `components/layout/status-line.tsx`): the terminal session's last
  row, fixed at the foot of the window, 26 px on the elevated ground under a 1 px line; `body`
  keeps 26 px of bottom padding so no page ends under it. From `md` up it is one row — the mode
  block (`LIVE` / `SYNCING` / `OFFLINE`), the network, the tip, the release, the key hints, the
  clock — and it must fit at 768 px (measured: 753 of 753 px with 26 px to spare, so a release
  string five or more characters longer than `0.8.2` would clip the clock). Below `md` it is one
  row of the mode, the network, the tip and the clock; the release and the hints are the wide
  form's only (a touch screen has no keys; the release is in the `?` help) — designed, not
  dropped. Never two rows, never a sideways scroller, never over the page's last prompt.
- **Transaction pipeline** (homepage): one row down to a 589 px canvas (two whole blocks); below
  that a vertical timeline (mempool, proposed, NEXT, the tip, older blocks as rows; one vertical
  chain line in the gaps). Never scaled, never folded, cards never smaller than 89 px.
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
  its index, a date, a hyphenated term — never broken inside.

## Check

Every page is checked at 1280, 640 and 390 px before it ships: no horizontal page scroll, no
sideways-scrolling header strip, no scaled-down visualization, no item missing from the narrow
form, no text under 10 px, the first data row of a list inside the first phone screen. The audit
tooling (a Playwright script that measures page overflow, inner scrollers, clipped elements and
page height, and cuts full-page screenshots into readable segments) is kept with the audit under
`docs/superpowers/reviews/`.
