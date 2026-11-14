import { describe, expect, it } from 'vitest';
import {
  ELLIPSIS,
  SHORT_FORMS,
  clipLabel,
  shortenAddress,
  shortenHash,
  shortenOutpoint,
  type ShortSize,
} from '@/lib/shorten';

/**
 * The one shortening rule (UI review 2026-11-14, U-09, PB-19): three sizes, two kinds, one
 * ellipsis character. Counts are characters of the value as written, `0x` and `ckb1` included.
 */
const HASH = '0x8e4f9c6f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5';
const ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqwgx292hnvmn68xf779vmzrshpmm6epn4c0cgwga';

describe('the three sizes (U-09)', () => {
  it('writes a hash at 12…8, 10…6 and 6…4', () => {
    expect(shortenHash(HASH, 'head')).toBe('0x8e4f9c6f1a…92a3b4c5');
    expect(shortenHash(HASH, 'row')).toBe('0x8e4f9c6f…a3b4c5');
    expect(shortenHash(HASH, 'tight')).toBe('0x8e4f…b4c5');
  });

  it('writes an address at 12…8, 10…6 and 8…6', () => {
    expect(shortenAddress(ADDRESS, 'head')).toBe('ckb1qzda0cr0…4c0cgwga');
    expect(shortenAddress(ADDRESS, 'row')).toBe('ckb1qzda0c…0cgwga');
    expect(shortenAddress(ADDRESS, 'tight')).toBe('ckb1qzda…0cgwga');
  });

  it('is one typed table, each form as long as it says', () => {
    const sizes: ShortSize[] = ['head', 'row', 'tight'];
    expect(SHORT_FORMS).toEqual({
      head: { hash: { head: 12, tail: 8 }, address: { head: 12, tail: 8 } },
      row: { hash: { head: 10, tail: 6 }, address: { head: 10, tail: 6 } },
      tight: { hash: { head: 6, tail: 4 }, address: { head: 8, tail: 6 } },
    });
    for (const size of sizes) {
      expect(shortenHash(HASH, size)).toHaveLength(
        SHORT_FORMS[size].hash.head + 1 + SHORT_FORMS[size].hash.tail
      );
      expect(shortenAddress(ADDRESS, size)).toHaveLength(
        SHORT_FORMS[size].address.head + 1 + SHORT_FORMS[size].address.tail
      );
    }
  });

  it('cuts with the one ellipsis character, never three dots', () => {
    expect(ELLIPSIS).toBe('…');
    for (const size of ['head', 'row', 'tight'] as const) {
      expect(shortenHash(HASH, size)).toContain('…');
      expect(shortenHash(HASH, size)).not.toContain('...');
      expect(shortenAddress(ADDRESS, size)).not.toContain('...');
    }
  });

  it('writes an out point as its hash at that size and its index', () => {
    expect(shortenOutpoint(HASH, 3, 'row')).toBe('0x8e4f9c6f…a3b4c5:3');
    expect(shortenOutpoint(HASH, '12', 'tight')).toBe('0x8e4f…b4c5:12');
    expect(shortenOutpoint(HASH, 0, 'head')).toBe('0x8e4f9c6f1a…92a3b4c5:0');
  });
});

describe('a value no longer than its shortened form is written whole', () => {
  it('keeps a value of exactly head + 1 + tail characters', () => {
    // tight hash: 6 + 1 + 4 = 11 characters
    expect(shortenHash('0x123456789', 'tight')).toBe('0x123456789'); // 11: whole
    expect(shortenHash('0x1234567890', 'tight')).toBe('0x1234…7890'); // 12: cut
    expect(shortenHash('0x', 'row')).toBe('0x');
    // row: 10 + 1 + 6 = 17
    expect(shortenHash('0x1234567890abcde', 'row')).toBe('0x1234567890abcde'); // 17: whole
    expect(shortenHash('0x1234567890abcdef', 'row')).toBe('0x12345678…abcdef'); // 18: cut
    // tight address: 8 + 1 + 6 = 15
    expect(shortenAddress('ckb1qyq1234abcd', 'tight')).toBe('ckb1qyq1234abcd'); // 15: whole
    expect(shortenAddress('ckb1qyq1234abcde', 'tight')).toBe('ckb1qyq1…4abcde'); // 16: cut
  });
});

describe('fails fast on what is not an id', () => {
  it('throws on an empty or non-string value', () => {
    expect(() => shortenHash('', 'row')).toThrow(/empty/);
    expect(() => shortenAddress('', 'head')).toThrow(/empty/);
    expect(() => shortenHash(undefined as unknown as string, 'row')).toThrow(/not a string/);
    expect(() => shortenHash(null as unknown as string, 'tight')).toThrow(/not a string/);
    expect(() => shortenAddress(42 as unknown as string, 'tight')).toThrow(/not a string/);
    expect(() => shortenOutpoint('', 0, 'row')).toThrow(/empty/);
  });

  it('throws on a size that is not one of the three', () => {
    expect(() => shortenHash(HASH, 'wide' as ShortSize)).toThrow(/size/);
  });

  it('throws on an out point index that is not a non-negative integer', () => {
    expect(() => shortenOutpoint(HASH, -1, 'row')).toThrow(/index/);
    expect(() => shortenOutpoint(HASH, '1a', 'row')).toThrow(/index/);
  });
});

describe('a text label cut to a width takes the same ellipsis', () => {
  it('keeps a label that fits and cuts one that does not to the budget, ellipsis included', () => {
    expect(clipLabel('Nervape', 8)).toBe('Nervape');
    expect(clipLabel('Nervape 3x', 8)).toBe('Nervape…');
    expect(clipLabel('Nervape 3x', 8)).toHaveLength(8);
    expect(clipLabel('abc', 3)).toBe('abc');
  });

  it('throws on a budget under two characters', () => {
    expect(() => clipLabel('Nervape', 1)).toThrow(/budget/);
    expect(() => clipLabel('Nervape', 2.5)).toThrow(/budget/);
  });
});
