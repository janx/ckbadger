import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { DaoOverview } from '@/components/dao-overview';
import { api, type DaoStatistics, type ChartResponse } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getDaoStatistics: vi.fn(),
    getDaoTotalDepositChart: vi.fn(),
  },
}));

function mockDaoStatistics(overrides: Partial<DaoStatistics> = {}): DaoStatistics {
  return {
    totalDeposited: '1120000000000000000',
    totalDepositedCkb: '11200000000.50',
    totalDepositors: 4521,
    activeDeposits: 1200,
    totalCompensationPaid: '50000000000000',
    totalCompensationPaidCkb: '500000.00',
    unclaimedCompensation: '30000000000000',
    unclaimedCompensationCkb: '300000.00',
    averageDepositDays: '365',
    estimatedApc: '2.45',
    miningReward: '100000000000000',
    miningRewardCkb: '1000000.00',
    depositCompensation: '50000000000000',
    depositCompensationCkb: '500000.00',
    burnt: '840000000000000000',
    burntCkb: '8400000000.00',
    ...overrides,
  };
}

function mockChartResponse(): ChartResponse {
  return {
    title: 'Total Deposit',
    yAxisLabel: 'CKB',
    data: Array.from({ length: 60 }, (_, i) => ({
      date: `2026-01-${String(i + 1).padStart(2, '0')}`,
      value: String(10000000000 + i * 100000000),
    })),
  };
}

describe('DaoOverview', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders DAO statistics after loading', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue(mockDaoStatistics());
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue(mockChartResponse());

    render(<DaoOverview />);

    await waitFor(() => {
      expect(screen.getByText('11.20B CKB')).toBeInTheDocument();
    });

    // APC value inline
    expect(screen.getByText('APC 2.45%')).toBeInTheDocument();

    // Depositors count inline
    expect(screen.getByText('4,521 depositors')).toBeInTheDocument();

    // Card title
    expect(screen.getByText('Nervos DAO')).toBeInTheDocument();
  });

  it('shows loading skeleton initially', () => {
    vi.mocked(api.getDaoStatistics).mockReturnValue(new Promise(() => {}));
    vi.mocked(api.getDaoTotalDepositChart).mockReturnValue(new Promise(() => {}));

    const { container } = render(<DaoOverview />);

    const pulseElements = container.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThanOrEqual(1);
  });

  it('renders delta bar chart when chart data is available', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue(mockDaoStatistics());
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue(mockChartResponse());

    const { container } = render(<DaoOverview />);

    await waitFor(() => {
      expect(screen.getByText('11.20B CKB')).toBeInTheDocument();
    });

    // Delta bar chart renders div bars with title attributes
    const bars = container.querySelectorAll('[title]');
    expect(bars.length).toBeGreaterThan(0);
  });

  it('formats millions correctly', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue(
      mockDaoStatistics({ totalDepositedCkb: '450000000.00' })
    );
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue(mockChartResponse());

    render(<DaoOverview />);

    await waitFor(() => {
      expect(screen.getByText('450.00M CKB')).toBeInTheDocument();
    });
  });
});
