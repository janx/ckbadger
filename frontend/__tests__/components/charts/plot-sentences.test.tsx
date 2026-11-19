import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';
import { SVG_TEXT } from '@/lib/chart-type';
import { fontPx } from '@/lib/type-metrics';

// The ink and size roles, finished (the chart lane's part): a SENTENCE in a plot is prose, the
// reading ink at 13 px (the small step): a plot's name line, an axis cap's note; a target line's
// key is a name and a value, the reading ink at 13 px. A plot's unit alone (an index card's
// name line) is a label, and the ticks, the band labels and the readout's hint keep the chart
// board's quiet 12 px.

const line = (values: number[]) =>
  values.map((v, i) => ({
    label: new Date(Date.UTC(2026, 0, 1 + i)).toISOString().slice(0, 10),
    values: { v: String(v) },
  }));

describe('a sentence in a plot is prose', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => vi.restoreAllMocks());

  it('has the role in lib/chart-type.ts: 13 px, the small step', () => {
    expect(SVG_TEXT.sentence).toBe(fontPx('small'));
    expect(SVG_TEXT.sentence).toBe(13);
  });

  it('writes a plot’s name line and an axis cap’s note in the reading ink at 13 px', () => {
    const values = [50.48, 17.21, ...Array.from({ length: 20 }, (_, i) => (i % 2 ? 4.05 : 3.95))];
    const { container } = render(
      <Chart
        title="Epochs"
        subtitle="x"
        data={line(values)}
        series={[{ key: 'v', label: 'v' }]}
        unit="h"
        height={220}
        formatValue={(v) => v}
        plotName="h · every completed epoch since #0"
        insideYTicks
        bareTicks
        axisCap={{ value: 4.05, note: '2 epochs above this scale; the largest, #0 · 50.48 h' }}
      />
    );
    const name = container.querySelector('[data-role="chart-name"]')!;
    expect(name).toHaveAttribute('font-size', '13');
    expect(name).toHaveAttribute('fill', 'var(--c-text)');
    const note = container.querySelector('[data-role="axis-cap"] text')!;
    expect(note).toHaveAttribute('font-size', '13');
    expect(note).toHaveAttribute('fill', 'var(--c-text)');
    // the ticks inside the plot keep the board's quiet 12 px
    for (const tick of container.querySelectorAll('[data-role="y-tick"]')) {
      expect(tick).toHaveAttribute('font-size', '12');
      expect(tick).toHaveAttribute('fill', 'var(--c-text-dim)');
    }
  });

  it('keys a target line in the reading ink at 13 px: a name and its value', () => {
    const { container } = render(
      <Chart
        title="Uncle rate"
        subtitle="daily"
        data={line([2.1, 2.4, 2.9, 2.6])}
        series={[{ key: 'v', label: 'Rate' }]}
        unit="%"
        referenceLines={[{ value: 2.5, label: 'target 2.50%' }]}
        formatValue={(v) => v}
      />
    );
    const key = container.querySelector('[data-reference-key="target 2.50%"] text')!;
    expect(key).toHaveAttribute('font-size', '13');
    expect(key).toHaveAttribute('fill', 'var(--c-text)');
  });

  it('writes a plot’s unit alone (an index card’s) as a label, quiet at 12 px', () => {
    const { container } = render(
      <Chart
        title="Transactions"
        subtitle="x"
        data={line([1, 2, 3, 4])}
        series={[{ key: 'v', label: 'v' }]}
        unit="transactions"
        height={130}
        hideCaption
        formatValue={(v) => v}
        plotUnit="transactions"
        insideYTicks
        bareTicks
      />
    );
    const name = container.querySelector('[data-role="chart-name"]')!;
    expect(name).toHaveTextContent('transactions');
    expect(name).toHaveAttribute('font-size', '12');
    expect(name).toHaveAttribute('fill', 'var(--c-text-dim)');
  });
});
