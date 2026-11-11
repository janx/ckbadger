import { createHash } from 'node:crypto';
import { expect } from 'vitest';
import { PAGE_SEAL_HEIGHTS, PAGE_SEAL_TILT } from '@/components/ui/page-seal';
import { OWNER_SEAL } from '@/components/ui/owner-seal';
import { ROW_SEAL } from '@/components/ui/row-seal';
import {
  drawSeal,
  sealLabel,
  type CellClass,
  type SealId,
  type SealKind,
  type SealState,
} from '@/lib/seal/stones';

/** Seal's paints (the seals by kind): the cinnabar mark, the page ground, the dead ink. */
export const SEAL_INKS = {
  ink: 'var(--c-mark)',
  ground: 'var(--c-base-bg)',
  dead: 'var(--c-text-dim)',
} as const;

/** The markup as the DOM serialises it once parsed. */
export function parsed(markup: string): string {
  const holder = document.createElement('span');
  holder.innerHTML = markup;
  return holder.innerHTML;
}

/** A 32-byte hash from a seed (sha256): fixed, and a different stone for every seed. */
export function sealHash(seed: string): string {
  return '0x' + createHash('sha256').update(`ckbadger seal fixture ${seed}`).digest('hex');
}

const CLASSES: readonly CellClass[] = ['tokens', 'dao', 'objects', 'identities', 'other'];
const SHANNONS = BigInt(100000000);

/**
 * One id of each kind from the i-th seeds, the counts, the class (or no type) and the capacity
 * varying with i: for drawing many different seals of a kind.
 */
export function sealIdsAt(i: number): { [K in SealKind]: Extract<SealId, { kind: K }> } {
  const tx = sealHash(`tx-${i}`);
  return {
    block: { kind: 'block', hash: sealHash(`block-${i}`), txs: [1, 7, 12, 248, 1820][i % 5] },
    tx: { kind: 'tx', hash: tx, consumed: i % 4, created: (i * 7) % 13 },
    cell: {
      kind: 'cell',
      tx,
      index: i,
      lock: sealHash(`lock-${i}`),
      type: i % 6 === 5 ? null : { class: CLASSES[i % 5], hash: sealHash(`type-${i}`) },
      capacity: BigInt(61 * 10 ** (i % 9)) * SHANNONS,
    },
    script: { kind: 'script', hash: sealHash(`script-${i}`) },
  };
}

/**
 * The fixed ids the components' tests and the reference drawings use: a block of 248
 * transactions, a transaction of 2 → 3 cells, its output 1 (a token cell of 1,204 CKB), a script.
 */
export const SEAL_IDS: { [K in SealKind]: Extract<SealId, { kind: K }> } = {
  block: { kind: 'block', hash: sealHash('block'), txs: 248 },
  tx: { kind: 'tx', hash: sealHash('tx'), consumed: 2, created: 3 },
  cell: {
    kind: 'cell',
    tx: sealHash('tx'),
    index: 1,
    lock: sealHash('lock'),
    type: { class: 'tokens', hash: sealHash('cell-type') },
    capacity: BigInt(1204) * SHANNONS,
  },
  script: { kind: 'script', hash: sealHash('script') },
};

/**
 * `root` holds exactly one page seal of the seals by kind, the stone of `id` in `state` (pressed
 * when not given): the drawing of that id (compared as markup with `drawSeal`'s, so it is this
 * id's seal, with these counts, this class, this capacity), at its kind's wide height, tilted,
 * with its narrow height below md, leading a SealedHead of its kind. Returns the seal's holder.
 * Row seals in `root` (a page's lists) are not counted.
 */
export function expectOnePageSeal(
  root: ParentNode,
  id: SealId,
  state: SealState = 'pressed'
): HTMLElement {
  const seals = root.querySelectorAll<HTMLElement>('[data-seal][data-seal-form="page"]');
  expect(seals).toHaveLength(1);
  const [seal] = Array.from(seals);
  expect(seal).toHaveAttribute('data-seal', id.kind);
  expect(seal).toHaveAttribute('data-seal-state', state);
  const { wide, narrow } = PAGE_SEAL_HEIGHTS[id.kind];
  const svg = seal.querySelector('svg');
  expect(svg).toHaveAttribute('role', 'img');
  expect(svg).toHaveAttribute('aria-label', sealLabel(id));
  expect(svg).toHaveAttribute('height', String(wide));
  expect(svg?.getAttribute('style')).toBe(`transform: rotate(${PAGE_SEAL_TILT}deg)`);
  expect(seal.innerHTML).toBe(
    parsed(
      drawSeal(id, { form: 'page', height: wide, state, ...SEAL_INKS, tilt: PAGE_SEAL_TILT }).svg
    )
  );
  expect(seal).toHaveClass(`max-md:[&>svg]:h-[${narrow}px]`, 'max-md:[&>svg]:w-auto');
  const head = seal.closest('[data-sealed-head]');
  expect(head).toHaveAttribute('data-sealed-head', id.kind);
  // The seal leads its head: the stamp first, the identity beside it.
  expect(head?.firstElementChild).toBe(seal);
  return seal;
}

