// Lane L (charts ↔ topics): a topic page links every chart whose numbers come from it (the
// catalogue's evidenceHref), by the chart's title; a chart links back through "Explore …".
import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { TopicCharts } from '@/components/charts/topic-charts';
import { CHARTS } from '@/components/charts/chart-catalogue';

describe('TopicCharts', () => {
  it('links each chart of the topic, in catalogue order, by its title', () => {
    render(<TopicCharts topic="/dao" />);
    const nav = screen.getByRole('navigation', { name: 'Charts of these numbers' });
    const expected = CHARTS.filter((c) => c.evidenceHref === '/dao');
    expect(expected.length).toBeGreaterThan(3);
    const links = within(nav).getAllByRole('link');
    expect(links.map((a) => a.textContent)).toEqual(expected.map((c) => c.title));
    expect(links.map((a) => a.getAttribute('href'))).toEqual(
      expected.map((c) => `/mainnet/charts/${c.slug}`)
    );
  });

  it('covers every chart of the catalogue across the six topics', () => {
    const topics = ['/blocks', '/transactions', '/activities', '/dao', '/scripts', '/inventory'];
    expect(new Set(CHARTS.map((c) => c.evidenceHref))).toEqual(new Set(topics));
  });

  it('refuses a topic no chart takes its numbers from', () => {
    expect(() => render(<TopicCharts topic={'/network' as '/dao'} />)).toThrow(
      'No chart takes its numbers from /network'
    );
  });
});
