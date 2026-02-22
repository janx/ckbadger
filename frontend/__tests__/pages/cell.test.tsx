import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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

const mockCellWithDataAnalysis = {
  ...mockCellWithDao,
  dataSize: 16,
  data: '0x2a000000000000000000000000000000',
  dataAnalysis: {
    deterministic: {
      kind: 'udt_amount',
      summary: 'SUDT cell data starts with amount=42 (u128 LE)',
      segments: [
        {
          label: 'amount',
          start: 0,
          end: 16,
          meaning: 'SUDT amount in little-endian u128',
          humanValue: '42',
        },
      ],
    },
    heuristicGuesses: [
      {
        kind: 'numeric_pattern',
        confidence: 'medium',
        reason: 'Payload length is exactly 16 bytes (common u128 LE encoding)',
        humanValue: '42',
      },
    ],
  },
};

const mockCellWithPartialParsedData = {
  ...mockCellWithDao,
  dataSize: 16,
  data: '0x0102030405060708090a0b0c0d0e0f10',
  dataAnalysis: {
    deterministic: {
      kind: 'partial_demo',
      summary: 'Only first 8 bytes are deterministically parsed',
      segments: [
        {
          label: 'header',
          start: 0,
          end: 8,
          meaning: 'Demo parsed header',
          humanValue: '0x0102030405060708',
        },
      ],
    },
    heuristicGuesses: [],
  },
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

  it('renders deterministic/heuristic data analysis and supports byte hover highlighting', async () => {
    mockGetCell.mockResolvedValue(mockCellWithDataAnalysis);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByTestId('data-deterministic-section')).toBeInTheDocument();
      expect(screen.getByText('Deterministic Decode')).toBeInTheDocument();
      expect(screen.getByText('Heuristic Guesses')).toBeInTheDocument();
      expect(screen.getByText('1 segments')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('data-analysis-panel')).not.toBeInTheDocument();
    expect(screen.queryByTestId('data-deterministic-panel')).not.toBeInTheDocument();
    expect(screen.queryByTestId('data-heuristic-panel')).not.toBeInTheDocument();

    fireEvent.mouseEnter(screen.getByTestId('data-byte-0'));
    expect(screen.getByTestId('data-active-segment-value')).toHaveTextContent('42');
    expect(screen.getByTestId('data-byte-0').className).toContain('byte-hover-breathe');
    expect(screen.getByTestId('data-segment-item-0')).toBeInTheDocument();
  });

  it('keeps segment detail stable while moving inside byte grid and clears when leaving grid', async () => {
    mockGetCell.mockResolvedValue(mockCellWithDataAnalysis);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByTestId('data-byte-0')).toBeInTheDocument();
      expect(screen.getByTestId('data-bytes-grid')).toBeInTheDocument();
      expect(screen.getByTestId('data-active-segment')).toBeInTheDocument();
    });

    expect(screen.getByTestId('data-active-segment').className).toContain('h-[132px]');

    const byte0 = screen.getByTestId('data-byte-0');
    const byte1 = screen.getByTestId('data-byte-1');
    const byteGrid = screen.getByTestId('data-bytes-grid');

    fireEvent.mouseEnter(byte0);
    expect(screen.getByTestId('data-active-segment-value')).toHaveTextContent('42');

    fireEvent.mouseLeave(byte0, { relatedTarget: byte1 });
    fireEvent.mouseEnter(byte1);
    expect(screen.getByTestId('data-active-segment-value')).toHaveTextContent('42');

    fireEvent.mouseLeave(byteGrid);
    expect(screen.getByTestId('data-byte-0').className).not.toContain('byte-hover-breathe');
    expect(
      screen.getByText('Hover a segment/byte to preview it, or click a segment to pin it.')
    ).toBeInTheDocument();
  });

  it('toggles heuristic detail expansion in compact mode', async () => {
    mockGetCell.mockResolvedValue(mockCellWithDataAnalysis);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByTestId('data-heuristics-list')).toBeInTheDocument();
      expect(screen.getByTestId('data-heuristic-item-0')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('data-heuristic-detail-0')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('data-heuristic-item-0'));
    expect(screen.getByTestId('data-heuristic-detail-0')).toBeInTheDocument();
    expect(
      screen.getByText('Payload length is exactly 16 bytes (common u128 LE encoding)')
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('data-heuristic-item-0'));
    expect(screen.queryByTestId('data-heuristic-detail-0')).not.toBeInTheDocument();
  });

  it('pins deterministic segment on click and keeps details visible after mouse leave', async () => {
    mockGetCell.mockResolvedValue(mockCellWithDataAnalysis);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByTestId('data-segment-item-0')).toBeInTheDocument();
    });

    const segmentItem = screen.getByTestId('data-segment-item-0');
    fireEvent.click(segmentItem);
    expect(screen.getByTestId('data-segment-pinned')).toBeInTheDocument();

    fireEvent.mouseLeave(segmentItem);
    expect(screen.getByTestId('data-active-segment-value')).toHaveTextContent('42');
    expect(screen.getByTestId('data-active-segment-hex')).toHaveTextContent(
      '0x2a000000000000000000000000000000'
    );
  });

  it('removes coverage/unparsed/filter controls from data detail section', async () => {
    mockGetCell.mockResolvedValue(mockCellWithPartialParsedData);

    renderWithQueryClient(<CellDetailPage />);

    await waitFor(() => {
      expect(screen.getByText('Deterministic Decode')).toBeInTheDocument();
    });

    expect(screen.queryByText('Parsed Coverage (Full Payload)')).not.toBeInTheDocument();
    expect(screen.queryByText('Parsed Coverage (Preview Window)')).not.toBeInTheDocument();
    expect(screen.queryByTestId('data-coverage-grid')).not.toBeInTheDocument();
    expect(screen.queryByTestId('data-unparsed-ranges')).not.toBeInTheDocument();
    expect(screen.queryByTestId('data-byte-filter')).not.toBeInTheDocument();
  });
});
