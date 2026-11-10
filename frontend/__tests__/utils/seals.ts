import { expect } from 'vitest';
import { PAGE_SEAL_SIZES, PAGE_SEAL_TILT, type PageSealScale } from '@/components/ui/page-seal';
import { ROW_SEAL_PX } from '@/components/ui/row-seal';
import { compound } from '@/lib/talisman/compound';
import { RECIPES } from '@/lib/talisman/recipes';

/** HashSeal's default paints: the cinnabar mark on the page ground. */
const PAINT = { ink: 'var(--c-mark)', ground: 'var(--c-base-bg)' } as const;

/** The markup as the DOM serialises it once parsed. */
function parsed(markup: string): string {
  const holder = document.createElement('span');
  holder.innerHTML = markup;
  return holder.innerHTML;
}

/**
 * `root` holds exactly one seal, the page seal of `hash` at its scale (rubbing × Song print,
 * phase 3): the talisman drawn from that hash (compared as markup, so it is this hash's seal, not
 * one with the same first ten characters), at the wide size, tilted, with the narrow size below
 * md, pressed in a SealedHead of that scale. Returns the seal's holder.
 */
export function expectOnePageSeal(
  root: ParentNode,
  hash: string,
  scale: PageSealScale
): HTMLElement {
  const seals = root.querySelectorAll<HTMLElement>('[data-hash-seal]');
  expect(seals).toHaveLength(1);
  const [seal] = Array.from(seals);
  expect(seal).toHaveAttribute('data-hash-seal', 'page');
  const { wide, narrow } = PAGE_SEAL_SIZES[scale];
  const svg = seal.querySelector('svg');
  expect(svg).toHaveAttribute('role', 'img');
  expect(svg).toHaveAttribute('aria-label', `Hash seal ${hash.slice(0, 10)}`);
  expect(svg).toHaveAttribute('height', String(wide));
  expect(svg?.getAttribute('style')).toBe(`transform: rotate(${PAGE_SEAL_TILT}deg)`);
  expect(seal.innerHTML).toBe(
    parsed(compound(hash, RECIPES.page, wide, { ...PAINT, tilt: PAGE_SEAL_TILT }).svg)
  );
  expect(seal).toHaveClass(`max-md:[&>svg]:h-[${narrow}px]`, 'max-md:[&>svg]:w-auto');
  const head = seal.closest('[data-sealed-head]');
  expect(head).toHaveAttribute('data-sealed-head', scale);
  // The seal leads its head: the stamp first, the identity beside it.
  expect(head?.firstElementChild).toBe(seal);
  return seal;
}

/**
 * `row` holds exactly one seal, the row seal of `hash` at 28 px, an image and not a link (the
 * row's own link stays the row's link). Returns the seal's holder.
 */
export function expectOneRowSeal(row: Element, hash: string): HTMLElement {
  const seals = row.querySelectorAll<HTMLElement>('[data-hash-seal]');
  expect(seals).toHaveLength(1);
  const [seal] = Array.from(seals);
  expect(seal).toHaveAttribute('data-hash-seal', 'row');
  const svg = seal.querySelector('svg');
  expect(svg).toHaveAttribute('role', 'img');
  expect(svg).toHaveAttribute('aria-label', `Hash seal ${hash.slice(0, 10)}`);
  expect(svg).toHaveAttribute('height', String(ROW_SEAL_PX));
  expect(seal.innerHTML).toBe(parsed(compound(hash, RECIPES.row, ROW_SEAL_PX, PAINT).svg));
  expect(seal.closest('a')).toBeNull();
  expect(seal.querySelector('a')).toBeNull();
  return seal;
}

/** `first` comes before `second` in document order. */
export function expectBefore(first: Node, second: Node): void {
  expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
}
