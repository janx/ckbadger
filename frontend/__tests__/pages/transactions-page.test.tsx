import { CHARTS } from '@/components/charts/chart-catalogue';
import { chooseView } from '@/__tests__/utils/tab-row';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { afterEach, describe, it, expect, vi, beforeEach, onTestFinished } from 'vitest';
import { atViewportWidth, stubViewportFromMd } from '@/__tests__/utils/viewport';
import {
  atContainerWidth,
  dottedItems,
  phoneHeadPx,
  phoneLines,
  statedHeadPx,
} from '@/__tests__/utils/phone-row';
import { expectBefore, expectOneRowSeal } from '@/__tests__/utils/seals';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { render } from '@/__tests__/utils/test-utils';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';
import { dailyActivities, feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionsPage, { TRANSACTIONS_FORM_PX } from '@/app/transactions/page';
import { api } from '@/lib/api';
import { formatTimeAgo, formatUtc8DateTime } from '@/lib/utils';
import { poolStatusLabel } from '@/components/ui/pool-status';
import {
  CELLBASE_LIST_MIN_PX,
  TRANSACTION_LIST_MIN_PX,
} from '@/components/transaction/transaction-list-table';
import { cellFaults, faultLines } from '@/__tests__/utils/list-table-cells';
import { CLOCK_BEHIND } from '@/lib/format-age';
import { pagedRead, poolPagedRead } from '@/__tests__/utils/paged-read';

// D11: the real navigation (setup.ts stubs it with an empty URL), so a router's location reaches
// the page; outside a router it reads window.location, as the stub's empty URL did.
vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', async () => ({
  ...(await vi.importActual('@/lib/api')),
  api: {
    getTransactions: vi.fn(),
    getPoolTransactions: vi.fn(),
    getDailyActivityStats: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const emptyPool = poolPagedRead({
  summary: { size: '0', cycles: '0', minFeeRate: null, maxFeeRate: null, newestSeenAt: null },
  data: [],
  total: 0,
  limit: 50,
  nextCursor: null,
  totals: { pending: 0, proposed: 0, awaitingIndex: 0 },
  truncated: false,
  lastPolledAt: null,
});
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getPoolTransactions).mockResolvedValue(emptyPool);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([]);
});

describe('TransactionsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // Lane L (charts ↔ topics).
  it('links the chart drawn from transactions (lane L)', async () => {
    render(<TransactionsPage />);
    const charts = await screen.findByRole('navigation', { name: 'Charts of these numbers' });
    const hrefs = within(charts)
      .getAllByRole('link')
      .map((a) => a.getAttribute('href'));
    expect(hrefs).toEqual(
      CHARTS.filter((c) => c.evidenceHref === '/transactions').map(
        (c) => `/mainnet/charts/${c.slug}`
      )
    );
  });

  it('renders neutral transaction hashes and block links from fetched transactions', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [
          {
            hash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            blockNumber: 123456,
            blockHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
            index: 0,
            inputsCount: 2,
            outputsCount: 3,
            fee: '100000000',
            value: '123456789012345678',
            txSize: 226,
            cycles: 1000000,
            cyclesStatus: 'complete',
            activity: feedSamples.eightParties,
            isCellbase: false,
            timestamp: '2026-02-20T00:00:00Z',
          },
        ],
        total: 1,
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );

    render(<TransactionsPage />);

    // Lane L: the "Transactions" chart link now shares the h1's word, so the h1 is named by role.
    expect(screen.getByRole('heading', { level: 1, name: 'Transactions' })).toBeInTheDocument();
    expect(
      screen.queryByText('Browse all transactions on the CKB network')
    ).not.toBeInTheDocument();

    await waitFor(() => {
      // re-pinned on purpose (lists lane P3, plan §7.6): the page opens on the user transactions
      expect(api.getTransactions).toHaveBeenCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'user',
      });
    });

    await waitFor(() => {
      // One form at a time (R1): the measured table here, the phone row on a phone.
      expect(screen.getAllByRole('link', { name: '#123,456' })).toHaveLength(1);
    });
    expect(
      screen
        .getAllByRole('link', { name: '#123,456' })
        .every((link) => link.getAttribute('href') === '/mainnet/blocks/123456')
    ).toBe(true);

    const hash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
    expect(screen.getAllByTitle(hash)).toHaveLength(1);
    // Table-rows T5: the hash is the row's key, 13 px bright on an 18 px line (it read in neutral
    // 12 px ink); still no colour of its own.
    // Re-pinned on purpose (lists lane P3, the link-look gate): the bright ink is the key look's,
    // on the link; the hash inherits it (it wrote text-text-bright itself, a link ink on a child)
    for (const hashDisplay of screen.getAllByTitle(hash)) {
      expect(hashDisplay).toHaveClass('text-body', 'leading-row');
      expect(hashDisplay).not.toHaveClass('text-text');
      expect(hashDisplay).not.toHaveClass('text-text-bright');
      expect(hashDisplay.closest('a')).toHaveClass('text-text-bright');
      expect(hashDisplay).not.toHaveClass('text-small');
      expect(hashDisplay.children).toHaveLength(0);
      expect(hashDisplay.closest('a')).toHaveAttribute('href', `/mainnet/tx/${hash}`);
    }
    expect(screen.queryByTitle(/Click to copy/)).not.toBeInTheDocument();
    expect(screen.getAllByText('2 → 3')).toHaveLength(1);
    // F2-tx (TX-25 / TX-26): the board's columns, newest first with cellbase rows set back.
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(table).toHaveTextContent('What it did');
    // re-pinned on purpose (lists lane P3): the cellbases are a tab of their own, no row set back
    expect(screen.getByText('committed · newest first')).toBeInTheDocument();
    // Re-pinned on purpose (readability B5): this fixture is one page (no older, no newer), and
    // one page draws no pager (it drew a disabled "Older →"). Controls lane C2: the board's
    // pager, whose way older is "older →".
    expect(screen.queryByRole('navigation', { name: 'committed transactions' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'older →' })).toBeNull();
  });
});

