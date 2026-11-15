import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { adjacentUnits, phraseUnits } from '@/__tests__/utils/nowrap-scan';

/**
 * Gate for the pattern behind the A4 Chrome findings F1 and F2: an unbreakable unit with no way for
 * the line to break around it. What the AST states exactly, in `app/` and `components/`:
 *
 * (a) two units side by side in an inline line with nothing rendered between them (a JSX comment
 *     or a line break renders nothing): the home supply figure and its label, each nowrap, ran off
 *     the page. A unit is a unit renderer (NoBreak, ShortId, OutpointIndex, Utc8Date,
 *     Utc8DateTime), a shortened Hash / HexCode / Address (a `size`), or an element with a literal
 *     `whitespace-nowrap` class. Not counted: the items of a flex or grid box (items break between),
 *     a unit that is a box of its own line (`block`, `flex`, `grid`), an out point's index after
 *     its hash (`hash:0`, one unit by design).
 * (b) a unit around a phrase: NoBreak or a literal `whitespace-nowrap` whose children are only
 *     literal text of two or more words. A unit carries a value (an id, an index, a date, a number
 *     with its unit, a label with its id); a phrase may break between its words. An arrow is held
 *     to its word by a nowrap span of the arrow and its space alone, as ExternalLink holds ` ↗`.
 *
 * Not stated (no exact reading from the AST): a unit around a phrase that comes from an
 * expression (F2's holder name was a string from a function: its kind is decided where it is
 * known, `holderParts`), and a unit too wide for a non-wrapping row (a width, not a structure).
 */
function parse(code: string): ts.SourceFile {
  return ts.createSourceFile('t.tsx', code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
}
const a = (jsx: string) => adjacentUnits(parse(`const x = (${jsx});`)).map((f) => f.text);
const b = (jsx: string) => phraseUnits(parse(`const x = (${jsx});`)).map((f) => f.text);

describe('the nowrap gate reads the AST exactly (self-tests)', () => {
  it('(a) finds two units with nothing rendered between them in an inline line', () => {
    // F1's shape: a figure and its label, a comment between them
    expect(
      a(`<div className="text-index">
          <span className="whitespace-nowrap">{integer} CKB</span>
          {/* what the figure is */}
          <span className="text-small ml-3 whitespace-nowrap">in circulation</span>
        </div>`)
    ).toEqual(['span + span']);
    expect(
      a(`<p><ShortId value={h} kind="hash" size="row" /><Utc8Date timestamp={t} /></p>`)
    ).toEqual(['ShortId + Utc8Date']);
    expect(a(`<p><Hash hash={h} size="tight" />\n<NoBreak>{d}</NoBreak></p>`)).toEqual([
      'Hash + NoBreak',
    ]);
  });

  it('(a) passes units the line can break between, and boxes that are not inline', () => {
    // a space between the two
    expect(a(`<p><NoBreak>{x}</NoBreak>{' '}<NoBreak>{y}</NoBreak></p>`)).toEqual([]);
    expect(a(`<p><NoBreak>{x}</NoBreak> <NoBreak>{y}</NoBreak></p>`)).toEqual([]);
    // the items of a flex or grid box, also through a fragment
    expect(
      a(
        `<div className="flex flex-wrap gap-x-3"><span className="whitespace-nowrap">{x}</span><span className="whitespace-nowrap">{y}</span></div>`
      )
    ).toEqual([]);
    expect(
      a(
        `<nav className="flex flex-wrap">{ok ? <><Link className="whitespace-nowrap">a</Link><Link className="whitespace-nowrap">b</Link></> : null}</nav>`
      )
    ).toEqual([]);
    // each a block line of its own
    expect(
      a(
        `<td><span className="block whitespace-nowrap">{s} B</span><span className="block whitespace-nowrap">{c}</span></td>`
      )
    ).toEqual([]);
    // an out point: its index after its hash is one unit by design
    expect(
      a(`<Link><ShortId value={h} kind="hash" size="row" /><OutpointIndex index={0} /></Link>`)
    ).toEqual([]);
    // a whole Hash is its own scrolling box, not a unit of the line
    expect(a(`<p><Hash hash={h} truncate={false} /><NoBreak>{d}</NoBreak></p>`)).toEqual([]);
  });

  it('(b) finds a unit around a phrase of words', () => {
    expect(b(`<span className="whitespace-nowrap">in circulation</span>`)).toEqual([
      'in circulation',
    ]);
    expect(b(`<NoBreak>an unspendable lock</NoBreak>`)).toEqual(['an unspendable lock']);
    expect(b(`<a className="hover:underline whitespace-nowrap">How it works ↓</a>`)).toEqual([
      'How it works ↓',
    ]);
  });

  it('(b) passes a unit that carries a value, and an arrow held to its word', () => {
    expect(b(`<span className="whitespace-nowrap">fee <ExactCkb shannons={f} /></span>`)).toEqual(
      []
    );
    expect(b(`<span className="whitespace-nowrap">{n} cycles</span>`)).toEqual([]);
    expect(b(`<NoBreak>{holder}</NoBreak>`)).toEqual([]);
    expect(b(`<a>How it works<span className="whitespace-nowrap"> ↓</span></a>`)).toEqual([]);
    expect(b(`<span className="whitespace-nowrap">· </span>`)).toEqual([]);
    expect(b(`<span className="whitespace-nowrap">inventory</span>`)).toEqual([]);
  });
});

const ROOT = join(__dirname, '..', '..');
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx$/.test(name) ? [path] : [];
  });
}

describe('no line holds an unbreakable unit it cannot break around (A4 Chrome sweep)', () => {
  const found = { adjacent: [] as string[], phrase: [] as string[] };
  for (const dir of ['app', 'components'])
    for (const path of sources(join(ROOT, dir))) {
      const file = parse(readFileSync(path, 'utf8'));
      const rel = relative(ROOT, path);
      for (const f of adjacentUnits(file)) found.adjacent.push(`${rel}:${f.line}: ${f.text}`);
      for (const f of phraseUnits(file)) found.phrase.push(`${rel}:${f.line}: ${f.text}`);
    }

  it('(a) sets no two units side by side with nothing between them in an inline line', () => {
    expect(found.adjacent).toEqual([]);
  });

  it('(b) makes no phrase of words one unit', () => {
    expect(found.phrase).toEqual([]);
  });
});
