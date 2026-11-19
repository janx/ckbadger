// The seam seal (seals by kind; C2, rewritten for C11). C2 pressed the transaction's stone whole at
// the head of the 112 px capacity-pool column, over no seam at all — the user's Gate C: "骑缝印没有
// 骑缝的感觉". C11 lays the cells on two sheets with a 4 px seam between them, and the stone is
// pressed across that seam and pulled apart by it: its left half on the consumed sheet up to that
// sheet's edge, its right half on the created sheet from that sheet's edge, the ground between.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import {
  SEAM_SEAL,
  SHEETS,
  TxStructure,
  type TxSealId,
} from '@/components/transaction/tx-structure';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import type { CellTileData } from '@/components/ui/cell-tile';
import { drawSeal, type SealState } from '@/lib/seal/stones';
import { parsed, SEAL_INKS, sealHash, sealIdsAt } from '@/__tests__/utils/seals';
import { textPx } from '@/lib/type-metrics';
import { sealedTile } from '@/__tests__/utils/cell-tile-seal';

const TX = sealHash('seam');
const ID: TxSealId = { kind: 'tx', hash: TX, consumed: 1, created: 2 };

const tile = (id: string): CellTileData =>
  sealedTile({
    id,
    testId: id,
    href: `/cell/${id}`,
    label: id,
    owner: 'owner',
    lockKind: null,
    kind: 'ckb',
    content: 'CKB',
    capacity: '20000000000',
    occupiedBytes: '61',
    state: id.startsWith('in') ? 'consumed' : 'live',
  });

function structure(seamSeal?: { id: TxSealId; state?: SealState }) {
  return render(
    <TxStructure
      txHash={TX}
      inputs={[tile('in:0')]}
      outputs={[tile('out:0'), tile('out:1')]}
      fee="10000"
      consumedCapacity="20000000000"
      createdCapacity="40000000000"
      seamSeal={seamSeal}
    />
  );
}

/** The two sheets' heads, consumed then created. */
function heads(container: HTMLElement) {
  return ['consumed', 'created'].map(
    (side) => container.querySelector<HTMLElement>(`[data-sheet="${side}"] [data-sheet-head]`)!
  );
}

/** The stone as the page's head draws it, at the seam's height: the same id, state and tilt. */
const stone = (state: SealState = 'pressed', id: TxSealId = ID) =>
  parsed(drawSeal(id, { form: 'page', height: 64, state, ...SEAL_INKS, tilt: PAGE_SEAL_TILT }).svg);

