import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { api, type Cell } from '@/lib/api';
import { expectBefore, expectOnePageSeal } from '@/__tests__/utils/seals';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { inkOf, READING } from '@/__tests__/utils/ink-role';

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
  // lane D3: the developer tier holds the transactions' script groups
  CellScriptGroups: () => <div data-testid="script-groups" />,
}));
vi.mock('@/lib/api', async (importOriginal) => ({
  // the error class itself: the page's failed-read line reads a gateway status off it
  ApiRequestError: (await importOriginal<typeof import('@/lib/api')>()).ApiRequestError,
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
  // the address that spells its lock (the lock words read the lock off it and check the two)
  address: ckbAddress({ codeHash: `0x${'22'.repeat(32)}`, args: `0x${'33'.repeat(20)}` }),
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
  // The cell detail's class fields (cell_type() and the tile display): no type script, plain.
  typeCellType: 'plain',
  display: null,
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
  // U-06: an out point the page cannot read is not an id: the voice page with its own line
  it('rejects an outpoint without an index before fetching', () => {
    outpoint = hash;
    view();
    expect(screen.getByText('that is not a cell out point.')).toBeInTheDocument();
    expect(getCell).not.toHaveBeenCalled();
  });
  it('rejects output indices outside the store range before fetching', () => {
    outpoint = `${hash}-65536`;
    view();
    expect(screen.getByText('that is not a cell out point.')).toBeInTheDocument();
    expect(getCell).not.toHaveBeenCalled();
  });
  // The seals by kind (C3): the cell's own page seal (112 px, 88 below md, −4°) — its
  // transaction's rim and border, its index in rods, its owner's stone, a bead per digit of its
  // capacity; no type script, so no window and no glyph — pressed beside the out point.
  it('presses the cell’s own seal beside the out point', async () => {
    getCell.mockResolvedValue({ ...base, outputIndex: 2 });
    outpoint = `${hash}-2`;
    const { container } = view();
    const title = await screen.findByRole('heading', { level: 1 });
    const seal = expectOnePageSeal(
      container,
      {
        kind: 'cell',
        tx: base.txHash,
        index: 2,
        lock: base.lockScriptHash,
        type: null,
        capacity: BigInt(10000000000),
      },
      'pressed'
    );
    expect(seal.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Cell seal ${base.txHash.slice(0, 10)}:2, ckb, 100.00000000 CKB`
    );
    const head = seal.closest('[data-sealed-head]')!;
    expect(head).toContainElement(title);
    expect(head).toHaveTextContent(`${base.txHash}:2`);
    expectBefore(seal, title);
    expect(container.querySelector('[data-tier="1"]')).not.toContainElement(seal);
    // the cell's stone, not its transaction's old hash seal
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
  });
  // U-09, the lead's ruling of the B3 follow-up (RESPONSIVE_LAYOUT principle 4): the h1 is the
  // out point at the head size where its measured line holds it, the row size otherwise; the row
  // size until measured. Below md the h1 is 26 px with −0.02 em tracking, 15.08 px a character:
  // the head out point "0xababababab…abababab:0" (23 characters) is 346.84 px, inside the 358 px line
  // at 390 and over the 328 at 360. The page picked the row size at every width before.
  it.each([
    [358, '0xababababab…abababab:0'],
    [328, '0xabababab…ababab:0'],
  ] as const)('names the cell on a %i px h1 line by %s', async (line, name) => {
    stubChartWidth(line);
    const real = window.getComputedStyle.bind(window);
    vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
      const style = real(el, pseudo);
      if (!(el as Element).closest('h1')) return style;
      return new Proxy(style, {
        get: (target, prop) =>
          prop === 'fontSize'
            ? '26px'
            : prop === 'letterSpacing'
              ? '-0.52px'
              : (target as unknown as Record<string | symbol, unknown>)[prop],
      });
    });
    try {
      getCell.mockResolvedValue(base);
      view();
      const title = await screen.findByRole('heading', { level: 1 });
      await waitFor(() => expect(title).toHaveTextContent(new RegExp(`^${name}$`)));
    } finally {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  });
  it('presses a spent cell’s seal in the dead ink', async () => {
    getCell.mockResolvedValue({
      ...base,
      status: 'dead',
      consumedAtBlock: 125,
      consumedByTx: `0x${'cd'.repeat(32)}`,
      consumingInputIndex: 2,
    });
    const { container } = view();
    await screen.findByRole('heading', { level: 1 });
    expectOnePageSeal(
      container,
      {
        kind: 'cell',
        tx: base.txHash,
        index: 0,
        lock: base.lockScriptHash,
        type: null,
        capacity: BigInt(10000000000),
      },
      'ink'
    );
  });
  it('classes a typed cell as its transaction page does: a token, its window and glyph', async () => {
    const typeHash = '0x3b02b7c0d2d8abe06500d473edc9956a11ef5601c58361d4c3048df3f15902ee';
    getCell.mockResolvedValue({
      ...base,
      type: {
        codeHash: '0x48dbf59b4c7ee1547238021b4869bceedf4eea6b43772e5d66ef8865b6ae7212',
        hashType: 'type',
        args: `0x${'30'.repeat(32)}`,
      },
      typeScriptHash: typeHash,
      typeCellType: 'sudt',
      display: {
        kind: 'token',
        standard: 'sudt',
        id: typeHash,
        name: 'Exact sudt',
        collectionId: null,
        collectionName: null,
        symbol: 'SUDT',
        decimals: 8,
        amount: '5',
        noAmountReason: null,
        compositionTier: null,
      },
    });
    const { container } = view();
    await screen.findByRole('heading', { level: 1 });
    const seal = expectOnePageSeal(container, {
      kind: 'cell',
      tx: base.txHash,
      index: 0,
      lock: base.lockScriptHash,
      type: { class: 'tokens', hash: typeHash },
      capacity: BigInt(10000000000),
    });
    // the tokens' window, a square
    expect(seal.querySelector('rect[fill="none"]')).not.toBeNull();
  });
  it('refuses a cell response without its class fields rather than pressing a plain stone', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      const { typeCellType: _drop, ...withoutClass } = base;
      void _drop;
      getCell.mockResolvedValue(withoutClass as Cell);
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
        `Cell ${hash}:0: no typeCellType`
      );
    } finally {
      quiet.mockRestore();
    }
  });
  it('presses no seal when the out point cannot be read', () => {
    outpoint = hash;
    const { container } = view();
    expect(screen.getByText('that is not a cell out point.')).toBeInTheDocument();
    expect(container.querySelector('[data-seal]')).toBeNull();
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
  });
  it('presses no seal when the cell cannot be read', async () => {
    getCell.mockRejectedValue(new Error('node unavailable'));
    const { container } = view();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not read this cell: node unavailable'
    );
    expect(container.querySelector('[data-seal]')).toBeNull();
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
    await waitFor(() =>
      expect(document.title).toMatch(/^Cell 0x[0-9a-f]{4}…[0-9a-f]{4}:\d+ · CKBadger$/)
    );
    expect(screen.getByText('Unspent for').parentElement).toHaveTextContent(
      /^Unspent for1mas of #125$/
    );
    expect(screen.queryByText(/ms/)).not.toBeInTheDocument();
    expect(screen.getByLabelText('CellOutput byte map')).toBeInTheDocument();
    expect(screen.getByTestId('transactions')).toHaveTextContent('live evidence');
    expect(screen.getByRole('region', { name: 'Cell facts' })).toHaveTextContent(
      'Occupied61 B · 39 CKB free'
    );
    // lane P1, the ink and size roles: its free CKB is a value, the reading ink (it was quiet)
    expect(
      inkOf(within(screen.getByRole('region', { name: 'Cell facts' })).getByText(/ free$/))
    ).toBe(READING);
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
    // Re-pinned on purpose (BRIEF-roles-rest part A; it read getByText(/input #2/) and
    // getByText(/30s on chain/), one text each): the Spent fact's second line keeps its words, its
    // label words ("input", "on chain") quiet spans beside the values in the reading ink
    const spent = within(screen.getByRole('region', { name: 'Cell facts' }))
      .getByText('Spent')
      .closest('div.min-w-0') as HTMLElement;
    expect(spent).toHaveTextContent(/· input #2 · 30s on chain$/);
    expect(inkOf(within(spent).getByText('· input'))).toBe('text-text-dim');
    expect(inkOf(within(spent).getByText('on chain'))).toBe('text-text-dim');
    // the line's own ink, which its values (the time, "#2", "30s") are written in
    expect(spent.lastElementChild).toHaveTextContent(/^2024-01-02 08:00:00 UTC\+8/);
    expect(inkOf(spent.lastElementChild!)).toBe(READING);
    // Created / Spent times read as the site UTC+8 clock, never raw ISO (#16, SC-C1 row).
    expect(screen.getByText('2024-01-01 08:00:00 UTC+8')).toBeInTheDocument();
    expect(screen.getByText('2024-01-02 08:00:00 UTC+8')).toBeInTheDocument();
    expect(screen.queryByText(/2024-01-02T00:00:00Z/)).not.toBeInTheDocument();
    expect(screen.getByTestId('transactions')).toHaveTextContent('spent evidence');
    // Lane U (U3): a spent cell's origin line named the block that spent it. Re-pinned on purpose
    // (lane P1, said once): the origin line is gone; the Spent fact names that block
    expect(screen.queryByTestId('cell-origin')).toBeNull();
    expect(
      within(screen.getByRole('region', { name: 'Cell facts' })).getByRole('link', { name: '#125' })
    ).toHaveAttribute('href', '/mainnet/blocks/125');
    // Lane L: the "only name" sentence links both blocks of the cell's life.
    const onlyName = screen.getByText(/The out point is the cell's only name/);
    expect(onlyName).toHaveTextContent('existed from block #123 until block #125 spent it.');
    // readability R3 / R4 (lane B7): the sentence explains, so it reads in the reading ink at the
    // body step, on the measure (it ran 171 characters a line at 1280, quiet, small). Re-pinned
    // on purpose (lane D3, U-17): it stands in the page's developer tier, its own measure, where it
    // stood in the lede's measured holder
    expect(onlyName).toHaveClass('text-text', 'text-body', 'max-w-measure');
    expect(onlyName.closest('[data-tier="2"]')).not.toBeNull();
    expect(onlyName.closest('[data-detail-lede]')).toBeNull();
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
        // F-9: the API sends every DAO cell's figure with its state and the block it is read at.
        compensationState: 'accruing',
        compensationAtBlock: 21016902,
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
    // F-9 (readability review 2026-11-18): a live deposit's compensation is unclaimed and still
    // accruing; the line says so and names the block the figure was read at.
    expect(line).toHaveTextContent(
      /^deposit block #123 · unclaimed compensation 12,345\.23456789 CKB, at block #21,016,902$/
    );
    expect(within(line).getByRole('link', { name: '#21,016,902' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/21016902'
    );
  });
  // F-9: a withdraw request fixes the figure at its own block; a completion claims it.
  it('says a DAO compensation is fixed at the withdrawal request, or claimed, at its block', async () => {
    const dao = {
      isDaoCell: true,
      depositBlockNumber: 123,
      depositTimestamp: '2024-01-01T00:00:00Z',
      withdrawRequestBlock: 150,
      withdrawRequestTimestamp: '2024-01-01T00:25:00Z',
      compensation: '898000001',
      compensationCkb: '8.98000001',
    };
    getCell.mockResolvedValue({
      ...base,
      daoInfo: {
        ...dao,
        daoStatus: 'withdrawing',
        compensationState: 'frozen',
        compensationAtBlock: 150,
      },
    } as Cell);
    const { unmount } = view();
    const frozen = (await screen.findByText(/^deposit block/)).closest('p') as HTMLElement;
    expect(frozen).toHaveTextContent(
      /^deposit block #123 · compensation 8\.98000001 CKB, fixed at the withdrawal request #150$/
    );
    expect(within(frozen).getByRole('link', { name: '#150' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/150'
    );
    unmount();

    getCell.mockResolvedValue({
      ...base,
      status: 'dead',
      consumedAtBlock: 180,
      daoInfo: {
        ...dao,
        daoStatus: 'withdrawn',
        withdrawBlock: 180,
        withdrawTimestamp: '2024-01-01T00:55:00Z',
        compensationState: 'claimed',
        compensationAtBlock: 180,
      },
    } as Cell);
    view();
    const claimed = (await screen.findByText(/^deposit block/)).closest('p') as HTMLElement;
    expect(claimed).toHaveTextContent(
      /^deposit block #123 · compensation 8\.98000001 CKB, claimed at block #180$/
    );
    expect(within(claimed).getByRole('link', { name: '#180' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/180'
    );
  });
  // Review of main..fix/readability (2026-11-18, test finding 2 and logic finding 4): the API sends
  // a DAO cell's compensation with its state and block (F-9); the page drew the DAO line with no
  // compensation, and no error, when an answer came without it (an older API binary behind a
  // preview). A DAO answer without its figure, or with a state the page does not know, is a broken
  // read: it fails, naming the out point and the field, and never draws an empty line.
  describe('a DAO cell read without its compensation', () => {
    const dao = {
      isDaoCell: true,
      daoStatus: 'deposited',
      depositBlockNumber: 123,
      depositTimestamp: '2024-01-01T00:00:00Z',
      compensation: '898000001',
      compensationCkb: '8.98000001',
      compensationState: 'accruing',
      compensationAtBlock: 150,
    };
    /** The page under a boundary, and what it threw. */
    async function failure(daoInfo: Record<string, unknown>): Promise<string> {
      const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
      try {
        getCell.mockResolvedValue({ ...base, daoInfo } as unknown as Cell);
        const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
        render(
          <QueryClientProvider client={client}>
            <ErrorBoundary fallback={<p>page-level boundary</p>}>
              <CellDetailPage />
            </ErrorBoundary>
          </QueryClientProvider>
        );
        expect(await screen.findByText('page-level boundary')).toBeInTheDocument();
        expect(screen.queryByText(/^deposit block/)).toBeNull();
        return quiet.mock.calls.flat().map(String).join(' ');
      } finally {
        quiet.mockRestore();
      }
    }

    it('fails, naming the out point and the field, where the compensation is missing', async () => {
      const { compensation: _c, compensationCkb: _ckb, ...without } = dao;
      void _c;
      void _ckb;
      expect(await failure(without)).toContain(
        `DAO cell ${hash}:0 compensation "undefined" is not an integer shannon amount`
      );
    });

    it('fails on a compensation state it does not know', async () => {
      expect(await failure({ ...dao, compensationState: 'pending' })).toContain(
        `DAO cell ${hash}:0 has compensation state pending`
      );
    });
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
    expect(await screen.findByText('no cell at this out point.')).toBeInTheDocument();
    // U-18: the checked endpoint stands in the facts, the hash shortened, once
    expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
      `/cells/${hash.slice(0, 10)}…${hash.slice(-6)}/0`
    );
    rendered.unmount();
    getCell.mockRejectedValue(new Error('node unavailable'));
    view();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not read this cell: node unavailable'
    );
  });
});
