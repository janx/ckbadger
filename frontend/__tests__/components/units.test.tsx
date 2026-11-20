import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { ReactNode } from 'react';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { NoteCount, unitText, unitNode } from '@/components/ui/units';
import { formatCount } from '@/lib/format-count';
import { shortenAddress, shortenHash, shortenOutpoint, type ShortSize } from '@/lib/shorten';

/**
 * A4 round 2 (the second Chrome pass): dates, shortened ids and counts broke inside — "2019-11-16"
 * at a hyphen, "0x9bd7e06f…a3cce8" after its ellipsis, "78,675,029 cells" inside "cells". The one
 * rendering point for a composed string: each unit in the forms the site's own formatters write
 * is one unit, a count breaks only between its number and its noun, and nothing is added to the
 * text (the strings below are the ones the pass measured).
 */
const units = (text: string) => {
  const { container } = render(<p>{unitText(text)}</p>);
  return {
    container,
    whole: [...container.querySelectorAll('[data-nobreak]')].map((u) => u.textContent),
    // re-pinned (A4 round 3): the unit's own marker; `data-count` is the filter chips' number
    counts: [...container.querySelectorAll<HTMLElement>('[data-count-unit]')],
  };
};

describe('unitText: the units a composed string carries', () => {
  it('keeps a date, a date-time, a time, a month and a range whole', () => {
    expect(units('on 2019-11-16').whole).toEqual(['2019-11-16']);
    expect(units(', decoded 2026-11-13 21:19:20 UTC+8').whole).toEqual([
      '2026-11-13 21:19:20 UTC+8',
    ]);
    expect(units('2026-11-15 12:03:29').whole).toEqual(['2026-11-15 12:03:29']);
    expect(units('mined 12:03:29 UTC+8').whole).toEqual(['12:03:29 UTC+8']);
    expect(units('2026-10 →').whole).toEqual(['2026-10']);
    expect(units('2026-11-08 → 2026-11-14').whole).toEqual(['2026-11-08 → 2026-11-14']);
    expect(units('Coverage: 2026-10-27 through 2026-11-14').whole).toEqual([
      '2026-10-27',
      '2026-11-14',
    ]);
  });

  it('keeps a shortened id whole, an out point with its index', () => {
    expect(units('pubkey hash 0x4a33…d637').whole).toEqual(['0x4a33…d637']);
    expect(units('MEMES · 0x5cb3…b4fa').whole).toEqual(['0x5cb3…b4fa']);
    // R9 (re-pinned): an address is `ckb1…` and its tail at every size, 8 or 6 characters
    expect(units('by the same miner (ckb1…4s93hhsj).').whole).toEqual(['ckb1…4s93hhsj']);
    expect(units('miner ckb1…93hhsj').whole).toEqual(['ckb1…93hhsj']);
    expect(units('cell 0x673e18…55f9:0').whole).toEqual(['0x673e18…55f9:0']);
  });

  it('sets a count as a unit that breaks only between its number and its noun', () => {
    const { counts, whole, container } = units(
      'one type id, 0x9bd7e06f…a3cce8, 1\u00a0binary · then 30\u00a0days of grace, to 2027-10-21'
    );
    expect(whole).toEqual(['0x9bd7e06f…a3cce8', '2027-10-21']);
    expect(counts.map((c) => c.textContent)).toEqual(['1 binary', '30 days']);
    for (const count of counts)
      expect(count).toHaveClass('inline-block', 'max-w-full', '[overflow-wrap:normal]');
    // a signed count keeps its sign with its number
    expect(units('+3\u00a0participants').counts.map((c) => c.textContent)).toEqual([
      '+3 participants',
    ]);
    // the copy is the text: nothing added, the count's no-break space a plain space
    expect(container.textContent).toBe(
      'one type id, 0x9bd7e06f…a3cce8, 1 binary · then 30 days of grace, to 2027-10-21'
    );
  });

  // A4 round 3: an underline is not drawn into an inline block, so "Show all 6,396 observations"
  // was underlined on "Show all" only. The unit takes its parent's decoration (text-decoration:
  // inherit): a link's or a button's underline, static or on hover, runs through the count.
  it('takes its parent’s text decoration, so an underline runs through it', () => {
    const [count] = units('Show all 6,396\u00a0observations').counts;
    expect(count).toHaveClass('[text-decoration:inherit]');
  });

  // A4 round 3: the unit holds the whole noun formatCount was given ("live cell", "DAO cell"),
  // read from formatCount's own joins (its words joined by the no-break space), never guessed from
  // the string; the noun is one piece, so where the count is wider than its line the break is
  // between the number and the noun only. A byte size written the same way ("85 B") is a count
  // by the same rule, by design.
  it('holds the whole noun it was given, the noun one piece after its number', () => {
    const [cells] = units('in 795,441\u00a0live\u00a0cells').counts;
    expect(cells.textContent).toBe('795,441 live cells');
    const noun = cells.lastElementChild as HTMLElement;
    expect(noun.textContent).toBe('live cells');
    expect(noun).toHaveClass('whitespace-nowrap');
    expect(cells.firstChild!.textContent).toBe('795,441 ');
    expect(units('1\u00a0DAO\u00a0cell').counts.map((c) => c.textContent)).toEqual(['1 DAO cell']);
    expect(units('85\u00a0B · fee').counts.map((c) => c.textContent)).toEqual(['85 B']);
  });

  it('leaves a string with no unit as it is, and a node it is not given as text', () => {
    expect(unitText('free to break')).toBe('free to break');
    const node = <b>x</b>;
    expect(unitNode(node)).toBe(node);
    expect(unitNode(null)).toBeNull();
  });
});

