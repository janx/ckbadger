import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { CellScriptGroups, CellTransactions } from '@/components/cell/cell-transactions';
import type { Cell, TransactionDetail } from '@/lib/api';
import { scriptLookupKey } from '@/lib/script-pair';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';
import { canvasLines, seamLines } from '@/__tests__/utils/tx-lines';

const getTransactionDetail = vi.fn();
const getTransactionCellDeps = vi.fn();
const lookupScripts = vi.fn();
vi.mock('@/lib/api', () => ({
  api: {
    getTransactionDetail: (...args: unknown[]) => getTransactionDetail(...args),
    getTransactionCellDeps: (...args: unknown[]) => getTransactionCellDeps(...args),
    lookupScripts: (...args: unknown[]) => lookupScripts(...args),
  },
}));

const TYPE_ID = '0x00000000000000000000000000000000000000000000000000545950455f4944';
const SECP = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const CKBFS = `0x${'31'.repeat(32)}`;
// 32 bytes (B4 second fix round: it was 31, which the address encoder refuses)
const OUT0_LOCK = `0x43b292${'00'.repeat(27)}895d`;
const cell = sporeCell as unknown as Cell;
const SPORE = cell.type!.codeHash;
const SHADOW = cell.lock!.codeHash;
const defaultLock = { codeHash: SECP, hashType: 'type' as const, args: `0x${'01'.repeat(20)}` };
const fileDisplay = {
  kind: 'file',
  standard: 'ckbfs',
  id: `0x${'f1'.repeat(32)}`,
  name: '3X0125NM…LOGS',
  collectionId: null,
  collectionName: null,
  symbol: null,
  decimals: null,
  amount: null,
  compositionTier: null,
};
const sporeDisplay = {
  ...fileDisplay,
  kind: 'object',
  standard: 'spore',
  id: cell.type!.args,
  name: null,
  collectionName: 'Nervape 3x',
  compositionTier: 'btc_ckb',
};
/**
 * A cell owned at the real address of its own lock (B4 second fix round: a tile refuses an address
 * that spells another lock; these were not addresses, and a cell given another lock kept one).
 */
const owned = <T extends { lock: { codeHash: string; hashType: string; args: string } }>(
  cell: T
) => ({
  ...cell,
  address: ckbAddress(cell.lock as { codeHash: string; hashType: 'type'; args: string }),
});
const input = (i: number, extra: object) =>
  owned({
    previousOutput: { txHash: `0x${String(i).repeat(64)}`, index: 0 },
    capacity: '19500000000',
    commonKnowledgeSize: '195',
    lock: defaultLock,
    display: null,
    ...extra,
  });
const output = (extra: object) =>
  owned({
    capacity: '32000000000',
    commonKnowledgeSize: '320',
    lock: defaultLock,
    status: 'live',
    display: null,
    ...extra,
  });
