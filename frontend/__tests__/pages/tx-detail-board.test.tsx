/**
 * The Transaction board (Main.dc.html v5) and the D4 variants, page level (F2-13 / report A):
 * headline + summary + copy, the facts rail, Balance changes with its Net row, hover linking by
 * group identity, resolved cell deps, the proposal ruler, the capacity equation, header deps,
 * the cellbase and DAO-withdrawal variants and the pending wording.
 */
import { JOYID_CODE_HASH } from '@/__tests__/utils/ckb-address';
import { addressScript } from '@/lib/lock-script';
import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { factsOf } from '@/__tests__/utils/facts';
import {
  expectBefore,
  expectNamePlate,
  expectOnePageSeal,
  parsed,
  SEAL_INKS,
} from '@/__tests__/utils/seals';
import { PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import { drawSeal, type SealId, type SealState } from '@/lib/seal/stones';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { api, type GlobalActivity, type TransactionDetail } from '@/lib/api';
import { expectAnswerUnclipped } from '@/__tests__/utils/copy-answer';

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
  // lane D3: the page reads the URL's anchor (a cell's out point); these tests give none
  useSearchAndHash: () => (mockSearchParams.size ? `?${mockSearchParams}` : ''),
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
        // B4 second fix round: the real address of the cell's own lock (a tile refuses one that spells
        // another; this one was not an address)
        address: ckbAddress({ codeHash: LOCK_CODE_HASH, args: `0x${'11'.repeat(20)}` }),
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
        // B4 second fix round: the real address of the cell's own lock (a tile refuses one that spells
        // another; this one was not an address)
        address: ckbAddress({ codeHash: LOCK_CODE_HASH, args: `0x${'22'.repeat(20)}` }),
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
 * 64 below md), and — when the cells are drawn — the same stone is pressed again at 64 px across
 * the seam between the two sheets (C11: two impressions of it, its left half on the consumed sheet
 * and its right half on the created one), shown from lg only. Re-pinned on purpose (lane D3,
 * PA-07): below lg the same stone is pressed across the gap of the stacked sheets, a second
 * holder of two halves (upper, lower) shown below lg only; it was none. Returns the head's stone
 * and the seam seal's first half (null when none).
 */
