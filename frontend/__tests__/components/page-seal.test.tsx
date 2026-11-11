import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT, PageSeal, SealedHead } from '@/components/ui/page-seal';
import { drawSeal, sealLabel, type SealId, type SealKind, type SealState } from '@/lib/seal/stones';
import {
  expectBefore,
  expectNamePlate,
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
          <SealedHead id={id} kind={kind} identity={id.kind}>
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
          <SealedHead id={id} state={state} kind={kind} identity={id.kind}>
            <h1>{kind}</h1>
          </SealedHead>
        );
        expectOnePageSeal(container, id, state);
        unmount();
      }
    const { container } = render(<PageSeal id={SEAL_IDS.cell} />);
    expect(container.firstElementChild).toHaveAttribute('data-seal-state', 'pressed');
  });

  // The name plate (seals by kind, C10; Gate C: "page head 需要优化"): beside the stone stands only
  // the object's name plate — the kind line, the name, the id line, three short lines on one left
  // edge, centred on the stone from md up — and everything else of the head is `below`, under the
  // stone and the plate from the page's left edge at every width (it was set in the column beside
  // the stone, indented by the stone's width). Below md the stone's row carries the kind line alone
  // and the name and the id line go under the stone at the full width: the plate's wrapper is
  // `contents` there, so its name-and-id box is the grid's next item, spanning both columns. One
  // DOM, nothing hidden; jsdom has no layout, so the classes that arrange it are what is pinned.
  it('stands the name plate beside the stone, and what is below under both from the left edge', () => {
    const { container } = render(
      <SealedHead
        id={SEAL_IDS.tx}
        kind="Transaction"
        status={<span data-testid="status">committed</span>}
        identity={<span>{SEAL_IDS.tx.hash}</span>}
        below={<p>2 cells became 3.</p>}
      >
        <h1>Transfer</h1>
      </SealedHead>
    );
    const title = screen.getByRole('heading', { level: 1 });
    const seal = screen.getByRole('img', { name: sealLabel(SEAL_IDS.tx) });
    const { head, stone, plate, kindLine, nameBox, below } = expectNamePlate(
      container,
      'Transaction'
    );
    expect(head).toHaveAttribute('data-sealed-head', 'tx');
    expect(stone).toContainElement(seal);
    expectOnePageSeal(container, SEAL_IDS.tx);
    // the kind line: the kind word, then the status chips, beside the stone at every width
    expect(Array.from(kindLine.children)).toEqual([
      kindLine.firstElementChild,
      screen.getByTestId('status'),
    ]);
    expect(kindLine).toHaveTextContent(/^Transactioncommitted$/);
    // the name and the id line, in the plate from md up, under the stone below md
    expect(Array.from(nameBox.children).map((child) => child.textContent)).toEqual([
      'Transfer',
      SEAL_IDS.tx.hash,
    ]);
    expect(nameBox).toHaveClass('max-md:col-span-2');
    expect(nameBox.className).not.toMatch(/(^|\s)(md|lg|xl):col-/);
    // below: the grid's last row, across both columns at every width, outside the plate
    expect(below).not.toBeNull();
    expect(below).toHaveTextContent(/^2 cells became 3\.$/);
    expect(plate).not.toContainElement(below);
    expect(below!.className).not.toMatch(/(^|\s)(sm|md|lg|xl):/);
    // reading order: stone, kind word, status, name, id line, below
    expectBefore(seal, kindLine);
    expectBefore(screen.getByTestId('status'), title);
    expectBefore(title, screen.getByText(SEAL_IDS.tx.hash));
    expectBefore(screen.getByText(SEAL_IDS.tx.hash), below!);
  });

  it('adds nothing under the head when it is given nothing below, and no status when given none', () => {
    const { container } = render(
      <SealedHead id={SEAL_IDS.block} kind="Block" identity={SEAL_IDS.block.hash}>
        <h1>#20,914,858</h1>
      </SealedHead>
    );
    const { head, kindLine, nameBox, below } = expectNamePlate(container, 'Block');
    expect(head).toHaveAttribute('data-sealed-head', 'block');
    expect(below).toBeNull();
    expect(head.querySelector('[data-sealed-below]')).toBeNull();
    expect(kindLine.children).toHaveLength(1);
    expect(kindLine).toHaveTextContent(/^Block$/);
    expect(nameBox).toHaveTextContent(`#20,914,858${SEAL_IDS.block.hash}`);
  });

  // C10 round 2: the id line speaks in one voice on every plate, the kind line's — text-small in
  // text-text-dim — set once here around what the page gives, so the page's own pieces carry no
  // size or ink (the cell's full out point was text-body in the bright ink, competing with the h1
  // above it; the block's hash and the address were text-body in their own inks).
  it('sets the id line in the kind line’s quiet voice, once, around what the page gives', () => {
    const { container } = render(
      <SealedHead
        id={SEAL_IDS.cell}
        kind="Cell"
        identity={
          <span className="flex items-center">
            <span data-testid="outpoint">{SEAL_IDS.cell.tx}:1</span>
            <button type="button" className="text-text-dim text-meta">
              copy
            </button>
          </span>
        }
      >
        <h1>0xabcdef01…234567:1</h1>
      </SealedHead>
    );
    const { kindLine, nameBox, idLine } = expectNamePlate(container, 'Cell');
    expect(nameBox.lastElementChild).toBe(idLine);
    expect(idLine).toHaveAttribute('data-sealed-id');
    expect(idLine.className).toBe('text-text-dim text-small min-w-0 [overflow-wrap:anywhere]');
    // the kind word's own voice
    expect(kindLine.firstElementChild).toHaveClass('text-text-dim', 'text-small');
    expect(idLine.firstElementChild!.firstElementChild).toBe(screen.getByTestId('outpoint'));
    // a copy control keeps its own classes
    expect(screen.getByRole('button', { name: 'copy' })).toHaveClass('text-text-dim', 'text-meta');
  });

  // The page's piece may not speak in a voice of its own: the check refuses it, by its markup.
  it('refuses an id line whose piece sets a size or an ink of its own', () => {
    const { container } = render(
      <SealedHead
        id={SEAL_IDS.cell}
        kind="Cell"
        identity={<span className="text-text-bright text-body">{SEAL_IDS.cell.tx}:1</span>}
      >
        <h1>0xabcdef01…234567:1</h1>
      </SealedHead>
    );
    expect(() => expectNamePlate(container, 'Cell')).toThrow(/text-text-bright text-body/);
  });

  // The plate's 8 px line gaps keep a one-line plate no taller than the stone on every page (kind
  // 18 + name 34.5, the block's 36 + id line 18 to 26 + 16: 86.5 to 94.5 px beside 94 to 112):
  // pinned as the classes that set them, with the rows' 16 px (20 from md).
  it('spaces the plate’s lines 8 px apart and the head’s rows 16 px, 20 from md', () => {
    const { container } = render(
      <SealedHead id={SEAL_IDS.cell} kind="Cell" identity="0xabcdef01:1" below={<p>the rest</p>}>
        <h1>0xabcdef01…234567:1</h1>
      </SealedHead>
    );
    const { head, plate, nameBox } = expectNamePlate(container, 'Cell');
    expect(plate).toHaveClass('md:gap-2');
    expect(nameBox).toHaveClass('gap-2');
    expect(head).toHaveClass('gap-y-4', 'md:gap-y-5');
    // no margin of their own: the gaps are the only spacing
    for (const box of [plate, nameBox, ...Array.from(plate.children)])
      expect(box.className).not.toMatch(/(^|\s|:)m[tby]?-/);
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
        <SealedHead
          id={{ ...SEAL_IDS.tx, consumed: 100000 }}
          kind="Transaction"
          identity={SEAL_IDS.tx.hash}
        >
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
