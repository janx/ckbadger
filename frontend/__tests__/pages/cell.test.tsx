import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { api, type Cell } from '@/lib/api';
import { expectBefore, expectOnePageSeal } from '@/__tests__/utils/seals';

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
  createdEpoch: { number: 7, index: 8, length: 1000 },
  dataHash: `0x${'00'.repeat(32)}`,
  status: 'live' as const,
  liveAgeMs: 60000,
  liveAgeAtBlock: 125,
  lock: { codeHash: `0x${'22'.repeat(32)}`, hashType: 'type', args: `0x${'33'.repeat(20)}` },
  type: undefined,
  // The cell detail always carries the registry slugs (null = not a registered protocol).
  protocolScript: { lock: null, type: null },
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
  // Rubbing × Song print, phase 3: the page seal (112 px, 80 on a phone, −4°) of the out point's
  // 32-byte part, the hash of the transaction that made the cell, pressed beside the out point.
  it('presses the seal of its out point’s transaction hash beside the out point', async () => {
    getCell.mockResolvedValue({ ...base, outputIndex: 2 });
    outpoint = `${hash}-2`;
    const { container } = view();
    const title = await screen.findByRole('heading', { level: 1 });
    const seal = expectOnePageSeal(container, base.txHash, 'object');
    const head = seal.closest('[data-sealed-head]')!;
    expect(head).toContainElement(title);
    expect(head).toHaveTextContent(`${base.txHash}:2`);
    expectBefore(seal, title);
    expect(container.querySelector('[data-tier="1"]')).not.toContainElement(seal);
  });
  it('presses no seal when the out point cannot be read', () => {
    outpoint = hash;
    const { container } = view();
    expect(screen.getByText('Invalid cell outpoint')).toBeInTheDocument();
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
  });
  it('shows loading and then exact live cell anatomy, chronology and byte map', async () => {
    let resolve!: (value: Cell) => void;
    getCell.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      })
    );
    view();
    // Visual finish V8: the shared detail skeleton, not a "Loading cell evidence…" line
    expect(screen.getByRole('status', { name: 'Loading …' }).children).toHaveLength(3);
    expect(screen.queryByText('Loading cell evidence…')).toBeNull();
    resolve(base);
    // Phase 5 (A7): the microscope's heading is the bytes command, run on the occupied sum.
    expect(
      await screen.findByRole('heading', { name: /^Bytes \d+ \+ \d+ \+ \d+ \+ \d+ = \d+$/ })
    ).toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Loading …' })).toBeNull();
    // Consistency plan C1: the tab names the page.
    // Lane U (U3): the tab names a cell — "Cell 0xabab…abab:0 · CKBadger".
    expect(document.title).toMatch(/^Cell 0x[0-9a-f]{4}…[0-9a-f]{4}:\d+ · CKBadger$/);
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
    // Lane U (U3): a spent cell's origin line names the block that spent it.
    expect(screen.getByTestId('cell-origin')).toHaveTextContent(
      /^output 0 of transaction 0xabababab…ababab · spent at #125$/
    );
    // Lane L: the "only name" sentence links both blocks of the cell's life.
    const onlyName = screen.getByText(/The out point is the cell's only name/);
    expect(onlyName).toHaveTextContent('existed from block #123 until block #125 spent it.');
    expect(within(onlyName).getByRole('link', { name: '#123' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/123'
    );
    expect(within(onlyName).getByRole('link', { name: '#125' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/125'
    );
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
    // Rubbing × Song print, phase 5 (D1): the command runs on the API's status.
    const heading = await screen.findByRole('heading', { name: 'Nervos DAO deposited' });
    expect(heading.lastElementChild).toHaveTextContent(/^deposited$/);
    expect(heading.lastElementChild).toHaveClass('text-text-dim');
    // the heading says the status; the line under it does not say it again
    const line = screen.getByText(/^deposit block/).closest('p') as HTMLElement;
    expect(line).not.toHaveTextContent('deposited');
    // Exact CKB in the site's one amount form, thousands grouped (SC-C3, #16).
    expect(line).toHaveTextContent('compensation 12,345.23456789 CKB');
    expect(screen.queryByText(/shannons/)).not.toBeInTheDocument();
  });
  // Rubbing × Song print, phase 5 (D2): the dep group's command runs on its member count.
  it('counts a dep group’s members in its heading, one item or more', async () => {
    const member = (index: number) => ({ txHash: `0x${'cd'.repeat(32)}`, outputIndex: index });
    getCell.mockResolvedValue({ ...base, isDepGroup: true, depGroupItems: [member(0), member(1)] });
    const { unmount } = view();
    const heading = await screen.findByRole('heading', { name: 'Dep group 2 items' });
    expect(heading.parentElement!.parentElement!.querySelectorAll('li')).toHaveLength(2);
    unmount();
    getCell.mockResolvedValue({ ...base, isDepGroup: true, depGroupItems: [member(0)] });
    view();
    expect(await screen.findByRole('heading', { name: 'Dep group 1 item' })).toBeInTheDocument();
  });
  it('refuses a dep group without its members rather than drawing an empty list', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      getCell.mockResolvedValue({ ...base, isDepGroup: true });
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <ErrorBoundary fallback={<p>page-level boundary</p>}>
            <CellDetailPage />
          </ErrorBoundary>
        </QueryClientProvider>
      );
      expect(await screen.findByText('page-level boundary')).toBeInTheDocument();
      expect(quiet.mock.calls.flat().map(String).join(' ')).toContain(
        `Cell ${hash}:0 is a dep group without its members`
      );
      expect(screen.queryByRole('heading', { name: /^Dep group/ })).toBeNull();
    } finally {
      quiet.mockRestore();
    }
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
    expect(screen.getByText('no cell at this address.')).toBeInTheDocument();
    rendered.unmount();
    getCell.mockRejectedValue(new Error('node unavailable'));
    view();
    expect(await screen.findByText('Cell data unavailable')).toBeInTheDocument();
    expect(
      screen.getAllByRole('alert').some((item) => item.textContent?.includes('node unavailable'))
    ).toBe(true);
  });
});