/** The parts of a sealed head (the name plate, seals by kind C10), as `expectNamePlate` finds them. */
export interface NamePlate {
  head: HTMLElement;
  stone: HTMLElement;
  plate: HTMLElement;
  kindLine: HTMLElement;
  nameBox: HTMLElement;
  title: HTMLElement;
  idLine: HTMLElement;
  below: HTMLElement | null;
}

/** The plate's id line's own classes (C10 round 2): the kind line's quiet voice, set once. */
export const ID_LINE_CLASS = 'text-text-dim text-small min-w-0 [overflow-wrap:anywhere]';

/** A class that sets a text size or ink, at any variant (`text-body`, `md:text-small`, …). */
const VOICE_CLASS = /(^|\s)([a-z-]+:)*text-/;

/**
 * `idLine` is a name plate's id line in its one quiet voice (seals by kind, C10 round 2): exactly
 * `ID_LINE_CLASS` (text-small in text-text-dim, the kind line's voice), and nothing in it outside
 * its copy control sets a size or an ink of its own, so the hash, the out point or the address
 * takes the line's voice on every page and a page cannot drift from it.
 */
export function expectQuietIdLine(idLine: HTMLElement): void {
  expect(idLine).toHaveAttribute('data-sealed-id');
  expect(idLine.className).toBe(ID_LINE_CLASS);
  const voiced = Array.from(idLine.querySelectorAll<HTMLElement>('[class]')).filter(
    (element) =>
      element.closest('button') === null && VOICE_CLASS.test(element.getAttribute('class')!)
  );
  const markup = voiced.map((element) => element.outerHTML).join(' ');
  expect(voiced, `the id line's pieces set a voice of their own: ${markup}`).toEqual([]);
}

/**
 * `root` holds exactly one sealed head and exactly one h1, laid out as the name plate (seals by
 * kind, C10): the head a two-column grid whose rows are centred (`items-center`) and whose children
 * are exactly the stone, the plate and — when there is one — `below`; the plate `contents` below md
 * (its two boxes are the grid's own items there) and a column from md; its first box the kind line,
 * opening on `kind` alone in the dim small voice; its second the name-and-id box, spanning both
 * columns below md, holding exactly the h1 and the id line in its one quiet voice
 * (`expectQuietIdLine`); `below` spanning both columns at every width. jsdom has
 * no layout, so the classes that make the arrangement are what is pinned; none of the head's boxes
 * is hidden at any width (one DOM). Returns the parts.
 */
export function expectNamePlate(root: ParentNode, kind: string): NamePlate {
  const heads = root.querySelectorAll<HTMLElement>('[data-sealed-head]');
  expect(heads).toHaveLength(1);
  const [head] = Array.from(heads);
  expect(head).toHaveClass(
    'grid',
    'min-w-0',
    'grid-cols-[auto_minmax(0,1fr)]',
    'items-center',
    'gap-x-4',
    'gap-y-4',
    'md:gap-x-5',
    'md:gap-y-5'
  );
  const titles = root.querySelectorAll<HTMLElement>('h1');
  expect(titles).toHaveLength(1);
  const [title] = Array.from(titles);
  const parts = Array.from(head.children) as HTMLElement[];
  expect([2, 3]).toContain(parts.length);
  const [stone, plate, below = null] = parts;
  expect(stone).toHaveAttribute('data-seal-form', 'page');
  expect(plate).toHaveAttribute('data-sealed-plate');
  expect(plate).toHaveClass('contents', 'md:flex', 'md:min-w-0', 'md:flex-col', 'md:gap-2');
  expect(plate.children).toHaveLength(2);
  const [kindLine, nameBox] = Array.from(plate.children) as HTMLElement[];
  expect(kindLine).toHaveAttribute('data-sealed-kind');
  expect(kindLine).toHaveClass('flex', 'min-w-0', 'flex-wrap', 'items-baseline', 'gap-x-3');
  const word = kindLine.firstElementChild as HTMLElement;
  expect(word.tagName).toBe('SPAN');
  expect(word.textContent).toBe(kind);
  expect(word).toHaveClass('text-text-dim', 'text-small');
  expect(nameBox).toHaveAttribute('data-sealed-name');
  expect(nameBox).toHaveClass('flex', 'min-w-0', 'flex-col', 'gap-2', 'max-md:col-span-2');
  expect(nameBox.children).toHaveLength(2);
  const idLine = nameBox.lastElementChild as HTMLElement;
  expect(nameBox.firstElementChild).toBe(title);
  expectQuietIdLine(idLine);
  if (below !== null) {
    expect(below).toHaveAttribute('data-sealed-below');
    expect(below).toHaveClass('col-span-2', 'min-w-0');
    expect(below.className).not.toMatch(/col-start/);
  }
  for (const box of [head, stone, plate, kindLine, nameBox, title, idLine, below])
    expect(box?.className ?? '').not.toMatch(/(^|\s|:)(hidden|sr-only|invisible)(\s|$)/);
  return { head, stone, plate, kindLine, nameBox, title, idLine, below };
}

