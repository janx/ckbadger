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
        {
          date: '2026-02-23',
          values: { circulating: '70.00000001', nervosdao: '30.00000002', burnt: '0.00000001' },
        },
      ],
      series: [
        { key: 'circulating', label: 'Circulating', color: '#00c389' },
        { key: 'nervosdao', label: 'Nervos DAO', color: '#3b82f6' },
        // the API always sends the three places (the head's sentence reads all three)
        { key: 'burnt', label: 'Burnt', color: '#6b7280' },
      ],
    });
    render(<TotalSupplyPage />);
    expect(await screen.findByText('Supply')).toBeInTheDocument();
    // F2 charts (CH-26): a breadcrumb to the chart's own section replaces "← All charts".
    // re-pinned for lane D1: the board's crumb, its section a link of its own
    expect(screen.getByRole('link', { name: 'nervos dao & economics' })).toHaveAttribute(
      'href',
      '/mainnet/charts#nervos-dao-economics'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [
        {
          label: '2026-02-23',
          values: { circulating: '70.00000001', nervosdao: '30.00000002', burnt: '0.00000001' },
        },
      ],
      // F2 charts: the DAO band is gold, the class colour, not the API's blue (CH-06).
      series: [
        { label: 'Circulating', color: 'var(--c-jade)' },
        { label: 'Nervos DAO', color: 'var(--c-gold)' },
        { label: 'Burnt', color: 'var(--c-slate-ckb)' },
      ],
    });
    // The head figure is the summary form, cut, every digit of it in its title (readability R5).
    // Re-pinned for lane D1 (rule 3): the figure is the circulating CKB, named in words (was the
    // stack's whole, 100.00000003, in a readings group); the whole is the sentence's.
    const figure = screen.getByTestId('figure-value');
    expect(figure).toHaveTextContent('70 CKB');
    expect(figure).toHaveAttribute('title', '70.00000001 CKB');
    expect(screen.getByTestId('figure-eyebrow')).toHaveTextContent('circulating · 2026-02-23');
    expect(screen.getByText('/charts/total-supply')).toBeInTheDocument();
  });
});