describe('Transactions pool view', () => {
  it('keeps full pool evidence inside the desktop width budget without shortening exact amounts', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      summary: {
        size: '226',
        cycles: '3500000000',
        minFeeRate: '54623',
        maxFeeRate: '54623',
        newestSeenAt: '1792886400000',
      },
      total: 1,
      totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'proposed',
          value: '18446744073709551615',
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '3500000000',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'complete', reasons: [] },
          activity: feedSamples.eightParties,
        },
      ],
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    // re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8): the table is drawn from 1,272 px (was within 1,248)
    // re-pinned on purpose (readability F3-e): every column its content, 1,220 px, which the
    // widest 1,232 px container holds
    // re-pinned on purpose (review fb-tests 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13568 → 9999" (102 → 110): 1,228 px
    // re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,212 px
    expect(Number.parseFloat(table.style.minWidth)).toBe(TRANSACTIONS_FORM_PX);
    expect(table).toHaveTextContent('184,467,440,737.09551615 CKB');
    expect(table).toHaveTextContent('0.00012345 CKB');
    expect(table).toHaveTextContent('3,500,000,000');
    expect(table.querySelectorAll('[data-ckb-amount]')).toHaveLength(2);
    for (const amount of table.querySelectorAll('[data-ckb-amount]'))
      expect(amount).toHaveClass('flex-wrap');
    // The seals by kind (C2): the transaction's row stone's footprint (56.5 px) is 29.5 px more
    // than the seal it replaced (27). Old issue C: the hash keeps its 21 characters (12 + 6); the
    // Transaction column's 30 px are the widest container's own (1,232 px, of which the columns
    // took 1,200), so every other column keeps its width and the table is 1,230 px, the committed
    // table's width; the most cycles still fit.
    const cols = Array.from(table.querySelectorAll('col')).map((c) =>
      Number.parseFloat((c as HTMLElement).style.minWidth)
    );
    // re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8): the derived columns grow with their text, 1,272 px, more than the
    // widest 1,232 px container holds
    // re-pinned on purpose (readability F3-e): every column its content (the spares gone; What it
    // does, Cells, Value and Fee hold their widest values; What it does is the flexible column,
    // its minimum read from minWidth): 1,220 px
    // re-pinned on purpose (review fb-tests 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13568 → 9999" (102 → 110): 1,228 px
    // re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,212 px
    expect(cols).toEqual([200, 126, 184, 110, 170, 131, 118, 71, 102]);
    expect(cols.reduce((sum, w) => sum + w, 0)).toBe(TRANSACTIONS_FORM_PX);
    expect(Number.parseFloat(table.style.minWidth)).toBe(TRANSACTIONS_FORM_PX);
    // U-09 (re-pinned): the row size, "0x" and 8 digits, the one ellipsis, 6 digits: 17
    // characters of JetBrains Mono at 14 px since R4 (8.4; 7.8 at 13 px; it was 21, 12 + 6 with
    // three dots)
    const shown = within(table).getByTitle(`0x${'ab'.repeat(32)}`);
    expect(shown).toHaveTextContent(/^0xabababab…ababab$/);
    const chars = shown.textContent!.length;
    expect(chars).toBe(17);
    // the stone, the hash and the 16 px gap fit the 247 px with 31.7 px to spare (the 31 px the
    // column kept so both lists changed form at one width; re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8), the pool's table at
    // 1,272 and the committed one at 1,286)
    // re-pinned on purpose (readability F3-e): the column is its content, 216 px, 0.7 of rounding
    // up to a whole pixel; both lists change form at 1,220 px, their sums being equal
    // re-pinned on purpose (readability R6): 200 px, the stone's footprint 40.3, 199.1 used and 0.9
    // of rounding up
    const used = ROW_SEAL.tx.footprint + chars * 8.4 + 16;
    expect(used).toBeCloseTo(199.1, 6);
    expect(cols[0] - used).toBeCloseTo(0.9, 6);
  });

  // Old issue C, round 2: at 1,200 px the pool's columns lived on the 32 px the 1,232 px container
  // spread over them (Status rendered 154 for its declared 150); the hash's 30 px took that, and
  // "● Pending · 26s ago" broke onto two lines. Status is now declared for its widest content on
  // one line, and Size, whose values are at most 7 characters, pays for it.
  it('declares Status wide enough for every status with the longest one-unit wait, on one line', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    const statuses = ['pending', 'proposed', 'committed_awaiting_index'] as const;
    // 59.5 s ago: "59s ago", the longest seconds form. The wait is read from the clock when the
    // row renders, which a loaded machine can put more than half a second after this line (it read
    // "1m ago" under two suites at once), so the clock stands still for this test; timers stay real.
    vi.useFakeTimers({ toFake: ['Date'] });
    onTestFinished(() => {
      vi.useRealTimers();
    });
    const timestamp = String(Date.now() - 59_500);
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      total: 3,
      totals: { pending: 1, proposed: 1, awaitingIndex: 1 },
      data: statuses.map((status, i) => ({
        hash: `0x${String(i + 1).repeat(64)}`,
        status,
        value: '100000000',
        fee: '12345',
        feeRate: '54623',
        txSize: 597000,
        cycles: '3500000000',
        inputsCount: 2,
        outputsCount: 3,
        timestamp,
        interpretation: { status: 'complete' as const, reasons: [] },
        activity: feedSamples.eightParties,
      })),
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    const heads = within(table).getAllByRole('columnheader');
    const at = heads.findIndex((th) => th.textContent === 'Status · waiting');
    const width = Number.parseFloat((table.querySelectorAll('col')[at] as HTMLElement).style.width);
    // re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8): 184 px (was 172)
    expect(width).toBe(184);
    // The cell: a 7 px dot, 6 px gaps (gap-1.5), the status word and "· <wait>", all at 13 px
    // (JetBrains Mono, 7.8 px a character), then the column's 16 px gap.
    const cells = Array.from(table.querySelectorAll('tbody tr')).map(
      (tr) => tr.querySelectorAll('td')[at].firstElementChild as HTMLElement
    );
    expect(cells.map((cell) => cell.textContent)).toEqual([
      'Pending· 59s ago',
      'Proposed· 59s ago',
      'Confirming· 59s ago',
    ]);
    for (const cell of cells) {
      expect(cell.className).toBe('text-small flex items-center gap-1.5');
      expect(cell.firstElementChild).toHaveClass('h-[7px]', 'w-[7px]');
    }
    // The widest status word the pool serves, and the longest wait the one-unit formatter writes
    // for any age under 100 days (the seconds, minutes and hours forms and two-digit days are all
    // 7 characters). A wait of 100 days or more, or "this device's clock is behind", wraps inside
    // the cell; nothing runs past it.
    const word = Math.max(...statuses.map((s) => poolStatusLabel(s).length));
    expect(word).toBe('Confirming'.length);
    const now = Date.now();
    const waits = [59.5, 59 * 60 + 30, 23 * 3600 + 1800, 99 * 86400 + 43200].map((s) =>
      formatTimeAgo(new Date(now - s * 1000), now)
    );
    expect(waits).toEqual(['59s ago', '59m ago', '23h ago', '99d ago']);
    const wait = Math.max(...waits.map((w) => `· ${w}`.length));
    expect(wait).toBe(9);
    const widest = 7 + 6 + word * 7.8 + 6 + wait * 7.8 + 16;
    expect(widest).toBeCloseTo(183.2, 6);
    expect(widest).toBeLessThanOrEqual(width);
    // what broke at the old 150: "● Pending · 26s ago" needs 159.8 at 13 px (150.2 at 12)
    expect(7 + 6 + 'Pending'.length * 7.8 + 6 + '· 26s ago'.length * 7.8 + 16).toBeGreaterThan(150);
    // Size pays: its values are at most "597,000" (a transaction is no larger than a block),
    // 7 characters at 13 px in 72 − 16 = 56 px; its head "Size, bytes" (12 px, 7.2 a character)
    // takes two lines, as "Fee rate, shannon/KB" beside it already does, so the head row keeps
    // its height (re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8)).
    const size = heads.findIndex((th) => th.textContent === 'Size, bytes');
    const rate = heads.findIndex((th) => th.textContent === 'Fee rate, shannon/KB');
    const colWidth = (i: number) =>
      Number.parseFloat((table.querySelectorAll('col')[i] as HTMLElement).style.width);
    // re-pinned on purpose (readability F3-e): 71 px, its 1 px spare gone
    expect(colWidth(size)).toBe(71);
    expect(within(table).getAllByText('597,000')).toHaveLength(3);
    expect('597,000'.length * 7.8).toBeLessThanOrEqual(colWidth(size) - 16);
    expect('Size, bytes'.length * 7.2).toBeGreaterThan(colWidth(size) - 16);
    expect('bytes'.length * 7.2).toBeLessThanOrEqual(colWidth(size) - 16);
    expect('Fee rate, shannon/KB'.length * 7.2).toBeGreaterThan(colWidth(rate) - 16);
  });

  // Review fb-logic 3 (the lead's ruling): the wait is a figure; the clock-behind sentence is
  // words. In the Status column (168 px of content: the dot, "Confirming" and their gaps leave 71
  // px for the wait) it was one NoBreak 233.4 px wide ("· " and 29 characters) in a flex row that
  // does not wrap; it breaks between its words, and the comment that said it did is now true.
  it('lets the clock-behind sentence break between its words in the Status column', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      total: 1,
      totals: { pending: 0, proposed: 0, awaitingIndex: 1 },
      data: [
        {
          hash: `0x${'7'.repeat(64)}`,
          status: 'committed_awaiting_index' as const,
          value: '100000000',
          fee: '12345',
          feeRate: '54623',
          txSize: 597000,
          cycles: '3500000000',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: String(Date.now() + 10 * 60_000),
          interpretation: { status: 'complete' as const, reasons: [] },
          activity: feedSamples.eightParties,
        },
      ],
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    const sentence = within(table).getByText(CLOCK_BEHIND);
    expect(sentence.closest('[data-nobreak], .whitespace-nowrap')).toBeNull();
    expect(faultLines(cellFaults(table))).toEqual([]);
  });

  // Review fb-tests 6 (the lead's ruling): a value one glyph past its bound, and what the pool's
  // table does with it today, for the lists page lane to rule on. A fee rate of 10,000,000,000
  // shannons a KB, "10,000,000,000" (14 glyphs at 13 px, 109.2; the bound of record,
  // "1,000,000,000", is 13, 101.4), in the Fee rate column (118 px, its box 102): one figure on
  // one line, 7.2 px wider than its box, start-aligned past its right edge (CSS Text 3) into its
  // own 16 px gap, short of the Size column.
  it('holds a fee rate one glyph past its bound on one line, 7.2 px into its gap', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      total: 1,
      totals: { pending: 1, proposed: 0, awaitingIndex: 0 },
      data: [
        {
          hash: `0x${'8'.repeat(64)}`,
          status: 'pending' as const,
          value: '100000000',
          fee: '12345',
          feeRate: '10000000000',
          txSize: 597000,
          cycles: '3500000000',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: String(Date.now() - 30_000),
          interpretation: { status: 'complete' as const, reasons: [] },
          activity: feedSamples.eightParties,
        },
      ],
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    const at = within(table)
      .getAllByRole('columnheader')
      .findIndex((th) => th.textContent === 'Fee rate, shannon/KB');
    const column = Number.parseFloat(
      (table.querySelectorAll('col')[at] as HTMLElement).style.minWidth
    );
    expect(column).toBe(118);
    const rate = within(table).getAllByRole('row')[1].querySelectorAll('td')[at] as HTMLElement;
    expect(rate).toHaveTextContent(/^10,000,000,000$/);
    expect(rate).toHaveClass('whitespace-nowrap', 'text-right');
    expect(cellFaults(table).filter((f) => f.rule === 'figure breaks')).toEqual([]);
    const past = '10,000,000,000'.length * 7.8 - (column - 16);
    expect(past).toBeCloseTo(7.2, 10);
    expect(past).toBeLessThan(16);
  });

  // Old issue C: the pool's table changed form at 1,230 px, where the committed one did, and the
  // widest container (1,232 px from 1280) held both. re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8): the pool's table is
  // drawn from 1,272 px, the committed one from 1,286; the widest container holds neither.
  // Re-pinned on purpose (readability F3-e): the pool's table is drawn from 1,220 px, as the
  // committed one is, so the widest container draws both.
  // re-pinned on purpose (review fb-tests 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13568 → 9999" (102 → 110): 1,228 px, still inside the widest 1,232 (renamed: "1,220 px").
  // Re-pinned on purpose (readability R6): the transaction's row stone 16 px, its footprint 56.5 →
  // 40.3, so a table from 1,212 px (renamed: "1,228 px").
  it('lays the pool out as a table from 1,212 px, phone rows below', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    const poolPage = {
      ...emptyPool,
      total: 1,
      totals: { pending: 1, proposed: 0, awaitingIndex: 0 },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'pending' as const,
          value: '100000000',
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '1709221',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'complete' as const, reasons: [] },
          activity: feedSamples.eightParties,
        },
      ],
    };
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    for (const [width, form] of [
      [1232, 'table'],
      [TRANSACTIONS_FORM_PX, 'table'],
      [TRANSACTIONS_FORM_PX - 1, 'rows'],
      [976, 'rows'],
    ] as const) {
      vi.mocked(api.getPoolTransactions).mockResolvedValue(poolPage);
      atContainerWidth(width);
      const { unmount } = render(<TransactionsPage />);
      fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
      const shown = await screen.findByRole(form === 'table' ? 'table' : 'list', {
        name: form === 'table' ? 'Transactions in the pool' : 'Transactions in the pool, compact',
      });
      expect(shown.closest('[data-list-form]')).toHaveAttribute('data-list-form', form);
      // U-09 (re-pinned): the table's hash at the row size, the phone row's at the tight size
      expect(within(shown).getByTitle(`0x${'ab'.repeat(32)}`)).toHaveTextContent(
        form === 'table' ? /^0xabababab…ababab$/ : /^0xabab…abab$/
      );
      unmount();
      vi.unstubAllGlobals();
    }
  });

  // The coordinator's ruling (F3-e): with its Size head in the tick form the committed list's own
  // sum is 1,203 px, the pool's 1,220; the page's two lists change form at the larger, so the
  // committed list's What it did keeps the 17 px between them.
  // Re-pinned on purpose (reviews fb-tests 6 and fb-logic 6, the lead's ruling): the committed
  // list holds 13,568 inputs and genesis's cellbase, 1,227 px; the pool 13,568 inputs, 1,228;
  // What it did keeps the 1 px between them (renamed: "keeps 17 px"). Re-pinned on purpose
  // (readability R6): the smaller transaction stone takes 16 px from both lists' Transaction
  // column, 1,211 and 1,212, and What it did keeps the same 1 px.
  // Re-pinned on purpose (lists lane P3): the user transactions' list is 898 px on its own (the
  // size and the cycles bars left it), the cellbases' 804, the pool's 1,212 the largest: What it
  // did keeps 314 px beyond its 204 (renamed: "keeps 1 px").
  it('changes the lists’ form at the largest sum: the committed What it did keeps 314 px', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [
          {
            hash: `0x${'cd'.repeat(32)}`,
            blockNumber: 123456,
            blockHash: `0x${'ef'.repeat(32)}`,
            index: 1,
            inputsCount: 1,
            outputsCount: 2,
            fee: '1000',
            value: '123456789',
            txSize: 500,
            cycles: 1000,
            cyclesStatus: 'complete',
            timestamp: '2026-10-25T00:00:00Z',
            isCellbase: false,
            activity: feedSamples.eightParties,
          },
        ],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    render(<TransactionsPage />);
    const table = await screen.findByRole('table', { name: 'Committed transactions' });
    expect(Number.parseFloat(table.style.minWidth)).toBe(TRANSACTIONS_FORM_PX);
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(Number.parseFloat(cols[2].minWidth)).toBe(204 + 314);
  });

  // Readability F3-e: every pool column budgeted from the widest value its formatter writes
  // (lib/list-bounds.ts), at 0.6 em a glyph (the hash 14 px, the cells 13, the units and heads 12)
  // and the 16 px gap after every column but the last. What it does is the flexible column: its
  // longest word whole, the rest of the container beyond the sum. The pool and the committed list
  // are one page and change form at one width, the larger of their sums: both are 1,220 px.
  // re-pinned on purpose (review fb-tests 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13568 → 9999" (102 → 110): 1,228 px (renamed: "1,220 px").
  // Re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px,
  // its footprint 56.5 → 40.3, Transaction 216 → 200: 1,212 px (renamed: "1,228 px").
  it('budgets every pool column from what it holds; both lists change form at 1,212 px', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // the stone and its gap, the hash at the row size at 14 px
      ceil(ROW_SEAL.tx.footprint + at('0xabababab…ababab', 14) + 16),
      // the longest word a headline or "Interpretation unavailable" breaks around
      ceil(at('Interpretation', 13) + 16),
      // the dot, its gap, "Confirming", the gap and the longest wait under 100 days
      ceil(7 + 6 + at('Confirming', 13) + 6 + at('· 99d ago', 13) + 16),
      // the most inputs a transaction holds (597,000 bytes of 44-byte inputs: 13,568) and four
      // digits of outputs, as the pool writes them (ungrouped)
      ceil(at('13568 → 9999', 13) + 16),
      // a value under a million CKB on one line with its unit; a larger one breaks before its
      // decimals
      ceil(at('999,999.99999999', 13) + at(' CKB', 12) + 16),
      // a fee under 100 CKB with its unit
      ceil(at('99.99999999', 13) + at(' CKB', 12) + 16),
      // a rate up to 1,000,000,000 (its head's longest word, "shannon/KB", is narrower)
      ceil(Math.max(at('1,000,000,000', 13), at('shannon/KB', 12)) + 16),
      // a transaction no larger than a block
      ceil(at('597,000', 13) + 16),
      // the most a transaction may take, the last column
      ceil(at('3,500,000,000', 13)),
    ];
    expect(budgets).toEqual([200, 126, 184, 110, 170, 131, 118, 71, 102]);
    // the page's one form width (review rules-tests NOTE-1) is the pool's sum of these budgets,
    // the larger of the two lists' own sums: 1,212
    expect(budgets.reduce((sum, w) => sum + w, 0)).toBe(TRANSACTIONS_FORM_PX);
    expect(TRANSACTIONS_FORM_PX).toBe(
      Math.max(
        TRANSACTION_LIST_MIN_PX,
        CELLBASE_LIST_MIN_PX,
        budgets.reduce((sum, w) => sum + w, 0)
      )
    );
    // re-pinned on purpose (the coordinator's ruling: the committed Size head in the tick form):
    // the committed list's own sum is 1,203, the pool's 1,220 the larger, so both change form at
    // the pool's (the committed What it did keeps the 17 px between them); re-pinned on purpose
    // (reviews fb-tests 6, fb-logic 6): 1,227 and 1,228, What it did keeps 1 px; re-pinned on
    // purpose (readability R6): 1,211 and 1,212, What it did keeps 1 px; re-pinned on purpose
    // (lists lane P3): the user transactions 898, the cellbases 804, the pool's 1,212 the largest
    expect(TRANSACTION_LIST_MIN_PX).toBe(898);
    expect(CELLBASE_LIST_MIN_PX).toBe(804);
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    const poolPage = {
      ...emptyPool,
      total: 1,
      totals: { pending: 1, proposed: 0, awaitingIndex: 0 },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'pending' as const,
          value: '100000000',
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '1709221',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'complete' as const, reasons: [] },
          activity: feedSamples.eightParties,
        },
      ],
    };
    vi.mocked(api.getPoolTransactions).mockResolvedValue(poolPage);
    const { unmount } = render(<TransactionsPage />);
    // the view through the row's form on this line: a stubbed 358 px line under a viewport from
    // 640 holds no 388.4 px command row, its Select (the lead's final round, item 4)
    await chooseView(/^in the pool/);
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[1].width).toBe('auto');
    expect(Number.parseFloat(table.style.minWidth)).toBe(TRANSACTIONS_FORM_PX);
    unmount();
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    for (const [width, form] of [
      [1232, 'table'],
      [TRANSACTIONS_FORM_PX, 'table'],
      [TRANSACTIONS_FORM_PX - 1, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      vi.mocked(api.getPoolTransactions).mockResolvedValue(poolPage);
      atContainerWidth(width);
      const shown = render(<TransactionsPage />);
      // the row's form on this line (the lead's final round, item 4): a tab or its Select
      await chooseView(/^in the pool/);
      const list = await screen.findByRole(form === 'table' ? 'table' : 'list', {
        name: form === 'table' ? 'Transactions in the pool' : 'Transactions in the pool, compact',
      });
      expect(list.closest('[data-list-form]')).toHaveAttribute('data-list-form', form);
      shown.unmount();
      vi.unstubAllGlobals();
    }
  });

  // The seals by kind (C2): a pool transaction's row leads with its stone (the stone its page
  // presses), dashed — it is not in a block yet — before the hash, in the table and the phone row.
  it('leads each pool row with its transaction’s stone, dashed, in the table and the phone row', async () => {
    const hash = `0x${'cd'.repeat(32)}`;
    // the pool never holds a cellbase, but a row the API marks one consumes no cell, as its Cells
    // column already reads
    const cellbaseHash = `0x${'ce'.repeat(32)}`;
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      total: 2,
      totals: { pending: 2, proposed: 0, awaitingIndex: 0 },
      data: [
        {
          hash,
          status: 'pending',
          value: null,
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '1709221',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'partial', reasons: [{ code: 'unresolved_input' }] },
          activity: null,
        },
        {
          hash: cellbaseHash,
          status: 'pending',
          value: '0',
          fee: '0',
          feeRate: '0',
          txSize: 226,
          cycles: '0',
          inputsCount: 1,
          outputsCount: 1,
          timestamp: '1792886400000',
          interpretation: { status: 'complete', reasons: [] },
          activity: { ...feedSamples.eightParties, isCellbase: true },
        },
      ],
    });
    const ids = [
      { kind: 'tx', hash, consumed: 2, created: 3 },
      { kind: 'tx', hash: cellbaseHash, consumed: 0, created: 1 },
    ] as const;
    const { unmount } = render(<TransactionsPage />);
    // the view through the row's form on this line: a stubbed 358 px line under a viewport from
    // 640 holds no 388.4 px command row, its Select (the lead's final round, item 4)
    await chooseView(/^in the pool/);
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    const rows = Array.from(table.querySelectorAll('tbody tr'));
    expect(rows).toHaveLength(2);
    rows.forEach((tr, i) => {
      const seal = expectOneRowSeal(tr, ids[i], 'unpressed');
      expect(tr.querySelector('td')).toContainElement(seal);
      // the 16 px stone stands centred on the 20 px row line, 2 px clear above and below
      // (re-pinned on purpose, readability R6: the 24 px stone overhung it by 2 px, `-my-[2px]`)
      expect(seal).toHaveClass('h-row', 'items-center');
      expect(seal.className).not.toMatch(/-my-/);
      expectBefore(seal, within(tr as HTMLElement).getByTitle(ids[i].hash));
    });
    expect(rows[1]).toHaveTextContent('0 → 1');
    unmount();

    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    render(<TransactionsPage />);
    // the row's form on this line (the lead's final round, item 4): a tab or its Select
    await chooseView(/^in the pool/);
    const cards = await screen.findAllByTestId('pool-row-mobile');
    expect(cards).toHaveLength(2);
    cards.forEach((card, i) => {
      const phoneSeal = expectOneRowSeal(card, ids[i], 'unpressed');
      expect(card.firstElementChild).toHaveAttribute('data-part', 'mark');
      expect(card.firstElementChild).toContainElement(phoneSeal);
      expect(phoneSeal.className).not.toMatch(/-my-/);
      expectBefore(phoneSeal, within(card).getByTitle(ids[i].hash));
    });
    expect(cards[1]).toHaveTextContent('0 → 1 cell');
  });
  // Lane E4: the pool banner's "newest seen" age, its time's hover, reads the page's one clock, so
  // it moves with no data change; it was written once at render. The clock stops with the page.
  it('moves the age in the newest-seen time’s title with no data change (E4)', async () => {
    vi.useFakeTimers({ now: T0 });
    onTestFinished(() => {
      vi.useRealTimers();
    });
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      summary: { ...emptyPool.summary, newestSeenAt: String(T0 - 20_000) },
      totals: { pending: 1, proposed: 0, awaitingIndex: 0 },
    });
    const { unmount } = render(<TransactionsPage />);
    const seen = () =>
      within(screen.getByTestId('pool-banner'))
        .getByText(formatUtc8DateTime(T0 - 20_000))
        .getAttribute('title');
    await loadUnderStillClock(() => screen.queryByTestId('pool-banner') !== null);
    expect(seen()).toBe('20s ago');
    tick(1000);
    expect(seen()).toBe('21s ago');
    tick(38_000);
    expect(seen()).toBe('59s ago');
    tick(1000);
    expect(seen()).toBe('1m ago');
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('reads the paged pool with neutral hashes and keeps partial interpretation explicit', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue(
      poolPagedRead({
        summary: {
          size: '226',
          cycles: '1709221',
          minFeeRate: '54623',
          maxFeeRate: '54623',
          newestSeenAt: '1792886400000',
        },
        data: [
          {
            hash: `0x${'ab'.repeat(32)}`,
            status: 'proposed',
            value: null,
            fee: '12345',
            feeRate: '54623',
            txSize: 226,
            cycles: '1709221',
            inputsCount: 2,
            outputsCount: 3,
            timestamp: '1792886400000',
            interpretation: { status: 'partial', reasons: [{ code: 'unresolved_input' }] },
            activity: null,
          },
        ],
        total: 1,
        limit: 50,
        nextCursor: null,
        totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
        truncated: false,
        lastPolledAt: '2026-10-25T00:00:00Z',
      })
    );
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    expect(
      await screen.findByRole('table', { name: 'Transactions in the pool' })
    ).toHaveTextContent('Interpretation partial');
    expect(api.getPoolTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    expect(screen.getByTestId('pool-banner')).toHaveTextContent(
      '0 pending · 1 proposed · 0 awaiting index'
    );
    // Lane P: when the newest pool tx was seen, on the UTC+8 clock; how long ago is its hover.
    expect(screen.getByTestId('pool-banner')).toHaveTextContent(
      `· newest seen ${formatUtc8DateTime(1792886400000)}`
    );
    expect(screen.getByTestId('pool-banner')).not.toHaveTextContent(/ ago/);
    expect(
      within(screen.getByTestId('pool-banner')).getByText(formatUtc8DateTime(1792886400000))
    ).toHaveAttribute('title', expect.stringMatching(/ ago$/));
    const summary = screen.getByLabelText('Pool summary');
    expect(summary).toHaveTextContent('Pending · proposed0 · 1');
    expect(summary).toHaveTextContent('Total size226 B');
    expect(summary).toHaveTextContent('Total cycles1,709,221');
    expect(summary).toHaveTextContent('Fee rate, shannon/KB54,623 – 54,623');
    expect(
      screen.getByText(
        'Newest first, 50 a page. A proposed transaction enters a block 2–10 blocks after its proposal.'
      )
    ).toBeInTheDocument();
    expect(screen.getByText('not yet in a block · newest first')).toBeInTheDocument();
    const pool = screen.getByRole('table', { name: 'Transactions in the pool' });
    expect(pool).toHaveTextContent('Status · waiting');
    expect(pool).not.toHaveTextContent('Seen');
    // Table-rows T5: the hash is the pool row's key, at body and bright on an 18 px line (it read
    // in neutral 12 px ink); still no status colour on it.
    // Re-pinned on purpose (lists lane P3, the link-look gate): the bright ink is the key look's,
    // on the link; the hash inherits it (it wrote text-text-bright itself, a link ink on a child)
    for (const hash of screen.getAllByTitle(`0x${'ab'.repeat(32)}`)) {
      expect(hash).toHaveClass('text-body', 'leading-row');
      expect(hash).not.toHaveClass('text-text');
      expect(hash).not.toHaveClass('text-text-bright');
      expect(hash.closest('a')).toHaveClass('text-text-bright');
      expect(hash).not.toHaveClass('text-small');
      // the table's 17 characters, the row size (U-09: see the width budget above)
      expect(hash).toHaveTextContent(/^0xabababab…ababab$/);
      expect(hash.children).toHaveLength(0);
      expect(hash.closest('a')).toHaveAttribute('href', `/mainnet/tx/0x${'ab'.repeat(32)}`);
    }
  });
  it('folds a pool transaction into a phone row at 358 px: hash and status · what and value · fee and figures', async () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue(
      poolPagedRead({
        summary: {
          size: '226',
          cycles: '1709221',
          minFeeRate: '54623',
          maxFeeRate: '54623',
          newestSeenAt: '1792886400000',
        },
        data: [
          {
            hash: `0x${'ab'.repeat(32)}`,
            status: 'proposed',
            value: null,
            fee: '12345',
            feeRate: '54623',
            txSize: 226,
            cycles: '1709221',
            inputsCount: 2,
            outputsCount: 3,
            timestamp: '1792886400000',
            interpretation: { status: 'partial', reasons: [{ code: 'unresolved_input' }] },
            activity: null,
          },
        ],
        total: 1,
        limit: 50,
        nextCursor: null,
        totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
        truncated: false,
        lastPolledAt: '2026-10-25T00:00:00Z',
      })
    );
    render(<TransactionsPage />);
    // the view through the row's form on this line: a stubbed 358 px line under a viewport from
    // 640 holds no 388.4 px command row, its Select (the lead's final round, item 4)
    await chooseView(/^in the pool/);
    const [row] = await screen.findAllByTestId('pool-row-mobile');
    expect(screen.queryByRole('table', { name: 'Transactions in the pool' })).toBeNull();
    // Re-pinned on purpose (lane D4, the phone-rows board): the value leaves the right of what it
    // does for line 3, where every figure stands after its name and the rate is named ("fee rate");
    // bytes and cycles leave the phone row (it was "fee 0.00012345 CKB" with "54,623 shannon/KB ·
    // 2 → 3 cells · 226 B · 1,709,221 cycles" at the right, the value bare beside what it does).
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xabab…abab', expect.stringMatching(/^Proposed· .+ ago$/)],
      ['what', 'Interpretation unavailablepartial: an input is not yet resolvable', null],
      [
        'figures',
        '2 → 3\u00a0cells · Interpretation partial · fee 0.00012345 CKB · fee rate 54,623 shannon/KB',
        null,
      ],
    ]);
  });
});

