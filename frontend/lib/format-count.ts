/**
 * The one plural rule (U-15): the noun that agrees with a count, `block` for 1 and `blocks` for
 * any other (0 included). For a count and its noun set apart on screen (the number in a brighter
 * ink); the two are one unit there too, kept whole by the caller's NoBreak.
 */
export function countNoun(
  count: number | bigint,
  singular: string,
  plural = `${singular}s`
): string {
  const valid =
    typeof count === 'bigint' ? count >= BigInt(0) : Number.isSafeInteger(count) && count >= 0;
  if (!valid) throw new Error(`Invalid count ${count} for ${singular}`);
  const one = typeof count === 'bigint' ? count === BigInt(1) : count === 1;
  return one ? singular : plural;
}

/**
 * A count written grouped (`5,653`; a zero is a count too): the number of `formatCount`, and a
 * count a control writes alone (a tab's, a filter chip's, a pager's figures; the controls lane C1).
 */
export function groupCount(count: number | bigint): string {
  const valid =
    typeof count === 'bigint' ? count >= BigInt(0) : Number.isSafeInteger(count) && count >= 0;
  if (!valid) throw new Error(`Invalid count ${count}`);
  return count.toLocaleString('en-US');
}

/** The no-break space that joins a count and its noun into one unit. */
const NBSP = '\u00a0';

/**
 * `1 block`, `2 blocks`, `1,234 confirmations`: the noun agrees with the count (FF-D6, TX-34). The
 * count and its noun are one unit (U-16: "78,647,937" / "cells" broke apart), joined by a no-break
 * space, so a line breaks before the count or after the noun, never between them. A noun of several
 * words is the noun whole ("795,441 live cells", "1 DAO cell"): its words are joined by the
 * no-break space too (A4 round 3), so the unit holds what it was given and a renderer reads it
 * exactly (components/ui/units.tsx), never guessing where a noun ends.
 */
export function formatCount(
  count: number | bigint,
  singular: string,
  plural = `${singular}s`
): string {
  const noun = countNoun(count, singular, plural);
  return `${groupCount(count)}${NBSP}${noun.split(' ').join(NBSP)}`;
}
