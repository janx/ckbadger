import { describe, expect, it } from 'vitest';
import { bitsOf, gridCell, HASH32, seedOf } from '@/lib/night/bits';
import {
  Current,
  FIELD_H,
  FIELD_W,
  fieldData,
  makeNoise,
  prng,
  sampleField,
} from '@/lib/night/field';
import { BATCH, BATCHES, Motes } from '@/lib/night/motes';
import { nightLayout } from '@/hooks/useNight';

// block #20,914,858's hash, which no transaction has
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';

describe('the hash as 256 bits', () => {
  it('reads every bit, most significant first, and nothing else', () => {
    const bits = bitsOf(HASH);
    expect(bits).toHaveLength(256);
    // 0xb = 1011, 0xb, 0xb, 0x6 = 0110
    expect(Array.from(bits.slice(0, 16))).toEqual([1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0]);
    const ones = bits.reduce((n, b) => n + b, 0);
    expect(ones).toBe(BigInt(HASH).toString(2).split('1').length - 1);
    expect(HASH32.test(HASH)).toBe(true);
    expect(() => bitsOf('0x12')).toThrow('not a 32-byte hash');
    expect(() => bitsOf(HASH.slice(0, -1))).toThrow('not a 32-byte hash');
  });

  it('seeds the current from the first eight hex digits', () => {
    expect(seedOf(HASH)).toBeCloseTo(0xbbb655ae / 4294967296, 9);
    expect(seedOf(HASH)).toBeGreaterThanOrEqual(0);
    expect(seedOf(HASH)).toBeLessThan(1);
    expect(() => seedOf('0xzz')).toThrow('not a 32-byte hash');
  });

  it('lays the grid out 16 × 16 around its centre, row 0 at the top', () => {
    const grid = { cx: 0.72, cy: 0.5, pitch: 18 };
    const [x0, y0] = gridCell(grid, 0, 1280, 740);
    expect(x0).toBeCloseTo(0.72 - (7.5 * 18) / 1280, 9);
    expect(y0).toBeCloseTo(0.5 + (7.5 * 18) / 740, 9);
    const [x255, y255] = gridCell(grid, 255, 1280, 740);
    expect(x255).toBeCloseTo(0.72 + (7.5 * 18) / 1280, 9);
    expect(y255).toBeCloseTo(0.5 - (7.5 * 18) / 740, 9);
    const [x17, y17] = gridCell(grid, 17, 1280, 740);
    expect(x17).toBeCloseTo(0.72 - (6.5 * 18) / 1280, 9);
    expect(y17).toBeCloseTo(0.5 + (6.5 * 18) / 740, 9);
    expect(() => gridCell(grid, 256, 1280, 740)).toThrow('out of range');
    expect(() => gridCell(grid, 0, 0, 740)).toThrow('empty canvas');
  });
});

describe('the current', () => {
  it('is deterministic per seed and smooth', () => {
    const a = prng(0.42);
    const b = prng(0.42);
    const xs = Array.from({ length: 5 }, () => a());
    expect(Array.from({ length: 5 }, () => b())).toEqual(xs);
    for (const x of xs) {
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThan(1);
    }
    const noise = makeNoise(prng(0.42));
    for (let i = 0; i < 50; i++) {
      const x = i * 0.37;
      const y = i * 0.11;
      const v = noise(x, y);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
      expect(Math.abs(noise(x + 1e-3, y) - v)).toBeLessThan(0.05);
    }
  });

  it('bakes a bounded field per seed, and samples it wrapping and in [-1, 1]', () => {
    const data = fieldData(0.3, 1280 / 740);
    expect(data).toHaveLength(FIELD_W * FIELD_H * 4);
    expect(data).toEqual(fieldData(0.3, 1280 / 740));
    expect(data).not.toEqual(fieldData(0.31, 1280 / 740));
    for (let i = 3; i < data.length; i += 4) expect(data[i]).toBe(255);
    for (const [x, y] of [
      [0.1, 0.2],
      [0.5, 0.5],
      [0.99, 0.01],
    ]) {
      const [vx, vy] = sampleField(data, x, y);
      expect(Math.abs(vx)).toBeLessThanOrEqual(1);
      expect(Math.abs(vy)).toBeLessThanOrEqual(1);
    }
    expect(sampleField(data, 1.0, 0.3)).toEqual(sampleField(data, 0.0, 0.3));
    expect(sampleField(data, -0.25, 0.3)).toEqual(sampleField(data, 0.75, 0.3));
    expect(() => fieldData(0.3, 0)).toThrow('invalid aspect');
  });

  it('blends the last block’s field into the new one over three seconds', () => {
    const current = new Current(0.3, 1280 / 740);
    expect(current.blend).toBe(1);
    expect(current.dirty).toBe(true);
    current.dirty = false;
    current.retarget(0.8);
    expect(current.blend).toBe(0);
    expect(current.dirty).toBe(true);
    current.advance(1.5, 0);
    expect(current.blend).toBeCloseTo(0.5, 9);
    current.advance(3, 0);
    expect(current.blend).toBe(1);
    // at t = 0 there is no drift and no sway: the sample is the new field itself
    expect(current.sample(0.4, 0.6)).toEqual(sampleField(current.b, 0.4, 0.6));
    current.advance(0, 10);
    const [vx, vy] = current.sample(0.4, 0.6);
    expect(Number.isFinite(vx) && Number.isFinite(vy)).toBe(true);
  });
});

