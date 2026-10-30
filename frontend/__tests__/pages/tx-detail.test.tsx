import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, screen, waitFor, fireEvent, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { api, ApiRequestError, type TransactionCellDisplay } from '@/lib/api';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';

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
    blockNumber: 18661531,
    blockHash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    index: 0,
    inputsCount: 1,
    outputsCount: 1,
    fee: '58803',
    feeRate: '52130',
    txSize: 1128,
    isCellbase: false,
    cycles: 1000,
    cyclesStatus: 'complete',
    timestamp: '2026-02-20T16:46:12Z',
    confirmations: 4,
    inputsCapacity: '55500000000',
    outputsCapacity: '55499941197',
    inputsCommonKnowledgeSize: '94',
    outputsCommonKnowledgeSize: '94',
    inputs: [
      {
        previousOutput: {
          txHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          index: 0,
        },
        capacity: '55500000000',
        commonKnowledgeSize: '94',
        cellType: 'typed',
        display: null,
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
        commonKnowledgeSize: '94',
        status: 'live',
        display: null,
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
    scriptGroups: [
      {
        scriptHash: '0xinputlock',
        kind: 'lock',
        ran: true,
        inputIndices: [0],
        outputIndices: [],
        witnessIndex: 0,
        script: {
          codeHash: LOCK_CODE_HASH,
          hashType: 'type',
          args: '0x1111111111111111111111111111111111111111',
        },
        cycles: '100',
      },
      {
        scriptHash: '0xtypescript',
        kind: 'type',
        ran: true,
        inputIndices: [0],
        outputIndices: [0],
        witnessIndex: 0,
        script: { codeHash: TYPE_CODE_HASH, hashType: 'type', args: '0x' },
        cycles: null,
      },
      {
        scriptHash: '0xoutputlock',
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: [0],
        witnessIndex: null,
        script: {
          codeHash: LOCK_CODE_HASH,
          hashType: 'type',
          args: '0x2222222222222222222222222222222222222222',
        },
        cycles: null,
      },
    ],
    witnessDetails: [
      {
        index: 0,
        size: 22,
        kind: 'witness_args',
        fields: [
          { name: 'header', start: 0, end: 16, hex: '0x16000000100000001600000016000000' },
          { name: 'lock length', start: 16, end: 20, hex: '0x02000000' },
          { name: 'lock', start: 20, end: 22, hex: '0x1122' },
        ],
      },
      {
        index: 1,
        size: 7,
        kind: 'opaque',
        fields: [{ name: 'raw', start: 0, end: 7, hex: '0x64617301020304' }],
      },
    ],
    witnesses: ['0x16000000100000001600000016000000020000001122', '0x64617301020304'],
    witnessesAvailable: true,
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
}

/** The keys the API omits (never nulls) for a transaction it has not indexed (TX-06). */
const UNINDEXED_KEYS = [
  'blockNumber',
  'blockHash',
  'index',
  'timestamp',
  'confirmations',
  'inputsCapacity',
  'outputsCapacity',
  'inputsCommonKnowledgeSize',
  'outputsCommonKnowledgeSize',
] as const;

function createPendingTransactionDetail(): Awaited<ReturnType<typeof api.getTransactionDetail>> {
  // Match the API wire shape for pending txs: `skip_serializing_if` omits these keys
  // (undefined), not JSON null — that previously crashed the page.
  const detail = {
    ...createCommittedTransactionDetail(),
    status: 'pending',
    pendingSince: '2026-03-12T12:00:00Z',
    feeRate: null,
    txSize: null,
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
  for (const key of UNINDEXED_KEYS) delete detail[key];
  return detail;
}

/**
 * What `/tx/{hash}` serves for a transaction the node has committed but this
 * explorer's store has not indexed yet: the committing block is known, the
 * block time, confirmations and tx index are not.
 */
function createCommittedAwaitingIndexTransactionDetail(): Awaited<
  ReturnType<typeof api.getTransactionDetail>
> {
  const detail = {
    ...createCommittedTransactionDetail(),
    status: 'committed',
    poolStatus: 'committed_awaiting_index',
    blockNumber: 123,
    blockHash: '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
  for (const key of ['index', 'timestamp', 'confirmations'] as const) delete detail[key];
  return detail;
}

vi.mock('@/lib/api', async () => {
  // The error class and its classifier are the real ones: the page must key on
  // exactly what the API client produces for a 503.
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    api: {
      getTransactionDetail: vi.fn(),
      getTransactionGraph: vi.fn(),
      getTransactionActivity: vi.fn(),
      getTransactionCellDeps: vi.fn(),
      getTransactionLifecycle: vi.fn(),
      lookupScripts: vi.fn(),
    },
    ApiRequestError: actual.ApiRequestError,
    isServiceUnavailableError: actual.isServiceUnavailableError,
    isWarmupPendingError: vi.fn(() => false),
    isNetworkInitializingError: vi.fn(() => false),
  };
});

/** What `/tx/{hash}` answers while a pending transaction's parent is unknown to the node. */
function inputsNotYetResolvable() {
  return new ApiRequestError(
    503,
    'service_unavailable',
    `inputs of uncommitted transaction ${TX_HASH} are not resolvable yet: the node does not know the transaction that created 0x${'d'.repeat(64)}:0; retry`
  );
}

vi.mock('@/hooks/useCyclesCalculation', () => ({
  useCyclesCalculation: vi.fn(() => ({
    cycles: 4446651,
    hasCycles: true,
    isCalculating: false,
    hasFailed: false,
    isPaused: false,
  })),
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
    vi.mocked(useCyclesCalculation).mockReturnValue({
      cycles: 4446651,
      hasCycles: true,
      isCalculating: false,
      hasFailed: false,
      isPaused: false,
    });
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
        ownedCapacitySum: '100000000000',
        ownedKnowledgeSum: '60000000000',
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
        ownedCapacitySum: '10000000000',
        ownedKnowledgeSum: '6000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
      },
    });

    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.objectTransfer,
      txHash: TX_HASH,
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

  it('requests missing executed-group cycles without discarding the measured transaction total', async () => {
    render(<TransactionDetailPage />);
    await waitFor(() => {
      expect(useCyclesCalculation).toHaveBeenLastCalledWith(TX_HASH, 1000, false, 'complete', true);
    });
  });

  it('labels in-block index as position, not commitment distance (TX-07)', async () => {
    vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
      hash: TX_HASH,
      phase: 'committed',
      proposalId: '0x01',
      proposedIn: {
        blockNumber: 99,
        blockHash: `0x${'11'.repeat(32)}`,
        timestamp: '2026-02-20T16:40:00Z',
      },
      committedIn: {
        blockNumber: 101,
        blockHash: `0x${'22'.repeat(32)}`,
        timestamp: '2026-02-20T16:46:12Z',
      },
      commitmentDistance: 2,
      commitmentWindow: { close: 2, far: 10 },
      isCellbase: false,
      confirmations: 4,
    });
    const detail = createCommittedTransactionDetail();
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...detail,
      index: 1,
      blockNumber: 101,
    });
    render(<TransactionDetailPage />);
    const line = await screen.findByText(/position 1/);
    expect(line).toHaveTextContent(/committed 2 blocks later/);
    expect(line).toHaveTextContent(/window 2–10/);
    expect(screen.queryByText(/position 2/)).not.toBeInTheDocument();
  });

  it('shows commit time as UTC with a relative age, not a raw ISO Z string (TX-10)', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-02-20T17:46:12Z'));
    render(<TransactionDetailPage />);
    const time = await screen.findByRole('time');
    expect(time).toHaveTextContent('2026-02-20 16:46:12 UTC');
    expect(time).toHaveTextContent(/· 1h ago/);
    expect(time.textContent).not.toMatch(/Z/);
  });

  it.each([null, 1000])('shows deep-fork replay pause with scalar cycles %s', async (cycles) => {
    vi.mocked(useCyclesCalculation).mockReturnValue({
      cycles,
      hasCycles: cycles !== null,
      isCalculating: false,
      hasFailed: false,
      isPaused: true,
    });
    render(<TransactionDetailPage />);
    expect(await screen.findByText('Paused: indexer is on a deep fork')).toBeInTheDocument();
    expect(screen.queryByText('Calculation failed')).not.toBeInTheDocument();
    expect(screen.queryByText('Calculating...')).not.toBeInTheDocument();
  });
  it('marks genesis transactions as outside canonical replay and keeps zero cycles', async () => {
    const tx = createCommittedTransactionDetail();
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...tx,
      blockNumber: 0,
      cycles: 0,
      cyclesStatus: 'complete',
      scriptGroups: tx.scriptGroups!.map((group) => ({ ...group, ran: false, cycles: null })),
    });
    render(<TransactionDetailPage />);
    await waitFor(() => {
      expect(useCyclesCalculation).toHaveBeenLastCalledWith(TX_HASH, 0, true, 'complete', false);
    });
  });

  it('renders pending placeholders while committed-only queries stay disabled', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue(createPendingTransactionDetail());

    render(<TransactionDetailPage />);

    expect((await screen.findAllByText('Pending')).length).toBeGreaterThan(0);
    expect(screen.getAllByText('pending...').length).toBeGreaterThan(0);
    expect(screen.getByRole('heading', { name: 'Cell deps' })).toBeInTheDocument();
    expect(screen.getByText('Cell graph')).toBeInTheDocument();

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalledWith(TX_HASH);
    });

    expect(api.getTransactionGraph).not.toHaveBeenCalled();
    expect(api.getTransactionCellDeps).not.toHaveBeenCalled();
    expect(api.getTransactionLifecycle).not.toHaveBeenCalled();
    expect(vi.mocked(useCyclesCalculation).mock.lastCall?.[2]).toBe(true);
    expect(vi.mocked(useCyclesCalculation).mock.lastCall?.[4]).toBe(false);
  });

  it('serves a committed-but-unindexed transaction provisionally and polls until it is indexed', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.getTransactionDetail)
      .mockResolvedValueOnce(createCommittedAwaitingIndexTransactionDetail())
      .mockResolvedValueOnce(createCommittedAwaitingIndexTransactionDetail())
      .mockResolvedValue(createCommittedTransactionDetail());

    render(<TransactionDetailPage />);

    // The committing block is known and shown; the block time and the
    // confirmation count are not known yet and are shown as pending.
    expect((await screen.findAllByRole('link', { name: '#123' })).length).toBeGreaterThan(0);
    expect(screen.getAllByText('Confirming').length).toBeGreaterThan(0);
    expect(screen.getAllByText('pending...').length).toBeGreaterThan(0);
    expect(screen.queryByText(/Confirmations$/)).not.toBeInTheDocument();
    expect(api.getTransactionDetail).toHaveBeenCalledTimes(1);

    // Still provisional: the page keeps asking until the store has it.
    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(2));
    expect(screen.getAllByRole('link', { name: '#123' }).length).toBeGreaterThan(0);

    // Indexed: `poolStatus` is gone, the committed view renders, polling stops.
    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(3));
    expect(await screen.findByText(/4\s+Confirmations/)).toBeInTheDocument();
    expect(screen.queryByText('Confirming')).not.toBeInTheDocument();

    await act(() => vi.advanceTimersByTimeAsync(9000));
    expect(api.getTransactionDetail).toHaveBeenCalledTimes(3);
  });

  it('keeps polling through a transient 503 for unresolvable inputs, then renders the transaction', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.getTransactionDetail)
      .mockRejectedValueOnce(inputsNotYetResolvable())
      .mockResolvedValue(createPendingTransactionDetail());

    render(<TransactionDetailPage />);

    // Provisional, not failed: a placeholder that says what it waits for.
    expect(await screen.findByTestId('tx-inputs-not-yet-resolvable')).toBeInTheDocument();
    expect(screen.getAllByText('Pending').length).toBeGreaterThan(0);
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
    expect(api.getTransactionDetail).toHaveBeenCalledTimes(1);

    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(2));

    expect(await screen.findByText('Pending Since')).toBeInTheDocument();
    expect(screen.queryByTestId('tx-inputs-not-yet-resolvable')).not.toBeInTheDocument();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
  });

  it('keeps showing a pending transaction when a later poll answers 503 for its inputs', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.getTransactionDetail)
      .mockResolvedValueOnce(createPendingTransactionDetail())
      .mockRejectedValueOnce(inputsNotYetResolvable())
      .mockResolvedValue(createPendingTransactionDetail());

    render(<TransactionDetailPage />);
    expect(await screen.findByText('Pending Since')).toBeInTheDocument();

    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(2));
    expect(screen.getByText('Pending Since')).toBeInTheDocument();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();

    // Still polling after the 503.
    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(3));
  });

  it('treats any other 503 as a failure, not as a pending transaction', async () => {
    vi.mocked(api.getTransactionDetail).mockRejectedValueOnce(
      new ApiRequestError(503, 'unknown_error', 'upstream down')
    );

    render(<TransactionDetailPage />);

    expect(await screen.findByText('Failed to load transaction')).toBeInTheDocument();
    expect(screen.queryByTestId('tx-inputs-not-yet-resolvable')).not.toBeInTheDocument();
  });

  it('links unknown type script to code-hash detail page', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    fireEvent.click(await screen.findByText('Raw data'));
    fireEvent.click(screen.getByText('Scripts'));

    const link = document.querySelector(
      `a[href="/mainnet/script/${TYPE_CODE_HASH}?hashType=type&kind=type"]`
    );
    expect(link).not.toBeNull();
    expect(document.querySelector('a[href="/scripts/Unknown"]')).toBeNull();
  }, 10000);

  it('uses domain cell labels without inventing an unknown script name', async () => {
    render(<TransactionDetailPage />);

    await waitFor(() => {
      expect(api.getTransactionDetail).toHaveBeenCalled();
    });

    const fallbackLabel = `type: ${TYPE_CODE_HASH.slice(0, 10)}...${TYPE_CODE_HASH.slice(-8)}`;
    expect(screen.queryByRole('link', { name: fallbackLabel })).toBeNull();
    expect(screen.queryByText(fallbackLabel)).toBeNull();
  });

  it('shows canonical cell structure first and loads the raw cell graph only when opened', async () => {
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('tx-io-input-0')).toBeInTheDocument();
    expect(screen.getByLabelText('Cells flow through the capacity pool')).toBeInTheDocument();
    expect(api.getTransactionGraph).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('Raw data'));
    fireEvent.click(screen.getByText('Cell graph'));
    expect(await screen.findByTestId('mock-cell-graph', {}, { timeout: 5000 })).toBeInTheDocument();
    expect(api.getTransactionGraph).toHaveBeenCalledWith(TX_HASH);
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
    expect(screen.getByText('Script groups')).toBeInTheDocument();
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

    fireEvent.mouseEnter(screen.getByRole('button', { name: 'lock bytes 20–21' }));
    expect(screen.getByText('lock: 0x1122')).toBeInTheDocument();
    expect(document.querySelector('[data-byte-offset="20"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );

    fireEvent.click(screen.getByTestId('tx-script-group-focus-0-type'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).toContain('wg=');
    expect(screen.getByTestId('tx-witness-focused-group')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-focused-group')).toHaveTextContent(
      /Focused script group:/
    );
    fireEvent.click(screen.getByRole('button', { name: 'Clear focus' }));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.queryByTestId('tx-witness-focused-group')).not.toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('io-linked-highlight');

    fireEvent.click(screen.getByTestId('tx-witness-item-1'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).toContain('witness=1');
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('wg=');
    expect(screen.getByText('Opaque witness bytes')).toBeInTheDocument();
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('io-linked-highlight');
    expect(screen.queryByText('DASWitness')).not.toBeInTheDocument();

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

  it('labels unavailable witness bytes without claiming the transaction has zero witnesses', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...createCommittedTransactionDetail(),
      witnesses: [],
      witnessesAvailable: false,
    });
    render(<TransactionDetailPage />);
    expect(await screen.findByText('Witnesses · unavailable')).toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-tab')).toHaveTextContent('Witness bytes are unavailable');
  });

  it('decodes .cell records next to the witness that carries them', async () => {
    const address =
      'ckt1qrfrwcdnvssswdwpn3s9v8fp87emat306ctjwsm3nmlkjg8qyza2cqgqq9x75zu4l7gld606r6eyd00m4lzy3zkxkq4nywzu';
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...createCommittedTransactionDetail(),
      dotcellNames: [
        {
          outputIndex: 1,
          label: 'joaom',
          name: 'joaom.cell',
          identityId: '0x241e3586a41eb75dd6d68bf555acea74d6649ed5',
          recordsHash: '0x3b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb18',
          records: [
            {
              key: 'address.309',
              label: '',
              valueHex: '0x636b7431',
              valueUtf8: address,
              ttl: 300,
              decodedAddress: {
                address,
                lockHash: '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d',
              },
            },
            {
              key: 'profile.email',
              label: '',
              valueHex: '0x6d61726961406578616d706c652e636f6d',
              valueUtf8: 'maria@example.com',
              ttl: 300,
              decodedAddress: null,
            },
          ],
        },
      ],
    });

    render(<TransactionDetailPage />);

    expect(await screen.findByTestId('tx-witness-tab')).toBeInTheDocument();
    // The witness list marks the witness at the name cell's own output index.
    expect(
      within(screen.getByTestId('tx-witness-item-1')).getByText('.cell records')
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId('tx-witness-item-0')).queryByText('.cell records')
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId('tx-witness-dotcell-records')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-witness-item-1'));
    const section = screen.getByTestId('tx-witness-dotcell-records');
    expect(section).toHaveTextContent('records payload of joaom.cell (output #1)');
    expect(within(section).getByRole('link', { name: 'joaom.cell' })).toHaveAttribute(
      'href',
      '/mainnet/identities/dotcell/joaom'
    );
    // Header row + the two records.
    expect(within(section).getAllByRole('row')).toHaveLength(3);
    expect(within(section).getByText('maria@example.com')).toBeInTheDocument();
    // Server-owned witness interpretation still renders beside the verified records.
    expect(screen.getByText('Opaque witness bytes')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(screen.queryByTestId('tx-witness-dotcell-records')).not.toBeInTheDocument();
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

  it('renders exact occupied bytes, output liveness, and output locks that did not run', async () => {
    render(<TransactionDetailPage />);
    const output = await screen.findByTestId('tx-io-output-0');
    expect(output).toHaveTextContent('occupied 94.00000000 CKB');
    expect(output).toHaveTextContent('live');
    expect(screen.getByText('occupied 94 → 94 bytes')).toBeInTheDocument();
    expect(screen.getByText('attached, runs when spent')).toBeInTheDocument();
    expect(screen.getByText('2 ran')).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: 'Balance changes' })).toBeInTheDocument();
    expect(api.getTransactionActivity).toHaveBeenCalledWith(TX_HASH);
  });

  it('links expanded dep groups and highlights their exact script members', async () => {
    vi.mocked(api.getTransactionCellDeps).mockResolvedValue([
      {
        outPointTxHash: '0xdep',
        outPointIndex: 2,
        depType: 'dep_group',
        scriptHashes: ['0xinputlock'],
        resolvedCells: [
          {
            txHash: '0xmember',
            index: 3,
            dataHash: '0xdata',
            typeHash: null,
            scriptHashes: ['0xinputlock'],
          },
        ],
      },
    ]);
    render(<TransactionDetailPage />);
    const member = await screen.findByRole('link', { name: '↳ 0xmember:3' });
    expect(member).toHaveAttribute('href', '/mainnet/cell/0xmember-3');
    fireEvent.mouseEnter(member.closest('#tx-dep-0')!);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'false');
    fireEvent.mouseLeave(member.closest('#tx-dep-0')!);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'false');
  });

  it('keeps a same-hash output lock separate from the executed input lock during hover', async () => {
    const tx = createCommittedTransactionDetail();
    tx.scriptGroups![2].scriptHash = tx.scriptGroups![0].scriptHash;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);
    fireEvent.mouseEnter(await screen.findByTestId('tx-io-input-0'));
    expect(document.querySelector('[data-script-group="lock:run:0xinputlock"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(
      document.querySelector('[data-script-group="lock:attached:0xinputlock"]')
    ).toHaveAttribute('data-highlighted', 'false');
  });

  it('links one-to-one CKBFS file continuity to its cells and exact type group', async () => {
    const tx = createCommittedTransactionDetail();
    tx.inputs![0].cellType = 'ckbfs';
    tx.outputs![0].cellType = 'ckbfs';
    const display: TransactionCellDisplay = {
      kind: 'file',
      standard: 'ckbfs',
      id: '0xtypescript',
      name: '3X0125NMNVAP033202.LOGS',
      collectionId: null,
      collectionName: null,
      symbol: null,
      decimals: null,
      amount: null,
      compositionTier: null,
    };
    tx.inputs![0].display = display;
    tx.outputs![0].display = display;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);

    const continuity = await screen.findByRole('button', {
      name: 'in:0 → out:0 · same file cell continues',
    });
    fireEvent.mouseEnter(continuity);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'true');
    expect(document.querySelector('path[data-continuity-kind="file"]')).toHaveAttribute(
      'stroke',
      '#a0a8be'
    );
    expect(document.querySelector('[data-script-group="type:run:0xtypescript"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    fireEvent.mouseLeave(continuity);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'false');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'false');
  });

  it.each(['different identity', 'multiple outputs'])(
    'does not infer file continuity with %s',
    async (reason) => {
      const tx = createCommittedTransactionDetail();
      const display: TransactionCellDisplay = {
        kind: 'file',
        standard: 'ckbfs',
        id: '0xtypescript',
        name: 'identical-name.txt',
        collectionId: null,
        collectionName: null,
        symbol: null,
        decimals: null,
        amount: null,
        compositionTier: null,
      };
      tx.inputs![0].display = display;
      tx.outputs![0].display =
        reason === 'different identity' ? { ...display, id: '0xanother' } : display;
      if (reason === 'multiple outputs') {
        tx.outputs!.push({ ...tx.outputs![0] });
        tx.scriptGroups![1].outputIndices = [0, 1];
      }
      vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
      render(<TransactionDetailPage />);
      await screen.findByTestId('tx-io-input-0');
      expect(screen.queryByRole('list', { name: 'Cell continuity' })).not.toBeInTheDocument();
    }
  );

  it.each([
    {
      label: 'object name, identity and verified media tier',
      display: {
        kind: 'object',
        standard: 'spore',
        id: `0x${'5'.repeat(64)}`,
        name: null,
        collectionId: `0x${'6'.repeat(64)}`,
        collectionName: 'Nervape 3x',
        symbol: null,
        decimals: null,
        amount: null,
        compositionTier: 'btc_ckb',
      } satisfies TransactionCellDisplay,
      expected: ['Spore', 'Nervape 3x', '0x55555555...555555', 'BTC+CKB'],
      href: `/mainnet/objects/0x${'5'.repeat(64)}`,
    },
    {
      label: 'exact CKBFS filename',
      display: {
        kind: 'file',
        standard: 'ckbfs',
        id: `0x${'7'.repeat(64)}`,
        name: '3X0125NMNVAP033202.LOGS',
        collectionId: null,
        collectionName: null,
        symbol: null,
        decimals: null,
        amount: null,
        compositionTier: null,
      } satisfies TransactionCellDisplay,
      expected: ['CKBFS file', '3X0125NMNVAP033202.LOGS'],
      href: `/mainnet/cell/0x${'b'.repeat(64)}-0`,
    },
    {
      label: 'exact token units beyond Number precision',
      display: {
        kind: 'token',
        standard: 'xudt',
        id: `0x${'8'.repeat(64)}`,
        name: 'Test USD',
        collectionId: null,
        collectionName: null,
        symbol: 'USDI',
        decimals: 6,
        amount: '9007199254740993123456',
        compositionTier: null,
      } satisfies TransactionCellDisplay,
      expected: ['xUDT', '9,007,199,254,740,993.123456', 'USDI'],
      href: `/mainnet/tokens/0x${'8'.repeat(64)}`,
    },
    {
      label: 'identity name and canonical detail link',
      display: {
        kind: 'identity',
        standard: 'dotbit',
        id: `0x${'9'.repeat(40)}`,
        name: 'alice.bit',
        collectionId: null,
        collectionName: null,
        symbol: null,
        decimals: null,
        amount: null,
        compositionTier: null,
      } satisfies TransactionCellDisplay,
      expected: ['.bit', 'alice.bit', '0x99999999...999999'],
      href: `/mainnet/identities/dotbit/0x${'9'.repeat(40)}`,
    },
  ])('renders $label from the per-cell API descriptor', async ({ display, expected, href }) => {
    const tx = createCommittedTransactionDetail();
    tx.inputs![0].display = display;
    tx.outputs![0].display = display;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);
    const input = await screen.findByTestId('tx-io-input-0');
    const output = screen.getByTestId('tx-io-output-0');
    for (const text of expected) {
      expect(input).toHaveTextContent(text);
      expect(output).toHaveTextContent(text);
    }
    expect(
      within(input)
        .getAllByRole('link')
        .some((link) => link.getAttribute('href') === href)
    ).toBe(true);
  });

  it('honors the graph deep link without requiring an extra raw-data click', async () => {
    mockSearchParams = new URLSearchParams('tab=graph');
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('mock-cell-graph', {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByText('Raw data').parentElement).toHaveAttribute('open');
  });

  it('shows not found message for 404 transaction errors', async () => {
    vi.mocked(api.getTransactionDetail).mockRejectedValueOnce(
      new ApiRequestError(404, 'not_found', 'transaction absent')
    );

    render(<TransactionDetailPage />);

    expect(await screen.findByText(`/transactions/${TX_HASH}/detail`)).toBeInTheDocument();
    expect(
      screen.getByText('This explorer could not open the transaction at this address.')
    ).toBeInTheDocument();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
  });

  it('does not infer HTTP 404 from a transport error message containing 404', async () => {
    vi.mocked(api.getTransactionDetail).mockRejectedValueOnce(
      new Error('upstream request 404 timed out')
    );
    render(<TransactionDetailPage />);
    expect(await screen.findByText('Failed to load transaction')).toBeInTheDocument();
    expect(
      screen.queryByText('This explorer could not open the transaction at this address.')
    ).not.toBeInTheDocument();
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