it('keeps the pool banner above tabs and uses exact day counts even when kinds overlap', async () => {
  const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    {
      ...dailyActivities[0],
      date: yesterday,
      transferCount: 12,
      tokenCount: 11,
      coinbaseCount: 3,
      daoDepositCount: 2,
      daoWithdrawRequestCount: 1,
      daoWithdrawCompleteCount: 0,
      // the API's count of the DAO actions (lists lane P3: the page sums none)
      daoCount: 3,
      // re-pinned on purpose (the P3 follow-up's 2b): the route's count of the day's user
      // transactions; the page read the day's 20 transactions from /stats/tx and subtracted 3
      userTransactionCount: 17,
    },
  ]);
  vi.mocked(api.getTransactions).mockResolvedValue(
    pagedRead({
      data: [],
      limit: 50,
      nextCursor: null,
      hasMore: false,
      total: 0,
    })
  );
  render(<TransactionsPage />);
  // Re-pinned on purpose (controls lane C2): the kind filter is the controls board's chips, a
  // radio per kind named by its lowercase words and yesterday's count, and the views a tablist
  // (they were the aria-pressed buttons All / Nervos DAO / Tokens / Cellbase and the group
  // "Transaction views").
  expect(await screen.findByRole('radio', { name: /^all/ })).toBeInTheDocument();
  // re-pinned on purpose (lists lane P3): "all" counts the user transactions, yesterday's 20
  // transactions less its 3 cellbases (it counted the 20); the cellbases are a tab, no chip
  await waitFor(() =>
    expect(screen.getByRole('radio', { name: /^all/ })).toHaveTextContent('all 17')
  );
  expect(screen.getByRole('radio', { name: /^nervos dao/ })).toHaveTextContent(/^nervos dao 3$/);
  expect(screen.getByText(/DAO counts actions/)).toBeInTheDocument();
  expect(screen.getByRole('radio', { name: /^tokens/ })).toHaveTextContent('11');
  expect(screen.queryByRole('radio', { name: /^cellbase/ })).toBeNull();
  // what the page says first: the day's user transactions and its cellbases, apart
  expect(document.querySelector('[data-role="lede"]')).toHaveTextContent(
    /^17 user transactions and 3 cellbases yesterday \(UTC\+8\)\.$/
  );
  expect(
    screen
      .getByTestId('pool-banner')
      .compareDocumentPosition(screen.getByRole('tablist', { name: 'Transaction views' })) &
      Node.DOCUMENT_POSITION_FOLLOWING
  ).toBeTruthy();
  fireEvent.click(screen.getByRole('radio', { name: /^tokens/ }));
  await waitFor(() =>
    expect(api.getTransactions).toHaveBeenLastCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'token',
    })
  );
  // re-pinned on purpose (lists lane P3, the placement lane's note): it opens the pool's tab on
  // this page, so "↓", not a link's "→"
  fireEvent.click(screen.getByRole('button', { name: 'Open the pool ↓' }));
  await waitFor(() =>
    expect(api.getPoolTransactions).toHaveBeenCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'token',
    })
  );
  // re-pinned on purpose (lists lane P3, the link-look gate): a way onward, its arrow the look's
  expect(screen.getByRole('link', { name: /^Activities/ })).toHaveAttribute(
    'href',
    '/mainnet/activities'
  );
});
// Re-pinned on purpose (the P3 follow-up's 2b): the day's user transactions are its activity row's
// (userTransactionCount), so a day without a complete row states no count; it was a day whose
// /stats/tx bucket was incomplete ("No complete transaction count for …").
it('does not infer a transaction total from kind counts when yesterday is incomplete', async () => {
  const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
  const before = new Date(Date.now() + 8 * 3600000 - 2 * 86400000).toISOString().slice(0, 10);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    { ...dailyActivities[0], date: before.replaceAll('-', '') },
  ]);
  vi.mocked(api.getTransactions).mockResolvedValue(
    pagedRead({
      data: [],
      limit: 50,
      nextCursor: null,
      hasMore: false,
    })
  );
  render(<TransactionsPage />);
  expect(
    await screen.findByText(`No complete activity statistics for ${yesterday} (UTC+8).`)
  ).toBeInTheDocument();
  expect(document.querySelector('[data-role="lede"]')).toHaveTextContent(
    'Yesterday’s counts are unavailable.'
  );
  // the kind filter's radio, lowercase (controls lane C2; it was the button "All")
  expect(screen.getByRole('radio', { name: 'all' })).toHaveTextContent(/^all$/);
});

