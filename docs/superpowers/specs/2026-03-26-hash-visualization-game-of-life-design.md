# Hash Visualization: Conway's Game of Life

**Date**: 2026-03-26
**Status**: Approved
**Mockup**: `.superpowers/brainstorm/hash-viz-gameoflife-v4.html`

## Goal

Design a hash visualization for all pure CKB and BTC+CKB collection gallery items. The visualization uses Conway's Game of Life seeded by hash bytes, with the only semantic encoding being chain attribute (CKB vs BTC+CKB) via color. All other visual properties — cell shape, seed pattern, evolution speed — are derived from the hash itself.

## Concept: CKB Cell = Life Cell

The metaphor is structurally native, not decorative:

| CKB | Game of Life |
|-----|-------------|
| Cell alive (live) | Cell alive (populated) |
| Cell consumed (dead) | Cell dead (unpopulated) |
| Transaction changes state | Generation applies rules |
| Hash uniqueness | Seed uniqueness → unique pattern |
| Spore growth | Glider / spaceship motion |

Simple rules produce complex emergent patterns — mirroring CKB's design philosophy where a simple cell model supports a complex on-chain ecosystem.

## Encoding Dimensions

Only one semantic dimension. Everything else is hash-derived identity.

| Visual | Source | Description |
|--------|--------|-------------|
| **Color** (semantic) | `mediaProfile.tier` | Pure CKB = jade green (#2edba3). BTC+CKB = dual-color: gold (#f2c55c) for BTC-origin cells + jade for CKB-origin cells. For non-Spore items (mNFT, DID, Dotbit) that lack `mediaProfile`, always pure CKB. |
| Cell shape | `hash[16] & 0x07` | 8 shapes: circle, square, diamond, triangle, hexagon, cross, star, rounded-square |
| Seed pattern | all hash bytes | Bit-walks all bytes with modular wrap: `byteIdx = floor(bitIndex/8) % bytes.length`. Each bit → one inner cell alive/dead. Effective inner grid = `(gridSize-2)²` cells. |
| Evolution speed | `hash[17]` | Formula: `300 + floor(byte / 255 * 300)` → range 300–600ms |
| Dual-color ratio | hash bytes offset+16 | BTC+CKB mode: color per cell from `bytes[(byteIdx+16) % bytes.length]`. Overlaps with shape/speed bytes intentionally — same hash region serves multiple derivations without conflict since they index different bits/bytes. |

### Dual-Color (BTC+CKB) Behavior

- Initial seed uses one hash region (bytes 0-15) for alive/dead, and a different region (offset by 16 bytes) for color assignment per cell
- Cell values: 0 = dead, 1 = CKB (jade), 2 = BTC (gold)
- Surviving cells keep their original color
- Newborn cells inherit the majority color of their 3 live neighbors (cultural propagation)
- The two colors coexist and interweave organically; ratio is not forced to 50/50

## Rendering

4-layer per-cell rendering (v1 style, simple and clean):

1. **Outer bloom** — shape at 1.8x radius, 8% opacity (soft glow)
2. **Inner bloom** — shape at 1.3x radius, 18% opacity
3. **Cell body** — shape at 1.0x radius, 75% opacity
4. **Core** — shape at 0.45x radius, 90% opacity (bright center)

Dead cells fade as:
- Shape at 0.7x radius, 30% opacity, decreasing by 0.12 per tick

### Cell Shapes (8 variants)

Index from `hash[16] & 0x07`:

| Value | Shape | Visual |
|-------|-------|--------|
| 0 | Circle | Organic, default |
| 1 | Square | Geometric, grid-aligned |
| 2 | Diamond | 45° rotated square |
| 3 | Triangle | Equilateral, point-up |
| 4 | Hexagon | 6-sided, honeycomb |
| 5 | Cross | Plus sign, axis-aligned |
| 6 | Star | 5-point star |
| 7 | Rounded square | Soft corners |

All cells within a single hash visualization use the same shape. The shape is a per-hash property, not per-cell.

## Grid Sizing

| Context | Grid | Canvas | Cell size |
|---------|------|--------|-----------|
| Gallery card (default) | 8×8 | 56px | 7px |
| Detail view / large | 14×14 | 168px | 12px |
| Scalable via `size` prop | proportional | any | computed |

Edge cells (row/col 0 and max) are always dead — they form a 1-cell dead border to simplify neighbor counting. Effective pattern area is `(gridSize-2)²`: 36 cells for 8×8, 144 cells for 14×14.

Grid size selection: caller passes explicit `gridSize` and `size`. No auto-selection. Gallery cards use `gridSize={8} size={56}`, detail views use `gridSize={14} size={168}`.

## Animation

### Evolution

- Standard Conway's Game of Life rules (B3/S23)
- Tick interval: 300–600ms derived from `hash[17]`
- `requestAnimationFrame` loop, throttled to tick interval
- Render runs every frame for smooth opacity transitions; `tick()` only advances generation at interval

### Opacity Transitions

- Birth: opacity jumps +0.5 per tick (fast fade-in)
- Death: opacity decreases by 0.12 per tick (slower fade-out, afterglow)
- Not instant on/off — gives organic breathing feel

### Lifecycle Reset

- If all cells die (population = 0) or generation exceeds 250: reset to original seed with opacity 0.3
- This creates a natural loop: seed → evolve → die/stabilize → reseed
- Deterministic: same hash always produces the same evolution sequence

### Gallery Behavior

- Each visualization runs its own independent animation loop
- Different hashes have different tick intervals → natural desynchronization
- Breathing glow animation phase is offset by `hash[0]` → no synchronized pulsing
- Hover pauses evolution (resume on mouse leave)

## Performance

- 8×8 grid = 64 cells, trivial computation per tick
- 18 simultaneous animations (gallery page) = 18 × 64 = 1,152 cells total
- Canvas 2D context, no WebGL required
- `requestAnimationFrame` with throttling, not `setInterval`
- Retina support: canvas internal resolution = display size × devicePixelRatio
- Tab visibility: RAF is paused by browsers when tab is hidden. On resume, the timestamp-gap check (`now - lastTick >= interval`) fires only one tick, not a burst — this is correct behavior by design.
- Pagination: when gallery page changes, all 18 canvases unmount (RAF cancelled) and new ones mount. Animation starts immediately on mount, no staggering needed — natural desynchronization comes from different hash-derived intervals.
- Hash length: shorter-than-32-byte hashes work correctly — byte indexing wraps via modulo (`% bytes.length`). Longer hashes also work (extra bytes participate in seed via wrap-around).

## Background

- Pure CKB: `#0a0f12` (dark teal tint)
- BTC+CKB: `#0c0b08` (dark warm tint)
- Grid lines: material-colored at 4-5% opacity
- Outer glow: CSS `animation: glow-breathe 4s ease-in-out infinite`, phase offset per hash

## Component Interface

```tsx
interface CellLifeProps {
  hash: string;           // 0x-prefixed hex hash (32 bytes)
  size?: number;           // canvas size in px (default: 56)
  gridSize?: number;       // grid dimension (default: 8)
  isDualChain?: boolean;   // true = BTC+CKB dual-color mode
}
```

The component:
- Parses hash → bytes (handles any length, wraps via modulo)
- Derives shape index, seed, interval from bytes
- Creates canvas, starts animation loop via `requestAnimationFrame`
- Cleans up on unmount (cancels `requestAnimationFrame` via stored ID)
- Respects `prefers-reduced-motion`: renders generation 0 as static image

## Replaces

The existing `CellGlyph` component (concentric arcs SVG in `object-gallery-panel.tsx` lines 24-107). The new component uses canvas instead of SVG due to per-frame animation requirements.

## Graceful Degradation

- Static snapshot (any single frame) is still a meaningful visual fingerprint
- If animation is disabled (e.g., `prefers-reduced-motion`), render generation 0 as a static image
- Small sizes (< 36px) still show recognizable patterns due to the 8×8 grid

## Files to Create/Modify

| File | Action |
|------|--------|
| `frontend/components/object/cell-life.tsx` | New component |
| `frontend/components/object/object-gallery-panel.tsx` | Replace CellGlyph with CellLife |
| `frontend/app/clusters/[clusterId]/client-page.tsx` | Pass isDualChain from mediaProfile |
| `frontend/app/classes/[classId]/client-page.tsx` | Pass isDualChain (always false for mNFT) |
