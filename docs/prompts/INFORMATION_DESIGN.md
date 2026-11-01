# Information Design

## Three Information Layers

ckbadger information is organized into three layers, from low to high:

1. Raw Data (Syntax Representations, Facts)
2. Domain Knowledge (Semantics, Opinions)
3. Aggregations and Intelligence

### Layer 0: Raw Data (Syntax Representations)

This layer contains on-chain primitive objects:

- blocks
- transactions
- cells
- scripts
- hardforks

Raw data is the factual foundation, because they come from and are verified by CKB nodes, however it should not be the default entry point for most users.

The append-only store should record all happened histories.

### Layer 1: Domain Knowledge (Semantics)

This layer contains user-facing semantic concepts:

- addresses
- assets
- asset standards
- activities
- Nervos DAO
- canonical chain and reorgs

Domain knowledge are like opinions about fact, for example, the activities of a transaction can be interpreted as either two payments or a swap, different nodes may see different canonical chain tip and reorgs. This is the primary focus of information presentation and navigation.

Reorgs could change opinions but don't change facts. The domain store should keep latest opinions.

### Layer 2: Aggregations and Intelligence

This layer contains higher-order semantics derived from syntax + semantics through broad synthesis and deep analysis, including:

- statistics
- user identity signals
- asset flow
- user intent inference
- historical analysis
- other advanced analytical conclusions

This layer has high value, but reliable automation is hard. Its display priority is medium.

The domain store should keep stats and intelligence.

## Information Display Priority

1. Highlight Domain Knowledge (Layer 1) as the default view and main navigation model.
2. Present Aggregations and Intelligence (Layer 2) as supportive, value-added context.
3. Keep Raw Data (Layer 0) always reachable for verification and deep investigation.

## UI/UX Design Principles

### 1) Intra-Layer Connectivity

Concepts within the same layer should be cross-linked to support continuous exploration.

Examples:

- address <-> activities
- asset <-> activities
- transaction <-> cells

### 2) Cross-Layer Traceability

Higher-layer concepts must link to their immediate lower-layer evidence so users can verify interpretation with concrete data.

Examples:

- statistics -> related assets/addresses -> source transactions/cells
- user identity signal -> activity evidence -> raw scripts/cells

### 3) Domain-First Interaction

The default UX should start from domain knowledge, while each important node should support:

- jumping upward to aggregations and intelligence
- drilling downward to raw data

## Layout Across Widths

ckbadger is read on a 1280 px desktop, in a narrow desktop window or portrait tablet (640–767 px,
no touch assumed) and on a phone (390 px reference, 360 px minimum). The same information is shown
at every width; only its arrangement changes.

1. **Content adapts to the width; it never overflows it.** The header, the vitals strip and the
   first screen of a page are laid out to fit the width they are given. Horizontal scrolling is
   reserved for regions that are lists by nature (tables, raw data, long hashes); those scroll
   inside their own box with the page's thin scrollbar, never the platform bar. Nothing in a sticky
   header scrolls sideways.
2. **A narrow form shows every item the wide form shows.** Hierarchy comes from arrangement and
   typography, never from dropping information (the same rule as the activity feed). A strip that
   is one row on a desktop becomes a few designed rows on a phone, grouped by meaning — not an
   organically wrapped bag of items, and not a row to pan sideways.
3. **Fixed-geometry visualizations re-flow; they never scale.** A canvas drawn on a pixel grid
   (the transaction pipeline: 10 px cells, 89 px cards) keeps its units at full size. In less width
   it shows fewer whole units, and when a row cannot hold its first unit it wraps like a line of
   text, keeping its reading direction and materials. Scaling the canvas makes its text
   unreadable; a half-cut unit shows a broken object.
4. **The measured width decides, not the breakpoint.** A component that lays itself out from
   numbers (the pipeline, the charts) measures its container and picks the arrangement from how
   many whole units fit. CSS breakpoints are for CSS-only layouts (the strip, grids of cards).
   The Tailwind `container` is 640 px wide for every viewport from 640 to 767 px, so that band has
   one content width to design for.
5. **One visual language across widths.** The same label/value pairs, the same materials, the
   same colours, type sizes at most one step smaller — so a reader recognises the page on a phone.

Content widths inside the 16 px page gutter:

| Viewport | Content width | Note                       |
| -------- | ------------- | -------------------------- |
| 1280     | 1248          | desktop reference          |
| 640–767  | 608           | the container locks at 640 |
| 390      | 358           | phone reference            |
| 360      | 326           | phone minimum              |

## Interaction Goals

- Help users understand semantics first, then validate details.
- Ensure every high-level conclusion can be traced back to verifiable on-chain facts.
- Keep exploration in a closed loop: raw data -> domain knowledge -> intelligence -> raw data.

## Implementation Checklist

- Every core page includes at least one intra-layer navigation link.
- Every high-level card or metric includes a source/evidence entry to lower layers.
- Every domain page includes a clear raw-data drill-down path.
- Aggregation pages clearly state analysis scope and source object boundaries.
- Every page is checked at 1280, 640 and 390 px: no horizontal page scroll, no sideways-scrolling
  header strip, no scaled-down visualization, no item missing from the narrow form.
