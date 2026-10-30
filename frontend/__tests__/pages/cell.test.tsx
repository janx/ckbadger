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
  api: { getCell: vi.fn(), lookupScripts: vi.fn(async () => ({})) },
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
  // 97-byte CellOutput: 16 B header + 8 B capacity + 73 B lock (20-byte args); 61 occupied.
  cellOutputMolecule: {
    bytes: `0x${'00'.repeat(97)}`,
    fields: [
      { label: 'total_size', start: 0, end: 4, occupied: false },
      { label: 'offset_capacity', start: 4, end: 8, occupied: false },
      { label: 'offset_lock', start: 8, end: 12, occupied: false },
      { label: 'offset_type', start: 12, end: 16, occupied: false },
      { label: 'capacity', start: 16, end: 24, occupied: true },
      { label: 'lock.total_size', start: 24, end: 28, occupied: false },
      { label: 'lock.offset_code_hash', start: 28, end: 32, occupied: false },
      { label: 'lock.offset_hash_type', start: 32, end: 36, occupied: false },
      { label: 'lock.offset_args', start: 36, end: 40, occupied: false },
      { label: 'lock.code_hash', start: 40, end: 72, occupied: true },
      { label: 'lock.hash_type', start: 72, end: 73, occupied: true },
      { label: 'lock.args_length', start: 73, end: 77, occupied: false },
      { label: 'lock.args', start: 77, end: 97, occupied: true },
    ],
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
    expect(screen.getByText('Unspent for').parentElement).toHaveTextContent(
      /^Unspent for1 mas of #125$/
    );
    expect(screen.queryByText(/ms/)).not.toBeInTheDocument();
    expect(screen.getByLabelText('CellOutput byte map')).toBeInTheDocument();
    expect(screen.getByTestId('transactions')).toHaveTextContent('live evidence');
    expect(screen.getByRole('region', { name: 'Cell facts' })).toHaveTextContent(
      'Occupied61 B · 39.00000000 CKB free'
    );
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
    // Created / Spent times read as the site UTC+8 clock, never raw ISO (#16, SC-C1 row).
    expect(screen.getByText('2024-01-01 08:00:00 UTC+8')).toBeInTheDocument();
    expect(screen.getByText('2024-01-02 08:00:00 UTC+8')).toBeInTheDocument();
    expect(screen.queryByText(/2024-01-02T00:00:00Z/)).not.toBeInTheDocument();
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
        compensation: '1234523456789',
        compensationCkb: '12345.23456789',
      },
    } as Cell);
    view();
    expect(await screen.findByText('Nervos DAO')).toBeInTheDocument();
    expect(screen.getByText(/deposited/)).toBeInTheDocument();
    // Exact CKB in the site's one amount form, thousands grouped (SC-C3, #16).
    expect(screen.getByText(/deposited/).closest('p')).toHaveTextContent(
      'compensation 12,345.23456789 CKB'
    );
    expect(screen.queryByText(/shannons/)).not.toBeInTheDocument();
  });
  it('links script code hash with its exact hash type, not the script instance hash', async () => {
    getCell.mockResolvedValue(base);
    view();
    // Unregistered (the lookup knows no name): the short code hash, linked with its hash type.
    const script = await screen.findByRole('link', { name: '0x22222222…222222' });
    expect(script).toHaveAttribute('href', `/mainnet/script/${base.lock!.codeHash}?hashType=type`);
    expect(script).toHaveAttribute('title', `type ${base.lock!.codeHash}`);
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
