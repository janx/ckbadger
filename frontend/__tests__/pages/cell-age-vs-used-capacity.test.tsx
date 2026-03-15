import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import CellAgeVsUsedCapacityPage from '@/app/charts/cell-age-vs-used-capacity/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getCellAgeVsUsedCapacityChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: () => <div data-testid="stacked-area-chart" />,
}));

describe('CellAgeVsUsedCapacityPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getCellAgeVsUsedCapacityChart).mockResolvedValue({
      title: 'Cell Age vs Used Capacity',
      data: [
        {
          date: '2026-02-19',
          values: {
            lt1d: '10',
            d1to7d: '20',
            d7to30d: '30',
            d30to180d: '40',
            gt180d: '50',
          },
        },
      ],
      series: [
        { key: 'lt1d', label: '< 1d', color: '#22c55e' },
        { key: 'd1to7d', label: '1-7d', color: '#84cc16' },
      ],
    });
  });

  it('renders title and stacked chart', async () => {
    render(<CellAgeVsUsedCapacityPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Cell Age vs Used Capacity')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId('stacked-area-chart')).toBeInTheDocument();
    });
  });
});
