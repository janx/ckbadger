import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHART_BY_SLUG, INDEX_CHARTS } from '@/components/charts/chart-catalogue';
import { SVG_TEXT } from '@/lib/chart-type';
import { textPx } from '@/lib/type-metrics';

/**
 * A tick's label stands at its own line, and no two labels of a plot stand on one spot (the
 * measurement of fix/readability @ 53d9b56f: the SVG text probe found "2" over "4" in the Nominal
 * APC card and "5" over "10" in the Inflation card, at 1280 and 390, on /charts and as related
 * charts). Each in-plot label was placed against the plot's marks alone, over its line or under it
 * (lane D1, rule 4, b285bbda), so the label under one line and the label over the next stood
 * together where the lines are under 30 px apart, as on a card whose cap's note shortens its plot
 * (the cards drew the page's cap from fe0cb0c8).
 *
 * The cards are drawn at the narrowest card of each row: 278 px at 1280 (four to a row) and 358 px
 * at 390 (one); the two series are the live store's (`/charts/nominal-apc`, `/charts/inflation-rate`
 * of fix/readability @ 53d9b56f, read on a temporary API on the mainnet stores, 2026-11-20).
 */

vi.mock('@/lib/api', async () => {
  const { CHART_FIXTURES: fixtures } = await import('./chart-hero-fixtures');
  const nominal = (await import('@/__tests__/fixtures/chart-api-producer/nominal-apc.json'))
    .default;
  const inflation = (await import('@/__tests__/fixtures/chart-api-producer/inflation-rate.json'))
    .default;
  const methods = Object.fromEntries(
    Object.keys(fixtures).map((name) => [name, vi.fn(async () => fixtures[name])])
  );
  return {
    api: {
      ...methods,
      getNominalApcChart: vi.fn(async () => nominal),
      getInflationRateChart: vi.fn(async () => inflation),
    },
  };
});

/** A tick label's box: its glyphs at the label size, 11 px over its baseline and 3 under. */
function box(label: Element) {
  const x = Number(label.getAttribute('x'));
  const y = Number(label.getAttribute('y'));
  const w = textPx((label.textContent ?? '').length, SVG_TEXT.label);
  const end = label.getAttribute('text-anchor') === 'end';
  return {
    text: label.textContent ?? '',
    x0: end ? x - w : x,
    x1: end ? x : x + w,
    y0: y - 11,
    y1: y + 3,
  };
}

/** Every pair of the plot's tick labels whose boxes share an area, as "a × b". */
function overlaps(svg: Element): string[] {
  const boxes = [...svg.querySelectorAll('[data-role="y-tick"]')].map(box);
  const found: string[] = [];
  for (let i = 0; i < boxes.length; i++)
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i],
        b = boxes[j];
      const w = Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0);
      const h = Math.min(a.y1, b.y1) - Math.max(a.y0, b.y0);
      if (w > 0 && h > 0)
        found.push(`"${a.text}" × "${b.text}" by ${w.toFixed(1)}×${h.toFixed(1)}`);
    }
  return found;
}

async function card(slug: string, width: number) {
  stubChartWidth(width);
  const definition = CHART_BY_SLUG.get(slug)!;
  render(<ChartSmallMultiple definition={definition} />);
  const chart = await screen.findByRole('img', { name: new RegExp(`^${definition.title}`) });
  return chart.closest('svg') ?? chart;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('the in-plot tick labels of a card', () => {
  for (const width of [278, 358])
    for (const slug of ['nominal-apc', 'inflation-rate'])
      it(`${slug} at ${width} px, its live series: no two labels on one spot`, async () => {
        const svg = await card(slug, width);
        expect(svg.querySelectorAll('[data-role="y-tick"]').length).toBeGreaterThan(0);
        expect(overlaps(svg)).toEqual([]);
      });

  // the rule, for every card of the index, so a third card cannot do it silently: no two numbers
  // on one spot, and every plot keeps a number (a plot too short for its scale is refused, and its
  // card would not draw)
  for (const width of [278, 358])
    it(`every card of the catalogue at ${width} px: no two tick labels on one spot`, async () => {
      const found: string[] = [];
      for (const definition of INDEX_CHARTS) {
        if (definition.visual === 'ranked') continue;
        const svg = await card(definition.slug, width);
        for (const overlap of overlaps(svg)) found.push(`${definition.slug}: ${overlap}`);
        if (!svg.querySelector('[data-role="y-tick"]')) found.push(`${definition.slug}: no number`);
        cleanup();
      }
      expect(found).toEqual([]);
    });
});
