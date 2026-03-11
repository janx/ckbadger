# 404 Page Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the 404 page with a continuous life simulation GLSL shader (petri dish layout), broken terminal effects (screen tear, character corruption), and new poetic CKB copy.

**Architecture:** Full rewrite of the WebGL component (`not-found-cell-ocean.tsx`) with a new fbm-based continuous life simulation shader. Simplify the page component (`not-found-page.tsx`) by removing API-driven telemetry and replacing with static terminal-style error line. Add JS-driven 404 character corruption and CSS screen tear/flash overlays.

**Tech Stack:** React 19, WebGL 1.0 (GLSL ES 1.0), Tailwind CSS, Vitest

---

### Task 1: Rewrite the WebGL shader component

**Files:**

- Rewrite: `frontend/components/not-found-cell-ocean.tsx`

**Step 1: Replace the component with new implementation**

Rewrite `frontend/components/not-found-cell-ocean.tsx`. The new component takes no props (remove `cellCount`, `splitPulse`, `haloBloom`, `motionSpeed`). It renders:

- A full-screen WebGL canvas with the new fragment shader
- A CSS fallback gradient when WebGL is unavailable

The new GLSL fragment shader implements:

- `hash2(vec2)` — pseudo-random hash
- `vnoise(vec2)` — value noise with smooth interpolation
- `fbm(vec2)` — 6-octave fractal brownian motion with rotation
- Main function:
  - Horizontal screen tear: UV displacement + chromatic aberration, triggered intermittently via `step(fract(t*0.37), 0.15)`
  - Three noise scales: macro (6x), mid (14x), fine (24x) — each with time-varying thresholds
  - Petri dish clear zone: `field *= smoothstep(0.15, 0.45, length(uv))`
  - Edge detection via gradient for membrane glow (jade/aqua color mix)
  - Red death zones at cell boundary edges
  - Film grain

Uniforms: `u_time` (float), `u_resolution` (vec2). No other uniforms needed.

Keep the existing WebGL boilerplate pattern (compileShader, createProgram, resize handler, animation loop, cleanup). Keep the fallback gradient div. Remove the vignette overlay div (CRT vignette is now handled by layout.tsx). Change attribute name from `a_position` to `a_position` (keep same).

Reference shader code from preview file: `/tmp/claude-1000/preview-term-b.html` (the fragment shader in that file is the target).

**Step 2: Verify it compiles**

Run: `cd frontend && pnpm type-check`
Expected: PASS (no type errors)

**Step 3: Commit**

```bash
git add frontend/components/not-found-cell-ocean.tsx
git commit -m "feat(frontend): rewrite 404 WebGL shader as continuous life simulation"
```

---

### Task 2: Rewrite the 404 page component

**Files:**

- Rewrite: `frontend/components/not-found-page.tsx`

**Step 1: Replace the page component**

Rewrite `frontend/components/not-found-page.tsx`. The new component:

**Removes:**

- All API queries (useQuery for network-stats and blocks)
- `api` import and all format helper functions
- Telemetry strip at bottom
- Old 404 text ("The cells you sought..." / "Elsewhere, unborn cells...")
- `oceanConfig` object and props passed to cell ocean

**Keeps:**

- `Header` import and usage
- `Link` import for "Return Home" button
- `'use client'` directive
- `NotFoundCellOcean` import (no props now)

**Adds:**

- Screen tear overlay: 3 absolutely-positioned divs with staggered CSS animation (8s sweep top-to-bottom)
- Screen flash overlay: single div with intermittent color flash animation (~7s cycle)
- Glitch 404 text: three separate `<span>` elements for each character (`4`, `0`, `4`), each with a `data-char` attribute and a ref. JS `useEffect` runs a glitch cycle that randomly corrupts one character every 2-6s (swap to block glyph in red/blue, snap back after ~120ms)
- Poetry block:
  - dim line: `some common knowledge has dissolved into the void`
  - bright (jade) line: `yet more is crystallizing from the chain`
- Fixed bottom error line: `ERR cell_not_found: outpoint unreachable` with blinking cursor span

CSS classes needed (add to component via Tailwind + inline styles or a small `<style>` block):

