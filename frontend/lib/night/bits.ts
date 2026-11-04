/**
 * The hash as what it is: 256 bits, laid out as a 16 × 16 grid. The night pours every block's
 * hash in this form (`lib/night/night.ts`); nothing here is derived, it is the hash itself.
 */

export const HASH32 = /^0x[0-9a-fA-F]{64}$/;

/** The 256 bits of a 32-byte hash, most significant first. */
export function bitsOf(hash: string): Uint8Array {
  if (!HASH32.test(hash)) throw new Error(`night: not a 32-byte hash: ${hash}`);
  const out = new Uint8Array(256);
  const hex = hash.slice(2);
  for (let i = 0; i < 64; i++) {
    const nibble = parseInt(hex[i], 16);
    for (let b = 0; b < 4; b++) out[i * 4 + b] = (nibble >> (3 - b)) & 1;
  }
  return out;
}

/** A seed in [0, 1) read from the hash's first eight hex digits: the current of that block. */
export function seedOf(hash: string): number {
  if (!HASH32.test(hash)) throw new Error(`night: not a 32-byte hash: ${hash}`);
  return parseInt(hash.slice(2, 10), 16) / 4294967296;
}

/** Where the grid stands: its centre in canvas uv (x right, y up, both 0–1) and its pitch in CSS px. */
export interface Grid {
  cx: number;
  cy: number;
  pitch: number;
}

/** The uv position of bit `i` (row-major, row 0 at the top) on a W × H canvas. */
export function gridCell(grid: Grid, i: number, width: number, height: number): [number, number] {
  if (!Number.isInteger(i) || i < 0 || i > 255)
    throw new Error(`night: bit index out of range: ${i}`);
  if (!(width > 0) || !(height > 0)) throw new Error(`night: empty canvas ${width}×${height}`);
  const px = grid.pitch / width;
  const py = grid.pitch / height;
  return [grid.cx + ((i % 16) - 7.5) * px, grid.cy + (7.5 - Math.floor(i / 16)) * py];
}
