// Two sheets and the seam (seals by kind, C11). The user's Gate C on the transaction page's cells:
// "布局需要优化，看着也不舒服". The approved board drew a transaction as two sheets of one contract —
// what it consumed | what it created — 4 px apart, each with a three-line head; the build had kept
// the old single frame of three columns, the middle one a 112 px "Capacity pool" bar. The sheets
// replace the frame and the pool: the seam is where capacity crosses.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { continuityColor, SHEETS, TxStructure } from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';
import { canvasLines, seamLines, seamOverlay } from '@/__tests__/utils/tx-lines';

const TX = `0x${'5e'.repeat(32)}`;

const tile = (id: string, capacity = '20000000000'): CellTileData =>
  sealedTile({
    id,
    testId: id,
    href: `/cell/${id}`,
    label: id,
    owner: 'owner',
    lockKind: null,
    kind: 'ckb',
    content: 'CKB',
    capacity,
    occupied: '6100000000',
    state: id.startsWith('in') ? 'consumed' : 'live',
  });

type Props = Partial<React.ComponentProps<typeof TxStructure>>;
function structure(props: Props = {}) {
  return render(
    <TxStructure
      txHash={TX}
      inputs={[tile('in:0')]}
      outputs={[tile('out:0'), tile('out:1', '19999990000')]}
      fee="10000"
      {...props}
    />
  );
}

/** The canvas: the consumed sheet, the seam, the created sheet (and the onward bridge). */
function canvas(container: HTMLElement) {
  const root = container.querySelector<HTMLElement>('[data-tx-canvas]')!;
  const [consumed, seam, created, ...rest] = Array.from(root.children) as HTMLElement[];
  expect(consumed).toHaveAttribute('data-sheet', 'consumed');
  expect(seam).toHaveAttribute('data-seam');
  expect(created).toHaveAttribute('data-sheet', 'created');
  return { root, consumed, seam, created, rest };
}

/** A sheet's rows: its tiles, the inflow source before them, the fee after them. */
function sheetRows(sheet: HTMLElement) {
  const list = sheet.querySelector<HTMLElement>('[data-sheet-rows]')!;
  return Array.from(list.children) as HTMLElement[];
}

/** jsdom has no layout: each measured element's top and height, by its cell id or row kind. */
function layout(tops: Record<string, [number, number]>) {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement
  ) {
    const key =
      this.dataset.cellId ??
      (this.dataset.inflow !== undefined ? 'inflow' : this.dataset.fee !== undefined ? 'fee' : '');
    const [top, height] = tops[key] ?? [0, 0];
    return {
      top,
      height,
      bottom: top + height,
      left: 0,
      right: 0,
      width: 0,
      x: 0,
      y: top,
      toJSON: () => ({}),
    } as DOMRect;
  });
}