function expectTxSeals(
  container: HTMLElement,
  id: SealId,
  state: SealState,
  seam: boolean
): { head: HTMLElement; seam: HTMLElement | null } {
  const header = container.querySelector('header') as HTMLElement;
  const head = expectOnePageSeal(header, id, state);
  const all = container.querySelectorAll('[data-seal][data-seal-form="page"]');
  const holders = container.querySelectorAll<HTMLElement>('[data-seam-seal]');
  if (!seam) {
    expect(all).toHaveLength(1);
    expect(holders).toHaveLength(0);
    return { head, seam: null };
  }
  // the seam seal's two halves of one stone in each of its two forms (one shown at a width), never
  // a second one in the header
  expect(all).toHaveLength(5);
  expect(Array.from(holders).map((h) => h.getAttribute('data-seam-seal'))).toEqual([
    'side-by-side',
    'stacked',
  ]);
  const [holder, stacked] = Array.from(holders);
  for (const h of [holder, stacked]) {
    expect(header).not.toContainElement(h);
    expect(screen.getByTestId('tx-cells-frame')).toContainElement(h);
    // in the seam (a column from lg, the row between the stacked sheets below it), hidden from a
    // screen reader, which has the header's
    expect(h.parentElement).toHaveAttribute('data-seam');
    expect(h).toHaveAttribute('aria-hidden', 'true');
    expect(
      Array.from(h.querySelectorAll<HTMLElement>('[data-seal]')).map((s) =>
        s.closest('[data-seam-half]')!.getAttribute('data-seam-half')
      )
    ).toEqual(['consumed', 'created']);
  }
  expect(holder).toHaveClass('hidden', 'lg:flex');
  expect(stacked).toHaveClass('flex', 'lg:hidden');
  const stamps = Array.from(holders).flatMap((h) =>
    Array.from(h.querySelectorAll<HTMLElement>('[data-seal]'))
  );
  for (const stamp of stamps) {
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
  }
  return { head, seam: stamps[0] };
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

/**
 * A feed sample's party at the row size, by the seed its fixture spells its args from: `ckb1…` and
 * the last 8 characters (R9; B4 re-pinned: the samples carry real addresses, a row reads the lock
 * off them, and their last characters are their checksum).
 */
const row = (seed: string) => `ckb1…${seededAddress(seed).slice(-8)}`;

describe('Transaction board, tier 1', () => {
  it('heads the page with the action, a plain summary and a copy button (TX-11)', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<TransactionDetailPage />);
    const h1 = await screen.findByRole('heading', { level: 1, name: 'Object transfer' });
    // the kind line says "Transaction" before the action (seals by kind, C10)
    const kindLine = document.querySelector('[data-sealed-kind]') as HTMLElement;
    expect(kindLine).toHaveTextContent(/^Transaction$/);
    expectBefore(kindLine, h1);
    expect(h1.querySelector('[data-class]')).toBeNull();
    const summary = await screen.findByTestId('tx-summary');
    expect(summary).toHaveTextContent(
      // U-09 (re-pinned): the summary under the headline is the lede, its ids at the head size;
      // R9 (re-pinned): an address is `ckb1…` and its last 8 characters
      // R9 (re-pinned): prose takes the row form, the object's id 10…6
      // re-pinned on purpose (lane D2, gate D0′'s "said once"): the fee stands in the status strip,
      // so the summary's last sentence, "The sender paid a 0.00064119 CKB fee.", is gone
      `Nervape 3x 0x51c75a00…0096f2 moved from ${row('cjk9lz')} to ${row('shp3hr')}.`
    );
    expect(summary.querySelector('[data-class="objects"]')).toBeInTheDocument();
    const copy = screen.getByRole('button', { name: 'copy transaction hash' });
    fireEvent.click(copy);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(TX_HASH));
    // A4 review 3: the one copy path; a copy that worked says so for a moment. Re-pinned on
    // purpose (lane c2-chain): the library's CopyButton says it in words beside the icon, heard
    // as a status, `copied` (it was the button's title, "Copied"); the name stays lowercase.
    await waitFor(() =>
      expect(within(copy.parentElement!).getByRole('status')).toHaveTextContent(/^copied$/)
    );
  });

  // A4 review 3: over plain HTTP on a LAN address there is no clipboard. The copy button copies
  // through the one copy path: nothing claims a copy, and nothing rejects unhandled. Re-pinned on
  // purpose (lane c2-chain): the library's CopyButton says so, `not copied` (the controls board;
  // the old button's title stayed "Copy transaction hash" and said nothing).
  it('claims no copy and rejects nothing where the clipboard is absent', async () => {
    const rejections: unknown[] = [];
    const onRejection = (reason: unknown) => rejections.push(reason);
    process.on('unhandledRejection', onRejection);
    try {
      Object.assign(navigator, { clipboard: undefined });
      render(<TransactionDetailPage />);
      await screen.findByRole('heading', { level: 1, name: 'Object transfer' });
      const button = screen.getByRole('button', { name: 'copy transaction hash' });
      await act(async () => {
        fireEvent.click(button);
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
      const answer = within(button.parentElement!).getByRole('status');
      expect(answer).toHaveTextContent(/^not copied$/);
      expect(answer).not.toHaveTextContent(/^copied$/);
      expect(rejections).toEqual([]);
    } finally {
      process.off('unhandledRejection', onRejection);
    }
  });

  // The seals by kind (C2; the Transaction board's one corner): the transaction's long stone,
  // 94 px (64 on a phone, −4°), its rods the cells it consumed and created, never in the facts.
  // The name plate (C10): beside it the kind line "Transaction", the action and the hash with its
  // copy button; the summary and the comment under both, from the page's left edge.
  it('presses the transaction’s stone beside its name plate, the summary under both, outside the facts and the cells', async () => {
    const { container } = render(<TransactionDetailPage />);
    const h1 = await screen.findByRole('heading', { level: 1, name: 'Object transfer' });
    await screen.findByTestId('tx-cells-frame');
    const { head: seal } = expectTxSeals(
      container,
      { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
      'pressed',
      true
    );
    const header = container.querySelector('header') as HTMLElement;
    const { head, stone, kindLine, nameBox, title, below } = expectNamePlate(header, 'Transaction');
    expect(stone).toBe(seal);
    expect(title).toBe(h1);
    expect(h1).toHaveClass('text-entity', 'max-md:text-index');
    expect(kindLine.children).toHaveLength(1);
    // the plate: the action, then the id line — the hash and its copy button, nothing else
    expect(nameBox.children).toHaveLength(2);
    const idLine = nameBox.lastElementChild as HTMLElement;
    expect(idLine).toHaveTextContent(new RegExp(`^${TX_HASH}$`));
    expect(idLine).toContainElement(screen.getByRole('button', { name: 'copy transaction hash' }));
    // C10 round 2: the id line in the plate's one voice, set by the plate; the hash and its
    // row carry no size or ink of their own (the row was text-small, the hash text-text-dim), and
    // the copy button keeps its own classes
    // re-pinned with readability ruling R3: one voice still, in the reading ink (the id is a
    // value; it was text-text-dim)
    expect(idLine.className).toBe('text-text text-small min-w-0 [overflow-wrap:anywhere]');
    // the lead's ruling a: the hash is the plate's PlateHash, one line or two halves, never a
    // scroller (re-pinned: it was a bare span, 'min-w-0'); still no size or ink of its own
    // re-pinned (A4 Chrome F3): the piece is every head's HeadId, still no size or ink of its own
    const hash = idLine.querySelector('[data-head-id]') as HTMLElement;
    expect(hash).toHaveAttribute('title', TX_HASH);
    // re-pinned (lane c2-finish, item 4: copy has one form): HeadId draws the copy after the id,
    // the one form of every head (8 px after it, at its first line: was items-center gap-0.5 by
    // the page's hand); the id no longer positions a tick over itself (was 'relative'). Re-pinned
    // again (lane c2-finish, rulings round 2): the id takes its content's width (it was flex-1 and
    // filled the row, so its copy stood at the row's far end, 352 to 727 px after the hash at 1280)
    expect(hash.className).toBe('block min-w-0 font-mono');
    expect(hash.querySelector('.overflow-x-auto')).toBeNull();
    expect(hash.parentElement!.className).toBe('flex min-w-0 items-start gap-2');
    // re-pinned on purpose (lane c2-chain): the copy is the library's CopyButton, the board's one
    // icon in the quiet ink in a 32 px box below 640 and 24 px from it, kept whole beside the
    // hash (was a 44 px box pulled 12 px into the line, -my-3 h-11 w-11). Re-pinned (lane
    // c2-finish, rulings round 1): its answer took 6 px and its words (43.2 or 72 px) from the
    // hash's line while it stood; it hangs under the whole id now, out of the line's flow
    const copy = screen.getByRole('button', { name: 'copy transaction hash' });
    expect(copy).toHaveClass('text-text-dim', 'hover:text-jade', 'h-8', 'w-8', 'sm:h-6', 'sm:w-6');
    // lane c2-finish, rulings round 1: the copy's answer stands out of the line's flow under
    // its box, and nothing above the copy clips it
    expectAnswerUnclipped(copy);
    expect(copy.parentElement).toHaveClass('shrink-0', 'self-stretch');
    // under both: re-pinned on purpose (lane D2, gate D0′): the status strip first, then the
    // summary (the summary was first; the facts stood in a rail outside the head)
    const strip = screen.getByRole('group', { name: 'Transaction status' });
    expect(below!.firstElementChild).toBe(strip);
    expect(below!.children[1]).toBe(screen.getByTestId('tx-summary'));
    expect(nameBox).not.toContainElement(screen.getByTestId('tx-summary'));
    expect(nameBox).not.toContainElement(strip);
    expect(head.closest('header')).toContainElement(h1);
    expectBefore(seal, h1);
    expect(head).not.toContainElement(
      await screen.findByRole('heading', { name: 'Cells 1 in → 1 out' })
    );
  });

  // The seam seal (C2, C11): from lg the cells lie on two sheets, consumed | created, with a 4 px
  // seam between; the same stone is pressed across that seam, its left half on the consumed sheet
  // and its right half on the created one. The sheets' heads end with the API's exact totals.
  it('presses the same stone across the seam between the two sheets, from lg', async () => {
    const { container } = render(<TransactionDetailPage />);
    await screen.findByTestId('tx-cells-frame');
    const { seam } = expectTxSeals(
      container,
      { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
      'pressed',
      true
    );
    const canvas = container.querySelector<HTMLElement>('[data-tx-canvas]')!;
    const [consumed, gap, created] = Array.from(canvas.children) as HTMLElement[];
    expect(consumed).toHaveAttribute('data-sheet', 'consumed');
    expect(gap).toHaveAttribute('data-seam');
    expect(gap).toContainElement(seam!);
    expect(created).toHaveAttribute('data-sheet', 'created');
    expect(canvas).not.toHaveTextContent(/capacity pool/i);
    expect(within(consumed).getByRole('heading', { name: '1 cell consumed' })).toBeInTheDocument();
    expect(within(created).getByRole('heading', { name: '1 cell created' })).toBeInTheDocument();
    // the totals are the API's inputsCapacity and outputsCapacity, exact
    expect(consumed.querySelector('[data-sheet-total]')).toHaveTextContent(/^555(\.0+)? CKB$/);
    expect(created.querySelector('[data-sheet-total]')).toHaveTextContent(/^554\.99935881 CKB$/);
    // the heads hold the stone's band; the fee closes the created sheet
    for (const sheet of [consumed, created])
      expect(sheet.querySelector('[data-sheet-head]')).toHaveClass('lg:min-h-[70px]');
    expect(within(created).getByTestId('tx-fee')).toHaveTextContent(/^fee0\.00064119 CKB$/);
  });

  // Rubbing × Song print, phase 5 (T4): the board's comment, from the figures the page draws — the
  // cells heading's counts and the evidence tier's common-knowledge delta (563 → 589 occupied
  // bytes). The name plate (C10): under the stone and the plate after the summary, from the page's
  // left edge (it was in the column beside the stone). Re-pinned on purpose (lane D2, gate D0′'s
  // "said once"; renamed, it was "comments on the cells, the fee and common knowledge under the
  // sealed head, before the facts"): two lines, its fee line ("0.00064119 ckb to the miner.")
  // gone, since the fee stands in the status strip; the strip comes before it in the head.
  it('comments on the cells and common knowledge under the sealed head, after the status strip', async () => {
    const { container } = render(<TransactionDetailPage />);
    const first = await screen.findByText('1 cell became 1.');
    const comment = first.closest('.comment') as HTMLElement;
    expect(comment).not.toBeNull();
    expect(
      Array.from(comment.querySelectorAll('[data-comment-line]')).map((l) => l.textContent)
    ).toEqual(['1\u00a0cell became 1.', 'common knowledge grew by 26\u00a0bytes.']);
    // re-pinned with readability R4 (lane B7): the comment's paragraph stands on the measure, which
    // the Comment sets itself (the page gave the comment max-w-[60ch])
    expect(comment).toHaveClass('mt-4');
    expect(comment.className).not.toMatch(/max-w-/);
    expect(comment.querySelector('p')).toHaveClass('max-w-measure');
    expect(container.querySelectorAll('.comment')).toHaveLength(1);
    // under the seal and the plate, across both columns, after the summary, inside the header
    const { head, below } = expectNamePlate(
      container.querySelector('header') as HTMLElement,
      'Transaction'
    );
    expect(comment.parentElement).toBe(below);
    const summary = await screen.findByTestId('tx-summary');
    const strip = screen.getByRole('group', { name: 'Transaction status' });
    expect(Array.from(below!.children)).toEqual([strip, summary, comment]);
    expect(head.closest('header')).toContainElement(comment);
    // reading order: the identity (the hash and its copy button), the strip, the summary, the
    // comment
    expectBefore(screen.getByRole('button', { name: 'copy transaction hash' }), strip);
    expectBefore(strip, comment);
    // the counts are the cells heading's
    expect(screen.getByRole('heading', { name: 'Cells 1 in → 1 out' })).toBeInTheDocument();
  });

  // Re-pinned on purpose (lane D2, gate D0′; renamed, it was "keeps the status, time, block and
  // fee in a facts rail (TX-18)"): the same four facts, in the status strip under the id line;
  // the status word lowercase as the board writes it (it was "Committed"), the fee exact
  // (0.00064119 has no trailing zero to drop).
  it('keeps the status, time, block and fee in the status strip (TX-18)', async () => {
    render(<TransactionDetailPage />);
    const strip = await screen.findByRole('group', { name: 'Transaction status' });
    expect(strip).toHaveTextContent('committed');
    expect(strip).toHaveTextContent('27,369 confirmations');
    expect(strip).toHaveTextContent('2026-10-22 22:43:55 UTC+8');
    expect(within(strip).getByRole('link', { name: '#101' })).toBeInTheDocument();
    expect(strip).toHaveTextContent('0.00064119 CKB');
    expect(strip).toHaveTextContent(`paid by ${row('cjk9lz')}`); // R9 (re-pinned): 4…8
  });

  // Lane P1 follow-up (item 3; the link-look gate's participant-ref entry): every party the page
  // links states its look. In a line (the lede, the fee's payer, What moved) a party is a name
  // inside it, the inline look; in the balance changes a party is its row's key. None writes an ink
  // by hand.
  it('states the look of every party it links: inline in a line, the key of a balance row', async () => {
    render(<TransactionDetailPage />);
    const strip = await screen.findByRole('group', { name: 'Transaction status' });
    const INLINE = ['underline', 'decoration-[color:var(--c-ink-muted)]'];
    const parties = (root: HTMLElement) =>
      within(root)
        .getAllByRole('link')
        .filter((a) => a.getAttribute('href')!.includes('/address/'));
    const expectLook = (links: HTMLElement[], classes: string[]) => {
      expect(links.length).toBeGreaterThan(0);
      for (const link of links) {
        expect(link).toHaveClass(...classes);
        expect(link).not.toHaveClass('hover:text-jade');
      }
    };
    expectLook(parties(strip), INLINE);
    const summary = await screen.findByTestId('tx-summary');
    await waitFor(() => expect(parties(summary).length).toBeGreaterThan(0));
    expectLook(parties(summary), INLINE);
    expectLook(parties(await screen.findByTestId('tx-what-moved')), INLINE);
    expectLook(parties(screen.getByLabelText('Balance changes by participant')), [
      'text-text-bright',
    ]);
  });

  it('reads status, time, block and fee under the hash, before the balances and the cells', async () => {
    // Responsive review A#17: on a phone the facts rail came after the six cell cards.
    render(<TransactionDetailPage />);
    // re-pinned on purpose (lane D2): the facts are the status strip (they were the rail)
    const strip = await screen.findByRole('group', { name: 'Transaction status' });
    const table = await screen.findByRole('table', { name: 'Balance changes by participant' });
    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    const hash = screen.getByRole('button', { name: 'copy transaction hash' });
    const cells = screen.getByRole('heading', { name: 'Cells 1 in → 1 out' });
    expect(before(hash, strip)).toBe(true);
    expect(before(strip, table)).toBe(true);
    expect(before(strip, cells)).toBe(true);
    // re-pinned on purpose (lane D2, U-13): at every width the strip is in the head, under the
    // hash, and no column beside the tier-1 story holds facts (from lg the rail was the
    // right-hand column, lg:col-start-2 lg:row-start-1)
    expect(strip.closest('header')).not.toBeNull();
    expect(strip.className).not.toMatch(/col-start|row-start/);
  });

  it('names the collection and composition of moved objects, occupied in whole bytes, and a Net row (TX-12, TX-19)', async () => {
    render(<TransactionDetailPage />);
    const table = await screen.findByRole('table', { name: 'Balance changes by participant' });
    await waitFor(() => expect(table).toHaveTextContent('Nervape 3x 0x51c75a00…0096f2 departed'));
    expect(table.querySelectorAll('[data-composition="btc_ckb"]')).toHaveLength(2);
    // B4 item 2 (re-pinned on purpose): occupied bytes carry their unit (it was "−502", "+26")
    expect(table).toHaveTextContent('−502 B');
    expect(table).not.toHaveTextContent('502.00000000');
    expect(screen.getByTestId('tx-balance-net')).toHaveTextContent(
      'Net−0.00064119 CKB+26 Bthe fee, paid to the miner'
    );
  });
});

// Lane D2 (gate D0′, the transaction board; PA-13, U-13): the facts rail becomes the status strip
// under the id line — status, time, block and fee — and tier 1 takes the whole measure. A fact is
// said once: the fee stands in the strip and in the created sheet's balance row, so neither the
// lede nor the comment repeats it.
describe('Transaction board, the status strip (lane D2)', () => {
  it('states status, time, block and fee in one strip under the id line', async () => {
    const { container } = render(<TransactionDetailPage />);
    const strip = await screen.findByRole('group', { name: 'Transaction status' });
    expect(strip).toHaveAttribute('data-status-strip');
    const facts = factsOf(strip);
    expect(facts.map(([label]) => label)).toEqual(['status', 'time', 'block', 'fee']);
    expect(facts[0][1]).toBe('committed · 27,369 confirmations');
    // the site's one clock: the date-time, then its age (Utc8Age)
    expect(facts[1][1]).toMatch(/^2026-10-22 22:43:55 UTC\+8 · \S+ ago$/);
    expect(facts[2][1]).toBe('#101');
    // the fee exact (the board's form, its trailing zeros dropped), who paid it at the row size
    expect(facts[3][1]).toBe(`0.00064119 CKB · paid by ${row('cjk9lz')}`);
    // committed: the jade dot before the word
    const dot = strip.querySelector<HTMLElement>('[data-strip-dot]')!;
    expect(dot.style.backgroundColor).toBe('var(--c-jade)');
    // the block and the payer are links inside the strip's line
    expect(within(strip).getByRole('link', { name: '#101' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/101'
    );
    // under the id line: the first thing under the stone and the plate, then the summary and the
    // comment
    const { below } = expectNamePlate(
      container.querySelector('header') as HTMLElement,
      'Transaction'
    );
    const summary = await screen.findByTestId('tx-summary');
    const comment = container.querySelector('.comment') as HTMLElement;
    expect(Array.from(below!.children)).toEqual([strip, summary, comment]);
  });

  it('gives tier 1 the whole measure: no facts rail beside it (U-13)', async () => {
    const { container } = render(<TransactionDetailPage />);
    await screen.findByRole('group', { name: 'Transaction status' });
    expect(screen.queryByRole('complementary', { name: 'Transaction facts' })).toBeNull();
    const tier1 = container.querySelector('[data-tier="1"]') as HTMLElement;
    expect(tier1.querySelector('aside')).toBeNull();
    expect(tier1.className).not.toMatch(/280px|col-start|row-span/);
    for (const name of ['What moved', 'Balance changes', 'Cells 1 in → 1 out'])
      expect(
        (await screen.findByRole('heading', { name })).closest('[data-tier="1"] > *')!.className
      ).not.toMatch(/col-start/);
  });

  it('says the fee once: neither the lede nor the comment repeats it', async () => {
    const { container } = render(<TransactionDetailPage />);
    const summary = await screen.findByTestId('tx-summary');
    // before: "… to ckb1…. The sender paid a 0.00064119 CKB fee."
    expect(summary).toHaveTextContent(
      new RegExp(
        `^Nervape 3x 0x51c75a00…0096f2 moved from ${row('cjk9lz')} to ${row('shp3hr')}\\.$`
      )
    );
    const comment = container.querySelector('.comment') as HTMLElement;
    // before: three lines, the second "0.00064119 ckb to the miner."
    expect(
      Array.from(comment.querySelectorAll('[data-comment-line]')).map((l) => l.textContent)
    ).toEqual(['1 cell became 1.', 'common knowledge grew by 26 bytes.']);
    // the head states the fee once, in the strip
    const header = container.querySelector('header') as HTMLElement;
    expect(header.textContent!.match(/0\.00064119/g)).toHaveLength(1);
    expect(screen.getByRole('group', { name: 'Transaction status' })).toHaveTextContent(
      '0.00064119 CKB'
    );
    // and the created sheet closes on it
    expect(await screen.findByTestId('tx-fee')).toHaveTextContent(/^fee0\.00064119 CKB$/);
  });

  it('states a pool transaction’s status, its time in the pool and its fee, and no block', async () => {
    const tx = { ...committed(), status: 'pending', pendingSince: '2026-10-30T08:14:17Z' };
    for (const key of ['blockNumber', 'blockHash', 'index', 'timestamp', 'confirmations'] as const)
      delete (tx as Partial<TransactionDetail>)[key];
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx as TransactionDetail);
    render(<TransactionDetailPage />);
    const strip = await screen.findByRole('group', { name: 'Transaction status' });
    const facts = factsOf(strip);
    // a fact the page does not know is not passed: no block, no block time
    expect(facts.map(([label]) => label)).toEqual(['status', 'pending since', 'fee']);
    expect(facts[0][1]).toBe('pending');
    expect(facts[1][1]).toMatch(/^2026-10-30 16:14:17 UTC\+8 · /);
    expect(facts[2][1]).toBe(`0.00064119 CKB · paid by ${row('cjk9lz')}`);
    expect(strip.querySelector<HTMLElement>('[data-strip-dot]')!.style.backgroundColor).toBe(
      'var(--c-aqua)'
    );
  });
});

describe('Transaction board, tier 2', () => {
  it('lights a witness, its groups and their dep, and dims everything unrelated (TX-14)', async () => {
    render(<TransactionDetailPage />);
    const w0 = await screen.findByTestId('tx-witness-item-0');
    // the cell deps have resolved (PA-12 re-pinned this wait: the rows no longer say "supplies")
    await waitFor(() => expect(document.querySelector('#tx-dep-0')).not.toBeNull());
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
    // set back in the dim ink (R4, re-pinned on purpose: the tile was faded to 30 %)
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('quiet');
    fireEvent.mouseLeave(w0);
    expect(screen.getByTestId('tx-witness-item-1')).not.toHaveClass('opacity-25');
  });

  // Visual finish V5: a dep row's out point link rests as ink with the faint aqua underline (was
  // `text-info/80`); the global a:hover lights it jade.
  // Re-pinned on purpose (lane P1, the link-look gate: one owner of a link's look): a dep's out
  // point is its row's key, the key look's bright ink with no hand-written underline, no aqua and
  // no hover colour of its own (it was V5's bright ink over a faint aqua underline, by hand).
  it("rests each cell dep's out point link as ink, not aqua (V5)", async () => {
    render(<TransactionDetailPage />);
    const dep0 = await waitFor(() => {
      const el = document.querySelector('#tx-dep-0');
      if (!el) throw new Error('no dep row yet');
      return el as HTMLElement;
    });
    const deps = [dep0, ...Array.from(document.querySelectorAll('[id^="tx-dep-"]')).slice(1)];
    expect(deps.length).toBeGreaterThan(1);
    for (const dep of deps) {
      const outPoint = dep.querySelector(':scope > a') as HTMLElement | null;
      expect(outPoint, dep.id).not.toBeNull();
      expect(outPoint!.getAttribute('href')).toMatch(/\/cell\/0x[0-9a-z]+-\d+$/);
      expect(outPoint).toHaveClass('text-text-bright');
      expect(Array.from(outPoint!.classList)).not.toContainEqual(
        expect.stringMatching(/^(decoration-aqua|underline$|hover:text-|text-(aqua|interactive))/)
      );
      // The out point still wraps anywhere in its narrow column.
      expect(outPoint).toHaveClass('min-w-0', '[overflow-wrap:anywhere]');
    }
  });

  // Readability review 2026-11-18 (lane A): a dep group's member that no script of the
  // transaction names read "1,048,576 B no script here names": when its size was added (R2b-a,
  // 63fabc55) the member lost "it" and became a size with no noun. The board's words are "a
  // 70,320 B cell no script here names" (Main.dc.html, DEPS); a named member keeps "Default Lock
  // code, 52,048 B".
  it("says what a dep group's member no script here names is: a cell of its size", async () => {
    render(<TransactionDetailPage />);
    const dep0 = await waitFor(() => {
      const el = document.querySelector('#tx-dep-0');
      if (!el) throw new Error('no dep row yet');
      return el as HTMLElement;
    });
    const [named, unnamed] = Array.from(dep0.querySelectorAll('.pl-3 > span'));
    expect(named).toHaveTextContent(/^↳ 0xmember:0 · Default Lock code, 52,048 B$/);
    expect(unnamed).toHaveTextContent(/^↳ 0xother:1 · a 1,048,576 B cell no script here names$/);
  });

  it('says which script each cell dep supplies code for (TX-15)', async () => {
    render(<TransactionDetailPage />);
    const dep0 = await waitFor(() => {
      const el = document.querySelector('#tx-dep-0');
      if (!el) throw new Error('no dep row yet');
      return el as HTMLElement;
    });
    // PA-12 re-pinned the wording: one statement led by the dep kind (was "supplies code for X"
    // beside "for X").
    expect(dep0).toHaveTextContent('dep group for Default Lock');
    expect(dep0).toHaveTextContent('↳ 0xmember:0 · Default Lock code, 52,048 B');
    // Re-pinned (readability 2026-11-18): the member is a cell of its size, in the board's words
    // (was "1,048,576 B no script here names", a size with no noun).
    expect(dep0).toHaveTextContent('↳ 0xother:1 · a 1,048,576 B cell no script here names');
    expect(document.querySelector('#tx-dep-1')).toHaveTextContent('code for Spore · 76,752 B');
    const dep2 = document.querySelector('#tx-dep-2') as HTMLElement;
    expect(dep2).toHaveTextContent('code of Default Lock, not run here · 122,552 B');
    expect(dep2).not.toHaveTextContent('dep_group');
    fireEvent.mouseEnter(dep2);
    expect(
      document.querySelector(`[data-group-row="lock:attached:${OUTPUT_LOCK}"]`)
    ).toHaveAttribute('data-lit', 'true');
    expect(screen.getByTestId('tx-io-output-0')).toHaveAttribute('data-highlighted', 'true');
    const attached = document.querySelector(
      `[data-group-row="lock:attached:${OUTPUT_LOCK}"]`
    ) as HTMLElement;
    expect(attached).toHaveTextContent('code 0xd00a0222…2c47f7:0 · not run here');
    // In the code column the out point opens the code cell; the kind jumps to the dep row.
    expect(within(attached).getByRole('link', { name: '0xd00a0222…2c47f7:0' })).toHaveAttribute(
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
    expect(within(spore).getByRole('link', { name: '0x96b198fb…b69824:0' })).toHaveAttribute(
      'href',
      expect.stringMatching(new RegExp(`/cell/0x96b198fb${'00'.repeat(26)}b69824-0$`))
    );
    expect(within(spore).getByRole('link', { name: 'code' })).toHaveAttribute('href', '#tx-dep-1');
  });

  // PA-12 (UI review 2026-11-14): a dep row said what it supplies twice, "supplies code for Spore ·
  // 76,752 B" and, in a second column, "for Spore" (and "no script here runs it" beside "not run
  // here"), each wrapping in half the width of a phone. The row says it once at every width: the
  // out point, then one statement led by the kind of dep — what it supplies code for, or whose code
  // it holds and that it does not run here — and the code's size.
  it('says once what each cell dep is for, the dep kind leading the statement (PA-12)', async () => {
    render(<TransactionDetailPage />);
    const dep = (i: number) =>
      waitFor(() => {
        const el = document.querySelector(`#tx-dep-${i}`);
        if (!el) throw new Error(`no dep row ${i} yet`);
        return el as HTMLElement;
      });
    const count = (el: HTMLElement, word: string) => el.textContent!.split(word).length - 1;
    const code = await dep(1);
    expect(code).toHaveTextContent(/^0x96b198fb…b69824:0code for Spore · 76,752 B$/);
    expect(count(code, 'Spore')).toBe(1);
    const group = await dep(0);
    expect(group).toHaveTextContent(/^0x71a7ba8f…72e46c:0dep group for Default Lock↳/);
    // the group's statement, then its member's own line
    expect(count(group, 'Default Lock')).toBe(2);
    expect(count(group, ' for ')).toBe(1);
    const attached = await dep(2);
    expect(attached).toHaveTextContent(
      /^0xd00a0222…2c47f7:0code of Default Lock, not run here · 122,552 B$/
    );
    expect(count(attached, 'not run here')).toBe(1);
  });

  it('draws the proposal ruler and the exact capacity equation (TX-17, TX-19)', async () => {
    render(<TransactionDetailPage />);
    expect(
      await screen.findByRole('img', {
        name: 'Proposed in block 99, committed 2\u00a0blocks later in 101; the commit window is 2 to 10 blocks',
      })
    ).toBeInTheDocument();
    expect(await screen.findByTestId('tx-capacity-equation')).toHaveTextContent(
      '555 CKB in = 554.99935881 CKB out + 0.00064119 CKB fee'
    );
    // B4 item 2 (re-pinned on purpose): the common-knowledge change carries its unit
    // B4 fix round (re-pinned on purpose, tests MUST-2): the pair carries its unit once, after
    // it ("occupied 563 → 589 B"); it was bare beside the delta's "+26 B"
    expect(screen.getByText(/^occupied 563 → 589/)).toHaveTextContent(
      /^occupied 563 → 589 B · common knowledge \+26 B$/
    );
  });

  it('states the CoBuild actions a witness carries and for whose script (TX-36)', async () => {
    render(<TransactionDetailPage />);
    const w1 = await screen.findByTestId('tx-witness-item-1');
    await waitFor(() => expect(w1).toHaveTextContent('#1 · CoBuild message7 B · read by Spore'));
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
        // the owner of the withdrawn deposit: the API tags it DAO (TAG_DAO, bit 3), as the indexer
        // does the party whose cells hold the step (lane P1 follow-up: the fixture carried no tag)
        { ...feedSamples.daoWithdrawRequest.participants[1], ckbDelta: '-2605000000000', tags: 8 },
        { ...feedSamples.daoWithdrawRequest.participants[2], ckbDelta: '2622530633229' },
      ],
    });
    render(<TransactionDetailPage />);
    // Lane P1 follow-up (the review's standing cause, What moved and the lede on a DAO step): the
    // lede says what happened from the activity's own kind, a withdrawal with its compensation
    expect(await screen.findByTestId('tx-summary')).toHaveTextContent(
      `${row('r4p74p')} withdrew 26,050 CKB from the Nervos DAO, with 175.3 CKB of compensation.`
    );
    const inflow = await screen.findByTestId('tx-inflow');
    expect(inflow).toHaveAttribute('data-inflow', 'dao');
    expect(inflow).toHaveTextContent('Nervos DAO compensation175.30634165 CKB');
    // What moved (v2): the compensation enters the CKB flow first, from the left, in gold
    const first = screen
      .getByTestId('tx-what-moved')
      .querySelector('[data-flow="ckb"] [data-flow-side="from"] [data-flow-end]');
    expect(first).toHaveAttribute('data-flow-end', 'compensation');
    expect(first).toHaveTextContent('Nervos DAO compensation175.30634165 CKB');
    await screen.findByTestId('tx-capacity-equation');
    expect(screen.getByTestId('tx-capacity-equation')).toHaveTextContent(
      '26,050 CKB in + 175.30634165 CKB compensation = 26,225.30633229 CKB out + 0.00000936 CKB fee'
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

  // Lane P1 follow-up (review chain-b tx-dao-deposit: the deposit read as a payment): the lede
  // says the CKB went into a Nervos DAO deposit and who holds it (the party the API tags DAO), and
  // What moved writes that party's end as the deposit; the payer is not its holder here.
  it('says a deposit went into the Nervos DAO and who holds it, the figure too', async () => {
    const tx = committed();
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.daoWithdrawRequest,
      txHash: TX_HASH,
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'deposit',
          metadata: { capacity: '12200000000000' },
        },
      ],
      participants: [
        {
          ...feedSamples.daoWithdrawRequest.participants[2],
          ckbDelta: `-${BigInt('12200000000000') + BigInt(tx.fee)}`,
        },
        { ...feedSamples.daoWithdrawRequest.participants[1], ckbDelta: '12200000000000', tags: 8 },
      ],
    });
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('tx-summary')).toHaveTextContent(
      `122,000 CKB went into a Nervos DAO deposit held by ${row('r4p74p')}.`
    );
    const moved = await screen.findByTestId('tx-what-moved');
    expect(moved.querySelector('[data-flow-side="to"]')).toHaveTextContent('Nervos DAO deposit');
  });

  // Lane P1, third round (review p-logic-chain MUST 2): the common shape of a DAO step, from the
  // owner's own address. Its one party gives the fee and holds the DAO cell (the API tags it DAO);
  // for a deposit and a withdrawal request its CKB delta is −fee, so the flows are the fee alone,
  // yet the transaction took a step: the lede says the step and never "Only the fee moved", What
  // moved is drawn with the party's step on its end, and the balance table is drawn.
  describe('a Nervos DAO step from the owner’s own address', () => {
    const self = (
      action: string,
      metadata: Record<string, string>,
      ckbDelta: (fee: bigint) => string
    ) => {
      const tx = committed();
      vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
      vi.mocked(api.getTransactionActivity).mockResolvedValue({
        ...feedSamples.daoWithdrawRequest,
        txHash: TX_HASH,
        protocolActions: [{ protocol: 'dao', protocolName: 'Nervos DAO', action, metadata }],
        participants: [
          {
            ...feedSamples.daoWithdrawRequest.participants[1],
            ckbDelta: ckbDelta(BigInt(tx.fee)),
            tags: 8,
          },
        ],
      });
      return tx;
    };
    const expectStep = async (lede: string, step: string) => {
      render(<TransactionDetailPage />);
      const summary = await screen.findByTestId('tx-summary');
      expect(summary).toHaveTextContent(lede);
      expect(summary).not.toHaveTextContent('Only the fee moved');
      const moved = await screen.findByTestId('tx-what-moved');
      expect(moved).toHaveTextContent(step);
      expect(
        screen.getByRole('heading', { name: 'Balance changes' }).closest('section')
      ).toHaveTextContent(row('r4p74p'));
    };

    it('a deposit', async () => {
      self('deposit', { capacity: '12200000000000' }, (fee) => `-${fee}`);
      await expectStep(
        `122,000 CKB went into a Nervos DAO deposit held by ${row('r4p74p')}.`,
        'Nervos DAO deposit'
      );
    });

    it('a withdrawal request', async () => {
      self(
        'withdraw_request',
        { capacity: '2605000000000', depositBlock: '19695550' },
        (fee) => `-${fee}`
      );
      await expectStep(
        `${row('r4p74p')} asked to withdraw a Nervos DAO deposit of 26,050 CKB, made at #19,695,550.`,
        'Nervos DAO withdrawal request'
      );
    });

    it('a withdrawal', async () => {
      const tx = committed();
      tx.inputsCapacity = '2605000000000';
      tx.inputs![0].capacity = '2605000000000';
      tx.outputsCapacity = '2622530633229';
      tx.outputs![0].capacity = '2622530633229';
      tx.fee = '936';
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
          { ...feedSamples.daoWithdrawRequest.participants[1], ckbDelta: '17530633229', tags: 8 },
        ],
      });
      await expectStep(
        `${row('r4p74p')} withdrew 26,050 CKB from the Nervos DAO, with 175.3 CKB of compensation.`,
        'Nervos DAO withdrawal'
      );
    });
  });

  // Lane P1, third round (review p-logic-chain SHOULD 2): a batch in which one party deposits and
  // another withdraws. The activity tags both DAO and its actions name no party, so the lede says
  // each step without its party and names the parties whose cells took the steps once; the figure
  // says "a Nervos DAO step" on each, never both kinds on both.
  it('says the DAO steps of a batch without giving a party a step it did not take', async () => {
    const tx = committed();
    tx.inputsCapacity = '2605000000000';
    tx.inputs![0].capacity = '2605000000000';
    tx.outputsCapacity = '2622530633229';
    tx.outputs![0].capacity = '2622530633229';
    tx.fee = '936';
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.daoWithdrawRequest,
      txHash: TX_HASH,
      protocolActions: [
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'deposit',
          metadata: { capacity: '12200000000000' },
        },
        {
          protocol: 'dao',
          protocolName: 'Nervos DAO',
          action: 'withdraw_complete',
          metadata: { capacity: '2605000000000', compensation: '17530634165' },
        },
      ],
      participants: [
        { ...feedSamples.daoWithdrawRequest.participants[1], ckbDelta: '-12182469366771', tags: 8 },
        { ...feedSamples.daoWithdrawRequest.participants[2], ckbDelta: '12200000000000', tags: 8 },
      ],
    });
    render(<TransactionDetailPage />);
    const summary = await screen.findByTestId('tx-summary');
    expect(summary).toHaveTextContent(
      `122,000 CKB went into a Nervos DAO deposit. 26,050 CKB came out of the Nervos DAO, with 175.3 CKB of compensation. The parties whose cells took these steps: ${row('r4p74p')} and ${row('hxw6h7')}.`
    );
    const moved = await screen.findByTestId('tx-what-moved');
    expect(moved).toHaveTextContent('a Nervos DAO step');
    expect(moved).not.toHaveTextContent('Nervos DAO deposit');
  });

  // Lane P1, third round (review p-logic-chain SHOULD 4): a cellbase that pays no block's reward
  // said "no block before it is final yet", which is false for block 0 (it created the chain's
  // first cells) and for the first blocks after it (the block whose reward they would pay is the
  // genesis block, which earns none, or comes before the chain began). The API's `pays: null`
  // means just that; each says what is true, and the cells' note with it.
  describe('a cellbase that pays no block’s reward', () => {
    const cellbase = (blockNumber: number) => {
      const tx = committed();
      return {
        ...tx,
        blockNumber,
        isCellbase: true,
        fee: '0',
        feeRate: null,
        inputs: [
          {
            previousOutput: { txHash: `0x${'00'.repeat(32)}`, index: -1 },
            since: '0x0',
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
        outputs: [
          {
            ...tx.outputs![0],
            type: undefined,
            cellType: 'plain',
            capacity: '55961739927',
            commonKnowledgeSize: '61',
          },
        ],
        scriptGroups: [tx.scriptGroups![2]],
        witnessDetails: [],
        witnesses: [],
        cellbaseWitness: null,
        feePaidBy: [],
        pays: null,
      } as TransactionDetail;
    };

    it('says the genesis cellbase created the chain’s first cells', async () => {
      vi.mocked(api.getTransactionDetail).mockResolvedValue(cellbase(0));
      render(<TransactionDetailPage />);
      const summary = await screen.findByTestId('tx-summary');
      expect(summary).toHaveTextContent(
        /^The genesis block’s cellbase pays no block’s reward: it created the chain’s first cells, 559\.61 CKB\.$/
      );
      expect(summary).not.toHaveTextContent('final');
      expect(
        screen.getByText(
          'The genesis cellbase consumes no cell: it creates the chain’s first cells.'
        )
      ).toBeInTheDocument();
    });

    it('says block 5’s cellbase pays no reward because genesis earns none', async () => {
      vi.mocked(api.getTransactionDetail).mockResolvedValue(cellbase(5));
      render(<TransactionDetailPage />);
      const summary = await screen.findByTestId('tx-summary');
      expect(summary).toHaveTextContent(
        /^It pays no block’s reward: the block whose reward it would pay is the genesis block, which earns none, or would come before the chain began\.$/
      );
      expect(
        screen.getByText('A cellbase consumes no cell; this one pays no block’s reward.')
      ).toBeInTheDocument();
      expect(document.body).not.toHaveTextContent('now final');
    });
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
    expect(await screen.findByRole('heading', { level: 1, name: 'Cellbase' })).toBeInTheDocument();
    // Re-pinned on purpose (lane P1, the plan's P1 paragraph; review chain-a tx-cellbase: "the lede
    // lacks the payee, two heights 11 apart in one sentence"): the lede names who is paid
    // (`tx.outputs[].address`, as the cell tile names its owner), how much (the API's outputs
    // capacity, in the summary form, exact in its title and in the reward source) and which block
    // the reward is for. It was "The first transaction of block #101. It consumes no cell and pays
    // the reward of block #89, 559.61739927 CKB.": the block it is in is the strip's, and that it
    // consumes no cell the cells' note and the comment say.
    const payee = committed().outputs![0].address!;
    expect(screen.getByTestId('tx-summary')).toHaveTextContent(
      new RegExp(`^It pays ckb1…${payee.slice(-8)} 559\\.61 CKB, the reward for block #89\\.$`)
    );
    expect(
      within(screen.getByTestId('tx-summary')).getByRole('link', {
        name: `ckb1…${payee.slice(-8)}`,
      })
    ).toHaveAttribute('href', `/mainnet/address/${payee}`);
    expect(screen.getByTestId('tx-summary').querySelector('[data-ckb-amount]')).toHaveAttribute(
      'title',
      '559.61739927 CKB'
    );
    // R-6a: the paid block is linked by its canonical hash.
    expect(
      within(screen.getByTestId('tx-summary')).getByRole('link', { name: '#89' })
    ).toHaveAttribute('href', `/mainnet/blocks/0x${'89'.repeat(32)}`);
    // the block the cellbase is in is the status strip's link, said once
    expect(
      within(screen.getByTestId('tx-summary')).queryByRole('link', { name: '#101' })
    ).toBeNull();
    expect(
      within(screen.getByRole('group', { name: 'Transaction status' })).getByRole('link', {
        name: '#101',
      })
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
      '1\u00a0cell from the block reward; none consumed.',
      'common knowledge grew by 61\u00a0bytes.',
    ]);
    expect(comment).not.toHaveTextContent('to the miner');
    expect(screen.getByText(/never proposed/)).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId('tx-witness-item-0')).toHaveTextContent(
        // U-09 (re-pinned): a witness note in its row, the row size
        'miner lock Default Lock(0xabababab…ababab) · message “0.210.0 (860c9bb 2026-09-21)”'
      )
    );
    expect(screen.getByTestId('tx-groups-ran')).toHaveTextContent('a cellbase runs no script');
    expect(api.getTransactionActivity).not.toHaveBeenCalled();
    expect(api.getTransactionLifecycle).not.toHaveBeenCalled();
  });

  // Lane P1 follow-up (the lock-words gate's known writer 12, the cellbase witness's lock): the
  // miner lock is named by the one decision. A lock on the default lock's code hash by data is not
  // the default lock, and the registry's name for that code hash ("Default Lock") is not its name.
  it('writes a cellbase’s miner lock on the default lock’s code hash by data by its code hash', async () => {
    const message = `0x${Buffer.from('0.210.0 (860c9bb 2026-09-21) ').toString('hex')}0011223344556677`;
    const cellbase = {
      ...committed(),
      isCellbase: true,
      fee: '0',
      feeRate: null,
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
      cellbaseWitness: {
        lock: { codeHash: LOCK_CODE_HASH, hashType: 'data', args: `0x${'ab'.repeat(20)}` },
        message,
      },
      feePaidBy: [],
      pays: { number: 89, hash: '0x' + '89'.repeat(32) },
    } as TransactionDetail;
    vi.mocked(api.getTransactionDetail).mockResolvedValue(cellbase);
    vi.mocked(api.getBlock).mockResolvedValue({
      number: 101,
      minerMessage: message,
      minerMessageText: '0.210.0 (860c9bb 2026-09-21)',
    } as Awaited<ReturnType<typeof api.getBlock>>);
    render(<TransactionDetailPage />);
    await waitFor(() =>
      expect(screen.getByTestId('tx-witness-item-0')).toHaveTextContent(
        'miner lock 0x9bd7e06f…a3cce8 by data(0xabababab…ababab) · message “0.210.0 (860c9bb 2026-09-21)”'
      )
    );
    expect(screen.getByTestId('tx-witness-item-0')).not.toHaveTextContent('Default Lock');
  });

  it('says a pending transaction will run when committed, and still lists its witnesses (TX-37)', async () => {
    const tx = { ...committed(), status: 'pending', pendingSince: '2026-10-30T08:14:17Z' };
    for (const key of ['blockNumber', 'blockHash', 'index', 'timestamp', 'confirmations'] as const)
      delete (tx as Partial<TransactionDetail>)[key];
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx as TransactionDetail);
    render(<TransactionDetailPage />);
    expect(await screen.findByTestId('tx-groups-ran')).toHaveTextContent('will run when committed');
    expect(screen.queryByText(/0 ran/)).not.toBeInTheDocument();
    expect(screen.getByTestId('tx-witness-item-0')).toBeInTheDocument();
    expect(screen.getByText('Witnesses · 2')).toBeInTheDocument();
    // re-pinned on purpose (lane D2): the time it entered the pool is the strip's fact, its label
    // lowercase as the board writes them (it was the rail's "Pending since")
    expect(screen.getByText('pending since')).toBeInTheDocument();
    // No comment while it waits: its cells and occupied bytes are read once it is indexed (T4).
    // Under the stone and the plate (C10) stand the status strip and its summary, the sentence
    // that says so (re-pinned on purpose, lane D2: the summary stood alone there)
    expect(document.querySelector('.comment')).toBeNull();
    const below = document.querySelector('[data-sealed-below]') as HTMLElement;
    expect(below.children).toHaveLength(2);
    expect(below.firstElementChild).toBe(screen.getByRole('group', { name: 'Transaction status' }));
    expect(below.lastElementChild).toHaveTextContent(
      /^Activity and canonical cell structure are available once this transaction is committed and indexed\.$/
    );
    // C2: not in a block yet, so its stone is dashed; its cells are not drawn, so no seam seal.
    expect(screen.queryByTestId('tx-cells-frame')).toBeNull();
    expectTxSeals(
      document.body,
      { kind: 'tx', hash: TX_HASH, consumed: 1, created: 1 },
      'unpressed',
      false
    );
  });

  // Lane P1 (lane D3's note): the witnesses' heading stated "29 bytes", their sizes added in the
  // page; a stated total is the API's figure, and the API sends none, so the heading counts the
  // witnesses (the API's list) and each row states its own size.
  it('states no total of the witnesses’ sizes, which the page would have to add (P1)', async () => {
    render(<TransactionDetailPage />);
    const witnesses = await screen.findByRole('heading', { name: 'Witnesses · 2' });
    const head = witnesses.parentElement!;
    expect(head).not.toHaveTextContent(/byte/);
    // each row its own size, the API's
    expect(screen.getByTestId('tx-witness-item-0')).toHaveTextContent('22 B');
    expect(screen.getByTestId('tx-witness-item-1')).toHaveTextContent('7 B');
  });

  // Review 2026-11-03: the evidence tier reads the witnesses and the cell deps first, then the
  // script groups that read them.
  it('sets Witnesses and Cell deps above Script groups in the evidence tier', async () => {
    render(<TransactionDetailPage />);
    const witnesses = await screen.findByRole('heading', { name: 'Witnesses · 2' });
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
    await screen.findByTestId('tx-groups-ran');
    expect(screen.getAllByRole('heading', { name: /script groups/i })).toHaveLength(1);
    expect(screen.getByRole('heading', { name: 'Script groups' })).toBeInTheDocument();
    expect(screen.queryByText(/Capacity entering the pool/)).toBeNull();
  });
});

