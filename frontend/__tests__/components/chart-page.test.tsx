import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

const lineChartMock = vi.fn((_: unknown) => <div data-testid="line-chart" />);
const chartCalculationNoteMock = vi.fn((_: unknown) => (
  <div data-testid="chart-calculation-note" />
));

vi.mock('@/lib/api', () => ({
  api: {
    getNetworkStats: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/line-chart', () => ({
  LineChart: (props: unknown) => lineChartMock(props),
}));

vi.mock('@/components/charts/chart-calculation-note', () => ({
  ChartCalculationNote: (props: unknown) => chartCalculationNoteMock(props),
}));

describe('ChartPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      latestBlock: 10,
      avgBlockTime: '10',
      hashRate: '1',
      difficulty: '1',
      epoch: '1',
      tps: '1',
      estimatedEpochTime: '1',
      transactionsPerMinute: '1',
      transactionsPerDay: '1',
      syncStatus: {
        isSyncing: false,
        syncedBlock: 10,
        tipBlock: 10,
        progress: 100,
        estimatedTime: null,
        chartDataMayBeIncomplete: false,
        blocksPerSecond: null,
        emaBlocksPerSecond: null,
        syncMode: 'synced',
        startedAt: null,
        elapsedTime: null,
        totalTime: null,
      },
      deepForkStatus: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: null,
        chainTip: null,
        forkPoint: null,
      },
      knowledgeSize: null,
      circulatingSupply: null,
      daoLocked: null,
    });
  });

  it('renders mapped description and forwards bar chart mode', async () => {
    const queryFn = vi.fn().mockResolvedValue({
      title: 'Cell Size Distribution',
      yAxisLabel: 'Count',
      data: [{ date: '0-1KB', value: '100' }],
    });

    render(
      <ChartPage
        title="Cell Size Distribution"
        queryKey="chart-cell-size-distribution"
        queryFn={queryFn}
        chartType="bar"
      />
    );

    await waitFor(() => {
      expect(screen.getByTestId('line-chart')).toBeInTheDocument();
    });

    expect(api.getNetworkStats).toHaveBeenCalledTimes(1);
    expect(queryFn).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('link', { name: /Back to Charts/i })).toHaveAttribute(
      'href',
      '/mainnet/charts'
    );
    expect(screen.getByText('Cell Size Distribution')).toBeInTheDocument();
    expect(screen.getByText('Count')).toBeInTheDocument();
    expect(screen.getByText(/Drag to select range/i)).toBeInTheDocument();
    expect(lineChartMock).toHaveBeenCalledWith(
      expect.objectContaining({
        chartType: 'bar',
        defaultLogScale: false,
        height: 400,
        yAxisLabel: 'Count',
        y2AxisLabel: undefined,
        data: [{ date: '0-1KB', value: '100' }],
      })
    );
    expect(chartCalculationNoteMock).toHaveBeenCalledWith(
      expect.objectContaining({
        description: expect.objectContaining({
          overview: expect.stringMatching(/live cells are distributed/i),
        }),
      })
    );
  });
});