it('draws awaiting-index pool squares hollow, apart from pending (cyan) and proposed (gold) (TX-38)', async () => {
  vi.mocked(api.getTransactions).mockResolvedValue(
    pagedRead({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    })
  );
  const row = (n: number, status: 'pending' | 'proposed' | 'committed_awaiting_index') => ({
    hash: `0x${String(n).repeat(64)}`,
    status,
    value: '0',
    fee: '1000',
    feeRate: '1000',
    txSize: 400,
    cycles: '1000',
    inputsCount: 1,
    outputsCount: 1,
    timestamp: '1792886400000',
    interpretation: { status: 'complete' as const, reasons: [] },
    activity: null,
  });
  vi.mocked(api.getPoolTransactions).mockResolvedValue({
    ...emptyPool,
    data: [row(1, 'pending'), row(2, 'proposed'), row(3, 'committed_awaiting_index')],
    total: 3,
    totals: { pending: 1, proposed: 1, awaitingIndex: 1 },
  });
  render(<TransactionsPage />);
  const banner = await screen.findByTestId('pool-banner');
  await waitFor(() => expect(banner.querySelectorAll('[data-pool-status]')).toHaveLength(3));
  expect(banner.querySelector('[data-pool-status="pending"]')).toHaveClass('bg-info');
  expect(banner.querySelector('[data-pool-status="proposed"]')).toHaveClass('bg-gold');
  const awaiting = banner.querySelector('[data-pool-status="committed_awaiting_index"]');
  expect(awaiting).toHaveClass('border', 'border-text-dim');
  expect(awaiting).not.toHaveClass('bg-info');
  // the views' tab, lowercase (controls lane C2; it was the button "In the pool2"). Re-pinned on
  // purpose (lists lane P3): no count after its name, which summed pending and proposed in the
  // page; the pool's line above states each ("1 pending · 1 proposed · 1 awaiting index")
  expect(screen.getByRole('tab', { name: /^in the pool/ })).toHaveTextContent(/^in the pool$/);
  expect(banner).toHaveTextContent('1 pending · 1 proposed · 1 awaiting index');
});