describe('TxStructure seam seal: one stone pulled apart by the seam (C11)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('presses none unless the page gives one: every other page keeps its sheets', () => {
    const { container } = structure();
    expect(container.querySelector('[data-seam-seal]')).toBeNull();
    // no stone of the transaction: the only seals are the cells' own and their owners', two a tile
    expect(container.querySelector('[data-seal="tx"]')).toBeNull();
    expect(container.querySelectorAll('[data-seal]')).toHaveLength(
      2 * container.querySelectorAll('[data-cell-id]').length
    );
    // the heads hold no band and keep no clearance on the seam side
    for (const head of heads(container)) {
      expect(head).toHaveClass('mb-3');
      expect(head.className).not.toMatch(/lg:min-h-|lg:p[lr]-6/);
    }
  });

  it('presses the transaction’s stone in the seam, centred on it, as two halves of one stone', () => {
    const { container } = structure({ id: ID });
    const holders = container.querySelectorAll<HTMLElement>('[data-seam-seal]');
    expect(holders).toHaveLength(1);
    const [holder] = Array.from(holders);
    // in the 4 px seam between the sheets (hidden below lg, where the sheets stack), centred on
    // it across the heads' band, over the sheets, hidden from a screen reader (the head's stone
    // is the page's image) and from the pointer
    const seam = container.querySelector('[data-seam]')!;
    expect(holder.parentElement).toBe(seam);
    expect(seam).toHaveClass('hidden', 'lg:block');
    expect(holder).toHaveAttribute('aria-hidden', 'true');
    expect(holder).toHaveClass(
      'pointer-events-none',
      'absolute',
      'left-1/2',
      'top-0',
      '-translate-x-1/2',
      'z-20',
      'flex',
      `h-[${SEAM_SEAL.band}px]`,
      'items-center'
    );
    // one stone, two impressions: each the head's stone (its id, state, −4°) at 64 px, whole in
    // the markup, the seam made by clipping and moving, not by the library's tilted seam line
    const stoneBox = holder.firstElementChild as HTMLElement;
    expect(holder.children).toHaveLength(1);
    expect(stoneBox).toHaveClass('relative', 'flex');
    const halves = Array.from(stoneBox.children) as HTMLElement[];
    expect(halves.map((h) => h.getAttribute('data-seam-half'))).toEqual(['consumed', 'created']);
    for (const half of halves) {
      const seals = half.querySelectorAll<HTMLElement>('[data-seal]');
      expect(seals).toHaveLength(1);
      const [seal] = Array.from(seals);
      expect(half.firstElementChild).toBe(seal);
      expect(seal).toHaveAttribute('data-seal', 'tx');
      expect(seal).toHaveAttribute('data-seal-form', 'page');
      expect(seal).toHaveAttribute('data-seal-state', 'pressed');
      expect(seal).toHaveClass('[&>svg]:block');
      expect(seal.innerHTML).toBe(stone());
      const svg = seal.querySelector('svg')!;
      expect(svg).toHaveAttribute('height', '64');
      expect(svg).toHaveAttribute('width', '104.5');
      expect(svg).toHaveAttribute(
        'aria-label',
        `Transaction seal ${TX.slice(0, 10)}, 1 consumed, 2 created`
      );
      expect(seal.querySelector('a')).toBeNull();
      expect(half).toHaveClass('flex');
    }
    const [consumed, created] = halves;
    // the left half: clipped at the stone's middle (the tilt's overhang kept on the three outer
    // sides), moved 2 px left, so its cut edge is the consumed sheet's edge
    expect(consumed).toHaveClass('-translate-x-0.5', '[clip-path:inset(-8px_50%_-8px_-8px)]');
    // the right half: over the left, clipped from the middle, moved 2 px right
    expect(created).toHaveClass(
      'absolute',
      'inset-0',
      'translate-x-0.5',
      '[clip-path:inset(-8px_-8px_-8px_50%)]'
    );
    // 2 + 2 px: the seam's width, so nothing of the stone is lost in it
    expect(2 + 2).toBe(SHEETS.seam);
    // no link anywhere near it
    expect(holder.closest('a')).toBeNull();
  });

  it('presses it in the state the page gives: dashed for a transaction not yet in a block', () => {
    for (const state of ['unpressed', 'ink'] as const) {
      const { container, unmount } = structure({ id: ID, state });
      const seals = container.querySelectorAll<HTMLElement>('[data-seam-seal] [data-seal]');
      expect(seals).toHaveLength(2);
      for (const seal of Array.from(seals)) {
        expect(seal).toHaveAttribute('data-seal-state', state);
        expect(seal.innerHTML).toBe(stone(state));
      }
      unmount();
    }
  });

  it('holds the heads’ band under the stone and keeps both heads clear of it', () => {
    const { container } = structure({ id: ID });
    const [consumed, created] = heads(container);
    // the band from lg: 1 px border + 12 px padding + the head's 70 px + its 12 px margin = 95
    for (const head of [consumed, created]) expect(head).toHaveClass('mb-3', 'lg:min-h-[70px]');
    expect(1 + 12 + 70 + 12).toBe(SEAM_SEAL.band);
    // each head keeps 24 px on its seam side, inside the sheet's 36 px lane: 60 px from the seam
    expect(consumed).toHaveClass('lg:pr-6');
    expect(created).toHaveClass('lg:pl-6');
    expect(SHEETS.lane + 24).toBeGreaterThan(SEAM_SEAL.reach);
  });

  // Lane D3 (PA-07, PA-21; the board's phone): below lg the sheets stack, created under consumed,
  // and the page lost its seam seal there. The stone is pressed across the 4 px gap between them,
  // at the right, pulled apart by the gap as it is by the seam from lg: its upper half on the
  // consumed sheet up to that sheet's edge, its lower half on the created sheet from its edge;
  // and it lies over nothing a reader can press.
  it('presses the stone across the gap between the stacked sheets below lg, over no control', () => {
    const { container } = structure({ id: ID });
    const seam = container.querySelector<HTMLElement>('[data-seam]')!;
    // below lg the seam is the 4 px row between the stacked sheets (the sheets keep no gap of
    // their own); from lg the 4 px column between them
    expect(seam).toHaveClass('h-1', 'lg:h-auto', 'self-stretch', 'relative');
    expect(seam.className).not.toMatch(/(^|\s)hidden(\s|$)/);
    const root = container.querySelector<HTMLElement>('[data-tx-canvas]')!;
    expect(root.className).not.toMatch(/(^|\s)(lg:)?gap-y-/);
    expect(2 * 2).toBe(SHEETS.seam);
    const holder = seam.querySelector<HTMLElement>('[data-seam-seal="stacked"]')!;
    expect(holder).not.toBeNull();
    expect(holder).toHaveAttribute('aria-hidden', 'true');
    expect(holder).toHaveClass(
      'pointer-events-none',
      'absolute',
      'right-[18px]',
      'top-1/2',
      '-translate-y-1/2',
      'z-20',
      'flex',
      'lg:hidden'
    );
    // the seam seal from lg is the other holder, hidden below it
    const wide = seam.querySelector<HTMLElement>('[data-seam-seal="side-by-side"]')!;
    expect(wide).toHaveClass('hidden', 'lg:flex');
    const halves = Array.from(holder.firstElementChild!.children) as HTMLElement[];
    expect(halves.map((h) => h.getAttribute('data-seam-half'))).toEqual(['consumed', 'created']);
    for (const half of halves) {
      const seal = half.querySelector<HTMLElement>('[data-seal]')!;
      expect(seal.innerHTML).toBe(stone());
    }
    // the upper half clipped at the stone's middle and moved 2 px up, the lower one moved 2 px
    // down: their cut edges are the sheets' edges, the 4 px of ground between
    expect(halves[0]).toHaveClass('-translate-y-0.5', '[clip-path:inset(-8px_-8px_50%_-8px)]');
    expect(halves[1]).toHaveClass(
      'absolute',
      'inset-0',
      'translate-y-0.5',
      '[clip-path:inset(50%_-8px_-8px_-8px)]'
    );
    // over no control: the tilted stone (71.13 px high) reaches half its height and the 2 px it is
    // moved into each sheet: 37.6 px past the seam's middle, 35.6 past the sheet's edge
    const a = (Math.abs(PAGE_SEAL_TILT) * Math.PI) / 180;
    const tall = 104.5 * Math.sin(a) + 64 * Math.cos(a);
    const across = 104.5 * Math.cos(a) + 64 * Math.sin(a);
    const reach = tall / 2;
    expect(reach).toBeCloseTo(35.57, 2);
    // into the consumed sheet: its 1 px border and its bottom padding, 44 px below lg where the
    // seal is pressed, hold it clear of the sheet's last row (its fold row and its control)
    const consumed = container.querySelector<HTMLElement>(
      '[data-sheet="consumed"] [data-sheet-content]'
    )!;
    expect(consumed).toHaveClass('max-lg:pb-11');
    expect(1 + 44 - reach).toBeGreaterThan(8);
    // into the created sheet: its 1 px border, 12 px of padding, the eyebrow's 18 px line and the
    // first 4.6 px of the count's line, at the right, where the stone's left edge stands 18 px +
    // the stone's width from the sheet's right edge; the head reads from the left below lg and the
    // widest count, "13,568 cells" at the section step, ends far left of it on a 360 px phone
    const sheetAt360 = 360 - 2 * 16;
    const stoneLeft = sheetAt360 - 18 - across;
    const countRight = 1 + 12 + textPx(`${(13568).toLocaleString('en-US')} cells`.length, 17);
    expect(stoneLeft - countRight).toBeGreaterThan(60);
    const created = container.querySelector<HTMLElement>('[data-sheet="created"]')!;
    expect(created.querySelector('[data-sheet-head] a, [data-sheet-head] button')).toBeNull();
  });

  // The band's numbers, from the stone as drawn: 104.5 × 64 for every transaction, tilted −4°.
  it('fits the stone in the band with 12 px above and below, reaching 55 px into each sheet', () => {
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
    // the tilt's overhang past the stone's box (2.1 px across, 3.6 px down) is inside the halves'
    // 8 px outer clip insets
    expect((across - w) / 2).toBeLessThan(8);
    expect((tall - h) / 2).toBeLessThan(8);
    // centred in the band: about 12 px above and below the tilted stone
    const clear = (SEAM_SEAL.band - tall) / 2;
    expect(clear).toBeCloseTo(11.93, 2);
    // each half reaches half the tilted stone into its sheet from the sheet's seam edge
    expect(across / 2).toBeCloseTo(54.35, 2);
    expect(Math.ceil(across / 2)).toBe(SEAM_SEAL.reach);
  });
});
