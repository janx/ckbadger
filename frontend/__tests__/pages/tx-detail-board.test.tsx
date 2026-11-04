/**
 * The Transaction board (Main.dc.html v5) and the D4 variants, page level (F2-13 / report A):
 * headline + summary + copy, the facts rail, Balance changes with its Net row, hover linking by
 * group identity, resolved cell deps, the proposal ruler, the capacity equation, header deps,
 * the cellbase and DAO-withdrawal variants and the pending wording.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { expectRestingLink } from '@/__tests__/utils/resting-link';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { api, type GlobalActivity, type TransactionDetail } from '@/lib/api';

const TX_HASH = '0x57a54eb7922190d5b0e0d7f5ad91dbbd91714a9bd85200994f99250ddc08e0f';
const LOCK_CODE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const TYPE_CODE_HASH = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';
let mockSearchParams = new URLSearchParams();
/** Other lanes' suites share this machine: give first renders room. */
const SLOW = { timeout: 5000 };

vi.mock('@/lib/api', async () => {
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
      getBlock: vi.fn(),
    },
    isWarmupPendingError: vi.fn(() => false),
    isNetworkInitializingError: vi.fn(() => false),
  };
});
vi.mock('@/hooks/useCyclesCalculation', () => ({
  useCyclesCalculation: vi.fn(() => ({
    cycles: 1923713,
    hasCycles: true,
    isCalculating: false,
    hasFailed: false,
    isPaused: false,
  })),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));
vi.mock('@/components/cell-graph', () => ({
  default: () => <div data-testid="mock-cell-graph" />,
}));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ hash: TX_HASH }),
  usePathname: () => `/tx/${TX_HASH}`,
  useSearchParams: () => mockSearchParams,
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
}));

const lockScript = (args: string) => ({ codeHash: LOCK_CODE_HASH, hashType: 'type', args });
const typeScript = { codeHash: TYPE_CODE_HASH, hashType: 'type', args: '0x' };

/** Three parties (feedSamples.objectTransfer): 555 in, a 0.00064119 fee. */
function committed(): TransactionDetail {
  return {
    hash: TX_HASH,
    status: 'committed',
    blockNumber: 101,
    blockHash: `0x${'aa'.repeat(32)}`,
    index: 1,
    inputsCount: 1,
    outputsCount: 1,
    fee: '64119',
    feeRate: '33222',
    txSize: 1930,
    isCellbase: false,
    cycles: 1923713,
    cyclesStatus: 'complete',
    timestamp: '2026-10-22T14:43:55Z',
    confirmations: 27369,
    inputsCapacity: '55500000000',
    outputsCapacity: '55499935881',
    inputsCommonKnowledgeSize: '563',
    outputsCommonKnowledgeSize: '589',
    inputs: [
      {
        previousOutput: { txHash: `0x${'bb'.repeat(32)}`, index: 0 },
        capacity: '55500000000',
        commonKnowledgeSize: '94',
        cellType: 'typed',
        display: null,
        lock: lockScript(`0x${'11'.repeat(20)}`),
        type: typeScript,
        address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq',
        lockArgsDecoded: { lock: 'secp256k1_blake160', pubkeyHash: `0x${'11'.repeat(20)}` },
      },
    ],
    outputs: [
      {
        capacity: '55499935881',
        commonKnowledgeSize: '94',
        status: 'live',
        isChange: true,
        lockArgsDecoded: null,
        display: null,
        lock: lockScript(`0x${'22'.repeat(20)}`),
        type: typeScript,
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
        script: lockScript(`0x${'11'.repeat(20)}`),
        cycles: '1694860',
      },
      {
        scriptHash: '0xtypescript',
        kind: 'type',
        ran: true,
        inputIndices: [0],
        outputIndices: [0],
        witnessIndex: 1,
        script: typeScript,
        cycles: '228853',
      },
      {
        scriptHash: '0xoutputlock',
        kind: 'lock',
        ran: false,
        inputIndices: [],
        outputIndices: [0],
        witnessIndex: null,
        script: lockScript(`0x${'22'.repeat(20)}`),
        cycles: null,
      },
    ],
    witnessDetails: [
      {
        index: 0,
        size: 22,
        kind: 'witness_args',
        fields: [
          { name: 'header', start: 0, end: 16, hex: '0x' },
          { name: 'lock length', start: 16, end: 20, hex: '0x' },
          { name: 'lock', start: 20, end: 22, hex: '0x1122' },
        ],
      },
      {
        index: 1,
        size: 7,
        kind: 'cobuild',
        fields: [{ name: 'message', start: 0, end: 7, hex: '0x' }],
        actions: [
          {
            scriptInfoHash: `0x${'44'.repeat(32)}`,
            scriptHash: '0xtypescript',
            data: '0x01',
            schema: 'spore',
            name: 'TransferSpore',
          },
        ],
      },
    ],
    witnesses: ['0x16000000100000001600000016000000020000001122', '0x64617301020304'],
    witnessesAvailable: true,
    headerDeps: [],
    cellbaseWitness: null,
    feePaidBy: [
      {
        lockHash: feedSamples.objectTransfer.participants[0].lockHash!,
        address: feedSamples.objectTransfer.participants[0].address,
        outflow: '52800064119',
      },
    ],
  } as TransactionDetail;
}

