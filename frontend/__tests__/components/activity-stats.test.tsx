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

  it('shares the types of the one transaction count the API sends: one kind per transaction', async () => {
    server.use(
      http.get(`${API}/stats/activity-summary-24h`, () =>
        HttpResponse.json({
          ...activitySummary24h,
          activityCount: 20,
          kindCounts: { dao: 1, token: 2, object: 3, identity: 4, protocol: 5, script: 0, ckb: 5 },
        })
      )
    );
    render(<ActivityStats />);
    await waitFor(() =>
      expect(screen.getByTestId('home-activity-stats')).toHaveTextContent('Activities20')
    );
    const types = screen.getByTestId('home-activity-types');
    expect(types).toHaveTextContent('By type, share of 20 transactions');
    expect(types).not.toHaveTextContent('occurrences');
    // F-6: a protocol-only transaction is a kind of its own, with its share of the total.
    expect(types).toHaveTextContent('Protocol25.00%');
  });

  it('ranks activity types by share, hiding the empty ones', async () => {
    render(<ActivityStats />);
    const types = await screen.findByTestId('home-activity-types');
    expect(types).toHaveTextContent('By type, share of 7,115');
    const labels = [...types.querySelectorAll('[data-rank-row]')].map((r) =>
      r.getAttribute('data-rank-row')
    );
    expect(labels).toEqual(['script', 'transfer', 'identity', 'dao', 'protocol']);
    // CH-14 (re-pinned): the one percentage of a count in the total, two decimals (was 81.4%)
    expect(types).toHaveTextContent('Script call81.45%');
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
    expect(rows[0]).toHaveTextContent('.bit Time Index State48.50%');
    expect(rows[5]).toHaveTextContent('3 others0.10%');
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

  // Lane rd/api-lock-names (T N9): each share row is a pair. An unnamed pair links to its code
  // hash with its hash type (no guess of another pair's page); a pair named by another reference
  // writes its hash type beside the name and links to its family's page.
  it('links an unnamed pair with its hash type and writes a name by another reference', async () => {
    const codeHash = `0x${'9b'.repeat(32)}`;
    server.use(
      http.get(`${API}/stats/activity-summary-24h`, () =>
        HttpResponse.json({
          ...activitySummary24h,
          scriptCounts: [
            { codeHash, hashType: 'data', name: null, count: 30 },
            { codeHash, hashType: 'data1', name: 'Default Lock', nameBy: 'data1', count: 20 },
          ],
          // the route's whole since the merge of fix/readability: its total and its rest
          scriptCalls: 50,
          scriptRest: null,
        })
      )
    );
    render(<ActivityStats />);
    const scripts = await screen.findByTestId('home-activity-scripts');
    await waitFor(() => expect(scripts.querySelectorAll('[data-rank-row]')).toHaveLength(2));
    expect(within(scripts).getByRole('link', { name: /0x9b9b.*by data\b/ })).toHaveAttribute(
      'href',
      `/mainnet/script/${codeHash}?hashType=data`
    );
    expect(within(scripts).getByRole('link', { name: /Default Lock · by data1/ })).toHaveAttribute(
      'href',
      '/mainnet/scripts/Default%20Lock'
    );
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

// Re-pinned on purpose (the P3 follow-up's 2b, then F-6): a day's bar is the route's count of its
// activities (activityCount, every transaction but the cellbases, each in one kind); it was the
// route's userTransactionCount while the kinds overlapped, and before that the transaction-count
// chart's day less the day's cellbases, joined and subtracted here.
describe('dailyActivityView', () => {
  it('draws each day the route’s activities, the activityCount its kinds partition', () => {
    const view = dailyActivityView([
      {
        ...dailyActivities[0],
        activityCount: 7,
        kindCounts: { dao: 1, token: 0, object: 0, identity: 0, protocol: 2, script: 0, ckb: 4 },
      },
    ]);
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
