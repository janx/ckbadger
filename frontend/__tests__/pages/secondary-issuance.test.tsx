import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import SecondaryIssuancePage from '@/app/charts/secondary-issuance/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getSecondaryIssuanceChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('SecondaryIssuancePage', () => {
  it('shows exact stacked components and the separate protocol total with overlap evidence', async () => {
    chartMock.mockClear();
    vi.mocked(api.getSecondaryIssuanceChart).mockResolvedValue({
      title: 'Secondary Issuance',
      data: [
        {
          date: '2026-02-23',
          values: { mining: '60.00000001', dao: '30', burnt: '10' },
          protocolTotalShannons: '9900000001',
          phase1CompensationShannons: '100000000',
        },
      ],
      series: [
        { key: 'mining', label: 'Mining Reward', color: '#00c389' },
        { key: 'dao', label: 'Deposit Compensation', color: '#ffb000' },
        { key: 'burnt', label: 'Treasury', color: '#68ccf0' },
      ],
    });
    render(<SecondaryIssuancePage />);
    expect(await screen.findByText('Secondary issuance')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [{ label: '2026-02-23', values: { mining: '60.00000001', dao: '30', burnt: '10' } }],
      series: [
        { label: 'Mining Reward' },
        { label: 'Deposit Compensation' },
        { label: 'Treasury' },
      ],
    });
    expect(screen.getByText(/99.00000001 CKB/)).toBeInTheDocument();
    expect(screen.getByText('1 CKB')).toBeInTheDocument();
  });
});
