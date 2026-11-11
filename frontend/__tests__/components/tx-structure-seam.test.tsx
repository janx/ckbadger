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
