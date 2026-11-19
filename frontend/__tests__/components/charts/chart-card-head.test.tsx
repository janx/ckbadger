import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHARTS } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { chartHead } from '@/components/charts/chart-head';
import { fontPx, textPx } from '@/lib/type-metrics';

/**
 * Rule 9 of the chart-hero board (lane D1, CH-16, CH-07): the index card has a fixed head of two
 * lines — the name and the figure with its unit; the caption and the date — each written to fit
 * the narrowest card on one line and never cut, so every plot of a row starts on one line; and the
 * card's figure is the page's own (the same head, the same point).
 *
 * The narrowest card: four to a row from 1280 (xl), the content 1232 px wide and 40 px between
 * cards (`gap-x-10`): (1232 − 3 × 40) / 4 = 278 px. Two to a row from 640: (608 − 40) / 2 = 284.
 */
const CARD_PX = (1232 - 3 * 40) / 4;
/** The gap between the two parts of a head line (`gap-x-2.5`). */
const GAP_PX = 10;

vi.mock('@/lib/api', async () => {
  const { CHART_FIXTURES: fixtures } = await import('./chart-hero-fixtures');
  return {
    api: Object.fromEntries(
      Object.keys(fixtures).map((name) => [name, vi.fn(async () => fixtures[name])])
    ),
  };
});
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => <div role="img" aria-label={props.title} />,
}));

describe('the index card’s head (rule 9)', () => {
  for (const definition of CHARTS) {
    it(`${definition.slug}: two lines that fit ${CARD_PX} px, the page's own figure`, async () => {
      render(<ChartSmallMultiple definition={definition} />);
      const readout = await screen.findByRole('group', {
        name: `${definition.title} current readout`,
      });
      const head = readout.closest('[data-role="card-head"]') as HTMLElement;
      const caption = head.querySelector('[data-role="card-caption"]')!.textContent!;
      const date = head.querySelector('[data-role="card-date"]')?.textContent ?? '';
      expect(caption).toBe(definition.cardSubtitle);
      // line 2: the caption and the date, 12 px
      const line2 =
        textPx(caption.length, fontPx('meta')) +
        (date ? GAP_PX + textPx(date.length, fontPx('meta')) : 0);
      expect(line2, `${caption} · ${date}`).toBeLessThanOrEqual(CARD_PX);
      // line 1: the name (14 px), the figure (17 px) and its unit (12 px)
      const unit = readout.querySelector('span')?.textContent ?? '';
      const figure = readout.textContent!.slice(0, readout.textContent!.length - unit.length);
      const line1 =
        textPx(definition.title.length, fontPx('body')) +
        GAP_PX +
        textPx(figure.length, fontPx('section')) +
        textPx(unit.length, fontPx('meta'));
      expect(line1, `${definition.title} ${readout.textContent}`).toBeLessThanOrEqual(CARD_PX);
      // the page's own figure: the same head, the same point
      // the catalogue fetches through the mocked api: the page's payload
      const pageHead = chartHead(definition, chartModel(definition, await definition.fetch()));
      expect(date).toBe(pageHead.at ?? '');
      if (pageHead.figure!.unit !== 'CKB') expect(figure).toBe(pageHead.figure!.display);
      else expect(readout).toHaveAttribute('title', expect.stringMatching(/ CKB$/));
    });
  }
});
