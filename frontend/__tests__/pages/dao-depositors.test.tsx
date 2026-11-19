import { beforeEach, describe, expect, it, vi, onTestFinished } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import DaoDepositorsPage from '@/app/dao/depositors/page';
import { ApiRequestError, api } from '@/lib/api';
import { resourceScale } from '@/components/blocks/block-visual';
import { formatShareTick } from '@/components/ui/share-bar';
import { pagedRead } from '@/__tests__/utils/paged-read';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, api: { ...actual.api, getDaoDepositors: vi.fn() } };
});
vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));

const address =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfzyg3zyg3zyg3zyg3zyg3zyg3zyg3zygssd2hqn';
const row = (rank: number) => ({
  rank,
  lockScriptHash: '0x' + String(rank).padStart(64, '0'),
  address,
  capacity: String(1000 - rank) + '00000000',
  depositCount: rank,
  sharePercent: '1.00',
});
// `limit` is the page size the API answers with: 2 where three depositors take two pages, so a
// fixture holds together (the controls lane's pager refuses figures that do not; it was 50 with a
// second page, which the old pager drew).
const page = (rows: number[], nextCursor: string | null, tip = 20879205, limit = 50, total = 3) =>
  pagedRead({
    anchor: { blockNumber: tip, blockHash: '0x' + 'a'.repeat(64) },
    total,
    limit,
    hasMore: nextCursor !== null,
    nextCursor,
    data: rows.map(row),
  });

