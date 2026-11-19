import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { api } from '@/lib/api';
import { dailyActivities } from '@/__tests__/msw/fixtures/home';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatTimeAgo, formatUtc8DateTime } from '@/lib/utils';
import { BELOW_STICKY_HEADER } from '@/components/layout/sticky-header';
import { loadUnderStillClock, tick } from '@/__tests__/utils/still-clock';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';

// D11: the real navigation (setup.ts stubs it with an empty URL), so a router's location reaches
// the page; outside a router it reads window.location, as the stub's empty URL did.
vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', async (importOriginal) => ({
  // a row's error is said in its own words (readErrorText knows the API's own error class)
  ApiRequestError: (await importOriginal<typeof import('@/lib/api')>()).ApiRequestError,
  api: {
    getGlobalActivities: vi.fn(),
  },
  TAG_TOKEN: 1,
  TAG_OBJECT: 2,
  TAG_IDENTITY: 4,
  TAG_CELLBASE: 32,
}));

/**
 * A lock hash for a fixture address, one per address as on chain (B4: a feed folds consecutive
 * rows by their parties' lock hashes, so two addresses never share one; it was '0xaa' for all).
 */
const lockHashOf = (address: string) =>
  `0x${[...address]
    .map((c) => c.charCodeAt(0).toString(16))
    .join('')
    .slice(-64)
    .padStart(64, '0')}`;