/** The object transfer with the R12 object context on the wire (contracts.md R12). */
function objectActivity(): GlobalActivity {
  const withContext = (delta: number) => ({
    kind: 'object' as const,
    objectId: `0x51c75a${'00'.repeat(26)}96f2`,
    delta,
    collection: { id: `0x${'cc'.repeat(32)}`, name: 'Nervape 3x' },
    class: 'object' as const,
    composition: 'btc_ckb' as const,
  });
  const [from, to, third] = feedSamples.objectTransfer.participants;
  return {
    ...feedSamples.objectTransfer,
    txHash: TX_HASH,
    participants: [
      { ...from, itemDeltas: [withContext(-1)] },
      { ...to, itemDeltas: [withContext(1)] },
      third,
    ],
  };
}

const lookup = {
  [LOCK_CODE_HASH]: { codeHash: LOCK_CODE_HASH, name: 'Default Lock' },
  [TYPE_CODE_HASH]: { codeHash: TYPE_CODE_HASH, name: 'Spore' },
} as unknown as Awaited<ReturnType<typeof api.lookupScripts>>;

beforeEach(() => {
  vi.clearAllMocks();
  mockSearchParams = new URLSearchParams();
  vi.mocked(api.getTransactionDetail).mockResolvedValue(committed());
  vi.mocked(api.lookupScripts).mockResolvedValue(lookup);
  vi.mocked(api.getTransactionActivity).mockResolvedValue(objectActivity());
  vi.mocked(api.getTransactionGraph).mockResolvedValue({ nodes: [], links: [] });
  vi.mocked(api.getTransactionCellDeps).mockResolvedValue([
    {
      outPointTxHash: `0x71a7ba8f${'00'.repeat(26)}72e46c`,
      outPointIndex: 0,
      depType: 'dep_group',
      scriptHashes: ['0xinputlock'],
      outputLockScriptHashes: [],
      resolvedCells: [
        {
          txHash: '0xmember',
          index: 0,
          dataHash: '0xdata',
          typeHash: null,
          scriptHashes: ['0xinputlock'],
          outputLockScriptHashes: [],
          codeSize: 52048,
          scriptNames: ['Default Lock'],
        },
        {
          txHash: '0xother',
          index: 1,
          dataHash: '0xd2',
          typeHash: null,
          scriptHashes: [],
          outputLockScriptHashes: [],
          codeSize: 1048576,
          scriptNames: [],
        },
      ],
    },
    {
      outPointTxHash: `0x96b198fb${'00'.repeat(26)}b69824`,
      outPointIndex: 0,
      depType: 'code',
      scriptHashes: ['0xtypescript'],
      outputLockScriptHashes: [],
      resolvedCells: [
        {
          txHash: `0x96b198fb${'00'.repeat(26)}b69824`,
          index: 0,
          dataHash: '0xd3',
          typeHash: null,
          scriptHashes: ['0xtypescript'],
          outputLockScriptHashes: [],
          codeSize: 76752,
          scriptNames: ['Spore'],
        },
      ],
    },
    {
      outPointTxHash: `0xd00a0222${'00'.repeat(26)}2c47f7`,
      outPointIndex: 0,
      depType: 'code',
      scriptHashes: [],
      outputLockScriptHashes: ['0xoutputlock'],
      resolvedCells: [
        {
          txHash: `0xd00a0222${'00'.repeat(26)}2c47f7`,
          index: 0,
          dataHash: '0xd4',
          typeHash: '0xtt',
          scriptHashes: [],
          outputLockScriptHashes: ['0xoutputlock'],
          codeSize: 122552,
          scriptNames: [],
        },
      ],
    },
  ]);
  vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
    hash: TX_HASH,
    phase: 'committed',
    proposalId: '0x01',
    proposedIn: { blockNumber: 99, blockHash: `0x${'11'.repeat(32)}`, timestamp: '' },
    committedIn: { blockNumber: 101, blockHash: `0x${'22'.repeat(32)}`, timestamp: '' },
    commitmentDistance: 2,
    commitmentWindow: { close: 2, far: 10 },
    isCellbase: false,
    confirmations: 27369,
  });
});

