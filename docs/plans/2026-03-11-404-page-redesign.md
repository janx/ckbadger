# 404 Page Redesign

## Goal

Replace the existing 404 page with a new design featuring continuous life simulation GLSL shader (petri dish layout), broken terminal effects, and poetic CKB-native copy.

## Principle Alignment

- CKB Native: "common knowledge" poetic text, cell lifecycle metaphor
- Local First: N/A (frontend-only)
- Agent Friendly: N/A

## Design Decisions

- **Shader**: Continuous life simulation via multi-scale fbm noise (not discrete particles)
- **Density**: Petri dish — finer cells crowding edges, dark void center for text readability
- **Terminal broken**: Screen tear (UV displacement + chromatic aberration), 404 character corruption (JS-driven random glyph swap), whole-screen color flash

## Visual Layers (back to front)

1. **WebGL Canvas** (z-0) — Full-screen GLSL fragment shader
   - 6-octave fbm noise at 3 frequency scales (macro 6x, mid 14x, fine 24x)
   - Petri dish clear zone: `smoothstep(0.15, 0.45, centerDist)`
   - Edge detection for glowing cell membranes (jade/aqua mix)
   - Red-edged death zones at cell boundaries
   - Horizontal screen tear: UV displacement + chromatic aberration, intermittent
   - Film grain

2. **CSS Screen Tear Lines** (z-12) — 3 divs sweeping top-to-bottom, 8s staggered

3. **Screen Flash** (z-13) — Whole-screen jade/rouge flash every ~7s

4. **CRT Effects** (z-10/11) — Existing scanlines + vignette from layout.tsx

5. **Content** (z-20):
   - `404` — 120px JetBrains Mono bold, jade glow, JS character corruption
   - Poetry dim: "some common knowledge has dissolved into the void"
   - Poetry bright: "yet more is crystallizing from the chain"
   - Error line (fixed bottom): `ERR cell_not_found: outpoint unreachable` + blinking cursor

## Files Changed

| File                                                    | Action                                                     |
| ------------------------------------------------------- | ---------------------------------------------------------- |
| `frontend/components/not-found-cell-ocean.tsx`          | Full rewrite — new GLSL shader + screen tear/corruption JS |
| `frontend/components/not-found-page.tsx`                | Update for new component, remove telemetry strip           |
| `frontend/__tests__/components/not-found-page.test.tsx` | Update tests for new copy/structure                        |

## What stays

- `frontend/app/not-found.tsx` entry point
- Header + "Return Home" link
- WebGL fallback gradient
- CRT scanlines/vignette from layout

## What's removed

- Live chain telemetry strip
- Old swimmer-based cell animation
- Debug UI / Ocean Tuning controls
