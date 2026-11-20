import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Every activity row a test serves goes through the checker (the third read's T SHOULD-1): 45
 * fixtures said `itemTypes: []` on rows that move an item or take a Nervos DAO step, and a pool row
 * said `[]` where the route sends `null`, so the feeds' folds ran on keys the API cannot send. So a
 * row is checked as it is made: `activityRead` (__tests__/utils/activity-read.ts) refuses a row
 * whose item types the route would not state, as `pagedRead` refuses a page.
 *
 * The reader: an object literal in `__tests__/**` is an activity row where it states the field
 * only the API's activity rows carry, `itemTypesUnread`, or `itemTypes` beside `participants`; and
 * a row's override, where it writes over a spread a field the checker's rules read: the parties
 * (`participants`, whose items a row moves), its `protocolActions` (a DAO step) or its `itemTypes`
 * (the home fixture's DAO deposit was a .bit oracle's row with its actions written over, so it
 * kept the oracle's `[]`). It passes where it is written inside the argument of a call of
 * `activityRead(…)`, through the expressions it may sit in (parentheses, `as`, `satisfies`, a `?:`
 * branch, a spread into the row), or where it is handed to a row builder of its own file: a
 * function whose body makes its row through `activityRead(…)`, so the overrides are checked in the
 * row it makes (`makeActivity({ itemTypes: [DAO], protocolActions: [deposit], … })`). And the
 * checker's own output: a row `activityRead` returns from its own body in its own module
 * (`CHECKER_MODULE`), the row it checked as the route states it (lane feed, the lead's F4), not
 * one a function nested in it returns. The fold rule's subjects (`RunSubject`: a `title` and
 * `parties`)
 * and an item's context rows (an `itemId` and its own `protocolActions`) are not the route's
 * activity rows, and the reader leaves them.
 *
 * Its limits, what it cannot see: a row over a spread that writes only other fields (a pool row
 * made from a committed one by its block alone inherits the committed row's item types: write the
 * pool's `null`s, and the reader sees it), a row with no literal of its own (built from a variable,
 * or its keys computed), a row in a JSON fixture (`*.json` is not read: the live feeds' fixture is
 * the route's own answer, its capture said in it), and a row a helper outside `__tests__/` builds.
 *
 * EXEMPT: the checker's own tests, whose rows are its inputs on purpose, to see them refused; and
 * the flow rule's (lib/tx-flows.ts), whose inputs are its pick of a row (the parties and the
 * actions), not a row the route sends.
 *
 * UNSENT: a row the route cannot send, written on purpose to see a page refuse it, goes through
 * `unsentRow(…)` instead, which checks nothing; its uses are pinned by file, each with its reason.
 */
const EXEMPT: Readonly<Record<string, string>> = {
  '__tests__/utils/activity-read.test.ts':
    'the checker’s own tests: rows the route cannot send are its inputs, to see them refused',
  '__tests__/lib/tx-flows.test.ts':
    'the flow rule’s own tests: its inputs are its pick of a row (the parties and the actions), not a row the route sends',
};

const UNSENT: Readonly<Record<string, { count: number; reason: string }>> = {
  '__tests__/components/activity-runs.test.tsx': {
    count: 1,
    reason:
      'runSubject refuses a committed row whose item types are null with no reason, by name (T MUST-2)',
  },
};

const ROOT = join(__dirname, '..', '..');

/**
 * The checker's own module: the one file whose `activityRead` gives back the rows it checks, as the
 * route states them (a row it returns from its own body is its output, not a fixture's row).
 */
const CHECKER_MODULE = '__tests__/utils/activity-read.ts';

/** The function a statement belongs to: its nearest enclosing function of any kind, if a declaration. */
function functionOf(statement: ts.Node): ts.FunctionDeclaration | null {
  let node = statement.parent;
  while (node) {
    if (ts.isFunctionDeclaration(node)) return node;
    if (ts.isFunctionLike(node)) return null;
    node = node.parent;
  }
  return null;
}

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

/** The activity rows of a source that no `activityRead(…)` holds, as `line: first key`. */
export function uncheckedRows(code: string, fileName = 'x.tsx'): string[] {
  return rowsOf(code, fileName).unchecked;
}

/** A source's activity rows: those no checker holds, and the count `unsentRow(…)` holds. */
function rowsOf(code: string, fileName: string): { unchecked: string[]; unsent: number } {
  const source = ts.createSourceFile(
    fileName,
    code,
    ts.ScriptTarget.Latest,
    true,
    fileName.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const out: string[] = [];
  let unsent = 0;
  const keyOf = (p: ts.ObjectLiteralElementLike) =>
    p.name && (ts.isIdentifier(p.name) || ts.isStringLiteral(p.name)) ? p.name.text : null;
  // the file's row builders: a function, declared or held by a const, whose body calls the checker
  const builders = new Set<string>();
  const callsChecker = (body: ts.Node): boolean => {
    let found = false;
    const walk = (n: ts.Node) => {
      if (
        ts.isCallExpression(n) &&
        ts.isIdentifier(n.expression) &&
        n.expression.text === 'activityRead'
      )
        found = true;
      if (!found) ts.forEachChild(n, walk);
    };
    walk(body);
    return found;
  };
  const findBuilders = (n: ts.Node) => {
    if (ts.isFunctionDeclaration(n) && n.name && n.body && callsChecker(n.body))
      builders.add(n.name.text);
    if (
      ts.isVariableDeclaration(n) &&
      ts.isIdentifier(n.name) &&
      n.initializer &&
      (ts.isArrowFunction(n.initializer) || ts.isFunctionExpression(n.initializer)) &&
      callsChecker(n.initializer.body)
    )
      builders.add(n.name.text);
    ts.forEachChild(n, findBuilders);
  };
  findBuilders(source);
  const checked = (literal: ts.Node): 'held' | 'unsent' | false => {
    let node: ts.Node = literal;
    while (node.parent) {
      const parent = node.parent;
      if (
        ts.isCallExpression(parent) &&
        ts.isIdentifier(parent.expression) &&
        (parent.expression.text === 'activityRead' || parent.expression.text === 'unsentRow')
      )
        return parent.arguments[0] === node
          ? parent.expression.text === 'activityRead'
            ? 'held'
            : 'unsent'
          : false;
      if (
        ts.isCallExpression(parent) &&
        ts.isIdentifier(parent.expression) &&
        builders.has(parent.expression.text)
      )
        return parent.arguments.some((a) => a === node) ? 'held' : false;
      // the checker's output: a row returned from activityRead's own body in its own module, the
      // row it checked as the route states it (lane feed); not one in a function nested in it
      if (ts.isReturnStatement(parent))
        return fileName === CHECKER_MODULE && functionOf(parent)?.name?.text === 'activityRead'
          ? 'held'
          : false;
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
      // the fold rule's subjects and an item's context rows are not the route's activity rows
      const other = ['title', 'parties', 'itemId'].some((k) => keys.includes(k));
      const row =
        !other &&
        (keys.includes('itemTypesUnread') ||
          (keys.includes('itemTypes') && keys.includes('participants')) ||
          (spread &&
            ['participants', 'protocolActions', 'itemTypes'].some((k) => keys.includes(k))));
      const by = row && checked(node);
      if (by === 'unsent') unsent += 1;
      if (row && !by) {
        const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
        out.push(`${line + 1}: ${keys.filter(Boolean)[0]}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { unchecked: out, unsent };
}

describe('the activity-row reader finds a row no checker holds, and passes one it holds', () => {
  it('finds a bare row, a row in a mock, a row over a spread; passes activityRead’s and the rule’s subjects', () => {
    expect(
      uncheckedRows(`
        const bare = { txHash: h, itemTypes: [], itemTypesUnread: null, participants: [] };
        vi.mocked(api.getGlobalActivities).mockResolvedValue({ data: [{ itemTypes: [], participants: [] }] });
        const pool = { ...committed, blockNumber: null, itemTypes: null, itemTypesUnread: null };
        const held = activityRead({ txHash: h, itemTypes: [], itemTypesUnread: null, participants: [] });
        const over = activityRead({ ...committed, itemTypes: [SUDT] } as Activity);
        const branch = activityRead(dao ? { ...base, itemTypes: [DAO], itemTypesUnread: null } : base);
        const subject = { title: 'Script call', itemTypes: [], parties: [] };
      `)
    ).toEqual(['2: txHash', '3: itemTypes', '4: blockNumber']);
  });

  it('leaves an override handed to a row builder of the file, which the builder checks as its row', () => {
    expect(
      uncheckedRows(`
        function makeActivity(o) { return activityRead({ txHash: h, itemTypes: [], itemTypesUnread: null, participants: [], ...o }); }
        const row = (ps, over) => activityRead({ ...base, participants: ps, ...over });
        const deposit = makeActivity({ itemTypes: [DAO], protocolActions: [dao], participants: [p] });
        const moved = row([party('a')], { ...over, itemTypes: [SUDT] });
        const elsewhere = build({ txHash: h, itemTypes: [], participants: [] });
      `)
    ).toEqual(['6: txHash']);
  });

  // the home fixture's DAO deposit was a .bit oracle's row with its actions written over, so it kept
  // the oracle's itemTypes []: an override of a field the rules read is a row of its own
  it('finds a row’s parties, actions or item types written over a spread; leaves the rule’s subjects and an item’s context', () => {
    expect(
      uncheckedRows(`
        const deposit = { ...base, typeCalls: [], protocolActions: [dao] };
        const moved = { ...latest[2], participants: [{ ...p, itemDeltas: [token] }] };
        const typed = { ...row, itemTypes: [SUDT] };
        const held = activityRead({ ...base, protocolActions: [dao], itemTypes: [DAO] });
        const subject = { ...over, title: 'DAO Deposit', protocolActions: [dao] };
        const context = { ...context, itemId: '0x41', protocolActions: [] };
        const age = { ...feedSamples.tokenTransfer, blockNumber: 7 };
      `)
    ).toEqual(['2: typeCalls', '3: participants', '4: itemTypes']);
  });

  it('counts a row unsentRow holds apart, as no checker’s', () => {
    const code = `
      const refused = unsentRow({ ...unread(1), itemTypesUnread: null });
      const held = activityRead({ ...unread(1), itemTypesUnread: 'why' });
    `;
    expect(rowsOf(code, 'x.tsx')).toEqual({ unchecked: [], unsent: 1 });
  });

  // The lead's F4 on lane feed: the checker gives back the row it checked as the route states it,
  // `{ ...checked, participants, partyIds, … }`; that literal, returned from activityRead's own
  // body in the checker's own module, is the checker's output, not a fixture's row. There and only
  // there: the same literal elsewhere in that module, returned by a function of another name, or
  // returned by an activityRead of another file, is still found.
  it('passes the row activityRead returns from its own body in the checker’s module, and no other', () => {
    const code = `
      export function activityRead(checked) {
        const kept = { ...checked, participants: drawn };
        const inner = () => { return { ...checked, participants: drawn }; };
        return { ...checked, participants: drawn, partyIds: ids } as Stated;
      }
      function stateParties(checked) { return { ...checked, participants: drawn }; }
      const loose = { ...checked, participants: drawn };
    `;
    expect(uncheckedRows(code, '__tests__/utils/activity-read.ts')).toEqual([
      '3: participants',
      '4: participants',
      '7: participants',
      '8: participants',
    ]);
    expect(uncheckedRows(code, '__tests__/components/x.test.tsx')).toEqual([
      '3: participants',
      '4: participants',
      '5: participants',
      '7: participants',
      '8: participants',
    ]);
  });

  it('does not take a row handed to another call inside activityRead as held', () => {
    expect(
      uncheckedRows(
        `activityRead(build({ itemTypes: [], itemTypesUnread: null, participants: [] }));`
      )
    ).toEqual(['1: itemTypes']);
  });
});

describe('every activity row of the suite goes through activityRead (the third read’s T SHOULD-1)', () => {
  const found: Record<string, string[]> = {};
  const unsent: Record<string, number> = {};
  for (const file of testSources()) {
    const path = relative(ROOT, file).split(sep).join('/');
    if (path in EXEMPT) continue;
    const rows = rowsOf(readFileSync(file, 'utf8'), path);
    if (rows.unchecked.length) found[path] = rows.unchecked;
    if (rows.unsent) unsent[path] = rows.unsent;
  }

  it('finds no activity row outside the checker', () => {
    expect(found).toEqual({});
  });

  it('writes a row the route cannot send only where the pinned list says, each with its reason', () => {
    expect(unsent).toEqual(
      Object.fromEntries(Object.entries(UNSENT).map(([path, { count }]) => [path, count]))
    );
    for (const { reason } of Object.values(UNSENT)) expect(reason.length).toBeGreaterThan(20);
  });

  it('exempts only the checker’s and the flow rule’s own tests, each with its reason', () => {
    expect(Object.keys(EXEMPT).sort()).toEqual([
      '__tests__/lib/tx-flows.test.ts',
      '__tests__/utils/activity-read.test.ts',
    ]);
    for (const reason of Object.values(EXEMPT)) expect(reason.length).toBeGreaterThan(20);
  });
});
