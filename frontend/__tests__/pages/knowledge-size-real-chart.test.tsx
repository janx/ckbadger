import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import KnowledgeSizePage from '@/app/charts/knowledge-size/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getKnowledgeSizeChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('KnowledgeSizePage with the real chart', () => {
  it('shows a falling exact signed daily flow through keyboard readout', async () => {
    stubChartWidth(600);
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue({
      title: 'Common Knowledge Size',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Utilization (%)',
      data: [
        { date: '2026-02-18', value: '130.00000000', value2: '13' },
        { date: '2026-02-19', value: '100.00000000', value2: '10' },
      ],
    });
    const { container } = render(<KnowledgeSizePage />);
    const svg = await screen.findByRole('img', { name: 'Net knowledge flow, CKB/day' });
    const path = svg.querySelector('path')?.getAttribute('d');
    expect(path).toMatch(/^M/);
    expect(path).not.toMatch(/NaN|Infinity/);
    fireEvent.keyDown(svg, { key: 'ArrowLeft' });
    expect(screen.getByText('-30')).toBeInTheDocument();
    expect(container).toHaveTextContent('Exact signed change in occupied CKB');
  });
});
