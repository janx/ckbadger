import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the one amount rule (readability plan 2026-11-18, ruling R5, lane B2): how a CKB or a
 * token amount is written — exact, column, summary or compact — is decided in one module,
 * `lib/amount-form.ts`, and drawn by one renderer, `components/ui/ckb-amount.tsx`. A page that
 * splits an amount into its parts to write it by hand writes a form of its own (an 8-decimal row,
 * a two-place cut, a rounded "9.01B"), and the forms drift apart again.
 *
 * In `app/`, `components/`, `hooks/` and `lib/` (but the modules that own the digits, and
 * `lib/seal/`, which no lane edits), no call splits an amount for display:
 *
 * - `splitCkbAmount(` and `splitTokenAmount(` (an amount's parts for a hand-drawn form; the CKB
 *   one is gone, folded into the column form, and the gate keeps it from coming back);
 * - `formatCkbCompact(` (the rounded compact form the module's `compact` replaced);
 * - `formatCkbAmount(…)`'s integer or decimal read apart: destructured (`{ integer, decimal } =
 *   formatCkbAmount(…)`, in any order beside other parts), read off the call
 *   (`formatCkbAmount(…).decimal`), or read off a name the file binds to the call
 *   (`const total = formatCkbAmount(x)` and then `total.integer`, `total.decimal`): the
 *   whole-and-8-decimals split by hand.
 *
 * A text that names the exact value whole (`formatCkbAmount(…).full` in a title) is not a split.
 * The splits that stand are EXCEPTIONS, each named by its file and its bound name, with its count
 * of reads and its reason; a count is exact, so a split that goes leaves the list and the list only
 * shrinks. Limits: the gate reads source text, so a result handed to a function or a component, or
 * bound through anything but a plain `const`/`let`/`var` of the call, is not followed.
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

const HAND_SPLITS: readonly { what: string; re: RegExp }[] = [
  { what: 'splitCkbAmount', re: /\bsplitCkbAmount\(/g },
  { what: 'splitTokenAmount', re: /\bsplitTokenAmount\(/g },
  { what: 'formatCkbCompact', re: /\bformatCkbCompact\(/g },
  {
    what: 'formatCkbAmount parts',
    re: /\{[^}]*\b(?:integer|decimal)\b[^}]*\}\s*=\s*formatCkbAmount\(|formatCkbAmount\([^()]*(?:\([^()]*\))?[^()]*\)\.(?:integer|decimal)\b/g,
  },
];
/** A name bound to `formatCkbAmount(…)`'s result, whose parts a later line may read apart. */
const BOUND = /\b(?:const|let|var)\s+(\w+)\s*(?::[^=;]+)?=\s*formatCkbAmount\(/g;
const HELD = 'formatCkbAmount parts held in a name';

interface Split {
  line: number;
  what: string;
  match: string;
  /** For a split held in a name: the name. */
  name?: string;
}

/** Every hand split of an amount in one source text, comment lines aside. */
function handSplits(code: string): Split[] {
  const lines = code.split('\n');
  const isComment = (line: string) => /^\s*(\/\/|\*|\/\*)/.test(line);
  const names = new Set<string>();
  lines.forEach((line) => {
    if (!isComment(line)) for (const m of line.matchAll(BOUND)) names.add(m[1]);
  });
  const parts = names.size
    ? new RegExp(`\\b(${[...names].join('|')})\\.(?:integer|decimal)\\b`, 'g')
    : null;
  const found: Split[] = [];
  lines.forEach((line, i) => {
    if (isComment(line)) return;
    for (const { what, re } of HAND_SPLITS)
      for (const m of line.matchAll(re)) found.push({ line: i + 1, what, match: m[0] });
    if (parts)
      for (const m of line.matchAll(parts))
        found.push({ line: i + 1, what: HELD, match: m[0], name: m[1] });
  });
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
        const splits = handSplits(readFileSync(path, 'utf8'));
        if (splits.length) found.set(rel, splits);
      }
    expect(failures(found)).toEqual([]);
  });

  it('keeps its exceptions few and reasoned: the list only shrinks', () => {
    expect(EXCEPTIONS.length).toBeLessThanOrEqual(2);
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
      '7 formatCkbAmount parts: { full, decimal } = formatCkbAmount(',
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
