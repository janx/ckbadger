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
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('MostUtilizedScriptsPage', () => {
  it('retains exact occupied and owned-capacity histories for one script family', async () => {
    chartMock.mockClear();
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts Used & Total CKBytes',
      // each day's whole, the route's (the D1 hand-back, item 2)
      usedShare: {
        title: 'Top Scripts Common Knowledge Share',
        data: [
          {
            date: '2024-01-01',
            values: {
              top0: '9007199254740993',
              others: '20',
              total: '9007199254741013',
              drawnTotal: '9007199254740993',
            },
          },
          {
            date: '2024-01-02',
            values: {
              top0: '9007199254740995',
              others: '30',
              total: '9007199254741025',
              drawnTotal: '9007199254740995',
            },
          },
        ],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
        // lane D1: the API names the bands a history is drawn in (re-pinned: it always does);
        // each day states the drawn ranks' total, drawnTotal (lane D1's third round, M2)
        bands: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', members: ['top0'] },
          { key: 'others', label: 'Others', members: ['others'] },
        ],
      },
      capacityShare: {
        title: 'Top Scripts Capacity Share',
        data: [
          {
            date: '2024-01-01',
            values: {
              top0: '100000001',
              others: '40',
              total: '100000041',
              drawnTotal: '100000001',
            },
          },
          {
            date: '2024-01-02',
            values: {
              top0: '200000002',
              others: '50',
              total: '200000052',
              drawnTotal: '200000002',
            },
          },
        ],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
        // lane D1: the API names the bands a history is drawn in (re-pinned: it always does)
        bands: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', members: ['top0'] },
          { key: 'others', label: 'Others', members: ['others'] },
        ],
      },
    });
    render(<MostUtilizedScriptsPage />);
    expect(await screen.findByText('Scripts')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalledTimes(2));
    // Re-pinned for lane D1 (the chart-hero board, rules 2 and 5): no row of facts under the
    // lede (the leader is the figure's eyebrow; was "Largest script" in a summary grid, C#10), and
    // each scope's bands are named at their right ends, solid, with their shares (were a swatch
    // legend under the plot, C#5; a phone still keys them under it).
    expect(screen.getByRole('banner').querySelector('dl')).toBeNull();
    for (const call of chartMock.mock.calls.slice(0, 2))
      expect(call[0]).toMatchObject({
        solidBands: true,
        bandEnds: { top0: expect.stringMatching(/%$/), others: expect.stringMatching(/%$/) },
      });
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
