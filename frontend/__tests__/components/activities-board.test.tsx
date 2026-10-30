/**
 * The Activities board (Activities / ActivitiesDao / ActivitiesObjects, report A): chip names and
 * order, quiet states in sentence case, day dividers on the site's one UTC+8 clock (D2), and
 * object items with their collection and composition (R12, TX-29).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { api, type GlobalActivity } from '@/lib/api';

vi.mock('@/lib/api', async () => ({
  ...(await vi.importActual('@/lib/api')),
  api: { getGlobalActivities: vi.fn() },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

const page = (data: GlobalActivity[]) => ({
  data,
  limit: 50,
  hasMore: false,
  nextCursor: null,
});
const activity = (
  txHash: string,
  at: string,
  extra: Partial<GlobalActivity> = {}
): GlobalActivity => ({
  txHash,
  blockNumber: 20835685,
  txIndex: 1,
  timestamp: String(Date.parse(at)),
  isCellbase: false,
  protocolActions: [],
  typeCalls: [],
  lockCalls: [],
  participants: [
    {
      address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqwlng7r',
      lockHash: `0x${'aa'.repeat(32)}`,
      lockHashPrefix: null,
      roles: [],
      ckbDelta: '-100000000',
      usedDelta: '0',
      itemDeltas: [],
      tags: 0,
    },
  ],
  ...extra,
});

class NoopObserver {
  observe() {}
  disconnect() {}
  unobserve() {}
  takeRecords() {
    return [];
  }
}

const tz = process.env.TZ;
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('IntersectionObserver', NoopObserver);
  window.scrollTo = vi.fn();
});
afterEach(() => {
  process.env.TZ = tz;
  vi.useRealTimers();
});

describe('Activities board (TX-30, TX-32, TX-29)', () => {
  it('uses the board chip names and order, with swatches for CKB transfers and script calls', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page([]));
    render(<ActivitiesStreamExplorer />);
    const group = screen.getByRole('group', { name: 'Filter by kind' });
    expect(
      within(group)
        .getAllByRole('button')
        .map((b) => b.getAttribute('aria-label'))
    ).toEqual([
      'All',
      'CKB transfers',
      'Script calls',
      'Nervos DAO',
      'Tokens',
      'Objects',
      'Identities',
      'Protocols',
    ]);
    expect(
      within(group).getByRole('button', { name: 'CKB transfers' }).querySelector('[data-swatch]')
    ).toHaveStyle({ backgroundColor: 'rgb(46, 219, 163)' });
    expect(
      within(group).getByRole('button', { name: 'Script calls' }).querySelector('[data-swatch]')
    ).toHaveStyle({ backgroundColor: 'rgb(58, 66, 88)' });
    expect(screen.queryByText(/loaded$/)).not.toBeInTheDocument();
  });

  it('says an empty filter in a quiet sentence, not in capitals', async () => {
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page([]));
    render(<ActivitiesStreamExplorer />);
    const empty = await screen.findByText('Nothing of this kind in the latest activities.');
    expect(empty).not.toHaveClass('uppercase');
  });

  it('says a load failure in sentence case with its reason', async () => {
    vi.mocked(api.getGlobalActivities).mockRejectedValue(new Error('index row missing'));
    render(<ActivitiesStreamExplorer />);
    const failed = await screen.findByText('Could not read activities', {}, { timeout: 5000 });
    expect(failed).not.toHaveClass('uppercase');
    expect(screen.getByText('index row missing')).toBeInTheDocument();
  });

  it('says what it is reading while the first page loads', async () => {
    vi.mocked(api.getGlobalActivities).mockReturnValue(new Promise(() => {}));
    render(<ActivitiesStreamExplorer />);
    expect(screen.getByRole('status')).toHaveTextContent('Reading activities…');
  });

  it('groups rows by the UTC+8 day whatever the browser zone (D2)', async () => {
    process.env.TZ = 'America/New_York';
    vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-10-30T02:00:00Z')); // 10:00 on Oct 30, UTC+8
    vi.mocked(api.getGlobalActivities).mockResolvedValue(
      page([
        activity('0x01', '2026-10-29T17:00:00Z'), // Oct 30 01:00 UTC+8
        activity('0x02', '2026-10-29T15:00:00Z'), // Oct 29 23:00 UTC+8
        activity('0x03', '2026-10-25T20:00:00Z'), // Oct 26 04:00 UTC+8
      ])
    );
    render(<ActivitiesStreamExplorer />);
    expect(await screen.findByTestId('activity-day-divider-today')).toBeInTheDocument();
    expect(screen.getByTestId('activity-day-divider-yesterday')).toBeInTheDocument();
    expect(screen.getByTestId('activity-day-divider-oct-26')).toBeInTheDocument();
    expect(screen.queryByTestId('activity-day-divider-oct-25')).not.toBeInTheDocument();
  });

  it('names an object item by its collection and shows its composition (R12, TX-29)', async () => {
    const row = activity('0x4bec9eca', '2026-10-29T15:00:00Z');
    row.participants[0].itemDeltas = [
      {
        kind: 'object',
        objectId: `0x0a3a5c${'00'.repeat(26)}6129`,
        delta: -1,
        collection: { id: `0x${'cc'.repeat(32)}`, name: 'Nervape Headwear' },
        class: 'object',
        composition: 'btc_ckb',
      },
    ];
    vi.mocked(api.getGlobalActivities).mockResolvedValue(page([row]));
    render(<ActivitiesStreamExplorer />);
    await waitFor(() =>
      expect(document.querySelector('[data-feed-row="0x4bec9eca"]')).toHaveTextContent(
        'Nervape Headwear 0x0a3a5c...6129 departed'
      )
    );
    expect(document.querySelector('[data-composition="btc_ckb"]')).toBeInTheDocument();
  });
});