- Screen tear line animation (`@keyframes tearScan`)
- Screen flash animation (`@keyframes screenFlash`)
- Blinking cursor (`@keyframes blink`)
- Flickering error line (`@keyframes flicker-line`)

The glitch corruption JS:

```typescript
const CORRUPTIONS = ['\u2588', '\u2592', '\u2593', '\u2591', '\u00d7', '#', '%', '\u2573'];
// In useEffect: setTimeout loop, pick random char index (0-2),
// set textContent to random corruption + red/blue color + small translate,
// after 50ms swap to another corruption,
// after 120-200ms restore original char and reset transform.
// Schedule next glitch in 2000-6000ms.
```

**Step 2: Verify types pass**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 3: Verify lint passes**

Run: `cd frontend && pnpm lint`
Expected: PASS (may need minor fixes)

**Step 4: Commit**

```bash
git add frontend/components/not-found-page.tsx
git commit -m "feat(frontend): redesign 404 page with terminal corruption and poetry"
```

---

### Task 3: Update tests

**Files:**

- Modify: `frontend/__tests__/components/not-found-page.test.tsx`

**Step 1: Rewrite the test file**

The test needs to match the new component structure. Key changes:

- Remove all `api` mocking (no more API calls)
- Remove telemetry assertions (no more `#18,888,888`, hash, hash rate)
- Remove `tip-values-strip` testid assertion
- Keep canvas getContext mock (returns null to trigger fallback)
- Keep header link assertions (DAO, Assets, Scripts, Charts)

New assertions:

- `404` text is present (rendered as three separate spans — test that container has text content "404")
- New poetry: `some common knowledge has dissolved into the void`
- New poetry: `yet more is crystallizing from the chain`
- Error line: `cell_not_found` text is present
- Return Home link exists and points to `/`
- Debug UI still absent (no "Ocean Tuning", "Track Blocks")

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { NotFoundPage } from '@/components/not-found-page';

describe('NotFoundPage', () => {
  beforeEach(() => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders 404 with poetry and terminal error', () => {
    render(<NotFoundPage />);

    // 404 text present
    expect(screen.getByText('4', { exact: true })).toBeInTheDocument();
    expect(screen.getByText('0', { exact: true })).toBeInTheDocument();

    // Poetry lines
    expect(
      screen.getByText('some common knowledge has dissolved into the void')
    ).toBeInTheDocument();
    expect(
      screen.getByText('yet more is crystallizing from the chain')
    ).toBeInTheDocument();

    // Terminal error line
    expect(screen.getByText(/cell_not_found/)).toBeInTheDocument();

    // Return home link
    expect(screen.getByRole('link', { name: /return home/i })).toHaveAttribute('href', '/');

    // Header nav links still present
    expect(screen.getByRole('link', { name: 'DAO' })).toHaveAttribute('href', '/dao');
    expect(screen.getByRole('link', { name: 'Assets' })).toHaveAttribute('href', '/assets');
    expect(screen.getByRole('link', { name: 'Scripts' })).toHaveAttribute('href', '/scripts');
    expect(screen.getByRole('link', { name: 'Charts' })).toHaveAttribute('href', '/charts');

    // No debug UI
    expect(screen.queryByText('Ocean Tuning')).not.toBeInTheDocument();
    expect(screen.queryByText('Track Blocks')).not.toBeInTheDocument();
  });
});
```

**Step 2: Run the test**

Run: `cd frontend && npx vitest run __tests__/components/not-found-page.test.tsx`
Expected: PASS

**Step 3: Run all frontend tests to check for regressions**

Run: `cd frontend && npx vitest run`
Expected: All PASS

**Step 4: Commit**

```bash
git add frontend/__tests__/components/not-found-page.test.tsx
git commit -m "test(frontend): update 404 page tests for new design"
```

---

### Task 4: Format and verify

**Step 1: Run prettier**

Run: `cd frontend && pnpm format`

**Step 2: Run full lint + type check**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 3: Run all tests one final time**

Run: `cd frontend && npx vitest run`
Expected: All PASS

**Step 4: Final commit if formatting changed anything**

```bash
git add -A frontend/
git commit -m "style(frontend): format 404 page files"
```
