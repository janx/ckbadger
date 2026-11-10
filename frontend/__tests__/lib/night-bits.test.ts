import { describe, expect, it } from 'vitest';
import { bitsOf } from '@/lib/night/bits';

// block #20,914,858's hash, which no transaction has
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';

describe('the hash as 256 bits', () => {
  it('reads every bit, most significant first, and nothing else', () => {
    const bits = bitsOf(HASH);
    expect(bits).toHaveLength(256);
    // 0xb = 1011, 0xb, 0xb, 0x6 = 0110
    expect(Array.from(bits.slice(0, 16))).toEqual([1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0]);
    const ones = bits.reduce((n, b) => n + b, 0);
    expect(ones).toBe(BigInt(HASH).toString(2).split('1').length - 1);
    expect(() => bitsOf('0x12')).toThrow('not a 32-byte hash');
    expect(() => bitsOf(HASH.slice(0, -1))).toThrow('not a 32-byte hash');
  });
});
