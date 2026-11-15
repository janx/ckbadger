import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { unprotectedUnits } from '@/__tests__/utils/unit-scan';

/**
 * Gate for units written as text (A4 round 2, the second Chrome pass): a date, a date-time, a date
 * range, a shortened id and a count are each one unit wherever a page writes them, and a count
 * breaks only between its number and its noun. Read from the AST, in `app/` and `components/`:
 *
 * - a call to a date formatter (formatUtc8Date, formatUtc8DateTime, formatUtc8Time,
 *   formatUnixSecondsUtc8), an id shortener (shortenHash, shortenId, shortenAddress,
 *   shortenOutpoint) or formatCount, inside an expression the JSX renders as a child — directly, in
 *   a template literal, a ternary, a concatenation;
 * - a field named for a date (`date`, `day`, `weekStart`, `activationDate`, `completeThrough`,
 *   `from`, `through`) rendered as a child as it is (`{peak.date}`);
 *
 * unless the string goes through `unitText` / `unitNode` (components/ui/units.tsx) on its way, or
 * an element around it renders it as a unit (NoBreak, ShortId, time, Utc8Date, Utc8DateTime,
 * Utc8Age, a literal whitespace-nowrap) or is SVG text, which never wraps.
 *
 * What it cannot see: a string composed elsewhere (a variable, a function's return, a describer)
 * and rendered as `{text}`; a string passed as a prop to a component that writes it — the rendering
 * components given strings (HeroMetric, AssetMetrics, Dotted, ListTable, PhoneRow, PromptHeading)
 * apply `unitNode` themselves; a date in a field it cannot name; and a unit that arrives inside
 * API text (third-party text — client versions, witness messages, DIDs — is out of scope).
 */
function parse(code: string): ts.SourceFile {
  return ts.createSourceFile('t.tsx', code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
}
const scan = (jsx: string) =>
  unprotectedUnits(parse(`const x = (${jsx});`)).map((f) => `${f.kind}: ${f.text}`);

describe('the unit gate reads the AST exactly (self-tests)', () => {
  it('finds a formatter call or a date field written as a child with no unit around it', () => {
    expect(scan(`<p>on {peak.date}</p>`)).toEqual(['field: peak.date']);
    expect(scan(`<span>{formatUtc8Date(t)}</span>`)).toEqual(['date: formatUtc8Date(t)']);
    expect(scan('<span>{`grace, to ${formatUtc8Date(g)}`}</span>')).toEqual([
      'date: formatUtc8Date(g)',
    ]);
    expect(scan('<p>{ok ? `one type id, ${shortenHash(h, "row")}` : null}</p>')).toEqual([
      'id: shortenHash(h, "row")',
    ]);
    expect(scan(`<dd>{formatCount(n, 'cell')}</dd>`)).toEqual(["count: formatCount(n, 'cell')"]);
  });

  it('passes a unit rendered as a unit, a string given to unitText, an attribute and SVG text', () => {
    expect(scan(`<NoBreak>{formatUtc8Date(t)}</NoBreak>`)).toEqual([]);
    expect(scan(`<time>{formatUtc8DateTime(t)}</time>`)).toEqual([]);
    expect(scan(`<span className="whitespace-nowrap">{shortenHash(h, 'row')}</span>`)).toEqual([]);
    expect(scan('<p>{unitText(`grace, to ${formatUtc8Date(g)}`)}</p>')).toEqual([]);
    expect(scan(`<p>{unitNode(peak.date)}</p>`)).toEqual([]);
    expect(scan(`<a title={formatUtc8DateTime(t)}>x</a>`)).toEqual([]);
    expect(scan(`<svg><text>{formatUtc8Date(t)}</text></svg>`)).toEqual([]);
    // a value computed for something else, not written as text here
    expect(scan(`<p>{rows.map((r) => r.day.length)}</p>`)).toEqual([]);
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

/**
 * Files another lane holds while this gate lands (the lead's brief, A4 round 2: lane B3 works on
 * the trait-value and genetics files in parallel); each goes when that lane has protected its
 * units. An entry that no longer has a finding fails (the last test).
 */
const ELSEWHERE: Readonly<Record<string, string>> = {
  'components/asset/genetics.tsx': "lane B3's genetics captions, in parallel",
  'components/asset/collection-evidence.tsx': "lane B3's trait-value files, in parallel",
  'components/asset/collection-objects.tsx': "lane B3's trait-value files, in parallel",
  'components/ui/allele-gel.tsx': "lane B3's genetics files, in parallel",
  'components/ui/sequencing-gel.tsx': "lane B3's genetics files, in parallel",
};

describe('every unit written as text is one unit (A4 round 2)', () => {
  const findings = (rel: string) =>
    unprotectedUnits(parse(readFileSync(join(ROOT, rel), 'utf8'))).map(
      (f) => `${rel}:${f.line}: ${f.kind}: ${f.text}`
    );

  it('writes no date, shortened id, count or date field as plain text', () => {
    const found: string[] = [];
    for (const dir of ['app', 'components'])
      for (const path of sources(join(ROOT, dir))) {
        const rel = relative(ROOT, path);
        if (rel in ELSEWHERE) continue;
        found.push(...findings(rel));
      }
    expect(found).toEqual([]);
  });

  it('keeps no file aside that has nothing left to protect', () => {
    for (const rel of Object.keys(ELSEWHERE)) expect(findings(rel), rel).not.toEqual([]);
  });
});
