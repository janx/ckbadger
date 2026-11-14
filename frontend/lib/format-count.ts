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

/** The no-break space that joins a count and its noun into one unit. */
const NBSP = ' ';

/**
 * `1 block`, `2 blocks`, `1,234 confirmations`: the noun agrees with the count (FF-D6, TX-34). The
 * count and its noun are one unit (U-16: "78,647,937" / "cells" broke apart), joined by a no-break
 * space, so a line breaks before the count or after the noun, never between them.
 */
export function formatCount(
  count: number | bigint,
  singular: string,
  plural = `${singular}s`
): string {
  const noun = countNoun(count, singular, plural);
  return `${count.toLocaleString('en-US')}${NBSP}${noun}`;
}