/**
 * `row` holds exactly one seal of the seals by kind, the row stone of `id` in `state` (pressed
 * when not given): the drawing of that id (compared as markup with `drawSeal`'s), at its kind's
 * row height and drawn width, with the 8 px gap after it; an image and not a link (the row's own
 * link stays the row's link). Owner stones before an address in the row (OwnerSeal) are not the
 * row's seal and are not counted. Returns the seal's holder.
 */
export function expectOneRowSeal(
  row: Element,
  id: SealId,
  state: SealState = 'pressed'
): HTMLElement {
  const seals = Array.from(row.querySelectorAll<HTMLElement>('[data-seal]')).filter(
    (s) => s.closest('[data-owner-seal]') === null
  );
  expect(seals).toHaveLength(1);
  const [seal] = Array.from(seals);
  expect(seal).toHaveAttribute('data-seal', id.kind);
  expect(seal).toHaveAttribute('data-seal-form', 'row');
  expect(seal).toHaveAttribute('data-seal-state', state);
  const { height, width } = ROW_SEAL[id.kind];
  const svg = seal.querySelector('svg');
  expect(svg).toHaveAttribute('role', 'img');
  expect(svg).toHaveAttribute('aria-label', sealLabel(id));
  expect(svg).toHaveAttribute('height', String(height));
  expect(svg).toHaveAttribute('width', String(width));
  expect(seal.innerHTML).toBe(
    parsed(drawSeal(id, { form: 'row', height, state, ...SEAL_INKS }).svg)
  );
  expect(seal).toHaveClass('mr-2', '[&>svg]:block');
  expect(seal.closest('a')).toBeNull();
  expect(seal.querySelector('a')).toBeNull();
  return seal;
}

/**
 * `holder` (an address's line in a cell row) holds exactly one owner's stone, the 20 px script row
 * stone of `lockHash` (compared as markup with `drawSeal`'s), an image and not a link, with the
 * 5 px gap after it. Returns the stone's holder (`[data-owner-seal]`).
 */
export function expectOwnerSealOf(holder: Element, lockHash: string): HTMLElement {
  const owners = holder.querySelectorAll<HTMLElement>('[data-owner-seal]');
  expect(owners).toHaveLength(1);
  const [owner] = Array.from(owners);
  const seals = owner.querySelectorAll<HTMLElement>('[data-seal]');
  expect(seals).toHaveLength(1);
  const [seal] = Array.from(seals);
  const id: SealId = { kind: 'script', hash: lockHash };
  expect(seal).toHaveAttribute('data-seal', 'script');
  expect(seal).toHaveAttribute('data-seal-form', 'row');
  expect(seal).toHaveAttribute('data-seal-state', 'pressed');
  const svg = seal.querySelector('svg');
  expect(svg).toHaveAttribute('role', 'img');
  expect(svg).toHaveAttribute('aria-label', sealLabel(id));
  expect(svg).toHaveAttribute('height', String(OWNER_SEAL.height));
  expect(svg).toHaveAttribute('width', String(OWNER_SEAL.width));
  expect(seal.innerHTML).toBe(
    parsed(
      drawSeal(id, { form: 'row', height: OWNER_SEAL.height, state: 'pressed', ...SEAL_INKS }).svg
    )
  );
  expect(owner).toHaveClass('mr-[5px]');
  expect(owner.closest('a')).toBeNull();
  expect(owner.querySelector('a')).toBeNull();
  return owner;
}

/** `first` comes before `second` in document order. */
export function expectBefore(first: Node, second: Node): void {
  expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
}
