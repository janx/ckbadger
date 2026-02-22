import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
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
  occupiedCapacity: 8600000000,
  occupiedCapacityBreakdown: {
    capacityFieldBytes: 8,
    lockScriptBytes: 53,
    typeScriptBytes: 17,
    dataBytes: 8,
    totalBytes: 86,
  },
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
  occupiedCapacity: 6100000000,
  occupiedCapacityBreakdown: {
    capacityFieldBytes: 8,
    lockScriptBytes: 53,
    typeScriptBytes: 0,
    dataBytes: 0,
    totalBytes: 61,
  },
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

const mockLargeDataCell = {
  ...mockCellWithDao,
  dataSize: 2048,
  data: `0x${'ab'.repeat(2048)}`,
};

const UNKNOWN_CODE_HASH = '0x709f3fda1234567890abcdef1234567890abcdef1234567890abcdefcce08649';
const DEPLOYMENT_TYPE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const DEPLOYMENT_DATA_HASH = '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649';

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
      const liveBadges = screen.getAllByText('Live');
      expect(liveBadges.length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.getByText('Capacity')).toBeInTheDocument();
    expect(screen.getByText('Total Capacity')).toBeInTheDocument();
    expect(screen.getByText('Occupied Capacity')).toBeInTheDocument();
    expect(screen.getByText('Utilization Ratio')).toBeInTheDocument();
    expect(screen.getByText('Byte Composition (61 bytes)')).toBeInTheDocument();
    expect(screen.getByText('Capacity Field')).toBeInTheDocument();
    expect(screen.getByText('Cell Data')).toBeInTheDocument();
    expect(screen.getByText(/^Formula:/)).toBeInTheDocument();
    expect(screen.getByText(/53B/)).toBeInTheDocument();

    const legend = screen.getByTestId('byte-composition-legend');

    expect(screen.queryByTestId('byte-composition-guides')).not.toBeInTheDocument();
    expect(legend.className).toContain('mt-2');
    expect(legend.querySelector('.grid')).toBeTruthy();

    expect(screen.getByText('Overview')).toBeInTheDocument();
    expect(screen.getByText('Address')).toBeInTheDocument();
    expect(screen.queryByText('Cell Relationship')).not.toBeInTheDocument();
    const sidePanels = screen.getByTestId('cell-side-panels');
    const addressPanel = within(sidePanels).getByTestId('cell-address-panel');
    const lockScriptHeader = within(sidePanels).getByText('Lock Script');
    expect(
      addressPanel.compareDocumentPosition(lockScriptHeader) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Lifecycle' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graph' })).toBeInTheDocument();
    expect(screen.getByTestId('cell-relationship-lifecycle')).toBeInTheDocument();
    expect(screen.getByText('Current Status')).toBeInTheDocument();
    expect(screen.getByText('Upstream Inputs (0)')).toBeInTheDocument();
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

  it('routes unknown script deployment entries to code-hash detail page', async () => {
    mockGetCell.mockResolvedValue({
      ...mockCellWithDao,
      codeCellOf: [
        {
          name: 'Unknown',
          codeHash: UNKNOWN_CODE_HASH,
          hashType: 'data',
          deploymentTypeHash: DEPLOYMENT_TYPE_HASH,
          deploymentDataHash: DEPLOYMENT_DATA_HASH,
        },
        {
          name: 'Default Lock',
          codeHash: DEPLOYMENT_TYPE_HASH,
          hashType: 'type',
          deploymentTypeHash: DEPLOYMENT_TYPE_HASH,
          deploymentDataHash: DEPLOYMENT_DATA_HASH,
        },
      ],
    });

    renderWithQueryClient(<CellDetailPage />);

    const fallbackLabel = `script: ${UNKNOWN_CODE_HASH.slice(0, 10)}...${UNKNOWN_CODE_HASH.slice(-8)}`;
    const unknownLink = await screen.findByRole('link', { name: fallbackLabel });
    expect(unknownLink).toHaveAttribute(
      'href',
      `/script/${DEPLOYMENT_TYPE_HASH}?hashType=type&kind=both`
    );
    expect(document.querySelector('a[href="/scripts/Unknown"]')).toBeNull();

    expect(screen.getByRole('link', { name: 'Default Lock' })).toHaveAttribute(
      'href',
      '/scripts/Default%20Lock'
    );
    expect(
      document.querySelector(`a[href="/script/${DEPLOYMENT_TYPE_HASH}?hashType=type&kind=both"]`)
    ).toBeTruthy();
    expect(
      document.querySelector(`a[href="/script/${DEPLOYMENT_DATA_HASH}?hashType=data&kind=both"]`)
    ).toBeTruthy();
    expect(screen.queryByText(/Deployment refs are shown as/i)).not.toBeInTheDocument();
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

  it('shows DATA header and truncates displayed bytes to first 1024 bytes', async () => {
    mockGetCell.mockResolvedValue(mockLargeDataCell);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('DATA')).toBeInTheDocument();
      expect(screen.getByText('Preview')).toBeInTheDocument();
      expect(screen.getByText('2,048 bytes')).toBeInTheDocument();
      expect(screen.getByText('Truncated at the 1,024-th byte')).toBeInTheDocument();
    });

    expect(screen.getByText('... 1,024 more bytes')).toBeInTheDocument();
  });
});
