import { describe, expect, it } from 'vitest';
import { u64Hex } from '@/lib/hex-bytes';

describe('u64Hex', () => {
  it('writes an exact u64 as its 8-byte value and its little-endian bytes as stored', () => {
    expect(u64Hex('11529216145580097542', 'since')).toEqual({
      value: '0xa000010000000006',
      littleEndian: '0x06000000000100a0',
    });
  });
  it('rejects a value that is not a u64', () => {
    expect(() => u64Hex('18446744073709551616', 'since')).toThrow(
      'since: 18446744073709551616 is not a u64'
    );
    expect(() => u64Hex('-1', 'since')).toThrow('since: -1 is not a u64');
  });
});
