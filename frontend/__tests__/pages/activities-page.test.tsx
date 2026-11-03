import { CHARTS } from '@/components/charts/chart-catalogue';
import { vi, it, expect, beforeEach, afterEach } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import ActivitiesPage from '@/app/activities/page';
import { api } from '@/lib/api';
import { dailyActivities } from '@/__tests__/msw/fixtures/home';
vi.mock('@/lib/api', async () => ({
  ...(await vi.importActual('@/lib/api')),
  api: { getDailyActivityStats: vi.fn(), getTxStats: vi.fn() },
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
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 99,
    hourlyData: [],
    dailyData: [
      { label: '10/25', value: 60, complete: true },
      { label: '10/26', value: 99, complete: false },
    ],
  });
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

it('uses the exact non-cellbase count instead of overlapping kind occurrences', async () => {
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    {
      ...dailyActivities[0],
      date: '20261025',
      coinbaseCount: 10,
      transferCount: 12,
      daoDepositCount: 1,
      daoWithdrawRequestCount: 2,
      daoWithdrawCompleteCount: 3,
      tokenCount: 4,
      objectCount: 5,
      identityCount: 6,
      scriptCallCount: 7,
      uniqueAddressCount: 42,
    },
    { ...dailyActivities[0], date: '20261026', uniqueAddressCount: 9999 },
  ]);
  render(<ActivitiesPage />);
  expect(await screen.findByText('50')).toBeInTheDocument();
  expect(screen.getByText('42')).toBeInTheDocument();
  expect(screen.getByTestId('stream-summary')).toHaveTextContent('20261025');
  expect(screen.getByRole('link', { name: 'Transactions →' })).toHaveAttribute(
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
    { ...dailyActivities[0], date: '20261025', coinbaseCount: 10 },
  ]);
  render(<ActivitiesPage />);
  expect(await screen.findByText('50')).toBeInTheDocument();
  expect(screen.getByTestId('stream-summary')).toHaveTextContent('20261025');
});
