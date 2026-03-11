import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getNetworkStats: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/line-chart', () => ({
  LineChart: ({ chartType }: { chartType?: string }) => (
    <div data-testid="line-chart">{chartType ?? 'line'}</div>
  ),
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

    expect(screen.getByText('bar')).toBeInTheDocument();
    expect(screen.getByText('Description')).toBeInTheDocument();
    expect(screen.getByText('Legend Item Calculation')).toBeInTheDocument();
    expect(
      screen.getByText(/Shows how live cells are distributed across size buckets/i)
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /For each bucket: count live cells whose serialized size falls within that size range/i
      )
    ).toBeInTheDocument();
  });
});
