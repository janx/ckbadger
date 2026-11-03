import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { api, type Cell } from '@/lib/api';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';

// The Cell board's own cell: live Spore 0xbe1b9761…936cd8:1 (320 CKB, 8 + 66 + 65 + 181 bytes),
// its CellOutput and data bytes taken from the chain by the board generator.
const cell = sporeCell as unknown as Cell;
let outpoint = `${cell.txHash}-1`;
const transactionsProps = vi.fn();
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ outpoint }),
  usePathname: () => '/cell',
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock('@/components/cell/cell-transactions', () => ({
  CellTransactions: (props: { lockName: string | null; typeName: string | null }) => {
    transactionsProps(props);
    return <div data-testid="transactions" />;
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/lib/api', () => ({
  api: {
    getCell: vi.fn(),
    lookupScripts: vi.fn(),
    // Lane U (U6): the hero's inventory line reads the object and its cluster.
    getSporeObject: vi.fn(),
    getSporeCluster: vi.fn(),
  },
  isNotFoundApiError: () => false,
}));

const CLUSTER_ID = '0x26a1fe3f4e84ae2cd1dc3e319ca416e27624c5ebe2f1b0aa731ca18d67784c4c';
/** The local API's answers for this object and its cluster (2026-11-03). */
function mockObjectReads() {
  vi.mocked(api.getSporeObject).mockResolvedValue({
    sporeId: cell.type!.args,
    clusterId: CLUSTER_ID,
    mediaProfile: { tier: 'btc_ckb' },
  } as never);
  vi.mocked(api.getSporeCluster).mockResolvedValue({
    clusterId: CLUSTER_ID,
    name: 'Nervape 3x',
  } as never);
}

function view() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <CellDetailPage />
    </QueryClientProvider>
  );
}

