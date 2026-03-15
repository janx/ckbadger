import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import type { GlobalActivity } from '@/lib/api';
import { api } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';

vi.mock('@/lib/api', () => ({
  api: {
    getGlobalActivities: vi.fn(),
  },
}));

function makeActivity(
  overrides: Partial<GlobalActivity> & Pick<GlobalActivity, 'address' | 'txHash'>
): GlobalActivity {
  return {
    address: overrides.address,
    txHash: overrides.txHash,
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000000',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    isCellbase: overrides.isCellbase ?? false,
    hasTypeScript: overrides.hasTypeScript ?? false,
    assetChanges: overrides.assetChanges ?? [],
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    protocolActions: overrides.protocolActions ?? [],
    peers: overrides.peers ?? [],
  };
}

let intersectionCallback: IntersectionObserverCallback | null = null;

class MockIntersectionObserver implements IntersectionObserver {
  readonly root = null;
  readonly rootMargin = '';
  readonly thresholds = [];

  constructor(callback: IntersectionObserverCallback) {
    intersectionCallback = callback;
  }

  disconnect() {}
  observe() {}
  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
  unobserve() {}
}

function emitIntersection(isIntersecting: boolean) {
  act(() => {
    intersectionCallback?.(
      [
        {
          isIntersecting,
          target: document.createElement('div'),
          boundingClientRect: {} as DOMRectReadOnly,
          intersectionRatio: isIntersecting ? 1 : 0,
          intersectionRect: {} as DOMRectReadOnly,
          rootBounds: null,
          time: Date.now(),
        },
      ],
      {} as IntersectionObserver
    );
  });
}

