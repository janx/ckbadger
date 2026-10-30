import { describe, expect, it } from 'vitest';
import { decodeDaoHeader, formatAccumulatedRate } from '@/lib/dao-header';

describe('decodeDaoHeader', () => {
  it('decodes the exact four little-endian u64 fields without Number precision loss', () => {
    expect(
      decodeDaoHeader(
        '0x28341e52bbf1665b9cb7fbb823dd2a0077f0143eecc3870a00e95a770f4f3407',
        'Block 20827932'
      )
    ).toEqual({
      C: BigInt('6586217291914818600'),
      AR: BigInt('12065094518945692'),
      S: BigInt('758790481656475767'),
      U: BigInt('519126784900000000'),
    });
    const max = BigInt('18446744073709551615');
    expect(decodeDaoHeader(`0x${'ff'.repeat(32)}`, 'max')).toEqual({
      C: max,
      AR: max,
      S: max,
      U: max,
    });
  });
  it('rejects malformed or truncated bytes with block context', () => {
    for (const value of ['0x1234', '0x' + 'zz'.repeat(32), '0x' + '00'.repeat(33)])
      expect(() => decodeDaoHeader(value, 'Block 42')).toThrow(
        'Block 42: DAO header must contain exactly 32 hexadecimal bytes'
      );
  });
});

describe('formatAccumulatedRate', () => {
  it('prints the ×10¹⁶ header integer as its exact 16-decimal rate', () => {
    expect(formatAccumulatedRate(BigInt('10000000000000000'))).toBe('1.0000000000000000');
    expect(formatAccumulatedRate(BigInt('12065094518945692'))).toBe('1.2065094518945692');
    expect(formatAccumulatedRate(BigInt('18446744073709551615'))).toBe('1844.6744073709551615');
    expect(() => formatAccumulatedRate(BigInt(-1))).toThrow('negative');
  });
});
