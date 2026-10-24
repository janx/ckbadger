import { describe, expect, it } from 'vitest';
import { parseEpoch } from '@/lib/epoch';

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
