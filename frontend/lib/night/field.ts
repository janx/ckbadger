/**
 * The current: one field of flow per block, seeded by the block's hash — a curl of value noise
 * baked to a small 128 × 64 texture. The old block's field blends into the new one over three
 * seconds. The ink is advected along it on the GPU and the motes ride it on the CPU; both read
 * the same bytes, so everything in the night moves as one body.
 */

export const FIELD_W = 128;
export const FIELD_H = 64;
const BLEND_SECONDS = 3;

/** mulberry32: a small deterministic generator, so a seed always gives the same current. */
export function prng(seed: number): () => number {
  let a = Math.floor(seed * 1e6) >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Value noise in [0, 1] over a shuffled permutation. */
export function makeNoise(rand: () => number): (x: number, y: number) => number {
  const perm = new Uint8Array(512);
  for (let i = 0; i < 256; i++) perm[i] = i;
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    const t = perm[i];
    perm[i] = perm[j];
    perm[j] = t;
  }
  for (let i = 0; i < 256; i++) perm[256 + i] = perm[i];
  const grid = new Float32Array(512);
  for (let i = 0; i < 512; i++) grid[i] = perm[i] / 255;
  return (x, y) => {
    const xi = Math.floor(x);
    const yi = Math.floor(y);
    let fx = x - xi;
    let fy = y - yi;
    fx = fx * fx * (3 - 2 * fx);
    fy = fy * fy * (3 - 2 * fy);
    const X = xi & 255;
    const Y = yi & 255;
    const a = grid[perm[X] + Y];
    const b = grid[perm[X + 1] + Y];
    const c = grid[perm[X] + Y + 1];
    const d = grid[perm[X + 1] + Y + 1];
    const top = a + (b - a) * fx;
    return top + (c + (d - c) * fx - top) * fy;
  };
}

/**
 * The field for one seed on a canvas of the given aspect: RGBA8, the velocity in the first two
 * channels mapped from [-1, 1] to 0–255. Deterministic per (seed, aspect).
 */
export function fieldData(seed: number, aspect: number): Uint8Array {
  if (!(aspect > 0)) throw new Error(`night: invalid aspect ${aspect}`);
  const noise = makeNoise(prng(seed));
  const data = new Uint8Array(FIELD_W * FIELD_H * 4);
  const amp = [0.6, 0.28, 0.12];
  const fbm = (x: number, y: number) => {
    let sum = 0;
    for (let k = 0; k < 3; k++) {
      sum += amp[k] * noise(x, y);
      x *= 2.1;
      y *= 2.1;
    }
    return sum;
  };
  const e = 0.01;
  for (let j = 0; j < FIELD_H; j++) {
    for (let i = 0; i < FIELD_W; i++) {
      const px = ((i + 0.5) / FIELD_W) * aspect * 1.5 + 7.1;
      const py = ((j + 0.5) / FIELD_H) * 1.5 + 3.3;
      const vx = Math.tanh((fbm(px, py + e) - fbm(px, py - e)) / (2 * e) / 2.2);
      const vy = Math.tanh(-(fbm(px + e, py) - fbm(px - e, py)) / (2 * e) / 2.2);
      const o = (j * FIELD_W + i) * 4;
      data[o] = Math.round((vx * 0.5 + 0.5) * 255);
      data[o + 1] = Math.round((vy * 0.5 + 0.5) * 255);
      data[o + 2] = 0;
      data[o + 3] = 255;
    }
  }
  return data;
}

/** Bilinear, wrapping sample of a field at uv (x, y); the velocity in [-1, 1]. */
export function sampleField(data: Uint8Array, x: number, y: number): [number, number] {
  x = ((x % 1) + 1) % 1;
  y = ((y % 1) + 1) % 1;
  const gx = x * FIELD_W - 0.5;
  const gy = y * FIELD_H - 0.5;
  const x0 = Math.floor(gx);
  const y0 = Math.floor(gy);
  const fx = gx - x0;
  const fy = gy - y0;
  const at = (i: number, j: number, k: number) =>
    (data[
      ((((j % FIELD_H) + FIELD_H) % FIELD_H) * FIELD_W + (((i % FIELD_W) + FIELD_W) % FIELD_W)) *
        4 +
        k
    ] /
      255) *
      2 -
    1;
  const lerp = (k: number) =>
    (at(x0, y0, k) * (1 - fx) + at(x0 + 1, y0, k) * fx) * (1 - fy) +
    (at(x0, y0 + 1, k) * (1 - fx) + at(x0 + 1, y0 + 1, k) * fx) * fy;
  return [lerp(0), lerp(1)];
}

/** The same drift and sway the shader applies, so the motes and the ink agree. */
export function driftOf(t: number): [number, number] {
  return [t * 0.0035, -t * 0.0015];
}
export function swayOf(t: number): number {
  return Math.sin(t * 0.11) * 0.3;
}

/** Two fields blended — the last block's and this one's — drifting and swaying with time. */
export class Current {
  a: Uint8Array;
  b: Uint8Array;
  blend = 1;
  t = 0;
  /** Set when a or b changed and the GPU copies must be re-uploaded. */
  dirty = true;

  constructor(
    seed: number,
    readonly aspect: number
  ) {
    this.a = fieldData(seed, aspect);
    this.b = fieldData(seed + 0.37, aspect);
  }

  /** A new block: what is blended now becomes the old field; the new seed is the target. */
  retarget(seed: number): void {
    const cur = new Uint8Array(this.a.length);
    for (let i = 0; i < cur.length; i++) cur[i] = this.a[i] + (this.b[i] - this.a[i]) * this.blend;
    this.a = cur;
    this.b = fieldData(seed, this.aspect);
    this.blend = 0;
    this.dirty = true;
  }

  advance(dt: number, t: number): void {
    this.blend = Math.min(1, this.blend + dt / BLEND_SECONDS);
    this.t = t;
  }

  /** The velocity at uv (x, y) in [-1, 1] units, as the shader computes it. */
  sample(x: number, y: number): [number, number] {
    const [dx, dy] = driftOf(this.t);
    const qa = sampleField(this.a, x + dx, y + dy);
    const qb = sampleField(this.b, x + dx, y + dy);
    const vx = qa[0] + (qb[0] - qa[0]) * this.blend;
    const vy = qa[1] + (qb[1] - qa[1]) * this.blend;
    const s = swayOf(this.t);
    const c = Math.cos(s);
    const n = Math.sin(s);
    return [vx * c - vy * n, vx * n + vy * c];
  }
}
