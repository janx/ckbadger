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
      data: [{ date: '2026-02-23', values: { primary: '70.00000001', secondary: '30.00000002' } }],
      series: [
        { key: 'primary', label: 'Primary Issuance', color: '#00c389' },
        { key: 'secondary', label: 'Secondary Issuance', color: '#ffb000' },
      ],
    });
    render(<TotalSupplyPage />);
    expect(await screen.findByText('Supply')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /All charts/ })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [{ label: '2026-02-23', values: { primary: '70.00000001', secondary: '30.00000002' } }],
      series: [{ label: 'Primary Issuance' }, { label: 'Secondary Issuance' }],
    });
    expect(screen.getByText(/100.00000003 CKB/)).toBeInTheDocument();
    expect(screen.getByText('/charts/total-supply')).toBeInTheDocument();
  });
});
