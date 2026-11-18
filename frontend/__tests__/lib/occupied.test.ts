// The one writer of an occupied figure (lane B4 fix round): bytes, grouped, with their unit.
import { describe, expect, it } from 'vitest';
import {
  formatOccupiedDelta,
  occupiedBytesDelta,
  occupiedBytesText,
  occupiedPairText,
} from '@/lib/occupied';

const MINUS = '−';

// Lane B4 second fix round (review-rules-fixes-logic NOTE 4, the coordinator's item 10): a figure
// and its unit never break apart (docs/RESPONSIVE_LAYOUT.md, "Non-breaking units"): the one writer
// joins them with a no-break space, U+00A0 (the pair's unit after its second figure).
describe('lib/occupied keeps a figure with its unit', () => {
  const NBSP = '\u00a0';
  it('joins every figure to its "B" with a no-break space', () => {
    expect(occupiedBytesText(BigInt(61))).toBe(`61${NBSP}B`);
    expect(occupiedPairText(BigInt(563), BigInt(589))).toBe(`563 → 589${NBSP}B`);
    expect(occupiedBytesDelta(BigInt(-61))).toBe(`${MINUS}61${NBSP}B`);
    expect(formatOccupiedDelta('2600000000')).toBe(`+26${NBSP}B`);
  });
});

describe('lib/occupied', () => {
  it('writes a count of bytes with its unit, grouped', () => {
    expect(occupiedBytesText(BigInt(61))).toBe('61 B');
    expect(occupiedBytesText(BigInt(597000))).toBe('597,000 B');
    expect(occupiedBytesText(BigInt(0))).toBe('0 B');
  });

  it('writes a pair with the unit once, after it', () => {
    expect(occupiedPairText(BigInt(563), BigInt(589))).toBe('563 → 589 B');
    expect(occupiedPairText(BigInt(1234), BigInt(1295))).toBe('1,234 → 1,295 B');
  });

  it('writes a change signed, and the shannons the API sends as the same bytes', () => {
    expect(occupiedBytesDelta(BigInt(26))).toBe('+26 B');
    expect(occupiedBytesDelta(BigInt(-61))).toBe(`${MINUS}61 B`);
    expect(occupiedBytesDelta(BigInt(0))).toBe('0 B');
    expect(formatOccupiedDelta('-6100000000')).toBe(occupiedBytesDelta(BigInt(-61)));
    expect(formatOccupiedDelta('123456700000000')).toBe('+1,234,567 B');
  });

  it('refuses a negative count and a change that is not whole bytes, naming them', () => {
    expect(() => occupiedBytesText(BigInt(-1))).toThrow('occupied -1 bytes is negative');
    expect(() => occupiedPairText(BigInt(1), BigInt(-2))).toThrow(
      'occupied after -2 bytes is negative'
    );
    expect(() => formatOccupiedDelta('150000001')).toThrow('not a whole number of bytes');
  });
});
