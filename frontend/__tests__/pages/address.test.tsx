import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import AddressDetailPage from '@/app/address/[addr]/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getAddress: vi.fn(),
    getAddressTokens: vi.fn(),
    getLiveCells: vi.fn(),
    getAddressTransactions: vi.fn(),
    getAddressDaoSummary: vi.fn(),
    getDaoDepositsByAddress: vi.fn(),
    getAddressStatsHistory: vi.fn(),
    getAddressActivities: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('next/navigation', () => ({
  useParams: () => ({ addr: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq' }),
  useRouter: () => ({ push: vi.fn() }),
}));

const mockAddressWithLockScriptInfo = {
  lockScriptHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
  balance: '10000000000',
  occupiedCapacity: '6100000000',
  liveCellsCount: 5,
  transactionsCount: 10,
  lockScript: {
    codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
    hashType: 'type',
    args: '0x89e3f8c2a9df2b0c8a1234567890abcdef123456',
  },
  lockScriptInfo: {
    codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
    name: 'Default Lock',
    scriptKind: 'lock',
    deprecated: false,
  },
};

const mockAddressWithoutLockScriptInfo = {
  lockScriptHash: '0x2222222222222222222222222222222222222222222222222222222222222222',
  address: undefined,
  balance: '5000000000',
  occupiedCapacity: '3000000000',
  liveCellsCount: 2,
  transactionsCount: 3,
  lockScript: {
    codeHash: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    hashType: 'data',
    args: '0x1234',
  },
  lockScriptInfo: undefined,
};

const mockAddressWithDeprecatedScript = {
  lockScriptHash: '0x3333333333333333333333333333333333333333333333333333333333333333',
  address: 'ckb1qtest',
  balance: '1000000000',
  occupiedCapacity: '500000000',
  liveCellsCount: 1,
  transactionsCount: 1,
  lockScript: {
    codeHash: '0xoldscript',
    hashType: 'type',
    args: '0x00',
  },
  lockScriptInfo: {
    codeHash: '0xoldscript',
    name: 'Old Lock v1',
    scriptKind: 'lock',
    deprecated: true,
  },
};

const emptyTokens = {
  data: [],
  total: 0,
  limit: 100,
  hasMore: false,
  nextCursor: null,
};

const emptyCells = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const emptyTransactions = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

const noDaoActivity = {
  hasDaoActivity: false,
  activeDepositsCount: 0,
  pendingWithdrawalsCount: 0,
  completedWithdrawalsCount: 0,
  totalLockedCapacity: '0',
  totalLockedCkb: '0',
  unclaimedCompensation: '0',
  unclaimedCompensationCkb: '0',
  totalCompensationEarned: '0',
  totalCompensationEarnedCkb: '0',
  estimatedApc: '',
};

const mockDaoSummary = {
  hasDaoActivity: true,
  activeDepositsCount: 3,
  pendingWithdrawalsCount: 1,
  completedWithdrawalsCount: 5,
  totalLockedCapacity: '500000000000',
  totalLockedCkb: '5000',
  unclaimedCompensation: '12500000000',
  unclaimedCompensationCkb: '125',
  totalCompensationEarned: '25000000000',
  totalCompensationEarnedCkb: '250',
  estimatedApc: '4.86',
};

const emptyDaoDeposits = {
  data: [],
  total: 0,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('AddressDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getAddressTokens).mockResolvedValue(emptyTokens);
    vi.mocked(api.getLiveCells).mockResolvedValue(emptyCells);
    vi.mocked(api.getAddressTransactions).mockResolvedValue(emptyTransactions);
    vi.mocked(api.getAddressDaoSummary).mockResolvedValue(noDaoActivity);
    vi.mocked(api.getDaoDepositsByAddress).mockResolvedValue(emptyDaoDeposits);
    vi.mocked(api.getAddressStatsHistory).mockResolvedValue({ title: '', data: [], series: [] });
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [],
      total: 0,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });
  });

  it('displays lock script name badge when lockScriptInfo is present', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Default Lock')).toBeInTheDocument();
    });

    const lockScriptLink = screen.getByText('Default Lock');
    expect(lockScriptLink.closest('a')).toHaveAttribute('href', '/scripts/Default%20Lock');
  });

  it('renders address section when lockScriptInfo is null', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithoutLockScriptInfo);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Active')).toBeInTheDocument();
    });
  });

  it('displays deprecated badge when script is deprecated', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithDeprecatedScript);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Old Lock v1')).toBeInTheDocument();
    });

    expect(screen.getByText('Deprecated')).toBeInTheDocument();
  });

  it('displays address balance and stats', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Balance')).toBeInTheDocument();
    });

    expect(screen.getAllByText('Live Cells').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Transactions').length).toBeGreaterThan(0);
    expect(screen.getAllByText('5').length).toBeGreaterThan(0);
    expect(screen.getAllByText('10').length).toBeGreaterThan(0);
  });

  it('displays Active badge', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Active')).toBeInTheDocument();
    });
  });

  it('displays DAO in Asset Holdings when address has DAO activity', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressDaoSummary).mockResolvedValue(mockDaoSummary);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Nervos DAO')).toBeInTheDocument();
    });

    expect(screen.getByText('4.86% APC')).toBeInTheDocument();
    expect(screen.getByText('Active Deposits')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('displays DAO deposit stats including pending withdrawals', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressDaoSummary).mockResolvedValue(mockDaoSummary);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Nervos DAO')).toBeInTheDocument();
    });

    expect(screen.getByText('Active Deposits')).toBeInTheDocument();
    expect(screen.getByText('Pending Withdrawals')).toBeInTheDocument();
    expect(screen.getByText('Compensation Earned')).toBeInTheDocument();
  });
});
