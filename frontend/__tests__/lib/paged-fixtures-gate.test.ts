import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Every fixture of a paged read in the suite goes through the checker (lane cd-fixes-controls,
 * review-p-tests SHOULD-5). A fixture that contradicts itself hid a crash: 14 rows on a page of 20
 * that said more rows followed could not show what a real last page does, and the address page's
 * page one with its pool's rows was a shape the checker refused and no test served. So a page a
 * test serves is checked as it is made: `pagedRead` (__tests__/utils/paged-read.ts) refuses a page
 * the API cannot send.
 *
 * The reader: an object literal in `__tests__/**` whose keys include `hasMore` and `nextCursor`
 * (a page of a cursor-paged list, as the API sends one) is a paged fixture. It passes where it is
 * written inside the argument of a call of `pagedRead(…)`, through the expressions it may sit in
 * (parentheses, `as`, `satisfies`, a spread into the page, a `?:` branch, an array of rows).
 *
 * Its limits, what it cannot see: a page built with no literal holding both keys (its keys spread
 * from a variable, or computed), a page written in a JSON fixture (`*.json` is not read: the
 * script pages' family fixtures hold their cells' page there, and their tests serve it through
 * `pagedRead` where they page it), and a page a helper outside `__tests__/` builds.
 *
 * EXEMPT: the files whose paged literals are inputs a test hands to the helper or to the checker
 * on purpose, to see them refused or drawn as the states live data reaches.
 */
const EXEMPT: Readonly<Record<string, string>> = {
  '__tests__/lib/pagination.test.ts':
    'the helper’s own tests: pages that contradict themselves are its inputs, to see them refused',
  '__tests__/utils/paged-read.test.ts':
    'the checker’s own tests: pages the API cannot send are its inputs, to see them refused',
};

const ROOT = join(__dirname, '..', '..');

/** Every TypeScript source under `__tests__`. */
function testSources(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.tsx?$/.test(name)) out.push(path);
    }
  };
  walk(join(ROOT, '__tests__'));
  return out;
}

/** The paged literals of a source that no `pagedRead(…)` holds, as `line: first key`. */
export function uncheckedPages(code: string, fileName = 'x.tsx'): string[] {
  const source = ts.createSourceFile(
    fileName,
    code,
    ts.ScriptTarget.Latest,
    true,
    fileName.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const out: string[] = [];
  const keyOf = (p: ts.ObjectLiteralElementLike) =>
    p.name && (ts.isIdentifier(p.name) || ts.isStringLiteral(p.name)) ? p.name.text : null;
  const checked = (literal: ts.Node): boolean => {
    let node: ts.Node = literal;
    while (node.parent) {
      const parent = node.parent;
      if (
        ts.isCallExpression(parent) &&
        ts.isIdentifier(parent.expression) &&
        parent.expression.text === 'pagedRead'
      )
        return parent.arguments[0] === node;
      const passes =
        ts.isParenthesizedExpression(parent) ||
        ts.isAsExpression(parent) ||
        ts.isSatisfiesExpression(parent) ||
        ts.isConditionalExpression(parent) ||
        ts.isSpreadAssignment(parent) ||
        (ts.isObjectLiteralExpression(parent) && ts.isSpreadAssignment(node));
      if (!passes) return false;
      node = parent;
    }
    return false;
  };
  const visit = (node: ts.Node) => {
    if (ts.isObjectLiteralExpression(node)) {
      const keys = node.properties.map(keyOf);
      if (keys.includes('hasMore') && keys.includes('nextCursor') && !checked(node)) {
        const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
        out.push(`${line + 1}: ${keys.filter(Boolean)[0]}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

describe('the paged-fixture reader finds a page no checker holds, and passes one it holds', () => {
  it('finds a bare page, a page in a mock, a page in a handler’s answer; passes pagedRead’s', () => {
    expect(
      uncheckedPages(`
        const bare = { data: [], limit: 20, hasMore: false, nextCursor: null };
        vi.mocked(api.getBlocks).mockResolvedValue({ data: [], limit: 20, hasMore: true, nextCursor: 'c' });
        http.get('/x', () => HttpResponse.json({ data: [], limit: 20, hasMore: false, nextCursor: null }));
        const held = pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null });
        const spread = pagedRead({ ...base, hasMore: true, nextCursor: 'c' });
        const typed = pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null } satisfies Page);
        const branch = pagedRead(more ? { ...base, hasMore: true, nextCursor: 'c' } : base);
        const notAPage = { data: [], limit: 20 };
      `)
    ).toEqual(['2: data', '3: data', '4: data']);
  });

  it('does not take a page handed to another call inside pagedRead as held', () => {
    expect(
      uncheckedPages(`pagedRead(build({ data: [], limit: 20, hasMore: false, nextCursor: null }));`)
    ).toEqual(['1: data']);
  });
});

describe('every paged fixture of the suite goes through pagedRead (review-p-tests SHOULD-5)', () => {
  const found: Record<string, string[]> = {};
  for (const file of testSources()) {
    const path = relative(ROOT, file).split(sep).join('/');
    if (path in EXEMPT) continue;
    const unchecked = uncheckedPages(readFileSync(file, 'utf8'), path);
    if (unchecked.length) found[path] = unchecked;
  }

  it('finds no paged fixture outside the checker', () => {
    expect(found).toEqual({});
  });

  it('exempts only the helper’s and the checker’s own tests, each with its reason', () => {
    expect(Object.keys(EXEMPT).sort()).toEqual([
      '__tests__/lib/pagination.test.ts',
      '__tests__/utils/paged-read.test.ts',
    ]);
    for (const reason of Object.values(EXEMPT)) expect(reason.length).toBeGreaterThan(20);
  });
});
