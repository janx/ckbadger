import { describe, expect, it } from 'vitest';
import { recordKeyLabel, truncateRecordHex } from '@/components/asset/identity-document';

describe('identity record labels', () => {
  it('falls back to the protocol key when the label is empty (ID-DC1)', () => {
    expect(recordKeyLabel('', 'address.0')).toBe('address.0');
    expect(recordKeyLabel('Bitcoin', 'address.0')).toBe('Bitcoin');
    expect(recordKeyLabel('  ', 'profile.bio')).toBe('profile.bio');
  });

  it('truncates binary record hex instead of dumping whole WebP blobs (ID-DC2)', () => {
    const hex = `0x${'ab'.repeat(40)}`;
    expect(truncateRecordHex(hex)).toBe(`0x${'ab'.repeat(9)}…${'ab'.repeat(4)}`);
    expect(truncateRecordHex('0xabcdef')).toBe('0xabcdef');
  });
});
