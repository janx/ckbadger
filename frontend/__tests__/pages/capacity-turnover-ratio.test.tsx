import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import CapacityTurnoverRatioPage from '@/app/charts/capacity-turnover-ratio/page';

vi.mock('@/lib/api', () => ({
  api: {
    getCapacityTurnoverRatioChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/charts/chart-page', () => ({
  ChartPage: ({ title, queryKey }: { title: string; queryKey: string }) => (
    <div>
      <span>{title}</span>
      <span>{queryKey}</span>
    </div>
  ),
}));

describe('CapacityTurnoverRatioPage', () => {
  it('passes chart title and query key to ChartPage', () => {
    render(<CapacityTurnoverRatioPage />);
    expect(screen.getByText('Capacity Turnover Ratio')).toBeInTheDocument();
    expect(screen.getByText('chart-capacity-turnover-ratio')).toBeInTheDocument();
  });
});
