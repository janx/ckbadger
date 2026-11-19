import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the one amount rule (readability plan 2026-11-18, ruling R5, lane B2): how a CKB or a
 * token amount is written — exact, column, summary or compact — is decided in one module,
 * `lib/amount-form.ts`, and drawn by one renderer, `components/ui/ckb-amount.tsx`. A page that
 * splits an amount into its parts to write it by hand writes a form of its own (an 8-decimal row,
 * a two-place cut, a rounded "9.01B"), and the forms drift apart again.
 *
 * In `app/`, `components/`, `hooks/` and `lib/` (but the modules that own the digits, and
 * `lib/seal/`, which no lane edits), the gate reads each source's syntax tree with the TypeScript
 * parser, as the scan and ink gates do, so a comment or a string is never a hit and a line break
 * never hides one. No source splits an amount for display:
 *
 * - `splitCkbAmount` and `splitTokenAmount` (an amount's parts for a hand-drawn form; the CKB one
 *   is gone, folded into the column form, and the gate keeps it from coming back) and
 *   `formatCkbCompact` (the rounded compact form the module's `compact` replaced): named anywhere
 *   but an import, called or handed on;
 * - `formatCkbAmount(…)`'s integer or decimal read apart, off the call or off a name bound to it:
 *   a name is bound by a declaration or an assignment whose value is the call or a bound name
 *   (through `?:`, `??`, `||`, `&&`, parentheses and casts), and by the rest of a pattern that
 *   destructures one; a read is a `.` or `?.` of `integer` or `decimal`, a `[]` by either key or
 *   by a key the source does not settle, an object pattern (a declaration's or an assignment's)
 *   that takes either part or a computed key, or a spread that hands the parts on apart: the
 *   whole-and-8-decimals split by hand.
 *
 * A text that names the exact value whole (`formatCkbAmount(…).full` in a title) or its sign
 * (`isNegative`) is not a split. The splits that stand are EXCEPTIONS, each named by its file and
 * its bound name, with its count of reads and its reason; a count is exact and the list is pinned
 * by name, so a split that goes leaves the list, and one cannot be swapped for another. Limits:
 * names are the file's, not a scope's (a name bound to the call anywhere in a file is read as
 * bound everywhere in it); a result handed to a function or a component, returned by one, or kept
 * in an object or an array, is not followed.
 */

const ROOT = join(__dirname, '..', '..');
const SOURCE_DIRS = ['app', 'components', 'hooks', 'lib'];
/** The modules that own an amount's digits. */
const OWNERS = new Set([
  'lib/amount-form.ts',
  'lib/ckb-decimal.ts',
  'lib/format-asset.ts',
  'lib/utils.ts',
  'components/ui/ckb-amount.tsx',
]);
const OUT_OF_SCOPE = ['lib/seal/'];

/** The removed splitters: named anywhere but an import, each is a split. */
const REMOVED = new Set(['splitCkbAmount', 'splitTokenAmount', 'formatCkbCompact']);
/** The function whose result's parts a page may not read apart, and those parts. */
const FORMAT = 'formatCkbAmount';
const PARTS = new Set(['integer', 'decimal']);
const DIRECT = 'formatCkbAmount parts';
const HELD = 'formatCkbAmount parts held in a name';

interface Split {
  line: number;
  what: string;
  match: string;
  /** For a split held in a name: the name. */
  name?: string;
}

/** Every hand split of an amount in one source (TSX unless `kind` says otherwise). */
function handSplits(code: string, kind: ts.ScriptKind = ts.ScriptKind.TSX): Split[] {
  const source = ts.createSourceFile('amounts.tsx', code, ts.ScriptTarget.Latest, true, kind);
  const text = (n: ts.Node) => n.getText(source).replace(/\s+/g, ' ');
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const unwrap = (e: ts.Expression): ts.Expression => {
    while (
      ts.isParenthesizedExpression(e) ||
      ts.isAsExpression(e) ||
      ts.isSatisfiesExpression(e) ||
      ts.isNonNullExpression(e) ||
      ts.isTypeAssertionExpression(e)
    )
      e = e.expression;
    return e;
  };
  const isFormatCall = (e: ts.Expression) =>
    ts.isCallExpression(e) && ts.isIdentifier(e.expression) && e.expression.text === FORMAT;

  // ---- the names bound to the call, to a fixed point (an alias may stand before its source) ----
  const names = new Set<string>();
  /** Whether `e0` is the call's result: the call, a bound name, or a branch that is one. */
  const reaches = (e0: ts.Expression): boolean => {
    const e = unwrap(e0);
    if (isFormatCall(e)) return true;
    if (ts.isIdentifier(e)) return names.has(e.text);
    if (ts.isConditionalExpression(e)) return reaches(e.whenTrue) || reaches(e.whenFalse);
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (
        op === ts.SyntaxKind.QuestionQuestionToken ||
        op === ts.SyntaxKind.BarBarToken ||
        op === ts.SyntaxKind.AmpersandAmpersandToken
      )
        return reaches(e.left) || reaches(e.right);
    }
    return false;
  };
  const isAssignment = (n: ts.Node): n is ts.BinaryExpression =>
    ts.isBinaryExpression(n) && n.operatorToken.kind === ts.SyntaxKind.EqualsToken;
  /** The rest of a pattern that destructures the result holds its parts: it is bound too. */
  const bindRest = (pattern: ts.Node) => {
    if (ts.isObjectBindingPattern(pattern))
      for (const el of pattern.elements)
        if (el.dotDotDotToken && ts.isIdentifier(el.name)) names.add(el.name.text);
    if (ts.isObjectLiteralExpression(pattern))
      for (const p of pattern.properties)
        if (ts.isSpreadAssignment(p) && ts.isIdentifier(p.expression)) names.add(p.expression.text);
  };
  let before = -1;
  while (names.size !== before) {
    before = names.size;
    const bind = (n: ts.Node): void => {
      if (ts.isVariableDeclaration(n) && n.initializer && reaches(n.initializer)) {
        if (ts.isIdentifier(n.name)) names.add(n.name.text);
        else bindRest(n.name);
      }
      if (isAssignment(n) && reaches(n.right)) {
        const left = unwrap(n.left);
        if (ts.isIdentifier(left)) names.add(left.text);
        else bindRest(left);
      }
      ts.forEachChild(n, bind);
    };
    bind(source);
  }

  // ---- the reads of a part ----
  const found: Split[] = [];
  /** A split read at `node` off `of`, the call's result: held in a name when `of` is one. */
  const split = (node: ts.Node, of: ts.Expression) => {
    const held = unwrap(of);
    found.push(
      ts.isIdentifier(held)
        ? { line: lineOf(node), what: HELD, match: text(node), name: held.text }
        : { line: lineOf(node), what: DIRECT, match: text(node) }
    );
  };
  /** A property's key as the source settles it, or null (a computed key). */
  const keyOf = (name: ts.PropertyName | ts.BindingName | undefined): string | null =>
    name !== undefined &&
    (ts.isIdentifier(name) || ts.isStringLiteralLike(name) || ts.isNumericLiteral(name))
      ? name.text
      : null;
  const takesPart = (k: string | null) => k === null || PARTS.has(k);
  const visit = (n: ts.Node): void => {
    // a removed splitter, named anywhere but an import
    if (
      ts.isIdentifier(n) &&
      REMOVED.has(n.text) &&
      !ts.isImportSpecifier(n.parent) &&
      !ts.isImportClause(n.parent)
    )
      found.push({
        line: lineOf(n),
        what: n.text,
        match: ts.isCallExpression(n.parent) && n.parent.expression === n ? text(n.parent) : n.text,
      });
    // `.part`, `?.part`
    if (ts.isPropertyAccessExpression(n) && PARTS.has(n.name.text) && reaches(n.expression))
      split(n, n.expression);
    // `['part']`, or `[k]` by a key the source does not settle
    if (ts.isElementAccessExpression(n) && reaches(n.expression)) {
      const k = n.argumentExpression;
      if (takesPart(ts.isStringLiteralLike(k) || ts.isNumericLiteral(k) ? k.text : null))
        split(n, n.expression);
    }
    // `const { integer, … } = …`: one split for a pattern that takes a part or a computed key
    if (
      ts.isVariableDeclaration(n) &&
      ts.isObjectBindingPattern(n.name) &&
      n.initializer &&
      reaches(n.initializer) &&
      n.name.elements.some(
        (el) => !el.dotDotDotToken && takesPart(keyOf(el.propertyName ?? el.name))
      )
    )
      split(n, n.initializer);
    // `({ integer } = …)`
    if (isAssignment(n) && reaches(n.right)) {
      const pattern = unwrap(n.left);
      if (
        ts.isObjectLiteralExpression(pattern) &&
        pattern.properties.some((p) => !ts.isSpreadAssignment(p) && takesPart(keyOf(p.name)))
      )
        split(n, n.right);
    }
    // the parts handed on apart: `{...parts}`, `<F {...parts} />`, `f(...parts)`
    if (
      (ts.isSpreadAssignment(n) || ts.isJsxSpreadAttribute(n) || ts.isSpreadElement(n)) &&
      reaches(n.expression)
    )
      split(n, n.expression);
    ts.forEachChild(n, visit);
  };
  visit(source);
  return found;
}

