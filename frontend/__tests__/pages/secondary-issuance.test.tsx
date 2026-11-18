import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import SecondaryIssuancePage from '@/app/charts/secondary-issuance/page';
import { api } from '@/lib/api';
import { exactDecimalRatio } from '@/lib/exact-integer';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getSecondaryIssuanceChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('SecondaryIssuancePage', () => {
  it('shows disjoint exact secondary components, protocol total, and frozen phase-1 evidence', async () => {
    chartMock.mockClear();
    vi.mocked(api.getSecondaryIssuanceChart).mockResolvedValue({
      title: 'Secondary Issuance',
      data: [
        {
          date: '2026-02-23',
          values: { mining: '60.00000001', compensation: '29', burnt: '10' },
          protocolTotalShannons: '9900000001',
          phase1CompensationShannons: '100000000',
        },
      ],
      series: [
        { key: 'compensation', label: 'Deposit Compensation', color: '#00c389' },
        { key: 'mining', label: 'Mining Reward', color: '#8b5cf6' },
        { key: 'burnt', label: 'Burnt', color: '#6b7280' },
      ],
    });
    render(<SecondaryIssuancePage />);
    expect(await screen.findByText('Secondary issuance')).toBeInTheDocument();
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [
        { label: '2026-02-23', values: { mining: '60.00000001', compensation: '29', burnt: '10' } },
      ],
      // F2 charts: board order and class colours, miners jade not violet (CH-06).
      series: [
        { label: 'Burnt', color: 'var(--c-slate-ckb)' },
        { label: 'DAO compensation', color: 'var(--c-gold)' },
        { label: 'Miners', color: 'var(--c-jade)' },
      ],
    });
    const values = (
      chartMock.mock.calls[0][0] as unknown as { data: { values: Record<string, string> }[] }
    ).data[0].values;
    const shannons = (key: string) => {
      const [numerator, denominator] = exactDecimalRatio(values[key], `secondary ${key}`);
      const scaled = numerator * BigInt(100_000_000);
      expect(scaled % denominator).toBe(BigInt(0));
      return scaled / denominator;
    };
    expect(shannons('mining') + shannons('compensation') + shannons('burnt')).toBe(
      BigInt('9900000001')
    );
    // Re-pinned for CH-18: the protocol total is the head figure alone (its fact repeated it), and
    // the frozen phase-1 overlap is a developer fact in the evidence tier.
    // The head figure is the summary form, cut, its exact value in the reading's title (readability R5, re-pinned on purpose:
    // was "99.00000001 CKB"); the unit after a no-break space (re-pinned for CH-14).
    const readings = screen.getByRole('group', { name: 'Secondary issuance readings' });
    expect(readings).toHaveTextContent('99 CKB');
    expect(readings.querySelector('[title="99.00000001 CKB"]')).not.toBeNull();
    const evidence = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(evidence).toHaveTextContent('Frozen phase-1 overlap: 1 CKB');
  });
});
