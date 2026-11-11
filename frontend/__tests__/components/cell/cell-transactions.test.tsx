import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { CellTransactions } from '@/components/cell/cell-transactions';
import type { Cell, TransactionDetail } from '@/lib/api';
import liveFixture from '@/__tests__/fixtures/d5/cell-live.json';
import spentFixture from '@/__tests__/fixtures/d5/cell-spent.json';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';

const getTransactionDetail = vi.fn();
const getTransactionCellDeps = vi.fn();
vi.mock('@/lib/api', () => ({
  api: {
    getTransactionDetail: (...args: unknown[]) => getTransactionDetail(...args),
    getTransactionCellDeps: (...args: unknown[]) => getTransactionCellDeps(...args),
    lookupScripts: async () => ({}),
  },
}));
vi.mock('@/components/transaction/tx-structure', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/components/transaction/tx-structure')>();
  return {
    TxStructure: (props: React.ComponentProps<typeof actual.TxStructure>) => (
      <div data-testid="structure">
        <actual.TxStructure {...props} />
      </div>
    ),
  };
});

const TYPE_ID_CODE_HASH = '0x00000000000000000000000000000000000000000000000000545950455f4944';
const lock = { codeHash: `0x${'22'.repeat(32)}`, hashType: 'type', args: '0x' };
const typeId = { codeHash: TYPE_ID_CODE_HASH, hashType: 'type', args: '0x' };
/** The real script hash of `lock` (blake2b, CKB personalisation). */
const LOCK_HASH = '0x0ee949b7b3d3e5e40183668bc71108c144424658397dcc2b20e25e8c261f8865';
/**
 * As the API lists them (transaction_structure::script_groups): every cell's lock is in a group —
 * the inputs' ran, the outputs' did not (a second group of the same hash) — and the Type ID group
 * lists the input and the output that carry it. Lock groups read the witness at their first input
 * (WitnessArgs), as the type group does.
 */
