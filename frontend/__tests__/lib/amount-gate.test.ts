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
 * - `splitCkbAmount(` and `splitTokenAmount(` (the parts every form is cut from);
 * - `formatCkbCompact(` (the rounded compact form the module's `compact` replaced);
 * - `formatCkbAmount(…)`'s integer or decimal read apart (`{ integer, decimal } =
 *   formatCkbAmount(…)`, `formatCkbAmount(…).decimal`): the whole-and-8-decimals split by hand.
 *
 * A text that names the exact value whole (`formatCkbAmount(…).full` in a title) is not a split.
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
    re: /\{\s*(?:integer|decimal)\b[^}]*\}\s*=\s*formatCkbAmount\(|formatCkbAmount\([^()]*(?:\([^()]*\))?[^()]*\)\.(?:integer|decimal)\b/g,
  },
];

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    return /\.(ts|tsx)$/.test(name) && !name.endsWith('.d.ts') ? [path] : [];
  });
}

describe('amounts are written by one module (R5)', () => {
  it('splits no amount for display outside lib/amount-form.ts and its renderer', () => {
    const found: string[] = [];
    for (const dir of SOURCE_DIRS)
      for (const path of sourceFiles(join(ROOT, dir))) {
        const rel = relative(ROOT, path).split('\\').join('/');
        if (OWNERS.has(rel) || OUT_OF_SCOPE.some((d) => rel.startsWith(d))) continue;
        const lines = readFileSync(path, 'utf8').split('\n');
        lines.forEach((line, i) => {
          if (/^\s*(\/\/|\*|\/\*)/.test(line)) return;
          for (const { what, re } of HAND_SPLITS)
            for (const m of line.matchAll(re)) found.push(`${rel}:${i + 1} ${what}: ${m[0]}`);
        });
      }
    expect(found).toEqual([]);
  });

  it('reads the four patterns it refuses (the gate is not blind)', () => {
    const hits = (text: string) =>
      HAND_SPLITS.filter(({ re }) => [...text.matchAll(re)].length > 0).map((h) => h.what);
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
});
