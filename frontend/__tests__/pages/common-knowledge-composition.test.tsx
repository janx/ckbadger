import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import CommonKnowledgeCompositionPage from '@/app/charts/common-knowledge-composition/page';
import { api } from '@/lib/api';

const chartMock = vi.fn(({ title }: { title: string }) => <div role="img" aria-label={title} />);
vi.mock('@/lib/api', () => ({
  api: {
    getCommonKnowledgeCompositionChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/ui/chart', () => ({ Chart: (props: never) => chartMock(props) }));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('CommonKnowledgeCompositionPage', () => {
  // F2 charts: the API's own kinds, named and coloured by class (CH-06, CH-14).
  it('renders exact occupied components by kind in the shared chart', async () => {
    chartMock.mockClear();
    vi.mocked(api.getCommonKnowledgeCompositionChart).mockResolvedValue({
      title: 'Common Knowledge Bytes Composition',
      data: [{ date: '2026-02-23', values: { transfer: '100.00000001', dao: '50.00000002' } }],
      series: [
        { key: 'transfer', label: 'CKB Transfer', color: '#22c55e' },
        { key: 'dao', label: 'DAO', color: '#f59e0b' },
      ],
    });
    render(<CommonKnowledgeCompositionPage />);
    expect(await screen.findByText('Knowledge by kind')).toBeInTheDocument();
    // F2 charts (CH-26): a breadcrumb to the chart's own section replaces "← All charts".
    expect(screen.getByRole('link', { name: 'Charts / Common knowledge' })).toHaveAttribute(
      'href',
      '/mainnet/charts#common-knowledge'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [{ label: '2026-02-23', values: { transfer: '100.00000001', dao: '50.00000002' } }],
      series: [
        { label: 'Plain CKB', color: '#3a4258' },
        { label: 'Nervos DAO', color: '#f2c55c' },
      ],
    });
  });
});
