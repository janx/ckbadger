// Two sheets and the seam (seals by kind, C11). The user's Gate C on the transaction page's cells:
// "布局需要优化，看着也不舒服". The approved board drew a transaction as two sheets of one contract —
// what it consumed | what it created — 4 px apart, each with a three-line head; the build had kept
// the old single frame of three columns, the middle one a 112 px "Capacity pool" bar. The sheets
// replace the frame and the pool: the seam is where capacity crosses.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { SHEETS, TxStructure } from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';

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
    expect(root).toHaveClass('grid', 'grid-cols-1', 'gap-y-3', 'lg:items-start', 'lg:gap-y-0');
    expect(root).toHaveClass('lg:grid-cols-[minmax(0,1fr)_4px_minmax(0,1fr)]');
    // the seam: empty ground, only the flows' overlay drawn over it; none below lg
    expect(seam).toHaveClass('relative', 'hidden', 'self-stretch', 'lg:block');
    expect(seam).not.toHaveAttribute('data-instrument-frame');
    expect(seam.children).toHaveLength(1);
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
    expect(container).not.toHaveTextContent(/capacity pool/i);
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
    // no dead floor: the sheet ends 12 px under its last row
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

  it('draws every flow in one overlay centred on the seam: connectors, inflow, fee and continuity', () => {
    layout({
      inflow: [13, 54],
      'in:0': [79, 98],
      'out:0': [13, 98],
      'out:1': [123, 98],
      fee: [233, 54],
    });
    const { container } = structure({
      inflow: { tone: 'dao', label: 'Nervos DAO compensation', amount: '2572859558410' },
      continuity: [{ from: 'in:0', to: 'out:1', label: 'USDI', kind: 'tokens' }],
    });
    const { seam } = canvas(container);
    const overlay = within(seam).getByLabelText(
      'Capacity flows from the consumed cells to the created ones'
    );
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
    const d = (path: Element) => path.getAttribute('d');
    const paths = Array.from(overlay.querySelectorAll('path'));
    // the consumed tile to its sheet's edge (0 → 36); the created sheet's edge to its tiles (40 → 76)
    const solid = paths.filter((p) => !p.hasAttribute('stroke-dasharray'));
    expect(solid.map(d)).toEqual(['M0 128H36', 'M40 62H76', 'M40 172H76']);
    for (const path of solid) {
      expect(path).toHaveAttribute('stroke', 'var(--c-ink-quiet)');
      expect(path).toHaveAttribute('stroke-width', '1.5');
    }
    // the inflow joins from its source, dashed gold; the fee leaves to its row, dashed dim
    const inflow = overlay.querySelector('[data-inflow-path]')!;
    expect(inflow).toHaveAttribute('data-inflow-path', 'dao');
    expect(d(inflow)).toBe('M0 40H36');
    expect(inflow).toHaveClass('stroke-gold');
    expect(inflow).toHaveAttribute('stroke-dasharray', '4 3');
    const fee = overlay.querySelector('[data-fee-path]')!;
    expect(d(fee)).toBe('M40 260H76');
    expect(fee).toHaveClass('stroke-text-dim');
    expect(fee).toHaveAttribute('stroke-dasharray', '4 3');
    // the continuity link crosses both lanes and the seam, tile edge to tile edge
    const link = overlay.querySelector('[data-continuity-kind]')!;
    expect(link).toHaveAttribute('data-continuity-kind', 'tokens');
    expect(d(link)).toBe('M0 128C30 128 46 172 76 172');
    expect(link).toHaveAttribute('stroke-dasharray', '4 3');
    expect(link).toHaveAttribute('fill', 'none');
    // nothing else: no pool, no stub under it
    expect(paths).toHaveLength(6);
    expect(overlay.querySelector('rect')).toBeNull();
  });

  it('draws a reward inflow dim and lights a linked cell’s connector', () => {
    layout({ inflow: [13, 54], 'out:0': [13, 98], 'out:1': [123, 98], fee: [233, 54] });
    const { container } = structure({
      inputs: [],
      inflow: { tone: 'reward', label: 'block reward', amount: '55961739927' },
      focusedCellIds: ['out:1'],
    });
    const { consumed, seam } = canvas(container);
    // the source is the consumed sheet's first row
    expect(sheetRows(consumed)[0]).toBe(screen.getByTestId('tx-inflow'));
    const overlay = seam.querySelector('svg')!;
    expect(overlay.querySelector('[data-inflow-path="reward"]')).toHaveClass('stroke-text-dim');
    const lit = Array.from(overlay.querySelectorAll('path')).find(
      (p) => p.getAttribute('d') === 'M40 172H76'
    );
    expect(lit).toHaveAttribute('stroke', 'var(--c-text-bright)');
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
    // its connector stays lit
    const lit = Array.from(container.querySelectorAll('[data-seam] path')).find(
      (p) => p.getAttribute('d') === 'M40 172H76'
    );
    expect(lit).toHaveAttribute('stroke', 'var(--c-text-bright)');
  });
});