/** The largest count formatCount is given: a chain quantity, at most a u64. */
const U64_MAX = BigInt('18446744073709551615');
const HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
const ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqwgx292hnvmn68xf779vmzrshpmm6epn4c0cgwga';
const SIZES: ShortSize[] = ['head', 'row', 'tight'];
const plain = (s: string) => s.replaceAll('\u00a0', ' ');

/**
 * A4 round 4: unitText runs on strings the site does not write — a miner's witness message, a
 * token's name, a trait value, API text given to a shared component — so it must be linear in its
 * input, and read only the forms the formatters write: every quantifier bounded by what they can
 * write, and no unit begun inside a longer run of its own characters (a number longer than the
 * bound is plain text, not a unit cut out of its tail). Measured with Node on 200,000 characters:
 * the matcher of 1a8688aa took 25.2 s on a run of digits, 19.5 s on digits grouped by commas and
 * 13.7 s on digits and bare commas (quadratic), and read a unit out of the id, chain and date runs
 * below; the bounded matcher takes 0.1 to 3.1 ms on each (unitText in this suite). One run's
 * budget is 250 ms: about eighty times the slowest shape's linear time, under a fiftieth of what
 * the quadratic one took. A run over the budget but not ten times over gets two more tries, so a
 * pause of the parallel suite is not read as quadratic.
 */
describe('unitText on text the site does not write (A4 round 4)', () => {
  const LENGTH = 200_000;
  const BUDGET_MS = 250;
  const fill = (head: string, piece: string) =>
    (head + piece.repeat(Math.ceil(LENGTH / piece.length))).slice(0, LENGTH);
  const SHAPES: [string, string][] = [
    ['a run of digits', fill('', '1')],
    ['digits grouped by commas', fill('1', ',234')],
    ['digits and bare commas', fill('', '1,')],
    ['0x and a run of hex', fill('0x', 'a')],
    ['a shortened hash with a run of hex for its tail', fill('0x1234…', 'a')],
    ['the address alphabet after ckb1', fill('ckb1', 'qyqszqgp')],
    ['ckb1 repeated', fill('', 'ckb1')],
    ['a shortened address with a run for its tail', fill('ckb1qzda0cr0…', 'q')],
    ['a chain of ⍽-joined words after a number', fill('1', '\u00a0word')],
    ['⍽-joined digits', fill('', '1\u00a0')],
    ['clock-like and date-like digits', fill('', '2026-11-1412:03:29')],
  ];

  it.each(SHAPES)('reads %s, 200,000 characters, as plain text in linear time', (_, text) => {
    expect(text).toHaveLength(LENGTH);
    let result: ReactNode = null;
    let best = Infinity;
    for (let run = 0; run < 3 && best >= BUDGET_MS; run++) {
      const start = performance.now();
      result = unitText(text);
      const ms = performance.now() - start;
      best = Math.min(best, ms);
      if (ms > 10 * BUDGET_MS) break;
    }
    expect(result === text, 'no unit: the same string back').toBe(true);
    expect(best).toBeLessThan(BUDGET_MS);
  });

  it('still reads every form the formatters write, at the bounds', () => {
    for (const size of SIZES) {
      const hash = shortenHash(HASH, size);
      expect(units(`at ${hash}.`).whole).toEqual([hash]);
      // a tight address is `ckb1` and its tail: nothing between the prefix and the ellipsis
      const address = shortenAddress(ADDRESS, size);
      expect(units(`by ${address}.`).whole).toEqual([address]);
      const outPoint = shortenOutpoint(HASH, Number.MAX_SAFE_INTEGER, size);
      expect(units(`cell ${outPoint}`).whole).toEqual([outPoint]);
    }
    // the largest count, the noun of the most words, a punctuation mark after it
    const most = formatCount(U64_MAX, 'byte no field describes', 'bytes no field describes');
    expect(units(`(${most})`).counts.map((c) => c.textContent)).toEqual([`${plain(most)})`]);
    const longest = formatCount(Number.MAX_SAFE_INTEGER, 'confirmation');
    expect(units(`${longest},`).counts.map((c) => c.textContent)).toEqual([`${plain(longest)},`]);
  });

  it('reads no unit out of a longer run of its own characters', () => {
    // a count of more digits than a u64, and digits formatCount would have grouped
    expect(units('123,456,789,012,345,678,901,234\u00a0cells').counts).toEqual([]);
    expect(units('12345678\u00a0cells').counts).toEqual([]);
    expect(units('x1,234\u00a0cells').counts.map((c) => c.textContent)).toEqual(['1,234 cells']);
    // a date or a clock inside a longer run of digits
    expect(units('12026-11-14').whole).toEqual([]);
    expect(units('2026-11-142').whole).toEqual([]);
    expect(units('11:12:03:29 UTC+8').whole).toEqual([]);
    // a date followed by a label's colon is still a date
    expect(units('Week of 2026-11-09: 5\u00a0transfers').whole).toEqual(['2026-11-09']);
    // a head or a tail longer than any form's
    expect(units(`0x${'1'.repeat(11)}…abcd`).whole).toEqual([]);
    expect(units('0x5cb3…b4fab4fab').whole).toEqual([]);
    expect(units('0x5cb3…b4fa:12345678901234567').whole).toEqual([]);
  });
});

