import { CHARTS } from '@/components/charts/chart-catalogue';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { afterEach, describe, it, expect, vi, beforeEach, onTestFinished } from 'vitest';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { expectBefore, expectOneRowSeal } from '@/__tests__/utils/seals';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { render } from '@/__tests__/utils/test-utils';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';
import { dailyActivities, feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionsPage from '@/app/transactions/page';
import { api } from '@/lib/api';
import { formatTimeAgo, formatUtc8DateTime } from '@/lib/utils';
import { poolStatusLabel } from '@/components/ui/pool-status';
import { TRANSACTION_LIST_MIN_PX } from '@/components/transaction/transaction-list-table';
import { cellFaults, faultLines } from '@/__tests__/utils/list-table-cells';
import { CLOCK_BEHIND } from '@/lib/format-age';

// D11: the real navigation (setup.ts stubs it with an empty URL), so a router's location reaches
// the page; outside a router it reads window.location, as the stub's empty URL did.
vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', async () => ({
  ...(await vi.importActual('@/lib/api')),
  api: {
    getTransactions: vi.fn(),
    getPoolTransactions: vi.fn(),
    getDailyActivityStats: vi.fn(),
    getTxStats: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const emptyPool = {
  summary: { size: '0', cycles: '0', minFeeRate: null, maxFeeRate: null, newestSeenAt: null },
  data: [],
  total: 0,
  limit: 50,
  nextCursor: null,
  totals: { pending: 0, proposed: 0, awaitingIndex: 0 },
  truncated: false,
  lastPolledAt: null,
};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getPoolTransactions).mockResolvedValue(emptyPool);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 0,
    hourlyData: [],
    dailyData: [],
  });
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
    vi.mocked(api.getTransactions).mockResolvedValue({
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
    });

    render(<TransactionsPage />);

    // Lane L: the "Transactions" chart link now shares the h1's word, so the h1 is named by role.
    expect(screen.getByRole('heading', { level: 1, name: 'Transactions' })).toBeInTheDocument();
    expect(
      screen.queryByText('Browse all transactions on the CKB network')
    ).not.toBeInTheDocument();

    await waitFor(() => {
      expect(api.getTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
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
    for (const hashDisplay of screen.getAllByTitle(hash)) {
      expect(hashDisplay).toHaveClass('text-text-bright', 'text-body', 'leading-row');
      expect(hashDisplay).not.toHaveClass('text-small');
      expect(hashDisplay.children).toHaveLength(0);
      expect(hashDisplay.closest('a')).toHaveAttribute('href', `/mainnet/tx/${hash}`);
    }
    expect(screen.queryByTitle(/Click to copy/)).not.toBeInTheDocument();
    expect(screen.getAllByText('2 → 3')).toHaveLength(1);
    // F2-tx (TX-25 / TX-26): the board's columns, newest first with cellbase rows set back.
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(table).toHaveTextContent('What it did');
    expect(screen.getByText('newest first · cellbase rows set back')).toBeInTheDocument();
    // Re-pinned on purpose (readability B5): this fixture is one page (no older, no newer), and
    // one page draws no pager (it drew a disabled "Older →").
    expect(screen.queryByRole('button', { name: 'Older →' })).toBeNull();
  });
});

