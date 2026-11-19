import { describe, expect, it } from 'vitest';
import { capNote } from '@/components/charts/chart-head';
import type { ChartModel } from '@/components/charts/chart-model';

/**
 * Rule 6 of the chart-hero board (lane D1): what a page writes over an axis its outliers leave —
 * the point above the scale with its exact value, cut for display; a stack's by its whole (the day
 * a spike lifts every kind is above the scale by its total, not by its largest kind).
 */
const model = (kind: ChartModel['kind'], keys: string[]): ChartModel => ({
  title: 'T',
  points: [],
  series: keys.map((key) => ({ key, label: key === 'b' ? 'Daily turnover' : key })),
  unit: kind === 'stacked' ? 'activities' : '%',
  kind,
  latest: null,
  delta: null,
});

describe('capNote (rule 6)', () => {
  // re-pinned for the D1 hand-back, item 2: the whole is the route's, each point's own (the page
  // summed its bands before: 6,225 + 1,103,188)
  it('names a stack above its scale by its whole, the route’s', () => {
    const points = [
      { label: '2024-01-18', values: { a: '6225', b: '1103188' }, whole: '1109413' },
      { label: '2024-01-19', values: { a: '5', b: '400000' }, whole: '400005' },
    ];
    // a count and its noun are held by a no-break space: compared as a space
    const note = capNote(model('stacked', ['a', 'b']), points, [0, 1], (l) => l, 'day');
    expect(note.replace(/\u00a0/g, ' ')).toBe(
      '2 days above this scale; the largest, 2024-01-18 · 1,109,413 activities'
    );
  });

  it('names a line above its scale by its largest series, named where there are two', () => {
    const points = [{ label: '2024-01-18', values: { a: '29.516483', b: '86.810526' } }];
    expect(capNote(model('line', ['a', 'b']), points, [0], (l) => l, 'day')).toBe(
      '2024-01-18 · daily turnover 86.81%, above this scale'
    );
  });
});
