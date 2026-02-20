import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import PipelinePage from '@/app/pipeline/page';

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/deep-fork-alert', () => ({
  DeepForkAlert: () => <div data-testid="deep-fork-alert">DeepForkAlert</div>,
}));

vi.mock('@/components/pipeline/pipeline-dashboard', () => ({
  PipelineDashboard: ({ initialBlocks }: { initialBlocks?: unknown[] }) => (
    <div data-testid="pipeline-dashboard">blocks:{initialBlocks?.length ?? 0}</div>
  ),
}));

describe('PipelinePage', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);

        if (url.includes('/statistics/network')) {
          return {
            ok: true,
            json: async () => ({
              latestBlock: 100,
              avgBlockTime: '8.5',
              hashRate: '1',
              difficulty: '1',
              epoch: '1(1/1000)',
              tps: '2',
              estimatedEpochTime: '1h',
              transactionsPerMinute: '10',
              transactionsPerDay: '100',
              syncStatus: {
                isSyncing: false,
                syncedBlock: 100,
                tipBlock: 100,
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
            }),
          } as Response;
        }

        if (url.includes('/blocks?limit=4')) {
          return {
            ok: true,
            json: async () => ({
              data: [{ number: 100 }, { number: 99 }],
              total: 2,
              limit: 4,
              hasMore: false,
              nextCursor: null,
            }),
          } as Response;
        }

        return {
          ok: false,
          json: async () => ({}),
        } as Response;
      })
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders pipeline page and passes initial blocks to PipelineDashboard', async () => {
    const page = await PipelinePage();
    render(page);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Pipeline')).toBeInTheDocument();
    expect(screen.getByTestId('deep-fork-alert')).toBeInTheDocument();
    expect(screen.getByTestId('pipeline-dashboard')).toHaveTextContent('blocks:2');
  });
});