describe('Transaction board, R2b-a fields', () => {
  it('marks change outputs and reads decoded lock args (TX-22, TX-20)', async () => {
    render(<TransactionDetailPage />);
    const out = await screen.findByTestId('tx-io-output-0');
    // B4 fix round (re-pinned on purpose): the default lock is named beside no owner, so a
    // default-lock change output says 'change' alone (it was 'Default Lock · change')
    await waitFor(() => expect(out).toHaveTextContent('change'));
    expect(out).not.toHaveTextContent('Default Lock');
    expect(screen.getByTestId('tx-io-input-0')).not.toHaveTextContent('· change');
    expect(document.querySelector(`[data-group-row="lock:run:${INPUT_LOCK}"]`)).toHaveTextContent(
      'pubkey hash 0x11111111…111111'
    );
  });

  // B4 second fix round (review-rules-fixes-logic SHOULD 3, the coordinator's item 5): the
  // "Lock of output i" fact reads the one decision off the output's lock, as its tile does: a
  // lock on the default code hash by data is named by its code hash and hash type, never the
  // default lock's name.
  it('names an output lock on the default code hash by data as its tile does', async () => {
    const base = committed();
    const lock = { codeHash: LOCK_CODE_HASH, hashType: 'data', args: `0x${'22'.repeat(20)}` };
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...base,
      outputs: [
        {
          ...base.outputs![0],
          lock,
          address: ckbAddress({ ...lock, hashType: 'data' }),
          lockArgsDecoded: null,
          lockArgsDecodeError: 'args not read for this hash type',
        },
      ],
    } as TransactionDetail);
    render(<TransactionDetailPage />);
    const tile = await screen.findByTestId('tx-io-output-0');
    await waitFor(() => expect(tile).toHaveTextContent('lock 0x9bd7e06f…a3cce8 by data'));
    const fact = screen.getByTestId('tx-fact-output-lock-0');
    expect(fact).toHaveTextContent(/^Lock of output 0lock 0x9bd7e06f…a3cce8 by data, /);
    expect(fact).not.toHaveTextContent('Default Lock');
  });

  // Lane B4 third round (review-rules-fixes2 SHOULD 1, the coordinator's item 3, ruling D7): the
  // lock decision's refusals placed above their parts fail the part they are in, said in its place,
  // never the page: here a cell whose owner address spells another lock (JoyID) than its own.
  it('fails the Cells section and the output’s lock fact in place, the page drawn', async () => {
    const base = committed();
    const joy = seededAddress('joyout', JOYID_CODE_HASH);
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...base,
      outputs: [
        { ...base.outputs![0], address: joy, lockArgsDecoded: null, lockArgsDecodeError: 'odd' },
      ],
    } as TransactionDetail);
    render(<TransactionDetailPage />);
    const cells = await screen.findByRole('heading', { name: /^Cells/ });
    expect(cells.closest('section')).toHaveTextContent(
      new RegExp(`address ${joy} spells the lock`)
    );
    expect(screen.getByTestId('tx-fact-output-lock-0')).toHaveTextContent(
      new RegExp(`address ${joy} spells the lock`)
    );
    expect(screen.getByRole('heading', { name: 'Balance changes' })).toBeInTheDocument();
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
    await screen.findByTestId('tx-io-output-0');
    // A misfit lock says why its args were not decoded, quietly, in place of the fields.
    await waitFor(() =>
      expect(document.querySelector(`[data-group-row="lock:run:${INPUT_LOCK}"]`)).toHaveTextContent(
        `args not decoded: ${joyidReason}`
      )
    );
    expect(
      document.querySelector(`[data-group-row="lock:attached:${OUTPUT_LOCK}"]`)
    ).toHaveTextContent('args 0x07 + hash 0x31da0000…005d74');
    const owner = screen.getByTestId('tx-fact-output-lock-0');
    expect(owner).toHaveTextContent('New owner of the object in output 0');
    expect(owner).toHaveTextContent('args 0x07 + hash 0x31da0000…005d74');
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
    const w1 = await screen.findByTestId('tx-witness-item-1');
    await waitFor(() =>
      expect(w1).toHaveTextContent(
        'message: 2 actions for Spore: MintProxy, Spore (script info 0x45454545…454545)'
      )
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
    // re-pinned on purpose (lane D2, gate D0′'s "said once"): the fee sentence ("ckb1…xxxxxxxx
    // paid a 0.00064119 CKB fee.") is gone; the payer and the fee stand in the status strip
    const summary = await screen.findByTestId('tx-summary');
    // re-pinned on purpose (lane P1, the plan: "a transaction that moves only its fee says so in
    // one line"; review chain-b tx-old-sample, its fee seven times above the fold): its one party
    // lost only the fee, so the lede says so after the scripts it ran (it ended at "Info.")
    expect(summary).toHaveTextContent(
      new RegExp(
        `^It ran \\.bit Time Index State and \\.bit Time Info\\. Only the fee moved: ${row('cjk9lz')} kept the rest of its CKB\\.$`
      )
    );
    expect(screen.queryByTestId('tx-what-moved')).toBeNull();
    expect(screen.getByRole('group', { name: 'Transaction status' })).toHaveTextContent(
      `0.00064119 CKB · paid by ${row('cjk9lz')}`
    );
  });

  // Lane P1 (the plan's P1 paragraph; review chain-a tx-transfer, chain-b tx-pool: "the lede lacks
  // the amount"): the summary states how much moved, from the flows the figure draws, in the
  // summary form, its exact value in its title and in the balance table.
  it('states how much CKB moved, cut to the summary form, its exact value in its title (P1)', async () => {
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.objectTransfer,
      txHash: TX_HASH,
      typeCalls: [],
      lockCalls: [],
      participants: [
        {
          ...feedSamples.objectTransfer.participants[0],
          ckbDelta: '-66959187575',
          usedDelta: '0',
          itemDeltas: [],
        },
        {
          ...feedSamples.objectTransfer.participants[1],
          ckbDelta: '66959123456',
          usedDelta: '0',
          itemDeltas: [],
        },
      ],
    });
    render(<TransactionDetailPage />);
    const summary = await screen.findByTestId('tx-summary');
    // what reached the receiver, 669.59123456 CKB, cut to two places; the fee is the strip's
    expect(summary).toHaveTextContent(
      new RegExp(`^669\\.59 CKB moved from ${row('cjk9lz')} to ${row('shp3hr')}\\.$`)
    );
    const amount = summary.querySelector('[data-ckb-amount]')!;
    expect(amount).toHaveAttribute('data-form', 'summary');
    expect(amount).toHaveAttribute('title', '669.59123456 CKB');
    // the exact value stands in the balance table, the receiver's row
    const balance = (await screen.findByRole('heading', { name: 'Balance changes' })).closest(
      'section'
    )!;
    expect(balance).toHaveTextContent('+669.59123456');
  });

  it('states a token’s amount in its own units, in the summary form (P1)', async () => {
    vi.mocked(api.getTransactionDetail).mockResolvedValue({
      ...committed(),
      fee: '8888',
      outputsCapacity: '55499991112',
    });
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.tokenTransfer,
      txHash: TX_HASH,
    });
    render(<TransactionDetailPage />);
    const summary = await screen.findByTestId('tx-summary');
    // 36,583.46853885 Seal, cut to whole tokens from 1,000 up
    expect(summary).toHaveTextContent(
      new RegExp(`^36,583 Seal moved from ${row('j3pkeh')} to ${row('3fzxu7')}\\.$`)
    );
    expect(within(summary).getByRole('link', { name: 'Seal' })).toBeInTheDocument();
  });

  // Lane P1 (the plan: "a transaction that moves only its fee says so in one line"; review chain-b
  // tx-old-sample: the fee printed seven times above the fold, the figure and the table restating
  // it): the lede says it once, and the figure and the one-party table, which said nothing else,
  // are not drawn.
  it('says in one line that only the fee moved, and draws no figure or table that restates it (P1)', async () => {
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.objectTransfer,
      txHash: TX_HASH,
      typeCalls: [],
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
    const summary = await screen.findByTestId('tx-summary');
    expect(summary).toHaveTextContent(
      new RegExp(`^Only the fee moved: ${row('cjk9lz')} kept the rest of its CKB\\.$`)
    );
    await screen.findByRole('heading', { name: 'Cells 1 in → 1 out' });
    expect(screen.queryByRole('heading', { name: 'What moved' })).toBeNull();
    expect(screen.queryByTestId('tx-what-moved')).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Balance changes' })).toBeNull();
  });

  it('keeps the balance table when several locks shared the fee, and says "their" (P1)', async () => {
    vi.mocked(api.getTransactionActivity).mockResolvedValue({
      ...feedSamples.objectTransfer,
      txHash: TX_HASH,
      typeCalls: [],
      lockCalls: [],
      participants: [
        {
          ...feedSamples.objectTransfer.participants[0],
          ckbDelta: '-60000',
          usedDelta: '0',
          itemDeltas: [],
        },
        {
          ...feedSamples.objectTransfer.participants[1],
          ckbDelta: '-4119',
          usedDelta: '0',
          itemDeltas: [],
        },
      ],
    });
    render(<TransactionDetailPage />);
    const summary = await screen.findByTestId('tx-summary');
    expect(summary).toHaveTextContent(
      new RegExp(
        `^Only the fee moved: ${row('cjk9lz')} and ${row('shp3hr')} kept the rest of their CKB\\.$`
      )
    );
    expect(await screen.findByRole('heading', { name: 'Balance changes' })).toBeInTheDocument();
    expect(screen.queryByTestId('tx-what-moved')).toBeNull();
  });
});

