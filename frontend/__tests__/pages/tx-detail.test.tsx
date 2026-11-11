import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, screen, waitFor, fireEvent, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { api, ApiRequestError, type TransactionCellDisplay } from '@/lib/api';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';
import { TYPE_ID_CODE_HASH } from '@/lib/tx-continuity';
import { CELL_COLORS } from '@/lib/design-tokens';
import { expectOnePageSeal } from '@/__tests__/utils/seals';
import { canvasLines, seamLines, seamOverlay } from '@/__tests__/utils/tx-lines';
import type { SealState } from '@/lib/seal/stones';

/** No seal on the page. */
function expectNoSeal() {
  expect(document.querySelectorAll('[data-seal]')).toHaveLength(0);
}

/** The header presses the transaction's one stone, 1 → 1 (the fixtures' counts), in `state`. */
function expectHeadStone(state: SealState) {
  expectOnePageSeal(
    document.querySelector('header')!,
    { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
    state
  );
}

const TX_HASH = '0x57a54eb7922190d5b0e0d7f5ad91dbbd91714a9bd852000994f99250ddc08e0f';
const LOCK_CODE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const TYPE_CODE_HASH = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';
/**
 * Script hashes as the API serves them, 32 bytes (seal kinds, C3: a cell's seal is drawn from its
 * lock and type hash, which must be real hashes): blake2b of the input lock (args 0x11 × 20), the
 * type script and the output lock (args 0x22 × 20) below.
 */
const INPUT_LOCK = '0x29e560749fc5c785e3c1cfc3e14c0e608ec87120f0c066facf023ee8bd569d84';
const TYPE_SCRIPT = '0xba0286acbad839ae8a95e182d7698efdf654135d483b92863c55c80d170868ad';
const OUTPUT_LOCK = '0xdf1b29cb0d9466e0ffbcdf05c4d598edc1fd8fc6b723fbbd8b8f5414ddca3ab0';
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
    fee: '64119',
    feeRate: '52130',
    txSize: 1128,
    isCellbase: false,
    cycles: 1000,
    cyclesStatus: 'complete',
    timestamp: '2026-02-20T16:46:12Z',
    confirmations: 4,
    inputsCapacity: '55500000000',
    outputsCapacity: '55499935881',
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
        capacity: '55499935881',
        commonKnowledgeSize: '94',
        status: 'live',
        isChange: false,
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
        scriptHash: INPUT_LOCK,
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
        scriptHash: TYPE_SCRIPT,
        kind: 'type',
        ran: true,
        inputIndices: [0],
        outputIndices: [0],
        witnessIndex: 0,
        script: { codeHash: TYPE_CODE_HASH, hashType: 'type', args: '0x' },
        cycles: null,
      },
      {
        scriptHash: OUTPUT_LOCK,
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
    headerDeps: [],
    cellbaseWitness: null,
    feePaidBy: [],
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
}

const GENESIS_DEP_GROUP_TX_HASH =
  '0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c';
const GENESIS_DEP_GROUP_SENDER =
  'ckb1qpcf7076zt6krnavly3883t6nrlduxy28ud9nv0c3rg387wvuzryjq9h89s7gmv7kyvd8hsar68npva00wlnzcqgh76tz';
const ZERO_LOCK_ADDRESS = 'ckb1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq4maw7d';

/**
 * The genesis dep-group transaction (review 45fd finding 5): block 0, index 1, not a cellbase, fee
 * 0, 234 CKB in and out — the summary fields as mainnet :8101 served them on 2026-11-10. The cells
 * are built to agree with them and with its activity: one 234 CKB input under a 20-byte-args lock
 * (61 bytes occupied), two Zero Lock dep-group outputs of 117 CKB each (a two-member dep group:
 * 8 + 33 + 76 bytes).
 */
function createGenesisDepGroupTransactionDetail(): Awaited<
  ReturnType<typeof api.getTransactionDetail>
