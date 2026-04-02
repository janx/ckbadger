import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import MostUtilizedAssetsPage from '@/app/charts/most-utilized-assets/page';
import { api } from '@/lib/api';

const stackedAreaChartMock = vi.fn((_: unknown) => <div data-testid="stacked-area-chart" />);

vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedAssetsChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: (props: unknown) => stackedAreaChartMock(props),
}));

describe('MostUtilizedAssetsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getMostUtilizedAssetsChart).mockResolvedValue({
      title: 'Assets Used & Total CKBytes',
      usedShare: {
        title: 'Top Assets Common Knowledge Share',
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
          { key: 'top0', label: 'Cluster A (object)', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      },
    });
  });

  it('renders occupied/capacity stacked charts', async () => {
    render(<MostUtilizedAssetsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Assets Used & Total CKBytes')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Back to Charts/i })).toHaveAttribute(
      'href',
      '/charts'
    );

    await waitFor(() => {
      expect(api.getMostUtilizedAssetsChart).toHaveBeenCalledTimes(1);
      expect(screen.getByText('Used CKBytes Share (%) - Top 20 + Others')).toBeInTheDocument();
      expect(screen.getByText('Total CKBytes Share (%) - Top 20 + Others')).toBeInTheDocument();
      expect(screen.getAllByTestId('stacked-area-chart')).toHaveLength(2);
      expect(screen.getByTitle('Token A (token)')).toBeInTheDocument();
      expect(screen.getByTitle('Cluster A (object)')).toBeInTheDocument();
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
          { key: 'top0', label: 'Token A (token)', color: '#00c389' },
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
          { key: 'top0', label: 'Cluster A (object)', color: '#00c389' },
          { key: 'others', label: 'Others', color: '#64748b' },
        ],
      })
    );
  });
});