describe('ActivitiesStreamExplorer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    intersectionCallback = null;
    vi.stubGlobal('IntersectionObserver', MockIntersectionObserver);
    Object.defineProperty(window, 'scrollY', {
      value: 0,
      writable: true,
      configurable: true,
    });
    window.scrollTo = vi.fn();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('loads older pages on intersection and resets the stream on filter change', async () => {
    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qall1111111111111111111111111111111111111111111',
            txHash: '0xall-head',
            ckbDelta: '100000000',
            blockNumber: 500,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: true,
        nextCursor: '500:0:0',
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qall1111111111111111111111111111111111111111111',
            txHash: '0xall-head',
            ckbDelta: '100000000',
            blockNumber: 500,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: true,
        nextCursor: '500:0:0',
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qall2222222222222222222222222222222222222222222',
            txHash: '0xall-older',
            ckbDelta: '200000000',
            blockNumber: 499,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qtoken111111111111111111111111111111111111111111',
            txHash: '0xtoken-only',
            assetChanges: [
              {
                type: 'token',
                typeScriptHash: '0xtoken',
                delta: '4200',
                symbol: 'TKN',
                decimals: 2,
              },
            ],
            blockNumber: 480,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qtoken111111111111111111111111111111111111111111',
            txHash: '0xtoken-only',
            assetChanges: [
              {
                type: 'token',
                typeScriptHash: '0xtoken',
                delta: '4200',
                symbol: 'TKN',
                decimals: 2,
              },
            ],
            blockNumber: 480,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      });

    render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('+1.00000000 CKB')).toBeInTheDocument();
    await waitFor(() => {
      expect(intersectionCallback).not.toBeNull();
    });

    emitIntersection(true);

    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenCalledWith(
        expect.objectContaining({ cursor: '500:0:0', filter: 'all', limit: DEFAULT_PAGE_SIZE })
      );
    });
    expect(await screen.findByText('+2.00000000 CKB')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Token' }));

    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenCalledWith(
        expect.objectContaining({ filter: 'token', limit: DEFAULT_PAGE_SIZE })
      );
    });
    expect(await screen.findByText(/TKN Transfer/)).toBeInTheDocument();
  });

  it('renders an empty state when the selected filter has no activities', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });

    render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('No activities yet')).toBeInTheDocument();
    expect(
      screen.getByText('This filter has no canonical activity rows in the current window.')
    ).toBeInTheDocument();
  });

  it('keeps polling when the current filter is empty and renders new head activity', async () => {
    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qfresh111111111111111111111111111111111111111111',
            txHash: '0xfresh-head',
            ckbDelta: '300000000',
            blockNumber: 777,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      });

    render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('+3.00000000 CKB')).toBeInTheDocument();
    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenNthCalledWith(2, {
        filter: 'all',
        limit: DEFAULT_PAGE_SIZE,
      });
    });
    expect(screen.queryByText('No activities yet')).not.toBeInTheDocument();
  });

  it('buffers new head activities behind a sticky banner when the reader is away from top', async () => {
    Object.defineProperty(window, 'scrollY', {
      value: 500,
      writable: true,
      configurable: true,
    });

    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qold11111111111111111111111111111111111111111111',
            txHash: '0xolder-head',
            ckbDelta: '100000000',
            blockNumber: 900,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qnew11111111111111111111111111111111111111111111',
            txHash: '0xnew-head',
            ckbDelta: '400000000',
            blockNumber: 901,
          }),
          makeActivity({
            address: 'ckb1qold11111111111111111111111111111111111111111111',
            txHash: '0xolder-head',
            ckbDelta: '100000000',
            blockNumber: 900,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      });

    render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('+1.00000000 CKB')).toBeInTheDocument();

    const banner = await screen.findByRole('button', { name: '1 new activity' });
    expect(banner.className).toContain('sticky');
    expect(screen.queryByText('+4.00000000 CKB')).not.toBeInTheDocument();

    fireEvent.click(banner);

    expect(screen.getByText('+4.00000000 CKB')).toBeInTheDocument();
  });

  it('drains more than one head page before buffering new activities', async () => {
    Object.defineProperty(window, 'scrollY', {
      value: 500,
      writable: true,
      configurable: true,
    });

    const headPageOne = Array.from({ length: DEFAULT_PAGE_SIZE }, (_, index) =>
      makeActivity({
        address: `ckb1qhead${String(index).padStart(2, '0')}111111111111111111111111111111111111`,
        txHash: `0xhead-page-one-${index}`,
        ckbDelta: `${(index + 2) * 100000000}`,
        blockNumber: 1_000 - index,
      })
    );

    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qanchor111111111111111111111111111111111111111111',
            txHash: '0xanchor-head',
            ckbDelta: '100000000',
            blockNumber: 900,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      })
      .mockResolvedValueOnce({
        data: headPageOne,
        limit: DEFAULT_PAGE_SIZE,
        hasMore: true,
        nextCursor: 'head-page-1',
      })
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qhead20111111111111111111111111111111111111111111',
            txHash: '0xhead-page-two-20',
            ckbDelta: '2200000000',
            blockNumber: 980,
          }),
          makeActivity({
            address: 'ckb1qanchor111111111111111111111111111111111111111111',
            txHash: '0xanchor-head',
            ckbDelta: '100000000',
            blockNumber: 900,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      });

    render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('+1.00000000 CKB')).toBeInTheDocument();
    const banner = await screen.findByRole('button', {
      name: `${DEFAULT_PAGE_SIZE + 1} new activities`,
    });

    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenNthCalledWith(3, {
        cursor: 'head-page-1',
        filter: 'all',
        limit: DEFAULT_PAGE_SIZE,
      });
    });

    fireEvent.click(banner);

    expect(await screen.findByText(`${DEFAULT_PAGE_SIZE + 2} loaded`)).toBeInTheDocument();
    expect(screen.getAllByText('#980').length).toBeGreaterThan(0);
  });

  it('shows a soft head refresh warning without clearing visible rows', async () => {
    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            address: 'ckb1qstable111111111111111111111111111111111111111111',
            txHash: '0xstable-head',
            ckbDelta: '100000000',
            blockNumber: 321,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      })
      .mockRejectedValueOnce(new Error('refresh failed'));

    render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('+1.00000000 CKB')).toBeInTheDocument();
    expect(await screen.findByText('Live refresh paused: refresh failed')).toBeInTheDocument();
  });
});
