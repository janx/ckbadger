import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import DaoPage from '@/app/dao/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getDaoStatistics: vi.fn(),
    getDaoDeposits: vi.fn(),
    getDaoTopDepositors: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('DaoPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getDaoStatistics).mockResolvedValue({
      totalDepositedCkb: '1000',
      totalDepositors: 1,
      averageDepositDays: '10',
      estimatedApc: '3.2',
      miningRewardCkb: '600',
      depositCompensationCkb: '300',
      burntCkb: '100',
      totalCompensationPaidCkb: '100',
      unclaimedCompensationCkb: '50',
    } as any);
    vi.mocked(api.getDaoDeposits).mockResolvedValue({
      data: [
        {
          txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          outputIndex: 0,
          address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
          lockScriptHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          lockCodeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          capacity: '10200000000',
          status: 'deposited',
          depositTimestamp: '2026-02-20T00:00:00Z',
          withdrawRequestTxHash: null,
          withdrawRequestOutputIndex: null,
          withdrawTxHash: null,
          withdrawToOutputIndex: null,
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
    vi.mocked(api.lookupScripts).mockResolvedValue({
      '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8': {
        name: 'Default Lock',
      },
    } as any);
  });

  it('renders script label in deposits table', async () => {
    render(<DaoPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Nervos DAO')).toBeInTheDocument();

    await waitFor(() => {
      expect(api.getDaoDeposits).toHaveBeenCalledWith({
        limit: 50,
        status: 0,
        cursor: undefined,
      });
      expect(screen.getByText('Default Lock')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: 'Default Lock' })).toHaveAttribute(
      'href',
      '/scripts/Default%20Lock'
    );
    expect(screen.getByRole('button', { name: 'Active Deposits' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Deposits' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Depositors' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'All' })).not.toBeInTheDocument();
    expect(screen.queryByRole('columnheader', { name: 'Status' })).not.toBeInTheDocument();
    expect(screen.getByText('Showing 1-1 of 1 deposits, 50 per page')).toBeInTheDocument();
    expect(screen.getByText('Page 1 of 1')).toBeInTheDocument();
  });

  it('renders withdraw request tx as reference for withdrawing rows', async () => {
    vi.mocked(api.getDaoDeposits).mockResolvedValueOnce({
      data: [
        {
          txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          outputIndex: 0,
          address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
          lockScriptHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          lockCodeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          capacity: '10200000000',
          status: 'withdrawing',
          depositTimestamp: '2026-02-20T00:00:00Z',
          withdrawRequestTxHash:
            '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
          withdrawRequestOutputIndex: 3,
          withdrawTxHash: null,
          withdrawToOutputIndex: null,
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);

    render(<DaoPage />);

    await waitFor(() => {
      expect(
        screen
          .getAllByRole('link')
          .some(
            (link) =>
              link.getAttribute('href') ===
              '/cell/0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc-3'
          )
      ).toBe(true);
    });
  });

  it('renders withdraw-to cell as reference for withdrawn rows', async () => {
    vi.mocked(api.getDaoDeposits).mockResolvedValueOnce({
      data: [
        {
          txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          outputIndex: 0,
          address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
          lockScriptHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          lockCodeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
          capacity: '10200000000',
          status: 'withdrawn',
          depositTimestamp: '2026-02-20T00:00:00Z',
          withdrawRequestTxHash:
            '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
          withdrawRequestOutputIndex: 1,
          withdrawTxHash: '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
          withdrawToOutputIndex: 2,
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);

    render(<DaoPage />);

    await waitFor(() => {
      expect(
        screen
          .getAllByRole('link')
          .some(
            (link) =>
              link.getAttribute('href') ===
              '/cell/0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-2'
          )
      ).toBe(true);
    });
  });
});
