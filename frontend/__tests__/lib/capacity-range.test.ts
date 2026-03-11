import { describe, expect, it } from 'vitest';
import { getCapacityRangeParams } from '@/lib/capacity-range';

function diffDays(from: string, to: string): number {
  const fromMs = new Date(`${from}T00:00:00Z`).getTime();
  const toMs = new Date(`${to}T00:00:00Z`).getTime();
  return Math.round((toMs - fromMs) / 86_400_000);
}

describe('capacity range', () => {
  it('returns undefined params for all range', () => {
    expect(getCapacityRangeParams('all')).toBeUndefined();
  });

  it('builds inclusive day windows', () => {
    const d30 = getCapacityRangeParams('30d');
    expect(d30).toBeDefined();
    expect(diffDays(d30!.from, d30!.to)).toBe(29);

    const d90 = getCapacityRangeParams('90d');
    expect(d90).toBeDefined();
    expect(diffDays(d90!.from, d90!.to)).toBe(89);

    const y1 = getCapacityRangeParams('1y');
    expect(y1).toBeDefined();
    expect(diffDays(y1!.from, y1!.to)).toBe(364);
  });
});
