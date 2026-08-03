import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import MostUtilizedScriptsPage from '@/app/charts/most-utilized-scripts/page';
import { api } from '@/lib/api';

const stackedAreaChartMock = vi.fn((_: unknown) => <div data-testid="stacked-area-chart" />);

vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedScriptsChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: (props: unknown) => stackedAreaChartMock(props),
}));

describe('MostUtilizedScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts Used & Total CKBytes',
      usedShare: {
        title: 'Top Scripts Common Knowledge Share',
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
    expect(screen.getByRole('link', { name: /Back to Charts/i })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );

    await waitFor(() => {
      expect(api.getMostUtilizedScriptsChart).toHaveBeenCalledTimes(1);
      expect(screen.getByText('Used Share (%) - Top 20 + Others')).toBeInTheDocument();
      expect(
        screen.getByText('Total Cells Capacity Share (%) - Top 20 + Others')
      ).toBeInTheDocument();
      expect(screen.getAllByTestId('stacked-area-chart')).toHaveLength(2);
      expect(screen.getAllByTitle('SECP256K1_BLAKE160')).toHaveLength(2);
      expect(screen.getByText(/Drag to select range/i)).toBeInTheDocument();
    });

    expect(stackedAreaChartMock).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        height: 360,
        isPercentage: true,
        valueUnit: 'shannon',
        data: [{ date: '2024-01-01', values: { top0: '100', others: '20' } }],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      })
    );
    expect(stackedAreaChartMock).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        height: 360,
        isPercentage: true,
        valueUnit: 'shannon',
        data: [{ date: '2024-01-01', values: { top0: '200', others: '30' } }],
        series: [
          { key: 'top0', label: 'SECP256K1_BLAKE160', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      })
    );
  });
});