describe('Transactions pool view', () => {
  it('keeps full pool evidence inside the desktop width budget without shortening exact amounts', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    // re-pinned with readability R4 (lane F3, step 2): the table text 13 px (7.8 a glyph, was 7.2), the hash 14 px (8.4, was 7.8): the table is drawn from 1,272 px (was within 1,248)
    // re-pinned on purpose (readability F3-e): every column its content, 1,220 px, which the
    // widest 1,232 px container holds
    // re-pinned on purpose (review fb-tests 6, the lead's ruling): Cells holds the most inputs a transaction holds, "13568 → 9999" (102 → 110): 1,228 px
    // re-pinned on purpose (readability R6): the transaction's row stone 24 × 48.5 → 16 × 32.3 px, its footprint 56.5 → 40.3, Transaction 216 → 200: 1,212 px
    expect(Number.parseFloat(table.style.minWidth)).toBe(1212);
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
    expect(cols.reduce((sum, w) => sum + w, 0)).toBe(1212);
    expect(Number.parseFloat(table.style.minWidth)).toBe(1212);
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
      [1212, 'table'],
      [1211, 'rows'],
      [976, 'rows'],
    ] as const) {
      vi.mocked(api.getPoolTransactions).mockResolvedValue(poolPage);
      atContainerWidth(width);
      const { unmount } = render(<TransactionsPage />);
      fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
  it('changes both lists’ form at the larger sum: the committed What it did keeps 1 px', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
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
    });
    render(<TransactionsPage />);
    const table = await screen.findByRole('table', { name: 'Committed transactions' });
    expect(Number.parseFloat(table.style.minWidth)).toBe(1212);
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(Number.parseFloat(cols[2].minWidth)).toBe(204 + 1);
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
    expect(budgets.reduce((sum, w) => sum + w, 0)).toBe(1212);
    // re-pinned on purpose (the coordinator's ruling: the committed Size head in the tick form):
    // the committed list's own sum is 1,203, the pool's 1,220 the larger, so both change form at
    // the pool's (the committed What it did keeps the 17 px between them); re-pinned on purpose
    // (reviews fb-tests 6, fb-logic 6): 1,227 and 1,228, What it did keeps 1 px; re-pinned on
    // purpose (readability R6): 1,211 and 1,212, What it did keeps 1 px
    expect(TRANSACTION_LIST_MIN_PX).toBe(1211);
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    const cols = Array.from(table.querySelectorAll('col')).map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(cols[1].width).toBe('auto');
    expect(Number.parseFloat(table.style.minWidth)).toBe(1212);
    unmount();
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    for (const [width, form] of [
      [1232, 'table'],
      [1212, 'table'],
      [1211, 'rows'],
      [608, 'rows'],
      [358, 'rows'],
    ] as const) {
      vi.mocked(api.getPoolTransactions).mockResolvedValue(poolPage);
      atContainerWidth(width);
      const shown = render(<TransactionsPage />);
      fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
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
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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
    for (const hash of screen.getAllByTitle(`0x${'ab'.repeat(32)}`)) {
      expect(hash).toHaveClass('text-text-bright', 'text-body', 'leading-row');
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
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
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
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
    const [row] = await screen.findAllByTestId('pool-row-mobile');
    expect(screen.queryByRole('table', { name: 'Transactions in the pool' })).toBeNull();
    expect(phoneLines(row).map((l) => [l.key, l.left, l.right])).toEqual([
      ['identity', '0xabab…abab', expect.stringMatching(/^Proposed· .+ ago$/)],
      [
        'what',
        'Interpretation unavailablepartial: an input is not yet resolvable',
        'Interpretation partial',
      ],
      [
        'figures',
        'fee 0.00012345 CKB',
        '54,623 shannon/KB · 2 → 3\u00a0cells · 226 B · 1,709,221 cycles',
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
    },
  ]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 500,
    hourlyData: [],
    dailyData: [{ label: yesterday.slice(5).replace('-', '/'), value: 20, complete: true }],
  });
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [],
    limit: 50,
    nextCursor: null,
    hasMore: false,
    total: 0,
  });
  render(<TransactionsPage />);
  expect(await screen.findByRole('button', { name: 'All' })).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'All' })).toHaveTextContent('All20')
  );
  expect(screen.getByRole('button', { name: 'Nervos DAO' })).toHaveTextContent(/^Nervos DAO3$/);
  expect(screen.getByText(/DAO counts actions/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Tokens' })).toHaveTextContent('11');
  expect(screen.getByRole('button', { name: 'Cellbase' })).toHaveTextContent('3');
  expect(
    screen
      .getByTestId('pool-banner')
      .compareDocumentPosition(screen.getByRole('group', { name: 'Transaction views' })) &
      Node.DOCUMENT_POSITION_FOLLOWING
  ).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));
  await waitFor(() =>
    expect(api.getTransactions).toHaveBeenLastCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'token',
    })
  );
  fireEvent.click(screen.getByRole('button', { name: 'Open the pool →' }));
  await waitFor(() =>
    expect(api.getPoolTransactions).toHaveBeenCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'token',
    })
  );
  expect(screen.getByRole('link', { name: 'Activities →' })).toHaveAttribute(
    'href',
    '/mainnet/activities'
  );
});
it('does not infer a transaction total from kind counts when yesterday is incomplete', async () => {
  const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    { ...dailyActivities[0], date: yesterday },
  ]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 500,
    hourlyData: [],
    dailyData: [{ label: yesterday.slice(5).replace('-', '/'), value: 999, complete: false }],
  });
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [],
    limit: 50,
    nextCursor: null,
    hasMore: false,
  });
  render(<TransactionsPage />);
  expect(
    await screen.findByText(`No complete transaction count for ${yesterday} (UTC+8).`)
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'All' })).toHaveTextContent(/^All$/);
});

