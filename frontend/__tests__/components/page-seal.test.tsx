import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT, PageSeal, SealedHead } from '@/components/ui/page-seal';
import { drawSeal, sealLabel, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import {
  expectBefore,
  expectOnePageSeal,
  SEAL_IDS,
  SEAL_INKS,
  sealIdsAt,
} from '@/__tests__/utils/seals';

const KINDS: SealKind[] = ['block', 'tx', 'cell', 'script'];
const STATES: SealState[] = ['pressed', 'ink', 'unpressed'];

describe('PageSeal and SealedHead by kind (the seals by kind)', () => {
  it('presses a block at 104 / 80 px, a transaction at 94 / 64, a cell at 112 / 88, a script at 104 / 80', () => {
    expect(PAGE_SEAL_HEIGHTS).toEqual({
      block: { wide: 104, narrow: 80 },
      tx: { wide: 94, narrow: 64 },
      cell: { wide: 112, narrow: 88 },
      script: { wide: 104, narrow: 80 },
    });
    expect(PAGE_SEAL_TILT).toBe(-4);
  });

  it('draws one page seal of the id at its kind’s wide height, tilted, and its narrow height below md', () => {
    for (let i = 0; i < 3; i++)
      for (const kind of KINDS) {
        const id = i ? sealIdsAt(i)[kind] : SEAL_IDS[kind];
        const { container, unmount } = render(
          <SealedHead id={id}>
            <h1>{kind}</h1>
          </SealedHead>
        );
        const seal = expectOnePageSeal(container, id);
        // The svg is a block, so no descender space opens under the stamp; the width follows the
        // height (w-auto), never a number of its own.
        expect(seal).toHaveClass('[&>svg]:block');
        expect(seal.className).not.toMatch(/w-\[/);
        unmount();
      }
  });

  // The narrow height is a CSS height on the one drawn seal, the width following the svg's ratio.
  // That is the stone drawn at that height only because a page stone is drawn in its own units and
  // the height writes nothing but the svg's width and height: pinned here for every kind.
  it('draws the same stone at both heights: only the svg’s width and height differ', () => {
    const drawn = (id: SealId, height: number) =>
      drawSeal(id, { form: 'page', height, state: 'pressed', ...SEAL_INKS, tilt: PAGE_SEAL_TILT });
    const widths: Record<SealKind, [number, number]> = {
      block: [104, 80],
      tx: [153.5, 104.5], // the long stone, about 1.63 : 1
      cell: [112, 88],
      script: [73.5, 56.5], // the tall stone
    };
    for (let i = 0; i < 6; i++)
      for (const kind of KINDS) {
        const id = sealIdsAt(i)[kind];
        const { wide, narrow } = PAGE_SEAL_HEIGHTS[kind];
        const unsized = (height: number) =>
          drawn(id, height).svg.replace(/^<svg width="[\d.]+" height="[\d.]+"/, '<svg');
        expect(unsized(narrow)).toBe(unsized(wide));
        expect([drawn(id, wide).width, drawn(id, narrow).width]).toEqual(widths[kind]);
        expect(drawn(id, narrow).svg).toMatch(
          new RegExp(`^<svg width="[\\d.]+" height="${narrow}"`)
        );
      }
  });

  // The state is the ink: a consumed cell in the dead ink, a transaction not yet committed
  // unpressed (dashed, its field at half strength).
  it('presses the stone in the state it is given, pressed when it is given none', () => {
    for (const kind of KINDS)
      for (const state of STATES) {
        const id = SEAL_IDS[kind];
        const { container, unmount } = render(
          <SealedHead id={id} state={state}>
            <h1>{kind}</h1>
          </SealedHead>
        );
        expectOnePageSeal(container, id, state);
        unmount();
      }
    const { container } = render(<PageSeal id={SEAL_IDS.cell} />);
    expect(container.firstElementChild).toHaveAttribute('data-seal-state', 'pressed');
  });

  it('presses the stone first, the identity beside it, what is below in the identity column', () => {
    render(
      <SealedHead id={SEAL_IDS.tx} below={<p>2 cells became 3.</p>}>
        <h1>Transfer</h1>
        <p>Transaction {SEAL_IDS.tx.hash}</p>
      </SealedHead>
    );
    const title = screen.getByRole('heading', { level: 1 });
    const seal = screen.getByRole('img', { name: sealLabel(SEAL_IDS.tx) });
    expectBefore(seal, title);
    const head = title.closest('[data-sealed-head]')!;
    expect(head).toHaveAttribute('data-sealed-head', 'tx');
    expect(head).toHaveClass(
      'grid',
      'grid-cols-[auto_minmax(0,1fr)]',
      'items-start',
      'min-w-0',
      'gap-x-4',
      'md:gap-x-5'
    );
    expect(head.children).toHaveLength(3);
    const [stone, column, below] = Array.from(head.children) as HTMLElement[];
    expect(stone).toHaveAttribute('data-seal', 'tx');
    expect(stone).toContainElement(seal);
    // two grid columns: the seal's own width, then the identity column, which may shrink
    expect(column).toHaveClass('min-w-0');
    expect(column).toContainElement(title);
    expect(title.parentElement).toBe(column);
    expect(column).toHaveTextContent(`Transaction ${SEAL_IDS.tx.hash}`);
    // Rubbing × Song print, phase 5 (T4): the Transaction board's comment stands under the seal
    // and the identity, in the identity column — the grid's second row, second column, so its left
    // edge is the column's for every stone, with no number for the seal's width.
    expect(below).toHaveAttribute('data-sealed-below');
    expect(below).toHaveClass('col-start-2', 'min-w-0');
    expect(below).toHaveTextContent(/^2 cells became 3\.$/);
    expect(column).not.toContainElement(below);
    expectBefore(title, below);
  });

  it('adds nothing under the head when it is given nothing below', () => {
    render(
      <SealedHead id={SEAL_IDS.block}>
        <h1>#20,914,858</h1>
        <p>Block {SEAL_IDS.block.hash}</p>
      </SealedHead>
    );
    const title = screen.getByRole('heading', { level: 1 });
    const head = title.closest('[data-sealed-head]')!;
    expect(head).toHaveAttribute('data-sealed-head', 'block');
    expectBefore(screen.getByRole('img', { name: sealLabel(SEAL_IDS.block) }), title);
    expect(head.children).toHaveLength(2);
    expect(title.parentElement!.parentElement).toBe(head);
    expect(head.querySelector('[data-sealed-below]')).toBeNull();
  });

  it('refuses an id it cannot draw on a page, naming it, with no fallback', () => {
    expect(() =>
      render(<PageSeal id={{ ...SEAL_IDS.script, hash: SEAL_IDS.script.hash.slice(0, 65) }} />)
    ).toThrow('the script hash is not a lowercase 32-byte hash');
    expect(() =>
      render(<PageSeal id={{ ...SEAL_IDS.block, kind: 'uncle' } as unknown as SealId} />)
    ).toThrow('no such kind of seal: "uncle"');
    expect(() => render(<PageSeal id={{ ...SEAL_IDS.block, txs: 10000 }} />)).toThrow(
      `block ${SEAL_IDS.block.hash}: a page seal writes the transaction count in up to 4 digits`
    );
    expect(() =>
      render(
        <SealedHead id={{ ...SEAL_IDS.tx, consumed: 100000 }}>
          <h1>Transfer</h1>
        </SealedHead>
      )
    ).toThrow('a page seal writes the cells consumed in up to 5 digits, got 100000');
    // a transaction that consumed thirteen thousand cells is pressed, five digits at its left
    const { container } = render(<PageSeal id={{ ...SEAL_IDS.tx, consumed: 13000 }} />);
    expect(container.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Transaction seal ${SEAL_IDS.tx.hash.slice(0, 10)}, 13000 consumed, 3 created`
    );
  });
});
