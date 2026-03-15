import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { api } from '@/lib/api';

const TX_HASH = '0x57a54eb7922190d5b0e0d7f5ad91dbbd91714a9bd85200994f99250ddc08e0f';
const LOCK_CODE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const TYPE_CODE_HASH = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';
let mockSearchParams = new URLSearchParams();
const mockPush = vi.fn();
const mockReplace = vi.fn((url: string) => {
  const query = url.includes('?') ? url.split('?')[1] : '';
  mockSearchParams = new URLSearchParams(query);
});

function createCommittedTransactionDetail(): Awaited<ReturnType<typeof api.getTransactionDetail>> {
  return {
    hash: TX_HASH,
    status: 'committed',
    pendingSince: null,
    blockNumber: 18661531,
    blockHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    index: 0,
    inputsCount: 1,
    outputsCount: 1,
    fee: '58803',
    feeRate: '52130',
    txSize: 1128,
    isCellbase: false,
    timestamp: '2026-02-20T16:46:12Z',
    confirmations: 4,
    inputsCapacity: '55500000000',
    outputsCapacity: '55499941197',
    inputsUsedCapacity: '6100000000',
    outputsUsedCapacity: '6100000650',
    inputs: [
      {
        previousOutput: {
          txHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          index: 0,
        },
        capacity: '55500000000',
        lock: {
          codeHash: LOCK_CODE_HASH,
          hashType: 'type',
          args: '0x1111111111111111111111111111111111111111',
        },
        type: {
          codeHash: TYPE_CODE_HASH,
          hashType: 'type',
          args: '0x',
        },
        address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
      },
    ],
    outputs: [
      {
        capacity: '55499941197',
        usedCapacity: 61,
        lock: {
          codeHash: LOCK_CODE_HASH,
          hashType: 'type',
          args: '0x2222222222222222222222222222222222222222',
        },
        type: {
          codeHash: TYPE_CODE_HASH,
          hashType: 'type',
          args: '0x',
        },
        address: 'ckb1qypk9w2g8j6v4e5xj0k9n3l2m1p8q7r6s5t4u3v2w1x0y9z8a7b6c5d4e3',
      },
    ],
    witnesses: ['0x1b00000010000000160000001600000006000000112205000000aa', '0x64617301020304'],
    witnessesAvailable: true,
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
}

function createPendingTransactionDetail(): Awaited<ReturnType<typeof api.getTransactionDetail>> {
  return {
    ...createCommittedTransactionDetail(),
    status: 'pending',
    pendingSince: '2026-03-12T12:00:00Z',
    blockNumber: null,
    blockHash: null,
    index: null,
    timestamp: null,
    confirmations: null,
    inputsCapacity: null,
    outputsCapacity: null,
    inputsUsedCapacity: null,
    outputsUsedCapacity: null,
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
}

vi.mock('@/lib/api', () => ({
  api: {
    getTransactionDetail: vi.fn(),
    getTransactionGraph: vi.fn(),
    getTransactionCellDeps: vi.fn(),
    getTransactionLifecycle: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/hooks/useCyclesCalculation', () => ({
  useCyclesCalculation: () => ({
    cycles: 4446651,
    hasCycles: true,
    isCalculating: false,
    hasFailed: false,
  }),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/cell-graph', () => ({
  default: () => <div data-testid="mock-cell-graph">Graph Mock</div>,
  CellGraph: () => <div data-testid="mock-cell-graph">Graph Mock</div>,
}));

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ hash: TX_HASH }),
  usePathname: () => `/tx/${TX_HASH}`,
  useSearchParams: () => mockSearchParams,
  useRouter: () => ({ push: mockPush, replace: mockReplace }),
}));

