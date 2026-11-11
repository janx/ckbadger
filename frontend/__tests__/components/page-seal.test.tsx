import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  PAGE_SEAL_HEIGHTS,
  PAGE_SEAL_SIZES,
  PAGE_SEAL_TILT,
  PageSeal,
  SealedHead,
  type PageSealScale,
} from '@/components/ui/page-seal';
import { drawSeal, sealLabel, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import { compound } from '@/lib/talisman/compound';
import { PAGE } from '@/lib/talisman/recipes';
import {
  expectBefore,
  expectOnePageSeal,
  expectOnePageSealOf,
  SEAL_IDS,
  SEAL_INKS,
  sealIdsAt,
} from '@/__tests__/utils/seals';

// block #20,914,858's hash (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';
const PAINT = { ink: 'var(--c-mark)', ground: 'var(--c-base-bg)' };
const SCALES = Object.keys(PAGE_SEAL_SIZES) as PageSealScale[];

describe('PageSeal (rubbing × Song print, phase 3)', () => {
  it('sizes a record (block, transaction) at 150 / 96 px and an object (address, cell) at 112 / 80', () => {
    expect(PAGE_SEAL_SIZES).toEqual({
      record: { wide: 150, narrow: 96 },
      object: { wide: 112, narrow: 80 },
    });
    expect(PAGE_SEAL_TILT).toBe(-4);
  });

  it('draws one page seal of the hash at the wide size, tilted, and the narrow size below md', () => {
    for (const scale of SCALES) {
      const { container, unmount } = render(
        <SealedHead hash={HASH} scale={scale}>
          <h1>#20,914,858</h1>
        </SealedHead>
      );
      const seal = expectOnePageSeal(container, HASH, scale);
      // The svg is a block, so no descender space opens under the stamp.
      expect(seal).toHaveClass('[&>svg]:block');
      unmount();
    }
  });

  // The narrow size is a CSS height on the one drawn seal. That is the seal drawn at that size
  // only because the page talisman is drawn in its own units and the size writes nothing but the
  // svg's width and height: pinned here for every size the pages use.
  it('draws the same talisman at every size: only the svg’s width and height differ', () => {
    const sized = (px: number) =>
      compound(HASH, PAGE, px, { ...PAINT, tilt: PAGE_SEAL_TILT })
        .svg.replace(/^<svg width="\d+" height="\d+"/, '<svg')
        .trim();
    for (const { wide, narrow } of Object.values(PAGE_SEAL_SIZES)) {
      expect(sized(narrow)).toBe(sized(wide));
      expect(compound(HASH, PAGE, narrow, PAINT).svg).toMatch(
        new RegExp(`^<svg width="\\d+" height="${narrow}"`)
      );
    }
  });

  it('presses the seal first, the identity beside it in a column that may shrink', () => {
    render(
      <SealedHead hash={HASH} scale="record">
        <h1>#20,914,858</h1>
        <p>Block {HASH}</p>
      </SealedHead>
    );
    const title = screen.getByRole('heading', { level: 1 });
    const seal = screen.getByRole('img', { name: 'Hash seal 0xbbb655ae' });
    expectBefore(seal, title);
    const head = title.closest('[data-sealed-head]')!;
    // two grid columns: the seal's own width, then the identity column, which may shrink
    expect(head).toHaveClass(
      'grid',
      'grid-cols-[auto_minmax(0,1fr)]',
      'items-start',
      'min-w-0',
      'gap-x-4',
      'md:gap-x-5'
    );
    const column = title.parentElement!;
    expect(column).toHaveClass('min-w-0');
    expect(column.parentElement).toBe(head);
    expect(column).toHaveTextContent(`Block ${HASH}`);
    expect(head.children).toHaveLength(2);
    expect(head.querySelector('[data-sealed-below]')).toBeNull();
  });

  // Rubbing × Song print, phase 5 (T4): the Transaction board's comment stands under the seal and
  // the identity, in the identity column — the grid's second row, second column, so its left edge
  // is the column's for either seal size, with no number for the seal's width.
  it('sets what is below the head in the identity column, under the seal', () => {
    render(
      <SealedHead hash={HASH} scale="record" below={<p>3 cells became 2.</p>}>
        <h1>Transfer</h1>
      </SealedHead>
    );
    const title = screen.getByRole('heading', { level: 1 });
    const head = title.closest('[data-sealed-head]')!;
    const [seal, column, below] = Array.from(head.children) as HTMLElement[];
    expect(seal).toHaveAttribute('data-hash-seal', 'page');
    expect(column).toContainElement(title);
    expect(below).toHaveAttribute('data-sealed-below');
    expect(below).toHaveClass('col-start-2', 'min-w-0');
    expect(below).toHaveTextContent(/^3 cells became 2\.$/);
    expect(column).not.toContainElement(below);
    expectBefore(title, below);
  });

  it('refuses a hash that is not 32 bytes, with no fallback', () => {
    expect(() => render(<PageSeal hash={HASH.slice(0, 65)} scale="object" />)).toThrow(
      'not a lowercase 32-byte hash'
    );
  });
});

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
        const seal = expectOnePageSealOf(container, id);
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
        expectOnePageSealOf(container, id, state);
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
    const [stone, column, below] = Array.from(head.children) as HTMLElement[];
    expect(stone).toHaveAttribute('data-seal', 'tx');
    expect(stone).toContainElement(seal);
    expect(column).toHaveClass('min-w-0');
    expect(column).toContainElement(title);
    expect(column).toHaveTextContent(`Transaction ${SEAL_IDS.tx.hash}`);
    expect(below).toHaveAttribute('data-sealed-below');
    expect(below).toHaveClass('col-start-2', 'min-w-0');
    expect(below).toHaveTextContent(/^2 cells became 3\.$/);
    expect(column).not.toContainElement(below);
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

  // The two shapes live side by side until the talisman goes: the stone of an id, or the talisman
  // of a hash at a scale; never a mix, and never neither.
  it('takes the stone of an id or the talisman of a hash, never a mix (a type check)', () => {
    const mixed = [
      // @ts-expect-error an id with a scale
      <PageSeal key="a" id={SEAL_IDS.block} scale="record" />,
      // @ts-expect-error a hash with a state
      <PageSeal key="b" hash={HASH} scale="record" state="ink" />,
      // @ts-expect-error a hash with no scale
      <PageSeal key="c" hash={HASH} />,
      // @ts-expect-error neither an id nor a hash
      <SealedHead key="d">x</SealedHead>,
    ];
    expect(mixed).toHaveLength(4);
    // the talisman's branch draws the talisman, and no stone
    const { container } = render(
      <SealedHead hash={HASH} scale="object">
        <h1>Address</h1>
      </SealedHead>
    );
    expect(container.querySelector('[data-hash-seal="page"]')).not.toBeNull();
    expect(container.querySelector('[data-seal]')).toBeNull();
    expect(container.firstElementChild).toHaveAttribute('data-sealed-head', 'object');
  });
});
