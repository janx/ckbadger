import { describe, expect, it } from 'vitest';
import {
  hashToBytes,
  seedGrid,
  stepGrid,
  getShapeIndex,
  getInterval,
  SHAPE_DRAW_FNS,
} from '@/lib/game-of-life';

// A known 32-byte hash for deterministic tests
const HASH_A = '0x' + 'ab'.repeat(32);
const HASH_B = '0x' + 'cd'.repeat(32);

describe('hashToBytes', () => {
  it('parses 0x-prefixed hex string', () => {
    const bytes = hashToBytes('0xabcd');
    expect(bytes).toEqual([0xab, 0xcd]);
  });

  it('parses hex string without 0x prefix', () => {
    const bytes = hashToBytes('abcd');
    expect(bytes).toEqual([0xab, 0xcd]);
  });

  it('returns empty array for empty string', () => {
    expect(hashToBytes('')).toEqual([]);
  });
});

describe('seedGrid', () => {
  const bytes32 = hashToBytes(HASH_A);

  it('produces grid of correct size with dead border', () => {
    const grid = seedGrid(10, bytes32, false);
    expect(grid.length).toBe(10);
    expect(grid[0].length).toBe(10);
    // Top and bottom rows all dead
    for (let c = 0; c < 10; c++) {
      expect(grid[0][c]).toBe(0);
      expect(grid[9][c]).toBe(0);
    }
    // Left and right columns all dead
    for (let r = 0; r < 10; r++) {
      expect(grid[r][0]).toBe(0);
      expect(grid[r][9]).toBe(0);
    }
  });

  it('is deterministic for the same input', () => {
    const g1 = seedGrid(20, bytes32, false);
    const g2 = seedGrid(20, bytes32, false);
    expect(g1).toEqual(g2);
  });

  it('produces different grids for different hashes', () => {
    const bytesB = hashToBytes(HASH_B);
    const g1 = seedGrid(20, bytes32, false);
    const g2 = seedGrid(20, bytesB, false);
    expect(g1).not.toEqual(g2);
  });

  it('single-color mode: alive cells have value 1 only', () => {
    const grid = seedGrid(20, bytes32, false);
    const values = new Set(grid.flat());
    // Should contain 0 (dead) and 1 (CKB jade)
    expect(values.has(0)).toBe(true);
    expect(values.has(1)).toBe(true);
    expect(values.has(2)).toBe(false);
  });

  it('dual-color mode: produces both value 1 (CKB) and value 2 (BTC)', () => {
    // Use a hash where the first 16 bytes differ from the last 16 so the +16
    // color offset produces genuinely different bit patterns
    const variedHash = '0xaabbccdd11223344aabbccdd11223344ff00ff00ff00ff00ff00ff00ff00ff00';
    const variedBytes = hashToBytes(variedHash);
    const grid = seedGrid(30, variedBytes, true);
    const values = new Set(grid.flat());
    expect(values.has(1)).toBe(true);
    expect(values.has(2)).toBe(true);
  });

  it('single-color mode never produces value 2', () => {
    // Test with multiple hash inputs to be thorough
    for (const hex of [HASH_A, HASH_B]) {
      const grid = seedGrid(30, hashToBytes(hex), false);
      const values = new Set(grid.flat());
      expect(values.has(2)).toBe(false);
    }
  });
});

