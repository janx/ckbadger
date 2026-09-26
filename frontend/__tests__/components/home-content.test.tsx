import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { HomeContent } from '@/components/home-content';
import { api } from '@/lib/api';

vi.mock('@/components/stats-cards', () => ({ SyncBanner: () => null }));
vi.mock('@/components/ckbytes-card', () => ({ CKBytesCard: () => null }));
vi.mock('@/components/home-charts', () => ({
  HomeCharts: () => <div data-testid="home-charts" />,
}));
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
vi.mock('@/components/latest-blocks', () => ({
  LatestBlocks: () => <div data-testid="latest-blocks" />,
}));
vi.mock('@/components/latest-transactions', () => ({ LatestTransactions: () => null }));
vi.mock('@/hooks/useRealtimeStore', () => ({
  useRealtimeData: () => ({ isConnected: false }),
}));

function renderHome(): HTMLElement {
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
  return screen.getByRole('main');
}

// Any class that sizes an element in viewport units (w-screen, 100vw, -ml-[50vw], ...) is wider
// than the page once a vertical scrollbar is present and produces a horizontal scrollbar.
function viewportSizedClasses(root: HTMLElement): string[] {
  return [root, ...Array.from(root.querySelectorAll<HTMLElement>('*'))]
    .flatMap((el) => Array.from(el.classList))
    .filter((cls) => cls.includes('w-screen') || cls.includes('vw'));
}

describe('HomeContent layout', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('keeps the constrained rows inside the site-wide container used by the header and footer', () => {
    const main = renderHome();

    for (const testId of ['home-charts', 'latest-blocks']) {
      const wrapper = screen.getByTestId(testId).closest('.container');
      expect(wrapper).not.toBeNull();
      expect(main).toContainElement(wrapper as HTMLElement);
      expect(wrapper).toHaveClass('mx-auto');
      expect(wrapper).toHaveClass('px-4');
    }
  });

  it('renders the transaction pipeline as a full-width sibling of the container rows', () => {
    const main = renderHome();
    const pipeline = screen.getByTestId('pipeline-preview');

    expect(main).toContainElement(pipeline);
    expect(pipeline.closest('.container')).toBeNull();
    expect(viewportSizedClasses(main)).toEqual([]);
  });
});