const ROOT = join(__dirname, '..', '..');
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx?$/.test(name) ? [path] : [];
  });
}

/** The nouns the source gives formatCount (or a file's `plural`, which calls it) as literals. */
function sourceNouns(): [string, string | undefined][] {
  const nouns: [string, string | undefined][] = [];
  for (const dir of ['app', 'components', 'lib', 'hooks'])
    for (const path of sources(join(ROOT, dir))) {
      const file = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest);
      const visit = (node: ts.Node): void => {
        if (
          ts.isCallExpression(node) &&
          ts.isIdentifier(node.expression) &&
          ['formatCount', 'plural'].includes(node.expression.text)
        ) {
          const [, one, many] = node.arguments;
          if (one && ts.isStringLiteralLike(one))
            nouns.push([one.text, many && ts.isStringLiteralLike(many) ? many.text : undefined]);
        }
        ts.forEachChild(node, visit);
      };
      visit(file);
    }
  return nouns;
}

// A4 round 4: the noun bounds are the source's — its longest noun word ("confirmations") and its
// noun of the most words ("bytes no field describes") — so a noun written later that is longer
// fails here instead of losing its unit on the page.
it('holds every noun the source gives formatCount, at the largest count', () => {
  const nouns = sourceNouns();
  expect(nouns.length).toBeGreaterThan(50);
  for (const [one, many] of nouns) {
    const text = formatCount(U64_MAX, one, many);
    expect(units(`${text})`).counts.map((c) => c.textContent)).toEqual([`${plain(text)})`]);
  }
});

// BRIEF-roles-rest part G: a count in a quiet note (a metric's note, a figure's second line) reads
// its number as the datum and its noun as its unit, whatever the note's ink
describe('NoteCount: a count in a note, its number read and its noun its unit', () => {
  it('reads the number in the reading ink and the noun quiet, one unit, the noun agreeing', () => {
    const { container } = render(
      <span className="text-text-dim">
        <NoteCount count={1234} singular="withdrawal" />, <NoteCount count={1} singular="deposit" />
        , <NoteCount count={2} singular="address" plural="addresses" />
      </span>
    );
    const counts = [...container.querySelectorAll('[data-note-count]')] as HTMLElement[];
    expect(counts.map((c) => c.textContent)).toEqual([
      '1,234 withdrawals',
      '1 deposit',
      '2 addresses',
    ]);
    for (const count of counts) {
      expect(count).toHaveAttribute('data-nobreak');
      expect(count.firstElementChild).toHaveClass('text-text');
      expect(count.lastElementChild).toHaveClass('text-text-dim');
    }
  });

  it('refuses a count that is no count, as groupCount and countNoun do', () => {
    expect(() => render(<NoteCount count={-1} singular="deposit" />)).toThrow('Invalid count -1');
  });
});
