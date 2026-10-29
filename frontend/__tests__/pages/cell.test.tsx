import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { api, type Cell } from '@/lib/api';

let outpoint = `0x${'ab'.repeat(32)}-0`;
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ outpoint }),
  usePathname: () => '/cell',
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock('@/components/cell/cell-transactions', () => ({
  CellTransactions: ({ cell }: { cell: Cell }) => (
    <div data-testid="transactions">
      {cell.status === 'dead' ? 'spent evidence' : 'live evidence'}
    </div>
  ),
}));
vi.mock('@/lib/api', () => ({
  api: { getCell: vi.fn() },
  isNotFoundApiError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'status' in error && error.status === 404,
}));
const getCell = vi.mocked(api.getCell);
const hash = `0x${'ab'.repeat(32)}`;
const base = {
  txHash: hash,
  outputIndex: 0,
  capacity: '10000000000',
  commonKnowledgeSize: '6100000000',
  commonKnowledgeSizeBreakdown: {
    capacityFieldBytes: 8,
    lockScriptBytes: 53,
    typeScriptBytes: 0,
    dataBytes: 0,
    totalBytes: 61,
  },
  lockScriptHash: `0x${'11'.repeat(32)}`,
  address: 'ckb1qtest',
  typeScriptHash: undefined,
  dataSize: 0,
  createdAtBlock: 123,
  createdTimestamp: '2024-01-01T00:00:00Z',
  status: 'live' as const,
  liveAgeMs: 60000,
  liveAgeAtBlock: 125,
  lock: { codeHash: `0x${'22'.repeat(32)}`, hashType: 'type', args: `0x${'33'.repeat(20)}` },
  type: undefined,
  data: '0x',
  cellOutputMolecule: {
    bytes: '0x00000000',
    fields: [{ label: 'framing', start: 0, end: 4, occupied: false }],
    lockScriptHash: `0x${'11'.repeat(32)}`,
    typeScriptHash: null,
  },
  currentSpendRequirements: [],
  isDepGroup: false,
} as Cell;
function view() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <CellDetailPage />
    </QueryClientProvider>
  );
}
describe('CellDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    outpoint = `${hash}-0`;
  });
  it('rejects an outpoint without an index before fetching', () => {
    outpoint = hash;
    view();
    expect(screen.getByText('Invalid cell outpoint')).toBeInTheDocument();
    expect(getCell).not.toHaveBeenCalled();
  });
  it('rejects output indices outside the store range before fetching', () => {
    outpoint = `${hash}-65536`;
    view();
    expect(screen.getByText('Invalid cell outpoint')).toBeInTheDocument();
    expect(getCell).not.toHaveBeenCalled();
  });
  it('shows loading and then exact live cell anatomy, chronology and byte map', async () => {
    let resolve!: (value: Cell) => void;
    getCell.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      })
    );
    view();
    expect(screen.getByText('Loading cell evidence…')).toBeInTheDocument();
    resolve(base);
    expect(await screen.findByText('The cell, byte by byte')).toBeInTheDocument();
    expect(screen.getByText('Age at indexed tip').parentElement).toHaveTextContent(
      /^Age at indexed tip 1 m · block #125$/
    );
    expect(screen.queryByText(/ms/)).not.toBeInTheDocument();
    expect(screen.getByLabelText('CellOutput byte map')).toBeInTheDocument();
    expect(screen.getByTestId('transactions')).toHaveTextContent('live evidence');
    expect(screen.getByText('Free')).toBeInTheDocument();
  });
  it('shows exact spent input, timestamps and chain lifetime', async () => {
    getCell.mockResolvedValue({
      ...base,
      status: 'dead',
      consumedAtBlock: 125,
      consumedByTx: `0x${'cd'.repeat(32)}`,
      consumingInputIndex: 2,
      spentTimestamp: '2024-01-02T00:00:00Z',
      lifetimeMs: 30000,
    });
    view();
    expect((await screen.findAllByText('Spent')).length).toBeGreaterThan(0);
    expect(screen.getByText(/input #2/)).toBeInTheDocument();
    expect(screen.getByText(/30 s on chain/)).toBeInTheDocument();
    expect(screen.getByTestId('transactions')).toHaveTextContent('spent evidence');
  });
  it('shows DAO lifecycle only for DAO cells', async () => {
    getCell.mockResolvedValue({
      ...base,
      daoInfo: {
        isDaoCell: true,
        daoStatus: 'deposited',
        depositBlockNumber: 123,
        depositTimestamp: '2024-01-01T00:00:00Z',
        compensation: '123456789',
        compensationCkb: '1.23456789',
      },
    } as Cell);
    view();
    expect(await screen.findByText('Nervos DAO')).toBeInTheDocument();
    expect(screen.getByText(/deposited/)).toBeInTheDocument();
    expect(screen.getByText(/compensation 1\.23456789 CKB/)).toBeInTheDocument();
    expect(screen.queryByText(/shannons/)).not.toBeInTheDocument();
  });
  it('links script code hash with its exact hash type, not the script instance hash', async () => {
    getCell.mockResolvedValue(base);
    view();
    const script = await screen.findByRole('link', { name: base.lock!.codeHash });
    expect(script).toHaveAttribute('href', `/mainnet/script/${base.lock!.codeHash}?hashType=type`);
    expect(screen.queryByRole('link', { name: base.lockScriptHash })).not.toBeInTheDocument();
  });
  it('fails visibly if exact occupied anatomy disagrees with indexed capacity', async () => {
    getCell.mockResolvedValue({ ...base, commonKnowledgeSize: '6000000000' });
    view();
    expect(await screen.findByTestId('cell-capacity-error')).toHaveTextContent('anatomy differs');
  });
  it('distinguishes missing cells from transport failures', async () => {
    getCell.mockRejectedValue(Object.assign(new Error('not found'), { status: 404 }));
    const rendered = view();
    expect(await screen.findByText(`${hash}-0`)).toBeInTheDocument();
    expect(
      screen.getByText('This explorer could not open the cell at this address.')
    ).toBeInTheDocument();
    rendered.unmount();
    getCell.mockRejectedValue(new Error('node unavailable'));
    view();
    expect(await screen.findByText('Cell data unavailable')).toBeInTheDocument();
    expect(
      screen.getAllByRole('alert').some((item) => item.textContent?.includes('node unavailable'))
    ).toBe(true);
  });
});
