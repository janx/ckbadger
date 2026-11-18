import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
import { ROW_LINE_PX } from '@/lib/type-metrics';
import { drawSeal, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import { expectOneRowSeal, SEAL_IDS, SEAL_INKS, sealIdsAt } from '@/__tests__/utils/seals';

// block #20,914,858's hash (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';

const KINDS: SealKind[] = ['block', 'tx', 'cell', 'script'];
const STATES: SealState[] = ['pressed', 'ink', 'unpressed'];

describe('RowSeal by kind (the seals by kind)', () => {
  it('leads a row with the row stone of the id at its kind’s height, an image that is not a link', () => {
    for (let i = 0; i < 3; i++)
      for (const kind of KINDS) {
        const id = i ? sealIdsAt(i)[kind] : SEAL_IDS[kind];
        const { container, unmount } = render(<RowSeal id={id} />);
        expectOneRowSeal(container, id);
        unmount();
      }
  });

  // Re-pinned on purpose (readability ruling R6, judged on the Gate F0 prototype: "good"): a list
  // row's stone is small, as in the home page's pipeline, the block's 16 px, each kind about two
  // thirds of what it was with its proportions kept. Was: block 26 × 26, transaction 24 × 48.5,
  // cell 30 × 30, script 28 × 21 (footprints 34, 56.5, 38, 29; overhangs 3, 2, 5, 4 on the 20 px
  // line). The widths are drawSeal's at the heights (the next test draws them for every id).
  it('measures each stone: block 16 × 16, transaction 16 high and 32.3 wide, cell 18 × 18, script 16 high and 12 wide, the gap 8', () => {
    expect(ROW_SEAL).toEqual({
      block: { height: 16, width: 16, gap: 8, footprint: 24 },
      tx: { height: 16, width: 32.3, gap: 8, footprint: 40.3 },
      cell: { height: 18, width: 18, gap: 8, footprint: 26 },
      script: { height: 16, width: 12, gap: 8, footprint: 20 },
    });
    // the arithmetic: footprint = width + gap; each about two thirds of the height it had
    const was = { block: 26, tx: 24, cell: 30, script: 28 } as const;
    for (const kind of KINDS) {
      const m = ROW_SEAL[kind];
      expect(m.footprint).toBe(m.width + m.gap);
    }
    expect(
      Object.fromEntries(
        KINDS.map((kind) => [kind, Math.round((ROW_SEAL[kind].height / was[kind]) * 100) / 100])
      )
    ).toEqual({ block: 0.62, tx: 0.67, cell: 0.6, script: 0.57 });
    // a kind's proportion is kept: the drawn width over the height, as at the old height (to the
    // tenth of a pixel drawSeal rounds a width to)
    const wasWidth = { block: 26, tx: 48.5, cell: 30, script: 21 } as const;
    for (const kind of KINDS)
      expect(ROW_SEAL[kind].width, kind).toBeCloseTo(
        (wasWidth[kind] / was[kind]) * ROW_SEAL[kind].height,
        1
      );
  });

  // Readability R6: at 16 and 18 px no stone is taller than the 20 px row line, so none overhangs
  // it; each stands centred on it, (line − height) / 2 clear above and below.
  it('stands every stone centred in a box one row line tall: 2 px clear of a 16 px stone, 1 px of the cell’s 18', () => {
    expect(ROW_LINE_PX).toBe(20);
    const clear = Object.fromEntries(
      KINDS.map((kind) => [kind, (ROW_LINE_PX - ROW_SEAL[kind].height) / 2])
    );
    expect(clear).toEqual({ block: 2, tx: 2, cell: 1, script: 2 });
    for (const kind of KINDS) {
      const { container, unmount } = render(<RowSeal id={SEAL_IDS[kind]} />);
      const holder = container.firstElementChild as HTMLElement;
      // the box is the row line (tailwind.config.ts: h-row is ROW_LINE_PX), the stone centred in it
      expect(holder).toHaveClass('flex', 'h-row', 'items-center');
      expect(holder).not.toHaveClass('inline-block');
      // nothing pulls it over the line or pushes it off it
      expect(holder.className).not.toMatch(/(^|\s)-?m[ty]-/);
      unmount();
    }
  });

  // A list column's minimum width adds the footprint; the footprint is only true while the drawn
  // stone is as wide as ROW_SEAL says for every id of its kind, in every state.
  it('is drawn as wide as ROW_SEAL says for every id of a kind, so a column budget built on it holds', () => {
    for (let i = 0; i < 24; i++)
      for (const kind of KINDS) {
        const id = sealIdsAt(i)[kind];
        const { height, width } = ROW_SEAL[kind];
        for (const state of STATES)
          expect(
            drawSeal(id, { form: 'row', height, state, ...SEAL_INKS }).svg,
            `${kind} ${i} ${state}`
          ).toMatch(new RegExp(`^<svg width="${width}" height="${height}"`));
        // and the svg the component renders
        const { container, unmount } = render(<RowSeal id={id} />);
        const svg = container.querySelector('svg');
        expect(svg).toHaveAttribute('width', String(width));
        expect(svg).toHaveAttribute('height', String(height));
        unmount();
      }
    // a transaction's counts past nine are a full block, the same width (re-pinned, R6: 24 high
    // and 48.5 wide)
    const many: SealId = { ...SEAL_IDS.tx, consumed: 13000, created: null };
    expect(drawSeal(many, { form: 'row', height: 16, state: 'pressed', ...SEAL_INKS }).width).toBe(
      32.3
    );
    // a block's count past nine is a full block too, and a cell's capacity of eleven digits
    for (const id of [
      { ...SEAL_IDS.block, txs: 13568 },
      { ...SEAL_IDS.cell, capacity: BigInt('99999999999') * BigInt(100000000) },
    ] satisfies SealId[])
      expect(
        drawSeal(id, { form: 'row', height: ROW_SEAL[id.kind].height, ...SEAL_INKS, state: 'ink' })
          .width
      ).toBe(ROW_SEAL[id.kind].width);
    // the gap is the seal's own 8 px right margin
    const { container } = render(<RowSeal id={SEAL_IDS.cell} />);
    expect(container.firstElementChild).toHaveClass('mr-2');
  });

  // C11: a cell tile stands its stone alone in its mark column, the column's 10 px gap being the
  // stone's gap; its own 8 px margin there would push it off the column's centre.
  it('keeps no gap of its own when asked, for a stone standing alone in a mark column', () => {
    for (const kind of KINDS) {
      const { container, rerender, unmount } = render(<RowSeal id={SEAL_IDS[kind]} gap={false} />);
      const holder = () => container.firstElementChild as HTMLElement;
      expectOneRowSeal(container, SEAL_IDS[kind], 'pressed', { gap: false });
      expect(holder()).not.toHaveClass('mr-2');
      expect(holder()).toHaveClass('flex', 'h-row', 'items-center');
      // the default keeps it
      rerender(<RowSeal id={SEAL_IDS[kind]} />);
      expect(holder()).toHaveClass('mr-2');
      unmount();
    }
  });

  it('draws the stone in its state: a consumed cell in the dead ink, a pool transaction unpressed', () => {
    for (const kind of KINDS)
      for (const state of STATES) {
        const { container, unmount } = render(<RowSeal id={SEAL_IDS[kind]} state={state} />);
        expectOneRowSeal(container, SEAL_IDS[kind], state);
        unmount();
      }
  });

  it('refuses an id it cannot draw, naming it; a row takes counts a page refuses', () => {
    expect(() => render(<RowSeal id={{ ...SEAL_IDS.cell, index: -1 }} />)).toThrow(
      `cell ${SEAL_IDS.cell.tx}:-1: the index must be a whole number ≥ 0, got -1`
    );
    expect(() =>
      render(<RowSeal id={{ ...SEAL_IDS.block, kind: 'uncle' } as unknown as SealId} />)
    ).toThrow('no such kind of seal: "uncle"');
    expect(() => render(<RowSeal id={{ ...SEAL_IDS.tx, hash: HASH.toUpperCase() }} />)).toThrow(
      'the transaction hash is not a lowercase 32-byte hash'
    );
    for (const id of [
      { ...SEAL_IDS.block, txs: 10000 },
      { ...SEAL_IDS.tx, consumed: 100000 },
      { ...SEAL_IDS.cell, index: 10000 },
    ] satisfies SealId[]) {
      const { container, unmount } = render(<RowSeal id={id} />);
      expectOneRowSeal(container, id);
      unmount();
    }
  });
});
