import { Fragment, type ReactNode } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { ELLIPSIS, SHORT_FORMS, type IdKind } from '@/lib/shorten';

/**
 * The units a composed string carries, in the exact forms the site's own formatters write them
 * (A4 round 2): a line never breaks inside one, and a count breaks only between its number and its
 * noun. Each is read from its formatter's output, so nothing is guessed:
 *
 * - a date, a month, a date-time and a date range: `formatUtc8Date` (`2026-11-14`), a month cut
 *   from it (`2026-11`), `formatUtc8DateTime` / `formatUnixSecondsUtc8` (`2026-11-14 12:03:29
 *   UTC+8`), `formatUtc8Time` (`12:03:29 UTC+8`), and two of them joined by ` → `;
 * - a shortened id: `lib/shorten.ts`'s forms, a hex id or an address cut at the one ellipsis
 *   (`0x5cb3…b4fa`, `ckb1…5svwd5rt`), an out point's `:index` with it;
 * - a count: `formatCount`'s number and its whole noun, joined by its no-break spaces
 *   (`78,675,029 cells`, `795,441 live cells`: formatCount joins a noun's words too, so the unit
 *   holds what formatCount was given, never a guess at where a noun ends). A size written the same
 *   way (`85 B`) is a count by the same rule, on purpose: a number and its unit.
 *
 * Linear in its input (A4 round 4): it also reads strings the site does not write — a miner's
 * witness message, a token's name, a trait value, API text given to a shared component — so every
 * quantifier is bounded by what the formatters can write, and no unit begins or ends inside a
 * longer run of its own characters (a lookbehind and a lookahead of one or two characters): a
 * number, a head or a tail longer than its bound is plain text, never a unit cut out of it. A start
 * position costs one bounded attempt: 200,000 characters of each shape the units test times take
 * under 4 ms.
 */
const DAY = String.raw`(?:19|20)\d{2}-(?:0[1-9]|1[0-2])(?:-(?:0[1-9]|[12]\d|3[01]))?`;
const CLOCK = String.raw`\d{2}:\d{2}(?::\d{2})?`;
const ZONE = String.raw`(?: UTC\+8)?`;
const AT = String.raw`(?:${DAY}(?: ${CLOCK}${ZONE})?|${CLOCK} UTC\+8)`;
// not after a digit (or a digit and `:` / `-`), not before one: `12026-11-14` is no date; a
// label's colon after a date (`Week of 2026-11-09: …`) is not a digit
const TIME = String.raw`(?<!\d[:-]?)${AT}(?: → ${AT})?(?!\d|[:-]\d)`;

/** `{min,max}` of one part of a kind of id over lib/shorten.ts's three sizes, less its prefix. */
function spread(kind: IdKind, part: 'head' | 'tail', prefix = 0): string {
  const counts = Object.values(SHORT_FORMS).map((size) => size[kind][part] - prefix);
  return `{${Math.min(...counts)},${Math.max(...counts)}}`;
}
// a hash: heads 12, 10, 6 with `0x` counted (4–10 hex after it), tails 8, 6, 4
const HASH_ID = String.raw`0x[0-9a-fA-F]${spread('hash', 'head', 2)}${ELLIPSIS}[0-9a-fA-F]${spread('hash', 'tail')}`;
// an address: its network alone at every size, `ckb1` or `ckt1` and nothing after it (R9), so the
// head's spread is {0,0}; tails 8, 8, 6
const ADDRESS_ID = String.raw`ck[bt]1[0-9a-z]${spread('address', 'head', 4)}${ELLIPSIS}[0-9a-z]${spread('address', 'tail')}`;
// an out point's index: shortenOutpoint takes a safe integer, 16 digits at most
const INDEX_DIGITS = String(Number.MAX_SAFE_INTEGER).length;
const SHORT_ID = String.raw`(?<![0-9a-zA-Z])(?:${HASH_ID}(?::\d{1,${INDEX_DIGITS}})?|${ADDRESS_ID})(?![0-9a-zA-Z]|:\d)`;

// formatCount's number, in en-US groups: a safe integer has 16 digits, the one bigint it is given
// (tx-comment's common-knowledge delta in bytes) is a chain quantity, a u64 of 20 at most
const COUNT_DIGITS = (BigInt(2) ** BigInt(64) - BigInt(1)).toString().length;
const NUMBER = String.raw`\d{1,3}(?:,\d{3}){0,${Math.ceil(COUNT_DIGITS / 3) - 1}}`;
// its noun, read from the source (the units test holds every noun literal to it): the noun of the
// most words, `bytes no field describes`, has 4; the longest word, `confirmations`, has 13, and a
// composed string sets punctuation against the last (one mark today, `,` `.` `:` `)`): 16
const NOUN_WORDS = 4;
const NOUN_WORD = 16;
// a count with its sign where it has one ("+3 participants"): the sign is the number's; not after a
// digit, a comma or a point, and its last word followed by neither more of a word nor another word
const COUNT = String.raw`(?<![\d,.])[+−]?${NUMBER}(?:\u00a0[^\s\u00a0]{1,${NOUN_WORD}}){1,${NOUN_WORDS}}(?![\S\u00a0])`;
const UNIT = new RegExp(`(${TIME})|(${SHORT_ID})|(${COUNT})`, 'g');

/**
 * A count as a unit of its line: `78,675,029 cells` stays one piece where the line holds it (an
 * inline block moves to the next line whole), and only where it is wider than a whole line does it
 * break, between the number and the noun, never inside either: the noun is one piece (its words
 * nowrap) and a word is never cut (`overflow-wrap: normal`, whatever its container allows). The copy
 * is the plain text, the no-break spaces plain spaces.
 *
 * An inline block is drawn no decoration from its ancestors (CSS Text Decoration: it does not
 * propagate into atomic inlines), so the unit takes its parent's: `text-decoration: inherit` copies
 * the parent's line, style, colour and thickness, a hover's too (A4 round 3: "Show all 6,396
 * observations" was underlined on "Show all" only). A decoration set on a farther element, with an
 * element between, is not its parent's and is not drawn through it; the unit gate refuses that
 * nesting where the file shows it.
 *
 * `data-count-unit` names it (the filter chips' number carries `data-count`).
 */
function CountUnit({ text }: { text: string }) {
  const [number, ...noun] = text.split('\u00a0');
  return (
    <span
      data-count-unit=""
      className="inline-block max-w-full [overflow-wrap:normal] [text-decoration:inherit]"
    >
      {`${number} `}
      <span className="whitespace-nowrap">{noun.join(' ')}</span>
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