describe('Transaction board, tier 1', () => {
  it('heads the page with the action, a plain summary and a copy button (TX-11)', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<TransactionDetailPage />);
    const h1 = await screen.findByRole('heading', { level: 1, name: 'Object transfer' }, SLOW);
    expect(h1.previousElementSibling).toHaveTextContent('Transaction');
    expect(h1.querySelector('[data-class]')).toBeNull();
    const summary = await screen.findByTestId('tx-summary', {}, SLOW);
    expect(summary).toHaveTextContent(
      'Nervape 3x 0x51c7...96f2 moved from ckb1qzda...cjk9lz to ckb1qzda...shp3hr. The sender paid a 0.00064119 CKB fee.'
    );
    expect(summary.querySelector('[data-class="objects"]')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Copy transaction hash' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(TX_HASH));
  });

  it('keeps the status, time, block and fee in a facts rail (TX-18)', async () => {
    render(<TransactionDetailPage />);
    const rail = await screen.findByRole('complementary', { name: 'Transaction facts' }, SLOW);
    expect(rail).toHaveTextContent('Committed');
    expect(rail).toHaveTextContent('27,369 confirmations');
    expect(rail).toHaveTextContent('2026-10-22 22:43:55 UTC+8');
    expect(within(rail).getByRole('link', { name: '#101' })).toBeInTheDocument();
    expect(rail).toHaveTextContent('0.00064119 CKB');
    expect(rail).toHaveTextContent('paid by ckb1qzda...cjk9lz');
  });

  it('reads status, time, block and fee under the hash, before the balances and the cells', async () => {
    // Responsive review A#17: on a phone the facts rail came after the six cell cards.
    render(<TransactionDetailPage />);
    const rail = await screen.findByRole('complementary', { name: 'Transaction facts' }, SLOW);
    const table = await screen.findByRole(
      'table',
      { name: 'Balance changes by participant' },
      SLOW
    );
    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    const hash = screen.getByRole('button', { name: 'Copy transaction hash' });
    const cells = screen.getByRole('heading', { name: /consumed → .* created/ });
    expect(before(hash, rail)).toBe(true);
    expect(before(rail, table)).toBe(true);
    expect(before(rail, cells)).toBe(true);
    // From lg up the rail is still the right-hand column beside the tier-1 story.
    expect(rail).toHaveClass('lg:col-start-2', 'lg:row-start-1');
  });

  it('names the collection and composition of moved objects, occupied in whole bytes, and a Net row (TX-12, TX-19)', async () => {
    render(<TransactionDetailPage />);
    const table = await screen.findByRole(
      'table',
      { name: 'Balance changes by participant' },
      SLOW
    );
    await waitFor(
      () => expect(table).toHaveTextContent('Nervape 3x 0x51c75a...96f2 departed'),
      SLOW
    );
    expect(table.querySelectorAll('[data-composition="btc_ckb"]')).toHaveLength(2);
    expect(table).toHaveTextContent('−502');
    expect(table).not.toHaveTextContent('502.00000000');
    expect(screen.getByTestId('tx-balance-net')).toHaveTextContent(
      'Net−0.00064119 CKB+26the fee, paid to the miner'
    );
  });
});

