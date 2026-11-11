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

/**
 * `row` holds exactly one seal of the seals by kind, the row stone of `id` in `state` (pressed
 * when not given): the drawing of that id (compared as markup with `drawSeal`'s), at its kind's
 * row height and drawn width, with the 8 px gap after it (none with `gap: false`, a stone standing
 * alone in a cell tile's mark column, the column's gap its gap); an image and not a link (the
 * row's own link stays the row's link). Owner stones before an address in the row (OwnerSeal) are
 * not the row's seal and are not counted. Returns the seal's holder.
 */
export function expectOneRowSeal(
  row: Element,
  id: SealId,
  state: SealState = 'pressed',
  { gap = true }: { gap?: boolean } = {}
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
  expect(seal).toHaveClass('[&>svg]:block');
  if (gap) expect(seal).toHaveClass('mr-2');
  else expect(seal.className).not.toMatch(/(^|\s)m[rx]-/);
  expect(seal.closest('a')).toBeNull();
  expect(seal.querySelector('a')).toBeNull();
  return seal;
}

/**
 * `holder` (an address's line in a cell row) holds exactly one owner's stone, the 20 px script row
 * stone of `lockHash` (compared as markup with `drawSeal`'s), an image and not a link, with the
 * 5 px gap after it (none with `gap: false`, in a cell tile's mark column). Returns the stone's
 * holder (`[data-owner-seal]`).
 */
export function expectOwnerSealOf(
  holder: Element,
  lockHash: string,
  { gap = true }: { gap?: boolean } = {}
): HTMLElement {
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
  if (gap) expect(owner).toHaveClass('mr-[5px]');
  else expect(owner.className).not.toMatch(/(^|\s)m[rx]-/);
  expect(owner.closest('a')).toBeNull();
  expect(owner.querySelector('a')).toBeNull();
  return owner;
}

/** `first` comes before `second` in document order. */
export function expectBefore(first: Node, second: Node): void {
  expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
}