// The Spore transfer that created the board's cell (0xbe1b9761…): 3 cells consumed → 3 created.
const sporeTx = {
  hash: cell.txHash,
  status: 'committed',
  fee: '2000',
  isCellbase: false,
  timestamp: '2026-10-22T14:43:55.651+00:00',
  inputsCapacity: '4266152741621',
  // the API's sum of the three outputs below
  outputsCapacity: '96000000000',
  inputs: [
    input(0, { type: { codeHash: CKBFS, hashType: 'data1', args: '0x' }, display: fileDisplay }),
    input(1, {}),
    input(2, { type: cell.type, display: sporeDisplay }),
  ],
  outputs: [
    output({
      lock: { codeHash: OUT0_LOCK, hashType: 'type', args: '0x' },
      type: { codeHash: CKBFS, hashType: 'data1', args: '0x' },
      display: fileDisplay,
    }),
    output({ lock: cell.lock, type: cell.type, display: sporeDisplay }),
    output({}),
  ],
  scriptGroups: [
    {
      scriptHash: `0x${'a1'.repeat(32)}`,
      kind: 'lock',
      ran: true,
      inputIndices: [0, 1, 2],
      outputIndices: [],
      witnessIndex: 0,
      script: defaultLock,
      cycles: '1500000',
    },
    {
      scriptHash: `0x${'a2'.repeat(32)}`,
      kind: 'type',
      ran: true,
      inputIndices: [0],
      outputIndices: [0],
      witnessIndex: 1,
      script: { codeHash: CKBFS, hashType: 'data1', args: '0x' },
      cycles: '200000',
    },
    {
      scriptHash: `0x${'a3'.repeat(32)}`,
      kind: 'type',
      ran: true,
      inputIndices: [2],
      outputIndices: [1],
      witnessIndex: 2,
      script: cell.type,
      cycles: '300000',
    },
    {
      scriptHash: `0x${'a4'.repeat(32)}`,
      kind: 'lock',
      ran: false,
      inputIndices: [],
      outputIndices: [1],
      witnessIndex: null,
      script: cell.lock,
      cycles: null,
    },
    // Every output's lock is a group, the ones that do not run here too (the API lists them all):
    // output 0's, and output 2's — the inputs' Default Lock again, a second group of that hash.
    {
      scriptHash: `0x${'a5'.repeat(32)}`,
      kind: 'lock',
      ran: false,
      inputIndices: [],
      outputIndices: [0],
      witnessIndex: null,
      script: { codeHash: OUT0_LOCK, hashType: 'type', args: '0x' },
      cycles: null,
    },
    {
      scriptHash: `0x${'a1'.repeat(32)}`,
      kind: 'lock',
      ran: false,
      inputIndices: [],
      outputIndices: [2],
      witnessIndex: null,
      script: defaultLock,
      cycles: null,
    },
  ],
  witnessDetails: [
    { index: 0, size: 85, kind: 'witness_args', fields: [] },
    { index: 1, size: 357, kind: 'ckbfs', fields: [] },
    { index: 2, size: 339, kind: 'cobuild', fields: [] },
  ],
} as unknown as TransactionDetail;

/**
 * The cell's transactions as the page draws them (lane D3, re-pinned on purpose: it was the one
 * section, the groups inside each transaction's structure): the embed in tier 1, then the script
 * groups of the developer tier, in the transaction page's rows.
 */
function view(target: Cell, props: { lockName?: string | null; typeName?: string | null } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CellTransactions cell={target} {...props} />
      <CellScriptGroups cell={target} />
    </QueryClientProvider>
  );
}