describe('Transaction board, tier 2', () => {
  it('lights a witness, its groups and their dep, and dims everything unrelated (TX-14)', async () => {
    render(<TransactionDetailPage />);
    const w0 = await screen.findByTestId('tx-witness-item-0', {}, SLOW);
    await screen.findAllByText(/supplies code for/, {}, SLOW);
    fireEvent.mouseEnter(w0);
    expect(w0).toHaveAttribute('data-lit', 'true');
    expect(screen.getByTestId('tx-witness-item-1')).toHaveClass('opacity-25');
    expect(document.querySelector('[data-group-row="lock:run:0xinputlock"]')).toHaveAttribute(
      'data-lit',
      'true'
    );
    expect(document.querySelector('[data-group-row="type:run:0xtypescript"]')).toHaveAttribute(
      'data-dimmed',
      'true'
    );
    expect(document.querySelector('#tx-dep-0')).toHaveAttribute('data-lit', 'true');
    expect(document.querySelector('#tx-dep-1')).toHaveClass('opacity-25');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('opacity-30');
    fireEvent.mouseLeave(w0);
    expect(screen.getByTestId('tx-witness-item-1')).not.toHaveClass('opacity-25');
  });

  // Visual finish V5: a dep row's out point link rests as ink with the faint aqua underline (was
  // `text-info/80`); the global a:hover lights it jade.
  it("rests each cell dep's out point link as ink, not aqua (V5)", async () => {
    render(<TransactionDetailPage />);
    const dep0 = await waitFor(() => {
      const el = document.querySelector('#tx-dep-0');
      if (!el) throw new Error('no dep row yet');
      return el as HTMLElement;
    }, SLOW);
    const deps = [dep0, ...Array.from(document.querySelectorAll('[id^="tx-dep-"]')).slice(1)];
    expect(deps.length).toBeGreaterThan(1);
    for (const dep of deps) {
      const outPoint = dep.querySelector(':scope > a') as HTMLElement | null;
      expect(outPoint, dep.id).not.toBeNull();
      expect(outPoint!.getAttribute('href')).toMatch(/\/cell\/0x[0-9a-z]+-\d+$/);
      expectRestingLink(outPoint!);
      // The out point still wraps anywhere in its narrow column.
      expect(outPoint).toHaveClass('min-w-0', '[overflow-wrap:anywhere]');
    }
  });

  it('says which script each cell dep supplies code for (TX-15)', async () => {
    render(<TransactionDetailPage />);
    const dep0 = await waitFor(() => {
      const el = document.querySelector('#tx-dep-0');
      if (!el) throw new Error('no dep row yet');
      return el as HTMLElement;
    }, SLOW);
    expect(dep0).toHaveTextContent('dep group');
    expect(dep0).toHaveTextContent('supplies code for Default Lock');
    expect(dep0).toHaveTextContent('↳ 0xmember:0 · Default Lock code, 52,048 B');
    expect(dep0).toHaveTextContent('↳ 0xother:1 · 1,048,576 B no script here names');
    expect(document.querySelector('#tx-dep-1')).toHaveTextContent(
      'supplies code for Spore · 76,752 B'
    );
    const dep2 = document.querySelector('#tx-dep-2') as HTMLElement;
    expect(dep2).toHaveTextContent('code of Default Lock · 122,552 B');
    expect(dep2).toHaveTextContent('not run here');
    expect(dep2).not.toHaveTextContent('dep_group');
    fireEvent.mouseEnter(dep2);
    expect(document.querySelector('[data-group-row="lock:attached:0xoutputlock"]')).toHaveAttribute(
      'data-lit',
      'true'
    );
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'true');
    const attached = document.querySelector(
      '[data-group-row="lock:attached:0xoutputlock"]'
    ) as HTMLElement;
    expect(attached).toHaveTextContent('code 0xd00a0222...2c47f7:0 · not run here');
    // In the code column the out point opens the code cell; the kind jumps to the dep row.
    expect(within(attached).getByRole('link', { name: '0xd00a0222...2c47f7:0' })).toHaveAttribute(
      'href',
      expect.stringMatching(new RegExp(`/cell/0xd00a0222${'00'.repeat(26)}2c47f7-0$`))
    );
    expect(within(attached).getByRole('link', { name: 'code' })).toHaveAttribute(
      'href',
      '#tx-dep-2'
    );
    const spore = document.querySelector('[data-group-row="type:run:0xtypescript"]') as HTMLElement;
    expect(within(spore).getByRole('link', { name: '0x96b198fb...b69824:0' })).toHaveAttribute(
      'href',
      expect.stringMatching(new RegExp(`/cell/0x96b198fb${'00'.repeat(26)}b69824-0$`))
    );
    expect(within(spore).getByRole('link', { name: 'code' })).toHaveAttribute('href', '#tx-dep-1');
  });

  it('draws the proposal ruler and the exact capacity equation (TX-17, TX-19)', async () => {
    render(<TransactionDetailPage />);
    expect(
      await screen.findByRole(
        'img',
        {
          name: 'Proposed in block 99, committed 2 blocks later in 101; the commit window is 2 to 10 blocks',
        },
        SLOW
      )
    ).toBeInTheDocument();
    expect(await screen.findByTestId('tx-capacity-equation')).toHaveTextContent(
      '555.00000000 CKB in = 554.99935881 CKB out + 0.00064119 CKB fee'
    );
    expect(screen.getByText(/^occupied 563 → 589/)).toHaveTextContent(
      'occupied 563 → 589 · common knowledge +26'
    );
  });

  it('states the CoBuild actions a witness carries and for whose script (TX-36)', async () => {
    render(<TransactionDetailPage />);
    const w1 = await screen.findByTestId('tx-witness-item-1', {}, SLOW);
    await waitFor(
      () => expect(w1).toHaveTextContent('#1 · CoBuild message7 B · read by Spore'),
      SLOW
    );
    expect(screen.getByTestId('tx-witness-item-1')).toHaveTextContent(
      'message: 1 action for Spore: TransferSpore'
    );
  });
});

