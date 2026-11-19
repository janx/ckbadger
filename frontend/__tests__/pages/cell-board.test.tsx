import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { api, type Cell } from '@/lib/api';
import { scriptLookupKey } from '@/lib/script-pair';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';
import { expectBefore, expectNamePlate, expectOnePageSeal } from '@/__tests__/utils/seals';
import { expectAnswerUnclipped } from '@/__tests__/utils/copy-answer';
import { factsOf } from '@/__tests__/utils/facts';
import { gapPx, widthRuleAt } from '@/__tests__/utils/tailwind-box';

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
  // lane D3: the developer tier holds the transactions' script groups
  CellScriptGroups: () => <div data-testid="script-groups" />,
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
      [scriptLookupKey(cell.lock!.codeHash, cell.lock!.hashType)]: { name: 'Nervape Shadow Lock' },
      [scriptLookupKey(cell.type!.codeHash, cell.type!.hashType)]: { name: 'Spore' },
    } as never);
    mockObjectReads();
  });

  // Lane D2 (PA-13; gate D0′'s strip, with the cell's own four facts): its state, when it was
  // created, the block that created it and its capacity stand in the status strip under the id
  // line (the state was on the kind line, the rest in the facts after the lede); the other facts
  // stay in the cell's facts section, none deleted.
  it('states status, created, block and capacity in one strip under the id line (lane D2)', async () => {
    const { container } = view();
    const strip = await screen.findByRole('group', { name: 'Cell status' });
    const facts = factsOf(strip);
    expect(facts.map(([label]) => label)).toEqual(['status', 'created', 'block', 'capacity']);
    expect(facts[0][1]).toBe('live');
    expect(facts[1][1]).toMatch(/^2026-10-22 22:43:55 UTC\+8 · \S+ ago$/);
    expect(facts[2][1]).toBe('#20,801,533');
    expect(facts[3][1]).toBe('320 CKB');
    expect(strip.querySelector<HTMLElement>('[data-strip-dot]')!.style.backgroundColor).toBe(
      'var(--c-jade)'
    );
    // under the stone and the plate, before the lede
    const { kindLine, below } = expectNamePlate(container, 'Cell');
    expect(below!.firstElementChild).toBe(strip);
    expect(below!.lastElementChild).toHaveAttribute('data-detail-lede');
    // the state is said once: the kind line holds the kind word alone
    expect(kindLine).toHaveTextContent(/^Cell$/);
    // the rest stay in the facts section
    const rest = screen.getByRole('region', { name: 'Cell facts' });
    expect(rest).not.toHaveTextContent('Capacity');
    expect(rest).toHaveTextContent('Occupied320 B · 0 free');
    expect(rest).toHaveTextContent('Epoch15,157 · 112/1800');
    expect(rest).toHaveTextContent('Unspent for3d 10h');
  });

  // A4 review 3: the out point's copy button copies through the one copy path (hooks/useCopy):
  // where there is no clipboard (plain HTTP on a LAN address) it claims no copy and rejects
  // nothing; a copy that worked says "copied". Re-pinned on purpose (lane c2-chain): the copy is
  // the library's CopyButton, the board's one icon (it was a boxed word, "copy"), which answers in
  // words beside it, heard as a status: `copied`, or `not copied` where the clipboard refused (the
  // word button stayed "copy" and said nothing).
  it('copies the out point through the one copy path, claiming nothing without a clipboard', async () => {
    const rejections: unknown[] = [];
    const onRejection = (reason: unknown) => rejections.push(reason);
    process.on('unhandledRejection', onRejection);
    try {
      Object.assign(navigator, { clipboard: undefined });
      const { unmount } = view();
      const button = await screen.findByRole('button', { name: 'copy out point' });
      // lane c2-finish, rulings round 1: the copy's answer stands out of the line's flow under
      // its box, and nothing above the copy clips it
      expectAnswerUnclipped(button);
      await act(async () => {
        fireEvent.click(button);
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
      expect(button).toHaveTextContent(/^$/);
      expect(within(button.parentElement!).getByRole('status')).toHaveTextContent(/^not copied$/);
      expect(rejections).toEqual([]);
      unmount();
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.assign(navigator, { clipboard: { writeText } });
      view();
      const again = await screen.findByRole('button', { name: 'copy out point' });
      fireEvent.click(again);
      await waitFor(() =>
        expect(within(again.parentElement!).getByRole('status')).toHaveTextContent(/^copied$/)
      );
      expect(writeText).toHaveBeenCalledWith(`${cell.txHash}:${cell.outputIndex}`);
    } finally {
      process.off('unhandledRejection', onRejection);
      Object.assign(navigator, { clipboard: undefined });
    }
  });

  it('presses the cell’s own seal: a Spore object, its hexagon window and type glyph (seal kinds, C3)', async () => {
    const { container } = view();
    await screen.findByRole('heading', { level: 1 });
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
    const line = await screen.findByTestId('cell-inventory');
    expect(await within(line).findByText(/Nervape 3x/)).toBeInTheDocument();
    // U-09 (re-pinned): the hero's lede line writes the id at the head size; R9 (re-pinned):
    // prose takes the row form
    expect(line).toHaveTextContent(/^This cell is Nervape 3x 0x51c75ad6…2796f2 BTC\+CKB$/);
    expect(within(line).getByRole('link')).toHaveAttribute(
      'href',
      `/mainnet/objects/${cell.type!.args}`
    );
    const { plate, below } = expectNamePlate(container, 'Cell');
    // re-pinned with readability R4 (lane B7): the lede stands in one holder that sets it on the
    // measure (it was below's own child)
    // re-pinned on purpose (lane D2): the status strip first under the plate, the lede's holder
    // after it (the holder was below's one child)
    expect(below!.children).toHaveLength(2);
    expect(below!.firstElementChild).toBe(screen.getByRole('group', { name: 'Cell status' }));
    const holder = below!.lastElementChild!;
    expect(holder).toHaveAttribute('data-detail-lede');
    expect(holder).toHaveClass('max-w-measure');
    // re-pinned on purpose (lane P1): the lede is the one sentence itself, no wrapper of two lines
    expect(holder.firstElementChild).toBe(line);
    expect(plate).not.toContainElement(line);
    // after the full out point and its copy, in the plate
    const outpointLine = screen.getByRole('button', { name: 'copy out point' }).closest('div')!;
    expect(plate).toContainElement(outpointLine);
    expectBefore(outpointLine, line);
  });

  // Lane c2-finish, rulings round 2: the out point and its copy are every id line's one form
  // (IdCopyRow, HeadId's): the copy 8 px after the out point at its first line (it was its own
  // flex-wrap row, 10 px apart and centred), the out point taking its content's width so the copy
  // stands right after it, the copy the row's full height so its answer hangs under the whole id.
  // The out point is not one of HeadId's kinds (a hash and its index): it wraps where its line does.
  it('writes the out point and its copy in the id line’s one form', async () => {
    view();
    const copy = await screen.findByRole('button', { name: 'copy out point' });
    const copyRoot = copy.parentElement!;
    const row = copyRoot.parentElement!;
    expect(row.className).toBe('flex min-w-0 items-start gap-2');
    expect(copyRoot).toHaveClass('shrink-0', 'self-stretch');
    const outpoint = row.firstElementChild as HTMLElement;
    expect(outpoint).toHaveTextContent(`${cell.txHash}:1`);
    for (const w of [1280, 390]) {
      expect(widthRuleAt(outpoint, w)).toBe('auto');
      expect(gapPx(row, w)).toBe(8);
    }
  });

  // The name plate (C10): the kind line "Cell", the h1 the short out point alone (the dim "Cell"
  // that opened it is the kind line now), the id line the full out point and its copy; under both,
  // the status strip, then the inventory line, the origin line and the "only name" sentence, in
  // order. Re-pinned on purpose (lane D2; renamed, it was "…: Cell and its status, the short out
  // point, the full one"): the state is the strip's first fact, said once, so the kind line holds
  // the kind word alone (it held "Cell" and the live dot and "live").
  it('stands the cell’s name plate beside its stone: Cell, the short out point, the full one', async () => {
    const { container } = view();
    await screen.findByTestId('cell-inventory');
    const { kindLine, nameBox, title, below } = expectNamePlate(container, 'Cell');
    expect(kindLine.children).toHaveLength(1);
    expect(kindLine).toHaveTextContent(/^Cell$/);
    expect(title).toHaveTextContent(/^0xbe1b9761…936cd8:1$/);
    // U-09, the lead's question 2 (re-pinned on purpose): the h1 holds one thing, the out point as
    // TitleId writes it (the row form until its line is measured), where it held the bare text
    expect(title.children).toHaveLength(1);
    expect(title.firstElementChild).toHaveAttribute('data-title-id', 'row');
    expect(title).toHaveClass('text-entity', 'max-md:text-index');
    expect(nameBox.children).toHaveLength(2);
    const idLine = nameBox.lastElementChild as HTMLElement;
    // re-pinned (lane c2-chain): the copy is the library's icon, no word ("copy" was its text)
    expect(idLine).toHaveTextContent(new RegExp(`^${cell.txHash}:1$`));
    expect(idLine).toContainElement(screen.getByRole('button', { name: 'copy out point' }));
    // C10 round 2: the id line in the plate's one voice, set by the plate: the full out
    // point (it was text-body in the bright ink, competing with the h1 above it) takes it, its
    // colon in the same ink; the copy button keeps its own classes
    // re-pinned with readability ruling R3: one voice still, in the reading ink (the id is a
    // value; it was text-text-dim)
    expect(idLine.className).toBe('text-text text-small min-w-0 [overflow-wrap:anywhere]');
    const full = within(idLine).getByText(`${cell.txHash}:1`);
    expect(full.className).toBe('min-w-0');
    expect(full.children).toHaveLength(0);
    // re-pinned on purpose (lane c2-chain): the library's CopyButton, the board's icon in the
    // quiet ink in a 32 px box below 640 and 24 px from it (was a 26 px box with the word "copy")
    expect(screen.getByRole('button', { name: 'copy out point' })).toHaveClass(
      'text-text-dim',
      'hover:text-jade',
      'h-8',
      'sm:h-6'
    );
    // re-pinned with readability R4 (lane B7): the lede in its measured holder under the plate.
    // Re-pinned on purpose (lane D3, U-17): the generic "only name" sentence went to the page's
    // developer tier. Re-pinned on purpose (lane P1, the plan: the out point written once whole
    // and once short; said once): the lede is one sentence of what the cell is; the origin line
    // under it ("output 1 of transaction 0xbe1b9761…936cd8 · live since #20,801,533") wrote the
    // transaction's hash a third time and the strip's state and block again, and is gone
    const holder = below!.querySelector('[data-detail-lede]')!;
    expect(Array.from(holder.children)).toEqual([screen.getByTestId('cell-inventory')]);
    expect(screen.queryByTestId('cell-origin')).toBeNull();
    expect(container.querySelector('[data-tier="2"]')).toContainElement(
      screen.getByText(/The out point is the cell's only name/)
    );
  });

  // Renamed and re-pinned on purpose (lane P1; it was "… with copy, its origin line and the 'only
  // name' note"): the out point is written twice, whole on the id line and short in the h1; the
  // origin line went (its transaction is the Transactions section's link, its block the strip's).
  it('names the cell by its out point, whole and short, with copy and the "only name" note (SC-C9)', async () => {
    view();
    await screen.findByRole('heading', { name: /^Bytes / });
    // Lane U (U3): the page presents a CELL, not a transaction output — no "Transaction … /
    // output 1" breadcrumb; the transaction and the index are one line under the full out point.
    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
    expect(screen.queryByText(/^Transaction 0x/)).toBeNull();
    expect(screen.getByRole('button', { name: 'copy out point' })).toBeInTheDocument();
    // Consistency round C13 + U3, the name plate (C10): the h1 is the bright short out point; the
    // kind line before it says "Cell".
    const h1 = screen.getByRole('heading', { level: 1 });
    expect(h1).toHaveTextContent(/^0xbe1b9761…936cd8:1$/);
    expect(h1).toHaveClass('text-entity');
    await waitFor(() => expect(document.title).toBe('Cell 0xbe1b…6cd8:1 · CKBadger'));
    expect(h1.closest('header')).toHaveTextContent(`${cell.txHash}:1`);
    // the head writes the out point twice: short in the h1, whole on the id line, and no third
    // form of its transaction's hash
    const head = h1.closest('header')!;
    expect(head).not.toHaveTextContent(/output 1 of transaction/);
    expect(head.textContent!.split(cell.txHash).length - 1).toBe(1);
    // its block is the strip's link
    expect(
      within(screen.getByRole('group', { name: 'Cell status' })).getByRole('link', {
        name: '#20,801,533',
      })
    ).toHaveAttribute('href', '/mainnet/blocks/20801533');
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
    expect(facts).toHaveTextContent('Unspent for3d 10h');
    expect(facts).toHaveTextContent('as of #20,834,844');
    expect(within(facts).getByRole('link', { name: '#20,834,844' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20834844'
    );
    // Lane L: the lock's owner, the cell's address, is named and linked under the lock.
    // R9 (re-pinned): at the row size, `ckb1…` and the last 8 characters (it was 10…6)
    const owner = within(facts).getByRole('link', { name: 'ckb1…t0shp3hr' });
    expect(owner).toHaveAttribute('href', `/mainnet/address/${cell.address}`);
    expect(owner).toHaveAttribute('title', cell.address);
  });

  it('shows the lock and type by name, never a raw code hash (SC-C4)', async () => {
    view();
    const facts = await screen.findByRole('region', { name: 'Cell facts' });
    expect(await within(facts).findByRole('link', { name: 'Nervape Shadow Lock' })).toHaveAttribute(
      'href',
      `/mainnet/script/${cell.lock!.codeHash}?hashType=data1`
    );
    expect(within(facts).getByRole('link', { name: 'Spore' })).toBeInTheDocument();
    expect(facts.textContent).not.toContain(cell.lock!.codeHash);
    // each script by its pair (lane rd/api-lock-names)
    expect(api.lookupScripts).toHaveBeenCalledWith(
      [
        { codeHash: cell.lock!.codeHash, hashType: cell.lock!.hashType },
        { codeHash: cell.type!.codeHash, hashType: cell.type!.hashType },
      ],
      cell.txHash
    );
    expect(transactionsProps).toHaveBeenLastCalledWith(
      expect.objectContaining({ lockName: 'Nervape Shadow Lock', typeName: 'Spore' })
    );
  });

  it('is one microscope: grouped pills, one hex box with both blocks, no view switch (SC-C5)', async () => {
    view();
    await screen.findByRole('link', { name: 'Spore' });
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
    const box = await screen.findByLabelText('Cell bytes');
    expect(within(box).getAllByText('0x0000')).toHaveLength(2);
    expect(within(box).getAllByText('0x0018').length).toBeGreaterThan(0);
    const dataBlock = within(box).getByLabelText('CellData byte map');
    expect(dataBlock.textContent).toContain('dob/0');
  });

  it('links byte, pill, map and the sticky detail (SC-C5, SC-C6)', async () => {
    view();
    await screen.findByRole('link', { name: 'Nervape Shadow Lock' });
    const box = await screen.findByLabelText('Cell bytes');
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
        // Re-pinned on purpose (review of main..fix/readability, test finding 2): the API sends
        // every DAO cell's compensation with its state and block (F-9); this mock lacked them, and
        // the line it endorsed had no compensation. A DAO answer without them now fails.
        compensation: '2245001108',
        compensationCkb: '22.45001108',
        compensationState: 'claimed',
        compensationAtBlock: 20400000,
      },
    } as Cell);
    view();
    const dao = (await screen.findByText(/^deposit block/)).closest('p') as HTMLElement;
    // phase 5 (D1): the section's command runs on the status, said there and not in the line
    expect(screen.getByRole('heading', { name: 'Nervos DAO withdrawn' })).toBeInTheDocument();
    expect(dao).not.toHaveTextContent('withdrawn');
    expect(dao).toHaveTextContent('deposit block #20,311,586');
    expect(dao).toHaveTextContent(
      /^deposit block #20,311,586 · compensation 22\.45001108 CKB, claimed at block #20,400,000$/
    );
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
      [scriptLookupKey(cell.lock!.codeHash, cell.lock!.hashType)]: { name: 'Nervape Shadow Lock' },
      [scriptLookupKey(cell.type!.codeHash, cell.type!.hashType)]: { name: 'Spore' },
    } as never);
    mockObjectReads();
  });

  // Re-pinned on purpose (lane D2; renamed, it was "…: epoch under Created, …"): the capacity,
  // the created time and block went to the status strip under the id line, so the epoch it was
  // created in is a fact of its own (it was the Created fact's second line, under the time)
  it('keeps the board facts on one strip: the epoch its own fact, "0 free", no Owner (SC-C9)', async () => {
    view();
    const facts = await screen.findByRole('region', { name: 'Cell facts' });
    expect(within(facts).getByText('Epoch').parentElement).toHaveTextContent(
      /^Epoch15,157 · 112\/1800$/
    );
    expect(facts).not.toHaveTextContent('2026-10-22 22:43:55 UTC+8');
    expect(facts).toHaveTextContent('Occupied320 B · 0 free');
    expect(within(facts).queryByText('Owner')).toBeNull();
  });

  it('shows a missing createdEpoch as an error in place, not as a blank (SC-C9)', async () => {
    const { createdEpoch: _drop, ...withoutEpoch } = cell as Cell & { createdEpoch: unknown };
    void _drop;
    vi.mocked(api.getCell).mockResolvedValue(withoutEpoch as Cell);
    view();
    expect(await screen.findByText(/has no createdEpoch/)).toHaveAttribute('role', 'alert');
  });

  it('gives the data its hash formula and value in the detail (SC-C6)', async () => {
    view();
    const map = await screen.findByRole('group', { name: 'Capacity map' });
    fireEvent.mouseEnter(within(map).getByTitle('data · 181 bytes'));
    const detail = screen.getByRole('complementary', { name: 'Field detail' });
    expect(detail).toHaveTextContent('data hashblake2b(data)');
    expect(detail).toHaveTextContent(
      '0x550fe383bf357f4eb3b1853f324f9800dd3e49ac792d892254b646f91b861df1'
    );
  });

  it('pads a short last hex row with bytes of the same width, so ASCII columns align', async () => {
    view();
    const box = await screen.findByLabelText('Cell bytes');
    const pads = box.querySelectorAll('[data-pad]');
    expect(pads.length).toBeGreaterThan(0);
    for (const pad of pads) expect(pad).toHaveClass('px-px');
  });
});
