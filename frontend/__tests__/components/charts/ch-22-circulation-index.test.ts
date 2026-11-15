import { describe, expect, it } from 'vitest';
import {
  CHART_BY_SLUG,
  CHART_SECTIONS,
  CHARTS,
  INDEX_CHARTS,
  chartApiPath,
} from '@/components/charts/chart-catalogue';

// UI review 2026-11-14, CH-22: `/charts/circulation-ratio` had a page and was a related card, but
// no card on the index (`supplemental: true`), so the index listed 28 of the 29 charts.

describe('the charts index lists every chart page (CH-22)', () => {
  it('lists all 29 charts, DAO / circulation in its own group', () => {
    expect(INDEX_CHARTS).toHaveLength(CHARTS.length);
    expect(INDEX_CHARTS).toHaveLength(29);
    const dao = INDEX_CHARTS.filter((chart) => chart.section === 'Nervos DAO & economics');
    expect(dao.map((chart) => chart.slug)).toContain('circulation-ratio');
    expect(dao).toHaveLength(7);
    // Every section the index draws holds the chart it lists.
    expect(
      CHART_SECTIONS.flatMap((section) => INDEX_CHARTS.filter((c) => c.section === section))
    ).toHaveLength(29);
  });

  it("links its raw series in the other DAO charts' form", () => {
    const ratio = CHART_BY_SLUG.get('circulation-ratio')!;
    const deposits = CHART_BY_SLUG.get('total-deposit')!;
    expect(chartApiPath(ratio)).toBe('/dao/charts/circulation-ratio');
    expect(chartApiPath(deposits)).toBe('/dao/charts/total-deposit');
    expect(ratio.evidenceHref).toBe(deposits.evidenceHref);
  });
});
