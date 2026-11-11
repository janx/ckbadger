import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { ROW_LINE_PX, ROW_SEAL, RowSeal } from '@/components/ui/row-seal';
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

  it('measures each stone: block 26 × 26, transaction 24 high and 48.5 wide, cell 30 × 30, script 28 high and 21 wide, the gap 8', () => {
    expect(ROW_LINE_PX).toBe(18);
    expect(ROW_SEAL).toEqual({
      block: { height: 26, width: 26, gap: 8, footprint: 34, overhang: 4 },
      tx: { height: 24, width: 48.5, gap: 8, footprint: 56.5, overhang: 3 },
      cell: { height: 30, width: 30, gap: 8, footprint: 38, overhang: 6 },
      script: { height: 28, width: 21, gap: 8, footprint: 29, overhang: 5 },
    });
    // the arithmetic: footprint = width + gap, overhang = (height − the 18 px line) / 2
    for (const kind of KINDS) {
      const m = ROW_SEAL[kind];
      expect(m.footprint).toBe(m.width + m.gap);
      expect(m.height).toBe(ROW_LINE_PX + 2 * m.overhang);
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
    // a transaction's counts past nine are a full block, the same width
    const many: SealId = { ...SEAL_IDS.tx, consumed: 13000, created: null };
    expect(drawSeal(many, { form: 'row', height: 24, state: 'pressed', ...SEAL_INKS }).width).toBe(
      48.5
    );
    // the gap is the seal's own 8 px right margin
    const { container } = render(<RowSeal id={SEAL_IDS.cell} />);
    expect(container.firstElementChild).toHaveClass('mr-2');
  });

  it('overhangs a one-line row by (height − 18) / 2 above and below only when asked', () => {
    const classes: Record<SealKind, string> = {
      block: '-my-[4px]',
      tx: '-my-[3px]',
      cell: '-my-[6px]',
      script: '-my-[5px]',
    };
    for (const kind of KINDS) {
      expect(classes[kind]).toBe(`-my-[${ROW_SEAL[kind].overhang}px]`);
      const { container, rerender, unmount } = render(<RowSeal id={SEAL_IDS[kind]} />);
      const holder = () => container.firstElementChild as HTMLElement;
      expect(holder().className).not.toMatch(/-my-/);
      rerender(<RowSeal id={SEAL_IDS[kind]} overhang />);
      expect(holder()).toHaveClass(classes[kind]);
      for (const other of KINDS)
        if (classes[other] !== classes[kind]) expect(holder()).not.toHaveClass(classes[other]);
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