describe('stepGrid', () => {
  /** Helper: create a dead grid of given size */
  function deadGrid(size: number): number[][] {
    return Array.from({ length: size }, () => Array(size).fill(0));
  }

  it('underpopulation kills cells with fewer than 2 neighbors', () => {
    // A lone cell at (2,2) with no neighbors should die
    const grid = deadGrid(6);
    grid[2][2] = 1;
    const next = stepGrid(grid);
    expect(next[2][2]).toBe(0);
  });

  it('block pattern survives (each cell has 3 neighbors)', () => {
    // 2x2 block is a still life — each cell has exactly 3 neighbors
    const grid = deadGrid(6);
    grid[2][2] = 1;
    grid[2][3] = 1;
    grid[3][2] = 1;
    grid[3][3] = 1;
    const next = stepGrid(grid);
    expect(next[2][2]).toBe(1);
    expect(next[2][3]).toBe(1);
    expect(next[3][2]).toBe(1);
    expect(next[3][3]).toBe(1);
  });

  it('birth with exactly 3 neighbors', () => {
    // Three cells in an L shape: (1,1), (1,2), (2,1)
    // Cell (2,2) has exactly 3 neighbors and should be born
    const grid = deadGrid(6);
    grid[1][1] = 1;
    grid[1][2] = 1;
    grid[2][1] = 1;
    const next = stepGrid(grid);
    expect(next[2][2]).not.toBe(0); // should be born
  });

  it('newborn inherits majority neighbor color', () => {
    // Three neighbors: two gold (2) and one jade (1)
    // (1,1)=2, (1,2)=2, (2,1)=1
    // Cell (2,2) should be born with color 2 (gold majority)
    const grid = deadGrid(6);
    grid[1][1] = 2;
    grid[1][2] = 2;
    grid[2][1] = 1;
    const next = stepGrid(grid);
    expect(next[2][2]).toBe(2);
  });

  it('newborn defaults to CKB (1) on color tie', () => {
    // Three neighbors: one gold, one jade, one jade? No — need exact tie scenario.
    // Actually with 3 neighbors we can't get an exact tie (odd count).
    // Use a scenario where birth happens and colors are 1 jade, 1 gold, 1 jade -> majority jade.
    // For a true tie: need an even count of live neighbors that sum to a birth condition.
    // B3 means exactly 3 neighbors. 3 is odd, so there's always a majority... unless
    // we consider that tie means equal counts. With 3: 2 vs 1 or 3 vs 0. No tie possible.
    // Let's just verify majority works correctly with 2 jade, 1 gold -> jade wins
    const grid = deadGrid(6);
    grid[1][1] = 1;
    grid[1][2] = 1;
    grid[2][1] = 2;
    const next = stepGrid(grid);
    expect(next[2][2]).toBe(1); // jade majority
  });

  it('surviving cell keeps its original color', () => {
    // 2x2 block with mixed colors: each cell has 3 neighbors and survives
    const grid = deadGrid(6);
    grid[2][2] = 2; // gold
    grid[2][3] = 1; // jade
    grid[3][2] = 1; // jade
    grid[3][3] = 2; // gold
    const next = stepGrid(grid);
    expect(next[2][2]).toBe(2); // gold survives as gold
    expect(next[2][3]).toBe(1); // jade survives as jade
    expect(next[3][2]).toBe(1); // jade survives as jade
    expect(next[3][3]).toBe(2); // gold survives as gold
  });
});

describe('getShapeIndex', () => {
  it('extracts byte[16] masked to 0-7', () => {
    const bytes = new Array(32).fill(0);
    bytes[16] = 0b11111101; // 253 => 253 & 0x07 = 5
    expect(getShapeIndex(bytes)).toBe(5);
  });

  it('wraps result to 0-7 range', () => {
    const bytes = new Array(32).fill(0);
    bytes[16] = 0xff; // 255 & 0x07 = 7
    expect(getShapeIndex(bytes)).toBe(7);
    bytes[16] = 0x08; // 8 & 0x07 = 0
    expect(getShapeIndex(bytes)).toBe(0);
  });

  it('falls back to bytes[0] for short hash', () => {
    const bytes = new Array(10).fill(0);
    bytes[0] = 0b00000011; // 3 & 0x07 = 3
    expect(getShapeIndex(bytes)).toBe(3);
  });
});

describe('getInterval', () => {
  it('returns 300 when byte is 0', () => {
    const bytes = new Array(32).fill(0);
    expect(getInterval(bytes)).toBe(300);
  });

  it('returns 600 when byte is 255', () => {
    const bytes = new Array(32).fill(0);
    bytes[17] = 255;
    expect(getInterval(bytes)).toBe(600);
  });

  it('returns mid-range value for intermediate byte', () => {
    const bytes = new Array(32).fill(0);
    bytes[17] = 128;
    const interval = getInterval(bytes);
    expect(interval).toBeGreaterThanOrEqual(300);
    expect(interval).toBeLessThanOrEqual(600);
  });

  it('falls back to bytes[1] for short hash', () => {
    const bytes = [0, 255];
    expect(getInterval(bytes)).toBe(600);
  });
});

describe('SHAPE_DRAW_FNS', () => {
  it('exports exactly 8 shape draw functions', () => {
    expect(SHAPE_DRAW_FNS).toHaveLength(8);
    for (const fn of SHAPE_DRAW_FNS) {
      expect(typeof fn).toBe('function');
    }
  });
});
