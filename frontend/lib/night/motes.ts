/**
 * The motes: only a hash's 1-bits leave. Each block's 256 bits are written at their squares, held
 * for a second, then released into the current; the trails of the last sixteen blocks are kept.
 * Plain arithmetic on typed arrays — `night.ts` hands the positions to the GPU.
 */

import { gridCell, type Grid } from '@/lib/night/bits';
import type { Current } from '@/lib/night/field';

export const BATCHES = 16;
export const BATCH = 256;

export class Motes {
  readonly N = BATCHES * BATCH;
  readonly pos = new Float32Array(this.N * 2);
  /** Hue index: 0 jade for a 1-bit (0-bits never leave: their alpha stays 0). */
  readonly col = new Float32Array(this.N);
  readonly alp = new Float32Array(this.N);
  readonly age = new Float32Array(this.N);
  private batch = 0;

  constructor(readonly holdSeconds: number) {}

  /** Write a block's bits at their grid squares; the oldest batch is reused. */
  spawn(bits: Uint8Array, grid: Grid, width: number, height: number): void {
    if (bits.length !== BATCH)
      throw new Error(`night: a hash has ${BATCH} bits, got ${bits.length}`);
    const k = (this.batch++ % BATCHES) * BATCH;
    for (let i = 0; i < BATCH; i++) {
      const [x, y] = gridCell(grid, i, width, height);
      this.pos[(k + i) * 2] = x;
      this.pos[(k + i) * 2 + 1] = y;
      this.col[k + i] = 0;
      this.alp[k + i] = bits[i] ? 1 : 0;
      this.age[k + i] = 0;
    }
  }

  /** One step: held motes stay; released motes ride the current and scatter from the pointer. */
  update(
    dt: number,
    current: Current,
    speed: number,
    mouse: readonly [number, number],
    pointerMoved: boolean,
    aspect: number
  ): void {
    for (let i = 0; i < this.N; i++) {
      if (this.alp[i] <= 0) continue;
      this.age[i] += dt;
      if (this.age[i] < this.holdSeconds) continue;
      let x = this.pos[i * 2];
      let y = this.pos[i * 2 + 1];
      const [fx, fy] = current.sample(x, y);
      let vx = fx;
      let vy = fy;
      if (pointerMoved) {
        const dx = x - mouse[0];
        const dy = y - mouse[1];
        const dd = dx * dx * aspect * aspect + dy * dy;
        if (dd < 0.02) {
          const k = (0.02 - dd) * 3;
          vx += dx * k * 30;
          vy += dy * k * 30;
        }
      }
      const release = Math.min(1, (this.age[i] - this.holdSeconds) / 1.2);
      x += (vx * speed * release) / aspect;
      y += vy * speed * release;
      if (x < -0.02 || x > 1.02 || y < -0.02 || y > 1.02) this.alp[i] = 0;
      this.pos[i * 2] = x;
      this.pos[i * 2 + 1] = y;
    }
  }

  /** How many motes are alive (for tests and the still frame). */
  alive(): number {
    let n = 0;
    for (let i = 0; i < this.N; i++) if (this.alp[i] > 0) n++;
    return n;
  }
}
