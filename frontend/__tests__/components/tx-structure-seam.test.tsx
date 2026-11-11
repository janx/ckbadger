import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { SEAM_SEAL, TxStructure, type TxSealId } from '@/components/transaction/tx-structure';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import type { CellTileData } from '@/components/ui/cell-tile';
import { drawSeal, type SealState } from '@/lib/seal/stones';
import { expectBefore, parsed, SEAL_INKS, sealHash, sealIdsAt } from '@/__tests__/utils/seals';

const TX = sealHash('seam');
const ID: TxSealId = { kind: 'tx', hash: TX, consumed: 1, created: 2 };

const tile = (id: string): CellTileData => ({
  id,
  testId: id,
  href: `/cell/${id}`,
  label: id,
  owner: 'owner',
  lockKind: null,
  kind: 'ckb',
  content: 'CKB',
  capacity: '20000000000',
  occupied: '6100000000',
  state: id.startsWith('in') ? 'consumed' : 'live',
});

function structure(seamSeal?: { id: TxSealId; state?: SealState }) {
  return render(
    <TxStructure
      txHash={TX}
      inputs={[tile('in:0')]}
      outputs={[tile('out:0'), tile('out:1')]}
      fee="10000"
      seamSeal={seamSeal}
    />
  );
}

/** The canvas's three columns: consumed, the capacity pool, created. */
function columns(container: HTMLElement) {
  const canvas = container.querySelector('[data-tx-canvas]')!;
  expect(canvas.children).toHaveLength(3);
  return Array.from(canvas.children) as HTMLElement[];
}