describe('Cell page against the Cell board', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    outpoint = `${cell.txHash}-1`;
    vi.mocked(api.getCell).mockResolvedValue(cell);
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [cell.lock!.codeHash]: { name: 'Nervape Shadow Lock' },
      [cell.type!.codeHash]: { name: 'Spore' },
    } as never);
    mockObjectReads();
  });

  it('says what inventory item the cell is, in one line right under the h1 (lane U, U6)', async () => {
    view();
    const line = await screen.findByTestId('cell-inventory', undefined, { timeout: 5000 });
    expect(await within(line).findByText(/Nervape 3x/)).toBeInTheDocument();
    expect(line).toHaveTextContent(/^This cell is Nervape 3x 0x51c75ad6…2796f2 BTC\+CKB$/);
    expect(within(line).getByRole('link')).toHaveAttribute(
      'href',
      `/mainnet/objects/${cell.type!.args}`
    );
    // first under the h1 row: before the full out point and its copy
    const header = screen.getByRole('heading', { level: 1 }).closest('header')!;
    expect(header).toContainElement(line);
    const outpointLine = screen.getByRole('button', { name: 'Copy out point' }).parentElement!;
    expect(line.compareDocumentPosition(outpointLine)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it('names the cell by its out point, with copy, its origin line and the "only name" note (SC-C9)', async () => {
    view();
    await screen.findByText('The cell, byte by byte', undefined, { timeout: 5000 });
    // Lane U (U3): the page presents a CELL, not a transaction output — no "Transaction … /
    // output 1" breadcrumb; the transaction and the index are one line under the full out point.
    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
    expect(screen.queryByText(/^Transaction 0x/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Copy out point' })).toBeInTheDocument();
    // Consistency round C13 + U3: the h1 is "Cell" (dim) before the bright short out point.
    const h1 = screen.getByRole('heading', { level: 1 });
    expect(h1).toHaveTextContent(/^Cell 0xbe1b9761…936cd8:1$/);
    expect(within(h1).getByText('Cell')).toHaveClass('text-text-dim');
    expect(h1).toHaveClass('text-entity');
    expect(document.title).toBe('Cell 0xbe1b…6cd8:1 · CKBadger');
    expect(h1.closest('header')).toHaveTextContent(`${cell.txHash}:1`);
    const origin = screen.getByTestId('cell-origin');
    expect(origin).toHaveTextContent(
      /^output 1 of transaction 0xbe1b9761…936cd8 · live since #20,801,533$/
    );
    expect(within(origin).getByRole('link', { name: '0xbe1b9761…936cd8' })).toHaveAttribute(
      'href',
      `/mainnet/tx/${cell.txHash}`
    );
    expect(within(origin).getByRole('link', { name: '#20,801,533' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20801533'
    );
    // the origin line sits under the full out point, before the "only name" sentence
    const outpointLine = screen.getByRole('button', { name: 'Copy out point' }).parentElement!;
    expect(outpointLine.compareDocumentPosition(origin)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(screen.getByText(/The out point is the cell's only name/)).toHaveTextContent(
      'exists from block #20,801,533 until a transaction spends it.'
    );
    const facts = screen.getByRole('region', { name: 'Cell facts' });
    expect(facts).toHaveTextContent('Occupied320 B');
    expect(facts).toHaveTextContent('Unspent for3 d 10 h');
    expect(facts).toHaveTextContent('as of #20,834,844');
  });

  it('shows the lock and type by name, never a raw code hash (SC-C4)', async () => {
    view();
    const facts = await screen.findByRole('region', { name: 'Cell facts' }, { timeout: 5000 });
    expect(
      await within(facts).findByRole('link', { name: 'Nervape Shadow Lock' }, { timeout: 5000 })
    ).toHaveAttribute('href', `/mainnet/script/${cell.lock!.codeHash}?hashType=data1`);
    expect(within(facts).getByRole('link', { name: 'Spore' })).toBeInTheDocument();
    expect(facts.textContent).not.toContain(cell.lock!.codeHash);
    expect(api.lookupScripts).toHaveBeenCalledWith(
      [cell.lock!.codeHash, cell.type!.codeHash],
      cell.txHash
    );
    expect(transactionsProps).toHaveBeenLastCalledWith(
      expect.objectContaining({ lockName: 'Nervape Shadow Lock', typeName: 'Spore' })
    );
  });

  it('is one microscope: grouped pills, one hex box with both blocks, no view switch (SC-C5)', async () => {
    view();
    await screen.findByRole('link', { name: 'Spore' }, { timeout: 5000 });
    expect(screen.queryByRole('button', { name: 'CellData' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'CellOutput' })).toBeNull();
    const index = screen.getByLabelText('Field index');
    expect(
      [...index.querySelectorAll('[data-index-group]')].map((node) => node.textContent)
    ).toEqual(['capacity', 'lock', 'type', 'data']);
    // at 390 px the group name sits above its pills instead of eating an 80 px column
    for (const label of index.querySelectorAll('[data-index-group]'))
      expect(label).toHaveClass('w-full', 'sm:absolute', 'sm:w-[74px]');
    // every field is one pill: 21 CellOutput fields + 10 SporeData fields
    expect(index.querySelectorAll('[data-pill]')).toHaveLength(31);
    const box = screen.getByLabelText('Cell bytes');
    expect(within(box).getByLabelText('CellOutput byte map')).toHaveTextContent(
      'outputs[1] · CellOutput · molecule195 B · 139 occupied'
    );
    expect(within(box).getByLabelText('CellData byte map')).toHaveTextContent(
      'outputs_data[1] · decoded by its type script, Spore181 B · all occupied'
    );
    // every byte of both blocks is drawn once
    expect(box.querySelectorAll('[data-byte^="output:"]')).toHaveLength(195);
    expect(box.querySelectorAll('[data-byte^="data:"]')).toHaveLength(181);
  });

  it('lays bytes out 24 a row with offsets and an ASCII column (SC-C6)', async () => {
    view();
    const box = await screen.findByLabelText('Cell bytes', undefined, { timeout: 5000 });
    expect(within(box).getAllByText('0x0000')).toHaveLength(2);
    expect(within(box).getAllByText('0x0018').length).toBeGreaterThan(0);
    const dataBlock = within(box).getByLabelText('CellData byte map');
    expect(dataBlock.textContent).toContain('dob/0');
  });

  it('links byte, pill, map and the sticky detail (SC-C5, SC-C6)', async () => {
    view();
    await screen.findByRole('link', { name: 'Nervape Shadow Lock' }, { timeout: 5000 });
    const box = await screen.findByLabelText('Cell bytes', undefined, { timeout: 5000 });
    fireEvent.mouseOver(box.querySelector('[data-byte="output:40"]')!);
    const detail = screen.getByRole('complementary', { name: 'Field detail' });
    expect(detail).toHaveTextContent('lock · code_hash');
    expect(detail).toHaveTextContent('[40..72) · 32 B');
    expect(detail).toHaveTextContent(`inoutputs[1]`);
    expect(detail).toHaveTextContent('typeByte32');
    expect(detail).toHaveTextContent(`value${cell.lock!.codeHash}`);
    expect(detail).toHaveTextContent('occupiedyes, counts toward the 320');
    // byte → field: its bytes are lit and the rest dimmed by one stylesheet rule set
    expect(box).toHaveAttribute('data-active', 'out:lock.code_hash');
    const css = document.querySelector(
      'section[aria-label="Cell byte microscope"] style'
    )!.textContent!;
    expect(css).toContain('[data-cell-bytes] [data-part-id]{opacity:.25}');
    expect(css).toContain('[data-byte][data-part-id="out:lock.code_hash"]{outline');
    // byte → pill
    expect(
      screen.getByLabelText('Field index').querySelector('[data-pill="out:lock.code_hash"]')
    ).toHaveClass('border-[#a0a8be]');
    const map = screen.getByRole('group', { name: 'Capacity map' });
    expect(within(map).getByTitle('lock · 66 bytes')).toHaveAttribute('data-lit', 'true');
    expect(within(map).getByTitle('type · 65 bytes')).toHaveAttribute('data-lit', 'false');

    fireEvent.mouseEnter(screen.getByRole('button', { name: /^capacity \[16\.\.24\)/ }));
    expect(detail).toHaveTextContent('value32,000,000,000 shannons');
    expect(detail).toHaveTextContent('typeUint64');

    fireEvent.mouseEnter(within(map).getByTitle('lock · 66 bytes'));
    expect(detail).toHaveTextContent('lock · Nervape Shadow Lock');
    expect(detail).toHaveTextContent('blake2b(bytes 24..110)');
    expect(detail).toHaveTextContent(cell.cellOutputMolecule!.lockScriptHash);

    fireEvent.mouseOver(box.querySelector('[data-byte="data:22"]')!);
    expect(detail).toHaveTextContent('content type');
    expect(detail).toHaveTextContent('valuedob/0');
    expect(detail).toHaveTextContent('inoutputs_data[1]');
  });

  it('writes the DAO deposit block with thousands separators (SC-C10)', async () => {
    vi.mocked(api.getCell).mockResolvedValue({
      ...cell,
      daoInfo: {
        isDaoCell: true,
        daoStatus: 'withdrawn',
        depositBlockNumber: 20311586,
        depositTimestamp: '2026-01-01T00:00:00Z',
      },
    } as Cell);
    view();
    expect(await screen.findByText(/withdrawn · deposit block/)).toHaveTextContent(
      'deposit block #20,311,586'
    );
  });
});

describe('Cell page · visual pass (R2b-b createdEpoch, dataHash)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    outpoint = `${cell.txHash}-1`;
    vi.mocked(api.getCell).mockResolvedValue(cell);
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [cell.lock!.codeHash]: { name: 'Nervape Shadow Lock' },
      [cell.type!.codeHash]: { name: 'Spore' },
    } as never);
    mockObjectReads();
  });

  it('keeps the board facts on one strip: epoch under Created, "0 free", no Owner (SC-C9)', async () => {
    view();
    const facts = await screen.findByRole('region', { name: 'Cell facts' }, { timeout: 5000 });
    // the epoch takes its own line under the time, so the seven facts fit one row at 1440
    expect(facts).toHaveTextContent('2026-10-22 22:43:55 UTC+8epoch 15,157 · 112/1800');
    expect(within(facts).getByText(/^epoch 15,157/)).toHaveClass('block');
    expect(facts).toHaveTextContent('Occupied320 B · 0 free');
    expect(within(facts).queryByText('Owner')).toBeNull();
  });

  it('shows a missing createdEpoch as an error in place, not as a blank (SC-C9)', async () => {
    const { createdEpoch: _drop, ...withoutEpoch } = cell as Cell & { createdEpoch: unknown };
    void _drop;
    vi.mocked(api.getCell).mockResolvedValue(withoutEpoch as Cell);
    view();
    expect(
      await screen.findByText(/has no createdEpoch/, undefined, { timeout: 5000 })
    ).toHaveAttribute('role', 'alert');
  });

  it('gives the data its hash formula and value in the detail (SC-C6)', async () => {
    view();
    const map = await screen.findByRole('group', { name: 'Capacity map' }, { timeout: 5000 });
    fireEvent.mouseEnter(within(map).getByTitle('data · 181 bytes'));
    const detail = screen.getByRole('complementary', { name: 'Field detail' });
    expect(detail).toHaveTextContent('data hashblake2b(data)');
    expect(detail).toHaveTextContent(
      '0x550fe383bf357f4eb3b1853f324f9800dd3e49ac792d892254b646f91b861df1'
    );
  });

  it('pads a short last hex row with bytes of the same width, so ASCII columns align', async () => {
    view();
    const box = await screen.findByLabelText('Cell bytes', undefined, { timeout: 5000 });
    const pads = box.querySelectorAll('[data-pad]');
    expect(pads.length).toBeGreaterThan(0);
    for (const pad of pads) expect(pad).toHaveClass('px-px');
  });
});
