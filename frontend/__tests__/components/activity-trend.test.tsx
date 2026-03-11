import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { ActivityTrend } from '@/components/activity-trend';
import { api, type DailyActivityStats, type ActivitySummary24h } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getDailyActivityStats: vi.fn(),
    getActivitySummary24h: vi.fn(),
  },
}));

function mockDailyStats(): DailyActivityStats[] {
  return Array.from({ length: 14 }, (_, i) => ({
    date: `2026-02-${String(i + 1).padStart(2, '0')}`,
    transferCount: 1000 + i * 100,
    daoDepositCount: 50 + i * 5,
    daoWithdrawRequestCount: 20 + i * 2,
    daoWithdrawCompleteCount: 10 + i,
    tokenCount: 80 + i * 10,
    objectCount: 12 + i,
    identityCount: 5 + i,
    scriptCallCount: 30 + i * 3,
    unknownCount: 0,
    coinbaseCount: 0,
    uniqueAddressCount: 300 + i * 20,
    totalCkbMoved: String(BigInt(50000000000000) + BigInt(i) * BigInt(1000000000000)),
    scriptCounts: [],
  }));
}

function mockSummary24h(): ActivitySummary24h {
  return {
    transferCount: 1200,
    daoDepositCount: 200,
    daoWithdrawRequestCount: 80,
    daoWithdrawCompleteCount: 60,
    tokenCount: 89,
    objectCount: 12,
    identityCount: 3,
    scriptCallCount: 45,
    unknownCount: 0,
    coinbaseCount: 0,
    uniqueAddressCount: 450,
    totalCkbMoved: '120000000000000000',
    scriptCounts: [],
    hoursCovered: 24,
  };
}

describe('ActivityTrend', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders activity summary stats after loading', async () => {
    vi.mocked(api.getDailyActivityStats).mockResolvedValue(mockDailyStats());
    vi.mocked(api.getActivitySummary24h).mockResolvedValue(mockSummary24h());

    render(<ActivityTrend />);

    await waitFor(() => {
      expect(screen.getByText('450')).toBeInTheDocument();
    });

    // Unique addresses label
    expect(screen.getByText('Unique Addr (24h)')).toBeInTheDocument();

    // CKB moved label
    expect(screen.getByText('CKB Moved (24h)')).toBeInTheDocument();

    // CKB moved value: 120000000000000000 shannons = 1,200,000,000 CKB = 1.2B
    expect(screen.getByText('1.2B')).toBeInTheDocument();
  });

  it('renders type breakdown text from 24h summary', async () => {
    vi.mocked(api.getDailyActivityStats).mockResolvedValue(mockDailyStats());
    vi.mocked(api.getActivitySummary24h).mockResolvedValue(mockSummary24h());

    render(<ActivityTrend />);

    await waitFor(() => {
      expect(screen.getByText(/Transfers:/)).toBeInTheDocument();
    });

    // DAO total = 200 + 80 + 60 = 340
    expect(screen.getByText(/DAO:/)).toBeInTheDocument();
    expect(screen.getByText(/Tokens:/)).toBeInTheDocument();
    expect(screen.getByText(/Objects:/)).toBeInTheDocument();
  });

  it('renders bar chart with 14 bars', async () => {
    vi.mocked(api.getDailyActivityStats).mockResolvedValue(mockDailyStats());
    vi.mocked(api.getActivitySummary24h).mockResolvedValue(mockSummary24h());

    const { container } = render(<ActivityTrend />);

    await waitFor(() => {
      expect(screen.getByText('450')).toBeInTheDocument();
    });

    // Each bar has a title attribute with the date and activity count
    const bars = container.querySelectorAll('[title*="activities"]');
    expect(bars).toHaveLength(14);
  });

  it('renders header with link to /charts', async () => {
    vi.mocked(api.getDailyActivityStats).mockResolvedValue(mockDailyStats());
    vi.mocked(api.getActivitySummary24h).mockResolvedValue(mockSummary24h());

    render(<ActivityTrend />);

    const headerLink = screen.getByRole('link', { name: /activity trend/i });
    expect(headerLink).toHaveAttribute('href', '/charts');
  });

  it('shows loading skeletons initially', () => {
    vi.mocked(api.getDailyActivityStats).mockReturnValue(new Promise(() => {}));
    vi.mocked(api.getActivitySummary24h).mockReturnValue(new Promise(() => {}));

    const { container } = render(<ActivityTrend />);

    const pulseElements = container.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThanOrEqual(1);
  });
});