describe('TxStructure: two sheets and the seam (C11)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('lays the cells on two sheets with a 4 px seam between, and no capacity pool column', () => {
    const { container } = structure();
    const group = screen.getByRole('group', { name: 'Cells consumed and created' });
    const { root, consumed, seam, created, rest } = canvas(container);
    expect(group).toBe(root);
    expect(rest).toHaveLength(0);
    // each sheet its own instrument frame, the page ground showing through the seam between them
    for (const sheet of [consumed, created]) {
      expect(sheet).toHaveAttribute('data-instrument-frame');
      expect(sheet).toHaveClass('glass', 'rounded-lg', 'border', 'min-w-0');
    }
    expect(SHEETS.seam).toBe(4);
    expect(root).toHaveClass('grid', 'grid-cols-1', 'gap-y-3', 'lg:gap-y-0');
    expect(root).toHaveClass('lg:grid-cols-[minmax(0,1fr)_4px_minmax(0,1fr)]');
    // the seam: empty ground (round 3: with no continuity link, not even an overlay); none below lg
    expect(seam).toHaveClass('relative', 'hidden', 'self-stretch', 'lg:block');
    expect(seam).not.toHaveAttribute('data-instrument-frame');
    expect(seam.children).toHaveLength(0);
    // the cells on their sheets
    expect(
      within(consumed)
        .getAllByRole('article')
        .map((a) => a.dataset.cellId)
    ).toEqual(['in:0']);
    expect(
      within(created)
        .getAllByRole('article')
        .map((a) => a.dataset.cellId)
    ).toEqual(['out:0', 'out:1']);
    // the pool is gone: no column, no bar, no label, no 112 px track
    expect(container).not.toHaveTextContent(/pool/i);
    expect(seam.querySelector('rect')).toBeNull();
    expect(container.innerHTML).not.toContain('112px');
  });

  it('heads each sheet in three lines: the label, the count, the exact total; the created head mirrored from lg', () => {
    const { container } = structure({
      consumedCapacity: '20000000000',
      createdCapacity: '39999990000',
    });
    const { consumed, created } = canvas(container);
    for (const [sheet, side, count, total] of [
      [consumed, 'consumed', '1 cell', /^200(\.0+)? CKB$/],
      [created, 'created', '2 cells', /^399\.9999 ?(0+)? CKB$/],
    ] as const) {
      const head = sheet.querySelector<HTMLElement>('[data-sheet-head]')!;
      expect(head).toHaveAttribute('data-sheet-head', side);
      // the head leads its sheet, one h3 per sheet, named as the old column heads were
      const content = sheet.querySelector('[data-sheet-content]')!;
      expect(content.firstElementChild).toBe(head);
      const h3 = within(sheet).getByRole('heading', { level: 3 });
      expect(h3).toHaveAccessibleName(`${count} ${side}`);
      expect(head).toContainElement(h3);
      // line 1, the eyebrow label (first on screen, after the count in the name); line 2, the count
      const [figure, label] = Array.from(h3.children) as HTMLElement[];
      expect(label).toHaveTextContent(new RegExp(`^${side}$`));
      expect(label).toHaveClass('eyebrow', 'text-meta', 'text-text-dim', 'order-first');
      expect(figure).toHaveTextContent(new RegExp(`^${count}$`));
      expect(figure).toHaveClass('text-section', 'text-text-bright');
      expect(h3).toHaveClass('flex', 'flex-col');
      // line 3, the API's exact total of the sheet's cells
      const totals = head.querySelectorAll<HTMLElement>('[data-sheet-total]');
      expect(totals).toHaveLength(1);
      expect(totals[0]).toHaveTextContent(total);
      expect(totals[0].querySelector('[data-ckb-amount]')).not.toBeNull();
      expect(h3).not.toContainElement(totals[0]);
    }
    // consumed reads from the left; created, from lg, from the right (both left below lg)
    const consumedHead = consumed.querySelector('[data-sheet-head]')!;
    const createdHead = created.querySelector('[data-sheet-head]')!;
    expect(consumedHead.className).not.toMatch(/lg:items-end|lg:text-right/);
    expect(createdHead).toHaveClass('lg:items-end', 'lg:text-right');
    expect(within(created).getByRole('heading')).toHaveClass('lg:items-end');
    expect(createdHead.className).not.toMatch(/(^|\s)(items-end|text-right)/);
  });

  it('draws a total only from the exact sum the caller supplies, never from the tiles', () => {
    const { container, unmount } = structure();
    expect(container.querySelector('[data-sheet-total]')).toBeNull();
    // the heads keep their two lines and their names
    expect(screen.getByRole('heading', { name: '1 cell consumed' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '2 cells created' })).toBeInTheDocument();
    unmount();
    // a supplied total is the API's, drawn as given even where the tiles would sum otherwise
    const other = structure({ consumedCapacity: '1', createdCapacity: '0' });
    const totals = other.container.querySelectorAll('[data-sheet-total]');
    expect(totals).toHaveLength(2);
    expect(totals[0]).toHaveTextContent(/^0\.00000001 CKB$/);
    expect(totals[1]).toHaveTextContent(/^0 CKB$/);
  });

  it('refuses a total that is not an exact unsigned integer, naming the transaction and the sheet', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => structure({ consumedCapacity: '200.5', createdCapacity: '1' })).toThrow(
      `${TX}: consumed capacity: expected a non-negative decimal integer, received "200.5"`
    );
    expect(() => structure({ consumedCapacity: '1', createdCapacity: '-3' })).toThrow(
      `${TX}: created capacity: expected a non-negative decimal integer, received "-3"`
    );
    quiet.mockRestore();
  });

  // Round 2 (the orchestrator's Chrome check of e20521ad): with lg:items-start the shorter sheet
  // stopped early — on a 30-in-1-out transaction the created sheet ended after its tile and the
  // fee, so 29 connectors met an edge with nothing beyond it and the seam ran 250 px. Two sheets
  // of one contract are one size: from lg both stretch to the taller, the seam runs the whole
  // height, and the inner corners are square so the pair reads as one frame cut by the seam.
  it('makes the two sheets one size from lg, square on the seam side, round outside', () => {
    const { container } = structure({
      inputs: Array.from({ length: 30 }, (_, i) => tile(`in:${i}`)),
      outputs: [tile('out:0', '599999990000')],
    });
    const { root, consumed, seam, created } = canvas(container);
    // stretched, never top-aligned: every item fills the row (below lg each row is its own)
    expect(root).toHaveClass('items-stretch');
    expect(root.className).not.toMatch(/items-start/);
    expect(seam).toHaveClass('self-stretch');
    for (const sheet of [consumed, created])
      expect(sheet.className).not.toMatch(/self-(start|center|end)/);
    // the panel's 8 px radius outside; from lg the seam-side corners square
    expect(consumed).toHaveClass('rounded-lg', 'lg:rounded-r-none');
    expect(consumed.className).not.toMatch(/rounded-l-none/);
    expect(created).toHaveClass('rounded-lg', 'lg:rounded-l-none');
    expect(created.className).not.toMatch(/rounded-r-none/);
    // below lg nothing squares a corner
    for (const sheet of [consumed, created])
      expect(sheet.className).not.toMatch(/(^|\s)rounded-[lr]-none/);
    // the fee stays directly under the last created tile, not pushed to the sheet's floor
    const rows = sheetRows(created);
    expect(rows.map((r) => r.dataset.cellId ?? (r.dataset.fee !== undefined ? 'fee' : ''))).toEqual(
      ['out:0', 'fee']
    );
    const list = created.querySelector<HTMLElement>('[data-sheet-rows]')!;
    expect(list.className).not.toMatch(/flex|justify-|grow|h-full/);
    expect(rows[1].className).not.toMatch(/mt-auto|self-end/);
  });

  it('keeps a 36 px lane on each sheet’s seam side and pads the outer side as before', () => {
    expect(SHEETS.lane).toBe(36);
    const { container } = structure();
    const { consumed, created } = canvas(container);
    // 35 px of padding and the sheet's 1 px border: a tile ends 36 px before the seam
    const content = (sheet: HTMLElement) =>
      sheet.querySelector<HTMLElement>('[data-sheet-content]')!;
    expect(content(consumed)).toHaveClass('p-3', 'lg:pr-[35px]');
    expect(content(consumed).className).not.toContain('lg:pl-');
    expect(content(created)).toHaveClass('p-3', 'lg:pl-[35px]');
    expect(content(created).className).not.toContain('lg:pr-');
    // the rows end 12 px above the sheet's padding edge; a shorter sheet's paper runs on below
    for (const sheet of [consumed, created])
      expect(content(sheet).className).not.toMatch(/(^|\s)(lg:)?pb-/);
  });

  it('closes the created sheet with the fee, a dashed row with its exact amount, at every width', () => {
    const { container } = structure({ fee: '64119' });
    const { consumed, created } = canvas(container);
    const rows = sheetRows(created);
    const fee = rows.at(-1)!;
    expect(rows.map((r) => r.dataset.cellId ?? (r.dataset.fee !== undefined ? 'fee' : ''))).toEqual(
      ['out:0', 'out:1', 'fee']
    );
    expect(fee).toBe(screen.getByTestId('tx-fee'));
    expect(fee).toHaveClass('border', 'border-dashed', 'border-text-dim/60', 'rounded');
    expect(fee.firstElementChild).toHaveTextContent(/^fee$/);
    expect(fee).toHaveTextContent(/^fee0\.00064119 CKB$/);
    // not hidden at any width, and once: the consumed sheet holds none
    expect(fee.className).not.toMatch(/hidden/);
    expect(consumed.querySelector('[data-fee]')).toBeNull();
    expect(container.querySelectorAll('[data-fee]')).toHaveLength(1);
    expect(within(container).getAllByText('fee')).toHaveLength(1);
  });

  it('shows a fee of 0, as the fee always showed', () => {
    structure({ inputs: [], fee: '0' });
    expect(screen.getByTestId('tx-fee')).toHaveTextContent(/^fee0 CKB$/);
  });

  it('refuses a fee that is not an exact unsigned integer', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => structure({ fee: '1e3' })).toThrow(
      `${TX}: fee: expected a non-negative decimal integer, received "1e3"`
    );
    quiet.mockRestore();
  });

  // Round 3 (the user on tx 0x8f86…17ea, 5 in → 5 out: "这些 ins/outs 真的是 1:1 对应的吗?"): with
  // the pool bar gone, the solid stubs tile → seam and seam → tile read as one line joining input
  // N to output N where the tiles stood level, and as a broken line where they did not. A
  // transaction pools capacity: no input is any output's source, and that transaction's real
  // correspondences crossed the rows. A line across the seam now says one thing only — this
  // consumed cell continues as that created cell — so a transaction with no continuity draws none.
  it('draws no overlay and no line between the sheets when no cell continues', () => {
    layout({
      inflow: [13, 54],
      'in:0': [79, 98],
      'out:0': [13, 98],
      'out:1': [123, 98],
      fee: [233, 54],
    });
    const { container } = structure({
      inflow: { tone: 'dao', label: 'Nervos DAO compensation', amount: '2572859558410' },
    });
    const { consumed, seam, created } = canvas(container);
    // the seam is bare ground: no image, no path, no label
    expect(seam.children).toHaveLength(0);
    expect(seamOverlay(container)).toBeNull();
    expect(canvasLines(container)).toHaveLength(0);
    expect(screen.queryByLabelText(/continue|flow/i)).toBeNull();
    // no connector anywhere: not from the tiles, not from the inflow source, not to the fee
    expect(container.querySelectorAll('path[data-continuity-kind]')).toHaveLength(0);
    expect(container.querySelector('[data-inflow-path], [data-fee-path]')).toBeNull();
    // the rows themselves stay as they were
    expect(sheetRows(consumed)[0]).toBe(screen.getByTestId('tx-inflow'));
    expect(screen.getByTestId('tx-inflow')).toHaveClass('border-dashed', 'border-gold/60');
    expect(sheetRows(created).at(-1)).toBe(screen.getByTestId('tx-fee'));
    expect(screen.getByTestId('tx-fee')).toHaveClass('border-dashed', 'border-text-dim/60');
    // the lanes stay: a tile's width never depends on whether the transaction has a link
    expect(consumed.querySelector('[data-sheet-content]')).toHaveClass('lg:pr-[35px]');
    expect(created.querySelector('[data-sheet-content]')).toHaveClass('lg:pl-[35px]');
  });

  it('draws one dashed path per continuity link, tile edge to tile edge, and nothing else', () => {
    layout({
      inflow: [13, 54],
      'in:0': [79, 98],
      'in:1': [189, 98],
      'out:0': [13, 98],
      'out:1': [123, 98],
      fee: [233, 54],
    });
    const { container } = structure({
      inputs: [tile('in:0'), tile('in:1')],
      inflow: { tone: 'dao', label: 'Nervos DAO compensation', amount: '2572859558410' },
      continuity: [
        { from: 'in:0', to: 'out:1', label: 'USDI', kind: 'tokens' },
        { from: 'in:1', to: 'out:0', label: 'same type id continues', kind: 'type-id' },
      ],
    });
    const { seam } = canvas(container);
    expect(seam.children).toHaveLength(1);
    const overlay = within(seam).getByLabelText('Cells that continue from consumed to created');
    expect(overlay.tagName.toLowerCase()).toBe('svg');
    // 36 + 4 + 36 px, from the consumed tiles' edge to the created tiles' edge, over the cells
    expect(overlay).toHaveClass(
      'pointer-events-none',
      'absolute',
      'inset-y-0',
      '-left-9',
      'w-[76px]',
      'h-full',
      'overflow-visible',
      'z-20'
    );
    // exactly one path per link, in the links' order, crossing both lanes and the seam
    const paths = Array.from(overlay.querySelectorAll('path'));
    expect(paths.map((p) => [p.getAttribute('data-continuity-kind'), p.getAttribute('d')])).toEqual(
      [
        ['tokens', 'M0 128C30 128 46 172 76 172'],
        ['type-id', 'M0 238C30 238 46 62 76 62'],
      ]
    );
    expect(paths[0]).toHaveAttribute('stroke', continuityColor('tokens'));
    expect(paths[1]).toHaveAttribute('stroke', continuityColor('type-id'));
    for (const path of paths) {
      expect(path).toHaveAttribute('stroke-dasharray', '4 3');
      expect(path).toHaveAttribute('fill', 'none');
      expect(path).toHaveAttribute('stroke-width', '1.3');
    }
    // nothing else in the overlay or anywhere between the sheets; no connector to the inflow or fee
    expect(overlay.children).toHaveLength(2);
    expect(canvasLines(container)).toEqual(paths);
    expect(container.querySelector('[data-inflow-path], [data-fee-path]')).toBeNull();
  });

  it('lights a link and both its tiles when either tile is hovered or focused', () => {
    layout({ 'in:0': [79, 98], 'in:1': [189, 98], 'out:0': [13, 98], 'out:1': [123, 98] });
    const { container } = structure({
      inputs: [tile('in:0'), tile('in:1')],
      continuity: [{ from: 'in:0', to: 'out:1', label: 'USDI', kind: 'tokens' }],
    });
    const [link] = seamLines(container);
    const article = (id: string) => container.querySelector(`[data-cell-id="${id}"]`)!;
    fireEvent.mouseEnter(article('in:0'));
    expect(link).toHaveAttribute('stroke-width', '2');
    expect(article('in:0')).toHaveAttribute('data-highlighted', 'true');
    expect(article('out:1')).toHaveAttribute('data-highlighted', 'true');
    expect(article('in:1')).toHaveClass('opacity-30');
    fireEvent.mouseLeave(article('in:0'));
    expect(link).toHaveAttribute('stroke-width', '1.3');
    expect(article('out:1')).toHaveAttribute('data-highlighted', 'false');
    fireEvent.focus(article('out:1'));
    expect(link).toHaveAttribute('stroke-width', '2');
    expect(article('in:0')).toHaveAttribute('data-highlighted', 'true');
    // a tile with no link lights alone and draws nothing
    fireEvent.blur(article('out:1'));
    fireEvent.mouseEnter(article('out:0'));
    expect(article('out:0')).toHaveAttribute('data-highlighted', 'true');
    expect(link).toHaveAttribute('stroke-width', '1.3');
    expect(seamLines(container)).toHaveLength(1);
  });
});

