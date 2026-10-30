import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { CellTransactions } from '@/components/cell/cell-transactions';
import type { Cell, TransactionDetail } from '@/lib/api';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';

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
const OUT0_LOCK = `0x43b292${'00'.repeat(26)}895d`;
const cell = sporeCell as unknown as Cell;
const SPORE = cell.type!.codeHash;
const SHADOW = cell.lock!.codeHash;
const defaultLock = { codeHash: SECP, hashType: 'type', args: `0x${'01'.repeat(20)}` };
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
const input = (i: number, extra: object) => ({
  previousOutput: { txHash: `0x${String(i).repeat(64)}`, index: 0 },
  capacity: '19500000000',
  commonKnowledgeSize: '195',
  address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqcjk9lz${i}`,
  lock: defaultLock,
  display: null,
  ...extra,
});
const output = (extra: object) => ({
  capacity: '32000000000',
  commonKnowledgeSize: '320',
  address: 'ckb1qp3kr4shp3hr',
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
  ],
  witnessDetails: [
    { index: 0, size: 85, kind: 'witness_args', fields: [] },
    { index: 1, size: 357, kind: 'ckbfs', fields: [] },
    { index: 2, size: 339, kind: 'cobuild', fields: [] },
  ],
} as unknown as TransactionDetail;

function view(target: Cell, props: { lockName?: string | null; typeName?: string | null } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CellTransactions cell={target} {...props} />
    </QueryClientProvider>
  );
}

describe('Its transactions against the Cell board', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getTransactionDetail.mockResolvedValue(sporeTx);
    getTransactionCellDeps.mockResolvedValue([]);
    lookupScripts.mockResolvedValue({
      [SECP]: { name: 'Default Lock' },
      [SHADOW]: { name: 'Nervape Shadow Lock' },
      [SPORE]: { name: 'Spore' },
      [CKBFS]: { name: 'CKBFS' },
    });
  });

  it('draws continuity dashes and names locks on the tiles (SC-C7)', async () => {
    view(cell, { lockName: 'Nervape Shadow Lock', typeName: 'Spore' });
    const structure = await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    expect(lookupScripts).toHaveBeenCalledWith(
      expect.arrayContaining([SECP, SHADOW, SPORE, CKBFS]),
      cell.txHash
    );
    const continuity = await within(structure).findByRole('list', { name: 'Cell continuity' });
    expect(continuity).toHaveTextContent('in:2 → out:1 · same object continues');
    expect(continuity).toHaveTextContent('in:0 → out:0 · same file cell continues');
    expect(structure.querySelector('[data-continuity-kind="objects"]')).not.toBeNull();
    expect(structure.querySelector('[data-continuity-kind="file"]')).not.toBeNull();
    const out1 = structure.querySelector('[data-cell-id="out:1"]') as HTMLElement;
    await within(out1).findByText('Nervape Shadow Lock');
    expect(
      within(structure.querySelector('[data-cell-id="in:1"]') as HTMLElement).getByText(
        'Default Lock'
      )
    ).toBeInTheDocument();
  });

  it('carries the live cell past its transaction into a dashed "Not spent yet" box (SC-C7)', async () => {
    view(cell, { lockName: 'Nervape Shadow Lock', typeName: 'Spore' });
    const structure = await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    expect(
      structure.querySelector('[data-onward-bridge][data-onward-from="out:1"]')
    ).not.toBeNull();
    const box = screen.getByTestId('not-spent-yet');
    expect(box).toHaveTextContent('Not spent yet');
    expect(box).toHaveTextContent('3 d 10 h · as of #20,834,844');
    expect(box).toHaveTextContent('The transaction that spends it will run:');
    expect(box).toHaveTextContent('lock groupNervape Shadow Lock');
    expect(box).toHaveTextContent('type groupSpore');
    expect(box).toHaveTextContent('0x5b20ec7d…724dc2:0');
  });

  it('labels groups by script name once, and wraps prose between words (SC-C8)', async () => {
    view(cell);
    const structure = await screen.findByLabelText(`Transaction ${cell.txHash} structure`);
    const groups = [...structure.querySelectorAll<HTMLElement>('[data-group-id]')];
    expect(groups).toHaveLength(4);
    await within(groups[2]).findByRole('link', { name: 'Spore' });
    expect(groups[2].textContent).toMatch(/^typeSporeexecuted/);
    expect(groups[0].textContent).not.toMatch(/locklock|typetype/);
    expect(groups[0].textContent).not.toContain(SECP.slice(0, 16));
    const outputOnly = within(groups[3]).getByText(
      'Output-only group: runs when these cells are spent'
    );
    expect(outputOnly.closest('.break-normal')).not.toBeNull();
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
      scriptGroups: [
        ...sporeTx.scriptGroups!.slice(0, 1),
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
      '#b5c77a'
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
    const groups = await screen.findByRole('heading', { name: 'Script groups' });
    const box = screen.getByTestId('not-spent-yet');
    expect(box.compareDocumentPosition(groups) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const onward = screen.getByText('this cell, onward: not spent yet');
    expect(onward.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
