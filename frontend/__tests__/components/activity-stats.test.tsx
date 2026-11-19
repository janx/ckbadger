import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  act,
  createTestQueryClient,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { dailyActivities, activitySummary24h } from '@/__tests__/msw/fixtures/home';
import { ActivityStats, dailyActivityView } from '@/components/home/activity-stats';
import { ACTIVITY_HEAD_CLASS } from '@/components/home/section';

const API = '/api/:network/v1';

describe('ActivityStats', () => {
  beforeEach(() => stubChartWidth(560));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  // Rubbing × Song print, phase 5 (H5): `■ activity --24 h`, "charts →" as the note, in the
  // feed's heading row (one class), so the two command lines stand on one line.
  it('heads the column "activity --24 h" with a link to the charts as its note', () => {
    render(<ActivityStats />);
    const heading = screen.getByRole('heading', { level: 2, name: 'activity --24 h' });
    expect(heading).toHaveClass('text-section');
    const [command, args] = Array.from(heading.children) as HTMLElement[];
    expect(command).toHaveTextContent(/^activity$/);
    expect(args).toHaveTextContent(/^--24 h$/);
    expect(args).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
    // re-pinned on purpose (lists lane P3, the link-look gate): a way onward, its arrow the look's
    const link = screen.getByRole('link', { name: /^charts/ });
    expect(link).toHaveAttribute('href', '/mainnet/charts');
    const note = link.closest('[data-prompt-note]') as HTMLElement;
    expect(note).toHaveClass('ml-auto');
    expect(note.parentElement).toBe(heading.parentElement);
    expect(heading).not.toContainElement(link);
    // the feed's heading row: the same class, no rule of its own
    const row = screen.getByTestId('home-activity-stats').firstElementChild as HTMLElement;
    expect(row).toContainElement(heading);
    expect(row).toHaveClass(ACTIVITY_HEAD_CLASS);
    expect(row).not.toHaveClass('border-b');
  });

  // Material pass v2 (move 4): a count flares (`.phosphor-flare`) when its served value changes.
  it('flares the count that changed, and no other', async () => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <ActivityStats />
      </QueryClientProvider>
    );
    const stats = screen.getByTestId('home-activity-stats');
    await waitFor(() => expect(stats).toHaveTextContent('Activities7,115'));
    expect(screen.getByText('7,115')).not.toHaveClass('phosphor-flare');
    act(() => {
      client.setQueryData(['activity-summary-24h'], {
        ...activitySummary24h,
        activityCount: 7116,
      });
    });
    // react-query notifies its observers on the next tick
    await waitFor(() => expect(screen.getByText('7,116')).toHaveClass('phosphor-flare'));
    expect(screen.getByText('1,497')).not.toHaveClass('phosphor-flare');
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
    // CH-14 (re-pinned): the one percentage of a count in the total, two decimals (was 81.4%)
    // re-pinned for lane exactness, part B2 (one percentage, cut): 5,795 script calls of 7,115 kind occurrences is 81.447…%, 81.44% (half up 81.45%)
    expect(types).toHaveTextContent('Script call81.44%');
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
    // re-pinned for lane exactness, part B2 (one percentage, cut): 5,480 of 11,300 calls is 48.495…%, 48.49% (half up 48.50%)
    expect(rows[0]).toHaveTextContent('.bit Time Index State48.49%');
    // and the rest's 11 calls of 11,300 0.0973…%, 0.09% (half up 0.10%)
    expect(rows[5]).toHaveTextContent('3 others0.09%');
    expect(rows[5].querySelector('a')).toBeNull();
    expect(within(scripts).getByRole('link', { name: /Bitcoin SPV Type Lock/ })).toHaveAttribute(
      'href',
      '/mainnet/scripts/Bitcoin%20SPV%20Type%20Lock'
    );
    expect(
      within(scripts).getByRole('link', { name: 'By script, share of 11,300\u00a0calls' })
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
    expect(fig).toHaveTextContent('activities per day, 09/25 to 10/23');
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

// Re-pinned on purpose (the P3 follow-up's 2b): a day's bar is the route's count of its user
// transactions (userTransactionCount); it was the transaction-count chart's day less the day's
// cellbases, joined and subtracted here.
describe('dailyActivityView', () => {
  it('draws each day the route’s user transactions, not its overlapping kinds', () => {
    const view = dailyActivityView([{ ...dailyActivities[0], userTransactionCount: 7 }]);
    expect(view?.last.value).toBe(7);
  });
  it('draws the days oldest first and rejects days out of order', () => {
    const view = dailyActivityView(dailyActivities);
    expect(view?.last).toEqual({ label: '10/23', value: 6691 });
    expect(() => dailyActivityView([dailyActivities[1], dailyActivities[0]])).toThrow(
      /out of order/
    );
    expect(dailyActivityView([])).toBeNull();
  });
});

// The third read's La N6, with S5's rule: a page on an API one version older refuses by name or
// draws truthfully. Every field this plan added and this page reads goes through the one reader
// (lib/api-fields.ts sentField): "<what>: the API sends no <field>: it is older than this page".
describe('dailyActivityView on an older API', () => {
  // it drew a TypeError on the day's count
  it('refuses a day without userTransactionCount by name', () => {
    const { userTransactionCount: _gone, ...older } = dailyActivities[0];
    expect(() => dailyActivityView([older as (typeof dailyActivities)[0]])).toThrow(
      `daily activities on ${dailyActivities[0].date}: the API sends no userTransactionCount: it is older than this page`
    );
  });
});
