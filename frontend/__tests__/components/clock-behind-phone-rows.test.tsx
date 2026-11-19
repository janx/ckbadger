import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import { containerContentPx } from '@/__tests__/utils/tailwind-box';
import { lineRuns } from '@/__tests__/utils/line-runs';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { TransactionListTable } from '@/components/transaction/transaction-list-table';
import { AssetTransactions } from '@/components/asset/asset-feed';
import TransactionsPage from '@/app/transactions/page';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { api, type Transaction } from '@/lib/api';
import { CLOCK_BEHIND } from '@/lib/format-age';
import { pagedRead } from '@/__tests__/utils/paged-read';

// The pool's phone row is drawn by the page (it is not exported): the page's reads are stubbed.
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

/**
 * Review fb-logic 2 (the lead's ruling, lane rd/fb-fixes-2 L2): "this device's clock is behind" is
 * words and breaks between them wherever it stands; only a figure is one unbreakable unit (the
 * block number, a real age). Three phone rows held their whole identity right part in one
 * `whitespace-nowrap` run, the sentence with it, and PhoneRow's `max-w-full` bounds the part's box,
 * not its unbreakable content: in Chrome with the device clock two minutes behind, /transactions
 * was 18 px wider than a 390 px viewport and 48 px wider than a 360 px one.
 *
 * The property, read from each rendered row: every run of the identity line's right part that a
 * line cannot break inside fits the row's line. The line is the list's container (the page's
 * content width, 358 at 390 and 328 at 360, Tailwind's container less its 16 px gutters) less the
 * row stone's footprint where the row has one (a transaction's slab, 32.3 px and its 8 px gap:
 * 317.7 at 390, 287.7 at 360; re-pinned on purpose, readability R6: beside the 24 px slab, 48.5
 * and its gap, they were 301.5 and 271.5); the token feed's row has no stone (358, 328).
 *
 * Before the fix, at 13 px (7.8 a glyph), the committed row's right part was one run,
 * "#21,045,845 · this device's clock is behind", 43 glyphs, 335.4 px; the pool's "● Confirming ·
 * this device's clock is behind", the 7 px dot, its 6 px gap, 10 glyphs, the 6 px gap and 31
 * glyphs, 338.8 px; the feed's the committed row's 335.4, past its 328 px line at 360.
 */

/** A block time ten minutes ahead of this device's clock: further than consensus allows (15 s). */
const ahead = () => new Date(Date.now() + 10 * 60_000).toISOString();

const committed = (): Transaction => ({
  hash: `0xcfaebc55${'00'.repeat(25)}e6dd61`,
  blockNumber: 21045845,
  blockHash: `0x${'cd'.repeat(32)}`,
  index: 1,
  inputsCount: 3,
  outputsCount: 3,
  fee: '30000',
  value: '76729874184',
  outputCapacity: '100000000000',
  txSize: 903,
  cycles: 1704957,
  cyclesStatus: 'complete',
  isCellbase: false,
  timestamp: ahead(),
  activity: { ...feedSamples.eightParties, typeCalls: [], protocolActions: [] },
});

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
        // "Confirming", the widest status word
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
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 0,
    hourlyData: [],
    dailyData: [],
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** The identity line's right part of a phone row. */
function identityRight(row: Element): HTMLElement {
  const right = row.querySelector(':scope > [data-line="identity"] > [data-part="right"]');
  if (!right) throw new Error('the row has no identity right part');
  return right as HTMLElement;
}

/** The runs of a right part wider than its line: none, where the line holds it. */
function pastTheLine(right: Element, viewport: number, line: number) {
  return lineRuns(right, viewport)
    .filter((run) => run.px > line + 1e-9)
    .map((run) => `${JSON.stringify(run.text)} ${run.px.toFixed(1)} px in ${line} px`);
}

const VIEWPORTS = [390, 360] as const;

describe('"this device\'s clock is behind" breaks between its words in a phone row (L2)', () => {
  it('holds the line in the 358 / 328 px content widths the arithmetic uses', () => {
    expect(VIEWPORTS.map(containerContentPx)).toEqual([358, 328]);
    // re-pinned on purpose (readability R6): the slab's footprint 56.5 → 40.3 (301.5, 271.5)
    expect(ROW_SEAL.tx.footprint).toBe(40.3);
    expect(VIEWPORTS.map((v) => containerContentPx(v) - ROW_SEAL.tx.footprint)).toEqual([
      317.7, 287.7,
    ]);
  });

  it.each(VIEWPORTS)('in the committed list’s row at %i px', (viewport) => {
    const line = containerContentPx(viewport) - ROW_SEAL.tx.footprint;
    atContainerWidth(containerContentPx(viewport));
    render(<TransactionListTable transactions={[committed()]} />);
    const right = identityRight(screen.getByTestId('transaction-row-mobile'));
    expect(right).toHaveTextContent(`#21,045,845 · ${CLOCK_BEHIND}`);
    expect(pastTheLine(right, viewport, line)).toEqual([]);
  });

  it.each(VIEWPORTS)('in the pool’s row at %i px', async (viewport) => {
    const line = containerContentPx(viewport) - ROW_SEAL.tx.footprint;
    atContainerWidth(containerContentPx(viewport));
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('tab', { name: /^in the pool/ }));
    const [row] = await screen.findAllByTestId('pool-row-mobile');
    const right = identityRight(row);
    expect(right).toHaveTextContent(`Confirming· ${CLOCK_BEHIND}`);
    expect(pastTheLine(right, viewport, line)).toEqual([]);
  });

  it.each(VIEWPORTS)('in a token feed’s row at %i px', (viewport) => {
    const line = containerContentPx(viewport);
    atContainerWidth(line);
    render(<AssetTransactions rows={[committed()]} />);
    const right = identityRight(screen.getByTestId('asset-tx-row-mobile'));
    expect(right).toHaveTextContent(`#21,045,845 · ${CLOCK_BEHIND}`);
    expect(pastTheLine(right, viewport, line)).toEqual([]);
  });
});
