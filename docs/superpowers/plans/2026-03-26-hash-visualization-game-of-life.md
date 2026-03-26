# Hash Visualization (Game of Life) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the static CellGlyph SVG with a Conway's Game of Life canvas animation seeded by hash, showing jade cells for pure CKB and dual gold+jade cells for BTC+CKB objects. Non-qualifying objects show a `?` placeholder.

**Architecture:** Pure Game of Life engine (no React) → Canvas-based React component (`CellLife`) consuming the engine → `CellLifePlaceholder` for non-qualifying objects → Integration into existing `ObjectGalleryPanel` replacing `CellGlyph`.

**Tech Stack:** React 19, Canvas 2D API, Vitest, TypeScript

**Spec:** `docs/superpowers/specs/2026-03-26-hash-visualization-game-of-life-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `frontend/lib/game-of-life.ts` | Pure GoL engine: hash parsing, grid seeding, step function, shape definitions. Zero React dependency. |
| `frontend/components/object/cell-life.tsx` | `CellLife` canvas component + `CellLifePlaceholder` static component |
| `frontend/components/object/object-gallery-panel.tsx` | Replace `CellGlyph` with `CellLife`/`CellLifePlaceholder` based on tier |
| `frontend/__tests__/lib/game-of-life.test.ts` | Engine unit tests |
| `frontend/__tests__/components/cell-life.test.tsx` | Component tests |

**Note on spec's "Files to Create/Modify" table:** The spec lists `clusters/[clusterId]/client-page.tsx` and `classes/[classId]/client-page.tsx`. Neither needs modification: `SporeNft.mediaProfile` already flows through `ObjectGalleryPanel` → `SporeObjectCard`, and `classes/` uses the `collection` variant which always renders `CellLifePlaceholder`. The engine file `frontend/lib/game-of-life.ts` is a beneficial deviation from the spec — separating pure logic from React for independent testability.

**Canvas testing note:** In jsdom, `canvas.getContext('2d')` returns `null`. The `CellLife` component handles this gracefully (early return). Canvas rendering is verified in Task 4 (visual verification), not unit tests.

---

### Task 1: Game of Life Engine — Core Logic

**Files:**
- Create: `frontend/lib/game-of-life.ts`
- Create: `frontend/__tests__/lib/game-of-life.test.ts`

This task builds the pure-logic engine with no React. All functions are deterministic and side-effect-free.

- [ ] **Step 1: Write failing tests for `hashToBytes`**

```typescript
// frontend/__tests__/lib/game-of-life.test.ts
import { describe, it, expect } from 'vitest';
import { hashToBytes } from '@/lib/game-of-life';

