import { createHash } from 'node:crypto';
import { expect } from 'vitest';
import { PAGE_SEAL_SIZES, PAGE_SEAL_TILT, type PageSealScale } from '@/components/ui/page-seal';
import { ROW_SEAL_PX } from '@/components/ui/row-seal';
import type { CellClass, SealId, SealKind } from '@/lib/seal/stones';
import { compound } from '@/lib/talisman/compound';
import { RECIPES } from '@/lib/talisman/recipes';

/** HashSeal's default paints: the cinnabar mark on the page ground. */
const PAINT = { ink: 'var(--c-mark)', ground: 'var(--c-base-bg)' } as const;

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
