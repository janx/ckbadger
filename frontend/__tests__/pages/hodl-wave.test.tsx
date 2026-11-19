import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import HodlWavePage from '@/app/charts/hodl-wave/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getHodlWaveChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

const response = {
  title: 'CKB HODL Wave',
  data: [
    {
      date: '2024-01-01',
      values: {
        '24h': '5.00',
        '1d1w': '10.00',
        '1w1m': '15.00',
        '1m3m': '10.00',
        '3m6m': '10.00',
        '6m1y': '15.00',
        '1y3y': '20.00',
        gt3y: '15.00',
        // lane D1: the API's merged bands (10 + 10; 15 + 10 + 5)
        '1m6m': '20.00',
        lt1m: '30.00',
        holderCount: '9007199254740993',
      },
    },
  ],
  bands: [
    { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
    { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
    { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
    { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
    { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
  ],
  series: [
    { key: '24h', label: '24h', color: '#6366f1' },
    { key: '1d1w', label: '1d-1w', color: '#4ade80' },
    { key: '1w1m', label: '1w-1m', color: '#f87171' },
    { key: '1m3m', label: '1m-3m', color: '#f59e0b' },
    { key: '3m6m', label: '3m-6m', color: '#d4e157' },
    { key: '6m1y', label: '6m-1y', color: '#22c55e' },
    { key: '1y3y', label: '1y-3y', color: '#67e8f9' },
    { key: 'gt3y', label: '> 3y', color: '#a78bfa' },
  ],
};

describe('HodlWavePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });
  it('shows the shared title, navigation, eight exact age bands and separate holder count', async () => {
    vi.mocked(api.getHodlWaveChart).mockResolvedValue(response);
    render(<HodlWavePage />);
    expect(await screen.findByText('HODL wave')).toBeInTheDocument();
    // F2 charts (CH-26): a breadcrumb to the chart's own section replaces "← All charts".
    // Re-pinned for lane D1: the board's crumb, its section a link of its own (was "Charts /
    // Activity", one link).
    expect(screen.getByRole('link', { name: 'activity' })).toHaveAttribute(
      'href',
      '/mainnet/charts#activity'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(2));
    const [ages, holders] = chartMock.mock.calls.map(([props]) => props);
    expect(ages).toMatchObject({
      kind: 'stacked',
      // Re-pinned for lane D1 (the chart-hero board, rule 5): the chart draws the API's five
      // bands, oldest first, its merged figures as sent (was every band the API returns, eight)
      series: [
        { key: 'gt3y', label: '3 years or more' },
        { key: '1y3y', label: '1 to 3 years' },
        { key: '6m1y', label: '6 to 12 months' },
        { key: '1m6m', label: '1 to 6 months' },
        { key: 'lt1m', label: 'under 1 month' },
      ],
      data: [
        {
          label: '2024-01-01',
          values: {
            gt3y: '15.00',
            '1y3y': '20.00',
            '6m1y': '15.00',
            '1m6m': '20.00',
            lt1m: '30.00',
          },
        },
      ],
    });
    // The eight ages stay, exact, named as ranges (CH-21), in the table under the chart (the
    // summary row of facts is gone: re-pinned for lane D1, rule 2).
    const table = screen.getByRole('table', { name: 'HODL wave, every band' });
    expect([...table.querySelectorAll('tbody tr')].map((row) => row.textContent)).toEqual([
      '< 1 d5.00%',
      '1 d – 1 w10.00%',
      '1 w – 1 m15.00%',
      '1 m – 3 m10.00%',
      '3 m – 6 m10.00%',
      '6 m – 1 y15.00%',
      '1 y – 3 y20.00%',
      '> 3 y15.00%',
    ]);
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
    expect(holders).toMatchObject({
      title: 'Holder count',
      unit: 'holders',
      data: [{ label: '2024-01-01', values: { holders: '9007199254740993' } }],
    });
  });
  it('shows loading separately from a failed request', async () => {
    vi.mocked(api.getHodlWaveChart).mockReturnValueOnce(new Promise(() => {}));
    const view = render(<HodlWavePage />);
    expect(screen.getByLabelText('HODL wave loading')).toBeInTheDocument();
    view.unmount();
    vi.mocked(api.getHodlWaveChart).mockRejectedValue(new Error('Network error'));
    render(<HodlWavePage />);
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Network error'));
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });
});
