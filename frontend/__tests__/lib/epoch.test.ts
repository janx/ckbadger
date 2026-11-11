import { describe, expect, it } from 'vitest';
import { checkHeaderEpoch, formatEpochPosition, isGenesisEpoch, parseEpoch } from '@/lib/epoch';

describe('parseEpoch', () => {
  it('reads number(index/length)', () => {
    expect(parseEpoch('15168(451/1590)', 20_819_348)).toEqual({
      number: 15168,
      index: 451,
      length: 1590,
    });
    expect(parseEpoch('0(0/1000)', 0)).toEqual({ number: 0, index: 0, length: 1000 });
  });

  it('throws on a malformed string instead of inventing {0, 0, 1800}', () => {
    expect(() => parseEpoch('100/50/1000', 1)).toThrow(
      'epoch "100/50/1000" is not number(index/length)'
    );
    expect(() => parseEpoch('', 1)).toThrow(/not number\(index\/length\)/);
    expect(() => parseEpoch('15168(451/1590) ', 1)).toThrow(/not number/);
  });

  it('throws when the index is not inside a non-empty epoch', () => {
    expect(() => parseEpoch('1(0/0)', 1800)).toThrow(/length 0/);
    expect(() => parseEpoch('1(1590/1590)', 3390)).toThrow(/outside its length/);
  });

  // Regression: with the chain tip at genesis (a fresh local chain) the tip header's epoch is
  // `0(0/0)` — CKB builds the genesis header with the zero epoch — and the strip refused it.
  it("reads genesis's zero epoch at block 0, and nowhere else", () => {
    expect(parseEpoch('0(0/0)', 0)).toEqual({ number: 0, index: 0, length: 0 });
    expect(() => parseEpoch('0(0/0)', 1)).toThrow(
      'block 1: epoch 0(0/0) is the genesis header’s alone'
    );
  });
});

describe('formatEpochPosition', () => {
  it('writes number · index / length with separators and rejects an index outside the epoch', () => {
    expect(formatEpochPosition({ number: 15173, index: 1001, length: 1472 }, 20_827_931)).toBe(
      '15,173 · 1,001 / 1,472'
    );
    expect(() => formatEpochPosition({ number: 1, index: 5, length: 5 }, 1805)).toThrow(
      'epoch index 5 is outside its length 5'
    );
  });

  // Regression: /blocks/0 went to the error page, "epoch index 0 is outside its length 0".
  it('writes what the genesis header states, 0 · 0 / 0, and refuses that form on any later block', () => {
    expect(formatEpochPosition({ number: 0, index: 0, length: 0 }, 0)).toBe('0 · 0 / 0');
    expect(() => formatEpochPosition({ number: 0, index: 0, length: 0 }, 20_000_000)).toThrow(
      'block 20000000: epoch 0(0/0) is the genesis header’s alone'
    );
  });
});

describe('checkHeaderEpoch', () => {
  it('accepts an index inside its epoch, the same epoch back', () => {
    const epoch = { number: 0, index: 1, length: 1743 };
    expect(checkHeaderEpoch(epoch, 1)).toBe(epoch);
  });

  it('accepts the zero epoch only as block 0’s', () => {
    expect(checkHeaderEpoch({ number: 0, index: 0, length: 0 }, 0)).toEqual({
      number: 0,
      index: 0,
      length: 0,
    });
    expect(() => checkHeaderEpoch({ number: 0, index: 0, length: 0 }, 1)).toThrow(/genesis/);
  });

  it('refuses every other index at or past its length, genesis included', () => {
    for (const [epoch, block] of [
      [{ number: 0, index: 1, length: 0 }, 0],
      [{ number: 1, index: 0, length: 0 }, 0],
      [{ number: 0, index: 0, length: 0 }, 1743],
      [{ number: 5, index: 1800, length: 1800 }, 10_800],
      [{ number: 5, index: 1801, length: 1800 }, 10_801],
    ] as const)
      expect(() => checkHeaderEpoch(epoch, block)).toThrow(new RegExp(`^block ${block}: epoch `));
  });

  it('refuses a block or a field that is not a non-negative integer', () => {
    expect(() => checkHeaderEpoch({ number: 0, index: 0, length: 0 }, -1)).toThrow(
      'block -1 is not a non-negative integer'
    );
    expect(() => checkHeaderEpoch({ number: 0, index: 0.5, length: 10 }, 3)).toThrow(
      'block 3: epoch index 0.5 is not a non-negative integer'
    );
  });
});

describe('isGenesisEpoch', () => {
  it('is CKB’s is_genesis: number, index and length all 0', () => {
    expect(isGenesisEpoch({ number: 0, index: 0, length: 0 })).toBe(true);
    expect(isGenesisEpoch({ number: 0, index: 0, length: 1743 })).toBe(false);
    expect(isGenesisEpoch({ number: 0, index: 1, length: 1743 })).toBe(false);
  });
});