function lockGroups(cellbase: boolean) {
  return [
    ...(cellbase
      ? []
      : [
          {
            scriptHash: LOCK_HASH,
            kind: 'lock',
            ran: true,
            inputIndices: [0],
            outputIndices: [],
            witnessIndex: 0,
            script: lock,
            cycles: null,
          },
        ]),
    {
      scriptHash: LOCK_HASH,
      kind: 'lock',
      ran: false,
      inputIndices: [],
      outputIndices: [0, 1],
      witnessIndex: null,
      script: lock,
      cycles: null,
    },
  ];
}
function tx(hash: string, cellbase = false): TransactionDetail {
  return {
    hash,
    status: 'committed',
    fee: '0',
    isCellbase: cellbase,
    timestamp: '2024-01-01T00:00:00Z',
    inputsCapacity: cellbase ? null : '10000000000',
    inputs: cellbase
      ? []
      : [
          {
            previousOutput: { txHash: spentFixture.txHash, index: spentFixture.outputIndex },
            capacity: '10000000000',
            commonKnowledgeSize: '61',
            address: spentFixture.address,
            lock,
            type: typeId,
            display: null,
          },
        ],
    outputs: [0, 1].map((index) => ({
      capacity: '10000000000',
      commonKnowledgeSize: '61',
      address: liveFixture.address,
      lock,
      ...(index === 0 ? { type: typeId } : {}),
      status: 'live',
      display: null,
    })),
    scriptGroups: [
      ...lockGroups(cellbase),
      {
        scriptHash: `0x${'11'.repeat(32)}`,
        kind: 'type',
        ran: !cellbase,
        inputIndices: cellbase ? [] : [0],
        outputIndices: [0],
        witnessIndex: cellbase ? null : 0,
        script: typeId,
        cycles: cellbase ? null : '123',
      },
    ],
    witnessDetails: cellbase ? [] : [{ index: 0, size: 42, kind: 'opaque', fields: [] }],
  } as unknown as TransactionDetail;
}
function view(cell: Cell) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <CellTransactions cell={cell} />
    </QueryClientProvider>
  );
}
describe('CellTransactions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getTransactionDetail.mockImplementation(async (hash: string) => tx(hash));
    getTransactionCellDeps.mockResolvedValue([]);
  });
  it('uses the real Cell API live producer and shows current candidates without claiming a future execution', async () => {
    const cell = {
      ...liveFixture,
      currentSpendRequirements: [
        {
          kind: 'lock',
          referenceHash: lock.codeHash,
          hashType: 'type',
          resolution: 'ambiguous',
          versionHashes: [`0x${'aa'.repeat(32)}`, `0x${'bb'.repeat(32)}`],
          candidateCells: [
            {
              txHash: `0x${'cc'.repeat(32)}`,
              outputIndex: 1,
              versionHash: `0x${'aa'.repeat(32)}`,
              live: true,
            },
          ],
        },
      ],
    } as unknown as Cell;
    view(cell);
    expect(
      (await screen.findByTestId('structure')).querySelector('[data-cell-id="out:1"]')
    ).toHaveAttribute('data-highlighted', 'true');
    expect(screen.getByText(/Ambiguous current code cells/)).toBeInTheDocument();
    expect(screen.getByTitle(/^Candidate cell 0x(cc){32}:1/)).toHaveTextContent(
      '0xcccccccc…cccccc:1'
    );
    expect(screen.getByText(/Built-in Type ID/)).toBeInTheDocument();
    expect(screen.queryByText(/Spent transaction/)).not.toBeInTheDocument();
    // Phase 5 (D11): a live cell's transactions are the one that created it.
    expectPrompt(screen.getByRole('heading', { name: 'Transactions 1' }), 'Transactions', '1');
  });
  it('uses the real Cell API spent producer and focuses its exact spending input', async () => {
    const cell = spentFixture as unknown as Cell;
    getTransactionDetail.mockImplementation(async (hash: string) => tx(hash, hash === cell.txHash));
    view(cell);
    expect((await screen.findAllByText(/in:0/)).length).toBeGreaterThan(0);
    // the spending transaction's lock group and its type group both read witness 0
    expect(screen.getAllByText(/42 bytes/)).toHaveLength(2);
    // the creating cellbase runs none of its groups: its outputs' lock group and its Type ID group
    expect(screen.getAllByText(/Cellbase group: no script execution/)).toHaveLength(2);
    expect(getTransactionDetail).toHaveBeenCalledWith(cell.consumedByTx);
    // Phase 5 (D11): a spent cell's are two, the one that created it and the one that spent it.
    expectPrompt(screen.getByRole('heading', { name: 'Transactions 2' }), 'Transactions', '2');
  });
  // Lane U follow-up (user report: "two Script groups sections"): a spent cell shows its creating
  // and its spending transaction, and each listed its groups under the same bare "Script groups".
  // Each list is named by its transaction (the Cell board: "Script groups of that transaction"),
  // beside that transaction's cells; nothing in either list is dropped.
  it('names each transaction’s script groups by that transaction, never two bare "Script groups"', async () => {
    const cell = spentFixture as unknown as Cell;
    view(cell);
    await screen.findAllByText(/in:0/);
    const headings = screen.getAllByRole('heading', { name: /script groups/i });
    expect(headings.map((h) => h.textContent)).toEqual([
      'Script groups of the transaction that created it',
      'Script groups of the transaction that spent it',
    ]);
    expect(screen.queryByRole('heading', { name: 'Script groups' })).toBeNull();
    // both transactions keep their group rows: witness, code and cycles (in each, the lock group
    // and the Type ID group read witness 0; the Type ID group ran 123 cycles)
    expect(screen.getAllByText(/Witness #0 · 42 bytes/)).toHaveLength(4);
    expect(screen.getAllByText('123 cycles')).toHaveLength(2);
  });
  it('keeps executed and attached lock groups with the same script hash distinct', async () => {
    const hash = `0x${'77'.repeat(32)}`;
    getTransactionDetail.mockImplementation(async () => {
      const detail = tx(hash);
      const scriptHash = `0x${'33'.repeat(32)}`;
      detail.scriptGroups = [
        {
          scriptHash,
          kind: 'lock',
          ran: true,
          inputIndices: [0],
          outputIndices: [],
          witnessIndex: 0,
          script: lock,
          cycles: '123',
        },
        {
          scriptHash,
          kind: 'lock',
          ran: false,
          inputIndices: [],
          // every output's lock is in a group: output 1's too
          outputIndices: [0, 1],
          witnessIndex: null,
          script: lock,
          cycles: null,
        },
        // the Type ID group of input 0 and output 0, as before
        ...detail.scriptGroups!.filter((group) => group.kind === 'type'),
      ];
      return detail;
    });
    view(liveFixture as unknown as Cell);
    const structure = await screen.findByTestId('structure');
    const groups = structure.querySelectorAll<HTMLElement>('[data-group-id]');
    expect(groups).toHaveLength(3);
    expect(groups[2]).toHaveAttribute('data-group-id', `type:run:0x${'11'.repeat(32)}`);
    expect(groups[0]).toHaveAttribute('data-group-id', `lock:run:0x${'33'.repeat(32)}`);
    expect(groups[1]).toHaveAttribute('data-group-id', `lock:attached:0x${'33'.repeat(32)}`);
    fireEvent.mouseEnter(groups[0]);
    expect(structure.querySelector('[data-cell-id="in:0"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(structure.querySelector('[data-cell-id="out:0"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
    fireEvent.mouseEnter(groups[1]);
    expect(structure.querySelector('[data-cell-id="in:0"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
    expect(structure.querySelector('[data-cell-id="out:0"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(structure.querySelector('[data-cell-id="out:1"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
  });
});