describe('Its transactions against the Cell board', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getTransactionDetail.mockResolvedValue(sporeTx);
    getTransactionCellDeps.mockResolvedValue([]);
    // the registry's names, each by its script's pair (lane rd/api-lock-names)
    lookupScripts.mockResolvedValue({
      [scriptLookupKey(SECP, 'type')]: { name: 'Default Lock' },
      [scriptLookupKey(SHADOW, cell.lock!.hashType)]: { name: 'Nervape Shadow Lock' },
      [scriptLookupKey(SPORE, cell.type!.hashType)]: { name: 'Spore' },
      [scriptLookupKey(CKBFS, 'data1')]: { name: 'CKBFS' },
    });
  });

  it('draws continuity dashes and names locks on the tiles (SC-C7)', async () => {
    view(cell, { lockName: 'Nervape Shadow Lock', typeName: 'Spore' });
    const structure = await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    expect(lookupScripts).toHaveBeenCalledWith(
      expect.arrayContaining([
        { codeHash: SECP, hashType: 'type' },
        { codeHash: SHADOW, hashType: cell.lock!.hashType },
        { codeHash: SPORE, hashType: cell.type!.hashType },
        { codeHash: CKBFS, hashType: 'data1' },
      ]),
      cell.txHash
    );
    const continuity = await within(structure).findByRole('list', { name: 'Cell continuity' });
    expect(continuity).toHaveTextContent('in:2 → out:1 · same object continues');
    expect(continuity).toHaveTextContent('in:0 → out:0 · same file cell continues');
    // Re-pinned on purpose (lane D3, the board's embed): the embed draws this cell (out:1) and its
    // continuity partner (in:2), so the link between them is drawn; the file cell's link joins two
    // cells the embed counts, not draws (in:0 → out:0), and stays in the list above (both links
    // were drawn, every cell being drawn)
    expect(structure.querySelector('[data-continuity-kind="objects"]')).not.toBeNull();
    expect(structure.querySelector('[data-continuity-kind="file"]')).toBeNull();
    expect(
      Array.from(structure.querySelectorAll<HTMLElement>('[data-cell-id]')).map(
        (t) => t.dataset.cellId
      )
    ).toEqual(['in:2', 'out:1']);
    // C11 round 3: the link is the only line between the sheets (no row-aligned connector
    // suggests that input N became output N)
    expect(seamLines(structure)).toHaveLength(1);
    // and beyond the created sheet, the onward bridge that carries the board's live cell on
    expect(canvasLines(structure)).toEqual([
      ...seamLines(structure),
      ...Array.from(structure.querySelectorAll('[data-onward-bridge] path')),
    ]);
    expect(structure.querySelectorAll('[data-onward-bridge] path')).toHaveLength(1);
    const out1 = structure.querySelector('[data-cell-id="out:1"]') as HTMLElement;
    await within(out1).findByText('Nervape Shadow Lock');
    // B4 fix round (re-pinned on purpose): a tile names its owner's lock only when it is not the
    // default lock, as every row does (it wrote "Default Lock" beside every default-lock owner).
    // Re-pinned on purpose (lane D3): read on the drawn partner, in:2 (in:1 is counted, not drawn)
    expect(
      within(structure.querySelector('[data-cell-id="in:2"]') as HTMLElement).queryByText(
        'Default Lock'
      )
    ).toBeNull();
  });

  it("links each executed group's cell dep out point to its cell page", async () => {
    const depTx = `0x${'dd'.repeat(32)}`;
    getTransactionCellDeps.mockResolvedValue([
      {
        outPointTxHash: depTx,
        outPointIndex: 1,
        depType: 'dep_group',
        scriptHashes: [`0x${'a1'.repeat(32)}`],
        outputLockScriptHashes: [],
        resolvedCells: [],
      },
    ]);
    view(cell, { lockName: 'Nervape Shadow Lock', typeName: 'Spore' });
    await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    // U-16 (re-pinned): the dep is a reference, written as the transaction page writes it — its
    // kind, then the out point shortened (was the full out point and "(dep_group)")
    const short = `${depTx.slice(0, 10)}…${depTx.slice(-6)}:1`;
    const dep = await screen.findByRole('link', { name: short });
    expect(dep).toHaveAttribute('href', expect.stringMatching(new RegExp(`/cell/${depTx}-1$`)));
    expect(dep.parentElement).toHaveTextContent(`dep group ${short}`);
  });

  it('carries the live cell past its transaction into a dashed "Not spent yet" box (SC-C7)', async () => {
    view(cell, { lockName: 'Nervape Shadow Lock', typeName: 'Spore' });
    const structure = await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    expect(
      structure.querySelector('[data-onward-bridge][data-onward-from="out:1"]')
    ).not.toBeNull();
    const box = screen.getByTestId('not-spent-yet');
    expect(box).toHaveTextContent('Not spent yet');
    expect(box).toHaveTextContent('3d 10h · as of #20,834,844');
    // Lane L: the block it is measured at is a link.
    expect(within(box).getByRole('link', { name: '#20,834,844' })).toHaveAttribute(
      'href',
      expect.stringMatching(/\/blocks\/20834844$/)
    );
    expect(box).toHaveTextContent('The transaction that spends it will run:');
    expect(box).toHaveTextContent('lock groupNervape Shadow Lock');
    expect(box).toHaveTextContent('type groupSpore');
    expect(box).toHaveTextContent('0x5b20ec7d…724dc2:0');
  });

  it('labels groups by script name once, and wraps prose between words (SC-C8)', async () => {
    view(cell);
    // Re-pinned on purpose (lane D3, PA-24): the groups are the developer tier's rows, in the
    // transaction page's words ("typeSporeexecuted" and "Output-only group: runs when these cells
    // are spent" were the structure's own list's)
    const groups = await waitFor(() => {
      const rows = document.querySelectorAll<HTMLElement>('[data-group-row]');
      // the four of the board, then the two output locks that do not run here
      expect(rows).toHaveLength(6);
      return Array.from(rows);
    });
    await within(groups[2]).findByRole('link', { name: 'Spore' });
    expect(groups[2].textContent).toMatch(/^typeSpore/);
    // it ran here: its cycles, measured
    expect(groups[2]).toHaveTextContent('300,000 cycles');
    expect(groups[0].textContent).not.toMatch(/locklock|typetype/);
    expect(groups[0].textContent).not.toContain(SECP.slice(0, 16));
    expect(groups[3]).toHaveTextContent('attached, runs when spent');
    // prose wraps between words: no cell of a row breaks a word
    for (const part of Array.from(groups[3].querySelectorAll('[role="cell"]')))
      expect(part.className).not.toMatch(/break-all/);
  });

  it('follows a code cell through its Type ID and says it is not replaced yet (SC-K5)', async () => {
    const codeCell = {
      ...cell,
      type: { codeHash: TYPE_ID, hashType: 'type', args: `0x${'77'.repeat(32)}` },
      currentSpendRequirements: [
        {
          kind: 'lock',
          referenceHash: SECP,
          hashType: 'type',
          resolution: 'resolved',
          versionHashes: [],
          candidateCells: [],
          historicalCells: [],
        },
        {
          kind: 'type',
          referenceHash: TYPE_ID,
          hashType: 'type',
          resolution: 'builtin',
          versionHashes: [],
          candidateCells: [],
          historicalCells: [],
        },
      ],
    } as unknown as Cell;
    getTransactionDetail.mockResolvedValue({
      ...sporeTx,
      // the Spore type group (input 2 → output 1) replaced by the Type ID one; the rest as before
      scriptGroups: [
        ...sporeTx.scriptGroups!.slice(0, 2),
        ...sporeTx.scriptGroups!.slice(3),
        {
          scriptHash: `0x${'b1'.repeat(32)}`,
          kind: 'type',
          ran: true,
          inputIndices: [2],
          outputIndices: [1],
          witnessIndex: null,
          script: codeCell.type,
          cycles: '0',
        },
      ],
    });
    view(codeCell, { lockName: 'Multisig', typeName: 'Type ID' });
    const structure = await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    expect(
      await within(structure).findByRole('list', { name: 'Cell continuity' })
    ).toHaveTextContent('in:2 → out:1 · same type id continues');
    expect(structure.querySelector('[data-continuity-kind="type-id"]')).toHaveAttribute(
      'stroke',
      'var(--c-cell-type)'
    );
    const box = screen.getByTestId('not-spent-yet');
    expect(box).toHaveTextContent('Not replaced yet');
    expect(box).toHaveTextContent('The transaction that replaces or destroys it will run:');
    expect(box).toHaveTextContent('Built-in Type ID; no code-cell dep');
  });
});

describe('Its transactions · visual pass', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getTransactionDetail.mockResolvedValue(sporeTx);
    getTransactionCellDeps.mockResolvedValue([]);
    lookupScripts.mockResolvedValue({});
  });

  it('puts the "Not spent yet" box right after the onward bridge, before the groups table', async () => {
    view(cell, { lockName: 'Nervape Shadow Lock', typeName: 'Spore' });
    // Lane U follow-up: the list is named by its transaction.
    const groups = await screen.findByRole('heading', {
      name: 'Script groups of the transaction that created it',
    });
    // re-pinned on purpose (lane D3): the groups' heading is the developer tier's, drawn before
    // the transaction is read, so the box is waited for (it was found with the heading)
    const box = await screen.findByTestId('not-spent-yet');
    expect(box.compareDocumentPosition(groups) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const onward = screen.getByText('this cell, onward: not spent yet');
    expect(onward.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
