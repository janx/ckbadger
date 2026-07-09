import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import SecondaryIssuancePage from '@/app/charts/secondary-issuance/page';
import { api } from '@/lib/api';

const stackedAreaChartMock = vi.fn((_: unknown) => <div data-testid="stacked-area-chart" />);

vi.mock('@/lib/api', () => ({
  api: {
    getSecondaryIssuanceChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: (props: unknown) => stackedAreaChartMock(props),
}));

describe('SecondaryIssuancePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getSecondaryIssuanceChart).mockResolvedValue({
      title: 'Secondary Issuance',
      data: [{ date: '2026-02-23', values: { mining: '60', dao: '30', burnt: '10' } }],
      series: [
        { key: 'mining', label: 'Mining Reward', color: '#00c389' },
        { key: 'dao', label: 'Deposit Compensation', color: '#ffb000' },
      ],
    });
  });

  it('renders stacked chart and guidance copy', async () => {
    render(<SecondaryIssuancePage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Secondary Issuance')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Back to Charts/i })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );

    await waitFor(() => {
      expect(screen.getByTestId('stacked-area-chart')).toBeInTheDocument();
      expect(screen.getByText('Mining Reward')).toBeInTheDocument();
      expect(screen.getByText('Deposit Compensation')).toBeInTheDocument();
      expect(screen.getByText(/Drag to select range/i)).toBeInTheDocument();
    });

    expect(stackedAreaChartMock).toHaveBeenCalledWith(
      expect.objectContaining({
        height: 400,
        isPercentage: true,
      })
    );
  });
});
