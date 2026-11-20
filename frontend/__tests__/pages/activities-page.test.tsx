import { CHARTS } from '@/components/charts/chart-catalogue';
import { vi, it, expect, beforeEach, afterEach } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import ActivitiesPage from '@/app/activities/page';
import { ErrorBoundary } from '@/components/error-boundary';
import { api } from '@/lib/api';
import { dailyActivities } from '@/__tests__/msw/fixtures/home';
vi.mock('@/lib/api', async () => ({
  ...(await vi.importActual('@/lib/api')),
  api: { getDailyActivityStats: vi.fn() },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
vi.mock('@/components/activities-stream-explorer', () => ({
  ActivitiesStreamExplorer: ({ summary }: { summary?: { date: string } }) => (
    <div data-testid="stream-summary">{summary?.date ?? 'none'}</div>
  ),
}));
beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(new Date('2026-10-26T06:00:00Z'));
});
afterEach(() => vi.useRealTimers());
// Lane L (charts ↔ topics).
it('links every chart drawn from activities (lane L)', async () => {
  render(<ActivitiesPage />);
  const charts = await screen.findByRole('navigation', { name: 'Charts of these numbers' });
  const hrefs = within(charts)
    .getAllByRole('link')
    .map((a) => a.getAttribute('href'));
  expect(hrefs).toEqual(
    CHARTS.filter((c) => c.evidenceHref === '/activities').map((c) => `/mainnet/charts/${c.slug}`)
  );
});

// Re-pinned on purpose (the P3 follow-up's 2b, then F-6): the count is the route's, the day's
// activityCount, every transaction but the cellbases in exactly one kind (it was the route's
// activityCount while the kinds overlapped; before that the page subtracted the cellbases).
it('states the exact non-cellbase count, the route’s activityCount, which the kinds partition', async () => {
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    {
      ...dailyActivities[0],
      date: '20261025',
      coinbaseCount: 10,
      activityCount: 50,
      kindCounts: { dao: 3, token: 4, object: 5, identity: 6, protocol: 13, script: 7, ckb: 12 },
      uniqueAddressCount: 42,
    },
    { ...dailyActivities[0], date: '20261026', uniqueAddressCount: 9999 },
  ]);
  render(<ActivitiesPage />);
  expect(await screen.findByText('50')).toBeInTheDocument();
  expect(screen.getByText('42')).toBeInTheDocument();
  // what the page says first (lists lane P3): the count with its noun and its period, once
  expect(document.querySelector('[data-role="lede"]')).toHaveTextContent(
    /^50\sactivities yesterday \(UTC\+8\), by 42\saddresses\.$/
  );
  expect(screen.getByTestId('stream-summary')).toHaveTextContent('20261025');
  // re-pinned on purpose (lists lane P3, the link-look gate): a way onward, its arrow the look's
  expect(screen.getByRole('link', { name: /^Transactions/ })).toHaveAttribute(
    'href',
    '/mainnet/transactions'
  );
});
it('does not silently substitute stale statistics for a missing completed day', async () => {
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    { ...dailyActivities[0], date: '20261024' },
  ]);
  render(<ActivitiesPage />);
  expect(
    await screen.findByText('No complete activity statistics for 2026-10-25 (UTC+8).')
  ).toBeInTheDocument();
  expect(screen.getByTestId('stream-summary')).toHaveTextContent('none');
});

it('selects UTC+8 yesterday at the UTC date boundary', async () => {
  vi.setSystemTime(new Date('2026-10-25T17:00:00Z'));
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    {
      ...dailyActivities[0],
      date: '20261025',
      activityCount: 50,
      kindCounts: { dao: 0, token: 0, object: 0, identity: 0, protocol: 0, script: 0, ckb: 50 },
    },
  ]);
  render(<ActivitiesPage />);
  expect(await screen.findByText('50')).toBeInTheDocument();
  expect(screen.getByTestId('stream-summary')).toHaveTextContent('20261025');
});

// The third read's La N6, with S5's rule: a page on an API one version older refuses by name or
// draws truthfully. Every field this plan added and this page reads goes through the one reader
// (lib/api-fields.ts sentField): "<what>: the API sends no <field>: it is older than this page".
// it drew the lede without its count, silently
it('refuses yesterday without activityCount by name, never a lede with no count', async () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
  const { activityCount: _gone, ...older } = dailyActivities[0];
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    { ...older, date: yesterday.replaceAll('-', '') } as (typeof dailyActivities)[0],
  ]);
  render(
    <ErrorBoundary fallbackRender={(error) => <p role="alert">{error.message}</p>}>
      <ActivitiesPage />
    </ErrorBoundary>
  );
  expect(
    await screen.findByText(
      `daily activities on ${yesterday.replaceAll('-', '')}: the API sends no activityCount: it is older than this page`
    )
  ).toHaveAttribute('role', 'alert');
  log.mockRestore();
});
