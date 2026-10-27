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
  it('renders both exact occupied and free components in the shared chart', async () => {
    chartMock.mockClear();
    vi.mocked(api.getCommonKnowledgeCompositionChart).mockResolvedValue({
      title: 'Common Knowledge Bytes Composition',
      data: [{ date: '2026-02-23', values: { used: '100.00000001', unused: '50.00000002' } }],
      series: [
        { key: 'used', label: 'Used', color: '#00c389' },
        { key: 'unused', label: 'Unused', color: '#64748b' },
      ],
    });
    render(<CommonKnowledgeCompositionPage />);
    expect(await screen.findByText('Knowledge by kind')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /All charts/ })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );
    await waitFor(() => expect(chartMock).toHaveBeenCalled());
    expect(chartMock.mock.calls[0][0]).toMatchObject({
      kind: 'stacked',
      unit: 'CKB',
      data: [{ label: '2026-02-23', values: { used: '100.00000001', unused: '50.00000002' } }],
      series: [{ label: 'Used' }, { label: 'Unused' }],
    });
  });
});