// L4 (responsive plan, board L2): below sm the chrome is one control line, and a phone reads
// twenty transactions a page.
// Readability 2026-11-18, lane B5: what a reader needs to read a column stood after the fiftieth
// row ("Value is what the participants received…"; the pool's proposal window): it stands above
// its table.
describe('Transactions notes a reader needs, above the table (readability B5)', () => {
  const precedes = (a: Element, b: Element) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

  it('says how to read the Value column before the committed table', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [
          {
            hash: `0x${'cd'.repeat(32)}`,
            blockNumber: 123456,
            blockHash: `0x${'ef'.repeat(32)}`,
            index: 1,
            inputsCount: 2,
            outputsCount: 3,
            fee: '100000000',
            value: '123456789',
            txSize: 226,
            cycles: 1000000,
            cyclesStatus: 'complete',
            activity: feedSamples.eightParties,
            isCellbase: false,
            timestamp: '2026-02-20T00:00:00Z',
          },
        ],
        total: 1,
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    render(<TransactionsPage />);
    const table = await screen.findByRole('table', { name: 'Committed transactions' });
    const note = screen.getByText(/^Value is what the participants received\./);
    expect(precedes(note, table)).toBe(true);
  });

  it('says how to read the pool before its table', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      truncated: true,
      total: 1,
      totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'proposed',
          value: null,
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '1709221',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'partial', reasons: [{ code: 'unresolved_input' }] },
          activity: null,
        },
      ],
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    for (const note of [
      screen.getByText(/^Newest first, 50 a page\. A proposed transaction enters a block/),
      screen.getByText(/^This node's tracked pool is truncated/),
    ])
      expect(precedes(note, table)).toBe(true);
  });
});