/**
 * The splits that stand: a file, the name it binds to `formatCkbAmount(…)`, its reads of that
 * name's parts (exact), and why. The list only shrinks; a new entry is a decision of its own.
 */
const EXCEPTIONS: readonly { path: string; name: string; reads: number; why: string }[] = [
  {
    path: 'components/home/ckbytes-section.tsx',
    name: 'total',
    reads: 2,
    why: "the home page's circulating figure, its display figure: set in two sizes by an approved board (the integer at the display step, the decimals and the unit small), exact to the shannon on purpose, not a row amount; the home page's lane decides its form",
  },
  {
    path: 'components/home/ckbytes-section.tsx',
    name: 'deposited',
    reads: 2,
    why: "the home page's DAO deposited figure, drawn as the circulating one: the integer at its step, the decimals small, exact to the shannon on purpose; the home page's lane decides its form",
  },
];

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    return /\.(ts|tsx)$/.test(name) && !name.endsWith('.d.ts') ? [path] : [];
  });
}

/** The gate's failures: every split that is not an exception, and every exception off its count. */
function failures(found: ReadonlyMap<string, readonly Split[]>): string[] {
  const out: string[] = [];
  const reads = new Map<string, number>();
  for (const [rel, splits] of found)
    for (const s of splits) {
      const excepted =
        s.name !== undefined && EXCEPTIONS.some((e) => e.path === rel && e.name === s.name);
      if (!excepted) out.push(`${rel}:${s.line} ${s.what}: ${s.match}`);
      else reads.set(`${rel}#${s.name}`, (reads.get(`${rel}#${s.name}`) ?? 0) + 1);
    }
  for (const e of EXCEPTIONS) {
    const n = reads.get(`${e.path}#${e.name}`) ?? 0;
    if (n !== e.reads)
      out.push(
        `${e.path}: ${n} read(s) of ${e.name}'s parts, ${e.reads} excepted — set it to ${n}`
      );
    if (e.why.trim().length <= 10) out.push(`${e.path}#${e.name}: a reason too short to say why`);
  }
  return out;
}