describe('DAO depositors page (DAO-12)', () => {
  beforeEach(() => {
    vi.mocked(api.getDaoDepositors).mockReset();
  });

  it('lists every depositor ranked at one tip and pages by rank (L1)', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockResolvedValueOnce(page([1, 2], '2', undefined, 2))
      .mockResolvedValueOnce(page([3], null, undefined, 2));
    render(<DaoDepositorsPage />);
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Nervos DAO depositors');
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    expect(table.querySelectorAll('tbody tr')).toHaveLength(2);
    expect(table.querySelector('tbody tr')).toHaveTextContent('999.00000000');
    const rankedAt = screen.getByText((_, el) => el?.getAttribute('data-role') === 'ranked-at');
    expect(rankedAt).toHaveTextContent('3 depositors ranked by CKB deposited at block #20,879,205');
    // Lane L: the block the ranking is taken at is a link.
    expect(within(rankedAt).getByRole('link', { name: '#20,879,205' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20879205'
    );
    expect(api.getDaoDepositors).toHaveBeenCalledWith({ limit: 50, cursor: undefined });
    // Re-pinned on purpose (controls lane C2): the controls board's pager, "next →" and "← prev"
    // in words (they were the buttons "Next 50 →" and "← Previous").
    fireEvent.click(screen.getByRole('button', { name: 'next →' }));
    await waitFor(() =>
      expect(api.getDaoDepositors).toHaveBeenLastCalledWith({ limit: 50, cursor: '2' })
    );
    const next = await screen.findByRole('table', { name: 'DAO depositors' });
    await waitFor(() => expect(next.querySelector('tbody tr')).toHaveTextContent('997.00000000'));
    // Re-pinned on purpose (controls lane C2): at the last page "next →" is quiet words and no
    // control (it was a disabled button).
    const pager = screen.getByRole('navigation', { name: 'depositors' });
    expect(pager.textContent).toBe('3–3 of 3 · 2 a page← prevpage 2 of 2next →');
    expect(within(pager).queryByRole('button', { name: 'next →' })).toBeNull();
    // Back to the page already ranked at the same tip (kept, not re-ranked).
    fireEvent.click(within(pager).getByRole('button', { name: '← prev' }));
    await waitFor(() =>
      expect(
        screen.getByRole('table', { name: 'DAO depositors' }).querySelector('tbody tr')
      ).toHaveTextContent('999.00000000')
    );
    expect(screen.getByRole('button', { name: 'next →' })).toBeInTheDocument();
  });

  it('waits behind the catching-up banner while the store catches up, never the error (contracts S)', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockRejectedValueOnce(
        new ApiRequestError(
          503,
          'store_catching_up',
          "the API's secondary stores have not caught up with the indexer yet: attempt 1 failed"
        )
      )
      .mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    expect(await screen.findByRole('status', { name: 'Catching up' })).toHaveTextContent(
      'attempt 1 failed'
    );
    expect(screen.queryByRole('alert')).toBeNull();
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    expect(table.querySelectorAll('tbody tr')).toHaveLength(2);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it("folds each depositor into board L5's two-line holder row on phones, not a five-fact sheet", async () => {
    atContainerWidth(358);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    const cards = await screen.findByRole('list', { name: 'DAO depositors, compact' });
    const first = within(cards).getAllByRole('listitem')[0];
    expect(first.querySelectorAll('dt')).toHaveLength(0);
    const [row] = within(first).getAllByTestId('holder-row-mobile');
    // Rank inline, the depositor and the CKB deposited; the first row's axis mark; then the share
    // bar with its number, and the deposit count. Re-pinned with ruling R7 (UI review 2026-11-14):
    // the bar is on the page's rounded maximum (1 % for two 1.00 % depositors), not 10 % = full.
    expect(phoneLines(row)).toEqual([
      // U-09 (re-pinned): the phone row's depositor at the tight size, 4…6 (the lead's ruling)
      // a phone row's summary form (readability R5, re-pinned on purpose: was "999.00000000 CKB")
      { key: 'holder', tone: 'head', left: '1ckb1…sd2hqn', right: '999 CKB' },
      { key: 'axis', tone: 'quiet', left: '01%', right: null },
      // re-pinned (A4 round 2): the count a unit, number and noun joined by a plain space
      { key: 'share', tone: 'quiet', left: '1.00%', right: '1 deposit' },
    ]);
    expect(row.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '100%');
  });

  // Table-rows T5: the depositor is the row's one bright thing, at body, on an 18 px line so the
  // row keeps its one-line height; the rank and the share stay dim, the numbers small.
  it('leads each row with the depositor at 13 px bright (T5)', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1], null));
    render(<DaoDepositorsPage />);
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    // U-09 (re-pinned): the table's depositor at the row size, 10…6 (it was 12…7); R9
    // (re-pinned): an address's row size is 4…8
    const depositor = within(table).getByRole('link', { name: 'ckb1…gssd2hqn' });
    expect(depositor).toHaveClass('text-body', 'text-text-bright', 'leading-row');
    const [rank, , deposits, capacity, share] = Array.from(table.querySelectorAll('tbody td'));
    // re-pinned on purpose (the P3 follow-up's 2d, the ink-role gate): a rank is a value, the
    // reading ink (it was the quiet ink)
    expect(rank.firstElementChild).toHaveClass('text-text');
    // R7: the share is a bar on the page's scale with its number beside it. Re-pinned on purpose
    // (BRIEF-ink-size-roles item 1: a figure in a cell is the reading ink): the number was dim.
    expect(share.querySelector('[data-aligned-bar]')).not.toBeNull();
    expect(within(share as HTMLElement).getByText('1.00%')).not.toHaveClass('text-text-dim');
    for (const cell of [rank, deposits, capacity, share])
      expect(cell.querySelector('.text-body')).toBeNull();
  });

  // Readability B5: the page drew its own Previous / Next, the same control as the site's pager
  // without its count; it now pages with the one pager, which draws nothing for one page.
  // Re-pinned on purpose (controls lane C2): the controls board's pager, whose figures are the
  // API's — the first and last rank shown, its total and its page size — "← prev" quiet and no
  // control on the first page (it was "Showing 1–2 of 3 depositors · 50 per page" beside a
  // disabled "← Previous" and a "Next 50 →" button).
  it('pages with the site’s one pager, and draws none for one page (B5)', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], '2', undefined, 2));
    const { unmount } = render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    const pager = screen.getByRole('navigation', { name: 'depositors' });
    expect(pager.textContent).toBe('1–2 of 3 · 2 a page← prevpage 1 of 2next →');
    expect(within(pager).queryByRole('button', { name: '← prev' })).toBeNull();
    expect(within(pager).getByRole('button', { name: 'next →' })).toBeInTheDocument();
    unmount();
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    expect(screen.queryByRole('navigation', { name: 'depositors' })).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
  });

  // Review fb-logic 7 (the lead's ruling): the share head writes "0" and its scale in hundredths
  // of a percent, and the widest tick it writes for any page is five glyphs ("0.75%", "0.01%": a
  // page of depositors holding under 1 % each), wider than the "100%" the track was measured from:
  // "0" and five glyphs a glyph apart at 12 px is 50.4 px, over the 48 a bar keeps at least.
  it('measures the share track from the widest tick its head writes, "0.75%": 50.4 px', async () => {
    const ticks = new Set<string>();
    for (let v = 0; v <= 10_000; v++) ticks.add(formatShareTick(resourceScale([v])));
    const widest = Math.max(...[...ticks].map((t) => t.length));
    expect([widest, ticks.has('0.75%'), '100%'.length]).toEqual([5, true, 4]);
    const track = (1 + 1 + widest) * 7.2;
    expect(track).toBeCloseTo(50.4, 10);
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce({
      ...page([1], null),
      data: [{ ...row(1), sharePercent: '0.72' }],
    });
    render(<DaoDepositorsPage />);
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    const head = within(table).getAllByRole('columnheader')[4];
    const ticksRow = head.querySelector<HTMLElement>('[data-scale-ticks]')!;
    expect(Array.from(ticksRow.children).map((t) => t.textContent)).toEqual(['0', '0.75%']);
    expect(Number.parseFloat(ticksRow.style.width)).toBeCloseTo(track, 10);
    const bar = table.querySelector<HTMLElement>('tbody td:nth-child(5) [data-track]')!;
    expect(Number.parseFloat(bar.style.width)).toBeCloseTo(track, 10);
  });

  // Readability B5: a head stands over what it names; the Share head over the share, not its bar.
  it('stands the Share head over the share, the ticks over the track (B5)', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1], null));
    render(<DaoDepositorsPage />);
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    const name = within(within(table).getAllByRole('columnheader')[4]).getByText('Share');
    expect(name).toHaveClass('justify-end');
    const share = table.querySelectorAll('tbody td')[4] as HTMLElement;
    const track = share.querySelector<HTMLElement>('[data-track]')!;
    const figure = share.querySelector<HTMLElement>('[data-aligned-bar] > :last-child')!;
    expect(figure).toHaveTextContent('1.00%');
    expect(Number.parseFloat(name.style.width)).toBe(
      Number.parseFloat(track.style.width) + 10 + Number.parseFloat(figure.style.width)
    );
  });

  // Readability F3-e: every column budgeted from the widest value it writes (lib/list-bounds.ts),
  // at 0.6 em a glyph (the depositor 14 px, the cells 13, heads and ticks 12) and the 16 px gap
  // after every column but the last: 605 px, so the 608 px band draws the table. Re-pinned on
  // purpose (review fb-logic 7, the lead's ruling): the share's track is 50.4 px, "0" and the
  // widest tick its head writes, "0.75%" (it was measured from "100%", 48), and the column rounds
  // once, track, gap and figure together (the figure's box was rounded to 55 first): 115, 607 px.
  it('budgets every column from what it holds and changes form at their sum (F3-e)', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const budgets = [
      // a rank under a million
      ceil(at('999,999', 13) + 16),
      // a depositor with no address, its lock hash at the row size (an address is 13 characters)
      ceil(at('0x00000000…000001', 14) + 16),
      // the head's word "Deposits" is wider than a count of four digits
      ceil(Math.max(at('Deposits', 12), at('9,999', 13)) + 16),
      // under ten billion CKB, every decimal
      ceil(at('9,999,999,999.99999999', 13) + 16),
      // the track (its widest tick row, "0 0.75%", 7 glyphs at 12 px), the 10 px gap, "100.00%";
      // the last column
      ceil(at('0 0.75%', 12) + 10 + at('100.00%', 13)),
    ];
    expect(budgets).toEqual([71, 159, 74, 188, 115]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(607);
    vi.mocked(api.getDaoDepositors).mockResolvedValue(page([1, 2], null));
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    const { unmount } = render(<DaoDepositorsPage />);
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    const cols = [...table.querySelectorAll('col')].map((c) => (c as HTMLElement).style);
    expect(cols.map((c) => Number.parseFloat(c.minWidth))).toEqual(budgets);
    expect(Number.parseFloat(table.style.minWidth)).toBe(sum);
    unmount();
    for (const [width, form] of [
      [1232, 'table'],
      [608, 'table'],
      [607, 'table'],
      [606, 'rows'],
      [358, 'rows'],
    ] as const) {
      atContainerWidth(width);
      const { container, unmount: drop } = render(<DaoDepositorsPage />);
      await waitFor(() =>
        expect(container.querySelector('[data-list-form]')).toHaveAttribute('data-list-form', form)
      );
      drop();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
      vi.mocked(api.getDaoDepositors).mockResolvedValue(page([1, 2], null));
    }
  });

  // Re-pinned on purpose (readability F3-e): the five columns, each its content, need 605 px, so
  // the 608 px band draws the table; the two-line holder row is the form one pixel under, 604 px
  // (renamed: it was "in the 640 band", where it no longer is the form).
  it('puts a depositor on one line where the five-column table does not fit (604 px)', async () => {
    atContainerWidth(604);
    onTestFinished(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    const [row] = await screen.findAllByTestId('holder-row-mobile');
    // Re-pinned with ruling R7: the first row carries the axis mark over the bar; the bar comes
    // after the amount and its number last, so every bar ends at one x.
    expect(phoneLines(row)).toEqual([
      { key: 'axis', tone: 'quiet', left: '', right: '01%' },
      {
        key: 'holder',
        tone: 'head',
        left: '1ckb1…sd2hqn',
        // a phone row's summary form (readability R5, re-pinned on purpose: was "999.00000000 CKB")
        right: '1\u00a0deposit999 CKB1.00%',
      },
    ]);
  });

  // Re-pinned on purpose (controls lane C2): the page's size is said by the pager's count line,
  // from the size the API answered with (it was the button "Next 20 →").
  it('ranks twenty depositors a page below md and says so on its pager (R2)', async () => {
    stubViewportFromMd(false);
    onTestFinished(() => {
      vi.unstubAllGlobals();
    });
    const twenty = Array.from({ length: 20 }, (_, i) => i + 1);
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page(twenty, '20', undefined, 20, 45));
    render(<DaoDepositorsPage />);
    const pager = await screen.findByRole('navigation', { name: 'depositors' });
    expect(pager.textContent).toBe('1–20 of 45 · 20 a page← prevpage 1 of 3next →');
    expect(api.getDaoDepositors).toHaveBeenCalledWith({ limit: 20, cursor: undefined });
  });

  it('says which tip each page was ranked at when the tip moved between pages (L1, J2)', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockResolvedValueOnce(page([1, 2], '2', undefined, 2))
      .mockResolvedValueOnce(page([3], null, 20879300, 2));
    render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    fireEvent.click(screen.getByRole('button', { name: 'next →' }));
    await waitFor(() =>
      expect(
        screen.getByText((_, el) => el?.getAttribute('data-role') === 'ranked-at')
      ).toHaveTextContent('3 depositors ranked by CKB deposited at block #20,879,300')
    );
    // The rank cursor continues the new tip's ranking; the page says the ranking moved.
    expect(screen.getByTestId('ranking-moved')).toHaveTextContent(
      'The chain tip moved since the first page (ranked at block #20,879,205): this page continues the ranking at block #20,879,300, so a depositor whose rank changed may repeat or be missing.'
    );
    // Lane L: both blocks are links.
    expect(
      within(screen.getByTestId('ranking-moved'))
        .getAllByRole('link')
        .map((a) => a.getAttribute('href'))
    ).toEqual(['/mainnet/blocks/20879205', '/mainnet/blocks/20879300']);
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Start again from rank 1' })).toBeNull();
  });

  it('names a depositor without an address by its lock hash', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce({
      ...page([1], null),
      data: [{ ...row(1), address: null }],
    });
    render(<DaoDepositorsPage />);
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    expect(within(table).getByRole('link', { name: /^0x00000000…000001$/ })).toHaveAttribute(
      'href',
      `/mainnet/address/0x${'1'.padStart(64, '0')}`
    );
  });

  it('waits behind the "being derived" banner while the depositor totals are derived (L0)', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockRejectedValueOnce(
        new ApiRequestError(503, 'derive_pending', 'dao_live_agg is being derived', {
          aggregate: 'dao_live_agg',
          state: 'not_started',
          rowsDone: null,
          derivedFromTip: null,
        })
      )
      .mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    expect(await screen.findByRole('status', { name: 'Being derived' })).toHaveTextContent(
      'The indexer will derive the DAO depositor totals once its live sync starts.'
    );
    expect(screen.queryByRole('alert')).toBeNull();
    expect(await screen.findByRole('table', { name: 'DAO depositors' })).toBeInTheDocument();
  });
});