> {
  const tx = createCommittedTransactionDetail();
  const zeroLock = { codeHash: `0x${'0'.repeat(64)}`, hashType: 'data', args: '0x' };
  const output = {
    capacity: '11700000000',
    commonKnowledgeSize: '117',
    status: 'live',
    isChange: false,
    display: null,
    lock: zeroLock,
    address: ZERO_LOCK_ADDRESS,
  };
  return {
    ...tx,
    hash: GENESIS_DEP_GROUP_TX_HASH,
    blockNumber: 0,
    index: 1,
    isCellbase: false,
    fee: '0',
    feeRate: '0',
    inputsCount: 1,
    outputsCount: 2,
    inputsCapacity: '23400000000',
    outputsCapacity: '23400000000',
    inputsCommonKnowledgeSize: '61',
    outputsCommonKnowledgeSize: '234',
    cycles: 0,
    cyclesStatus: 'complete',
    timestamp: '2019-11-15T21:09:50.812+00:00',
    inputs: [
      {
        ...tx.inputs![0],
        capacity: '23400000000',
        commonKnowledgeSize: '61',
        cellType: undefined,
        type: undefined,
        address: GENESIS_DEP_GROUP_SENDER,
      },
    ],
    outputs: [output, { ...output }],
    scriptGroups: [
      { ...tx.scriptGroups![0], ran: false, cycles: null, witnessIndex: null },
      {
        scriptHash: '0x77c93b0632b5b6c3ef922c5b7cea208fb0a7c427a13d50e13d3fefad17e0c590',
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: [0, 1],
        witnessIndex: null,
        script: zeroLock,
        cycles: null,
      },
    ],
    witnessDetails: [],
    witnesses: [],
  } as Awaited<ReturnType<typeof api.getTransactionDetail>>;
}