it('draws awaiting-index pool squares hollow, apart from pending (cyan) and proposed (gold) (TX-38)', async () => {
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [],
    limit: 50,
    hasMore: false,
    nextCursor: null,
  });
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
  expect(screen.getByRole('button', { name: /^In the pool/ })).toHaveTextContent('In the pool2');
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
    vi.mocked(api.getTransactions).mockResolvedValue({
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
    });
    render(<TransactionsPage />);
    const table = await screen.findByRole('table', { name: 'Committed transactions' });
    const note = screen.getByText(/^Value is what the participants received\./);
    expect(precedes(note, table)).toBe(true);
  });

  it('says how to read the pool before its table', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
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
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
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

  it('folds the kind chips into one select with yesterday’s counts beside a pool chip', async () => {
    const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
    vi.mocked(api.getDailyActivityStats).mockResolvedValue([
      { ...dailyActivities[0], date: yesterday, tokenCount: 11, coinbaseCount: 3 },
    ]);
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      nextCursor: null,
      hasMore: false,
      total: 0,
    });
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      totals: { pending: 4, proposed: 1, awaitingIndex: 0 },
    });
    render(<TransactionsPage />);
    const line = await screen.findByTestId('kind-pool-line');
    expect(line).toHaveClass('sm:hidden');
    const select = within(line).getByLabelText("Filter by kind, with yesterday's counts");
    await waitFor(() =>
      expect(within(select).getByRole('option', { name: /^Tokens/ })).toHaveTextContent('Tokens 11')
    );
    // The same kinds as the chips, in the same order (the board's "All kinds ▾").
    expect(
      within(select)
        .getAllByRole('option')
        .map((o) => o.getAttribute('value'))
    ).toEqual(['all', 'ckb', 'script', 'cellbase', 'dao', 'token', 'object', 'identity']);
    expect(within(select).getAllByRole('option')[0].textContent).toMatch(/^All kinds/);
    // From sm the chips, the pool banner and the view tabs; below it the line replaces them.
    expect(
      screen.getByRole('group', { name: "Filter by kind, with yesterday's counts" })
    ).toHaveClass('hidden', 'sm:flex');
    expect(screen.getByTestId('pool-banner')).toHaveClass('hidden', 'sm:flex');
    expect(screen.getByRole('group', { name: 'Transaction views' })).toHaveClass(
      'hidden',
      'sm:flex'
    );
    fireEvent.change(select, { target: { value: 'token' } });
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'token',
      })
    );
    const pool = within(line).getByRole('button', { name: 'pool · 4 pending' });
    expect(pool).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(pool);
    expect(pool).toHaveAttribute('aria-pressed', 'true');
    expect(await screen.findByText('not yet in a block · newest first')).toBeInTheDocument();
  });

  it('asks for twenty transactions a page below md', async () => {
    stubViewportFromMd(false);
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 20,
      nextCursor: null,
      hasMore: false,
      total: 0,
    });
    render(<TransactionsPage />);
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 20 })
    );
    expect(api.getTransactions).not.toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
  });
});

// Hierarchy D11 (ruling R7): "DAO transactions →" on /dao links /transactions?kind=dao, so the
// page starts on the kind its URL names and writes a chosen kind back (replace, not push).
describe('Transactions kind in the URL (D11)', () => {
  beforeEach(() => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      nextCursor: null,
      hasMore: false,
      total: 0,
    });
  });

  function renderAt(url: string) {
    const router = createMemoryRouter([{ path: '*', element: <TransactionsPage /> }], {
      initialEntries: [url],
    });
    render(<RouterProvider router={router} />);
    return router;
  }
  const chip = (name: string) =>
    within(
      screen.getByRole('group', { name: "Filter by kind, with yesterday's counts" })
    ).getByRole('button', { name });

  it('starts on the kind its URL names and asks for it (?kind=dao)', async () => {
    renderAt('/mainnet/transactions?kind=dao');
    expect(chip('Nervos DAO')).toHaveAttribute('aria-pressed', 'true');
    expect(chip('All')).toHaveAttribute('aria-pressed', 'false');
    expect(
      screen.getByLabelText("Filter by kind, with yesterday's counts", { selector: 'select' })
    ).toHaveValue('dao');
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'dao',
      })
    );
    expect(api.getTransactions).not.toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
  });

  it.each(['?kind=interest', ''])(
    'starts on All when the URL names an unknown kind or none (%s)',
    async (search) => {
      renderAt(`/mainnet/transactions${search}`);
      expect(chip('All')).toHaveAttribute('aria-pressed', 'true');
      await waitFor(() =>
        expect(api.getTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 50 })
      );
    }
  );

  it('writes a chosen kind back to the URL by replace, and All drops it', async () => {
    const router = renderAt('/mainnet/transactions?kind=dao');
    fireEvent.click(chip('Tokens'));
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
    fireEvent.click(chip('All'));
    await waitFor(() => expect(router.state.location.search).toBe(''));
    expect(chip('All')).toHaveAttribute('aria-pressed', 'true');
  });

  it('follows the URL when it changes under the mounted page', async () => {
    const router = renderAt('/mainnet/transactions?kind=dao');
    expect(chip('Nervos DAO')).toHaveAttribute('aria-pressed', 'true');
    await act(async () => {
      await router.navigate('/mainnet/transactions?kind=object');
    });
    expect(chip('Objects')).toHaveAttribute('aria-pressed', 'true');
    await waitFor(() =>
      expect(api.getTransactions).toHaveBeenLastCalledWith({
        cursor: undefined,
        limit: 50,
        activityType: 'object',
      })
    );
  });
});

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('TransactionsPage while its read is paused', () => {
  it('says it is loading, not nothing; a real empty answer still says so once back online', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    const { resume } = renderPaused(<TransactionsPage />);
    expect(await screen.findByText('Loading transactions…')).toHaveAttribute('role', 'status');
    expect(screen.queryByText('No transactions.')).toBeNull();
    resume();
    expect(await screen.findByText('No transactions.')).toBeInTheDocument();
    expect(screen.queryByText('Loading transactions…')).toBeNull();
  });
});
