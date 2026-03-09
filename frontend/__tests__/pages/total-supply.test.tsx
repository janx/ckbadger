import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TotalSupplyPage from '@/app/charts/total-supply/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getTotalSupplyChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: () => <div data-testid="stacked-area-chart" />,
}));

describe('TotalSupplyPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getTotalSupplyChart).mockResolvedValue({
      title: 'Total Supply',
      data: [{ date: '2026-02-23', values: { primary: '70', secondary: '30' } }],
      series: [
        { key: 'primary', label: 'Primary Issuance', color: '#00c389' },
        { key: 'secondary', label: 'Secondary Issuance', color: '#ffb000' },
      ],
    });
  });

  it('renders stacked chart and legend labels', async () => {
    render(<TotalSupplyPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Total Supply')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId('stacked-area-chart')).toBeInTheDocument();
      expect(screen.getByText('Primary Issuance')).toBeInTheDocument();
      expect(screen.getByText('Secondary Issuance')).toBeInTheDocument();
      expect(screen.getByText(/Drag to select range/i)).toHaveClass('text-text-muted');
    });
  });
});