/** Its activity, verbatim from mainnet :8101 (`/transactions/0x71a7…e46c/activity`). */
const GENESIS_DEP_GROUP_ACTIVITY = JSON.parse(
  '{"txHash":"0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c","blockNumber":0,"txIndex":1,"timestamp":"1573852190812","isCellbase":false,"protocolActions":[],"typeCalls":[],"lockCalls":[{"lockCodeHash":"0x0000000000000000000000000000000000000000000000000000000000000000","lockHashType":"data","lockArgs":"0x","scriptHash":"0x77c93b0632b5b6c3ef922c5b7cea208fb0a7c427a13d50e13d3fefad17e0c590","scriptName":"Zero Lock","decoded":null,"decodeError":null}],"participants":[{"address":"ckb1qpcf7076zt6krnavly3883t6nrlduxy28ud9nv0c3rg387wvuzryjq9h89s7gmv7kyvd8hsar68npva00wlnzcqgh76tz","lockHash":"0x64b2aa1eadd2ad55a37296d7624c1c822156eee760506a9b67db51badebfdea2","lockHashPrefix":null,"lockKind":null,"roles":[],"ckbDelta":"-23400000000","usedDelta":"-6100000000","itemDeltas":[],"tags":128},{"address":"ckb1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq4maw7d","lockHash":"0x77c93b0632b5b6c3ef922c5b7cea208fb0a7c427a13d50e13d3fefad17e0c590","lockHashPrefix":null,"lockKind":"Zero Lock","roles":[],"ckbDelta":"23400000000","usedDelta":"23400000000","itemDeltas":[],"tags":0}]}'
) as Awaited<ReturnType<typeof api.getTransactionActivity>>;

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

  // Visual finish V8: the page loads as the shared detail skeleton, not a bare block.
  it('shows the detail skeleton while the transaction loads, then the page', async () => {
    let resolve!: (value: ReturnType<typeof createCommittedTransactionDetail>) => void;
    vi.mocked(api.getTransactionDetail).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      })
    );
    render(<TransactionDetailPage />);
    const skeleton = screen.getByRole('status', { name: 'Loading …' });
    expect(skeleton.children).toHaveLength(3);
    expect(document.querySelector('.h-72')).toBeNull();
    // No comment while loading: none of its figures is known yet (T4); no seal either (C2).
    expect(document.querySelector('.comment')).toBeNull();
    expectNoSeal();
    resolve(createCommittedTransactionDetail());
    expect(await screen.findByRole('heading', { level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Loading …' })).toBeNull();
    expectHeadStone('pressed');
    // Then the comment, from the fixture's figures: 94 occupied bytes in and out (T4).
    expect(
      Array.from(document.querySelectorAll('.comment [data-comment-line]')).map(
        (l) => l.textContent
      )
    ).toEqual(['1 cell became 1.', '0.00064119 ckb to the miner.', 'common knowledge unchanged.']);
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

  // Rubbing × Song print, phase 5 (T1): the cells heading is a command and its args, `cells 1 in →
  // 1 out` (plain counts, no nouns to agree); the confirmations still agree with their count.
  it('runs the cells command on its counts and agrees nouns with counts in the confirmations (TX-34)', async () => {
    render(<TransactionDetailPage />);
    const heading = await screen.findByRole('heading', { name: 'Cells 1 in → 1 out' });
    expect(heading).toHaveClass('prompt', 'text-section');
    const [command, args] = Array.from(heading.children) as HTMLElement[];
    expect(command).toHaveTextContent(/^Cells$/);
    expect(command).toHaveClass('lowercase');
    expect(args).toHaveTextContent(/^1 in → 1 out$/);
    expect(args).toHaveClass('text-text-dim');
    // Consistency plan C1: the tab names the page.
    expect(document.title).toMatch(/^0x[0-9a-f]{4}…[0-9a-f]{4} · transaction · CKBadger$/);
    expect(screen.queryByText(/\b1 cells\b/)).toBeNull();
    expect(screen.getByText(/\d confirmations?$/)).toBeInTheDocument();
  });

  it('shows commit time as UTC+8 with a relative age, not a raw ISO Z string (TX-10)', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-02-20T17:46:12Z'));
    render(<TransactionDetailPage />);
    const time = await screen.findByRole('time');
    expect(time).toHaveTextContent('2026-02-21 00:46:12 UTC+8');
    expect(time).toHaveTextContent(/1h ago$/);
    expect(time.textContent).not.toMatch(/Z/);
  });

  // Review 45fd finding 3 (ruling D4): the node's clock 2 s ahead of this browser's is two
  // clocks, not a broken page.
  it('renders a pool transaction whose pendingSince is 2 s after this device’s now', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date('2026-03-12T11:59:58Z'));
    vi.mocked(api.getTransactionDetail).mockResolvedValue(createPendingTransactionDetail());
    render(<TransactionDetailPage />);
    expect(await screen.findByRole('heading', { level: 1 })).toHaveTextContent(
      'Waiting in the pool'
    );
    const row = screen.getByText('Pending since').parentElement!;
    expect(row).toHaveTextContent('2026-03-12 20:00:00 UTC+8');
    expect(row).toHaveTextContent(/0s ago$/);
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

  // Review 45fd finding 5 (ruling D6): issuance happens in a cellbase, not in "a transaction of
  // block 0"; the genesis dep-group transaction conserves and pays no fee.
  it('reads the genesis dep-group transaction as a conserving transfer with no fee', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue(createGenesisDepGroupTransactionDetail());
    vi.mocked(api.getTransactionActivity).mockResolvedValue(GENESIS_DEP_GROUP_ACTIVITY);
    render(<TransactionDetailPage />);
    expect(await screen.findByRole('heading', { name: 'What moved' })).toBeInTheDocument();
    expect(screen.queryByTestId('activity-read-error')).toBeNull();
    const moved = screen.getByTestId('tx-what-moved');
    const flows = within(moved).getAllByRole('group');
    expect(flows.map((f) => f.getAttribute('data-flow'))).toEqual(['ckb']);
    const from = flows[0].querySelector('[data-flow-side="from"]')!;
    const to = flows[0].querySelector('[data-flow-side="to"]')!;
    expect([...from.querySelectorAll('[data-flow-end]')].map((e) => e.textContent)).toEqual([
      expect.stringMatching(/^ckb1qpcf\.\.\.gh76tz.*234\.00000000 CKB$/),
    ]);
    expect([...to.querySelectorAll('[data-flow-end]')].map((e) => e.textContent)).toEqual([
      expect.stringMatching(/4maw7d.*234\.00000000 CKB$/),
    ]);
    // no fee and nothing issued: no miner end, no genesis end
    expect(moved.querySelector('[data-flow-end="miner"]')).toBeNull();
    expect(moved).not.toHaveTextContent(/genesis|issued/);
    const net = screen.getByTestId('tx-balance-net');
    expect(net).toHaveTextContent(/^Net0 CKB\+173no fee: nothing paid to the miner$/);
    expect(net).toHaveTextContent('no fee: nothing paid to the miner');
    expect(net).not.toHaveTextContent(/fee, paid|genesis|issued/);
    expect(screen.queryByText(/issued in the genesis block/)).toBeNull();
    // Its comment: not a cellbase, so a fee line, the rail's zero (T4).
    expect(
      Array.from(document.querySelectorAll('.comment [data-comment-line]')).map(
        (l) => l.textContent
      )
    ).toEqual([
      '1 cell became 2.',
      '0 ckb to the miner.',
      // 61 occupied bytes in, 117 + 117 out
      'common knowledge grew by 173 bytes.',
    ]);
  });

  // Review 45fd finding 7 (ruling D7): an activity value the page cannot read fails in its own
  // section, not through the page-level boundary (here the shared ErrorBoundary, standing in for
  // the route's errorElement).
  it('keeps the page when a participant ckbDelta is not a number', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      const [first, ...rest] = feedSamples.objectTransfer.participants;
      vi.mocked(api.getTransactionActivity).mockResolvedValue({
        ...feedSamples.objectTransfer,
        txHash: TX_HASH,
        typeCalls: [],
        participants: [first, ...rest].map((p, i) => ({
          ...p,
          itemDeltas: [],
          ...(i === 0 ? { ckbDelta: 'abc' } : {}),
        })),
      });
      render(
        <ErrorBoundary fallback={<p>page-level boundary</p>}>
          <TransactionDetailPage />
        </ErrorBoundary>
      );
      // Each section that renders the activity's values fails in place; the page stays.
      const balance = (await screen.findByRole('heading', { name: 'Balance changes' })).closest(
        'section'
      )!;
      expect(screen.queryByText('page-level boundary')).toBeNull();
      const header = screen.getByRole('heading', { level: 1 }).closest('header')!;
      expect(within(header).getByText(TX_HASH)).toBeInTheDocument();
      // the summary slot, under the h1
      expect(within(header).getByTestId('activity-read-error')).toHaveTextContent(
        `Cannot read tx ${TX_HASH}: Cannot convert abc to a BigInt`
      );
      expect(within(header).queryByTestId('tx-summary')).toBeNull();
      const moved = screen.getByRole('heading', { name: 'What moved' }).closest('section')!;
      expect(within(moved).getByTestId('activity-read-error')).toHaveTextContent(
        `${TX_HASH}: participant ckbDelta "abc"`
      );
      expect(within(moved).queryByTestId('tx-what-moved')).toBeNull();
      // Balance changes keeps its heading; its body is one error, the table is gone
      expect(within(balance).getAllByTestId('activity-read-error')).toHaveLength(1);
      expect(within(balance).getByTestId('activity-read-error')).toHaveTextContent(
        `Cannot read tx ${TX_HASH}: Cannot convert abc to a BigInt`
      );
      expect(within(balance).queryByRole('table')).toBeNull();
      // The sections that do not read the activity render as always.
      const facts = screen.getByRole('complementary', { name: 'Transaction facts' });
      expect(within(facts).getByText('Fee')).toBeInTheDocument();
      expect(screen.getByTestId('tx-io-input-0')).toBeInTheDocument();
      expect(screen.getByTestId('tx-io-output-0')).toBeInTheDocument();
      expect(screen.getByTestId('tx-capacity-equation')).toBeInTheDocument();
      expect(screen.getAllByTestId('activity-read-error')).toHaveLength(3);
    } finally {
      quiet.mockRestore();
    }
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
    expect(screen.queryByText(/confirmations?$/i)).not.toBeInTheDocument();
    expect(api.getTransactionDetail).toHaveBeenCalledTimes(1);
    // C2: in a block the store has not indexed yet, its stone is dashed; no cells, no seam seal.
    expectHeadStone('unpressed');
    expect(document.querySelector('[data-seam-seal]')).toBeNull();

    // Still provisional: the page keeps asking until the store has it.
    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(2));
    expect(screen.getAllByRole('link', { name: '#123' }).length).toBeGreaterThan(0);

    // Indexed: `poolStatus` is gone, the committed view renders, polling stops.
    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(3));
    expect(await screen.findByText(/4\s+confirmations/)).toBeInTheDocument();
    expect(screen.queryByText('Confirming')).not.toBeInTheDocument();
    // Indexed: the same stone, pressed, and pressed again across its cells (C11: its two halves,
    // one on each sheet).
    expectHeadStone('pressed');
    expect(document.querySelectorAll('[data-seam-seal] [data-seal-state="pressed"]')).toHaveLength(
      2
    );

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
    expect(document.querySelector('.comment')).toBeNull();
    // C2: no transaction yet, so no stone.
    expectNoSeal();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
    expect(api.getTransactionDetail).toHaveBeenCalledTimes(1);

    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(2));

    expect(await screen.findByText('Pending since')).toBeInTheDocument();
    expect(screen.queryByTestId('tx-inputs-not-yet-resolvable')).not.toBeInTheDocument();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
    // Then the pool transaction: its stone, dashed.
    expectHeadStone('unpressed');
  });

  it('keeps showing a pending transaction when a later poll answers 503 for its inputs', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.getTransactionDetail)
      .mockResolvedValueOnce(createPendingTransactionDetail())
      .mockRejectedValueOnce(inputsNotYetResolvable())
      .mockResolvedValue(createPendingTransactionDetail());

    render(<TransactionDetailPage />);
    expect(await screen.findByText('Pending since')).toBeInTheDocument();

    await act(() => vi.advanceTimersByTimeAsync(3000));
    await waitFor(() => expect(api.getTransactionDetail).toHaveBeenCalledTimes(2));
    expect(screen.getByText('Pending since')).toBeInTheDocument();
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
    expect(document.querySelector('.comment')).toBeNull();
    // C2: nothing was read, so no stone.
    expectNoSeal();
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

  // Lane L (L1): every cell of the transaction links to its cell page by its out point.
  it('links every input and output tile to its cell page by its out point (L1)', async () => {
    render(<TransactionDetailPage />);
    const input = await screen.findByTestId('tx-io-input-0');
    const inputLink = within(input).getByRole('link', { name: '0xbbbb...bbbb:0' });
    expect(inputLink).toHaveAttribute('href', `/mainnet/cell/0x${'b'.repeat(64)}-0`);
    expect(inputLink).toHaveAttribute('title', `0x${'b'.repeat(64)}:0`);
    const output = screen.getByTestId('tx-io-output-0');
    const outputLink = within(output).getByRole('link', { name: '0x57a5...8e0f:0' });
    expect(outputLink).toHaveAttribute('href', `/mainnet/cell/${TX_HASH}-0`);
    expect(outputLink).toHaveAttribute('title', `${TX_HASH}:0`);
    // the script matrix names the same cells, and links them too
    const matrix = screen.getByRole('table', { name: 'Script groups' });
    expect(within(matrix).getByRole('link', { name: 'in #0' })).toHaveAttribute(
      'href',
      `/mainnet/cell/0x${'b'.repeat(64)}-0`
    );
    expect(within(matrix).getByRole('link', { name: 'out #0' })).toHaveAttribute(
      'href',
      `/mainnet/cell/${TX_HASH}-0`
    );
  });

  it('lets the out point of a lit tile open its cell page (review 2026-11-03)', async () => {
    // Hovering a tile lights it; that used to arm the tile's clear-focus click, whose URL replace
    // landed right after the link's navigation and cancelled it. A lit tile with nothing
    // selected has no clearing click, and a click inside the out point link is the link's alone.
    render(<TransactionDetailPage />);
    const output = await screen.findByTestId('tx-io-output-0');
    // outside a router the anchor would ask jsdom to navigate, which it cannot
    output.addEventListener('click', (event) => event.preventDefault());
    fireEvent.mouseEnter(output);
    expect(output).toHaveAttribute('data-highlighted', 'true');
    const replaces = mockReplace.mock.calls.length;
    fireEvent.click(within(output).getByRole('link', { name: '0x57a5...8e0f:0' }));
    fireEvent.click(output);
    expect(mockReplace.mock.calls.length).toBe(replaces);
    // with a witness selected, the tile body still clears the selection; the link still does not
    fireEvent.click(screen.getByTestId('tx-witness-item-0'));
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).toContain('witness=0');
    const selected = mockReplace.mock.calls.length;
    fireEvent.click(within(output).getByRole('link', { name: '0x57a5...8e0f:0' }));
    expect(mockReplace.mock.calls.length).toBe(selected);
    fireEvent.click(output);
    expect(mockReplace.mock.calls.length).toBe(selected + 1);
    expect(String(mockReplace.mock.calls.at(-1)?.[0])).not.toContain('witness=');
  });

  it('links a pool transaction’s inputs to the cells it spends; its outputs are not cells yet (L1)', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue(createPendingTransactionDetail());
    render(<TransactionDetailPage />);
    const matrix = await screen.findByRole('table', { name: 'Script groups' });
    const input = within(matrix).getByRole('link', { name: 'in #0' });
    expect(input).toHaveAttribute('href', `/mainnet/cell/0x${'b'.repeat(64)}-0`);
    expect(input).toHaveAttribute('title', `0x${'b'.repeat(64)}:0`);
    expect(within(matrix).queryByRole('link', { name: 'out #0' })).toBeNull();
    expect(within(matrix).getByText('out #0')).toHaveAttribute(
      'title',
      'not a cell until the transaction is committed'
    );
  });

  it('shows canonical cell structure first and loads the raw cell graph only when opened', async () => {
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('tx-io-input-0')).toBeInTheDocument();
    // C11 round 3: a line across the seam means only that a consumed cell continues as a created
    // one; this transaction's cells do not (its row-aligned connectors read as false 1:1 pairs),
    // so the seam between the two sheets is bare
    expect(document.querySelector('[data-seam]')).toBeInTheDocument();
    expect(seamOverlay(document)).toBeNull();
    expect(canvasLines(document)).toHaveLength(0);
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

  // Review 45fd finding 7: readable() now always returns an element, so the witness note decides
  // inside it whether there is a note at all; a witness with nothing to name gets no empty line.
  it('draws no empty note under a witness that has nothing to name', async () => {
    const tx = createCommittedTransactionDetail();
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...tx,
      witnessDetails: [tx.witnessDetails![0], { index: 1, size: 0, kind: 'empty', fields: [] }],
      witnesses: [tx.witnesses[0], '0x'],
    });
    render(<TransactionDetailPage />);
    const empty = await screen.findByTestId('tx-witness-item-1');
    // the kind/size line only: no byte bar, no note
    expect(empty.children).toHaveLength(1);
    expect(empty).toHaveTextContent(/^#1 · empty0 B$/);
    // a witness with fields keeps its bar and its note
    const args = screen.getByTestId('tx-witness-item-0');
    expect(args.children).toHaveLength(3);
    expect(args.lastElementChild).toHaveTextContent('header 16 B · lock length 4 B · lock 2 B');
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
    // Lane L: "output #1" names the name's cell in this transaction, so it links to it.
    const cellLink = within(section).getByRole('link', { name: 'output #1' });
    expect(cellLink).toHaveAttribute('href', `/mainnet/cell/${TX_HASH}-1`);
    expect(cellLink).toHaveAttribute('title', `${TX_HASH}:1`);
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
    expect(output).toHaveTextContent('occupied 94');
    expect(output).not.toHaveTextContent('occupied 94.00000000');
    expect(output).toHaveTextContent('live');
    expect(screen.getByText(/^occupied 94 → 94/)).toHaveTextContent(
      'occupied 94 → 94 · common knowledge 0'
    );
    expect(
      document.querySelector(`[data-group-row="lock:attached:${OUTPUT_LOCK}"]`)
    ).toHaveTextContent('attached, runs when spent');
    expect(screen.getByTestId('tx-groups-ran')).toHaveTextContent('2 ran, 4,446,651 cycles');
    expect(await screen.findByRole('heading', { name: 'Balance changes' })).toBeInTheDocument();
    expect(api.getTransactionActivity).toHaveBeenCalledWith(TX_HASH);
  });

  it('links expanded dep groups and highlights their exact script members', async () => {
    vi.mocked(api.getTransactionCellDeps).mockResolvedValue([
      {
        outPointTxHash: '0xdep',
        outPointIndex: 2,
        depType: 'dep_group',
        scriptHashes: [INPUT_LOCK],
        outputLockScriptHashes: [],
        resolvedCells: [
          {
            txHash: '0xmember',
            index: 3,
            dataHash: '0xdata',
            typeHash: null,
            scriptHashes: [INPUT_LOCK],
            outputLockScriptHashes: [],
            codeSize: 52048,
            scriptNames: ['Default Multisig'],
          },
        ],
      },
    ]);
    render(<TransactionDetailPage />);
    const member = await screen.findByRole('link', { name: '↳ 0xmember:3' }, { timeout: 5000 });
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
    expect(document.querySelector(`[data-group-row="lock:run:${INPUT_LOCK}"]`)).toHaveAttribute(
      'data-lit',
      'true'
    );
    expect(
      document.querySelector(`[data-group-row="lock:attached:${INPUT_LOCK}"]`)
    ).toHaveAttribute('data-lit', 'false');
  });

  it('links one-to-one CKBFS file continuity to its cells and exact type group', async () => {
    const tx = createCommittedTransactionDetail();
    tx.inputs![0].cellType = 'ckbfs';
    tx.outputs![0].cellType = 'ckbfs';
    const display: TransactionCellDisplay = {
      kind: 'file',
      standard: 'ckbfs',
      id: TYPE_SCRIPT,
      name: '3X0125NMNVAP033202.LOGS',
      collectionId: null,
      collectionName: null,
      symbol: null,
      decimals: null,
      amount: null,
      noAmountReason: null,
      compositionTier: null,
    };
    tx.inputs![0].display = display;
    tx.outputs![0].display = display;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);

    const continuity = await screen.findByRole('button', { name: 'same file cell continues' });
    fireEvent.mouseEnter(continuity);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'true');
    expect(document.querySelector('path[data-continuity-kind="file"]')).toHaveAttribute(
      'stroke',
      'var(--c-text)'
    );
    // C11 round 3: the link is the one line across the seam
    expect(screen.getByLabelText('Cells that continue from consumed to created')).toBe(
      seamOverlay(document)
    );
    expect(seamLines(document)).toHaveLength(1);
    expect(canvasLines(document)).toEqual(seamLines(document));
    expect(document.querySelector(`[data-group-row="type:run:${TYPE_SCRIPT}"]`)).toHaveAttribute(
      'data-lit',
      'true'
    );
    fireEvent.mouseLeave(continuity);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'false');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'false');
  });

  it('links a one-in one-out Type ID group as the same type id, through the shared continuity path', async () => {
    const tx = createCommittedTransactionDetail();
    const typeId = {
      codeHash: TYPE_ID_CODE_HASH,
      hashType: 'type' as const,
      args: `0x${'7'.repeat(64)}`,
    };
    tx.inputs![0].type = typeId;
    tx.outputs![0].type = typeId;
    tx.scriptGroups![1].script = typeId;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);

    const continuity = await screen.findByRole('button', { name: 'same type id continues' });
    // The legend stays the cells row's actions, beside the command and outside its name (T1).
    const cells = screen.getByRole('heading', { name: 'Cells 1 in → 1 out' });
    expect(cells).not.toContainElement(continuity);
    expect(cells.parentElement).toContainElement(continuity);
    expect(document.querySelector('path[data-continuity-kind="type-id"]')).toHaveAttribute(
      'stroke',
      CELL_COLORS.type
    );
    // C11 round 3: the link is the one line across the seam
    expect(canvasLines(document)).toEqual(seamLines(document));
    expect(seamLines(document)).toHaveLength(1);
    expect(continuity.querySelector('path')).toHaveAttribute('stroke', CELL_COLORS.type);
    fireEvent.mouseEnter(continuity);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'true');
  });

  it.each(['different identity', 'multiple outputs'])(
    'does not infer file continuity with %s',
    async (reason) => {
      const tx = createCommittedTransactionDetail();
      const display: TransactionCellDisplay = {
        kind: 'file',
        standard: 'ckbfs',
        id: TYPE_SCRIPT,
        name: 'identical-name.txt',
        collectionId: null,
        collectionName: null,
        symbol: null,
        decimals: null,
        amount: null,
        noAmountReason: null,
        compositionTier: null,
      };
      tx.inputs![0].display = display;
      tx.outputs![0].display =
        reason === 'different identity' ? { ...display, id: '0xanother' } : display;
      if (reason === 'multiple outputs') {
        tx.outputs!.push({ ...tx.outputs![0] });
        tx.scriptGroups![1].outputIndices = [0, 1];
        // the pushed output's lock is the first output's: the same group lists it
        tx.scriptGroups![2].outputIndices = [0, 1];
      }
      vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
      render(<TransactionDetailPage />);
      await screen.findByTestId('tx-io-input-0');
      expect(screen.queryByRole('list', { name: 'Cell continuity' })).not.toBeInTheDocument();
      // The page draws its legend itself (continuityList={false}), so check its buttons too.
      expect(screen.queryByRole('button', { name: /continues$/ })).not.toBeInTheDocument();
      expect(document.querySelector('path[data-continuity-kind]')).toBeNull();
      // C11 round 3: and so no line at all between the sheets
      expect(seamOverlay(document)).toBeNull();
      expect(canvasLines(document)).toHaveLength(0);
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
        noAmountReason: null,
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
        noAmountReason: null,
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
        noAmountReason: null,
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
        noAmountReason: null,
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
    expect(screen.getByText('no transaction at this address.')).toBeInTheDocument();
    expect(screen.queryByText('Failed to load transaction')).not.toBeInTheDocument();
  });

  it('does not infer HTTP 404 from a transport error message containing 404', async () => {
    vi.mocked(api.getTransactionDetail).mockRejectedValueOnce(
      new Error('upstream request 404 timed out')
    );
    render(<TransactionDetailPage />);
    expect(await screen.findByText('Failed to load transaction')).toBeInTheDocument();
    expect(screen.queryByText('no transaction at this address.')).not.toBeInTheDocument();
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