function makeParticipant(overrides: Partial<ParticipantInfo> = {}): ParticipantInfo {
  const address = overrides.address ?? seededAddress('test');
  return {
    address,
    lockHash: overrides.lockHash ?? (address === null ? '0xaa' : lockHashOf(address)),
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

// A participant's CKB change, by its text. Readability R5 (re-pinned on purpose): a feed line is
// the summary form, so +1 CKB, was the column's "+1.00000000 CKB".
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
    // the clock stands still (Date only; timers stay real): a row's age and its expectation read
    // one time (the lead's addendum 3)
    vi.useFakeTimers({ toFake: ['Date'] });
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
    // Re-pinned on purpose (controls lane C2): the kind filter is the controls board's chips, a
    // radio per kind named by its lowercase words and its count (they were the aria-pressed
    // buttons "All", "Tokens", "Nervos DAO", "Protocols", named by their words alone).
    expect(screen.getByRole('radio', { name: 'all 100' })).toHaveTextContent('all 100');
    expect(screen.getByRole('radio', { name: 'tokens 0' })).toHaveTextContent('tokens 0');
    expect(screen.getByRole('radio', { name: 'nervos dao 6' })).toHaveTextContent('nervos dao 6');
    expect(screen.getByText(/DAO counts actions/)).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'protocols' })).toHaveTextContent(/^protocols$/);
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
                address: seededAddress('all11111111111111111'),
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
                address: seededAddress('all11111111111111111'),
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
                address: seededAddress('all22222222222222222'),
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
                address: seededAddress('token111111111111111'),
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
                address: seededAddress('token111111111111111'),
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

    expect(await screen.findByText(exactAmount('+1 CKB'))).toBeInTheDocument();
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
    expect(await screen.findByText(exactAmount('+2 CKB'))).toBeInTheDocument();

    // the kind filter's radio (controls lane C2; it was the button "Tokens")
    fireEvent.click(screen.getByRole('radio', { name: /^tokens/ }));

    await waitFor(() => {
      expect(api.getGlobalActivities).toHaveBeenCalledWith(
        expect.objectContaining({ filter: 'token', limit: DEFAULT_PAGE_SIZE })
      );
    });
    // Token symbol shown inline on participant line
    expect(await screen.findByText(/TKN/)).toBeInTheDocument();
  });

  // The controls lane (C2, c2-lists), written before the button under the stream became the
  // controls board's short-list form: it asks for the page after the cursor the stream served, as
  // the intersection does, and goes when the stream has no more.
  it('loads the next page from the button under the stream, and draws none at its end', async () => {
    const page = (txHash: string, ckb: string, blockNumber: number, more: boolean) => ({
      data: [
        makeActivity({
          txHash,
          participants: [
            makeParticipant({ address: seededAddress(`${txHash}111111111111111`), ckbDelta: ckb }),
          ],
          blockNumber,
        }),
      ],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: more,
      nextCursor: more ? '500:0:0' : null,
    });
    vi.mocked(api.getGlobalActivities).mockImplementation(async (params) =>
      params?.cursor === '500:0:0'
        ? page('0xolder', '200000000', 499, false)
        : page('0xhead', '100000000', 500, true)
    );
    render(<ActivitiesStreamExplorer />);
    expect(await screen.findByText(exactAmount('+1 CKB'))).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^(older|next \d+ ↓)$/i }));
    expect(await screen.findByText(exactAmount('+2 CKB'))).toBeInTheDocument();
    expect(api.getGlobalActivities).toHaveBeenCalledWith(
      expect.objectContaining({ cursor: '500:0:0', filter: 'all' })
    );
    expect(screen.queryByRole('button', { name: /^(older|next \d+ ↓)$/i })).toBeNull();
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

  // Lane E4: a day divider's "Today" / "Yesterday" is relative to the reader's day on the UTC+8
  // clock, read from the page's one clock: at midnight the dividers move on with no data change
  // (they were read once per render from the wall clock). The clock stops with the stream.
  it('moves its day dividers on at UTC+8 midnight with no data change (E4)', async () => {
    vi.useRealTimers();
    // 23:59:59 on 2026-11-14 in UTC+8, a whole second
    vi.useFakeTimers({ now: Date.parse('2026-11-14T15:59:59Z') });
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        makeActivity({
          txHash: '0xlate',
          timestamp: String(Date.parse('2026-11-14T15:59:40Z')),
          participants: [makeParticipant({ address: seededAddress('late'), ckbDelta: '-1' })],
        }),
        makeActivity({
          txHash: '0xeve',
          timestamp: String(Date.parse('2026-11-13T15:00:00Z')),
          participants: [makeParticipant({ address: seededAddress('eve'), ckbDelta: '-1' })],
        }),
      ],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });
    const { unmount } = render(<ActivitiesStreamExplorer />);
    const dividers = () =>
      Array.from(document.querySelectorAll('[data-testid^="activity-day-divider"]')).map(
        (d) => d.textContent
      );
    await loadUnderStillClock(() => dividers().length === 2);
    expect(dividers()).toEqual(['Today', 'Yesterday']);
    tick(1000);
    expect(dividers()).toEqual(['Yesterday', 'Nov 13']);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  // Lane B4 second fix round (review-rules-fixes-logic MUST 1, the coordinator's item 1): the
  // turn of the day re-labels every divider ("Today" becomes "Yesterday") and must remount
  // nothing: a group is keyed by the UTC+8 date of its rows, not by its label. Open a run, open a
  // row's hidden parties, focus the run's toggle, and let the clock pass midnight.
  it('keeps what the reader opened, and the focus, when the day turns (B4 fix round)', async () => {
    vi.useRealTimers();
    vi.useFakeTimers({ now: Date.parse('2026-11-14T15:59:59Z') });
    const late = (n: number) =>
      makeActivity({
        txHash: `0xlate${n}`,
        timestamp: String(Date.parse(`2026-11-14T15:59:${50 - n}Z`)),
        participants: [makeParticipant({ address: seededAddress('late'), ckbDelta: '-1' })],
      });
    // a script call, so it is not one of the two transfers' run (Gate B's rule folds transfers by
    // what was done and the code called, whoever the parties are: re-pinned on purpose, it was a
    // transfer that its seven parties kept apart)
    const crowd = makeActivity({
      txHash: '0xcrowd',
      timestamp: String(Date.parse('2026-11-14T15:00:00Z')),
      typeCalls: [
        {
          typeCodeHash: `0x${'3a'.repeat(32)}`,
          typeHashType: 'type',
          typeArgs: '0x00',
          scriptHash: `0x${'3a'.repeat(31)}00`,
          scriptName: '.bit Time Index State',
        },
      ],
      participants: ['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((c, i) =>
        makeParticipant({ address: seededAddress(`crowd${c}`), ckbDelta: String(i + 1) })
      ),
    });
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        late(1),
        late(2),
        crowd,
        makeActivity({
          txHash: '0xeve',
          timestamp: String(Date.parse('2026-11-13T15:00:00Z')),
          participants: [makeParticipant({ address: seededAddress('eve'), ckbDelta: '-1' })],
        }),
      ],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });
    const { unmount } = render(<ActivitiesStreamExplorer />);
    const dividers = () =>
      Array.from(document.querySelectorAll('[data-testid^="activity-day-divider"]')).map(
        (d) => d.textContent
      );
    await loadUnderStillClock(() => dividers().length === 2);
    const toggle = screen.getByRole('button', { name: '2 transactions ↓' });
    fireEvent.click(toggle);
    const more = screen.getByRole('button', { name: '+2 participants' });
    fireEvent.click(more);
    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    tick(1000);
    expect(dividers()).toEqual(['Yesterday', 'Nov 13']);
    expect(toggle.isConnected).toBe(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(document.activeElement).toBe(toggle);
    expect(more.isConnected).toBe(true);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    unmount();
  });

  it('renders each activity row with tx hash, event rows, and participant lines', async () => {
    const timestamp = String(Date.now() - 23_000);
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [
        makeActivity({
          txHash: '0xscriptcall0000000000000000000000000000000000000000000000000001',
          blockNumber: 123_456,
          timestamp,
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
              address: seededAddress('zdaaaaaaaaaaaaaaaaaa'),
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
    // T7 (option A): the tx hash is the row's hover; the headline is the link to the tx.
    // Consistency C3: the age joins the hash in that hover.
    expect(row).toHaveAttribute('title', expect.stringMatching(/^0xscriptca.* · \d+s ago$/));
    expect(row).not.toHaveTextContent(/0xscriptca/);
    expect(within(row).getByRole('link', { name: 'Script call' })).toHaveAttribute(
      'href',
      expect.stringMatching(/^\/mainnet\/tx\/0xscriptca/)
    );
    // Block number shown
    expect(within(row).getByText('#123,456')).toBeInTheDocument();
    // PA-22 (re-pinned): a live feed's row says its age, the full date-time its hover (it was the
    // date, consistency C3, which left a row of seconds ago reading only today's date)
    const evidence = row.querySelector('[data-role="evidence"]') as HTMLElement;
    expect(within(evidence).getByText(formatTimeAgo(timestamp, Date.now()))).toHaveAttribute(
      'title',
      formatUtc8DateTime(timestamp)
    );
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
                // B4: a JoyID address; a row names no lock beside a default-lock address (R9)
                address: seededAddress('zdaaaaaaaaaaaaaaaaaa', JOYID_CODE_HASH),
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
                address: seededAddress('fresh111111111111111'),
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

    expect(await screen.findByText(exactAmount('+3 CKB'))).toBeInTheDocument();
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
                address: seededAddress('old11111111111111111'),
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
                address: seededAddress('new11111111111111111'),
                ckbDelta: '400000000',
              }),
            ],
            blockNumber: 901,
          }),
          makeActivity({
            txHash: '0xolder-head',
            participants: [
              makeParticipant({
                address: seededAddress('old11111111111111111'),
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

    expect(await screen.findByText(exactAmount('+1 CKB'))).toBeInTheDocument();

    const banner = await screen.findByRole('button', { name: '1 new activity' });
    expect(screen.queryByText(exactAmount('+4 CKB'))).not.toBeInTheDocument();
    // Visual finish V11: the bar and its hover are named inks (STREAM_COLORS), both read by classes
    // through CSS variables, so the hover still wins over the bar; no bracket hex
    expect(banner).toHaveClass('bg-[color:var(--bar)]', 'hover:bg-[color:var(--bar-hover)]');
    expect(banner.style.getPropertyValue('--bar')).toBe('var(--c-stream-pending)');
    expect(banner.style.getPropertyValue('--bar-hover')).toBe('var(--c-stream-pending-hover)');
    expect(banner.style.backgroundColor).toBe('');

    fireEvent.click(banner);

    expect(screen.getByText(exactAmount('+4 CKB'))).toBeInTheDocument();
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
            address: seededAddress(`head${String(index).padStart(2, '0')}`),
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
                address: seededAddress('anchor11111111111111'),
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
                address: seededAddress(`head${String(DEFAULT_PAGE_SIZE).padStart(2, '0')}`),
                ckbDelta: '2200000000',
              }),
            ],
            blockNumber: 980,
          }),
          makeActivity({
            txHash: '0xanchor-head',
            participants: [
              makeParticipant({
                address: seededAddress('anchor11111111111111'),
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

    expect(await screen.findByText(exactAmount('+1 CKB'))).toBeInTheDocument();
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
                address: seededAddress('old11111111111111111'),
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
                address: seededAddress('new11111111111111111'),
                ckbDelta: '400000000',
              }),
            ],
            blockNumber: 901,
          }),
          makeActivity({
            txHash: '0xolder-head',
            participants: [
              makeParticipant({
                address: seededAddress('old11111111111111111'),
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

    expect(await screen.findByText(exactAmount('+1 CKB'))).toBeInTheDocument();
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
                address: seededAddress('stable11111111111111'),
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

    expect(await screen.findByText(exactAmount('+1 CKB'))).toBeInTheDocument();
    expect(await screen.findByText('Could not refresh: refresh failed')).toBeInTheDocument();
  });

  // A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
  // awaited: the stream says it is reading, never "Nothing of this kind", and a real empty answer
  // still says so once back online.
  it('says it is reading while its read is paused, then the true empty state once back online', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      hasMore: false,
      nextCursor: null,
      total: 0,
      limit: 20,
    });
    const { resume } = renderPaused(<ActivitiesStreamExplorer />);
    expect(await screen.findByText('Reading activities…')).toHaveAttribute('role', 'status');
    expect(screen.queryByText('Nothing of this kind in the latest activities.')).toBeNull();
    resume();
    expect(
      await screen.findByText('Nothing of this kind in the latest activities.')
    ).toBeInTheDocument();
  });
});

