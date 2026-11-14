import { describe, expect, it } from 'vitest';
import { niceTicks } from '@/lib/chart-ticks';
import { chartValueAxis, type ChartValueAxisInput } from '@/components/ui/chart';
import { resourceScale } from '@/components/blocks/block-visual';

// Lane A5 follow-up (UI review 2026-11-14, U-19): a count axis has whole-number ticks only — no
// "0.5 transfers", no 2.5-step scale — and the history plots write bare values in their gutter,
// the unit said once in the plot's name. Both are options of the shared axis; off, an axis keeps
// today's ticks, so the chart pages do not change.

const counts = (max: number): ChartValueAxisInput => ({
  title: 'Transfers per week',
  data: [
    { label: '2024-01-01', values: { count: '0' } },
    { label: '2024-01-08', values: { count: String(max) } },
  ],
  series: [{ key: 'count', label: 'Transfers' }],
  unit: 'transfers',
  kind: 'bars',
});

describe('integer ticks on a count axis', () => {
  it.each([1, 2, 3, 5, 7])('ticks a count axis up to %i in whole numbers only', (max) => {
    const ticks = niceTicks(0, max, 4, 0, { integer: true });
    expect(Number.isInteger(ticks.step) && ticks.step >= 1).toBe(true);
    for (const v of ticks.values) expect(Number.isInteger(v), `${v}`).toBe(true);
    expect(ticks.values[0]).toBe(0);
    expect(ticks.values.at(-1)!).toBeGreaterThanOrEqual(max);
  });

  it.each([1, 2, 3, 5])('writes no fractional tick for %i transfers a week', (max) => {
    const { labels } = chartValueAxis({ ...counts(max), integerTicks: true });
    for (const label of labels) expect(label).not.toMatch(/\./);
  });

  it('keeps the measure axes as they are: 0.5 and 2.5 steps without the option', () => {
    expect(niceTicks(0, 1).step).toBe(0.5);
    expect(niceTicks(0, 7).step).toBe(2.5);
    expect(chartValueAxis(counts(1)).labels).toContain('0.5 transfers');
  });

  it('draws a row bar on a whole-number scale for any count (a holder of 7 items)', () => {
    // An AlignedBar refuses a scale that is not a safe integer: 7 drew a 7.5 scale.
    expect(resourceScale([7])).toBe(10);
    for (const max of [1, 2, 3, 5, 6, 7, 8, 13, 70, 7000])
      expect(Number.isSafeInteger(resourceScale([max])), `${max}`).toBe(true);
  });
});

describe('bare ticks, the unit said once in the plot name', () => {
  it('writes the values without the unit when asked, with it by default', () => {
    const big = counts(1500);
    expect(chartValueAxis(big).labels).toContain('1.5k transfers');
    const bare = chartValueAxis({ ...big, bareTicks: true }).labels;
    expect(bare).toContain('1.5k');
    for (const label of bare) expect(label).not.toMatch(/transfers/);
  });
});
