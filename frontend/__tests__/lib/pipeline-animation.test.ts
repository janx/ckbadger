import { describe, expect, it } from 'vitest';
import { computeUniformShiftDeltaX, computeUniformShiftDistance } from '@/lib/pipeline-animation';

function rect(left: number): { left: number } {
  return { left };
}

describe('computeUniformShiftDistance', () => {
  it('uses the normal card gap when one larger divider gap exists', () => {
    const distance = computeUniformShiftDistance([
      rect(0),
      rect(170),
      rect(340),
      rect(620), // divider outlier
      rect(790),
    ]);

    expect(distance).toBe(170);
  });

  it('falls back to default gap when positions are unusable', () => {
    const distance = computeUniformShiftDistance([rect(10), rect(10.3), rect(10.7)]);
    expect(distance).toBe(168);
  });
});

describe('computeUniformShiftDeltaX', () => {
  it('returns negative delta for left-to-right layout', () => {
    const deltaX = computeUniformShiftDeltaX([rect(20), rect(190), rect(360)]);
    expect(deltaX).toBe(-170);
  });

  it('returns positive delta for right-to-left layout', () => {
    const deltaX = computeUniformShiftDeltaX([rect(360), rect(190), rect(20)]);
    expect(deltaX).toBe(170);
  });
});
