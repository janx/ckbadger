import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
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
    getAddressDotCellNames: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
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
  commonKnowledgeSize: '6100000000',
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
  }, // The mirror reports nothing for these addresses.
  pendingSummary: null,
};

const mockAddressWithoutLockScriptInfo = {
  lockScriptHash: '0x2222222222222222222222222222222222222222222222222222222222222222',
  address: undefined,
  balance: '5000000000',
  commonKnowledgeSize: '3000000000',
  liveCellsCount: 2,
  transactionsCount: 3,
  lockScript: {
    codeHash: '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    hashType: 'data',
    args: '0x1234',
  },
  lockScriptInfo: undefined, // The mirror reports nothing for these addresses.
  pendingSummary: null,
};

const mockAddressWithDeprecatedScript = {
  lockScriptHash: '0x3333333333333333333333333333333333333333333333333333333333333333',
  address: 'ckb1qtest',
  balance: '1000000000',
  commonKnowledgeSize: '500000000',
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
  }, // The mirror reports nothing for these addresses.
  pendingSummary: null,
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
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

const emptyTransactions = {
  data: [],
  total: 0,
  limit: 50,
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
  limit: 50,
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
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getAddressDotCellNames).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
  });

  it('lists the .cell names this address owns', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressDotCellNames).mockResolvedValue({
      data: [
        {
          identityId: '0x62d71147ac82b83c8531126cacb0d2f072bfd94a',
          label: 'support',
          name: 'support.cell',
          expiredAt: 1821507678,
        },
      ],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(api.getAddressDotCellNames).toHaveBeenCalledWith(
        mockAddressWithLockScriptInfo.lockScriptHash,
        { limit: 50 }
      );
    });

    const link = await screen.findByRole('link', { name: 'support.cell' });
    expect(link).toHaveAttribute(
      'href',
      '/mainnet/identities/dotcell/0x62d71147ac82b83c8531126cacb0d2f072bfd94a'
    );
    expect(screen.getByText('Names (1)')).toBeInTheDocument();
  });

  it('pages through the .cell names instead of counting one page as all of them', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    const name = (n: number) => ({
      identityId: `0x${n.toString(16).padStart(40, '0')}`,
      label: `name${n}`,
      name: `name${n}.cell`,
      expiredAt: 1821507678,
    });
    vi.mocked(api.getAddressDotCellNames).mockImplementation(async (_addr, params) =>
      params?.cursor
        ? { data: [name(3)], limit: 50, hasMore: false, nextCursor: null }
        : { data: [name(1), name(2)], limit: 50, hasMore: true, nextCursor: name(2).identityId }
    );

    render(<AddressDetailPage />);

    const names = await screen.findByTestId('address-dotcell-names');
    expect(await within(names).findByRole('link', { name: 'name1.cell' })).toBeInTheDocument();
    // One page of a longer list is not a count of the list.
    expect(screen.queryByText(/^Names \(/)).not.toBeInTheDocument();

    fireEvent.click(within(names).getByRole('button', { name: 'Next' }));

    await waitFor(() => {
      expect(api.getAddressDotCellNames).toHaveBeenCalledWith(
        mockAddressWithLockScriptInfo.lockScriptHash,
        { limit: 50, cursor: name(2).identityId }
      );
    });
    expect(await within(names).findByRole('link', { name: 'name3.cell' })).toBeInTheDocument();
    expect(within(names).getByRole('button', { name: 'Previous' })).toBeEnabled();
  });

  it('omits the Names section for an address that owns no .cell name', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(api.getAddressDotCellNames).toHaveBeenCalled();
    });
    expect(screen.queryByText(/^Names \(/)).not.toBeInTheDocument();
  });

  it('displays lock script name badge when lockScriptInfo is present', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Default Lock')).toBeInTheDocument();
    });

    const lockScriptLink = screen.getByText('Default Lock');
    expect(lockScriptLink.closest('a')).toHaveAttribute('href', '/mainnet/scripts/Default%20Lock');
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
    expect(screen.getByText(/^Free Capacity:/)).toBeInTheDocument();
    expect(screen.queryByText(/^Free:/)).not.toBeInTheDocument();
  });

  it('uses script call labels in the activity filter and empty state', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);

    render(<AddressDetailPage />);

    const filter = await screen.findByLabelText('Filter');
    expect(screen.getByRole('option', { name: 'Script Call (type)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Script Call (lock)' })).toBeInTheDocument();

    fireEvent.change(filter, { target: { value: 'type_call' } });
    expect(screen.getByText('No Script Call (type) activities on this page')).toBeInTheDocument();

    fireEvent.change(filter, { target: { value: 'lock_call' } });
    expect(screen.getByText('No Script Call (lock) activities on this page')).toBeInTheDocument();
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
        `/mainnet/tokens/${typeScriptHash}`
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
          participants: [],
          roles: [],
          tags: 1,
          typeCalls: [],
          lockCalls: [],
          protocolActions: [],
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash,
              delta: '100000000',
              decimals: 8,
            },
          ],
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    // ActivityEventGroup uses truncateHash(hash, 8, 6) for token fallback label
    const fallbackLabel = `${typeScriptHash.slice(0, 8)}...${typeScriptHash.slice(-6)}`;
    await waitFor(() => {
      // The token delta link text includes the amount + label, so find by partial match
      const links = screen.getAllByRole('link');
      const tokenLink = links.find(
        (l) =>
          l.getAttribute('href') === `/mainnet/tokens/${typeScriptHash}` &&
          l.textContent?.includes(fallbackLabel)
      );
      expect(tokenLink).toBeDefined();
    });
  });

  it('never renders the address transaction count as an activity total', async () => {
    // R4-G item 3: activities exclude cellbase, so the address's transaction
    // count is not a total this list can ever page to. The API stopped
    // declaring one; the page must not substitute `transactionsCount`.
    vi.mocked(api.getAddress).mockResolvedValue({
      ...mockAddressWithLockScriptInfo,
      transactionsCount: 4727769,
    });
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
          blockNumber: 123,
          txIndex: 0,
          timestamp: '2026-02-20T00:00:00Z',
          ckbDelta: '100000000',
          usedDelta: '0',
          isCellbase: false,
          participants: [],
          roles: [],
          tags: 1,
          typeCalls: [],
          lockCalls: [],
          protocolActions: [],
          itemDeltas: [],
        },
      ],
      limit: 50,
      hasMore: true,
      nextCursor: '123:0',
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Next' })).toBeInTheDocument();
    });
    expect(screen.queryByText(/4,727,769 activities/)).not.toBeInTheDocument();
    expect(screen.getByText(/Showing 1-1 activities/)).toBeInTheDocument();
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
        limit: 50,
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
            limit: 50,
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
            limit: 50,
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
            limit: 50,
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

    // Activity filter is now a <select> dropdown
    const filterSelect = screen.getByRole('combobox');
    fireEvent.change(filterSelect, { target: { value: 'ckb' } });
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

  it('shows dotbit label in activities for object changes', async () => {
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
          participants: [],
          roles: [],
          tags: 4,
          typeCalls: [],
          lockCalls: [],
          protocolActions: [],
          itemDeltas: [
            {
              kind: 'identity',
              standard: 'dotbit',
              identityId: '0x1111111111111111111111111111111111111111',
              delta: 1,
            },
          ],
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    // ActivityEventGroup renders identity as "\u2736 Identity Registered" (appears in both mobile + desktop views)
    await waitFor(() => {
      expect(screen.getAllByText(/Identity Registered/)[0]).toBeInTheDocument();
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
          participants: [],
          roles: [],
          tags: 4,
          typeCalls: [],
          lockCalls: [],
          protocolActions: [],
          itemDeltas: [
            {
              kind: 'identity',
              standard: 'did_ckb',
              identityId: '0x2222222222222222222222222222222222222222222222222222222222222222',
              delta: 1,
            },
          ],
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    // ActivityEventGroup renders identity as "\u2736 Identity Registered" (appears in both mobile + desktop views)
    await waitFor(() => {
      expect(screen.getAllByText(/Identity Registered/)[0]).toBeInTheDocument();
    });
  });

  it('shows script calls and token changes in activity event rows', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [
        {
          txHash: '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
          blockNumber: 999,
          txIndex: 0,
          timestamp: '2026-02-22T00:00:00Z',
          ckbDelta: '0',
          usedDelta: '0',
          isCellbase: false,
          participants: [],
          roles: [],
          tags: 1,
          itemDeltas: [
            {
              kind: 'token',
              typeScriptHash: '0xtokenhash',
              delta: '100000000',
              symbol: 'SEAL',
              decimals: 8,
            },
          ],
          typeCalls: [
            {
              typeCodeHash: '0xcodehash',
              typeHashType: 'type',
              typeArgs: '0x1234abcd',
              scriptHash: '0xscript-hash',
              scriptName: 'RGB++ Lock',
            },
          ],
          lockCalls: [],
          protocolActions: [],
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<AddressDetailPage />);

    // ActivityEventGroup renders type calls inline with TypeCallExpr link (mobile + desktop)
    await waitFor(() => {
      expect(screen.getAllByText('RGB++ Lock')[0]).toBeInTheDocument();
    });

    // Token change rendered as "SEAL Transfer" label with link (mobile + desktop)
    expect(screen.getAllByText(/SEAL Transfer/)[0]).toBeInTheDocument();

    // Script call name is a link to the script detail page
    expect(screen.getAllByRole('link', { name: 'RGB++ Lock' })[0]).toHaveAttribute(
      'href',
      '/mainnet/scripts/RGB%2B%2B%20Lock'
    );
  });
});

