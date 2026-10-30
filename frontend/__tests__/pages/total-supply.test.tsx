import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import TotalSupplyPage from '@/app/charts/total-supply/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getTotalSupplyChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('TotalSupplyPage', () => {
  it('keeps exact supply components and complete-day source navigation', async () => {
    chartMock.mockClear();
    vi.mocked(api.getTotalSupplyChart).mockResolvedValue({
      title: 'Total Supply',
      data: [
        { date: '2026-02-23', values: { circulating: '70.00000001', nervosdao: '30.00000002' } },
      ],
      series: [
        { key: 'circulating', label: 'Circulating', color: '#00c389' },
        { key: 'nervosdao', label: 'Nervos DAO', color: '#3b82f6' },
      ],
    });
    render(<TotalSupplyPage />);
    expect(await screen.findByText('Supply')).toBeInTheDocument();
    // F2 charts (CH-26): a breadcrumb to the chart's own section replaces "← All charts".
    expect(screen.getByRole('link', { name: 'Charts / Nervos DAO & economics' })).toHaveAttribute(
      'href',
      '/mainnet/charts#nervos-dao-economics'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [
        { label: '2026-02-23', values: { circulating: '70.00000001', nervosdao: '30.00000002' } },
      ],
      // F2 charts: the DAO band is gold, the class colour, not the API's blue (CH-06).
      series: [
        { label: 'Circulating', color: '#2edba3' },
        { label: 'Nervos DAO', color: '#f2c55c' },
      ],
    });
    // The head reading (CH-21): CKB stays exact, every digit.
    expect(screen.getByRole('group', { name: 'Supply readings' })).toHaveTextContent(
      '2026-02-23100.00000003CKB'
    );
    expect(screen.getByText('/charts/total-supply')).toBeInTheDocument();
  });
});
