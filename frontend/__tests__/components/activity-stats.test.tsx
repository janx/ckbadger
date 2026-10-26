import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { dailyActivities, activitySummary24h } from '@/__tests__/msw/fixtures/home';
import { ActivityStats, dailyActivityView } from '@/components/home/activity-stats';

const API = '/api/:network/v1';

describe('ActivityStats', () => {
  beforeEach(() => stubChartWidth(560));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('heads the column "Activity, last 24 h" with a link to the charts', () => {
    render(<ActivityStats />);
    expect(
      screen.getByRole('heading', { level: 2, name: 'Activity, last 24 h' })
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Charts →' })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );
  });

  it('shows three KPIs: activities, addresses and CKB moved', async () => {
    render(<ActivityStats />);
    const stats = screen.getByTestId('home-activity-stats');
    await waitFor(() => expect(stats).toHaveTextContent('Activities7,115'));
    expect(stats).toHaveTextContent('Addresses1,497');
    expect(stats).toHaveTextContent('CKB moved346.28M');
  });

  it('uses the exact transaction count independently of overlapping category occurrences', async () => {
    server.use(
      http.get(`${API}/stats/activity-summary-24h`, () =>
        HttpResponse.json({ ...activitySummary24h, activityCount: 123 })
      )
    );
    render(<ActivityStats />);
    await waitFor(() =>
      expect(screen.getByTestId('home-activity-stats')).toHaveTextContent('Activities123')
    );
    expect(screen.getByTestId('home-activity-types')).toHaveTextContent('7,115 kind occurrences');
  });

  it('ranks activity types by share, hiding the empty ones', async () => {
    render(<ActivityStats />);
    const types = await screen.findByTestId('home-activity-types');
    expect(types).toHaveTextContent('By type, share of 7,115');
    const labels = [...types.querySelectorAll('[data-rank-row]')].map((r) =>
      r.getAttribute('data-rank-row')
    );
    expect(labels).toEqual(['script', 'transfer', 'identity', 'dao']);
    expect(types).toHaveTextContent('Script call81.4%');
    expect(types).not.toHaveTextContent('Token');
    expect(within(types).getByRole('link', { name: /Script call/ })).toHaveAttribute(
      'href',
      '/mainnet/charts/activity-type-breakdown'
    );
  });

  it('ranks the top five scripts and folds the rest into one row', async () => {
    render(<ActivityStats />);
    const scripts = await screen.findByTestId('home-activity-scripts');
    expect(scripts).toHaveTextContent('By script, share of 11,300 calls');
    const rows = [...scripts.querySelectorAll('[data-rank-row]')];
    expect(rows).toHaveLength(6);
    expect(rows[0]).toHaveTextContent('.bit Time Index State48.5%');
    expect(rows[5]).toHaveTextContent('3 others0.1%');
    expect(rows[5].querySelector('a')).toBeNull();
    expect(within(scripts).getByRole('link', { name: /Bitcoin SPV Type Lock/ })).toHaveAttribute(
      'href',
      '/mainnet/scripts/Bitcoin%20SPV%20Type%20Lock'
    );
    expect(
      within(scripts).getByRole('link', { name: 'By script, share of 11,300 calls' })
    ).toHaveAttribute('href', '/mainnet/charts/most-utilized-scripts');
    // No donuts and no placeholder.
    expect(screen.queryByText(/Hover for details/)).not.toBeInTheDocument();
  });

  it('charts daily activity with the last day highlighted as the headline', async () => {
    render(<ActivityStats />);
    await waitFor(() =>
      expect(screen.getByTestId('home-daily-activity-headline')).toHaveTextContent('6,691')
    );
    const fig = screen.getByTestId('home-daily-activity');
    expect(fig).toHaveTextContent('kind occurrences per day, 09/25 to 10/23');
    const svg = within(fig).getByRole('img');
    const bars = svg.querySelectorAll('[data-bar]');
    expect(bars).toHaveLength(29);
    expect(bars[28]).toHaveAttribute('data-bar', 'highlight');
    expect(svg).toHaveAccessibleName(/6,691 on 10\/23$/);
  });

  it('reports a failed summary as an error', async () => {
    server.use(
      http.get(`${API}/stats/activity-summary-24h`, () => HttpResponse.json({}, { status: 500 }))
    );
    render(<ActivityStats />);
    expect(await screen.findByRole('alert')).toHaveTextContent(/Failed to load/);
  });
});

describe('dailyActivityView', () => {
  it('counts typed activities per day and rejects days out of order', () => {
    const view = dailyActivityView(dailyActivities);
    expect(view?.last).toEqual({ label: '10/23', value: 6691 });
    expect(() => dailyActivityView([dailyActivities[1], dailyActivities[0]])).toThrow(
      /out of order/
    );
    expect(dailyActivityView([])).toBeNull();
  });
});
