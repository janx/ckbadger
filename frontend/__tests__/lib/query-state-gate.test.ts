import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the one test of "this read is awaited" (A1 review fix, round 2): `isAwaited` in
 * `lib/query-state.ts`. A query result's `isLoading` misses a paused read and its `isPending`
 * waits forever on a disabled one, so in `app/`, `components/`, `hooks/` and `lib/` no code reads
 * either flag — no `.isLoading` / `.isPending` access, no `{ isLoading }` / `{ isPending }`
 * destructuring. The sites tolerated are listed per file with their reason.
 */
const ALLOWLIST: Readonly<Record<string, string>> = {};

const ROOT = join(__dirname, '..', '..');
const DIRS = ['app', 'components', 'hooks', 'lib'];
const FLAGS = new Set(['isLoading', 'isPending']);

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx?$/.test(name) && !name.endsWith('.d.ts') ? [path] : [];
  });
}

/** Every read of a query's loading flags in a file, as `line: text`. */
function flagReads(path: string): string[] {
  const text = readFileSync(path, 'utf8');
  const file = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true);
  const sites: string[] = [];
  const at = (node: ts.Node) =>
    `${file.getLineAndCharacterOfPosition(node.getStart()).line + 1}: ${node.getText()}`;
  const visit = (node: ts.Node) => {
    if (ts.isPropertyAccessExpression(node) && FLAGS.has(node.name.text)) sites.push(at(node));
    if (
      ts.isBindingElement(node) &&
      ts.isObjectBindingPattern(node.parent) &&
      FLAGS.has((node.propertyName ?? node.name).getText())
    )
      sites.push(at(node));
    ts.forEachChild(node, visit);
  };
  visit(file);
  return sites;
}

describe('isAwaited is the one test of an awaited read', () => {
  it('no code reads a query result’s isLoading or isPending', () => {
    const found: string[] = [];
    for (const dir of DIRS)
      for (const path of sources(join(ROOT, dir))) {
        const rel = relative(ROOT, path);
        if (rel in ALLOWLIST) continue;
        for (const site of flagReads(path)) found.push(`${rel}:${site}`);
      }
    expect(found).toEqual([]);
  });

  it('every allowlisted file still holds such a read (an entry that is no longer needed goes)', () => {
    for (const rel of Object.keys(ALLOWLIST))
      expect(flagReads(join(ROOT, rel)), rel).not.toEqual([]);
  });
});