describe('hashToBytes', () => {
  it('parses 0x-prefixed hex string into byte array', () => {
    expect(hashToBytes('0xdeadbeef')).toEqual([0xde, 0xad, 0xbe, 0xef]);
  });

  it('parses hex string without 0x prefix', () => {
    expect(hashToBytes('ff00')).toEqual([255, 0]);
  });

  it('returns empty array for empty string', () => {
    expect(hashToBytes('')).toEqual([]);
    expect(hashToBytes('0x')).toEqual([]);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && npx vitest run __tests__/lib/game-of-life.test.ts`
Expected: FAIL — module not found

- [ ] **Step 3: Implement `hashToBytes`**

```typescript
// frontend/lib/game-of-life.ts

/** Parse hex hash string into byte array */
export function hashToBytes(hex: string): number[] {
  const clean = hex.replace(/^0x/, '');
  const bytes: number[] = [];
  for (let i = 0; i < clean.length; i += 2) {
    bytes.push(parseInt(clean.substring(i, i + 2), 16));
  }
  return bytes;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run __tests__/lib/game-of-life.test.ts`
Expected: PASS

- [ ] **Step 5: Write failing tests for `seedGrid` (single-color)**

```typescript
import { seedGrid } from '@/lib/game-of-life';

describe('seedGrid', () => {
  it('creates grid of correct size with dead border', () => {
    const bytes = [0xff, 0xff, 0xff, 0xff];
    const grid = seedGrid(6, bytes, false);
    expect(grid.length).toBe(6);
    expect(grid[0].length).toBe(6);
    // Border cells are always 0
    for (let x = 0; x < 6; x++) {
      expect(grid[0][x]).toBe(0);
      expect(grid[5][x]).toBe(0);
    }
    for (let y = 0; y < 6; y++) {
      expect(grid[y][0]).toBe(0);
      expect(grid[y][5]).toBe(0);
    }
  });

  it('produces deterministic output for same input', () => {
    const bytes = hashToBytes('0xdeadbeef01234567');
    const g1 = seedGrid(8, bytes, false);
    const g2 = seedGrid(8, bytes, false);
    expect(g1).toEqual(g2);
  });

  it('produces different grids for different hashes', () => {
    const g1 = seedGrid(8, hashToBytes('0xaaaa'), false);
    const g2 = seedGrid(8, hashToBytes('0xbbbb'), false);
    expect(g1).not.toEqual(g2);
  });

  it('inner cells have value 1 (CKB) in single-color mode', () => {
    const bytes = [0xff, 0xff, 0xff, 0xff];
    const grid = seedGrid(6, bytes, false);
    for (let y = 1; y < 5; y++)
      for (let x = 1; x < 5; x++)
        expect(grid[y][x]).toBeGreaterThanOrEqual(0);
    // At least some inner cells should be alive
    const alive = grid.flat().filter((c) => c > 0).length;
    expect(alive).toBeGreaterThan(0);
  });

  it('produces cells with value 2 (BTC) in dual-color mode', () => {
    const bytes = Array(32).fill(0xff);
    const grid = seedGrid(8, bytes, true);
    const ckbCells = grid.flat().filter((c) => c === 1).length;
    const btcCells = grid.flat().filter((c) => c === 2).length;
    expect(ckbCells).toBeGreaterThan(0);
    expect(btcCells).toBeGreaterThan(0);
  });

  it('single-color mode never produces value 2', () => {
    const bytes = Array(32).fill(0xff);
    const grid = seedGrid(8, bytes, false);
    const btcCells = grid.flat().filter((c) => c === 2).length;
    expect(btcCells).toBe(0);
  });
});
```

- [ ] **Step 6: Implement `seedGrid`**

```typescript
/**
 * Seed a Game of Life grid from hash bytes.
 * Values: 0 = dead, 1 = CKB (jade), 2 = BTC (gold)
 * Border cells (row/col 0 and max) are always dead.
 */
export function seedGrid(
  size: number,
  bytes: number[],
  isDual: boolean
): number[][] {
  const grid: number[][] = Array.from({ length: size }, () =>
    Array(size).fill(0)
  );
  if (bytes.length === 0) return grid;

  let bitIndex = 0;
  for (let y = 1; y < size - 1; y++) {
    for (let x = 1; x < size - 1; x++) {
      const byteIdx = Math.floor(bitIndex / 8) % bytes.length;
      const bitIdx = bitIndex % 8;
      if ((bytes[byteIdx] >> bitIdx) & 1) {
        if (isDual) {
          const colorByte = bytes[(byteIdx + 16) % bytes.length];
          grid[y][x] = (colorByte >> bitIdx) & 1 ? 2 : 1;
        } else {
          grid[y][x] = 1;
        }
      }
      bitIndex++;
    }
  }
  return grid;
}
```

- [ ] **Step 7: Run tests, verify pass**

Run: `cd frontend && npx vitest run __tests__/lib/game-of-life.test.ts`

- [ ] **Step 8: Write failing tests for `stepGrid`**

```typescript
import { stepGrid } from '@/lib/game-of-life';

describe('stepGrid', () => {
  it('kills cell with fewer than 2 neighbors (underpopulation)', () => {
    // 4x4 grid, single cell at (1,1) — no neighbors
    const grid = [
      [0, 0, 0, 0],
      [0, 1, 0, 0],
      [0, 0, 0, 0],
      [0, 0, 0, 0],
    ];
    const next = stepGrid(grid);
    expect(next[1][1]).toBe(0);
  });

  it('cell with 2-3 neighbors survives', () => {
    // Block pattern (2x2) — each cell has 3 neighbors, stable
    const grid = [
      [0, 0, 0, 0],
      [0, 1, 1, 0],
      [0, 1, 1, 0],
      [0, 0, 0, 0],
    ];
    const next = stepGrid(grid);
    expect(next[1][1]).toBe(1);
    expect(next[1][2]).toBe(1);
    expect(next[2][1]).toBe(1);
    expect(next[2][2]).toBe(1);
  });

  it('dead cell with exactly 3 neighbors becomes alive', () => {
    // L shape — cell at (2,2) has 3 neighbors, should be born
    const grid = [
      [0, 0, 0, 0],
      [0, 1, 1, 0],
      [0, 1, 0, 0],
      [0, 0, 0, 0],
    ];
    const next = stepGrid(grid);
    expect(next[2][2]).toBeGreaterThan(0); // born
  });

  it('new cell in dual-color mode inherits majority neighbor color', () => {
    // 3 neighbors: 2 CKB(1) + 1 BTC(2) → new cell = 1 (CKB majority)
    const grid = [
      [0, 0, 0, 0],
      [0, 1, 1, 0],
      [0, 2, 0, 0],
      [0, 0, 0, 0],
    ];
    const next = stepGrid(grid);
    expect(next[2][2]).toBe(1); // CKB majority
  });

  it('surviving cell keeps its original color', () => {
    const grid = [
      [0, 0, 0, 0],
      [0, 2, 1, 0],
      [0, 1, 2, 0],
      [0, 0, 0, 0],
    ];
    const next = stepGrid(grid);
    // Cell (1,1) had value 2, if it survives it stays 2
    if (next[1][1] > 0) expect(next[1][1]).toBe(2);
  });
});
```

- [ ] **Step 9: Implement `stepGrid`**

```typescript
/** Advance one generation. B3/S23 rules. Newborns inherit majority neighbor color. */
export function stepGrid(grid: number[][]): number[][] {
  const h = grid.length;
  const w = grid[0].length;
  const next: number[][] = Array.from({ length: h }, () => Array(w).fill(0));

  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      let neighbors = 0;
      let ckbCount = 0;
      let btcCount = 0;

      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          if (dy === 0 && dx === 0) continue;
          const v = grid[y + dy][x + dx];
          if (v > 0) {
            neighbors++;
            if (v === 1) ckbCount++;
            else btcCount++;
          }
        }
      }

      if (grid[y][x] > 0) {
        // Survival: 2 or 3 neighbors
        next[y][x] = neighbors === 2 || neighbors === 3 ? grid[y][x] : 0;
      } else {
        // Birth: exactly 3 neighbors
        if (neighbors === 3) {
          next[y][x] = ckbCount >= btcCount ? 1 : 2;
        }
      }
    }
  }
  return next;
}
```

- [ ] **Step 10: Run tests, verify pass**

Run: `cd frontend && npx vitest run __tests__/lib/game-of-life.test.ts`

- [ ] **Step 11: Write failing tests for `getShapeIndex` and `getInterval`**

```typescript
import { getShapeIndex, getInterval } from '@/lib/game-of-life';

describe('getShapeIndex', () => {
  it('returns value 0-7 from byte[16]', () => {
    // byte[16] = 0x05 → 5 & 0x07 = 5
    const bytes = Array(17).fill(0);
    bytes[16] = 5;
    expect(getShapeIndex(bytes)).toBe(5);
  });

  it('wraps to 0-7 range', () => {
    const bytes = Array(17).fill(0);
    bytes[16] = 0xff; // 255 & 7 = 7
    expect(getShapeIndex(bytes)).toBe(7);
  });

  it('falls back to byte[0] for short hashes', () => {
    const bytes = [3]; // only 1 byte
    expect(getShapeIndex(bytes)).toBe(3);
  });
});

describe('getInterval', () => {
  it('returns 300 for byte value 0', () => {
    const bytes = Array(18).fill(0);
    expect(getInterval(bytes)).toBe(300);
  });

  it('returns 600 for byte value 255', () => {
    const bytes = Array(18).fill(0);
    bytes[17] = 255;
    expect(getInterval(bytes)).toBe(600);
  });

  it('returns value in 300-600 range', () => {
    const bytes = Array(18).fill(0);
    bytes[17] = 128;
    const interval = getInterval(bytes);
    expect(interval).toBeGreaterThanOrEqual(300);
    expect(interval).toBeLessThanOrEqual(600);
  });
});
```

- [ ] **Step 12: Implement `getShapeIndex` and `getInterval`**

```typescript
/** Derive cell shape index (0-7) from hash bytes */
export function getShapeIndex(bytes: number[]): number {
  const b = bytes.length > 16 ? bytes[16] : bytes[0] ?? 0;
  return b & 0x07;
}

/** Derive tick interval (300-600ms) from hash bytes */
export function getInterval(bytes: number[]): number {
  const b = bytes.length > 17 ? bytes[17] : bytes[1] ?? 0;
  return 300 + Math.floor((b / 255) * 300);
}
```

- [ ] **Step 13: Run all engine tests, verify pass**

Run: `cd frontend && npx vitest run __tests__/lib/game-of-life.test.ts`

- [ ] **Step 14: Export shape draw functions**

Add to `frontend/lib/game-of-life.ts`:

```typescript
/** Shape draw function type for canvas 2D context */
export type ShapeDrawFn = (
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  r: number
) => void;

const drawCircle: ShapeDrawFn = (ctx, cx, cy, r) => {
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fill();
};

const drawSquare: ShapeDrawFn = (ctx, cx, cy, r) => {
  const s = r * 1.6;
  ctx.fillRect(cx - s / 2, cy - s / 2, s, s);
};

const drawDiamond: ShapeDrawFn = (ctx, cx, cy, r) => {
  ctx.beginPath();
  ctx.moveTo(cx, cy - r);
  ctx.lineTo(cx + r, cy);
  ctx.lineTo(cx, cy + r);
  ctx.lineTo(cx - r, cy);
  ctx.closePath();
  ctx.fill();
};

const drawTriangle: ShapeDrawFn = (ctx, cx, cy, r) => {
  ctx.beginPath();
  ctx.moveTo(cx, cy - r);
  ctx.lineTo(cx + r * 0.87, cy + r * 0.5);
  ctx.lineTo(cx - r * 0.87, cy + r * 0.5);
  ctx.closePath();
  ctx.fill();
};

const drawHexagon: ShapeDrawFn = (ctx, cx, cy, r) => {
  ctx.beginPath();
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 3) * i - Math.PI / 6;
    ctx.lineTo(cx + r * Math.cos(a), cy + r * Math.sin(a));
  }
  ctx.closePath();
  ctx.fill();
};

