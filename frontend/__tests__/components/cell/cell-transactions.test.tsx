import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
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
  },
}));
vi.mock('@/components/transaction/tx-structure', () => ({
  TxStructure: ({
    groups,
    focusedCellIds,
  }: {
    groups: Array<{ dependency: string; witness: string; executed: boolean }>;
    focusedCellIds: string[];
  }) => (
    <div data-testid="structure">
      <span>{focusedCellIds.join(',')}</span>
      {groups.map((group, index) => (
        <p key={index}>
          {group.executed ? 'executed' : 'not executed'} · {group.witness} · {group.dependency}
        </p>
      ))}
    </div>
  ),
}));

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
    expect(await screen.findByText(/out:1/)).toBeInTheDocument();
    expect(screen.getByText(/Ambiguous current code cells/)).toBeInTheDocument();
    expect(screen.getByText(/Candidate cell/)).toBeInTheDocument();
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
});
