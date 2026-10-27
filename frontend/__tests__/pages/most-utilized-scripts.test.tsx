import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import MostUtilizedScriptsPage from '@/app/charts/most-utilized-scripts/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedScriptsChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('MostUtilizedScriptsPage', () => {
  it('retains exact occupied and owned-capacity histories for one script family', async () => {
    chartMock.mockClear();
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts Used & Total CKBytes',
      usedShare: {
        title: 'Top Scripts Common Knowledge Share',
        data: [
          { date: '2024-01-01', values: { top0: '9007199254740993', others: '20' } },
          { date: '2024-01-02', values: { top0: '9007199254740995', others: '30' } },
        ],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
      capacityShare: {
        title: 'Top Scripts Capacity Share',
        data: [
          { date: '2024-01-01', values: { top0: '100000001', others: '40' } },
          { date: '2024-01-02', values: { top0: '200000002', others: '50' } },
        ],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
    });
    render(<MostUtilizedScriptsPage />);
    expect(await screen.findByText('Scripts')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(2));
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      title: 'Occupied common knowledge',
      unit: 'CKB',
      data: [
        { label: '2024-01-01', values: { top0: '90071992.54740993', others: '0.0000002' } },
        { label: '2024-01-02', values: { top0: '90071992.54740995', others: '0.0000003' } },
      ],
      series: [{ label: 'SECP256K1_BLAKE160' }, { label: 'Others' }],
    });
    expect(chartMock.mock.calls[1][0]).toMatchObject({
      title: 'Owned live capacity',
      unit: 'CKB',
      data: [
        { label: '2024-01-01', values: { top0: '1.00000001', others: '0.0000004' } },
        { label: '2024-01-02', values: { top0: '2.00000002', others: '0.0000005' } },
      ],
    });
  });
});