describe('TxStructure onward bridge on the sheets (C11)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('keeps the 96 px bridge right of the created sheet, from the followed tile’s edge out of the sheet', () => {
    layout({ 'in:0': [13, 98], 'out:0': [13, 98], 'out:1': [123, 98] });
    const { container } = structure({ meOut: 'out:1', onward: 'the channel, onward' });
    const { root, created, rest } = canvas(container);
    expect(root).toHaveClass('lg:grid-cols-[minmax(0,1fr)_4px_minmax(0,1fr)_96px]');
    expect(rest).toHaveLength(1);
    const [bridge] = rest;
    expect(created.nextElementSibling).toBe(bridge);
    expect(bridge).toHaveAttribute('data-onward-bridge');
    expect(bridge).toHaveAttribute('data-onward-from', 'out:1');
    expect(bridge).toHaveAttribute('aria-hidden', 'true');
    expect(bridge).toHaveClass('relative', 'hidden', 'self-stretch', 'lg:block');
    const svg = bridge.querySelector('svg')!;
    expect(svg).toHaveClass('absolute', 'inset-0', 'overflow-visible', 'z-20');
    // from the tile's right edge (the sheet's 12 px padding and 1 px border inside it) to 96
    const path = svg.querySelector('path')!;
    expect(path).toHaveAttribute('d', 'M-13 172H96');
    expect(path).toHaveAttribute('stroke-dasharray', '3 3');
    expect(screen.getByText('the channel, onward')).toBeInTheDocument();
    expect(container.querySelector('[data-cell-id="out:1"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    // round 3: no connector stub to the followed tile (none to any tile); with no continuity the
    // bridge is the canvas's only line
    expect(seamOverlay(container)).toBeNull();
    expect(canvasLines(container)).toEqual([path]);
  });
});
