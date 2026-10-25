import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import DaoPage from '@/app/dao/page';
import { api } from '@/lib/api';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';

vi.mock('@/lib/api', () => ({
  api: {
    getDaoStatistics: vi.fn(),
    getDaoDeposits: vi.fn(),
    getDaoTopDepositors: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('DaoPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // The full /dao/statistics contract (every field is required by DaoStatisticsResponse).
    vi.mocked(api.getDaoStatistics).mockResolvedValue({
      ...daoStatistics,
    });
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
      '/mainnet/scripts/Default%20Lock'
    );
    expect(screen.getByRole('button', { name: 'Active Deposits' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Deposits' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Depositors' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'All' })).not.toBeInTheDocument();
    expect(screen.queryByRole('columnheader', { name: 'Status' })).not.toBeInTheDocument();
    expect(screen.getByText('Showing 1-1 of 1 deposits, 50 per page')).toBeInTheDocument();
    expect(screen.getByText('Page 1 of 1')).toBeInTheDocument();
  });

  it('shows undefined average deposit time as an em dash', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue({
      ...daoStatistics,
      averageDepositDays: null,
    });
    render(<DaoPage />);
    expect(await screen.findByText('Average Deposit Time')).toBeInTheDocument();
    await waitFor(() => expect(screen.getAllByText('—').length).toBeGreaterThan(0));
  });

  it('distinguishes unavailable 24-hour DAO changes from exact zero changes', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue({
      ...daoStatistics,
      depositChange24h: null,
      depositorsChange24h: 0,
      claimedCompensationChange24h: null,
      unclaimedCompensationChange24h: null,
    });
    render(<DaoPage />);
    await waitFor(() => expect(screen.getAllByText('24h: —')).toHaveLength(3));
    expect(screen.getByText('→ 0')).toBeInTheDocument();
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
              '/mainnet/cell/0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc-3'
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
              '/mainnet/cell/0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-2'
          )
      ).toBe(true);
    });
  });
});

describe('DaoPage issuance and compensation breakdowns', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getDaoDeposits).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as any);
    vi.mocked(api.lookupScripts).mockResolvedValue({} as any);
  });

  it('prints every part exactly from its shannons, with exact shares', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue({
      ...daoStatistics,
    });
    render(<DaoPage />);
    const issuance = await screen.findByTestId('dao-secondary-issuance');
    // mining 96304962619313853, compensation 159838862391653367, burnt 674731136445379763 shannons
    await waitFor(() => expect(issuance).toHaveTextContent('Mining Reward963,049,626.19313853'));
    expect(issuance).toHaveTextContent('Deposit Compensation1,598,388,623.91653367');
    expect(issuance).toHaveTextContent('Burnt6,747,311,364.45379763');
    expect(issuance.querySelector('[data-part="mining"]')).toHaveAttribute('data-percent', '10.3');
    const compensation = screen.getByTestId('dao-compensation');
    expect(compensation).toHaveTextContent('Claimed760,797,612.35165103');
    expect(compensation).toHaveTextContent('Unclaimed837,591,011.56488264');
  });

  it('shows a missing amount as an error, never as 0', async () => {
    const { miningReward: _gone, ...broken } = daoStatistics;
    vi.mocked(api.getDaoStatistics).mockResolvedValue({
      ...broken,
    } as any);
    render(<DaoPage />);
    const issuance = await screen.findByTestId('dao-secondary-issuance');
    expect(await within(issuance).findByRole('alert')).toHaveTextContent(
      'miningReward "undefined" is not an integer shannon amount'
    );
    // The compensation breakdown does not depend on it and still reads.
    expect(screen.getByTestId('dao-compensation')).toHaveTextContent('Claimed760,797,612.35165103');
  });
});
