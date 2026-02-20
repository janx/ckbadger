import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import MostUtilizedAssetsPage from '@/app/charts/most-utilized-assets/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedAssetsChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: () => <div data-testid="stacked-area-chart" />,
}));

describe('MostUtilizedAssetsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getMostUtilizedAssetsChart).mockResolvedValue({
      title: 'Assets Occupied & Total CKBytes',
      occupiedShare: {
        title: 'Top Assets Occupied Share',
        data: [{ date: '2024-01-01', values: { top0: '100', others: '20' } }],
        series: [
          { key: 'top0', label: 'Token A (token)', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
      capacityShare: {
        title: 'Top Assets Capacity Share',
        data: [{ date: '2024-01-01', values: { top0: '200', others: '30' } }],
        series: [
          { key: 'top0', label: 'Cluster A (dob)', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
    });
  });

  it('renders occupied/capacity stacked charts', async () => {
    render(<MostUtilizedAssetsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Assets Occupied & Total CKBytes')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('Occupied CKBytes Share (%) - Top 20 + Others')).toBeInTheDocument();
      expect(screen.getByText('Total CKBytes Share (%) - Top 20 + Others')).toBeInTheDocument();
      expect(screen.getAllByTestId('stacked-area-chart')).toHaveLength(2);
      expect(screen.getByText('Description')).toBeInTheDocument();
      expect(
        screen.getByText(
          'Ranks token, NFT collection, and DOB collection assets by utilization in live state.'
        )
      ).toBeInTheDocument();
    });
  });
});