describe('What moved (the asset flows, review 2026-11-03 v2)', () => {
  it('draws the object flow and the CKB flow, from the left to the right, before the ledger', async () => {
    render(<TransactionDetailPage />);
    const frame = await screen.findByTestId('tx-what-moved');
    const groups = within(frame).getAllByRole('group');
    expect(groups.map((g) => g.getAttribute('data-flow'))).toEqual([
      `object:0x51c75a${'00'.repeat(26)}96f2`,
      'ckb',
    ]);
    const [object, ckb] = groups;
    // U-09 (re-pinned): the ribbon's parties at the row size; R9 (re-pinned): 4…8
    expect(object.querySelector('[data-flow-side="from"]')).toHaveTextContent(row('cjk9lz'));
    expect(object.querySelector('[data-flow-side="to"]')).toHaveTextContent(row('shp3hr'));
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
    // the giver's cell: its address and the lock it spells (B4 second fix round: one cell's)
    const giverAddress = feedSamples.objectTransfer.participants[0].address!;
    tx.inputs![0].address = giverAddress;
    tx.inputs![0].lock = addressScript(giverAddress);
    vi.mocked(api.getTransactionDetail).mockResolvedValue(tx);
    render(<TransactionDetailPage />);
    const frame = await screen.findByTestId('tx-what-moved');
    const [object] = within(frame).getAllByRole('group');
    const giver = object.querySelector('[data-flow-side="from"] [data-flow-end="party"]')!;
    fireEvent.mouseEnter(giver);
    expect(screen.getByTestId('tx-io-input-0')).toHaveAttribute('data-highlighted', 'true');
    // set back in the dim ink (R4, re-pinned on purpose: the tile was faded to 30 %)
    expect(screen.getByTestId('tx-io-output-0')).toHaveClass('quiet');
    fireEvent.mouseLeave(giver);
    expect(screen.getByTestId('tx-io-output-0')).not.toHaveClass('quiet');
  });
});
