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
 * The reader: an object literal in `__tests__/**` is a paged fixture where it says whether more
 * rows follow or where they start: a `hasMore` key, or a `nextCursor` key beside the page's rows
 * (`data`) or beside a spread (a page's key written over a page: lane cd-fixes-controls,
 * review-fix-tests SHOULD-3; the reader read only a literal holding both keys, so a checked page
 * spread with one key overridden passed, and three such fixtures contradicted themselves). It
 * passes where it is written inside the argument of a call of `pagedRead(…)` or of its pool form
 * `poolPagedRead(…)` (the tx pool's list states no `hasMore`: its "more" is its next cursor),
 * through the expressions it may sit in (parentheses, `as`, `satisfies`, a spread into the page, a
 * `?:` branch, an array of rows). So a key overridden goes into the checker with the page:
 * `pagedRead({ ...page, hasMore: true, … })`, and nothing else may write one.
 *
 * Its limits, what it cannot see: a page whose rows alone are written over a spread
 * (`{ ...page, data: rows }`: a cell's `data` is spelled as a page's rows, so the reader cannot
 * tell the two), a page with no literal of its own (built from a variable, or its keys computed),
 * a page written in a JSON fixture (`*.json` is not read: the script pages' family fixtures hold
 * their cells' page there, and their tests serve it through `pagedRead` where they page it), a
 * page a helper outside `__tests__/` builds, and the crawler's peers (`items` beside `nextCursor`),
 * a list that states no page size or "more", so nothing in its page can contradict itself.
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

/** The checker's calls that hold a page: the one checker and its pool form. */
const CHECKERS = new Set(['pagedRead', 'poolPagedRead']);

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
        CHECKERS.has(parent.expression.text)
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
      const spread = node.properties.some(ts.isSpreadAssignment);
      const paged =
        keys.includes('hasMore') ||
        (keys.includes('nextCursor') && (keys.includes('data') || spread));
      if (paged && !checked(node)) {
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

  // Lane cd-fixes-controls (review-fix-tests SHOULD-3): a page key written over a spread is the
  // page's override, a paged fixture of its own. The reader counted a literal only when it held
  // both keys, so a checked page spread with one key overridden passed, and three such fixtures
  // contradicted themselves. A page's rows alone written over a spread (`rows`) it cannot tell
  // from any other object's `data` (a cell's is spelled the same): a limit it says.
  it('finds a page key written over a spread, over a checked page too', () => {
    expect(
      uncheckedPages(`
        const a = { ...pageFixture([row]), hasMore: true };
        const b = { ...page, data: page.data.slice(0, 1), hasMore: true };
        const c = { ...pagedRead({ data: [], limit: 20, hasMore: false, nextCursor: null }), hasMore: true };
        const d = { ...page, nextCursor: 'c' };
        const held = pagedRead({ ...page, data: page.data.slice(0, 1), limit: 1, hasMore: true, nextCursor: 'c' });
        const rows = { ...page, data: rows };
      `)
    ).toEqual(['2: hasMore', '3: data', '4: hasMore', '5: nextCursor']);
  });

  // The tx pool's list states no `hasMore`: its "more" is its next cursor, as its page reads it
  // (app/transactions/page.tsx); its pages go through the checker's pool form. The crawler's peers
  // (`items`, `nextCursor`) state no page size: nothing in them can contradict itself.
  it('finds a page of the pool’s list, which states no hasMore; passes poolPagedRead’s and the peers’', () => {
    expect(
      uncheckedPages(`
        const bare = { data: [], limit: 50, total: 0, nextCursor: null };
        const over = { ...emptyPool, data: [row], nextCursor: 'p2' };
        const held = poolPagedRead({ ...emptyPool, data: [row], limit: 1, total: 2, nextCursor: 'p2' });
        const peers = { items: [peer], nextCursor: null };
      `)
    ).toEqual(['2: data', '3: data']);
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
