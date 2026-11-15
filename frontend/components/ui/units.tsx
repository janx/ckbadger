import { Fragment, type ReactNode } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { ELLIPSIS } from '@/lib/shorten';

/**
 * The units a composed string carries, in the exact forms the site's own formatters write them
 * (A4 round 2): a line never breaks inside one, and a count breaks only between its number and its
 * noun. Each is read from its formatter's output, so nothing is guessed:
 *
 * - a date, a month, a date-time and a date range: `formatUtc8Date` (`2026-11-14`), a month cut
 *   from it (`2026-11`), `formatUtc8DateTime` / `formatUnixSecondsUtc8` (`2026-11-14 12:03:29
 *   UTC+8`), `formatUtc8Time` (`12:03:29 UTC+8`), and two of them joined by ` → `;
 * - a shortened id: `lib/shorten.ts`'s forms, a hex id or an address cut at the one ellipsis
 *   (`0x5cb3…b4fa`, `ckb1qzda0cr0…5svwd5rt`), an out point's `:index` with it;
 * - a count: `formatCount`'s number and noun, joined by its no-break space (`78,675,029 cells`).
 */
const DAY = String.raw`(?:19|20)\d{2}-(?:0[1-9]|1[0-2])(?:-(?:0[1-9]|[12]\d|3[01]))?`;
const CLOCK = String.raw`\d{2}:\d{2}(?::\d{2})?`;
const ZONE = String.raw`(?: UTC\+8)?`;
const AT = String.raw`(?:${DAY}(?: ${CLOCK}${ZONE})?|${CLOCK} UTC\+8)`;
const TIME = String.raw`${AT}(?: → ${AT})?`;
const SHORT_ID = String.raw`(?:0x[0-9a-fA-F]+|ck[bt]1[0-9a-z]+)${ELLIPSIS}[0-9a-zA-Z]+(?::\d+)?`;
// a count with its sign where it has one ("+3 participants"): the sign is the number's
const COUNT = String.raw`[+−]?\d[\d,]*(?:\.\d+)?\u00a0[^\s\u00a0]+`;
const UNIT = new RegExp(`(${TIME})|(${SHORT_ID})|(${COUNT})`, 'g');

/**
 * A count as a unit of its line: `78,675,029 cells` stays one piece where the line holds it (an
 * inline block moves to the next line whole), and only where it is wider than a whole line does it
 * break, between the number and the noun (the space inside it), never inside either: a word is never
 * cut (`overflow-wrap: normal`, whatever its container allows). The copy is the plain text.
 */
function CountUnit({ text }: { text: string }) {
  return (
    <span data-count="" className="inline-block max-w-full [overflow-wrap:normal]">
      {text.replace('\u00a0', ' ')}
    </span>
  );
}

/**
 * A composed string with its units protected (A4 round 2): each date, time, range and shortened
 * id is one unit (`NoBreak`), each count a unit that breaks only between its number and its noun;
 * the rest is plain text that breaks as text does. No character is added: the copy is the string.
 * The one rendering point for a string that carries units — a rendering component applies it to the
 * strings it is given (HeroMetric, AssetMetrics, Dotted, ListTable, PhoneRow, PromptHeading), a
 * page applies it where it writes a composed string itself.
 */
export function unitText(text: string): ReactNode {
  const parts: ReactNode[] = [];
  let at = 0;
  for (const match of text.matchAll(UNIT)) {
    const start = match.index ?? 0;
    if (start > at) parts.push(text.slice(at, start));
    const [whole, , , count] = match;
    parts.push(
      count !== undefined ? (
        <CountUnit key={start} text={whole} />
      ) : (
        <NoBreak key={start}>{whole}</NoBreak>
      )
    );
    at = start + whole.length;
  }
  if (parts.length === 0) return text;
  if (at < text.length) parts.push(text.slice(at));
  return <Fragment>{parts}</Fragment>;
}

/** `unitText` for a node a component is given: a string is read, anything else stays as it is. */
export function unitNode(node: ReactNode): ReactNode {
  return typeof node === 'string' ? unitText(node) : node;
}