describe('Transaction variants (D4)', () => {
  it('draws DAO compensation flowing in, adds it to the equation and lists the header deps (TX-33)', async () => {
    const tx = committed();
    tx.inputsCapacity = '2605000000000';
    tx.inputs![0].capacity = '2605000000000';
    tx.outputsCapacity = '2622530633229';
    tx.outputs![0].capacity = '2622530633229';
    tx.fee = '936';
    tx.headerDeps = [
      { blockHash: `0x${'55'.repeat(32)}`, blockNumber: 5 },
      { blockHash: `0x${'66'.repeat(32)}`, blockNumber: 9 },
    ];
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.daoWithdrawRequest,
      txHash: TX_HASH,
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_complete',
          metadata: { capacity: '2605000000000', compensation: '17530634165' },
        },
      ],
      participants: [
        { ...feedSamples.daoWithdrawRequest.participants[1], ckbDelta: '-2605000000000' },
        { ...feedSamples.daoWithdrawRequest.participants[2], ckbDelta: '2622530633229' },
      ],
    });
    render(<TransactionDetailPage />);
    const inflow = await screen.findByTestId('tx-inflow', {}, SLOW);
    expect(inflow).toHaveAttribute('data-inflow', 'dao');
    expect(inflow).toHaveTextContent('Nervos DAO compensation175.30634165 CKB');
    // What moved (v2): the compensation enters the CKB flow first, from the left, in gold
    const first = screen
      .getByTestId('tx-what-moved')
      .querySelector('[data-flow="ckb"] [data-flow-side="from"] [data-flow-end]');
    expect(first).toHaveAttribute('data-flow-end', 'compensation');
    expect(first).toHaveTextContent('Nervos DAO compensation175.30634165 CKB');
    await screen.findByTestId('tx-capacity-equation', {}, SLOW);
    expect(screen.getByTestId('tx-capacity-equation')).toHaveTextContent(
      '26,050.00000000 CKB in + 175.30634165 CKB compensation = 26,225.30633229 CKB out + 0.00000936 CKB fee'
    );
    expect(screen.getByTestId('tx-balance-net')).toHaveTextContent(
      'Nervos DAO compensation, less the fee'
    );
    const headers = screen.getByTestId('tx-header-deps');
    expect(headers).toHaveTextContent('Header deps');
    expect(headers).toHaveTextContent('2 · read, not consumed');
    expect(within(headers).getByRole('link', { name: '#5' })).toBeInTheDocument();
    expect(within(headers).getByRole('link', { name: '#9' })).toBeInTheDocument();
  });

  it('shows a cellbase with its block reward source, decoded witness and no proposal (TX-23, TX-35)', async () => {
    const message = `0x${Buffer.from('0.210.0 (860c9bb 2026-09-21) ').toString('hex')}0011223344556677`;
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...committed(),
      isCellbase: true,
      fee: '0',
      feeRate: null,
      cycles: 0,
      inputs: [],
      inputsCapacity: '0',
      inputsCommonKnowledgeSize: '0',
      outputsCapacity: '55961739927',
      outputsCommonKnowledgeSize: '61',
      outputs: [{ ...committed().outputs![0], capacity: '55961739927', commonKnowledgeSize: '61' }],
      scriptGroups: [committed().scriptGroups![2]],
      witnessDetails: [
        {
          index: 0,
          size: 127,
          kind: 'cellbase',
          fields: [
            { name: 'table header', start: 0, end: 12, hex: '0x' },
            { name: 'lock', start: 12, end: 85, hex: '0x' },
            { name: 'message', start: 85, end: 127, hex: '0x' },
          ],
        },
      ],
      witnesses: ['0x00'],
      cellbaseWitness: { lock: lockScript(`0x${'ab'.repeat(20)}`), message },
      feePaidBy: [],
      pays: { number: 89, hash: '0x' + '89'.repeat(32) },
    } as TransactionDetail);
    vi.mocked(api.getBlock).mockResolvedValue({
      number: 101,
      minerMessage: message,
      minerMessageText: '0.210.0 (860c9bb 2026-09-21)',
    } as Awaited<ReturnType<typeof api.getBlock>>);
    render(<TransactionDetailPage />);
    expect(
      await screen.findByRole('heading', { level: 1, name: 'Cellbase' }, SLOW)
    ).toBeInTheDocument();
    expect(screen.getByTestId('tx-summary')).toHaveTextContent(
      'The first transaction of block #101. It consumes no cell and pays the reward of block #89, 559.61739927 CKB.'
    );
    // R-6a: the paid block is linked by its canonical hash.
    expect(
      within(screen.getByTestId('tx-summary')).getByRole('link', { name: '#89' })
    ).toHaveAttribute('href', `/mainnet/blocks/0x${'89'.repeat(32)}`);
    // Lane L: the block the cellbase is in is a link too.
    expect(
      within(screen.getByTestId('tx-summary')).getByRole('link', { name: '#101' })
    ).toHaveAttribute('href', '/mainnet/blocks/101');
    expect(screen.getByTestId('tx-inflow')).toHaveTextContent('block reward559.61739927 CKB');
    expect(screen.getByText(/never proposed/)).toBeInTheDocument();
    await waitFor(
      () =>
        expect(screen.getByTestId('tx-witness-item-0')).toHaveTextContent(
          'miner lock Default Lock(0xabab...abab) · message “0.210.0 (860c9bb 2026-09-21)”'
        ),
      SLOW
    );
    expect(screen.getByTestId('tx-groups-ran')).toHaveTextContent('a cellbase runs no script');
    expect(api.getTransactionActivity).not.toHaveBeenCalled();
    expect(api.getTransactionLifecycle).not.toHaveBeenCalled();
  });

  it('says a pending transaction will run when committed, and still lists its witnesses (TX-37)', async () => {
    const tx = { ...committed(), status: 'pending', pendingSince: '2026-10-30T08:14:17Z' };
    for (const key of ['blockNumber', 'blockHash', 'index', 'timestamp', 'confirmations'] as const)
      delete (tx as Partial<TransactionDetail>)[key];
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx as TransactionDetail);
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('tx-groups-ran', {}, SLOW)).toHaveTextContent(
      'will run when committed'
    );
    expect(screen.queryByText(/0 ran/)).not.toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-item-0')).toBeInTheDocument();
    expect(screen.getByText('Witnesses · 2')).toBeInTheDocument();
    expect(screen.getByText('Pending since')).toBeInTheDocument();
  });

  // Review 2026-11-03: the evidence tier reads the witnesses and the cell deps first, then the
  // script groups that read them.
  it('sets Witnesses and Cell deps above Script groups in the evidence tier', async () => {
    render(<TransactionDetailPage />);
    const witnesses = await screen.findByRole('heading', { name: 'Witnesses · 2' }, SLOW);
    const deps = screen.getByRole('heading', { name: 'Cell deps' });
    const groups = screen.getByRole('heading', { name: 'Script groups' });
    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(before(witnesses, groups)).toBe(true);
    expect(before(deps, groups)).toBe(true);
  });

  // Lane U follow-up (user report "two Script groups"): the pool view names its script groups once,
  // the designed matrix in tier 2; no second list from the tier-1 tiles.
  it('shows one Script groups section for a pending transaction', async () => {
    const tx = { ...committed(), status: 'pending', pendingSince: '2026-10-30T08:14:17Z' };
    for (const key of ['blockNumber', 'blockHash', 'index', 'timestamp', 'confirmations'] as const)
      delete (tx as Partial<TransactionDetail>)[key];
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx as TransactionDetail);
    render(<TransactionDetailPage />);
    await screen.findByTestId('tx-groups-ran', {}, SLOW);
    expect(screen.getAllByRole('heading', { name: /script groups/i })).toHaveLength(1);
    expect(screen.getByRole('heading', { name: 'Script groups' })).toBeInTheDocument();
    expect(screen.queryByText(/Capacity entering the pool/)).toBeNull();
  });
});

