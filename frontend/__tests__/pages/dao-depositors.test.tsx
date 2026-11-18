import { beforeEach, describe, expect, it, vi, onTestFinished } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { atContainerWidth, phoneLines } from '@/__tests__/utils/phone-row';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import DaoDepositorsPage from '@/app/dao/depositors/page';
import { ApiRequestError, api } from '@/lib/api';

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
const page = (rows: number[], nextCursor: string | null, tip = 20879205) => ({
  anchor: { blockNumber: tip, blockHash: '0x' + 'a'.repeat(64) },
  total: 3,
  limit: 50,
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
      .mockResolvedValueOnce(page([1, 2], '2'))
      .mockResolvedValueOnce(page([3], null));
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
    fireEvent.click(screen.getByRole('button', { name: 'Next 50 →' }));
    await waitFor(() =>
      expect(api.getDaoDepositors).toHaveBeenLastCalledWith({ limit: 50, cursor: '2' })
    );
    const next = await screen.findByRole('table', { name: 'DAO depositors' });
    await waitFor(() => expect(next.querySelector('tbody tr')).toHaveTextContent('997.00000000'));
    // Re-pinned on purpose (readability B5): the page pages with the site's one pager, whose
    // last page keeps a disabled Next (it hid the button).
    expect(screen.getByRole('button', { name: 'Next 50 →' })).toBeDisabled();
    // Back to the page already ranked at the same tip (kept, not re-ranked).
    fireEvent.click(screen.getByRole('button', { name: '← Previous' }));
    await waitFor(() =>
      expect(
        screen.getByRole('table', { name: 'DAO depositors' }).querySelector('tbody tr')
      ).toHaveTextContent('999.00000000')
    );
    expect(screen.getByRole('button', { name: 'Next 50 →' })).toBeInTheDocument();
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
    expect(rank.firstElementChild).toHaveClass('text-text-dim');
    // R7: the share is a bar on the page's scale with its number beside it, the number dim.
    expect(share.querySelector('[data-aligned-bar]')).not.toBeNull();
    expect(within(share as HTMLElement).getByText('1.00%')).toHaveClass('text-text-dim');
    for (const cell of [rank, deposits, capacity, share])
      expect(cell.querySelector('.text-body')).toBeNull();
  });

  // Readability B5: the page drew its own Previous / Next, the same control as the site's pager
  // without its count; it now pages with the one pager, which draws nothing for one page.
  it('pages with the site’s one pager, and draws none for one page (B5)', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], '2'));
    const { unmount } = render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    const controls = document.querySelector('[data-role="pager-controls"]') as HTMLElement;
    expect(controls).not.toBeNull();
    expect(within(controls).getByRole('button', { name: '← Previous' })).toBeDisabled();
    expect(within(controls).getByRole('button', { name: 'Next 50 →' })).toBeEnabled();
    expect(screen.getByText('Showing 1–2 of 3 depositors · 50 per page')).toBeInTheDocument();
    unmount();
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    expect(document.querySelector('[data-role="pager-controls"]')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
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
  // after every column but the last: 605 px, so the 608 px band draws the table.
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
      // the track (its tick row "0 100%" is under 48 px), the 10 px gap, "100.00%"; the last column
      ceil(48 + 10 + Math.ceil(at('100.00%', 13))),
    ];
    expect(budgets).toEqual([71, 159, 74, 188, 113]);
    const sum = budgets.reduce((s, w) => s + w, 0);
    expect(sum).toBe(605);
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
      [605, 'table'],
      [604, 'rows'],
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

  it('puts a depositor on one line in the 640 band, where the five-column table does not fit', async () => {
    atContainerWidth(608);
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

  it('ranks twenty depositors a page below md and says so on the button (R2)', async () => {
    stubViewportFromMd(false);
    onTestFinished(() => {
      vi.unstubAllGlobals();
    });
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], '2'));
    render(<DaoDepositorsPage />);
    expect(await screen.findByRole('button', { name: 'Next 20 →' })).toBeInTheDocument();
    expect(api.getDaoDepositors).toHaveBeenCalledWith({ limit: 20, cursor: undefined });
  });

  it('says which tip each page was ranked at when the tip moved between pages (L1, J2)', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockResolvedValueOnce(page([1, 2], '2'))
      .mockResolvedValueOnce(page([3], null, 20879300));
    render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    fireEvent.click(screen.getByRole('button', { name: 'Next 50 →' }));
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
