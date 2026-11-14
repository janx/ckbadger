/**
 * The one shortening rule for ids (UI review 2026-11-14, U-09, PB-19; `docs/RESPONSIVE_LAYOUT.md`,
 * "Formats"). An id is a hash or an address; any other hex id (script args, a peer id, a record
 * value, a channel id) takes the hash column. It is written at one of three sizes:
 *
 * | size    | hash | address | where                                              |
 * | ------- | ---- | ------- | -------------------------------------------------- |
 * | `head`  | 12…8 | 12…8    | an h1, a lede                                      |
 * | `row`   | 10…6 | 10…6    | tables, feeds, tiles                               |
 * | `tight` | 6…4  | 4…6     | a phone row, a chart label, a tile under 200 px    |
 *
 * A tight address keeps the network in its head (`ckb1`, `ckt1`) and its identity in the tail; the
 * characters after `ckb1` start the lock's code hash, the same for every address of a lock. At 11
 * characters it is a tight hash's width.
 *
 * Counts are characters of the value as written (`0x` and `ckb1` included). The ellipsis is the
 * one character `…`. A value no longer than its shortened form is written whole. An out point is
 * its hash at that size and `:N`.
 *
 * This module is the one place an id is cut and the ellipsis is written
 * (`__tests__/lib/shorten-gate.test.ts`). JSX writes a shortened id through its renderer
 * (`HexCode`, `Hash`, `Address`, `ShortId`, which keep it one unbroken unit); a string that is not
 * JSX (an SVG or canvas label, a chart model, a document title) calls the functions here.
 */

export type ShortSize = 'head' | 'row' | 'tight';
export type IdKind = 'hash' | 'address';

export interface ShortForm {
  /** Characters kept at the start, `0x` / `ckb1` included. */
  readonly head: number;
  /** Characters kept at the end. */
  readonly tail: number;
}

/** The one ellipsis character. */
export const ELLIPSIS = '…';

/** The three sizes for the two kinds of id. */
export const SHORT_FORMS: Readonly<Record<ShortSize, Readonly<Record<IdKind, ShortForm>>>> = {
  head: { hash: { head: 12, tail: 8 }, address: { head: 12, tail: 8 } },
  row: { hash: { head: 10, tail: 6 }, address: { head: 10, tail: 6 } },
  tight: { hash: { head: 6, tail: 4 }, address: { head: 4, tail: 6 } },
};

/** An id of a kind at a size; whole when it is no longer than the shortened form. */
export function shortenId(value: string, kind: IdKind, size: ShortSize): string {
  if (typeof value !== 'string')
    throw new Error(`shorten: a ${kind} is not a string: ${JSON.stringify(value)}`);
  if (value.length === 0) throw new Error(`shorten: an empty ${kind}`);
  const forms = SHORT_FORMS[size];
  if (forms === undefined) throw new Error(`shorten: no size ${JSON.stringify(size)}`);
  const { head, tail } = forms[kind];
  if (value.length <= head + 1 + tail) return value;
  return `${value.slice(0, head)}${ELLIPSIS}${value.slice(-tail)}`;
}

/** A hash, or any other hex id, at a size. */
export function shortenHash(value: string, size: ShortSize): string {
  return shortenId(value, 'hash', size);
}

/** An address at a size. */
export function shortenAddress(value: string, size: ShortSize): string {
  return shortenId(value, 'address', size);
}

/** An out point: its transaction hash at a size and `:N`. */
export function shortenOutpoint(txHash: string, index: number | string, size: ShortSize): string {
  const n = typeof index === 'number' ? index : /^\d+$/.test(index) ? Number(index) : NaN;
  if (!Number.isSafeInteger(n) || n < 0)
    throw new Error(`shorten: an out point index that is not a count: ${JSON.stringify(index)}`);
  return `${shortenHash(txHash, size)}:${n}`;
}

/**
 * A text label (a name, a gene, a miner's pool) cut by code to a character budget the caller
 * measured, the ellipsis counted in it. Where the cut is only for width and the element has one,
 * CSS truncation with the whole text in `title` is preferred; this is for SVG and canvas labels.
 */
export function clipLabel(text: string, chars: number): string {
  if (!Number.isInteger(chars) || chars < 2)
    throw new Error(`clipLabel: a budget of ${chars} characters cannot hold a cut label`);
  return text.length <= chars ? text : `${text.slice(0, chars - 1)}${ELLIPSIS}`;
}