describe('TransactionDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSearchParams = new URLSearchParams();

    vi.mocked(api.getTransactionDetail).mockResolvedValue(createCommittedTransactionDetail());

    vi.mocked(api.lookupScripts).mockResolvedValue({
      [LOCK_CODE_HASH]: {
        codeHash: LOCK_CODE_HASH,
        name: 'Default Multisig',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 10,
        liveCapacitySum: '100000000000',
        liveUsedCapacitySum: '60000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
      },
      [TYPE_CODE_HASH]: {
        codeHash: TYPE_CODE_HASH,
        name: 'Unknown',
        scriptKind: 'type',
        decoderType: null,
        hashType: 'type',
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 1,
        liveCapacitySum: '10000000000',
        liveUsedCapacitySum: '6000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
      },
    });

    vi.mocked(api.getTransactionGraph).mockResolvedValue({ nodes: [], links: [] });
    vi.mocked(api.getTransactionCellDeps).mockResolvedValue([]);
    vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
      hash: TX_HASH,
      phase: 'committed',
      proposalId: '0x01',
      proposedIn: null,
      committedIn: null,
      commitmentDistance: null,
      commitmentWindow: { close: 2, far: 10 },
      isCellbase: false,
      confirmations: 4,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders pending placeholders while committed-only queries stay disabled', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue(createPendingTransactionDetail());

    render(<TransactionDetailPage />);

    expect(await screen.findByText('Pending')).toBeInTheDocument();
    expect(screen.getAllByText('pending...').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Cell Deps' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graph' })).toBeInTheDocument();

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalledWith(TX_HASH);
    });

    expect(api.getTransactionGraph).not.toHaveBeenCalled();
    expect(api.getTransactionCellDeps).not.toHaveBeenCalled();
    expect(api.getTransactionLifecycle).not.toHaveBeenCalled();
  });

  it('polls pending transaction detail until committed and then loads chain-derived sections', async () => {
    vi.mocked(api.getTransactionDetail)
      .mockResolvedValueOnce(createPendingTransactionDetail())
      .mockResolvedValueOnce(createCommittedTransactionDetail());

    render(<TransactionDetailPage />);

    expect(await screen.findByText('Pending')).toBeInTheDocument();
    expect(api.getTransactionGraph).not.toHaveBeenCalled();
    expect(api.getTransactionCellDeps).not.toHaveBeenCalled();
    expect(api.getTransactionLifecycle).not.toHaveBeenCalled();

    await waitFor(
      () => {
        expect(api.getTransactionDetail).toHaveBeenCalledTimes(2);
      },
      { timeout: 5000 }
    );
    expect(await screen.findByText('4 Confirmations')).toBeInTheDocument();
    await waitFor(() => {
      expect(api.getTransactionGraph).toHaveBeenCalledWith(TX_HASH);
    });
    expect(api.getTransactionCellDeps).toHaveBeenCalledWith(TX_HASH);
    expect(api.getTransactionLifecycle).toHaveBeenCalledWith(TX_HASH);
  }, 10000);

  it('links unknown type script to code-hash detail page', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    fireEvent.click(await screen.findByRole('button', { name: 'Scripts' }, { timeout: 5000 }));

    const link = document.querySelector(
      `a[href="/script/${TYPE_CODE_HASH}?hashType=type&kind=type"]`
    );
    expect(link).not.toBeNull();
    expect(document.querySelector('a[href="/scripts/Unknown"]')).toBeNull();
  }, 10000);

  it('does not render hash-only fallback label in IO tab', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    const fallbackLabel = `type: ${TYPE_CODE_HASH.slice(0, 10)}...${TYPE_CODE_HASH.slice(-8)}`;
    expect(screen.queryByRole('link', { name: fallbackLabel })).toBeNull();
    expect(screen.queryByText(fallbackLabel)).toBeNull();
  });

  it('shows flow view by default and switches to graph view in graph tab', async () => {
    vi.mocked(api.getTransactionGraph).mockResolvedValue({
      nodes: [
        {
          id: `tx-${TX_HASH}`,
          nodeType: 'transaction',
          label: 'TX',
          data: { hash: TX_HASH, blockNumber: 18661531 },
        },
        {
          id: `cell-${TX_HASH}-0`,
          nodeType: 'cell',
          label: '554.99 CKB',
          data: { txHash: TX_HASH, outputIndex: 0, status: 'live', capacity: '55499941197' },
        },
      ],
      links: [
        {
          source: `tx-${TX_HASH}`,
          target: `cell-${TX_HASH}-0`,
          linkType: 'output',
        },
      ],
    });

    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    fireEvent.click(await screen.findByRole('button', { name: 'Graph' }));

    expect(await screen.findByRole('button', { name: 'Flow View' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graph View' })).toBeInTheDocument();
    expect(screen.getByTestId('tx-relationship-flow')).toBeInTheDocument();
    expect(screen.getByText('Transaction Flow Snapshot')).toBeInTheDocument();
    expect(screen.getByText('2 nodes / 1 links')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Graph View' }));

    expect(screen.getByText('Loading graph section...')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.queryByTestId('tx-relationship-flow')).not.toBeInTheDocument();
    });
    expect(await screen.findByTestId('mock-cell-graph')).toBeInTheDocument();
  });

  it('renders witness tab with deterministic decode and byte interaction', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    expect(await screen.findByTestId('tx-witness-tab')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-witness-item-0')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-item-1')).toBeInTheDocument();
    expect(screen.getByText('Script Groups')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-selection-empty')).toBeInTheDocument();
    expect(screen.queryByTestId('tx-witness-deterministic-section')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).toContain('witness=0');
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('border-emphasis/70');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('border-emphasis/70');
    expect(screen.getByTestId('tx-witness-deterministic-section')).toBeInTheDocument();
    expect(screen.getByText('WitnessArgs')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-witness-segment-item-1'));
    expect(screen.getByTestId('tx-witness-active-segment')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-active-segment-value')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-bytes-grid')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-script-group-focus-0-type'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).toContain('wg=');
    expect(screen.getByTestId('tx-witness-focused-group')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-focused-group')).toHaveTextContent(
      /Focused script group:/
    );
    expect(screen.getByTestId('tx-witness-segment-pinned')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-active-segment')).toHaveTextContent(
      /(inputType|outputType)/
    );

    fireEvent.click(screen.getByRole('button', { name: 'Clear focus' }));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.queryByTestId('tx-witness-focused-group')).not.toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-witness-item-1'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).toContain('witness=1');
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.getByText('DASWitness')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');
    fireEvent.click(screen.getByTestId('tx-witness-heuristic-item-0'));
    expect(screen.getByTestId('tx-witness-heuristic-detail-0')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-witness-clear-selection'));
    const lastReplaceCall = String(mockReplace.mock.calls.at(-1)?.[0]);
    expect(lastReplaceCall).not.toContain('witness=');
    expect(lastReplaceCall).not.toContain('wg=');
    expect(screen.queryByTestId('tx-witness-focused-group')).not.toBeInTheDocument();
    expect(screen.queryByTestId('tx-witness-deterministic-section')).not.toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-selection-empty')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');
  });

  it('toggles highlighted witness, script group, input, and output off on second click', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    expect(await screen.findByTestId('tx-witness-tab')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('witness=');
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.getByTestId('tx-witness-selection-empty')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-script-group-focus-0-type'));
    expect(screen.getByTestId('tx-witness-focused-group')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-script-group-focus-0-type'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('witness=');
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.queryByTestId('tx-witness-focused-group')).not.toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-selection-empty')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('io-linked-highlight');
    fireEvent.click(screen.getByTestId('tx-io-input-0'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('witness=');
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.getByTestId('tx-witness-selection-empty')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('io-linked-highlight');
    fireEvent.click(screen.getByTestId('tx-io-output-0'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('witness=');
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.getByTestId('tx-witness-selection-empty')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');
  });

  it('shows not found message for 404 transaction errors', async () => {
    vi.mocked(api.getTransactionDetail).mockRejectedValueOnce(new Error('API error: 404'));

    render(<TransactionDetailPage />);

    expect(await screen.findByText('Transaction not found')).toBeInTheDocument();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
  });

  it('shows detailed message for non-404 transaction errors', async () => {
    vi.mocked(api.getTransactionDetail).mockRejectedValueOnce(
      new Error(
        'API error: 500 - transaction exists in CKB RocksDB but tx index mapping is missing: tx_hash=0xabc, block_number=42'
      )
    );

    render(<TransactionDetailPage />);

    expect(await screen.findByText('Failed to load transaction')).toBeInTheDocument();
    expect(
      screen.getByText(
        /transaction exists in CKB RocksDB but tx index mapping is missing: tx_hash=0xabc, block_number=42/
      )
    ).toBeInTheDocument();
  });
});
