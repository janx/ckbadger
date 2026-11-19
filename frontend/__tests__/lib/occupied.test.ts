// The one writer of an occupied figure (lane B4 fix round): bytes, grouped, with their unit.
import { describe, expect, it } from 'vitest';
import {
  formatOccupiedDelta,
  occupiedBytesDelta,
  occupiedBytesText,
  occupiedPairText,
} from '@/lib/occupied';

const MINUS = '−';
/** The space a figure's unit follows (B4 second fix round: it was a plain space). */
const NBSP = '\u00a0';

// Lane B4 second fix round (review-rules-fixes-logic NOTE 4, the coordinator's item 10): a figure
// and its unit never break apart (docs/RESPONSIVE_LAYOUT.md, "Non-breaking units"): the one writer
// joins them with a no-break space, U+00A0 (the pair's unit after its second figure).
describe('lib/occupied keeps a figure with its unit', () => {
  it('joins every figure to its "B" with a no-break space', () => {
    expect(occupiedBytesText(BigInt(61), 'tx 0xab')).toBe(`61${NBSP}B`);
    expect(occupiedPairText(BigInt(563), BigInt(589), 'tx 0xab')).toBe(`563 → 589${NBSP}B`);
    expect(occupiedBytesDelta(BigInt(-61))).toBe(`${MINUS}61${NBSP}B`);
    expect(formatOccupiedDelta('2600000000', 'tx 0xab')).toBe(`+26${NBSP}B`);
  });
});

describe('lib/occupied', () => {
  it('writes a count of bytes with its unit, grouped', () => {
    expect(occupiedBytesText(BigInt(61), 'tx 0xab')).toBe(`61${NBSP}B`);
    expect(occupiedBytesText(BigInt(597000), 'tx 0xab')).toBe(`597,000${NBSP}B`);
    expect(occupiedBytesText(BigInt(0), 'tx 0xab')).toBe(`0${NBSP}B`);
  });

  it('writes a pair with the unit once, after it', () => {
    expect(occupiedPairText(BigInt(563), BigInt(589), 'tx 0xab')).toBe(`563 → 589${NBSP}B`);
    expect(occupiedPairText(BigInt(1234), BigInt(1295), 'tx 0xab')).toBe(`1,234 → 1,295${NBSP}B`);
  });

  it('writes a change signed, and the shannons the API sends as the same bytes', () => {
    expect(occupiedBytesDelta(BigInt(26))).toBe(`+26${NBSP}B`);
    expect(occupiedBytesDelta(BigInt(-61))).toBe(`${MINUS}61${NBSP}B`);
    expect(occupiedBytesDelta(BigInt(0))).toBe(`0${NBSP}B`);
    expect(formatOccupiedDelta('-6100000000', 'tx 0xab')).toBe(occupiedBytesDelta(BigInt(-61)));
    expect(formatOccupiedDelta('123456700000000', 'tx 0xab')).toBe(`+1,234,567${NBSP}B`);
  });

  it('refuses a negative count and a change that is not whole bytes, naming them', () => {
    // B4 second fix round: each refusal names where the figure stands
    expect(() => occupiedBytesText(BigInt(-1), 'cell 0xab:0')).toThrow(
      'cell 0xab:0: occupied -1 bytes is negative'
    );
    expect(() => occupiedPairText(BigInt(1), BigInt(-2), 'transaction 0xab')).toThrow(
      'transaction 0xab: occupied after -2 bytes is negative'
    );
    expect(() => formatOccupiedDelta('150000001', 'transaction 0xab')).toThrow(
      'transaction 0xab: occupied change 150000001 shannons is not a whole number of bytes'
    );
  });
});
