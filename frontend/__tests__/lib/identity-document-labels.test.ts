import { describe, expect, it } from 'vitest';
import { truncateRecordHex } from '@/components/asset/identity-document';

describe('identity record labels', () => {
  it('truncates binary record hex instead of dumping whole WebP blobs (ID-DC2)', () => {
    const hex = `0x${'ab'.repeat(40)}`;
    expect(truncateRecordHex(hex)).toBe(`0x${'ab'.repeat(9)}…${'ab'.repeat(4)}`);
    expect(truncateRecordHex('0xabcdef')).toBe('0xabcdef');
  });
});