/** A source's splits as `line what: match`. */
const splitsOf = (lines: string[]) =>
  handSplits(lines.join('\n')).map((h) => `${h.line} ${h.what}: ${h.match}`);

describe('amounts are written by one module (R5)', () => {
  it('splits no amount for display outside lib/amount-form.ts and its renderer', () => {
    const found = new Map<string, Split[]>();
    for (const dir of SOURCE_DIRS)
      for (const path of sourceFiles(join(ROOT, dir))) {
        const rel = relative(ROOT, path).split('\\').join('/');
        if (OWNERS.has(rel) || OUT_OF_SCOPE.some((d) => rel.startsWith(d))) continue;
        const splits = handSplits(
          readFileSync(path, 'utf8'),
          path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
        );
        if (splits.length) found.set(rel, splits);
      }
    expect(failures(found)).toEqual([]);
  });

  // Review of the fixes (2026-11-19, N4): the list was held to a length, so one exception could be
  // swapped for another; it is pinned by file, name and count, and only shrinks.
  it('keeps exactly its two exceptions, by file, name and count: the list only shrinks', () => {
    expect(EXCEPTIONS.map((e) => `${e.path}#${e.name} ×${e.reads}`)).toEqual([
      'components/home/ckbytes-section.tsx#total ×2',
      'components/home/ckbytes-section.tsx#deposited ×2',
    ]);
  });

  it('reads the patterns it refuses (the gate is not blind)', () => {
    const hits = (text: string) => handSplits(text).map((h) => h.what);
    expect(hits('const p = splitCkbAmount(x);')).toEqual(['splitCkbAmount']);
    expect(hits('splitTokenAmount(a, d).fraction')).toEqual(['splitTokenAmount']);
    expect(hits('formatCkbCompact(v).value')).toEqual(['formatCkbCompact']);
    expect(hits('const { integer, decimal } = formatCkbAmount(sum);')).toEqual([
      'formatCkbAmount parts',
    ]);
    expect(hits('formatCkbAmount(BigInt(v)).decimal.slice(0, 2)')).toEqual([
      'formatCkbAmount parts',
    ]);
    expect(hits('title={`${formatCkbAmount(v).full} CKB`}')).toEqual([]);
  });

  // Review of main..fix/readability (2026-11-18, test finding 1): the gate read one line at a time
  // and saw neither a split held in a name nor parts destructured after another; the home page's
  // two display figures split that way passed it.
  it('reads a split held in a name, and parts destructured after another', () => {
    expect(
      handSplits(
        [
          'const total = formatCkbAmount(circulating);',
          'const title = `${formatCkbAmount(v).full} CKB`;',
          '  {total.integer}',
          '  <span>.{total.decimal}</span>',
          '  {total.full}',
          '// total.integer in a comment is not a read',
          'const { full, decimal } = formatCkbAmount(sum);',
          'let held: Parts = formatCkbAmount(x); held.decimal;',
        ].join('\n')
      ).map((h) => `${h.line} ${h.what}: ${h.match}`)
    ).toEqual([
      `3 ${HELD}: total.integer`,
      `4 ${HELD}: total.decimal`,
      // re-pinned (S1): the gate reads nodes, and a match is the node's text, the call whole
      '7 formatCkbAmount parts: { full, decimal } = formatCkbAmount(sum)',
      `8 ${HELD}: held.decimal`,
    ]);
  });

  // Review of the fixes (2026-11-19, S1): the gate matched source text one line at a time, and the
  // review's four splits by hand passed it: a held name destructured, a held name read with `?.`, a
  // destructure a formatter keeps on several lines, and a call whose argument holds two groups.
  it('reads the four splits the text matching missed', () => {
    expect(
      splitsOf([
        'const total = formatCkbAmount(x);',
        'const { integer, decimal } = total;',
        '<b>{total?.integer}</b>;',
        'const {',
        '  integer,',
        '  decimal,',
        '} = formatCkbAmount(y);',
        'formatCkbAmount(BigInt(a) + BigInt(b)).decimal;',
      ])
    ).toEqual([
      `2 ${HELD}: { integer, decimal } = total`,
      `3 ${HELD}: total?.integer`,
      '4 formatCkbAmount parts: { integer, decimal, } = formatCkbAmount(y)',
      '8 formatCkbAmount parts: formatCkbAmount(BigInt(a) + BigInt(b)).decimal',
    ]);
  });

  it('follows every binding of the call and reads every part read of it, by `.`, `?.`, `[]`, a pattern or a spread', () => {
    expect(
      splitsOf([
        'const t = on ? formatCkbAmount(a) : formatCkbAmount(b);',
        "t['decimal'];",
        'const u = t;',
        'u.integer;',
        'let v;',
        'v = formatCkbAmount(c);',
        'v?.decimal;',
        'const { full, ...rest } = formatCkbAmount(d);',
        'rest.integer;',
        '<Figure {...formatCkbAmount(e)} />;',
        'const { [key]: part } = formatCkbAmount(f);',
        'u[key];',
        '({ integer: w } = u);',
      ])
    ).toEqual([
      `2 ${HELD}: t['decimal']`,
      `4 ${HELD}: u.integer`,
      `7 ${HELD}: v?.decimal`,
      `9 ${HELD}: rest.integer`,
      '10 formatCkbAmount parts: {...formatCkbAmount(e)}',
      '11 formatCkbAmount parts: { [key]: part } = formatCkbAmount(f)',
      `12 ${HELD}: u[key]`,
      `13 ${HELD}: { integer: w } = u`,
    ]);
  });

  it('passes the whole value, its sign and a name bound to anything else', () => {
    expect(
      splitsOf([
        'const total = formatCkbAmount(x);',
        'title={`${total.full} CKB`}',
        "total['full'];",
        'formatCkbAmount(v).isNegative;',
        'const { full, isNegative } = formatCkbAmount(y);',
        'other.integer;',
        '// total.integer in a comment is not a read',
        "const s = 'total.decimal in a string is not a read';",
      ])
    ).toEqual([]);
  });
});
