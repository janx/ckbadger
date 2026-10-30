import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { CellTransactions } from '@/components/cell/cell-transactions';
import type { Cell, TransactionDetail } from '@/lib/api';
import liveFixture from '@/__tests__/fixtures/d5/cell-live.json';
import spentFixture from '@/__tests__/fixtures/d5/cell-spent.json';

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
            display: null,
          },
        ],
    outputs: [0, 1].map(() => ({
      capacity: '10000000000',
      commonKnowledgeSize: '61',
      address: liveFixture.address,
      lock,
      status: 'live',
      display: null,
    })),
    scriptGroups: [
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
  });
  it('uses the real Cell API spent producer and focuses its exact spending input', async () => {
    const cell = spentFixture as unknown as Cell;
    getTransactionDetail.mockImplementation(async (hash: string) => tx(hash, hash === cell.txHash));
    view(cell);
    expect((await screen.findAllByText(/in:0/)).length).toBeGreaterThan(0);
    expect(screen.getByText(/42 bytes/)).toBeInTheDocument();
    expect(screen.getByText(/Cellbase group: no script execution/)).toBeInTheDocument();
    expect(getTransactionDetail).toHaveBeenCalledWith(cell.consumedByTx);
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
          outputIndices: [0],
          witnessIndex: null,
          script: lock,
          cycles: null,
        },
      ];
      return detail;
    });
    view(liveFixture as unknown as Cell);
    const structure = await screen.findByTestId('structure');
    const groups = structure.querySelectorAll<HTMLElement>('[data-group-id]');
    expect(groups).toHaveLength(2);
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
  });
});
