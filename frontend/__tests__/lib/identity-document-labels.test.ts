import { describe, expect, it } from 'vitest';
import { shortenHash } from '@/lib/shorten';

describe('identity record labels', () => {
  // ID-DC2, U-09 (re-pinned): a record's binary value is a hex id in the evidence's records
  // column, written at the row size through the one rule (it was 18…8 by its own helper)
  it('shortens binary record hex instead of dumping whole WebP blobs (ID-DC2)', () => {
    const hex = `0x${'ab'.repeat(40)}`;
    expect(shortenHash(hex, 'row')).toBe(`0x${'ab'.repeat(4)}…${'ab'.repeat(3)}`);
    expect(shortenHash('0xabcdef', 'row')).toBe('0xabcdef');
  });
});