describe('TxStructure seam seal (the seals by kind, C2)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('presses none unless the page gives one: every other page keeps its structure', () => {
    const { container } = structure();
    expect(container.querySelector('[data-seam-seal]')).toBeNull();
    expect(container.querySelector('[data-seal]')).toBeNull();
    const [consumed, pool, created] = columns(container);
    expect(within(pool).getByText('Capacity pool')).toHaveClass('lg:absolute', 'lg:top-0');
    for (const side of [consumed, created]) {
      const head = side.querySelector('h3')!;
      expect(head).toHaveClass('mb-3');
      expect(head).not.toHaveClass('lg:mb-0');
      expect(head.className).not.toContain('lg:min-h-');
    }
  });

  it('presses the transaction’s stone at the head of the capacity pool column, from lg, by exact markup', () => {
    const { container } = structure({ id: ID });
    const holders = container.querySelectorAll<HTMLElement>('[data-seam-seal]');
    expect(holders).toHaveLength(1);
    const [holder] = Array.from(holders);
    const [consumed, pool, created] = columns(container);
    // the head of the middle column, centred across its 112 px; none below lg (display: none)
    expect(pool.firstElementChild).toBe(holder);
    expect(holder).toHaveClass('absolute', 'inset-x-0', 'top-0', 'justify-center');
    expect(holder).toHaveClass('hidden', 'lg:flex');
    expect(holder).toHaveAttribute('aria-hidden', 'true');
    const seals = holder.querySelectorAll<HTMLElement>('[data-seal]');
    expect(seals).toHaveLength(1);
    const [seal] = Array.from(seals);
    expect(seal).toHaveAttribute('data-seal', 'tx');
    expect(seal).toHaveAttribute('data-seal-form', 'page');
    expect(seal).toHaveAttribute('data-seal-state', 'pressed');
    expect(seal).toHaveClass('[&>svg]:block');
    // the header's stone (its id, its state, its −4°) at the narrow page height, no seam gap
    expect(seal.innerHTML).toBe(
      parsed(
        drawSeal(ID, {
          form: 'page',
          height: 64,
          state: 'pressed',
          ...SEAL_INKS,
          tilt: PAGE_SEAL_TILT,
        }).svg
      )
    );
    const svg = seal.querySelector('svg')!;
    expect(svg).toHaveAttribute('height', '64');
    expect(svg).toHaveAttribute('width', '104.5');
    expect(svg).toHaveAttribute(
      'aria-label',
      `Transaction seal ${TX.slice(0, 10)}, 1 consumed, 2 created`
    );
    expect(seal.closest('a')).toBeNull();
    expect(seal.querySelector('a')).toBeNull();
    // the label moves under it, and both cell columns start under the band
    const label = within(pool).getByText('Capacity pool');
    expectBefore(seal, label);
    expect(label).toHaveClass('lg:absolute', 'lg:inset-x-0', `lg:top-[${SEAM_SEAL.labelTop}px]`);
    expect(label).not.toHaveClass('lg:top-0');
    for (const side of [consumed, created]) {
      // below lg the head keeps its 12 px margin; from lg it holds the band
      expect(side.querySelector('h3')).toHaveClass(
        'mb-3',
        'lg:mb-0',
        `lg:min-h-[${SEAM_SEAL.band}px]`
      );
    }
    expect(consumed).toHaveTextContent('1 cell consumed');
    expect(created).toHaveTextContent('2 cells created');
  });

  it('presses it in the state the page gives: dashed for a transaction not yet in a block', () => {
    for (const state of ['unpressed', 'ink'] as const) {
      const { container, unmount } = structure({ id: ID, state });
      const seal = container.querySelector<HTMLElement>('[data-seam-seal] [data-seal]')!;
      expect(seal).toHaveAttribute('data-seal-state', state);
      expect(seal.innerHTML).toBe(
        parsed(
          drawSeal(ID, { form: 'page', height: 64, state, ...SEAL_INKS, tilt: PAGE_SEAL_TILT }).svg
        )
      );
      unmount();
    }
  });

  // The band's numbers, from the stone as drawn: 104.5 × 64 for every transaction, tilted −4°.
  it('fits the 112 px column and keeps the label and every flow under the stone', () => {
    expect(SEAM_SEAL.height).toBe(PAGE_SEAL_HEIGHTS.tx.narrow);
    expect(SEAM_SEAL.height).toBe(64);
    const ids: TxSealId[] = [
      ...Array.from({ length: 24 }, (_, i) => sealIdsAt(i).tx),
      { kind: 'tx', hash: TX, consumed: 0, created: 1 },
      { kind: 'tx', hash: TX, consumed: 13000, created: 1 },
      { kind: 'tx', hash: TX, consumed: null, created: null },
    ];
    for (const id of ids) {
      const drawn = drawSeal(id, {
        form: 'page',
        height: 64,
        state: 'pressed',
        ...SEAL_INKS,
        tilt: PAGE_SEAL_TILT,
      });
      expect([drawn.width, drawn.height]).toEqual([104.5, 64]);
    }
    const [w, h] = [104.5, 64];
    const a = (Math.abs(PAGE_SEAL_TILT) * Math.PI) / 180;
    const across = w * Math.cos(a) + h * Math.sin(a);
    const tall = w * Math.sin(a) + h * Math.cos(a);
    expect(across).toBeCloseTo(108.71, 2);
    expect(tall).toBeCloseTo(71.13, 2);
    // inside the 112 px column at either side
    expect(across).toBeLessThan(112);
    // its lowest corner (h/2 + tall/2 from the column's top) is above the label
    const lowest = h / 2 + tall / 2;
    expect(lowest).toBeCloseTo(67.57, 2);
    expect(lowest).toBeLessThan(SEAM_SEAL.labelTop);
    // the label, 11 px at 1.5 (16.5 px), ends before the pool starts: the pool's top is 16 px
    // above the first cell's middle, and the shortest first tile is the inflow source, 54 px
    // (two 18 px lines, 8 px padding above and below, a 1 px border)
    const labelBottom = SEAM_SEAL.labelTop + 16.5;
    const shortestTile = 2 * 18 + 2 * 8 + 2 * 1;
    expect(shortestTile).toBe(54);
    expect(SEAM_SEAL.band + shortestTile / 2 - 16).toBeGreaterThan(labelBottom);
  });

  // Laid out as from lg (jsdom has no layout): the side columns' cells under the 88 px band. The
  // flows and the pool are drawn from the measured middles, all under the label.
  it('draws the flows and the pool from the measured cells, under the stone and its label', () => {
    const tops: Record<string, [number, number]> = {
      'in:0': [SEAM_SEAL.band, 98],
      'out:0': [SEAM_SEAL.band, 98],
      'out:1': [SEAM_SEAL.band + 98 + 12, 98],
    };
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement
    ) {
      const [top, height] = tops[this.dataset.cellId ?? ''] ?? [0, 0];
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
    const { container } = structure({ id: ID });
    const flows = screen.getByLabelText('Cells flow through the capacity pool');
    const labelBottom = SEAM_SEAL.labelTop + 16.5;
    const rect = flows.querySelector('rect')!;
    // the first middles are 88 + 49 = 137: the pool from 121
    expect(Number(rect.getAttribute('y'))).toBe(121);
    expect(Number(rect.getAttribute('y'))).toBeGreaterThan(labelBottom);
    const ys = Array.from(flows.querySelectorAll('path')).map((p) =>
      Number(/^M\d+ ([\d.]+)/.exec(p.getAttribute('d')!)![1])
    );
    expect(ys.length).toBeGreaterThanOrEqual(3);
    for (const y of ys) expect(y).toBeGreaterThan(labelBottom);
    expect(container.querySelectorAll('[data-seam-seal]')).toHaveLength(1);
  });
});
