import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import AddressDetailPage from '@/app/address/[addr]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getAddress: vi.fn(),
    getAddressTokens: vi.fn(),
    getLiveCells: vi.fn(),
    getAddressTransactions: vi.fn(),
    getAddressDaoSummary: vi.fn(),
    getDaoDepositsByAddress: vi.fn(),
    getAddressActivities: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

let mockRouteAddr = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ addr: mockRouteAddr }),
  useRouter: () => ({ push: vi.fn() }),
}));

const mockAddressWithLockScriptInfo = {
  lockScriptHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
  address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
  balance: '10000000000',
  usedCapacity: '6100000000',
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
  usedCapacity: '3000000000',
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
  usedCapacity: '500000000',
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
    mockRouteAddr = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';
    vi.mocked(api.getAddressTokens).mockResolvedValue(emptyTokens);
    vi.mocked(api.getLiveCells).mockResolvedValue(emptyCells);
    vi.mocked(api.getAddressTransactions).mockResolvedValue(emptyTransactions);
    vi.mocked(api.getAddressDaoSummary).mockResolvedValue(noDaoActivity);
    vi.mocked(api.getDaoDepositsByAddress).mockResolvedValue(emptyDaoDeposits);
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
    expect(screen.getByText(/^Unused:/)).toBeInTheDocument();
    expect(screen.queryByText(/^Free:/)).not.toBeInTheDocument();
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
    expect(screen.getByText('D')).toHaveClass('bg-base-elevated');
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

  it('uses token hash fallback in asset holdings when token has no name/symbol', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    const typeScriptHash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
    vi.mocked(api.getAddressTokens).mockResolvedValue({
      data: [
        {
          typeScriptHash,
          standard: 'xudt',
          name: null,
          symbol: null,
          decimals: 8,
          iconUrl: null,
          balance: '123450000000',
        },
      ],
      total: 1,
      limit: 100,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    const fallbackLabel = `${typeScriptHash.slice(0, 10)}...${typeScriptHash.slice(-8)}`;
    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: fallbackLabel })[0]).toHaveAttribute(
        'href',
        `/tokens/${typeScriptHash}`
      );
    });
  });

  it('uses token hash fallback link in activities when symbol is missing', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    const typeScriptHash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          blockNumber: 123,
          txIndex: 0,
          timestamp: '2026-02-20T00:00:00Z',
          ckbDelta: '0',
          usedDelta: '0',
          isCellbase: false,
          peers: [],
          assetChanges: [
            {
              type: 'token',
              typeScriptHash,
              delta: '100000000',
              decimals: 8,
            },
          ],
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    const fallbackLabel = `${typeScriptHash.slice(0, 10)}...${typeScriptHash.slice(-8)}`;
    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: fallbackLabel })[0]).toHaveAttribute(
        'href',
        `/tokens/${typeScriptHash}`
      );
    });
  });

  it('resets activity filter and pagination cursors when route address changes', async () => {
    const addrA = mockAddressWithLockScriptInfo.address!;
    const lockA = mockAddressWithLockScriptInfo.lockScriptHash;
    const addrB = 'ckb1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5d7y6v5';
    const lockB = '0x4444444444444444444444444444444444444444444444444444444444444444';

    vi.mocked(api.getAddress).mockImplementation(async (addr: string) => {
      if (addr === addrA) {
        return mockAddressWithLockScriptInfo;
      }
      if (addr === addrB) {
        return {
          ...mockAddressWithLockScriptInfo,
          address: addrB,
          lockScriptHash: lockB,
          transactionsCount: 1,
        };
      }
      throw new Error(`unexpected address request: ${addr}`);
    });

    vi.mocked(api.getAddressActivities).mockImplementation(
      async (
        _lockHash: string,
        _params?: { limit?: number; cursor?: string; filter?: string }
      ) => ({
        data: [],
        total: 0,
        limit: 20,
        hasMore: false,
        nextCursor: null,
      })
    );

    vi.mocked(api.getAddressTransactions).mockImplementation(
      async (lockHash: string, params?: { limit?: number; cursor?: string }) => {
        if (lockHash === lockA && !params?.cursor) {
          return {
            data: [
              {
                txHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                blockNumber: 200,
                txType: 'received',
                capacityChange: '100000000',
                timestamp: '2026-02-20T00:00:00Z',
                inputsCount: 1,
                outputsCount: 2,
                fee: '1000',
                isCellbase: false,
                txSize: 100,
                cycles: 100000,
                scriptLabels: [],
              },
            ],
            total: 2,
            limit: 20,
            hasMore: true,
            nextCursor: '100:0',
          };
        }
        if (lockHash === lockA && params?.cursor === '100:0') {
          return {
            data: [
              {
                txHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                blockNumber: 100,
                txType: 'sent',
                capacityChange: '-100000000',
                timestamp: '2026-02-19T00:00:00Z',
                inputsCount: 2,
                outputsCount: 1,
                fee: '1200',
                isCellbase: false,
                txSize: 120,
                cycles: 120000,
                scriptLabels: [],
              },
            ],
            total: 2,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          };
        }
        if (lockHash === lockB) {
          return {
            data: [
              {
                txHash: '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                blockNumber: 300,
                txType: 'received',
                capacityChange: '50000000',
                timestamp: '2026-02-21T00:00:00Z',
                inputsCount: 1,
                outputsCount: 1,
                fee: '800',
                isCellbase: false,
                txSize: 90,
                cycles: 90000,
                scriptLabels: [],
              },
            ],
            total: 1,
            limit: 20,
            hasMore: false,
            nextCursor: null,
          };
        }
        return emptyTransactions;
      }
    );

    const { rerender } = render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Active')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'CKB' }));
    await waitFor(() => {
      expect(api.getAddressActivities).toHaveBeenCalledWith(
        lockA,
        expect.objectContaining({ filter: 'ckb' })
      );
    });

    fireEvent.click(screen.getByRole('button', { name: /Transactions/ }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Next' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    await waitFor(() => {
      expect(api.getAddressTransactions).toHaveBeenCalledWith(
        lockA,
        expect.objectContaining({ cursor: '100:0' })
      );
    });

    vi.mocked(api.getAddressActivities).mockClear();
    vi.mocked(api.getAddressTransactions).mockClear();

    mockRouteAddr = addrB;
    rerender(<AddressDetailPage />);

    await waitFor(() => {
      expect(api.getAddress).toHaveBeenCalledWith(addrB);
    });
    await waitFor(() => {
      expect(api.getAddressActivities).toHaveBeenCalledWith(
        lockB,
        expect.objectContaining({ filter: 'all', cursor: undefined })
      );
    });
    await waitFor(() => {
      expect(api.getAddressTransactions).toHaveBeenCalledWith(
        lockB,
        expect.objectContaining({ cursor: undefined })
      );
    });
    expect(api.getAddressTransactions).not.toHaveBeenCalledWith(
      lockB,
      expect.objectContaining({ cursor: '100:0' })
    );
  });

  it('shows dotbit label in activities for nft changes', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
          blockNumber: 456,
          txIndex: 1,
          timestamp: '2026-02-20T00:00:00Z',
          ckbDelta: '0',
          usedDelta: '0',
          isCellbase: false,
          peers: [],
          assetChanges: [
            {
              type: 'identity',
              identityId: '0x1111111111111111111111111111111111111111',
              standard: 'dotbit',
              action: 'mint',
            },
          ],
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getAllByText('Mint .bit')[0]).toBeInTheDocument();
    });
  });

  it('shows did:ckb label in activities for identity changes', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
          blockNumber: 789,
          txIndex: 0,
          timestamp: '2026-02-21T00:00:00Z',
          ckbDelta: '0',
          usedDelta: '0',
          isCellbase: false,
          peers: [],
          assetChanges: [
            {
              type: 'identity',
              identityId: '0x2222222222222222222222222222222222222222222222222222222222222222',
              standard: 'did_ckb',
              action: 'mint',
            },
          ],
        },
      ],
      total: 1,
      limit: 20,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getAllByText('Mint did:ckb')[0]).toBeInTheDocument();
    });
  });
});
