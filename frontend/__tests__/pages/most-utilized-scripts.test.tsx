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

describe('MostUtilizedScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Most Utilized Scripts',
      byOccupied: [
        {
          name: 'SECP256K1_BLAKE160',
          codeHash: null,
          isKnownScript: true,
          scriptKind: 'lock',
          occupiedCapacity: '10000000000',
          totalCellsCapacity: '20000000000',
        },
      ],
      byTotalCellsCapacity: [
        {
          name: '0x' + '11'.repeat(32),
          codeHash: '0x' + '11'.repeat(32),
          isKnownScript: false,
          scriptKind: 'type',
          occupiedCapacity: '9000000000',
          totalCellsCapacity: '30000000000',
        },
      ],
    });
  });

  it('renders both ranking tables', async () => {
    render(<MostUtilizedScriptsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Most Utilized Scripts')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('Top 20 by Occupied CKB')).toBeInTheDocument();
      expect(screen.getByText('Top 20 by Total Cells Capacity')).toBeInTheDocument();
      expect(screen.getByText('SECP256K1_BLAKE160')).toBeInTheDocument();
    });
  });
});