describe('Transaction board, R2b-a fields', () => {
  it('marks change outputs and reads decoded lock args (TX-22, TX-20)', async () => {
    render(<TransactionDetailPage />);
    const out = await screen.findByTestId('tx-io-output-0', {}, SLOW);
    await waitFor(() => expect(out).toHaveTextContent('Default Lock · change'), SLOW);
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveTextContent('· change');
    expect(document.querySelector('[data-group-row="lock:run:0xinputlock"]')).toHaveTextContent(
      'pubkey hash 0x111111...1111'
    );
  });

  it('reads a Shadow Lock and a JoyID lock by their registry layouts (R-8, TX-20)', async () => {
    const base = committed();
    const shadow = `0x31da${'00'.repeat(28)}5d74`;
    const joyidReason = 'JoyID: args are 23 bytes, layout subType(2) + pubkeyHash(20) expects 22';
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...base,
      inputs: [
        {
          ...base.inputs![0],
          // Pass 3 (revised R-8): a real 23-byte JoyID args — not decoded, the reason served.
          lockArgsDecoded: null,
          lockArgsDecodeError: joyidReason,
        },
      ],
      outputs: [
        {
          ...base.outputs![0],
          isChange: false,
          display: {
            kind: 'object',
            standard: 'spore',
            id: `0x${'51'.repeat(32)}`,
            name: null,
            collectionId: null,
            collectionName: null,
            symbol: null,
            decimals: null,
            amount: null,
            noAmountReason: null,
            compositionTier: null,
          },
          lockArgsDecoded: { lock: 'shadow-lock', prefix: '0x07', hash: shadow },
          lockArgsDecodeError: null,
        },
      ],
    } as TransactionDetail);
    render(<TransactionDetailPage />);
    await screen.findByTestId('tx-io-output-0', {}, SLOW);
    // A misfit lock says why its args were not decoded, quietly, in place of the fields.
    await waitFor(
      () =>
        expect(document.querySelector('[data-group-row="lock:run:0xinputlock"]')).toHaveTextContent(
          `args not decoded: ${joyidReason}`
        ),
      SLOW
    );
    expect(
      document.querySelector('[data-group-row="lock:attached:0xoutputlock"]')
    ).toHaveTextContent('args 0x07 + hash 0x31da00...5d74');
    const owner = screen.getByTestId('tx-fact-output-lock-0');
    expect(owner).toHaveTextContent('New owner of the object in output 0');
    expect(owner).toHaveTextContent('args 0x07 + hash 0x31da00...5d74');
    expect(owner).not.toHaveTextContent(/CKBFS|held through/);
  });

  it('names a CoBuild proxy action and shows the script info hash of an undeclared one (R-7, TX-36)', async () => {
    const base = committed();
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...base,
      witnessDetails: [
        base.witnessDetails![0],
        {
          ...base.witnessDetails![1],
          actions: [
            {
              scriptInfoHash: `0x${'44'.repeat(32)}`,
              scriptHash: '0xtypescript',
              data: '0x01',
              schema: 'spore',
              name: 'MintProxy',
            },
            {
              scriptInfoHash: `0x${'45'.repeat(32)}`,
              scriptHash: '0xtypescript',
              data: '0x02',
              schema: null,
              name: null,
            },
          ],
        },
      ],
    } as TransactionDetail);
    render(<TransactionDetailPage />);
    const w1 = await screen.findByTestId('tx-witness-item-1', {}, SLOW);
    await waitFor(
      () =>
        expect(w1).toHaveTextContent(
          'message: 2 actions for Spore: MintProxy, Spore (script info 0x454545...4545)'
        ),
      SLOW
    );
  });

  it('names the type scripts a plain script call ran (TX-11)', async () => {
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.objectTransfer,
      txHash: TX_HASH,
      typeCalls: [
        {
          typeCodeHash: '0xtime',
          typeHashType: 'type',
          typeArgs: '0x02',
          scriptHash: '0xs1',
          scriptName: '.bit Time Index State',
        },
        {
          typeCodeHash: '0xinfo',
          typeHashType: 'type',
          typeArgs: '0x02',
          scriptHash: '0xs2',
          scriptName: '.bit Time Info',
        },
      ],
      lockCalls: [],
      participants: [
        {
          ...feedSamples.objectTransfer.participants[0],
          ckbDelta: '-64119',
          usedDelta: '0',
          itemDeltas: [],
        },
      ],
    });
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('tx-summary', {}, SLOW)).toHaveTextContent(
      'It ran .bit Time Index State and .bit Time Info. ckb1qzda...cjk9lz paid a 0.00064119 CKB fee.'
    );
  });
});

