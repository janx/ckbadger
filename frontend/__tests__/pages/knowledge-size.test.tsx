import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import KnowledgeSizePage from '@/app/charts/knowledge-size/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getKnowledgeSizeChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/line-chart', () => ({
  LineChart: ({
    yAxisLabel,
    y2AxisLabel,
    data,
  }: {
    yAxisLabel: string;
    y2AxisLabel?: string;
    data: Array<{ date: string; value: string }>;
  }) => (
    <div data-testid="line-chart">
      <span>{yAxisLabel}</span>
      {y2AxisLabel && <span>{y2AxisLabel}</span>}
      <span>points:{data.length}</span>
    </div>
  ),
}));

describe('KnowledgeSizePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue({
      title: 'Common Knowledge Size',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Utilization (%)',
      data: [
        { date: '2026-02-18', value: '100.0', value2: '10.0' },
        { date: '2026-02-19', value: '130.0', value2: '13.0' },
      ],
    });
  });

  it('renders merged size/utilization chart and net flow chart', async () => {
    render(<KnowledgeSizePage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Common Knowledge Size')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
      expect(screen.getAllByText('Utilization (%)').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Net Flow (CKB/day)').length).toBeGreaterThan(0);
      expect(screen.getByText('Description')).toBeInTheDocument();
      expect(screen.getByText('Legend Item Calculation')).toBeInTheDocument();
    });
  });
});