// Hierarchy D11 (ruling R7): "All DAO activity →" on /dao links /activities?filter=dao, so the
// stream starts on the filter its URL names and writes a chosen filter back (replace, not push).
describe('ActivitiesStreamExplorer filter in the URL (D11)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      limit: DEFAULT_PAGE_SIZE,
      hasMore: false,
      nextCursor: null,
    });
  });

  function renderAt(url: string) {
    const router = createMemoryRouter([{ path: '*', element: <ActivitiesStreamExplorer /> }], {
      initialEntries: [url],
    });
    render(<RouterProvider router={router} />);
    return router;
  }
  // Re-pinned on purpose (controls lane C2): the kind filter is the controls board's chips, a
  // radio group named "kind", a radio per kind named by its lowercase words, the chosen one
  // aria-checked (it was the aria-pressed group "Filter by kind": "Nervos DAO", "All", "Tokens",
  // "Objects").
  const chip = (name: string) =>
    within(screen.getByRole('radiogroup', { name: 'kind' })).getByRole('radio', { name });

  it('starts on the filter its URL names and asks the API for it (?filter=dao)', async () => {
    renderAt('/mainnet/activities?filter=dao');
    expect(chip('nervos dao')).toHaveAttribute('aria-checked', 'true');
    expect(chip('all')).toHaveAttribute('aria-checked', 'false');
    await waitFor(() =>
      expect(api.getGlobalActivities).toHaveBeenCalledWith(
        expect.objectContaining({ filter: 'dao', limit: DEFAULT_PAGE_SIZE })
      )
    );
    expect(api.getGlobalActivities).not.toHaveBeenCalledWith(
      expect.objectContaining({ filter: 'all' })
    );
  });

  it.each(['?filter=interest', ''])(
    'starts on All when the URL names an unknown filter or none (%s)',
    async (search) => {
      renderAt(`/mainnet/activities${search}`);
      expect(chip('all')).toHaveAttribute('aria-checked', 'true');
      await waitFor(() =>
        expect(api.getGlobalActivities).toHaveBeenCalledWith(
          expect.objectContaining({ filter: 'all' })
        )
      );
    }
  );

  it('writes a chosen filter back to the URL by replace, and All drops it', async () => {
    const router = renderAt('/mainnet/activities?filter=dao');
    fireEvent.click(chip('tokens'));
    await waitFor(() => expect(router.state.location.search).toBe('?filter=token'));
    expect(router.state.location.pathname).toBe('/mainnet/activities');
    expect(router.state.historyAction).toBe('REPLACE');
    expect(chip('tokens')).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(chip('all'));
    await waitFor(() => expect(router.state.location.search).toBe(''));
    expect(chip('all')).toHaveAttribute('aria-checked', 'true');
  });

  it('follows the URL when it changes under the mounted stream', async () => {
    const router = renderAt('/mainnet/activities?filter=dao');
    expect(chip('nervos dao')).toHaveAttribute('aria-checked', 'true');
    await act(async () => {
      await router.navigate('/mainnet/activities');
    });
    expect(chip('all')).toHaveAttribute('aria-checked', 'true');
  });
});