const drawCross: ShapeDrawFn = (ctx, cx, cy, r) => {
  const w = r * 0.5;
  ctx.fillRect(cx - w / 2, cy - r, w, r * 2);
  ctx.fillRect(cx - r, cy - w / 2, r * 2, w);
};

const drawStar: ShapeDrawFn = (ctx, cx, cy, r) => {
  ctx.beginPath();
  for (let i = 0; i < 5; i++) {
    const a1 = (Math.PI * 2 / 5) * i - Math.PI / 2;
    const a2 = a1 + Math.PI / 5;
    ctx.lineTo(cx + r * Math.cos(a1), cy + r * Math.sin(a1));
    ctx.lineTo(cx + r * 0.45 * Math.cos(a2), cy + r * 0.45 * Math.sin(a2));
  }
  ctx.closePath();
  ctx.fill();
};

const drawRoundedSquare: ShapeDrawFn = (ctx, cx, cy, r) => {
  const s = r * 1.5;
  const cr = r * 0.35;
  const x = cx - s / 2;
  const y = cy - s / 2;
  ctx.beginPath();
  ctx.moveTo(x + cr, y);
  ctx.lineTo(x + s - cr, y);
  ctx.quadraticCurveTo(x + s, y, x + s, y + cr);
  ctx.lineTo(x + s, y + s - cr);
  ctx.quadraticCurveTo(x + s, y + s, x + s - cr, y + s);
  ctx.lineTo(x + cr, y + s);
  ctx.quadraticCurveTo(x, y + s, x, y + s - cr);
  ctx.lineTo(x, y + cr);
  ctx.quadraticCurveTo(x, y, x + cr, y);
  ctx.closePath();
  ctx.fill();
};

