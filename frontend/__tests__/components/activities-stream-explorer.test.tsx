import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { api } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';

vi.mock('@/lib/api', () => ({
  api: {
    getGlobalActivities: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
  TAG_TOKEN: 1,
  TAG_OBJECT: 2,
  TAG_IDENTITY: 4,
  TAG_DAO: 8,
  TAG_PROTOCOL: 16,
  TAG_CELLBASE: 32,
}));

function makeParticipant(overrides: Partial<ParticipantInfo> = {}): ParticipantInfo {
  return {
    address: overrides.address ?? 'ckb1qtest',
    ckbDelta: overrides.ckbDelta ?? '0',
    usedDelta: overrides.usedDelta ?? '0',
    itemDeltas: overrides.itemDeltas ?? [],
    tags: overrides.tags ?? 0,
  };
}

function makeActivity(
  overrides: Partial<GlobalActivity> & { participants: ParticipantInfo[] }
): GlobalActivity {
  return {
    txHash: overrides.txHash ?? '0xtx',
    blockNumber: overrides.blockNumber ?? 10_000,
    txIndex: overrides.txIndex ?? 0,
    timestamp: overrides.timestamp ?? '1700000000000',
    isCellbase: overrides.isCellbase ?? false,
    typeCalls: overrides.typeCalls ?? [],
    lockCalls: overrides.lockCalls ?? [],
    protocolActions: overrides.protocolActions ?? [],
    participants: overrides.participants,
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
            txHash: '0xall-head',
            participants: [
              makeParticipant({
                address: 'ckb1qall1111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
            txHash: '0xall-head',
            participants: [
              makeParticipant({
                address: 'ckb1qall1111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
            txHash: '0xall-older',
            participants: [
              makeParticipant({
                address: 'ckb1qall2222222222222222222222222222222222222222222',
                ckbDelta: '200000000',
              }),
            ],
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
            txHash: '0xtoken-only',
            participants: [
              makeParticipant({
                address: 'ckb1qtoken111111111111111111111111111111111111111111',
                itemDeltas: [
                  {
                    kind: 'token',
                    typeScriptHash: '0xtoken',
                    delta: '4200',
                    symbol: 'TKN',
                    decimals: 2,
                  },
                ],
                tags: 1,
              }),
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
            txHash: '0xtoken-only',
            participants: [
              makeParticipant({
                address: 'ckb1qtoken111111111111111111111111111111111111111111',
                itemDeltas: [
                  {
                    kind: 'token',
                    typeScriptHash: '0xtoken',
                    delta: '4200',
                    symbol: 'TKN',
                    decimals: 2,
                  },
                ],
                tags: 1,
              }),
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
        expect.objectContaining({
          cursor: '500:0:0',
          filter: 'all',
          limit: DEFAULT_PAGE_SIZE,
        })
      );
    });
    expect(await screen.findByText('+2.00000000 CKB')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Token' }));

    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenCalledWith(
        expect.objectContaining({ filter: 'token', limit: DEFAULT_PAGE_SIZE })
      );
    });
    // Token symbol shown inline on participant line
    expect(await screen.findByText(/TKN/)).toBeInTheDocument();
  });

  it('renders an empty state when the selected filter has no activities', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });

    render(<ActivitiesStreamExplorer />);

    const toolbar = screen.getByTestId('activities-stream-toolbar');
    const stickyStack = screen.getByTestId('activities-stream-sticky-stack');
    const panel = screen.getByTestId('activities-stream-panel');
    const allFilter = within(toolbar).getByRole('button', { name: 'All' });
    const ckbFilter = within(toolbar).getByRole('button', { name: 'CKB' });

    expect(panel.contains(toolbar)).toBe(true);
    expect(panel.contains(stickyStack)).toBe(true);
    expect(toolbar.textContent).toContain('filter');
    expect(toolbar.textContent).toContain('ALL');
    expect(within(toolbar).queryByText('STREAM CTRL')).not.toBeInTheDocument();
    expect(stickyStack.className).toContain('sticky');
    expect(stickyStack.className).toContain('top-[5.25rem]');
    expect(stickyStack.className).toContain('z-30');
    expect(stickyStack.className).toContain('border-x');
    expect(toolbar.className).toContain('bg-[#060810]');
    expect(toolbar.className).toContain('border-y');
    expect(toolbar.className).not.toContain('border-x');
    expect(allFilter.className).toContain('bg-jade/8');
    expect(allFilter.className).toContain('border-jade/20');
    expect(ckbFilter.className).toContain('border-transparent');
    expect(ckbFilter.className).toContain('hover:bg-jade/[0.04]');
    expect(panel.className).toContain('overflow-visible');
    expect(screen.queryByText('Global Activity Stream')).not.toBeInTheDocument();
    expect(await screen.findByText('No activities yet')).toBeInTheDocument();
    expect(
      screen.getByText('This filter has no canonical activity rows in the current window.')
    ).toBeInTheDocument();
  });

  it('renders each activity row with tx hash, event rows, and participant lines', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        makeActivity({
          txHash: '0xscriptcall0000000000000000000000000000000000000000000000000001',
          blockNumber: 123_456,
          timestamp: String(Date.now() - 23_000),
          typeCalls: [
            {
              scriptHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
              scriptName: '.bit Time Index State',
              typeCodeHash: '0x2222222222222222222222222222222222222222222222222222222222222222',
              typeHashType: 'type',
              typeArgs: '0x00',
            },
          ],
          participants: [
            makeParticipant({
              address: 'ckb1qzdaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaq3q7ue',
              ckbDelta: '-30000',
            }),
          ],
        }),
      ],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });

    render(<ActivitiesStreamExplorer />);

    const row = await screen.findByRole('article');
    const divider = screen.getByTestId('activity-day-divider-today');
    const dividerDot = divider.querySelector('span');

    expect(screen.getByText('Today')).toBeInTheDocument();
    // TX hash shown prominently
    expect(within(row).getByText(/0xscriptca/)).toBeInTheDocument();
    // Block number shown
    expect(within(row).getByText('#123,456')).toBeInTheDocument();
    // Time shown
    expect(within(row).getByText(/ago$/i)).toBeInTheDocument();
    // Script call event row (badge includes icon prefix)
    expect(within(row).getByText(/Script Call \(type\)/)).toBeInTheDocument();
    expect(within(row).getByText('.bit Time Index State')).toBeInTheDocument();
    // Participant line with address
    expect(
      within(row)
        .getAllByRole('link')
        .some((link) => link.getAttribute('href')?.startsWith('/mainnet/address/'))
    ).toBe(true);
    expect(row.className).toContain('py-4');
    expect(row.className).not.toContain('grid-cols-[0.625rem_minmax(0,1fr)]');
    expect(within(row).queryByTestId('activity-terminal-marker')).not.toBeInTheDocument();
    expect(divider.className).toContain('gap-2');
    expect(dividerDot?.className).toContain('h-1');
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
            txHash: '0xfresh-head',
            participants: [
              makeParticipant({
                address: 'ckb1qfresh111111111111111111111111111111111111111111',
                ckbDelta: '300000000',
              }),
            ],
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
            txHash: '0xolder-head',
            participants: [
              makeParticipant({
                address: 'ckb1qold11111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
            txHash: '0xnew-head',
            participants: [
              makeParticipant({
                address: 'ckb1qnew11111111111111111111111111111111111111111111',
                ckbDelta: '400000000',
              }),
            ],
            blockNumber: 901,
          }),
          makeActivity({
            txHash: '0xolder-head',
            participants: [
              makeParticipant({
                address: 'ckb1qold11111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
    const toolbar = screen.getByTestId('activities-stream-toolbar');
    const stickyStack = screen.getByTestId('activities-stream-sticky-stack');
    const panel = screen.getByTestId('activities-stream-panel');
    expect(stickyStack.className).toContain('sticky');
    expect(stickyStack.className).toContain('top-[5.25rem]');
    expect(stickyStack.className).toContain('z-30');
    expect(stickyStack.className).toContain('border-x');
    expect(stickyStack.className).toContain('shadow-[');
    expect(banner.className).toContain('bg-[#04070d]');
    expect(banner.className).not.toContain('bg-[#060810]');
    expect(banner.className).not.toContain('/92');
    expect(banner.className).toContain('rounded-none');
    expect(banner.className).not.toContain('border-x');
    expect(banner.className).toContain('border-y');
    expect(banner.className).not.toContain('shadow-[');
    expect(panel.contains(banner)).toBe(true);
    expect(stickyStack.contains(banner)).toBe(true);
    expect(stickyStack.contains(toolbar)).toBe(true);
    expect(toolbar.className).toContain('border-b');
    expect(toolbar.className).not.toContain('border-y');
    expect(toolbar.className).not.toContain('border-x');
    expect(banner.textContent).toContain('|');
    expect(banner.textContent).toContain('LIVE BUFFER');
    expect(banner.textContent).toContain('1 new activity');
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
        txHash: `0xhead-page-one-${index}`,
        participants: [
          makeParticipant({
            address: `ckb1qhead${String(index).padStart(2, '0')}111111111111111111111111111111111111`,
            ckbDelta: `${(index + 2) * 100000000}`,
          }),
        ],
        blockNumber: 1_000 - index,
      })
    );

    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            txHash: '0xanchor-head',
            participants: [
              makeParticipant({
                address: 'ckb1qanchor111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
            txHash: `0xhead-page-two-${DEFAULT_PAGE_SIZE}`,
            participants: [
              makeParticipant({
                address: `ckb1qhead${String(DEFAULT_PAGE_SIZE).padStart(2, '0')}111111111111111111111111111111111111`,
                ckbDelta: '2200000000',
              }),
            ],
            blockNumber: 980,
          }),
          makeActivity({
            txHash: '0xanchor-head',
            participants: [
              makeParticipant({
                address: 'ckb1qanchor111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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

    await waitFor(() => {
      expect(screen.getByTestId('activities-stream-toolbar').textContent).toContain(
        `${DEFAULT_PAGE_SIZE + 2}`
      );
    });
    expect(screen.getAllByText('#980').length).toBeGreaterThan(0);
  });

  it('clears the row highlight timeout on unmount', async () => {
    Object.defineProperty(window, 'scrollY', {
      value: 500,
      writable: true,
      configurable: true,
    });

    const setTimeoutSpy = vi.spyOn(window, 'setTimeout');
    const clearTimeoutSpy = vi.spyOn(window, 'clearTimeout');

    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            txHash: '0xolder-head',
            participants: [
              makeParticipant({
                address: 'ckb1qold11111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
            txHash: '0xnew-head',
            participants: [
              makeParticipant({
                address: 'ckb1qnew11111111111111111111111111111111111111111111',
                ckbDelta: '400000000',
              }),
            ],
            blockNumber: 901,
          }),
          makeActivity({
            txHash: '0xolder-head',
            participants: [
              makeParticipant({
                address: 'ckb1qold11111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
            blockNumber: 900,
          }),
        ],
        limit: DEFAULT_PAGE_SIZE,
        hasMore: false,
        nextCursor: null,
      });

    const { unmount } = render(<ActivitiesStreamExplorer />);

    expect(await screen.findByText('+1.00000000 CKB')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: '1 new activity' }));

    const highlightTimerIndex = setTimeoutSpy.mock.calls.findIndex(([, delay]) => delay === 2_000);
    expect(highlightTimerIndex).toBeGreaterThanOrEqual(0);

    const highlightTimerHandle = setTimeoutSpy.mock.results[highlightTimerIndex]?.value;
    expect(highlightTimerHandle).toBeDefined();

    unmount();

    expect(clearTimeoutSpy).toHaveBeenCalledWith(highlightTimerHandle);
  });

  it('shows a soft head refresh warning without clearing visible rows', async () => {
    vi.mocked(api.getGlobalActivities)
      .mockResolvedValueOnce({
        data: [
          makeActivity({
            txHash: '0xstable-head',
            participants: [
              makeParticipant({
                address: 'ckb1qstable111111111111111111111111111111111111111111',
                ckbDelta: '100000000',
              }),
            ],
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
