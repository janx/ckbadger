import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { api } from '@/lib/api';
import { dailyActivities } from '@/__tests__/msw/fixtures/home';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';

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
    lockHash: overrides.lockHash ?? '0xaa',
    lockHashPrefix: overrides.lockHashPrefix ?? null,
    roles: overrides.roles ?? [],
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

function exactAmount(value: string) {
  return (_text: string, element: Element | null) =>
    element?.hasAttribute('data-ckb-amount') === true && element.textContent === value;
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

  it('shows exact completed-day counts, including zero, and leaves unavailable protocol count unnumbered', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      hasMore: false,
      nextCursor: null,
      total: 0,
      limit: 20,
    });
    render(
      <ActivitiesStreamExplorer
        totalCount={100}
        summary={{
          ...dailyActivities[0],
          transferCount: 12,
          daoDepositCount: 1,
          daoWithdrawRequestCount: 2,
          daoWithdrawCompleteCount: 3,
          tokenCount: 0,
          objectCount: 4,
          identityCount: 5,
          scriptCallCount: 6,
        }}
      />
    );
    expect(screen.getByRole('button', { name: 'All' })).toHaveTextContent('All100');
    expect(screen.getByRole('button', { name: 'Tokens' })).toHaveTextContent('Tokens0');
    expect(screen.getByRole('button', { name: 'Nervos DAO' })).toHaveTextContent('Nervos DAO6');
    expect(screen.getByText(/DAO counts actions/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Protocols' })).toHaveTextContent(/^Protocols$/);
    expect(
      await screen.findByText('Nothing of this kind in the latest activities.')
    ).toBeInTheDocument();
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

    expect(await screen.findByText(exactAmount('+1.00000000 CKB'))).toBeInTheDocument();
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
    expect(await screen.findByText(exactAmount('+2.00000000 CKB'))).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));

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

    expect(
      await screen.findByText('Nothing of this kind in the latest activities.')
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

    expect(screen.getByText('Today')).toBeInTheDocument();
    // TX hash shown prominently
    expect(within(row).getByText(/0xscriptca/)).toBeInTheDocument();
    // Block number shown
    expect(within(row).getByText('#123,456')).toBeInTheDocument();
    // Time shown
    expect(within(row).getByText(/ago$/i)).toBeInTheDocument();
    // Script call event row (badge includes icon prefix)
    expect(within(row).getByText('Script call')).toBeInTheDocument();
    expect(within(row).getByText('.bit Time Index State')).toBeInTheDocument();
    // Participant line with address
    expect(
      within(row)
        .getAllByRole('link')
        .some((link) => link.getAttribute('href')?.startsWith('/mainnet/address/'))
    ).toBe(true);
  });

  it('gives each participant lock kind its own column instead of wrapping it under the address', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        makeActivity({
          txHash: '0xlockkind00000000000000000000000000000000000000000000000000000001',
          participants: [
            {
              ...makeParticipant({
                address: 'ckb1qzdaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaq3q7ue',
                ckbDelta: '-30000',
              }),
              lockKind: 'JoyID',
            },
          ],
        }),
      ],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });

    render(<ActivitiesStreamExplorer />);

    const row = await screen.findByRole('article');
    const kinds = row.querySelectorAll('[data-role="lock-kind"]');
    expect(kinds).toHaveLength(1);
    expect(kinds[0]).toHaveTextContent('JoyID');
    // The party cell holds only the address: the kind is not repeated inline under it.
    const party = row.querySelector('[data-party]');
    expect(party).not.toBeNull();
    expect(party).not.toHaveTextContent('JoyID');
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

    expect(await screen.findByText(exactAmount('+3.00000000 CKB'))).toBeInTheDocument();
    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenNthCalledWith(2, {
        filter: 'all',
        limit: DEFAULT_PAGE_SIZE,
      });
    });
    expect(
      screen.queryByText('Nothing of this kind in the latest activities.')
    ).not.toBeInTheDocument();
  });

  it('shows an unreadable DAO step as an error in its row, naming the tx, and keeps the others', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        makeActivity({
          txHash: '0xbad0000000000000000000000000000000000000000000000000000000000001',
          protocolActions: [
            {
              protocol: 'dao',
              protocolName: 'Nervos DAO',
              action: 'withdraw_complete',
              metadata: { capacity: '1' },
            },
          ],
          participants: [makeParticipant({ ckbDelta: '100000000' })],
        }),
        makeActivity({
          txHash: '0x9000000000000000000000000000000000000000000000000000000000000002',
          protocolActions: [
            {
              protocol: 'dao',
              protocolName: 'Nervos DAO',
              action: 'deposit',
              metadata: { capacity: '10200000000' },
            },
          ],
          participants: [makeParticipant({ ckbDelta: '-10200000000' })],
        }),
      ],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });
    render(<ActivitiesStreamExplorer />);
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(
      'Cannot read tx 0xbad0000000000000000000000000000000000000000000000000000000000001: dao:withdraw_complete metadata.compensation is missing'
    );
    // The other row still reads.
    expect(screen.getByText(/DAO Deposit/)).toBeInTheDocument();
    expect(screen.getAllByRole('article')[1]).toHaveTextContent('102.00000000 CKB locked');
  });

  it('sticks its toolbar right below the site header, at the height the header publishes', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });
    render(<ActivitiesStreamExplorer />);
    const stack = await screen.findByTestId('activities-stream-sticky-stack');
    expect(stack).toHaveClass('sticky', BELOW_STICKY_HEADER);
    // Not the old 56 px row + 28 px stats bar.
    expect(stack.className).not.toContain('5.25rem');
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

    expect(await screen.findByText(exactAmount('+1.00000000 CKB'))).toBeInTheDocument();

    const banner = await screen.findByRole('button', { name: '1 new activity' });
    expect(screen.queryByText(exactAmount('+4.00000000 CKB'))).not.toBeInTheDocument();

    fireEvent.click(banner);

    expect(screen.getByText(exactAmount('+4.00000000 CKB'))).toBeInTheDocument();
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

    expect(await screen.findByText(exactAmount('+1.00000000 CKB'))).toBeInTheDocument();
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
      // Every buffered row is shown once merged (the toolbar no longer counts rows, TX-30).
      expect(document.querySelectorAll('[data-feed-row]')).toHaveLength(DEFAULT_PAGE_SIZE + 2);
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

    expect(await screen.findByText(exactAmount('+1.00000000 CKB'))).toBeInTheDocument();
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

    expect(await screen.findByText(exactAmount('+1.00000000 CKB'))).toBeInTheDocument();
    expect(await screen.findByText('Could not refresh: refresh failed')).toBeInTheDocument();
  });
});
