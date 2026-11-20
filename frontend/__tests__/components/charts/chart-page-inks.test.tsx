import { describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '../../utils/test-utils';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { ChartDetail } from '@/components/charts/chart-detail';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';

// The ink and size roles, finished (the chart lane's part, the chart pages' HTML): a value, a
// link's words and a sentence read in the reading ink at 13 px; a label, a unit and a hint keep the
// quiet 12 px. A key's band name and its figure are a name and a value. An index card's head line
// 2 keeps the chart-hero board's 12 px (rule 9: two lines that fit the narrowest card, 278 px),
// its date a value in the reading ink.

vi.mock('@/lib/api', async () => {
  const { CHART_FIXTURES: fixtures } = await import('./chart-hero-fixtures');
  const methods = Object.fromEntries(
    Object.keys(fixtures).map((name) => [name, vi.fn(async () => fixtures[name])])
  );
  return {
    api: {
      ...methods,
      getNetworkStats: vi.fn(async () => ({ syncStatus: { chartDataMayBeIncomplete: false } })),
      getHardforks: vi.fn(async () => ({ events: [] })),
    },
  };
});
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => <div role="img" aria-label={props.title} />,
}));

const definition = (slug: string) => {
  const found = CHART_BY_SLUG.get(slug);
  if (!found) throw new Error(`no chart ${slug}`);
  return found;
};

describe('the index card', () => {
  it('writes its key’s band names and figures in the reading ink at 13 px', async () => {
    render(<ChartSmallMultiple definition={definition('total-supply')} />);
    const key = await screen.findByRole('list', { name: 'Supply legend' });
    expect(key).toHaveClass('text-small');
    expect(key).not.toHaveClass('text-meta');
    expect(key).not.toHaveClass('text-text-dim');
    for (const item of within(key).getAllByRole('listitem'))
      for (const span of item.querySelectorAll('span:not([aria-hidden])'))
        expect(span).toHaveClass('text-text');
  });

  it('writes its date in the reading ink, its caption a label, both at the board’s 12 px', async () => {
    render(<ChartSmallMultiple definition={definition('total-supply')} />);
    await screen.findByRole('group', { name: 'Supply current readout' });
    const date = document.querySelector('[data-role="card-date"]')!;
    expect(date).toHaveClass('text-text');
    expect(date.closest('div')).toHaveClass('text-meta');
    const caption = document.querySelector('[data-role="card-caption"]')!;
    expect(caption).toHaveClass('text-text-dim');
  });

  it('writes a ranking’s names, links, and its shares at 13 px', async () => {
    render(<ChartSmallMultiple definition={definition('miner-address-distribution')} />);
    const ranking = await screen.findByRole('list', { name: /ranked preview$/ });
    for (const item of within(ranking).getAllByRole('listitem')) {
      expect(item).toHaveClass('text-small');
      expect(item).not.toHaveClass('text-meta');
    }
  });
});

describe('the chart page', () => {
  it('writes the eyebrow’s date, the datum, in the reading ink at 13 px; its words a label', async () => {
    render(<ChartDetail slug="total-supply" />);
    const eyebrow = await screen.findByTestId('figure-eyebrow');
    expect(eyebrow).toHaveClass('text-text-dim', 'text-meta');
    const date = eyebrow.querySelector('[data-role="eyebrow-date"]')!;
    expect(date).not.toBeNull();
    expect(date).toHaveClass('text-text', 'text-small');
    expect(date.textContent).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});