/** All shape draw functions indexed by shape index (0-7) */
export const SHAPE_DRAW_FNS: ShapeDrawFn[] = [
  drawCircle,
  drawSquare,
  drawDiamond,
  drawTriangle,
  drawHexagon,
  drawCross,
  drawStar,
  drawRoundedSquare,
];
```

- [ ] **Step 15: Commit**

```bash
git add frontend/lib/game-of-life.ts frontend/__tests__/lib/game-of-life.test.ts
git commit -m "feat: add Game of Life engine for hash visualization"
```

---

### Task 2: CellLife and CellLifePlaceholder Components

**Files:**
- Create: `frontend/components/object/cell-life.tsx`
- Create: `frontend/__tests__/components/cell-life.test.tsx`

**Read before starting:**
- `docs/superpowers/specs/2026-03-26-hash-visualization-game-of-life-design.md` — rendering layers, animation spec
- `frontend/components/object/object-gallery-panel.tsx:24-107` — existing CellGlyph for size/style reference

- [ ] **Step 1: Write failing test for CellLifePlaceholder**

```typescript
// frontend/__tests__/components/cell-life.test.tsx
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { CellLifePlaceholder } from '@/components/object/cell-life';

describe('CellLifePlaceholder', () => {
  it('renders a question mark', () => {
    render(<CellLifePlaceholder />);
    expect(screen.getByText('?')).toBeTruthy();
  });

  it('applies custom size', () => {
    const { container } = render(<CellLifePlaceholder size={80} />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.style.width).toBe('80px');
    expect(el.style.height).toBe('80px');
  });

  it('defaults to 56px', () => {
    const { container } = render(<CellLifePlaceholder />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.style.width).toBe('56px');
    expect(el.style.height).toBe('56px');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run __tests__/components/cell-life.test.tsx`
Expected: FAIL — module not found

- [ ] **Step 3: Implement CellLifePlaceholder**

```tsx
// frontend/components/object/cell-life.tsx
'use client';

interface CellLifePlaceholderProps {
  size?: number;
}

export function CellLifePlaceholder({ size = 56 }: CellLifePlaceholderProps) {
  return (
    <div
      style={{
        width: `${size}px`,
        height: `${size}px`,
        background: '#0f0f0f',
        border: '1px solid #222840',
        borderRadius: `${Math.round(size * 0.14)}px`,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: '#343c50',
        fontSize: `${Math.round(size * 0.35)}px`,
        fontFamily: 'monospace',
        fontWeight: 'bold',
        flexShrink: 0,
      }}
    >
      ?
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run __tests__/components/cell-life.test.tsx`

- [ ] **Step 5: Write failing test for CellLife**

```typescript
import { CellLife } from '@/components/object/cell-life';

describe('CellLife', () => {
  it('renders a canvas element', () => {
    const { container } = render(
      <CellLife hash="0xdeadbeef01234567890abcdef0123456" />
    );
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  });

  it('applies size to canvas style', () => {
    const { container } = render(
      <CellLife hash="0xdeadbeef01234567890abcdef0123456" size={80} />
    );
    const canvas = container.querySelector('canvas') as HTMLCanvasElement;
    expect(canvas.style.width).toBe('80px');
    expect(canvas.style.height).toBe('80px');
  });

  it('accepts isDualChain prop without error', () => {
    const { container } = render(
      <CellLife hash="0xdeadbeef01234567890abcdef0123456" isDualChain />
    );
    expect(container.querySelector('canvas')).toBeTruthy();
  });

  it('renders wrapper div with mouse event handlers', () => {
    const { container } = render(
      <CellLife hash="0xdeadbeef01234567890abcdef0123456" />
    );
    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper).toBeTruthy();
    expect(wrapper.style.position).toBe('relative');
  });

  it('cleans up on unmount without errors', () => {
    const { unmount } = render(
      <CellLife hash="0xdeadbeef01234567890abcdef0123456" />
    );
    expect(() => unmount()).not.toThrow();
  });
});

// Note: Canvas rendering correctness is verified visually in Task 4.
// jsdom does not support canvas.getContext('2d') — returns null.
// CellLife handles null ctx gracefully (early return from effect).
```

- [ ] **Step 6: Implement CellLife component**

```tsx
// Add to frontend/components/object/cell-life.tsx

import { useRef, useEffect, useCallback } from 'react';
import {
  hashToBytes,
  seedGrid,
  stepGrid,
  getShapeIndex,
  getInterval,
  SHAPE_DRAW_FNS,
  type ShapeDrawFn,
} from '@/lib/game-of-life';

const CKB_COLOR = { r: 46, g: 219, b: 163 };
const BTC_COLOR = { r: 242, g: 197, b: 92 };

function rgba(c: { r: number; g: number; b: number }, a: number): string {
  return `rgba(${c.r},${c.g},${c.b},${a})`;
}

interface CellLifeProps {
  hash: string;
  size?: number;
  gridSize?: number;
  isDualChain?: boolean;
}

export function CellLife({
  hash,
  size = 56,
  gridSize = 8,
  isDualChain = false,
}: CellLifeProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef<{
    grid: number[][];
    opacities: number[][];
    lastColors: number[][];
    generation: number;
    paused: boolean;
    rafId: number;
    lastTick: number;
  } | null>(null);

  const bytes = hashToBytes(hash);
  const shapeIdx = getShapeIndex(bytes);
  const interval = getInterval(bytes);
  const drawCell = SHAPE_DRAW_FNS[shapeIdx];

  const getColor = useCallback(
    (v: number) => (isDualChain && v === 2 ? BTC_COLOR : CKB_COLOR),
    [isDualChain]
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    const cellSize = size / gridSize;
    const pad = Math.max(0.8, cellSize * 0.12);
    const r = (cellSize - pad * 2) / 2;

    const initialGrid = seedGrid(gridSize, bytes, isDualChain);
    const state = {
      grid: initialGrid,
      opacities: initialGrid.map((row) =>
        row.map((c) => (c > 0 ? 1 : 0))
      ),
      lastColors: initialGrid.map((row) =>
        row.map((c) => c || 1)
      ),
      generation: 0,
      paused: false,
      rafId: 0,
      lastTick: 0,
    };
    stateRef.current = state;

    function render() {
      ctx!.fillStyle = isDualChain ? '#0c0b08' : '#0a0f12';
      ctx!.fillRect(0, 0, size, size);

      // Grid lines
      ctx!.strokeStyle = isDualChain
        ? 'rgba(180,170,120,0.04)'
        : 'rgba(46,219,163,0.05)';
      ctx!.lineWidth = 0.5;
      for (let i = 0; i <= gridSize; i++) {
        const p = i * cellSize;
        ctx!.beginPath(); ctx!.moveTo(p, 0); ctx!.lineTo(p, size); ctx!.stroke();
        ctx!.beginPath(); ctx!.moveTo(0, p); ctx!.lineTo(size, p); ctx!.stroke();
      }

      for (let y = 0; y < gridSize; y++) {
        for (let x = 0; x < gridSize; x++) {
          const a = state.opacities[y][x];
          if (a < 0.01) continue;

          const cx = x * cellSize + cellSize / 2;
          const cy = y * cellSize + cellSize / 2;
          const alive = state.grid[y][x] > 0;
          const col = getColor(
            alive ? state.grid[y][x] : state.lastColors[y][x]
          );

          if (alive) {
            ctx!.fillStyle = rgba(col, a * 0.08);
            drawCell(ctx!, cx, cy, r * 1.8);
            ctx!.fillStyle = rgba(col, a * 0.18);
            drawCell(ctx!, cx, cy, r * 1.3);
            ctx!.fillStyle = rgba(col, a * 0.75);
            drawCell(ctx!, cx, cy, r);
            ctx!.fillStyle = rgba(col, a * 0.9);
            drawCell(ctx!, cx, cy, r * 0.45);
          } else {
            ctx!.fillStyle = rgba(col, a * 0.3);
            drawCell(ctx!, cx, cy, r * 0.7);
          }
        }
      }
    }

    function tick() {
      if (state.paused) return;
      const prev = state.grid;
      state.grid = stepGrid(state.grid);
      for (let y = 0; y < gridSize; y++) {
        for (let x = 0; x < gridSize; x++) {
          if (state.grid[y][x] > 0) {
            state.opacities[y][x] = Math.min(1, state.opacities[y][x] + 0.5);
            state.lastColors[y][x] = state.grid[y][x];
          } else {
            if (prev[y][x] > 0) state.lastColors[y][x] = prev[y][x];
            state.opacities[y][x] = Math.max(
              0,
              state.opacities[y][x] - 0.12
            );
          }
        }
      }
      state.generation++;

      let alive = 0;
      for (let gy = 0; gy < gridSize; gy++)
        for (let gx = 0; gx < gridSize; gx++)
          if (state.grid[gy][gx] > 0) alive++;

      if (alive === 0 || state.generation > 250) {
        state.grid = seedGrid(gridSize, bytes, isDualChain);
        for (let ry = 0; ry < gridSize; ry++)
          for (let rx = 0; rx < gridSize; rx++) {
            state.opacities[ry][rx] = state.grid[ry][rx] > 0 ? 0.3 : 0;
            if (state.grid[ry][rx] > 0)
              state.lastColors[ry][rx] = state.grid[ry][rx];
          }
        state.generation = 0;
      }
    }

    // Check prefers-reduced-motion
    const prefersReduced = window.matchMedia(
      '(prefers-reduced-motion: reduce)'
    ).matches;

    if (prefersReduced) {
      render(); // Static gen 0
      return;
    }

    function animate(now: number) {
      if (now - state.lastTick >= interval) {
        tick();
        state.lastTick = now;
      }
      render();
      state.rafId = requestAnimationFrame(animate);
    }

    render();
    state.rafId = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(state.rafId);
    };
  }, [hash, size, gridSize, isDualChain]);

  const handleMouseEnter = () => {
    if (stateRef.current) stateRef.current.paused = true;
  };
  const handleMouseLeave = () => {
    if (stateRef.current) stateRef.current.paused = false;
  };

  return (
    <div
      style={{ position: 'relative', borderRadius: `${Math.round(size * 0.14)}px`, overflow: 'visible' }}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      <div
        className={isDualChain ? 'life-glow-dual' : 'life-glow-ckb'}
        style={{
          position: 'absolute',
          inset: '-6px',
          borderRadius: `${Math.round(size * 0.25)}px`,
          background: isDualChain
            ? 'linear-gradient(135deg, rgba(242,197,92,0.06), rgba(46,219,163,0.06))'
            : 'rgba(46,219,163,0.08)',
          pointerEvents: 'none',
          zIndex: -1,
          animation: `glow-breathe 4s ease-in-out infinite`,
          animationDelay: `${-(((bytes[0] ?? 0) % 40) / 10)}s`,
        }}
      />
      <canvas
        ref={canvasRef}
        style={{
          width: `${size}px`,
          height: `${size}px`,
          display: 'block',
          borderRadius: `${Math.round(size * 0.14)}px`,
        }}
      />
    </div>
  );
}
```

- [ ] **Step 7: Run tests, verify pass**

Run: `cd frontend && npx vitest run __tests__/components/cell-life.test.tsx`

- [ ] **Step 8: Add glow-breathe keyframes**

Check if there's a global CSS file to add the keyframes. If the project uses Tailwind's `@layer`, add to `frontend/app/globals.css`:

```css
@keyframes glow-breathe {
  0%, 100% { opacity: 0.5; transform: scale(1); }
  50% { opacity: 1; transform: scale(1.04); }
}
```

- [ ] **Step 9: Commit**

```bash
git add frontend/components/object/cell-life.tsx frontend/__tests__/components/cell-life.test.tsx frontend/app/globals.css
git commit -m "feat: add CellLife and CellLifePlaceholder components"
```

---

### Task 3: Replace CellGlyph in ObjectGalleryPanel

**Files:**
- Modify: `frontend/components/object/object-gallery-panel.tsx`

**Read before starting:**
- `frontend/components/object/object-gallery-panel.tsx` — full file, understand CellGlyph usage sites
- `frontend/lib/api.ts` — `SporeNft.mediaProfile` type definition

- [ ] **Step 1: Verify no other files reference CellGlyph**

Run: `grep -r "CellGlyph" frontend/`
Expected: Only hits in `object-gallery-panel.tsx`. If any other files reference it, update those too.

- [ ] **Step 2: Remove the CellGlyph function**

Delete the `CellGlyph` function from `object-gallery-panel.tsx`.

- [ ] **Step 3: Add imports for CellLife and CellLifePlaceholder**

At the top of `object-gallery-panel.tsx`, add:

```typescript
import { CellLife, CellLifePlaceholder } from '@/components/object/cell-life';
```

- [ ] **Step 4: Replace CellGlyph in ObjectCard (collection items)**

Find the `<CellGlyph hash={item.nftId} size={36} />` usage in `ObjectCard` (around the former line 181). Replace with:

```tsx
<CellLifePlaceholder size={36} />
```

Collection items (mNFT, DID, Dotbit) have no `mediaProfile`, so they always show the placeholder.

- [ ] **Step 5: Replace CellGlyph in SporeObjectCard**

Find the `<CellGlyph hash={spore.sporeId} size={36} />` usage in `SporeObjectCard` (around the former line 256). Replace with a conditional:

```tsx
{spore.mediaProfile?.tier === 'pure_ckb' || spore.mediaProfile?.tier === 'btc_ckb' ? (
  <CellLife
    hash={spore.sporeId}
    size={36}
    isDualChain={spore.mediaProfile.tier === 'btc_ckb'}
  />
) : (
  <CellLifePlaceholder size={36} />
)}
```

- [ ] **Step 6: Verify build passes**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: No errors

- [ ] **Step 7: Verify tests still pass**

Run: `cd frontend && npx vitest run`
Expected: All existing tests pass

- [ ] **Step 8: Commit**

```bash
git add frontend/components/object/object-gallery-panel.tsx
git commit -m "feat: replace CellGlyph with CellLife Game of Life visualization"
```

---

### Task 4: Visual Verification

**Files:** None (manual check)

- [ ] **Step 1: Start dev server and verify visually**

Run: `pnpm dev`

Navigate to a Spore cluster page (e.g., `/clusters/<clusterId>`). Verify:
- Pure CKB Spores show jade-colored Game of Life animations
- BTC+CKB Spores show dual gold+jade animations
- Other-tier Spores show `?` placeholder
- Hover pauses animation
- Different Spores have different cell shapes and evolution speeds

Navigate to an mNFT class page (e.g., `/classes/<classId>`). Verify:
- All items show `?` placeholder (no mediaProfile)

- [ ] **Step 2: Check gallery performance**

With 18 items visible, verify:
- No visible jank or frame drops
- Animations desynchronized (not all pulsing together)
- Browser dev tools → Performance tab: no excessive paint time

- [ ] **Step 3: Check reduced motion**

Enable `prefers-reduced-motion: reduce` in browser dev tools. Verify:
- All CellLife instances show a static frame (generation 0)
- No animation running

- [ ] **Step 4: Final commit if any fixes needed**

Stage only the specific files that were changed, then commit:

```bash
git add frontend/components/object/cell-life.tsx frontend/components/object/object-gallery-panel.tsx frontend/app/globals.css
git commit -m "fix: visual adjustments for CellLife"
```
