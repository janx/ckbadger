/**
 * The one shortening rule for ids (UI review 2026-11-14, U-09, PB-19; readability 2026-11-18, R9;
 * `docs/RESPONSIVE_LAYOUT.md`, "Formats"). An id is a hash or an address; any other hex id (script
 * args, a peer id, a record value, a channel id) takes the hash column. It is written at one of
 * three sizes:
 *
 * | size    | hash | address | where                                                          |
 * | ------- | ---- | ------- | -------------------------------------------------------------- |
 * | `head`  | 12…8 | 4…8     | an h1 and a title line only                                    |
 * | `row`   | 10…6 | 4…8     | tables, feeds, tiles, and prose: a lede, a sentence, a note    |
 * | `tight` | 6…4  | 4…6     | below `sm` (a phone row), an SVG or tile label under 200 px,   |
 * |         |      |         | a browser tab's title                                          |
 *
 * An address keeps its network and its identity: `ckb1` (or `ckt1`) and its tail, never more of
 * its head. The characters after `ckb1` are the address format and the lock's code hash, the same
 * for every address of a lock: two default-lock addresses share their first 58 characters, so a
 * head of `ckb1qzda0c` spends six characters on what every row of a list has in common and the
 * tail is all that tells the rows apart. A lock is named beside the address (the row's lock
 * label), never by the head of it. An address is 13 characters at the head and row sizes, 11 at
 * tight (a tight hash's width).
 *
 * A page writes one id in one form: prose takes the row form, for a hash too, so a party named in
 * a lede is the same string as in the table under it; `head` is for the page's own name. A phone
 * row that is also drawn in a desktop column, or a tile under 200 px only on a phone, takes its
 * size from `useTightBelowSm` (tight below `sm`, the row size from it).
 *
 * Counts are characters of the value as written (`0x` and `ckb1` included). The ellipsis is the
 * one character `…`. A value no longer than its shortened form is written whole. An out point is
 * its hash at that size and `:N`.
 *
 * This module is the one place an id is cut and the ellipsis is written
 * (`__tests__/lib/shorten-gate.test.ts`, which also holds where `head` and `tight` may be chosen).
 * JSX writes a shortened id through its renderer (`HexCode`, `Hash`, `Address`, `ShortId`, which
 * keep it one unbroken unit); a string that is not JSX (an SVG or canvas label, a chart model, a
 * document title) calls the functions here.
 */

import { foreignGraphemes } from '@/lib/type-metrics';

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
  head: { hash: { head: 12, tail: 8 }, address: { head: 4, tail: 8 } },
  row: { hash: { head: 10, tail: 6 }, address: { head: 4, tail: 8 } },
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

/**
 * A text that is not ours in a figure — a gene's name, rule or trait value, a pool's name, an
 * organisation (review F-B, logic finding 5) — cut to `room` px at `px` by the upper bound
 * `foreignGraphemes` (lib/type-metrics.ts) gives each of its grapheme clusters: as many whole
 * clusters as fit with the one ellipsis, so a cut never splits an emoji or a letter from its
 * accent. Whole where it fits. A room that cannot hold the first cluster and the ellipsis is
 * refused.
 */
export function clipForeignLabel(text: string, px: number, room: number): string {
  const graphemes = foreignGraphemes(text);
  const width = (units: number) => (units * px) / 1000;
  if (width(graphemes.reduce((sum, g) => sum + g.units, 0)) <= room) return text;
  let units = foreignGraphemes(ELLIPSIS)[0].units;
  let kept = '';
  for (const g of graphemes) {
    if (width(units + g.units) > room) break;
    units += g.units;
    kept += g.grapheme;
  }
  if (kept === '')
    throw new Error(
      `clipForeignLabel: ${room} px at ${px} px cannot hold "${graphemes[0].grapheme}" and the ellipsis`
    );
  return `${kept}${ELLIPSIS}`;
}
