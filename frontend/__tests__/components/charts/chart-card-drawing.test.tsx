import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { days } from './chart-hero-fixtures';

/**
 * The D1 hand-back, item 3: an index card draws its page's default scale and its cap (rule 6), so
 * a series that spans orders of magnitude, or one spike, does not draw a flat line under a spike on
 * the index while its page draws its shape; the card's axis says which (the plot's name).
 */

vi.mock('@/lib/api', async () => {
  const { CHART_FIXTURES: fixtures } = await import('./chart-hero-fixtures');
  return {
    api: Object.fromEntries(
      Object.keys(fixtures).map((name) => [name, vi.fn(async () => fixtures[name])])
    ),
  };
});
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: {
    title: string;
    yScale?: string;
    plotName?: string;
    axisCap?: { value: number; note: string };
  }) => (
    <div
      role="img"
      aria-label={props.title}
      data-y-scale={props.yScale ?? 'linear'}
      data-plot-name={props.plotName}
      data-cap={props.axisCap?.note ?? ''}
    />
  ),
}));

async function drawn(slug: string) {
  const definition = CHART_BY_SLUG.get(slug)!;
  render(<ChartSmallMultiple definition={definition} />);
  return screen.findByRole('img', { name: definition.title });
}

describe('an index card draws its page’s default scale and cap (item 3)', () => {
  it('Transactions: the page’s log scale, said on the axis', async () => {
    const chart = await drawn('transaction-count');
    expect(chart).toHaveAttribute('data-y-scale', 'log');
    expect(chart).toHaveAttribute('data-plot-name', 'transactions · log scale');
    expect(chart).toHaveAttribute('data-cap', '');
  });

  it('Activities: the page’s log scale, said on the axis', async () => {
    const chart = await drawn('activity-volume');
    expect(chart).toHaveAttribute('data-y-scale', 'log');
    // re-pinned for lane D1's second follow-up, item 2: the series is the day's kind counts
    expect(chart).toHaveAttribute('data-plot-name', 'kind counts · log scale');
  });

  it('Activity by type: linear, its spike named above the scale with the day’s whole', async () => {
    const chart = await drawn('activity-type-breakdown');
    expect(chart).toHaveAttribute('data-y-scale', 'linear');
    expect(chart).toHaveAttribute('data-plot-name', 'activities');
    // day 50 of 120: 1,000,000 script calls and 698 more of the other kinds, the API's figure
    expect(chart).toHaveAttribute(
      'data-cap',
      // re-pinned for lane D1's second follow-up, item 2: the stack's whole is kind counts
      `${days(120)[50]} · 1,000,698 kind counts, above this scale`
    );
  });
});
