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
import { expectBefore, expectOnePageSealOf, parsed, SEAL_INKS } from '@/__tests__/utils/seals';
import { PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import { drawSeal, type SealId, type SealState } from '@/lib/seal/stones';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { api, type GlobalActivity, type TransactionDetail } from '@/lib/api';

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
        scriptHash: INPUT_LOCK,
        kind: 'lock',
        ran: true,
        inputIndices: [0],
        outputIndices: [],
        witnessIndex: 0,
        script: lockScript(`0x${'11'.repeat(20)}`),
        cycles: '1694860',
      },
      {
        scriptHash: TYPE_SCRIPT,
        kind: 'type',
        ran: true,
        inputIndices: [0],
        outputIndices: [0],
        witnessIndex: 1,
        script: typeScript,
        cycles: '228853',
      },
      {
        scriptHash: OUTPUT_LOCK,
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
            scriptHash: TYPE_SCRIPT,
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

/**
 * The page's seals by kind (C2): the header presses one page stone of `id` (its long stone, 94 px,
 * 64 below md), and — when the cells are drawn — the cells frame presses the same stone again at
 * 64 px, its seam seal, shown from lg only. Returns both (the seam seal null when none).
 */
function expectTxSeals(
  container: HTMLElement,
  id: SealId,
  state: SealState,
  seam: boolean
): { head: HTMLElement; seam: HTMLElement | null } {
  const header = container.querySelector('header') as HTMLElement;
  const head = expectOnePageSealOf(header, id, state);
  const all = container.querySelectorAll('[data-seal][data-seal-form="page"]');
  const holders = container.querySelectorAll<HTMLElement>('[data-seam-seal]');
  if (!seam) {
    expect(all).toHaveLength(1);
    expect(holders).toHaveLength(0);
    return { head, seam: null };
  }
  // one extra impression, never a second one in the header
  expect(all).toHaveLength(2);
  expect(holders).toHaveLength(1);
  const [holder] = Array.from(holders);
  expect(header).not.toContainElement(holder);
  expect(screen.getByTestId('tx-cells-frame')).toContainElement(holder);
  // from lg only (below it the cells stack and the header's stone is the one), and hidden from a
  // screen reader, which has the header's
  expect(holder).toHaveClass('hidden', 'lg:flex');
  expect(holder).toHaveAttribute('aria-hidden', 'true');
  const stamp = holder.querySelector<HTMLElement>('[data-seal]')!;
  expect(stamp).toHaveAttribute('data-seal', 'tx');
  expect(stamp).toHaveAttribute('data-seal-form', 'page');
  expect(stamp).toHaveAttribute('data-seal-state', state);
  expect(stamp.innerHTML).toBe(
    parsed(
      drawSeal(id, { form: 'page', height: 64, state, ...SEAL_INKS, tilt: PAGE_SEAL_TILT }).svg
    )
  );
  expect(stamp.querySelector('a')).toBeNull();
  expect(stamp.closest('a')).toBeNull();
  return { head, seam: stamp };
}

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
      scriptHashes: [INPUT_LOCK],
      outputLockScriptHashes: [],
      resolvedCells: [
        {
          txHash: '0xmember',
          index: 0,
          dataHash: '0xdata',
          typeHash: null,
          scriptHashes: [INPUT_LOCK],
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
      scriptHashes: [TYPE_SCRIPT],
      outputLockScriptHashes: [],
      resolvedCells: [
        {
          txHash: `0x96b198fb${'00'.repeat(26)}b69824`,
          index: 0,
          dataHash: '0xd3',
          typeHash: null,
          scriptHashes: [TYPE_SCRIPT],
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
      outputLockScriptHashes: [OUTPUT_LOCK],
      resolvedCells: [
        {
          txHash: `0xd00a0222${'00'.repeat(26)}2c47f7`,
          index: 0,
          dataHash: '0xd4',
          typeHash: '0xtt',
          scriptHashes: [],
          outputLockScriptHashes: [OUTPUT_LOCK],
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

  // The seals by kind (C2; the Transaction board's one corner): the transaction's long stone,
  // 94 px (64 on a phone, −4°), its rods the cells it consumed and created, pressed beside the
  // page's own header — the eyebrow, the action, the summary and the hash — never in the facts.
  it('presses the transaction’s stone beside its header, outside the facts and the cells', async () => {
    const { container } = render(<TransactionDetailPage />);
    const h1 = await screen.findByRole('heading', { level: 1, name: 'Object transfer' }, SLOW);
    await screen.findByTestId('tx-cells-frame', {}, SLOW);
    const { head: seal } = expectTxSeals(
      container,
      { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
      'pressed',
      true
    );
    const head = seal.closest('[data-sealed-head]')!;
    expect(head.closest('header')).toContainElement(h1);
    expect(head).toContainElement(h1);
    expect(head).toContainElement(screen.getByTestId('tx-summary'));
    expect(head).toContainElement(screen.getByRole('button', { name: 'Copy transaction hash' }));
    expectBefore(seal, h1);
    expect(head).not.toContainElement(
      await screen.findByRole('complementary', { name: 'Transaction facts' }, SLOW)
    );
    expect(head).not.toContainElement(
      await screen.findByRole('heading', { name: 'Cells 1 in → 1 out' }, SLOW)
    );
  });

  // The seam seal (C2): from lg, where the cells lie consumed | capacity pool | created, the same
  // stone is pressed again at the head of the pool column, its left rods (consumed) toward the
  // consumed cells and its right rods (created) toward the created ones; the column's label moves
  // under it and both cell columns start under the band.
  it('presses the same stone across the cells, at the head of the capacity pool column, from lg', async () => {
    const { container } = render(<TransactionDetailPage />);
    await screen.findByTestId('tx-cells-frame', {}, SLOW);
    const { seam } = expectTxSeals(
      container,
      { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
      'pressed',
      true
    );
    const pool = seam!.closest('[data-seam-seal]')!.parentElement!;
    const label = within(pool).getByText('Capacity pool');
    expect(pool.firstElementChild).toBe(seam!.parentElement);
    expectBefore(seam!, label);
    expect(label).toHaveClass('lg:absolute', 'lg:top-[72px]');
    // the pool column stands between the consumed cells and the created ones
    const canvas = container.querySelector('[data-tx-canvas]')!;
    expect(Array.from(canvas.children).indexOf(pool)).toBe(1);
    expect(canvas.children[0]).toHaveTextContent('1 cell consumed');
    expect(canvas.children[2]).toHaveTextContent('1 cell created');
    for (const side of [canvas.children[0], canvas.children[2]])
      expect(side.querySelector('h3')).toHaveClass('mb-3', 'lg:mb-0', 'lg:min-h-[88px]');
  });

  // Rubbing × Song print, phase 5 (T4): the board's comment under the sealed head, in the identity
  // column: three lines from the figures the page draws — the cells heading's counts, the rail's
  // fee and the evidence tier's common-knowledge delta (563 → 589 occupied bytes).
  it('comments on the cells, the fee and common knowledge under the sealed head, before the facts', async () => {
    const { container } = render(<TransactionDetailPage />);
    const first = await screen.findByText('1 cell became 1.', {}, SLOW);
    const comment = first.closest('.comment') as HTMLElement;
    expect(comment).not.toBeNull();
    expect(
      Array.from(comment.querySelectorAll('[data-comment-line]')).map((l) => l.textContent)
    ).toEqual([
      '1 cell became 1.',
      '0.00064119 ckb to the miner.',
      'common knowledge grew by 26 bytes.',
    ]);
    expect(comment).toHaveClass('mt-4', 'max-w-[60ch]');
    expect(container.querySelectorAll('.comment')).toHaveLength(1);
    // under the seal and the identity, in the identity column, inside the page's header
    const head = container.querySelector('[data-sealed-head]') as HTMLElement;
    const below = comment.parentElement!;
    expect(below).toHaveAttribute('data-sealed-below');
    expect(below.parentElement).toBe(head);
    expect(below).toHaveClass('col-start-2');
    expect(head.closest('header')).toContainElement(comment);
    // reading order: the identity (the hash and its copy button), the comment, then the facts
    expectBefore(screen.getByRole('button', { name: 'Copy transaction hash' }), comment);
    expectBefore(comment, screen.getByRole('complementary', { name: 'Transaction facts' }));
    // the fee is the one the rail prints, digit for digit
    expect(screen.getByRole('complementary', { name: 'Transaction facts' })).toHaveTextContent(
      '0.00064119 CKB'
    );
    // the counts are the cells heading's
    expect(screen.getByRole('heading', { name: 'Cells 1 in → 1 out' })).toBeInTheDocument();
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
    const cells = screen.getByRole('heading', { name: 'Cells 1 in → 1 out' });
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
    expect(document.querySelector(`[data-group-row="lock:run:${INPUT_LOCK}"]`)).toHaveAttribute(
      'data-lit',
      'true'
    );
    expect(document.querySelector(`[data-group-row="type:run:${TYPE_SCRIPT}"]`)).toHaveAttribute(
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
    expect(
      document.querySelector(`[data-group-row="lock:attached:${OUTPUT_LOCK}"]`)
    ).toHaveAttribute('data-lit', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'true');
    const attached = document.querySelector(
      `[data-group-row="lock:attached:${OUTPUT_LOCK}"]`
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
    const spore = document.querySelector(
      `[data-group-row="type:run:${TYPE_SCRIPT}"]`
    ) as HTMLElement;
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
      // The one input of every cellbase as the detail endpoint serves it: the null out point,
      // no cell behind it, every cell field null (a page that read `lock.codeHash` off it crashed).
      inputs: [
        {
          previousOutput: { txHash: `0x${'00'.repeat(32)}`, index: -1 },
          since: '0x14023e7',
          capacity: null,
          commonKnowledgeSize: null,
          lock: null,
          type: null,
          address: null,
          cellType: null,
          display: null,
        } as unknown as NonNullable<TransactionDetail['inputs']>[number],
      ],
      inputsCapacity: '0',
      inputsCommonKnowledgeSize: '0',
      outputsCapacity: '55961739927',
      outputsCommonKnowledgeSize: '61',
      // a cellbase's reward cell is plain, 61 bytes: no type script, so no type group
      outputs: [
        {
          ...committed().outputs![0],
          type: undefined,
          cellType: 'plain',
          capacity: '55961739927',
          commonKnowledgeSize: '61',
        },
      ],
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
    // A cellbase consumes no cell: the cells command runs on 0 in (T1), and its comment says where
    // its cells come from, with no fee line (T4).
    expect(screen.getByRole('heading', { name: 'Cells 0 in → 1 out' })).toBeInTheDocument();
    // C2: its stones say the same 0 → 1 — the API counts its one null input (inputsCount 1), the
    // page knows a cellbase by isCellbase — on the header and across the cells
    const { head, seam } = expectTxSeals(
      document.body,
      { kind: 'tx', hash: TX_HASH, consumed: 0, created: 1 },
      'pressed',
      true
    );
    for (const stone of [head, seam!])
      expect(stone.querySelector('svg')).toHaveAttribute(
        'aria-label',
        'Transaction seal 0x57a54eb7, 0 consumed, 1 created'
      );
    const comment = screen.getByText('1 cell from the block reward; none consumed.').closest('p')!;
    expect(
      Array.from(comment.querySelectorAll('[data-comment-line]')).map((l) => l.textContent)
    ).toEqual([
      '1 cell from the block reward; none consumed.',
      'common knowledge grew by 61 bytes.',
    ]);
    expect(comment).not.toHaveTextContent('to the miner');
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
    // No comment while it waits: its cells and occupied bytes are read once it is indexed (T4).
    expect(document.querySelector('.comment')).toBeNull();
    expect(document.querySelector('[data-sealed-below]')).toBeNull();
    // C2: not in a block yet, so its stone is dashed; its cells are not drawn, so no seam seal.
    expect(screen.queryByTestId('tx-cells-frame')).toBeNull();
    expectTxSeals(
      document.body,
      { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
      'unpressed',
      false
    );
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
    expect(document.querySelector(`[data-group-row="lock:run:${INPUT_LOCK}"]`)).toHaveTextContent(
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
        expect(
          document.querySelector(`[data-group-row="lock:run:${INPUT_LOCK}"]`)
        ).toHaveTextContent(`args not decoded: ${joyidReason}`),
      SLOW
    );
    expect(
      document.querySelector(`[data-group-row="lock:attached:${OUTPUT_LOCK}"]`)
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
              scriptHash: TYPE_SCRIPT,
              data: '0x01',
              schema: 'spore',
              name: 'MintProxy',
            },
            {
              scriptInfoHash: `0x${'45'.repeat(32)}`,
              scriptHash: TYPE_SCRIPT,
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
