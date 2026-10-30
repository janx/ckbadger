import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
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
const page = (rows: number[], nextCursor: string | null) => ({
  anchor: { blockNumber: 20879205, blockHash: '0x' + 'a'.repeat(64) },
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

  it('lists every depositor ranked at one tip and pages with the tip-bound cursor', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockResolvedValueOnce(page([1, 2], '20879205:2'))
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
      expect(api.getDaoDepositors).toHaveBeenLastCalledWith({ limit: 50, cursor: '20879205:2' })
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

  it('folds each depositor into a two-line card on phones, not a five-fact sheet', async () => {
    vi.mocked(api.getDaoDepositors).mockResolvedValueOnce(page([1, 2], null));
    render(<DaoDepositorsPage />);
    const cards = await screen.findByRole('list', { name: 'DAO depositors, compact' });
    const first = within(cards).getAllByRole('listitem')[0];
    expect(first.querySelectorAll('dt')).toHaveLength(0);
    expect(first).toHaveTextContent('1ckb1qzda0cr0…ssd2hqn1.00%999.00000000 CKB · 1 deposit');
  });

  it('says the ranking moved with the tip and starts again from rank 1', async () => {
    vi.mocked(api.getDaoDepositors)
      .mockResolvedValueOnce(page([1, 2], '20879205:2'))
      .mockRejectedValueOnce(
        new ApiRequestError(
          400,
          'bad_request',
          'depositor cursor ranks at block 20879205, not at the current tip; start again'
        )
      )
      .mockResolvedValueOnce(page([1, 2], '20879300:2'));
    render(<DaoDepositorsPage />);
    await screen.findByRole('table', { name: 'DAO depositors' });
    fireEvent.click(screen.getByRole('button', { name: 'Next 50 →' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('not at the current tip; start again');
    fireEvent.click(within(alert).getByRole('button', { name: 'Start again from rank 1' }));
    // A fresh ranking at the new tip, not the cached first page of the old one.
    await waitFor(() => expect(api.getDaoDepositors).toHaveBeenCalledTimes(3));
    expect(api.getDaoDepositors).toHaveBeenLastCalledWith({ limit: 50, cursor: undefined });
    expect(
      await screen.findByText((_, el) => el?.getAttribute('data-role') === 'ranked-at')
    ).toHaveTextContent('3 depositors ranked by CKB deposited at block #20,879,205');
  });
});
