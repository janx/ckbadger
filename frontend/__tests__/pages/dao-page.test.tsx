import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import DaoPage from '@/app/dao/page';
import { api } from '@/lib/api';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import evidence from '@/__tests__/fixtures/dao-evidence.json';

vi.mock('@/lib/api', () => ({
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
  api: {
    getDaoStatistics: vi.fn(),
    getDaoEvidence: vi.fn(),
    getDaoTotalDepositChart: vi.fn(),
    getDaoCirculationRatioChart: vi.fn(),
    getGlobalActivities: vi.fn(),
    getTransactions: vi.fn(),
    calculateDaoCompensation: vi.fn(),
    getDaoArHistory: vi.fn(),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));

const stats = {
  ...daoStatistics,
  tipBlockNumber: 5,
  // Consistent with the evidence header at #5: S (500000000) = still owed + burnt.
  miningReward: '100000000',
  depositCompensation: '300000000',
  totalCompensationPaid: '100000000',
  unclaimedCompensation: '200000000',
  burnt: '300000000',
  tipSecondaryPool: '500000000',
  totalDeposited: '50000000000',
  totalDepositedCkb: '500',
  daoFreeCapacity: '40000000000',
  depositorAllocationWeight: '45000000000',
  tipTotalIssuance: '1000000000000000',
  tipProtocolOccupiedCapacity: '100000000',
};
const history = {
  data: [{ date: '2024-01-15', value: '500', value2: '2' }],
  title: 'Total Deposit',
  yAxisLabel: 'CKB',
  y2AxisLabel: 'Current depositors',
  peak: { date: '2024-01-15', value: '500' },
  peak2: { date: '2024-01-15', value: '2' },
};
const ratio = { data: [{ date: '2024-01-15', value: '1.25' }], title: 'Ratio', yAxisLabel: '%' };

describe('DaoPage', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.getDaoStatistics).mockResolvedValue(stats);
    vi.mocked(api.getDaoEvidence).mockResolvedValue(evidence);
    vi.mocked(api.getDaoTotalDepositChart).mockResolvedValue(history);
    vi.mocked(api.getDaoCirculationRatioChart).mockResolvedValue(ratio);
    vi.mocked(api.getGlobalActivities).mockResolvedValue({
      data: [],
      hasMore: false,
      nextCursor: null,
      limit: 12,
    });
    vi.mocked(api.getDaoArHistory).mockResolvedValue({
      anchor: { blockNumber: 4, blockHash: '0x' + '4'.repeat(64) },
      days: [
        {
          date: '2024-01-14',
          blockNumber: 3,
          ar: '10100000000000000',
          arIndex: '1.0100000000000000',
        },
        {
          date: '2024-01-15',
          blockNumber: 4,
          ar: '10150000000000000',
          arIndex: '1.0150000000000000',
        },
      ],
    });
    vi.mocked(api.calculateDaoCompensation).mockImplementation(
      async (capacity: string, depositBlock: number, withdrawBlock?: number) => ({
        capacity,
        capacityCkb: '10000',
        depositBlock,
        withdrawBlock: withdrawBlock!,
        estimatedCompensation: '20000000000',
        estimatedCompensationCkb: '200',
        totalWithdrawable: '1020000000000',
        totalWithdrawableCkb: '10200',
        apc: null,
        occupiedCapacity: '10200000000',
        freeCapacity: '989800000000',
      })
    );
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      hasMore: false,
      nextCursor: null,
      limit: 12,
    });
  });

  it('renders the API-produced epoch, owner, AR and raw DAO evidence', async () => {
    render(<DaoPage />);
    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(
      await screen.findByRole('heading', { name: 'When deposits can leave' })
    ).toBeInTheDocument();
    expect(screen.getByText(/at indexed epoch 185\+2\/4/)).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'When deposits can leave' })).toHaveTextContent(
      '400.00000000 CKB in 1 cell is past its window and can be claimed now'
    );
    expect(screen.getByLabelText('History reading')).toHaveTextContent('depositors2');
    expect(screen.getByRole('region', { name: 'Who deposits' })).toHaveTextContent('60.00%');
    expect(screen.getByText(evidence.header.daoHex)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open source block →' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/5'
    );
  });

  it('shows evidence API errors instead of fabricated zero values', async () => {
    vi.mocked(api.getDaoEvidence).mockRejectedValue(
      new Error('missing canonical DAO header at block 4')
    );
    render(<DaoPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'missing canonical DAO header at block 4'
    );
    expect(
      screen.queryByRole('heading', { name: 'When deposits can leave' })
    ).not.toBeInTheDocument();
  });

  it('keeps capacity use visible at its statistics anchor when evidence has advanced', async () => {
    vi.mocked(api.getDaoStatistics).mockResolvedValue({ ...stats, tipBlockNumber: 4 });
    render(<DaoPage />);
    expect(
      await screen.findByText('Current capacity use at statistics block #4')
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Protocol occupied includes the genesis virtual occupied baseline/)
    ).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: 'Verify statistics anchor block #4 →' })
    ).toHaveAttribute('href', '/mainnet/blocks/4');
  });

  it('shows a canonical history error without an endless loading message', async () => {
    vi.mocked(api.getDaoTotalDepositChart).mockRejectedValue(new Error('missing chart snapshot'));
    render(<DaoPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'DAO daily history could not be loaded'
    );
    expect(screen.queryByText('Loading canonical daily history…')).not.toBeInTheDocument();
  });
});