describe('Transactions chrome on a phone (board L2, R2)', () => {
  afterEach(() => vi.unstubAllGlobals());

  // Re-pinned on purpose (controls lane C2): the kinds are the controls board's filter chips, and
  // their phone form is the chips' own: below sm a select after "kind" (eight kinds are wider than
  // a 360 px window's 328 px line), with yesterday's counts and the same words and order; the
  // pool is the views' tab at every width, its count after it. It was the page's own line below
  // sm, a select ("All kinds", "Tokens 11") beside a "pool · 4 pending" toggle, the chips, the
  // pool banner and the view tabs hidden there by their classes.
  it('folds the kind chips into one select after "kind" below sm; the pool is the views’ tab', async () => {
    atViewportWidth(390);
    const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
    vi.mocked(api.getDailyActivityStats).mockResolvedValue([
      { ...dailyActivities[0], date: yesterday, tokenCount: 11, coinbaseCount: 3 },
    ]);
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        nextCursor: null,
        hasMore: false,
        total: 0,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      totals: { pending: 4, proposed: 1, awaitingIndex: 0 },
    });
    render(<TransactionsPage />);
    const select = await screen.findByRole('combobox', { name: 'kind' });
    await waitFor(() =>
      expect(within(select).getByRole('option', { name: /^tokens/ })).toHaveTextContent('tokens 11')
    );
    // The same kinds as the chips, in the same order and words.
    expect(
      within(select)
        .getAllByRole('option')
        .map((o) => o.getAttribute('value'))
      // re-pinned on purpose (lists lane P3): the cellbases are a tab of their own, no kind
    ).toEqual(['all', 'ckb', 'script', 'dao', 'token', 'object', 'identity']);
    expect(within(select).getAllByRole('option')[0].textContent).toMatch(/^all/);
    expect(screen.queryByRole('radiogroup', { name: 'kind' })).toBeNull();
    // Re-pinned on purpose (lane D4, PA-20): the pool's strip keeps its figures below sm (it was
    // hidden there whole, `hidden sm:flex`); its "Open the pool ↓" (it was "→") stays from sm, and the views'
    // tab row is drawn at every width.
    expect(screen.getByTestId('pool-banner')).not.toHaveClass('hidden');
    expect(
      within(screen.getByTestId('pool-banner')).getByRole('button', { name: 'Open the pool ↓' })
    ).toHaveClass('hidden', 'sm:inline-flex');
    expect(screen.getByRole('tablist', { name: 'Transaction views' })).toBeInTheDocument();
    fireEvent.change(select, { target: { value: 'token' } });
    // a phone's window, so a phone's twenty a page (R2; the test drew the phone line in a
    // desktop's window, which asked for fifty)
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 20,
        activityType: 'token',
      })
    );
    // re-pinned on purpose (lists lane P3): the tab names the list and counts nothing (its count
    // summed pending and proposed in the page; the pool's line states each)
    const pool = screen.getByRole('tab', { name: 'in the pool' });
    expect(pool).toHaveAttribute('aria-selected', 'false');
    fireEvent.click(pool);
    expect(pool).toHaveAttribute('aria-selected', 'true');
    expect(await screen.findByText('not yet in a block · newest first')).toBeInTheDocument();
  });

  it('asks for twenty transactions a page below md', async () => {
    stubViewportFromMd(false);
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 20,
        nextCursor: null,
        hasMore: false,
        total: 0,
      })
    );
    render(<TransactionsPage />);
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({
        cursor: undefined,
        limit: 20,
        // re-pinned on purpose (lists lane P3): the user transactions
        activityType: 'user',
      })
    );
    expect(api.getTransactions).not.toHaveBeenCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'user',
    });
  });
});

