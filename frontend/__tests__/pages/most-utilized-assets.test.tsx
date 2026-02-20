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

describe('MostUtilizedAssetsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getMostUtilizedAssetsChart).mockResolvedValue({
      title: 'Most Utilized Assets',
      byOccupied: [
        {
          id: '0x' + '22'.repeat(32),
          assetType: 'token',
          standard: 'xudt',
          name: 'Token A',
          symbol: 'TA',
          occupiedCapacity: '12000000000',
          totalCellsCapacity: '20000000000',
        },
      ],
      byTotalCellsCapacity: [
        {
          id: '0x' + '33'.repeat(32),
          assetType: 'dob',
          standard: 'spore',
          name: 'Cluster A',
          symbol: null,
          occupiedCapacity: '9000000000',
          totalCellsCapacity: '25000000000',
        },
      ],
    });
  });

  it('renders both ranking tables', async () => {
    render(<MostUtilizedAssetsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Most Utilized Assets')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('Top 20 by Occupied CKB')).toBeInTheDocument();
      expect(screen.getByText('Top 20 by Total Cells Capacity')).toBeInTheDocument();
      expect(screen.getByText('TA')).toBeInTheDocument();
      expect(screen.getByText('Cluster A')).toBeInTheDocument();
    });
  });
});
