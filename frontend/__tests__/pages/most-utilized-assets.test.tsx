import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import MostUtilizedAssetsPage from '@/app/charts/most-utilized-assets/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedAssetsChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('MostUtilizedAssetsPage', () => {
  it('retains the complete distinct occupied and owned-capacity histories', async () => {
    chartMock.mockClear();
    vi.mocked(api.getMostUtilizedAssetsChart).mockResolvedValue({
      title: 'Assets Used & Total CKBytes',
      usedShare: {
        title: 'Top Assets Common Knowledge Share',
        data: [
          { date: '2024-01-01', values: { top0: '100000001', others: '20' } },
          { date: '2024-01-02', values: { top0: '200000002', others: '30' } },
        ],
        series: [
          { key: 'top0', label: 'Token A (token)', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
        // lane D1: the API names the bands a history is drawn in (re-pinned: it always does)
        bands: [
          { key: 'top0', label: 'Token A (token)', members: ['top0'] },
          { key: 'others', label: 'Others', members: ['others'] },
        ],
      },
      capacityShare: {
        title: 'Top Assets Capacity Share',
        data: [
          { date: '2024-01-01', values: { top0: '300000003', others: '40' } },
          { date: '2024-01-02', values: { top0: '400000004', others: '50' } },
        ],
        series: [
          { key: 'top0', label: 'Cluster A (object)', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
        // lane D1: the API names the bands a history is drawn in (re-pinned: it always does)
        bands: [
          { key: 'top0', label: 'Cluster A (object)', members: ['top0'] },
          { key: 'others', label: 'Others', members: ['others'] },
        ],
      },
    });
    render(<MostUtilizedAssetsPage />);
    expect(await screen.findByText('Assets')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(2));
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      title: 'Occupied common knowledge',
      unit: 'CKB',
      data: [
        { label: '2024-01-01', values: { top0: '1.00000001', others: '0.0000002' } },
        { label: '2024-01-02', values: { top0: '2.00000002', others: '0.0000003' } },
      ],
      // re-pinned for lane D1: an asset's band is its name; its class is the ranking's mark (CH-06),
      // the API's "(token)" kept out of tier 1 (CH-04)
      series: [{ label: 'Token A' }, { label: 'Others' }],
    });
    expect(chartMock.mock.calls[1][0]).toMatchObject({
      title: 'Owned live capacity',
      unit: 'CKB',
      data: [
        { label: '2024-01-01', values: { top0: '3.00000003', others: '0.0000004' } },
        { label: '2024-01-02', values: { top0: '4.00000004', others: '0.0000005' } },
      ],
      series: [{ label: 'Cluster A' }, { label: 'Others' }],
    });
  });
});