// ---------------------------------------------------------------------------
// Unconfirmed (tx-pool) state on the address page
// ---------------------------------------------------------------------------

const poolSummary = {
  enabled: true,
  healthy: true,
  lastPolledAt: '2026-09-23T12:00:00+00:00',
  count: 1,
  pendingCkbDelta: '-50000000000',
  truncated: false,
};

function poolActivityRow() {
  return {
    txHash: '0xfeed000000000000000000000000000000000000000000000000000000000001',
    blockNumber: null,
    txIndex: null,
    timestamp: null,
    poolStatus: 'pending' as const,
    timeAddedToPool: new Date(Date.now() - 5 * 60 * 1000).toISOString(),
    interpretation: { status: 'complete' as const },
    ckbDelta: '-50000000000',
    usedDelta: '0',
    isCellbase: false,
    itemDeltas: [],
    typeCalls: [],
    lockCalls: [],
    protocolActions: [],
    participants: [],
    roles: [],
    tags: 0,
  };
}

describe('AddressDetailPage — unconfirmed transactions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockRouteAddr = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';
    vi.mocked(api.getAddress).mockResolvedValue(mockAddressWithLockScriptInfo);
    vi.mocked(api.getAddressTokens).mockResolvedValue(emptyTokens);
    vi.mocked(api.getLiveCells).mockResolvedValue(emptyCells);
    vi.mocked(api.getAddressTransactions).mockResolvedValue(emptyTransactions);
    vi.mocked(api.getAddressDaoSummary).mockResolvedValue(noDaoActivity);
    vi.mocked(api.getDaoDepositsByAddress).mockResolvedValue(emptyDaoDeposits);
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
  });

  it('shows an Unconfirmed stat separate from Balance, never folded into it', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...mockAddressWithLockScriptInfo,
      pendingSummary: { txCount: 1, capacityDelta: '-50000000000' },
    });
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [poolActivityRow()],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: poolSummary,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Unconfirmed')).toBeInTheDocument();
    });
    // Balance stands on its own; the pool figure is reported beside it as a
    // separate "pending" amount and is never summed into the balance.
    expect(screen.getByText('Balance')).toBeInTheDocument();
    expect(screen.getByText(/^-500\.00000000 CKB pending$/)).toBeInTheDocument();
  });

  // The header describes the address, not the list below it: it comes from the
  // address summary, so no tab, page or activity filter can change or hide it.
  const addressWithPending = {
    ...mockAddressWithLockScriptInfo,
    pendingSummary: { txCount: 2, capacityDelta: '-50000000000' },
  };

  async function expectPendingHeader() {
    await waitFor(() => {
      expect(screen.getByText('Unconfirmed')).toBeInTheDocument();
    });
    expect(screen.getByText('2 tx')).toBeInTheDocument();
    expect(screen.getByText(/^-500\.00000000 CKB pending$/)).toBeInTheDocument();
  }

  it('keeps the pending header on the Cells tab', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(addressWithPending);

    render(<AddressDetailPage />);
    await expectPendingHeader();

    fireEvent.click(screen.getByRole('button', { name: /Live Cells/ }));
    await waitFor(() => {
      expect(screen.getByText('No live cells')).toBeInTheDocument();
    });
    await expectPendingHeader();
  });

  it('keeps the pending header on page two of the activity list', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(addressWithPending);
    vi.mocked(api.getAddressActivities).mockImplementation(async (_addr, params) => ({
      data: [
        {
          ...poolActivityRow(),
          txHash: params?.cursor
            ? '0xc0de000000000000000000000000000000000000000000000000000000000002'
            : '0xc0de000000000000000000000000000000000000000000000000000000000001',
          blockNumber: 12345,
          txIndex: 0,
          timestamp: '1700000000000',
          poolStatus: undefined,
          timeAddedToPool: undefined,
          interpretation: undefined,
        },
      ],
      limit: 50,
      hasMore: !params?.cursor,
      nextCursor: params?.cursor ? null : '12345:0',
      // Only page one carries the pool segment.
      ...(params?.cursor ? {} : { pool: { ...poolSummary, count: 0, pendingCkbDelta: '0' } }),
    }));

    render(<AddressDetailPage />);
    await expectPendingHeader();

    fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
    await waitFor(() => {
      expect(api.getAddressActivities).toHaveBeenCalledWith(
        mockAddressWithLockScriptInfo.lockScriptHash,
        expect.objectContaining({ cursor: '12345:0' })
      );
    });
    await expectPendingHeader();
  });

  it('keeps the pending header when the activity filter matches none of the pool rows', async () => {
    vi.mocked(api.getAddress).mockResolvedValue(addressWithPending);
    vi.mocked(api.getAddressActivities).mockImplementation(async (_addr, params) =>
      params?.filter === 'token'
        ? {
            data: [],
            limit: 50,
            hasMore: false,
            nextCursor: null,
            pool: { ...poolSummary, count: 0, pendingCkbDelta: '0' },
          }
        : {
            data: [poolActivityRow()],
            limit: 50,
            hasMore: false,
            nextCursor: null,
            pool: { ...poolSummary, count: 1, pendingCkbDelta: '-20000000000' },
          }
    );

    render(<AddressDetailPage />);
    await expectPendingHeader();

    fireEvent.change(screen.getByLabelText('Filter'), { target: { value: 'token' } });
    await waitFor(() => {
      expect(api.getAddressActivities).toHaveBeenCalledWith(
        mockAddressWithLockScriptInfo.lockScriptHash,
        expect.objectContaining({ filter: 'token' })
      );
    });
    await waitFor(() => {
      expect(screen.getByText('No Token activities on this page')).toBeInTheDocument();
    });
    await expectPendingHeader();
  });

  it('shows no pending amount when the pending transactions net to zero', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...mockAddressWithLockScriptInfo,
      pendingSummary: { txCount: 1, capacityDelta: '0' },
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Unconfirmed')).toBeInTheDocument();
    });
    expect(screen.getByText('1 tx')).toBeInTheDocument();
    expect(screen.queryByText(/CKB pending/)).not.toBeInTheDocument();
  });

  it('shows no pending header when the address summary carries none', async () => {
    vi.mocked(api.getAddress).mockResolvedValue({
      ...mockAddressWithLockScriptInfo,
      pendingSummary: null,
    });
    // Even if a list segment reports pool rows, the header follows the summary.
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [poolActivityRow()],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: poolSummary,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getAllByText(/in pool for 5m/).length).toBeGreaterThan(0);
    });
    expect(screen.queryByText('Unconfirmed')).not.toBeInTheDocument();
    expect(screen.queryByText(/CKB pending/)).not.toBeInTheDocument();
  });

  it('reports a capped unconfirmed segment in the list it belongs to', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [poolActivityRow()],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: { ...poolSummary, truncated: true },
    });

    render(<AddressDetailPage />);

    expect(
      await screen.findByText('More unconfirmed transactions than shown.')
    ).toBeInTheDocument();
  });

  it('renders the pool badge instead of a block link for an unconfirmed activity', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [poolActivityRow()],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: poolSummary,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getAllByText('Pending').length).toBeGreaterThan(0);
    });
    expect(screen.getAllByText(/in pool for 5m/).length).toBeGreaterThan(0);
  });

  it('says the pool view is unavailable rather than showing an empty segment', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: { ...poolSummary, healthy: false, count: 0, pendingCkbDelta: '0' },
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText(/Pool view unavailable/i)).toBeInTheDocument();
    });
    expect(screen.queryByText('Unconfirmed')).not.toBeInTheDocument();
  });

  it('does not mention the pool when the mirror is disabled', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: { ...poolSummary, enabled: false, healthy: false, count: 0, pendingCkbDelta: '0' },
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getAllByText('Live Cells').length).toBeGreaterThan(0);
    });
    expect(screen.queryByText(/Pool view unavailable/i)).not.toBeInTheDocument();
    expect(screen.queryByText('Unconfirmed')).not.toBeInTheDocument();
  });

  it('counts only committed rows in the pagination range', async () => {
    vi.mocked(api.getAddressActivities).mockResolvedValue({
      data: [
        poolActivityRow(),
        {
          ...poolActivityRow(),
          txHash: '0xc0de000000000000000000000000000000000000000000000000000000000001',
          blockNumber: 12345,
          txIndex: 0,
          timestamp: '1700000000000',
          poolStatus: undefined,
          timeAddedToPool: undefined,
          interpretation: undefined,
        },
      ],
      limit: 50,
      hasMore: true,
      nextCursor: '12345:0',
      pool: poolSummary,
    });

    render(<AddressDetailPage />);

    await waitFor(() => {
      expect(screen.getByText(/Showing 1-1 activities/)).toBeInTheDocument();
    });
  });

  it('marks an unconfirmed row in the transactions table instead of linking to a block', async () => {
    vi.mocked(api.getAddressTransactions).mockResolvedValue({
      data: [
        {
          txHash: '0xfeed000000000000000000000000000000000000000000000000000000000002',
          blockNumber: null,
          txType: 'sent' as const,
          capacityChange: '-50000000000',
          timestamp: null,
          poolStatus: 'proposed' as const,
          timeAddedToPool: new Date(Date.now() - 2 * 60 * 1000).toISOString(),
          interpretation: { status: 'complete' as const },
          inputsCount: 1,
          outputsCount: 2,
          fee: '1000',
          isCellbase: false,
          txSize: 500,
          cycles: 200000,
          scriptLabels: [],
        },
      ],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: { ...poolSummary, count: 1 },
    });

    render(<AddressDetailPage />);
    await waitFor(() => {
      expect(screen.getAllByText('Transactions').length).toBeGreaterThan(0);
    });
    fireEvent.click(screen.getByRole('button', { name: /Transactions/ }));

    await waitFor(() => {
      expect(screen.getAllByText('Proposed').length).toBeGreaterThan(0);
    });
    expect(screen.getAllByText(/in pool for 2m/).length).toBeGreaterThan(0);
    const blockLinks = screen
      .getAllByRole('link')
      .filter((link) => link.getAttribute('href')?.includes('/blocks/'));
    expect(blockLinks).toHaveLength(0);
  });

  it('gives the mobile transaction row the same pool badge and partial notice as desktop', async () => {
    vi.mocked(api.getAddressTransactions).mockResolvedValue({
      data: [
        {
          txHash: '0xfeed000000000000000000000000000000000000000000000000000000000003',
          blockNumber: null,
          txType: 'sent' as const,
          capacityChange: '-50000000000',
          timestamp: null,
          poolStatus: 'proposed' as const,
          timeAddedToPool: new Date(Date.now() - 2 * 60 * 1000).toISOString(),
          interpretation: {
            status: 'partial' as const,
            reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
          },
          inputsCount: 1,
          outputsCount: 2,
          fee: '1000',
          isCellbase: false,
          txSize: 500,
          cycles: 200000,
          scriptLabels: [],
        },
      ],
      limit: 50,
      hasMore: false,
      nextCursor: null,
      pool: { ...poolSummary, count: 1 },
    });

    render(<AddressDetailPage />);
    await waitFor(() => {
      expect(screen.getAllByText('Transactions').length).toBeGreaterThan(0);
    });
    fireEvent.click(screen.getByRole('button', { name: /Transactions/ }));

    const mobileRow = await screen.findByTestId('address-tx-row-mobile');
    expect(within(mobileRow).getByText('Proposed')).toBeInTheDocument();
    expect(
      within(mobileRow).getByText('partial: input not yet resolvable (0xabc:1)')
    ).toBeInTheDocument();
  });
});