describe('What moved (the asset flows, review 2026-11-03 v2)', () => {
  it('draws the object flow and the CKB flow, from the left to the right, before the ledger', async () => {
    render(<TransactionDetailPage />);
    const frame = await screen.findByTestId('tx-what-moved', {}, SLOW);
    const groups = within(frame).getAllByRole('group');
    expect(groups.map((g) => g.getAttribute('data-flow'))).toEqual([
      `object:0x51c75a${'00'.repeat(26)}96f2`,
      'ckb',
    ]);
    const [object, ckb] = groups;
    expect(object.querySelector('[data-flow-side="from"]')).toHaveTextContent('ckb1qzda...cjk9lz');
    expect(object.querySelector('[data-flow-side="to"]')).toHaveTextContent('ckb1qzda...shp3hr');
    expect(object.querySelector('[data-flow-plate]')).toHaveTextContent('Nervape 3x');
    expect(object.querySelector('[data-composition="btc_ckb"]')).not.toBeNull();
    // the CKB flow: cjk9lz gave 528.00064119; shp3hr and 204m4g received; the miner the fee
    expect(ckb.querySelector('[data-flow-side="from"]')).toHaveTextContent('528.00064119 CKB');
    expect(
      Array.from(ckb.querySelectorAll('[data-flow-side="to"] [data-flow-end]')).map((e) =>
        e.getAttribute('data-flow-end')
      )
    ).toEqual(['party', 'party', 'miner']);
    expect(ckb.querySelector('[data-flow-end="miner"]')).toHaveTextContent('0.00064119 CKB');
    // its place: under its own heading, before the Balance changes ledger
    const heading = screen.getByRole('heading', { name: 'What moved' });
    const table = screen.getByRole('table', { name: 'Balance changes by participant' });
    expect(Boolean(heading.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(
      true
    );
  });

  it('lights a hovered party’s cells in the cells diagram and dims the rest', async () => {
    const tx = committed();
    tx.inputs![0].address = feedSamples.objectTransfer.participants[0].address!;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);
    const frame = await screen.findByTestId('tx-what-moved', {}, SLOW);
    const [object] = within(frame).getAllByRole('group');
    const giver = object.querySelector('[data-flow-side="from"] [data-flow-end="party"]')!;
    fireEvent.mouseEnter(giver);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('opacity-30');
    fireEvent.mouseLeave(giver);
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('opacity-30');
  });
});
