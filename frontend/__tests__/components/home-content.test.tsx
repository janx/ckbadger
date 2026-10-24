import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { HomeContent } from '@/components/home-content';
import { api } from '@/lib/api';

const realtime = vi.hoisted(() => ({ calls: 0 }));

vi.mock('@/components/stats-cards', () => ({
  SyncBanner: () => <div data-testid="sync-banner" />,
}));
vi.mock('@/components/pipeline/pipeline-panel', () => ({
  PipelinePanel: () => <div data-testid="pipeline-panel" />,
}));
vi.mock('@/components/home/network-section', () => ({
  NetworkSection: () => <div data-testid="network-section" />,
}));
vi.mock('@/components/home/ckbytes-section', () => ({
  CkbytesSection: () => <div data-testid="ckbytes-section" />,
}));
vi.mock('@/components/home/activity-section', () => ({
  ActivitySection: () => <div data-testid="activity-section" />,
}));
vi.mock('@/hooks/useRealtimeStore', () => ({
  useRealtimeData: () => {
    realtime.calls++;
    return { isConnected: true, latestBlock: null, latestTx: null };
  },
}));

function renderHome(stats: Awaited<ReturnType<typeof api.getNetworkStats>> | null = null) {
  vi.spyOn(api, 'getNetworkStats').mockImplementation(() =>
    stats ? Promise.resolve(stats) : new Promise(() => {})
  );
  render(<HomeContent />);
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

  it('stacks the pipeline, Network, CKBytes and Activity, in that order, 52 px apart', () => {
    const main = renderHome();
    const column = screen.getByTestId('pipeline-panel').parentElement as HTMLElement;
    expect(main).toContainElement(column);
    expect(column).toHaveClass('container', 'mx-auto', 'px-4', 'flex-col', 'gap-[52px]');
    expect([...column.children].map((c) => c.getAttribute('data-testid'))).toEqual([
      'pipeline-panel',
      'network-section',
      'ckbytes-section',
      'activity-section',
    ]);
    expect(viewportSizedClasses(main)).toEqual([]);
  });

  it('puts the sync banner first once the stats are in', async () => {
    renderHome({ syncStatus: { isSyncing: true } } as Awaited<
      ReturnType<typeof api.getNetworkStats>
    >);
    const banner = await screen.findByTestId('sync-banner');
    expect(banner.parentElement?.firstElementChild).toBe(banner);
  });

  it('keeps the realtime connection on the homepage, without a floating LIVE pill', () => {
    realtime.calls = 0;
    const main = renderHome();
    expect(realtime.calls).toBeGreaterThan(0);
    expect(main.querySelector('.fixed')).toBeNull();
    expect(screen.queryByText(/^Live$/i)).not.toBeInTheDocument();
  });
});