// Hierarchy D11 (ruling R7): "DAO transactions →" on /dao links /transactions?kind=dao, so the
// page starts on the kind its URL names and writes a chosen kind back (replace, not push).
describe('Transactions kind in the URL (D11)', () => {
  beforeEach(() => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        nextCursor: null,
        hasMore: false,
        total: 0,
      })
    );
  });

  function renderAt(url: string) {
    const router = createMemoryRouter([{ path: '*', element: <TransactionsPage /> }], {
      initialEntries: [url],
    });
    render(<RouterProvider router={router} />);
    return router;
  }
  // Re-pinned on purpose (controls lane C2): the kind filter is the controls board's chips, a
  // radio group named "kind", a radio per kind named by its lowercase words, the chosen one
  // aria-checked (it was the aria-pressed group "Filter by kind, with yesterday's counts": "Nervos
  // DAO", "All", "Tokens", "Objects").
  const chip = (name: string) =>
    within(screen.getByRole('radiogroup', { name: 'kind' })).getByRole('radio', { name });

  it('starts on the kind its URL names and asks for it (?kind=dao)', async () => {
    renderAt('/mainnet/transactions?kind=dao');
    expect(chip('nervos dao')).toHaveAttribute('aria-checked', 'true');
    expect(chip('all')).toHaveAttribute('aria-checked', 'false');
    // The phone's form of the filter, the select after "kind", names the same kind (controls
    // lane C2: it was the page's own select, always drawn and hidden from sm by its class).
    atViewportWidth(390);
    renderAt('/mainnet/transactions?kind=dao');
    expect(screen.getByRole('combobox', { name: 'kind' })).toHaveValue('dao');
    vi.unstubAllGlobals();
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'dao',
      })
    );
    expect(api.getTransactions).not.toHaveBeenCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'user',
    });
  });

  it.each(['?kind=interest', ''])(
    'starts on All when the URL names an unknown kind or none (%s)',
    async (search) => {
      renderAt(`/mainnet/transactions${search}`);
      expect(chip('all')).toHaveAttribute('aria-checked', 'true');
      await waitFor(() =>
        expect(api.getTransactions).toHaveBeenCalledWith({
          cursor: undefined,
          limit: 50,
          // re-pinned on purpose (lists lane P3): the user transactions
          activityType: 'user',
        })
      );
    }
  );

  it('writes a chosen kind back to the URL by replace, and All drops it', async () => {
    const router = renderAt('/mainnet/transactions?kind=dao');
    fireEvent.click(chip('tokens'));
    await waitFor(() => expect(router.state.location.search).toBe('?kind=token'));
    expect(router.state.location.pathname).toBe('/mainnet/transactions');
    expect(router.state.historyAction).toBe('REPLACE');
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'token',
      })
    );
    fireEvent.click(chip('all'));
    await waitFor(() => expect(router.state.location.search).toBe(''));
    expect(chip('all')).toHaveAttribute('aria-checked', 'true');
  });

  // The page-phase review's SHOULD 6 (the P3 follow-up): the cellbases tab left a chosen kind
  // pressed, and in the URL, over a list of every cellbase, which no kind filters (no cellbase is a
  // DAO transaction). Opening the cellbases sets the kind back to "all", here and in the URL.
  it('sets the kind back to all, in the URL too, when the cellbases are opened', async () => {
    const router = renderAt('/mainnet/transactions?kind=dao');
    expect(chip('nervos dao')).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(screen.getByRole('tab', { name: /^cellbases/ }));
    await waitFor(() => expect(router.state.location.search).toBe(''));
    expect(router.state.historyAction).toBe('REPLACE');
    expect(chip('all')).toHaveAttribute('aria-checked', 'true');
    expect(chip('nervos dao')).toHaveAttribute('aria-checked', 'false');
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'cellbase',
      })
    );
  });

  it('follows the URL when it changes under the mounted page', async () => {
    const router = renderAt('/mainnet/transactions?kind=dao');
    expect(chip('nervos dao')).toHaveAttribute('aria-checked', 'true');
    await act(async () => {
      await router.navigate('/mainnet/transactions?kind=object');
    });
    expect(chip('objects')).toHaveAttribute('aria-checked', 'true');
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'object',
      })
    );
  });
});

// The controls lane (C2, c2-lists), written before the page's chips, tabs and pagers became the
// controls board's: what each of them asks for, so the new controls are proven against it.
describe('Transactions paging and kind (what the controls ask for)', () => {
  const tx = (n: number) => ({
    hash: `0x${n.toString(16).padStart(64, '0')}`,
    blockNumber: n,
    blockHash: `0x${'bb'.repeat(32)}`,
    index: 1,
    inputsCount: 2,
    outputsCount: 3,
    fee: '100000000',
    value: '123456789012',
    txSize: 226,
    cycles: 1000000,
    cyclesStatus: 'complete' as const,
    activity: feedSamples.eightParties,
    isCellbase: false,
    timestamp: '2026-02-20T00:00:00Z',
  });
  const committed = (from: number, nextCursor: string | null) =>
    pagedRead({
      data: [tx(from), tx(from - 1)],
      total: 120,
      // a full page of its rows with more after it (lane cd-fixes-controls, SHOULD-5)
      limit: 2,
      hasMore: nextCursor !== null,
      nextCursor,
    });
  const block = async (n: number) =>
    within(await screen.findByRole('table', { name: 'Committed transactions' })).findByRole(
      'link',
      { name: `#${n.toLocaleString('en-US')}` }
    );

  it('walks the committed list older by its served cursor and newer back to the page before', async () => {
    vi.mocked(api.getTransactions).mockImplementation(async ({ cursor } = {}) =>
      cursor === 'c2' ? committed(500, null) : committed(900, 'c2')
    );
    render(<TransactionsPage />);
    expect(await block(900)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^older/i }));
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: 'c2',
        limit: 50,
        // re-pinned on purpose (lists lane P3): the user transactions
        activityType: 'user',
      })
    );
    expect(await block(500)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /newer$/i }));
    expect(await block(900)).toBeInTheDocument();
  });

  it('starts the committed list again from its newest when a kind is chosen', async () => {
    vi.mocked(api.getTransactions).mockImplementation(async ({ cursor } = {}) =>
      cursor === 'c2' ? committed(500, null) : committed(900, 'c2')
    );
    render(<TransactionsPage />);
    expect(await block(900)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^older/i }));
    expect(await block(500)).toBeInTheDocument();
    // the kind filter's radio (controls lane C2, the same commit: it was the button "Tokens")
    fireEvent.click(screen.getByRole('radio', { name: /^tokens/ }));
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'token',
      })
    );
  });

  it('walks the pool older by its served cursor', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(committed(900, null));
    const poolRow = (n: number) => ({
      hash: `0x${n.toString(16).padStart(64, '0')}`,
      status: 'pending' as const,
      value: '100000000',
      fee: '1000',
      feeRate: '1000',
      txSize: 226,
      cycles: '1000',
      inputsCount: 1,
      outputsCount: 2,
      timestamp: '1792886400000',
      interpretation: { status: 'complete' as const },
      activity: null,
    });
    // the pool's pages as its route sends them: full at the 50 asked where more follow, 51 in
    // all (lane cd-fixes-controls, review-fix-tests SHOULD-3: page one was one row at 50 a page that
    // named a next cursor, a page the route cannot send: it pages by its limit)
    vi.mocked(api.getPoolTransactions).mockImplementation(async ({ cursor } = {}) =>
      poolPagedRead(
        {
          ...emptyPool,
          data:
            cursor === 'p2' ? [poolRow(51)] : Array.from({ length: 50 }, (_, i) => poolRow(i + 1)),
          total: 51,
          nextCursor: cursor === 'p2' ? null : 'p2',
          totals: { pending: 51, proposed: 0, awaitingIndex: 0 },
        },
        cursor === 'p2' ? 50 : 0
      )
    );
    render(<TransactionsPage />);
    fireEvent.click(await screen.findByRole('tab', { name: /^in the pool/ }));
    const pool = await screen.findByRole('table', { name: 'Transactions in the pool' });
    expect(within(pool).getAllByTitle(poolRow(1).hash).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole('button', { name: /^older/i }));
    await waitFor(() =>
      expect(api.getPoolTransactions).toHaveBeenCalledWith({ cursor: 'p2', limit: 50 })
    );
    await waitFor(() =>
      expect(
        within(screen.getByRole('table', { name: 'Transactions in the pool' })).getAllByTitle(
          poolRow(51).hash
        ).length
      ).toBeGreaterThan(0)
    );
  });
});

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('TransactionsPage while its read is paused', () => {
  it('says it is loading, not nothing; a real empty answer still says so once back online', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        total: 0,
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    const { resume } = renderPaused(<TransactionsPage />);
    expect(await screen.findByText('Loading transactions…')).toHaveAttribute('role', 'status');
    expect(screen.queryByText('No transactions.')).toBeNull();
    resume();
    expect(await screen.findByText('No transactions.')).toBeInTheDocument();
    expect(screen.queryByText('Loading transactions…')).toBeNull();
  });
});

