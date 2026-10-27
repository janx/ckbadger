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
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
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
        holderCount: '9007199254740993',
      },
    },
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
    expect(screen.getByRole('link', { name: /All charts/ })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(2));
    const [ages, holders] = chartMock.mock.calls.map(([props]) => props);
    expect(ages).toMatchObject({
      kind: 'stacked',
      series: response.series.map(({ key, label }) => ({ key, label })),
      data: [{ label: '2024-01-01', values: expect.objectContaining({ gt3y: '15.00' }) }],
    });
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
