// Lane P2's third round (a regression of its own, found by `pnpm build`): the agent renderer
// (lib/ai/server-entry.ts) is bundled into an IIFE that scripts/build-agent.mjs runs in a bare
// `vm` context, with no `process`, no DOM and no React. lib/ai/markdown-renderer.ts imported the
// .cell parties' lock words from components/ui/lock-kind.tsx, which brought React in, and the build
// failed ("ReferenceError: process is not defined"). The decision's text half lives in
// lib/lock-words.ts, which imports none of it. This test walks the renderer's import graph from its
// entry and refuses any module of the page (components/, hooks/, app/) and any import of React.
import { existsSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..', '..');
const IMPORT =
  /(?:^|\n)\s*(?:import|export)\s[^;]*?from\s+'([^']+)'|(?:^|\n)\s*import\s+'([^']+)'/g;

function resolveAlias(spec: string): string {
  const base = join(ROOT, spec.slice(2));
  for (const candidate of [`${base}.ts`, `${base}.tsx`, join(base, 'index.ts')])
    if (existsSync(candidate)) return candidate;
  throw new Error(`cannot resolve ${spec}`);
}

/** Every module the renderer reaches from its entry, and every bare package it imports. */
function importGraph(entry: string) {
  const seen = new Set<string>();
  const packages = new Map<string, string>();
  const stack = [entry];
  while (stack.length > 0) {
    const file = stack.pop()!;
    if (seen.has(file)) continue;
    seen.add(file);
    const text = readFileSync(file, 'utf8');
    for (const m of text.matchAll(IMPORT)) {
      const spec = m[1] ?? m[2];
      // a type-only import is erased from the bundle
      if (/^\s*(?:import|export)\s+type\s/.test(m[0].trimStart())) continue;
      if (spec.startsWith('@/')) stack.push(resolveAlias(spec));
      else if (!spec.startsWith('.')) packages.set(spec, relative(ROOT, file));
    }
  }
  return { modules: [...seen].map((f) => relative(ROOT, f)), packages };
}

describe('the agent renderer bundles no page module and no React', () => {
  const { modules, packages } = importGraph(join(ROOT, 'lib', 'ai', 'server-entry.ts'));

  it('reaches only lib/ modules from its entry', () => {
    expect(modules.length).toBeGreaterThan(1);
    expect(modules.filter((m) => !m.startsWith('lib/'))).toEqual([]);
  });

  it('imports no React', () => {
    expect([...packages].filter(([name]) => /^react(?:-dom)?(?:\/|$)/.test(name))).toEqual([]);
  });

  it('reads the .cell parties’ lock words from the decision’s text half', () => {
    expect(modules).toContain('lib/lock-words.ts');
  });
});