// Lane D4 (the phone-rows board, built from its transactions pane): a pool transaction's phone row
// is three parts, as a committed one's: the hash with its status and wait; what it does; then,
// quiet, the cells, the value and the fee with its rate, each figure after its name (PA-09). Bytes
// and cycles leave the phone row (the transaction's page, the table, the cards from 560 px). And
// the strip above the list keeps its figures on a phone (PA-20: below sm it was hidden whole and
// the page said nothing of the pool but its tab's count); its "Open the pool ↓" (it was "→") stays from sm, as
// the views' tab row opens the pool at every width.
describe('the pool on a phone (lane D4, PA-20)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const pool = (value: string | null) =>
    poolPagedRead({
      summary: {
        size: '226',
        cycles: '1709221',
        minFeeRate: '54623',
        maxFeeRate: '54623',
        newestSeenAt: '1792886400000',
      },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'proposed' as const,
          value,
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '1709221',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation:
            value === null
              ? { status: 'partial' as const, reasons: [{ code: 'unresolved_input' as const }] }
              : { status: 'complete' as const, reasons: [] },
          activity: null,
        },
      ],
      total: 1,
      limit: 50,
      nextCursor: null,
      totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
      truncated: false,
      lastPolledAt: '2026-10-25T00:00:00Z',
    });

  it('is three parts at 358 px: hash and status, what it does, then cells · value · fee · rate', async () => {
    atContainerWidth(358);
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue(pool('76729874184'));
    render(<TransactionsPage />);
    // the view through the row's form on this line: a stubbed 358 px line under a viewport from
    // 640 holds no 388.4 px command row, its Select (the lead's final round, item 4)
    await chooseView(/^in the pool/);
    const [row] = await screen.findAllByTestId('pool-row-mobile');
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xabab…abab', expect.stringMatching(/^Proposed· .+ ago$/)],
      ['what', 'Interpretation unavailable', null],
      [
        'figures',
        '2 → 3\u00a0cells · value 767.29 CKB · fee 0.00012345 CKB · fee rate 54,623 shannon/KB',
        null,
      ],
    ]);
    const figures = row.querySelector('[data-line="figures"]')!;
    expect(dottedItems(figures)).toEqual([
      '2 → 3\u00a0cells',
      'value 767.29 CKB',
      'fee 0.00012345 CKB',
      'fee rate 54,623 shannon/KB',
    ]);
    for (const label of ['value', 'fee', 'fee rate'])
      expect(
        Array.from(figures.querySelectorAll('[data-role="figure-label"]')).map((l) => l.textContent)
      ).toContain(label);
    expect(row.textContent).not.toMatch(/226 B|cycles/);
    // the stone, the tight hash, and the widest status with the widest wait under 100 days
    expect(statedHeadPx(row)).toBe(311.9);
    expect(phoneHeadPx(row, 360)).toBeLessThanOrEqual(statedHeadPx(row));
  });

  it('says a value it cannot read in the value’s place, and the strip keeps its figures', async () => {
    atViewportWidth(390);
    atContainerWidth(358);
    vi.mocked(api.getTransactions).mockResolvedValue(
      pagedRead({
        data: [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    vi.mocked(api.getPoolTransactions).mockResolvedValue(pool(null));
    render(<TransactionsPage />);
    const banner = await screen.findByTestId('pool-banner');
    // drawn below sm: the strip, its squares and its counts
    expect(banner).not.toHaveClass('hidden');
    expect(banner).toHaveTextContent('0 pending · 1 proposed · 0 awaiting index');
    expect(within(banner).getAllByRole('link', { name: /^Pool transaction / })).toHaveLength(1);
    // its way into the pool stays from sm: below it the views' tab row is that way
    expect(within(banner).getByRole('button', { name: 'Open the pool ↓' })).toHaveClass(
      'hidden',
      'sm:inline-flex'
    );
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const [row] = await screen.findAllByTestId('pool-row-mobile');
    expect(dottedItems(row.querySelector('[data-line="figures"]')!)).toEqual([
      '2 → 3\u00a0cells',
      'Interpretation partial',
      'fee 0.00012345 CKB',
      'fee rate 54,623 shannon/KB',
    ]);
  });
});

// The lists lane P3 (plan §7.6): the page opens on the user transactions, the cellbases one tab
// away, a list of their own with their own words; a kind chosen there is a kind of the user
// transactions, so the page shows them.
describe('Transactions: the cellbases, one tab away (lists lane P3)', () => {
  const cellbase = {
    hash: `0x${'c8'.repeat(32)}`,
    blockNumber: 21054100,
    blockHash: `0x${'bb'.repeat(32)}`,
    index: 0,
    inputsCount: 1,
    outputsCount: 1,
    fee: '0',
    value: '0',
    outputCapacity: '56136576541',
    txSize: 360,
    cycles: 0,
    cyclesStatus: 'complete' as const,
    activity: null,
    isCellbase: true,
    timestamp: '2026-11-19T00:00:00Z',
    pays: { number: 21054089, hash: `0x${'89'.repeat(32)}` },
  };

  it('asks for the cellbases on their tab, lists them in their own words, and back on a kind', async () => {
    vi.mocked(api.getTransactions).mockImplementation(async (params = {}) =>
      pagedRead({
        data: params.activityType === 'cellbase' ? [cellbase] : [],
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })
    );
    render(<TransactionsPage />);
    expect(screen.getByRole('tab', { name: 'transactions' })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    fireEvent.click(screen.getByRole('tab', { name: 'cellbases' }));
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'cellbase',
      })
    );
    const table = await screen.findByRole('table', { name: 'Cellbases' });
    expect(within(table).getAllByRole('row')[1]).toHaveTextContent('reward for #21,054,089');
    expect(screen.getByText('one in every block · newest first')).toBeInTheDocument();
    // a kind is of the user transactions: chosen here, the page shows them
    fireEvent.click(screen.getByRole('radio', { name: /^tokens/ }));
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: 'transactions' })).toHaveAttribute(
        'aria-selected',
        'true'
      )
    );
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'token',
      })
    );
  });
});
