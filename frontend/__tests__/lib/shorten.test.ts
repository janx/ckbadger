import { describe, expect, it } from 'vitest';
import {
  ELLIPSIS,
  SHORT_FORMS,
  clipForeignLabel,
  clipLabel,
  shortenAddress,
  shortenHash,
  shortenOutpoint,
  type ShortSize,
} from '@/lib/shorten';

/**
 * The one shortening rule (UI review 2026-11-14, U-09, PB-19; readability 2026-11-18, R9): three
 * sizes, two kinds, one ellipsis character. Counts are characters of the value as written, `0x`
 * and `ckb1` included.
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

  // R9 (readability 2026-11-18): an address keeps its network and its identity at every size,
  // `ckb1…` and its tail, 8 characters at the head and row sizes, 6 at tight. The characters after
  // `ckb1` are the lock's code hash, the same for every address of a lock. 13 characters at the
  // head and row sizes; 11 at tight, a tight hash's width.
  it('writes an address at 4…8, 4…8 and 4…6', () => {
    expect(shortenAddress(ADDRESS, 'head')).toBe('ckb1…4c0cgwga');
    expect(shortenAddress(ADDRESS, 'row')).toBe('ckb1…4c0cgwga');
    expect(shortenAddress(ADDRESS, 'tight')).toBe('ckb1…0cgwga');
    expect(shortenAddress(ADDRESS, 'row')).toHaveLength(13);
    expect(shortenAddress(ADDRESS, 'tight')).toHaveLength(shortenHash(HASH, 'tight').length);
  });

  it('spends no character of a shortened address on what every address of a lock shares', () => {
    // two default-lock addresses from the chain: the same first 58 characters (`ckb1`, the
    // format, the lock's code hash and its hash type), different args
    const a =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq0exelczgn2y6k0p9udvjs4fdmj8cghs3shprdnn';
    const b =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq0tpsqq08mkay9ewrfdwlcghv62qw704s93hhsj';
    expect(a.slice(0, 58)).toBe(b.slice(0, 58));
    for (const size of ['head', 'row', 'tight'] as const) {
      const [shortA, shortB] = [shortenAddress(a, size), shortenAddress(b, size)];
      expect(shortA).not.toBe(shortB);
      // the head is the network alone: everything kept after it is the tail, which tells them apart
      expect(shortA.split(ELLIPSIS)[0]).toBe('ckb1');
      expect(shortB.split(ELLIPSIS)[0]).toBe('ckb1');
    }
    expect(shortenAddress(a, 'row')).toBe('ckb1…3shprdnn');
    expect(shortenAddress(b, 'row')).toBe('ckb1…4s93hhsj');
    expect(shortenAddress(b, 'tight')).toBe('ckb1…93hhsj');
  });

  it('is one typed table, each form as long as it says', () => {
    const sizes: ShortSize[] = ['head', 'row', 'tight'];
    expect(SHORT_FORMS).toEqual({
      head: { hash: { head: 12, tail: 8 }, address: { head: 4, tail: 8 } },
      row: { hash: { head: 10, tail: 6 }, address: { head: 4, tail: 8 } },
      tight: { hash: { head: 6, tail: 4 }, address: { head: 4, tail: 6 } },
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
    // tight address: 4 + 1 + 6 = 11
    expect(shortenAddress('ckb1q1abcde', 'tight')).toBe('ckb1q1abcde'); // 11: whole
    expect(shortenAddress('ckb1q12abcde', 'tight')).toBe('ckb1…2abcde'); // 12: cut
    // row (and head) address: 4 + 1 + 8 = 13
    expect(shortenAddress('ckb1q1abcdefg', 'row')).toBe('ckb1q1abcdefg'); // 13: whole
    expect(shortenAddress('ckb1q12abcdefg', 'row')).toBe('ckb1…2abcdefg'); // 14: cut
    expect(shortenAddress('ckb1q12abcdefg', 'head')).toBe('ckb1…2abcdefg'); // 14: cut
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

  // Review fb-logic N4 (lane rd/fb-fixes-2): a cluster's description is the creator's text and was
  // cut by UTF-16 unit, so an emoji across the cut left a lone surrogate, drawn as U+FFFD. It is
  // cut at whole graphemes (what a reader sees as one character), a grapheme one of the budget.
  it('cuts at whole graphemes, never inside an emoji or a letter and its accent', () => {
    const lone = (s: string) =>
      /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(s);
    // 'ab' and a cat (two code units): the cut keeps two graphemes and the ellipsis
    expect(clipLabel('ab🐱cd', 3)).toBe('ab…');
    expect(clipLabel('a🐱bcd', 3)).toBe('a🐱…');
    expect(lone(clipLabel('a🐱bcd', 3))).toBe(false);
    // a family of three (one grapheme of eight code units) is one of the budget, kept or not whole
    const family = '👨‍👩‍👧';
    expect(clipLabel(`${family}${family}x`, 3)).toBe(`${family}${family}x`);
    expect(clipLabel(`${family}${family}xy`, 3)).toBe(`${family}${family}…`);
    // an e and its combining accent are one character
    expect(clipLabel('ééé', 2)).toBe('é…');
  });
});

// Review F-B (logic finding 5): a text that is not ours in a figure is cut to a room in px by the
// upper bound of lib/type-metrics.ts, a grapheme of the served Latin subset 0.6 em and any other
// 1.25 em (at 13 px: 7.8 and 16.25; at 12 px: 7.2 and 15), at whole graphemes.
describe('a text that is not ours, cut to a room at whole graphemes', () => {
  it('cuts a Latin label as clipLabel does at the same number of glyphs', () => {
    for (const [text, chars] of [
      ['Nervape 3x', 8],
      ['Nervape', 8],
      ['NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN', 25],
      ['prev.type', 11],
    ] as const)
      expect(clipForeignLabel(text, 13, chars * 7.8), text).toBe(clipLabel(text, chars));
  });

  it('keeps a CJK text whole where its bound fits, and cuts it to whole characters', () => {
    expect(clipForeignLabel('颜色', 13, 32.5)).toBe('颜色');
    // 25 Latin glyphs' room, 195 px: 11 CJK characters (178.75) and the ellipsis (7.8)
    expect(clipForeignLabel('猫'.repeat(20), 13, 195)).toBe(`${'猫'.repeat(11)}…`);
  });

  it('never splits an emoji, whatever it is composed of', () => {
    const family = '👨‍👩‍👧';
    // 23 Latin glyphs (179.4 px), the family (16.25) and the ellipsis (7.8) need 203.45
    expect(clipForeignLabel(`${'N'.repeat(23)}${family}tail`, 13, 195)).toBe(`${'N'.repeat(23)}…`);
    expect(clipForeignLabel(`${'N'.repeat(23)}${family}tail`, 13, 203.45)).toBe(
      `${'N'.repeat(23)}${family}…`
    );
    // a skin-tone modifier stays with its hand; eight ants in 88 px at 12 px: five and the ellipsis
    expect(clipForeignLabel('👍🏽👍🏽👍🏽', 12, 40)).toBe('👍🏽👍🏽…');
    expect(clipForeignLabel('🐜'.repeat(8), 12, 88)).toBe(`${'🐜'.repeat(5)}…`);
  });

  it('refuses a room that cannot hold the first grapheme and the ellipsis', () => {
    // a CJK character and the ellipsis at 12 px: 22.2 px
    expect(() => clipForeignLabel('猫猫', 12, 22)).toThrow(
      /22 px at 12 px cannot hold "猫" and the ellipsis/
    );
    expect(clipForeignLabel('猫猫猫', 12, 22.2)).toBe('猫…');
  });
});
