import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import MostUtilizedScriptsPage from '@/app/charts/most-utilized-scripts/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedScriptsChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: () => <div data-testid="stacked-area-chart" />,
}));

describe('MostUtilizedScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts Used & Total CKBytes',
      usedShare: {
        title: 'Top Scripts Used Share',
        data: [{ date: '2024-01-01', values: { top0: '100', others: '20' } }],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
      capacityShare: {
        title: 'Top Scripts Capacity Share',
        data: [{ date: '2024-01-01', values: { top0: '200', others: '30' } }],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
    });
  });

  it('renders occupied/capacity stacked charts', async () => {
    render(<MostUtilizedScriptsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Scripts Used & Total CKBytes')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('Used Share (%) - Top 20 + Others')).toBeInTheDocument();
      expect(
        screen.getByText('Total Cells Capacity Share (%) - Top 20 + Others')
      ).toBeInTheDocument();
      expect(screen.getAllByTestId('stacked-area-chart')).toHaveLength(2);
      expect(screen.getByText(/Drag to select range/i)).toHaveClass('text-text-dim');
      expect(screen.getByText('Description')).toBeInTheDocument();
      expect(
        screen.getByText(
          'Ranks scripts by utilization in live state: used capacity and total cells capacity.'
        )
      ).toBeInTheDocument();
    });
  });
});
