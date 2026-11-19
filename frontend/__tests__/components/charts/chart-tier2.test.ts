import { describe, expect, it } from 'vitest';
import { CHARTS } from '@/components/charts/chart-catalogue';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';

/**
 * Rule 10 of the chart-hero board (lane D1, CH-15): each chart page's evidence tier says how its
 * figures are computed in one formula line of the fields it reads — its own, not a template with a
 * word swapped — and the five pages whose tier 2 was written by hand and good keep their items.
 */
const KEPT = ['hash-rate', 'difficulty', 'uncle-rate', 'inflation-rate', 'ckb-volume'];

describe('the chart pages’ evidence tier (rule 10)', () => {
  const described = CHARTS.map((chart) => ({
    chart,
    description: getChartDescription(chart.queryKey, {
      yAxisLabel: 'value',
      seriesLabels: ['A', 'B'],
    })!,
  }));

  it('gives each of the 29 charts one formula line of its own, in its fields', () => {
    expect(described).toHaveLength(29);
    for (const { chart, description } of described) {
      expect(description, chart.slug).toBeDefined();
      expect(description.formula, chart.slug).toMatch(/ = /);
      expect(description.formula.length, chart.slug).toBeLessThanOrEqual(260);
    }
    // no template: every chart's line is its own
    expect(new Set(described.map(({ description }) => description.formula)).size).toBe(29);
  });

  it('keeps the hand-written items of the five good pages and drops the series-name lines', () => {
    for (const { chart, description } of described) {
      if (KEPT.includes(chart.slug))
        expect(description.legendItems.length, chart.slug).toBeGreaterThan(0);
      else expect(description.legendItems, chart.slug).toEqual([]);
      // a line that explained a series by its own name ("… "A" component …") is gone
      expect(JSON.stringify(description), chart.slug).not.toMatch(/"A"|"B"/);
    }
  });
});
