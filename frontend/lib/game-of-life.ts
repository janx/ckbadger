/**
 * Game of Life engine for CKB hash visualization.
 *
 * Pure-logic, deterministic, side-effect-free.
 * Values: 0 = dead, 1 = CKB (jade), 2 = BTC (gold).
 */

/** Parse a hex hash string (with or without 0x prefix) into a byte array. */
export function hashToBytes(hex: string): number[] {
  const clean = hex.startsWith('0x') ? hex.slice(2) : hex;
  if (clean.length === 0) return [];
  const bytes: number[] = [];
  for (let i = 0; i < clean.length; i += 2) {
    bytes.push(parseInt(clean.slice(i, i + 2), 16));
  }
  return bytes;
}

/**
 * Seed a Game of Life grid from hash bytes.
 *
 * Bit-walks all bytes with modular wrap. Border cells are always dead.
 * In dual mode, color is derived from a separate byte offset.
 *
 * @param size - Grid side length
 * @param bytes - Hash bytes (from hashToBytes)
 * @param isDual - If true, produce both CKB (1) and BTC (2) colors
 * @returns 2D grid of cell values
 */
export function seedGrid(size: number, bytes: number[], isDual: boolean): number[][] {
  const grid: number[][] = Array.from({ length: size }, () => Array(size).fill(0));
  if (bytes.length === 0) return grid;

  // Walk interior cells (skip border row/col on each side)
  const interiorRows = size - 2;
  const interiorCols = size - 2;
  const totalInterior = interiorRows * interiorCols;

  for (let i = 0; i < totalInterior; i++) {
    const bitIndex = i;
    const byteIdx = Math.floor(bitIndex / 8) % bytes.length;
    const bitPos = bitIndex % 8;
    const alive = (bytes[byteIdx] >> (7 - bitPos)) & 1;

    if (alive) {
      const row = 1 + Math.floor(i / interiorCols);
      const col = 1 + (i % interiorCols);

      if (isDual) {
        const colorByteIdx = (byteIdx + 16) % bytes.length;
        const colorBit = (bytes[colorByteIdx] >> (7 - bitPos)) & 1;
        grid[row][col] = colorBit === 0 ? 1 : 2;
      } else {
        grid[row][col] = 1;
      }
    }
  }

  return grid;
}

/**
 * Advance the grid one step using B3/S23 rules.
 *
 * - Surviving cells keep their original color.
 * - Newborns inherit the majority neighbor color (CKB/jade on tie).
 */
export function stepGrid(grid: number[][]): number[][] {
  const rows = grid.length;
  const cols = grid[0].length;
  const next: number[][] = Array.from({ length: rows }, () => Array(cols).fill(0));

  for (let r = 1; r < rows - 1; r++) {
    for (let c = 1; c < cols - 1; c++) {
      let liveNeighbors = 0;
      let jadeCount = 0;
      let goldCount = 0;

      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          if (dr === 0 && dc === 0) continue;
          const nr = r + dr;
          const nc = c + dc;
          if (nr >= 0 && nr < rows && nc >= 0 && nc < cols) {
            const val = grid[nr][nc];
            if (val > 0) {
              liveNeighbors++;
              if (val === 1) jadeCount++;
              else goldCount++;
            }
          }
        }
      }

      const isAlive = grid[r][c] > 0;

      if (isAlive) {
        // Survival: 2 or 3 neighbors
        if (liveNeighbors === 2 || liveNeighbors === 3) {
          next[r][c] = grid[r][c]; // keep original color
        }
        // else: death by underpopulation or overpopulation (stays 0)
      } else {
        // Birth: exactly 3 neighbors
        if (liveNeighbors === 3) {
          // Majority color; CKB (jade) wins ties
          next[r][c] = goldCount > jadeCount ? 2 : 1;
        }
      }
    }
  }

  return next;
}

/**
 * Extract a shape index (0-7) from hash bytes.
 * Uses byte[16], falling back to byte[0] for short hashes.
 */
export function getShapeIndex(bytes: number[]): number {
  const byte = bytes.length > 16 ? bytes[16] : (bytes[0] ?? 0);
  return byte & 0x07;
}

/**
 * Derive animation interval (ms) from hash bytes.
 * Range: 300ms to 600ms. Uses byte[17], falling back to byte[1].
 */
export function getInterval(bytes: number[]): number {
  const byte = bytes.length > 17 ? bytes[17] : (bytes[1] ?? 0);
  return 300 + Math.floor((byte / 255) * 300);
}

// ---------------------------------------------------------------------------
// Shape draw functions
// ---------------------------------------------------------------------------

/**
 * A shape draw function renders a shape onto a canvas 2D context.
 * @param ctx - Canvas rendering context
 * @param cx - Center x coordinate
 * @param cy - Center y coordinate
 * @param r - Radius (half cell size)
 */
export type ShapeDrawFn = (
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  r: number
) => void;

function drawCircle(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fill();
}

function drawSquare(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  ctx.fillRect(cx - r, cy - r, r * 2, r * 2);
}

function drawDiamond(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  ctx.beginPath();
  ctx.moveTo(cx, cy - r);
  ctx.lineTo(cx + r, cy);
  ctx.lineTo(cx, cy + r);
  ctx.lineTo(cx - r, cy);
  ctx.closePath();
  ctx.fill();
}

function drawTriangle(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  ctx.beginPath();
  ctx.moveTo(cx, cy - r);
  ctx.lineTo(cx + r, cy + r);
  ctx.lineTo(cx - r, cy + r);
  ctx.closePath();
  ctx.fill();
}

function drawHexagon(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  ctx.beginPath();
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 3) * i - Math.PI / 2;
    const x = cx + r * Math.cos(angle);
    const y = cy + r * Math.sin(angle);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
  ctx.fill();
}

function drawCross(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  const arm = r / 3;
  ctx.fillRect(cx - arm, cy - r, arm * 2, r * 2);
  ctx.fillRect(cx - r, cy - arm, r * 2, arm * 2);
}

function drawStar(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  const inner = r * 0.4;
  ctx.beginPath();
  for (let i = 0; i < 5; i++) {
    // Outer point
    const outerAngle = (Math.PI * 2 * i) / 5 - Math.PI / 2;
    const ox = cx + r * Math.cos(outerAngle);
    const oy = cy + r * Math.sin(outerAngle);
    if (i === 0) ctx.moveTo(ox, oy);
    else ctx.lineTo(ox, oy);

    // Inner point
    const innerAngle = outerAngle + Math.PI / 5;
    const ix = cx + inner * Math.cos(innerAngle);
    const iy = cy + inner * Math.sin(innerAngle);
    ctx.lineTo(ix, iy);
  }
  ctx.closePath();
  ctx.fill();
}

function drawRoundedSquare(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number): void {
  const cornerRadius = r * 0.3;
  const x = cx - r;
  const y = cy - r;
  const w = r * 2;
  const h = r * 2;

  ctx.beginPath();
  ctx.moveTo(x + cornerRadius, y);
  ctx.lineTo(x + w - cornerRadius, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + cornerRadius);
  ctx.lineTo(x + w, y + h - cornerRadius);
  ctx.quadraticCurveTo(x + w, y + h, x + w - cornerRadius, y + h);
  ctx.lineTo(x + cornerRadius, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - cornerRadius);
  ctx.lineTo(x, y + cornerRadius);
  ctx.quadraticCurveTo(x, y, x + cornerRadius, y);
  ctx.closePath();
  ctx.fill();
}

/** 8 shape renderers indexed by getShapeIndex(). */
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
