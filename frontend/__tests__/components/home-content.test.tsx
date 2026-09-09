import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { HomeContent } from '@/components/home-content';
import { api } from '@/lib/api';

vi.mock('@/components/stats-cards', () => ({ SyncBanner: () => null }));
vi.mock('@/components/ckbytes-card', () => ({ CKBytesCard: () => null }));
vi.mock('@/components/home-charts', () => ({ HomeCharts: () => null }));
vi.mock('@/components/mini-stats-cards', () => ({ MiniStatsCards: () => null }));
vi.mock('@/components/chain-wave/epoch-progress', () => ({ EpochProgress: () => null }));
vi.mock('@/components/chain-wave/pipeline-preview', () => ({
  PipelinePreview: () => <div data-testid="pipeline-preview" />,
}));
vi.mock('@/components/dao-overview', () => ({ DaoOverview: () => null }));
vi.mock('@/components/home-layer2', () => ({ KnowledgeSizeTrend: () => null }));
vi.mock('@/components/latest-activities', () => ({ LatestActivities: () => null }));
vi.mock('@/components/activity-card', () => ({
  ActivityCard: () => null,
  ActivityBarChartCard: () => null,
}));
vi.mock('@/components/latest-blocks', () => ({ LatestBlocks: () => null }));
vi.mock('@/components/latest-transactions', () => ({ LatestTransactions: () => null }));
vi.mock('@/hooks/useRealtimeStore', () => ({
  useRealtimeData: () => ({ isConnected: false }),
}));

describe('HomeContent layout', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('gives the transaction pipeline a definite full-page grid track', () => {
    vi.spyOn(api, 'getNetworkStats').mockImplementation(() => new Promise(() => {}));

    render(
      <HomeContent
        initialData={{
          stats: null,
          blocks: [],
          transactions: [],
          blockTimeChart: null,
          hashRateChart: null,
        }}
      />
    );

    const pageGrid = screen.getByTestId('home-page-grid');
    expect(pageGrid).toHaveClass(
      'grid',
      'grid-cols-[minmax(0,1fr)_minmax(0,1280px)_minmax(0,1fr)]'
    );
    expect(pageGrid).not.toHaveClass('container', 'w-screen');

    const pipelineRow = screen.getByTestId('home-pipeline-row');
    expect(pipelineRow.parentElement).toBe(pageGrid);
    expect(pipelineRow).toHaveClass('col-span-full', 'min-w-0');
    expect(screen.getByTestId('pipeline-preview')).toBeInTheDocument();
  });
});