describe('the motes', () => {
  const grid = { cx: 0.72, cy: 0.5, pitch: 18 };

  it('only the 1-bits leave; they hold, then ride the current, and die beyond the frame', () => {
    const motes = new Motes(1.0);
    const bits = bitsOf(HASH);
    motes.spawn(bits, grid, 1280, 740);
    const ones = bits.reduce((n, b) => n + b, 0);
    expect(motes.alive()).toBe(ones);
    const current = new Current(0.3, 1280 / 740);
    const before = Array.from(motes.pos);
    motes.update(0.5, current, 0.003, [0.5, 0.5], false, 1280 / 740);
    expect(Array.from(motes.pos)).toEqual(before);
    for (let i = 0; i < 120; i++)
      motes.update(1 / 60, current, 0.003, [0.5, 0.5], false, 1280 / 740);
    expect(Array.from(motes.pos)).not.toEqual(before);
    expect(motes.alive()).toBeLessThanOrEqual(ones);
    // the current drifts and sways with time (a static field would keep them in its eddies)
    for (let i = 0; i < 3000; i++) {
      current.advance(1 / 60, i / 60);
      motes.update(1 / 60, current, 0.01, [0.5, 0.5], false, 1280 / 740);
    }
    // most have left the frame and died; every survivor is still inside it
    expect(motes.alive()).toBeLessThan(ones / 2);
    for (let i = 0; i < motes.N; i++) {
      if (motes.alp[i] <= 0) continue;
      expect(motes.pos[i * 2]).toBeGreaterThanOrEqual(-0.02);
      expect(motes.pos[i * 2]).toBeLessThanOrEqual(1.02);
      expect(motes.pos[i * 2 + 1]).toBeGreaterThanOrEqual(-0.02);
      expect(motes.pos[i * 2 + 1]).toBeLessThanOrEqual(1.02);
    }
  });

  it('keeps sixteen blocks and reuses the oldest batch', () => {
    const motes = new Motes(1.0);
    const bits = bitsOf(HASH);
    for (let n = 0; n < BATCHES + 1; n++) motes.spawn(bits, grid, 1280, 740);
    expect(motes.N).toBe(BATCHES * BATCH);
    expect(motes.alive()).toBe(BATCHES * bits.reduce((n, b) => n + b, 0));
    expect(() => motes.spawn(new Uint8Array(8), grid, 1280, 740)).toThrow('256 bits');
  });
});

describe('where the grid stands', () => {
  it('under the sign on a phone at half resolution, right of centre on a desk', () => {
    expect(nightLayout(390)).toEqual({ grid: { cx: 0.5, cy: 0.42, pitch: 12 }, half: true });
    expect(nightLayout(800)).toEqual({ grid: { cx: 0.62, cy: 0.48, pitch: 14 }, half: false });
    expect(nightLayout(1280)).toEqual({ grid: { cx: 0.72, cy: 0.5, pitch: 18 }, half: false });
  });
});
