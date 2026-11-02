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
    expect(
      screen.getByText((_, el) => el?.getAttribute('data-role') === 'ranked-at')
    ).toHaveTextContent('3 depositors ranked by CKB deposited at block #20,879,205');
    expect(api.getDaoDepositors).toHaveBeenCalledWith({ limit: 50, cursor: undefined });
    fireEvent.click(screen.getByRole('button', { name: 'Next 50 →' }));
    await waitFor(() =>
      expect(api.getDaoDepositors).toHaveBeenLastCalledWith({ limit: 50, cursor: '2' })
    );
    const next = await screen.findByRole('table', { name: 'DAO depositors' });
    await waitFor(() => expect(next.querySelector('tbody tr')).toHaveTextContent('997.00000000'));
    expect(screen.queryByRole('button', { name: 'Next 50 →' })).toBeNull();
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
    // Rank inline, the depositor and the CKB deposited; then the share bar, its number and count.
    expect(phoneLines(row)).toEqual([
      { key: 'holder', tone: 'head', left: '1ckb1qzda0cr0…ssd2hqn', right: '999.00000000 CKB' },
      { key: 'share', tone: 'quiet', left: '', right: '1.00% · 1 deposit' },
    ]);
    // 10 % fills the bar: 1.00 % draws a tenth of it.
    expect(row.querySelector('[data-share-bar]')).toHaveAttribute('data-share-bar', '10%');
  });

  // Table-rows T5: the depositor is the row's one bright thing, at body, on an 18 px line so the
  // row keeps its one-line height; the rank and the share stay dim, the numbers small.
  it('leads each row with the depositor at 13 px bright (T5)', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1], null));
    render(<DaoDepositorsPage />);
    const table = await screen.findByRole('table', { name: 'DAO depositors' });
    const depositor = within(table).getByRole('link', { name: 'ckb1qzda0cr0…ssd2hqn' });
    expect(depositor).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]');
    const [rank, , deposits, capacity, share] = Array.from(table.querySelectorAll('tbody td'));
    expect(rank.firstElementChild).toHaveClass('text-text-dim');
    expect(share.firstElementChild).toHaveClass('text-text-dim');
    for (const cell of [rank, deposits, capacity, share])
      expect(cell.querySelector('.text-body')).toBeNull();
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
    expect(phoneLines(row)).toEqual([
      {
        key: 'holder',
        tone: 'head',
        left: '1ckb1qzda0cr0…ssd2hqn',
        right: '1.00% · 1 deposit999.00000000 CKB',
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
    expect(within(table).getByRole('link', { name: /^0x0000000000…0000001$/ })).toHaveAttribute(
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
