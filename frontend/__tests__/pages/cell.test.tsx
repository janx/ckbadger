import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/page';

const mockPush = vi.fn();

vi.mock('next/navigation', () => ({
  useParams: () => ({
    outpoint: '0xabc123def456789012345678901234567890123456789012345678901234abcd-0',
  }),
  useRouter: () => ({ push: mockPush }),
}));

const mockCellWithDao = {
  txHash: '0xabc123def456789012345678901234567890123456789012345678901234abcd',
  outputIndex: 0,
  capacity: '50000000000',
  lockScriptHash: '0xlockscripthash123456789012345678901234567890123456789012345678',
  address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
  typeScriptHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
  dataSize: 8,
  createdAtBlock: 5000000,
  status: 'live' as const,
  lock: {
    codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
    hashType: 'type',
    args: '0x1234567890123456789012345678901234567890',
  },
  type: {
    codeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
    hashType: 'type',
    args: '0x',
  },
  data: '0x0000000000000000',
  isDepGroup: false,
  daoInfo: {
    isDaoCell: true,
    daoStatus: 'deposited',
    depositBlockNumber: 5000000,
    depositTimestamp: '2024-01-15T10:30:00Z',
    withdrawRequestBlock: null,
    withdrawRequestTimestamp: null,
    withdrawBlock: null,
    withdrawTimestamp: null,
    compensation: null,
    compensationCkb: null,
    estimatedApc: null,
  },
};

const mockCellWithoutDao = {
  txHash: '0xdef456789012345678901234567890123456789012345678901234567890abcd',
  outputIndex: 0,
  capacity: '10000000000',
  lockScriptHash: '0xlockscripthash123456789012345678901234567890123456789012345678',
  address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq...',
  typeScriptHash: null,
  dataSize: 0,
  createdAtBlock: 5000000,
  status: 'live' as const,
  lock: {
    codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
    hashType: 'type',
    args: '0x1234567890123456789012345678901234567890',
  },
  type: null,
  data: null,
  isDepGroup: false,
};

const mockWithdrawnDaoCell = {
  ...mockCellWithDao,
  status: 'dead' as const,
  consumedAtBlock: 5100000,
  consumedByTx: '0xconsumeTx12345678901234567890123456789012345678901234567890abcdef',
  daoInfo: {
    isDaoCell: true,
    daoStatus: 'withdrawn',
    depositBlockNumber: 5000000,
    depositTimestamp: '2024-01-15T10:30:00Z',
    withdrawRequestBlock: 5050000,
    withdrawRequestTimestamp: '2024-02-15T10:30:00Z',
    withdrawBlock: 5100000,
    withdrawTimestamp: '2024-03-15T10:30:00Z',
    compensation: '123456789',
    compensationCkb: '1.23456789',
    estimatedApc: null,
  },
};

vi.mock('@/lib/api', () => ({
  api: {
    getCell: vi.fn(),
    getCellGraph: vi.fn(() => Promise.resolve({ nodes: [], links: [] })),
    lookupScripts: vi.fn(() => Promise.resolve({})),
  },
}));

import { api } from '@/lib/api';
const mockGetCell = api.getCell as ReturnType<typeof vi.fn>;

function renderWithQueryClient(component: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return render(<QueryClientProvider client={queryClient}>{component}</QueryClientProvider>);
}

describe('CellDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders DAO cell with Nervos DAO badge and info panel', async () => {
    mockGetCell.mockResolvedValue(mockCellWithDao);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      const daoElements = screen.getAllByText('Nervos DAO');
      expect(daoElements.length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.getByText('Active')).toBeInTheDocument();
    expect(screen.getByText('Deposit Block')).toBeInTheDocument();
    expect(screen.getByText('Deposit Time')).toBeInTheDocument();
    const blockLinks = screen.getAllByText('#5,000,000');
    expect(blockLinks.length).toBeGreaterThanOrEqual(1);
  });

  it('renders regular cell without DAO badge or info panel', async () => {
    mockGetCell.mockResolvedValue(mockCellWithoutDao);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Live')).toBeInTheDocument();
    });
  });

  it('renders withdrawn DAO cell with compensation info', async () => {
    mockGetCell.mockResolvedValue(mockWithdrawnDaoCell);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      const daoElements = screen.getAllByText('Nervos DAO');
      expect(daoElements.length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.getByText('Withdrawn')).toBeInTheDocument();
    expect(screen.getByText('Withdraw Request')).toBeInTheDocument();
    expect(screen.getByText('Withdrawn Block')).toBeInTheDocument();
    expect(screen.getByText('Compensation Earned')).toBeInTheDocument();
    const ckbElements = screen.getAllByText(/CKB$/);
    expect(ckbElements.length).toBeGreaterThanOrEqual(1);
  });

  it('renders withdrawing DAO cell status correctly', async () => {
    const withdrawingCell = {
      ...mockCellWithDao,
      daoInfo: {
        ...mockCellWithDao.daoInfo,
        daoStatus: 'withdrawing',
        withdrawRequestBlock: 5050000,
        withdrawRequestTimestamp: '2024-02-15T10:30:00Z',
      },
    };
    mockGetCell.mockResolvedValue(withdrawingCell);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Withdrawing')).toBeInTheDocument();
    });
  });
});
