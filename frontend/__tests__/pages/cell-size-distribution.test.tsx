import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import CellSizeDistributionPage from '@/app/charts/cell-size-distribution/page';

vi.mock('@/lib/api', () => ({
  api: {
    getCellSizeDistributionChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/charts/chart-page', () => ({
  ChartPage: ({
    title,
    queryKey,
    chartType,
  }: {
    title: string;
    queryKey: string;
    chartType?: string;
  }) => (
    <div>
      <span>{title}</span>
      <span>{queryKey}</span>
      <span>{chartType}</span>
    </div>
  ),
}));

describe('CellSizeDistributionPage', () => {
  it('passes chart title and query key to ChartPage', () => {
    render(<CellSizeDistributionPage />);
    expect(screen.getByText('Cell Size Distribution')).toBeInTheDocument();
    expect(screen.getByText('chart-cell-size-distribution')).toBeInTheDocument();
    expect(screen.getByText('bar')).toBeInTheDocument();
  });
});
