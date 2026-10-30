import { describe, expect, it } from 'vitest';
import { formatEpochPosition, parseEpoch } from '@/lib/epoch';

describe('parseEpoch', () => {
  it('reads number(index/length)', () => {
    expect(parseEpoch('15168(451/1590)')).toEqual({ number: 15168, index: 451, length: 1590 });
    expect(parseEpoch('0(0/1000)')).toEqual({ number: 0, index: 0, length: 1000 });
  });

  it('throws on a malformed string instead of inventing {0, 0, 1800}', () => {
    expect(() => parseEpoch('100/50/1000')).toThrow(
      'epoch "100/50/1000" is not number(index/length)'
    );
    expect(() => parseEpoch('')).toThrow(/not number\(index\/length\)/);
    expect(() => parseEpoch('15168(451/1590) ')).toThrow(/not number/);
  });

  it('throws when the index is not inside a non-empty epoch', () => {
    expect(() => parseEpoch('1(0/0)')).toThrow(/length 0/);
    expect(() => parseEpoch('1(1590/1590)')).toThrow(/outside its length/);
  });
});

describe('formatEpochPosition', () => {
  it('writes number · index / length with separators and rejects an index outside the epoch', () => {
    expect(formatEpochPosition({ number: 15173, index: 1001, length: 1472 })).toBe(
      '15,173 · 1,001 / 1,472'
    );
    expect(() => formatEpochPosition({ number: 1, index: 5, length: 5 })).toThrow(
      'epoch index 5 is outside its length 5'
    );
  });
});
