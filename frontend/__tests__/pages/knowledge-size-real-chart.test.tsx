import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import KnowledgeSizePage from '@/app/charts/knowledge-size/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: { getKnowledgeSizeChart: vi.fn() },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div>Header</div>,
}));

describe('KnowledgeSizePage with the real line chart', () => {
  it('renders a falling knowledge size as signed net flow with an exact hover value', async () => {
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
    const charts = await screen.findAllByTestId('line-series-primary');
    expect(charts).toHaveLength(2);
    expect(charts[1].getAttribute('d')).not.toMatch(/NaN/);
    const svg = container.querySelectorAll('svg')[1];
    Object.defineProperty(svg, 'getBoundingClientRect', { value: () => ({ left: 0, width: 600 }) });
    fireEvent.mouseMove(svg, { clientX: 580 });
    expect(screen.getByText('-30.00')).toBeInTheDocument();
  });
});
