import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { Seal } from '@/components/ui/seal';
import {
  drawSeal,
  sealLabel,
  type SealForm,
  type SealId,
  type SealKind,
  type SealState,
} from '@/lib/seal/stones';
import { parsed, SEAL_IDS, SEAL_INKS, sealIdsAt } from '@/__tests__/utils/seals';

// count the generator's calls without changing what it draws
vi.mock('@/lib/seal/stones', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/seal/stones')>();
  return { ...actual, drawSeal: vi.fn(actual.drawSeal) };
});

const KINDS: SealKind[] = ['block', 'tx', 'cell', 'script'];
const FORMS: SealForm[] = ['page', 'row'];
const STATES: SealState[] = ['pressed', 'ink', 'unpressed'];
/** The heights the pages press them at (the plan's §0): page (wide), row. */
const HEIGHT: Record<SealForm, Record<SealKind, number>> = {
  page: { block: 104, tx: 94, cell: 112, script: 104 },
  row: { block: 26, tx: 24, cell: 30, script: 28 },
};

describe('Seal (the seals by kind)', () => {
  it('renders one svg image per kind and form, named for the id, marked with its kind, form and state', () => {
    for (const kind of KINDS)
      for (const form of FORMS) {
        const id = SEAL_IDS[kind];
        const { container, unmount } = render(
          <Seal id={id} form={form} height={HEIGHT[form][kind]} />
        );
        const svgs = container.querySelectorAll('svg');
        expect(svgs).toHaveLength(1);
        expect(svgs[0]).toHaveAttribute('role', 'img');
        expect(svgs[0]).toHaveAttribute('aria-label', sealLabel(id));
        const holder = container.firstElementChild as HTMLElement;
        expect(holder.tagName).toBe('SPAN');
        expect(holder).toHaveAttribute('data-seal', kind);
        expect(holder).toHaveAttribute('data-seal-form', form);
        expect(holder).toHaveAttribute('data-seal-state', 'pressed');
        expect(holder).toHaveClass('inline-block', 'shrink-0', 'leading-none');
        expect(holder).toContainElement(svgs[0]);
        unmount();
      }
    // the labels say the kind, the id and what it counts
    const { container } = render(<Seal id={SEAL_IDS.tx} form="row" height={24} />);
    expect(container.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Transaction seal ${SEAL_IDS.tx.hash.slice(0, 10)}, 2 consumed, 3 created`
    );
  });

  it('draws at the height it is asked, the width following from the kind and the form', () => {
    for (const kind of KINDS)
      for (const form of FORMS)
        for (const height of [HEIGHT[form][kind], form === 'page' ? 64 : 20]) {
          const id = SEAL_IDS[kind];
          const { container, unmount } = render(<Seal id={id} form={form} height={height} />);
          const svg = container.querySelector('svg');
          expect(svg).toHaveAttribute('height', String(height));
          const drawn = drawSeal(id, { form, height, state: 'pressed', ...SEAL_INKS });
          expect(svg).toHaveAttribute('width', String(drawn.width));
          unmount();
        }
    const width = (id: SealId, form: SealForm, height: number) => {
      const { container, unmount } = render(<Seal id={id} form={form} height={height} />);
      const w = container.querySelector('svg')?.getAttribute('width');
      unmount();
      return w;
    };
    expect(width(SEAL_IDS.tx, 'row', 24)).toBe('48.5'); // the long stone
    expect(width(SEAL_IDS.tx, 'page', 94)).toBe('153.5');
    expect(width(SEAL_IDS.block, 'row', 26)).toBe('26'); // the square
    expect(width(SEAL_IDS.cell, 'row', 30)).toBe('30'); // the round
    expect(width(SEAL_IDS.script, 'row', 28)).toBe('21'); // the tall
    expect(width(SEAL_IDS.script, 'row', 20)).toBe('15');
  });

  // The state is the ink: the cinnabar `mark` in force (UI grade: the prompt square's red, never
  // the text red `rouge`); the dead ink for a consumed cell or an uncle; dashed and at half strength
  // for what is not committed. A row stone's text is cut in the page ground.
  it('presses in three states, each in its ink, with nothing but the palette’s three paints', () => {
    for (const kind of KINDS)
      for (const form of FORMS)
        for (const state of STATES) {
          const id = SEAL_IDS[kind];
          const { container, unmount } = render(
            <Seal id={id} form={form} height={HEIGHT[form][kind]} state={state} />
          );
          const holder = container.firstElementChild as HTMLElement;
          expect(holder).toHaveAttribute('data-seal-state', state);
          const html = holder.innerHTML;
          expect(html).not.toMatch(/#[0-9a-f]{3,6}/i);
          expect(html).not.toContain('--c-rouge');
          for (const paint of new Set(html.match(/var\(--c-[a-z-]+\)/g)))
            expect(Object.values(SEAL_INKS)).toContain(paint);
          if (state === 'ink') {
            expect(html).toContain('var(--c-text-dim)');
            expect(html).not.toContain('var(--c-mark)');
          } else {
            expect(html).toContain('var(--c-mark)');
            expect(html).not.toContain('var(--c-text-dim)');
          }
          if (state === 'unpressed') expect(html).toContain('stroke-dasharray');
          else expect(html).not.toContain('stroke-dasharray');
          expect(html).toBe(
            parsed(drawSeal(id, { form, height: HEIGHT[form][kind], state, ...SEAL_INKS }).svg)
          );
          unmount();
        }
    // a solid row stone is the mark, its text cut in the ground
    const { container } = render(<Seal id={SEAL_IDS.block} form="row" height={26} />);
    expect(container.querySelector('svg > rect')).toHaveAttribute('fill', 'var(--c-mark)');
    expect(container.querySelector('svg > g')).toHaveAttribute('fill', 'var(--c-base-bg)');
  });

  it('tilts when asked, opens a seam when pressed across one, and carries the caller’s classes', () => {
    const { container, rerender } = render(
      <Seal id={SEAL_IDS.tx} form="page" height={94} tilt={-4} className="mt-2" />
    );
    expect(container.querySelector('svg')?.getAttribute('style')).toBe('transform: rotate(-4deg)');
    expect(container.firstElementChild).toHaveClass('mt-2', 'inline-block');
    rerender(<Seal id={SEAL_IDS.tx} form="page" height={94} />);
    expect(container.querySelector('svg')).not.toHaveAttribute('style');
    expect(container.innerHTML).not.toContain('stroke="var(--c-base-bg)"');
    rerender(<Seal id={SEAL_IDS.tx} form="page" height={64} seamPx={3} />);
    const seam = container.querySelector('svg')!.lastElementChild;
    expect(seam?.tagName).toBe('path');
    expect(seam).toHaveAttribute('stroke', 'var(--c-base-bg)');
    expect(seam?.getAttribute('d')).toMatch(/^M[\d.]+ -1L[\d.]+ [\d.]+$/);
  });

  it('draws once per id’s fields: a re-render with an equal id, a fresh object, keeps the same element', () => {
    const draw = vi.mocked(drawSeal);
    for (const kind of KINDS) {
      const fresh = (): SealId => {
        const id = SEAL_IDS[kind];
        // a deep copy: a new object, a new type object, a new BigInt
        return id.kind === 'cell'
          ? { ...id, type: id.type && { ...id.type }, capacity: BigInt(id.capacity.toString()) }
          : { ...id };
      };
      draw.mockClear();
      const { container, rerender, unmount } = render(<Seal id={fresh()} form="row" height={30} />);
      const first = container.querySelector('svg');
      expect(draw).toHaveBeenCalledTimes(1);
      rerender(<Seal id={fresh()} form="row" height={30} />);
      rerender(<Seal id={fresh()} form="row" height={30} state="pressed" />);
      expect(container.querySelector('svg')).toBe(first);
      expect(draw).toHaveBeenCalledTimes(1);
      // anything else drawn differently is drawn again
      rerender(<Seal id={fresh()} form="row" height={30} state="ink" />);
      expect(draw).toHaveBeenCalledTimes(2);
      rerender(<Seal id={fresh()} form="row" height={32} state="ink" />);
      expect(draw).toHaveBeenCalledTimes(3);
      expect(container.querySelector('svg')).toHaveAttribute('height', '32');
      unmount();
    }
    // every field of an id is part of what it is drawn by
    const other = sealIdsAt(7);
    const changed: SealId[] = [
      { ...SEAL_IDS.block, txs: 249 },
      { ...SEAL_IDS.block, hash: other.block.hash },
      { ...SEAL_IDS.tx, consumed: null },
      { ...SEAL_IDS.tx, created: 4 },
      { ...SEAL_IDS.tx, hash: other.tx.hash },
      { ...SEAL_IDS.cell, tx: other.tx.hash },
      { ...SEAL_IDS.cell, index: 2 },
      { ...SEAL_IDS.cell, lock: other.cell.lock },
      { ...SEAL_IDS.cell, type: null },
      { ...SEAL_IDS.cell, type: { class: 'objects', hash: SEAL_IDS.cell.type!.hash } },
      { ...SEAL_IDS.cell, type: { class: 'tokens', hash: other.script.hash } },
      { ...SEAL_IDS.cell, capacity: SEAL_IDS.cell.capacity + BigInt(1) },
      { ...SEAL_IDS.script, hash: other.script.hash },
    ];
    for (const id of changed) {
      const kind = id.kind;
      const { rerender, unmount } = render(<Seal id={SEAL_IDS[kind]} form="row" height={30} />);
      draw.mockClear();
      rerender(<Seal id={id} form="row" height={30} />);
      expect(
        draw,
        JSON.stringify(id, (_k, v) => (typeof v === 'bigint' ? `${v}` : v))
      ).toHaveBeenCalledTimes(1);
      unmount();
    }
  });

  it('checks the id it is given on every render, refusing one it cannot draw by name', () => {
    const refuses = (id: unknown, message: string, form: SealForm = 'row') =>
      expect(() => render(<Seal id={id as SealId} form={form} height={30} />)).toThrow(message);
    refuses(
      { ...SEAL_IDS.block, hash: SEAL_IDS.block.hash.slice(0, 65) },
      'the block hash is not a lowercase 32-byte hash'
    );
    refuses(
      { ...SEAL_IDS.tx, consumed: -1 },
      `transaction ${SEAL_IDS.tx.hash}: the cells consumed must be a whole number ≥ 0, got -1`
    );
    refuses(
      { ...SEAL_IDS.cell, type: { class: 'ckb', hash: SEAL_IDS.cell.lock } },
      `cell ${SEAL_IDS.cell.tx}:1: class ckb is no type's class`
    );
    refuses({ ...SEAL_IDS.cell, lock: `${SEAL_IDS.cell.lock}"/><b>` }, 'the lock hash is not');
    refuses({ ...SEAL_IDS.cell, capacity: 1204 }, 'a capacity is a whole number of shannons');
    refuses({ ...SEAL_IDS.script, kind: 'address' }, 'no such kind of seal: "address"');
    refuses(SEAL_IDS.block, 'no such form "brush"', 'brush' as SealForm);
    // a page seal writes up to five digits of a transaction's cells; a row seal draws a full block
    refuses({ ...SEAL_IDS.tx, consumed: 100000 }, 'in up to 5 digits, got 100000', 'page');
    expect(() =>
      render(<Seal id={{ ...SEAL_IDS.tx, consumed: 13000 }} form="page" height={94} />)
    ).not.toThrow();
    expect(() =>
      render(<Seal id={{ ...SEAL_IDS.tx, consumed: 100000 }} form="row" height={24} />)
    ).not.toThrow();
    // a valid id drawn, then an id that is not: the second render refuses it
    const { rerender } = render(<Seal id={SEAL_IDS.tx} form="row" height={24} />);
    expect(() =>
      rerender(<Seal id={{ ...SEAL_IDS.tx, created: Number.NaN }} form="row" height={24} />)
    ).toThrow('the cells created must be a whole number ≥ 0, got NaN');
  });

  // The pins are the design reference: one fixed id per kind and form, the generator's markup as
  // the stones were approved (round 16, "其他 ok"; round 17, "good"). A change to them is a change
  // to the design and is reviewed as one.
  for (const kind of KINDS)
    for (const form of FORMS)
      it(`pins the ${kind} ${form} seal of one id to the design reference`, async () => {
        const height = HEIGHT[form][kind];
        const markup = drawSeal(SEAL_IDS[kind], {
          form,
          height,
          state: 'pressed',
          ...SEAL_INKS,
        }).svg;
        await expect(markup).toMatchFileSnapshot(`../fixtures/seal/${kind}-${form}.svg`);
        const { container } = render(<Seal id={SEAL_IDS[kind]} form={form} height={height} />);
        expect(container.firstElementChild?.innerHTML).toBe(parsed(markup));
      });
});
