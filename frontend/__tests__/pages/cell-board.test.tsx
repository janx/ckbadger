import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { api, type Cell } from '@/lib/api';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';
import { expectBefore, expectNamePlate, expectOnePageSeal } from '@/__tests__/utils/seals';

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

  it('presses the cell’s own seal: a Spore object, its hexagon window and type glyph (seal kinds, C3)', async () => {
    const { container } = view();
    await screen.findByRole('heading', { level: 1 }, { timeout: 5000 });
    // the class is the transaction page's: kindOf() of the API's typeCellType and display
    const seal = expectOnePageSeal(container, {
      kind: 'cell',
      tx: cell.txHash,
      index: 1,
      lock: cell.lockScriptHash,
      type: { class: 'objects', hash: cell.typeScriptHash! },
      capacity: BigInt(32000000000),
    });
    expect(seal.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Cell seal ${cell.txHash.slice(0, 10)}:1, objects, 320.00000000 CKB`
    );
    // the objects' window, a hexagon: one closed six-sided outline among the open strokes
    expect(seal.querySelector('path[fill="none"][d$="Z"]')).not.toBeNull();
  });

  // Lane U (U6): the inventory item the cell is, in one line. The name plate (seals by kind, C10):
  // it opens what is under the stone and the plate, after the full out point, from the page's left
  // edge (it was the first line beside the stone, under the h1).
  it('says what inventory item the cell is, in one line opening the head under the plate (lane U, U6)', async () => {
    const { container } = view();
    const line = await screen.findByTestId('cell-inventory', undefined, { timeout: 5000 });
    expect(await within(line).findByText(/Nervape 3x/)).toBeInTheDocument();
    expect(line).toHaveTextContent(/^This cell is Nervape 3x 0x51c75ad6…2796f2 BTC\+CKB$/);
    expect(within(line).getByRole('link')).toHaveAttribute(
      'href',
      `/mainnet/objects/${cell.type!.args}`
    );
    const { plate, below } = expectNamePlate(container, 'Cell');
    expect(below!.children).toHaveLength(1);
    expect(below!.firstElementChild!.firstElementChild).toBe(line);
    expect(plate).not.toContainElement(line);
    // after the full out point and its copy, in the plate
    const outpointLine = screen.getByRole('button', { name: 'Copy out point' }).parentElement!;
    expect(plate).toContainElement(outpointLine);
    expectBefore(outpointLine, line);
  });

  // The name plate (C10): the kind line "Cell" with its status, the h1 the short out point alone
  // (the dim "Cell" that opened it is the kind line now), the id line the full out point and its
  // copy; under both, the inventory line, the origin line and the "only name" sentence, in order.
  it('stands the cell’s name plate beside its stone: Cell and its status, the short out point, the full one', async () => {
    const { container } = view();
    await screen.findByTestId('cell-inventory', undefined, { timeout: 5000 });
    const { kindLine, nameBox, title, below } = expectNamePlate(container, 'Cell');
    // the kind word, then the status: the live dot (no text) and "live"
    expect(kindLine.children).toHaveLength(2);
    expect(kindLine).toHaveTextContent(/^Celllive$/);
    expect(title).toHaveTextContent(/^0xbe1b9761…936cd8:1$/);
    expect(title.children).toHaveLength(0);
    expect(title).toHaveClass('text-entity', 'max-md:text-index');
    expect(nameBox.children).toHaveLength(2);
    const idLine = nameBox.lastElementChild as HTMLElement;
    expect(idLine).toHaveTextContent(new RegExp(`^${cell.txHash}:1copy$`));
    expect(idLine).toContainElement(screen.getByRole('button', { name: 'Copy out point' }));
    // C10 round 2: the id line in the plate's one quiet voice, set by the plate: the full out
    // point (it was text-body in the bright ink, competing with the h1 above it) takes it, its
    // colon in the same ink; the copy button keeps its own classes
    expect(idLine.className).toBe('text-text-dim text-small min-w-0 [overflow-wrap:anywhere]');
    const full = within(idLine).getByText(`${cell.txHash}:1`);
    expect(full.className).toBe('min-w-0');
    expect(full.children).toHaveLength(0);
    expect(screen.getByRole('button', { name: 'Copy out point' })).toHaveClass(
      'text-text-dim',
      'text-meta',
      'h-[26px]'
    );
    const parts = Array.from(below!.firstElementChild!.children);
    expect(parts).toEqual([
      screen.getByTestId('cell-inventory'),
      screen.getByTestId('cell-origin'),
      screen.getByText(/The out point is the cell's only name/),
    ]);
    // the origin and the sentence keep the identity lines' dim small voice
    expect(below!.firstElementChild).toHaveClass('text-text-dim', 'text-small', 'space-y-1.5');
  });

  it('names the cell by its out point, with copy, its origin line and the "only name" note (SC-C9)', async () => {
    view();
    await screen.findByRole('heading', { name: /^Bytes / }, { timeout: 5000 });
    // Lane U (U3): the page presents a CELL, not a transaction output — no "Transaction … /
    // output 1" breadcrumb; the transaction and the index are one line under the full out point.
    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
    expect(screen.queryByText(/^Transaction 0x/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Copy out point' })).toBeInTheDocument();
    // Consistency round C13 + U3, the name plate (C10): the h1 is the bright short out point; the
    // kind line before it says "Cell".
    const h1 = screen.getByRole('heading', { level: 1 });
    expect(h1).toHaveTextContent(/^0xbe1b9761…936cd8:1$/);
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
    const onlyName = screen.getByText(/The out point is the cell's only name/);
    expect(onlyName).toHaveTextContent(
      'exists from block #20,801,533 until a transaction spends it.'
    );
    // Lane L: every block height on the page links to its block.
    expect(within(onlyName).getByRole('link', { name: '#20,801,533' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20801533'
    );
    const facts = screen.getByRole('region', { name: 'Cell facts' });
    expect(facts).toHaveTextContent('Occupied320 B');
    expect(facts).toHaveTextContent('Unspent for3 d 10 h');
    expect(facts).toHaveTextContent('as of #20,834,844');
    expect(within(facts).getByRole('link', { name: '#20,834,844' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20834844'
    );
    // Lane L: the lock's owner, the cell's address, is named and linked under the lock.
    const owner = within(facts).getByRole('link', { name: /^ckb1qp3kr4/ });
    expect(owner).toHaveAttribute('href', `/mainnet/address/${cell.address}`);
    expect(owner).toHaveAttribute('title', cell.address);
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
    ).toHaveClass('border-text');
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
    const dao = (await screen.findByText(/^deposit block/)).closest('p') as HTMLElement;
    // phase 5 (D1): the section's command runs on the status, said there and not in the line
    expect(screen.getByRole('heading', { name: 'Nervos DAO withdrawn' })).toBeInTheDocument();
    expect(dao).not.toHaveTextContent('withdrawn');
    expect(dao).toHaveTextContent('deposit block #20,311,586');
    expect(within(dao).getByRole('link', { name: '#20,311,586' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20311586'
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
