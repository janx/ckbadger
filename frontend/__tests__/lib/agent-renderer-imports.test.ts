// Lane P2's third round (a regression of its own, found by `pnpm build`): the agent renderer
// (lib/ai/server-entry.ts) is bundled into an IIFE that scripts/build-agent.mjs runs in a bare
// `vm` context, with no `process`, no DOM and no React. lib/ai/markdown-renderer.ts imported the
// .cell parties' lock words from components/ui/lock-kind.tsx, which brought React in, and the build
// failed ("ReferenceError: process is not defined"). The decision's text half lives in
// lib/lock-words.ts, which imports none of it. This test walks the renderer's import graph from its
// entry and refuses any module of the page (components/, hooks/, app/) and any package outside
// ALLOWED_PACKAGES.
//
// Lane cd-fixes-controls (review-fix-tests NOTE-1): the walk read `import … from` by a regex,
// skipped a relative specifier (lib/type-metrics.ts's `./type-scale` was never visited), matched no
// `import()` and no `require()`, and refused only React. It reads the syntax tree now: a static
// import or re-export, a side-effect import, `import('…')` and `require('…')`, an `@/` or relative
// specifier resolved and walked; a dynamic specifier it cannot read is refused. A package the
// renderer reaches must be on the ALLOWED list, and a refusal names the chain of imports that
// reached it.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..', '..');

/**
 * The packages the renderer may reach, each with the reason it loads in the bundle's bare vm (it
 * reads no `process`, `window` or `document` as it loads; `pnpm build` runs the bundle there,
 * scripts/build-agent.mjs). A package that does breaks the bundle; one joins this list only with its
 * reason. The four below are what the walk found when it learned to read the whole graph (the regex
 * listed none of them as anything but "not React").
 */
const ALLOWED_PACKAGES: Readonly<Record<string, string>> = {
  'url-search-params-polyfill':
    'the entry’s own polyfill (lib/ai/server-entry.ts): the embedded runtime has no URLSearchParams',
  buffer: 'the entry’s own Buffer (lib/ai/server-entry.ts): the embedded runtime has none',
  clsx: 'lib/utils.ts’s class joiner, reached through lib/fork-format.ts’s date format: a pure function',
  'tailwind-merge':
    'lib/utils.ts’s class merge, reached the same way: built at load from its own tables, no globals',
};

/** How the walk reads a module: its text, and whether a path is one. */
export interface ModuleReader {
  read(path: string): string;
  exists(path: string): boolean;
}

const DISK: ModuleReader = { read: (p) => readFileSync(p, 'utf8'), exists: existsSync };

/** The package a bare specifier names: `@scope/name` or `name`, without its subpath. */
function packageOf(spec: string): string {
  const parts = spec.split('/');
  return spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
}

/** The specifiers a module loads at run time, each with the line it is written on. */
function specifiersOf(file: string, code: string): { spec: string; line: number }[] {
  const source = ts.createSourceFile(file, code, ts.ScriptTarget.Latest, true);
  const out: { spec: string; line: number }[] = [];
  const at = (node: ts.Node) =>
    source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  const visit = (node: ts.Node) => {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      const clause = node.importClause;
      // a type-only import is erased from the bundle: `import type`, or named types alone
      const typeOnly =
        clause !== undefined &&
        (clause.isTypeOnly ||
          (!clause.name &&
            clause.namedBindings !== undefined &&
            ts.isNamedImports(clause.namedBindings) &&
            clause.namedBindings.elements.length > 0 &&
            clause.namedBindings.elements.every((e) => e.isTypeOnly)));
      if (!typeOnly) out.push({ spec: node.moduleSpecifier.text, line: at(node) });
    } else if (
      ts.isExportDeclaration(node) &&
      node.moduleSpecifier &&
      ts.isStringLiteral(node.moduleSpecifier)
    ) {
      if (!node.isTypeOnly) out.push({ spec: node.moduleSpecifier.text, line: at(node) });
    } else if (
      ts.isCallExpression(node) &&
      (node.expression.kind === ts.SyntaxKind.ImportKeyword ||
        (ts.isIdentifier(node.expression) && node.expression.text === 'require'))
    ) {
      const [arg] = node.arguments;
      if (!arg || !ts.isStringLiteralLike(arg))
        throw new Error(
          `${file}:${at(node)}: a dynamic import the walk cannot read (${node.getText(source)})`
        );
      out.push({ spec: arg.text, line: at(node) });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

/**
 * Every module the renderer reaches from its entry (paths relative to `root`), and every package it
 * imports with the chain of modules that reached it (the entry first, the importing module last).
 */
export function importGraph(entry: string, root = ROOT, reader: ModuleReader = DISK) {
  const resolve = (spec: string, from: string): string => {
    const base = spec.startsWith('@/') ? join(root, spec.slice(2)) : join(dirname(from), spec);
    for (const candidate of [
      `${base}.ts`,
      `${base}.tsx`,
      join(base, 'index.ts'),
      join(base, 'index.tsx'),
      base,
    ])
      if (reader.exists(candidate) && /\.tsx?$/.test(candidate)) return candidate;
    throw new Error(`${relative(root, from)}: cannot resolve ${spec}`);
  };
  const reachedFrom = new Map<string, string | null>([[entry, null]]);
  const chainTo = (file: string): string[] => {
    const chain: string[] = [];
    for (let f: string | null = file; f !== null; f = reachedFrom.get(f) ?? null)
      chain.unshift(relative(root, f));
    return chain;
  };
  const packages = new Map<string, string[]>();
  const queue = [entry];
  while (queue.length > 0) {
    const file = queue.shift()!;
    for (const { spec } of specifiersOf(relative(root, file), reader.read(file))) {
      if (spec.startsWith('@/') || spec.startsWith('.')) {
        const target = resolve(spec, file);
        if (!reachedFrom.has(target)) {
          reachedFrom.set(target, file);
          queue.push(target);
        }
      } else if (!packages.has(packageOf(spec))) packages.set(packageOf(spec), chainTo(file));
    }
  }
  return { modules: [...reachedFrom.keys()].map((f) => relative(root, f)), packages };
}

/** The packages outside the allowed list, each as `name (entry → … → importer)`. */
export function refusedPackages(
  packages: ReadonlyMap<string, readonly string[]>,
  allowed: Readonly<Record<string, string>> = ALLOWED_PACKAGES
): string[] {
  return [...packages]
    .filter(([name]) => !(name in allowed))
    .map(([name, chain]) => `${name} (${chain.join(' → ')})`);
}

describe('the import walk reads what the bundle loads', () => {
  const tree = (files: Record<string, string>): ModuleReader => ({
    read: (p) => {
      if (!(p in files)) throw new Error(`no module ${p}`);
      return files[p];
    },
    exists: (p) => p in files,
  });

  it('follows a relative specifier and an @/ one, through an index', () => {
    const { modules } = importGraph(
      '/r/lib/ai/entry.ts',
      '/r',
      tree({
        '/r/lib/ai/entry.ts': `import { a } from './a';\nexport { b } from '../b';`,
        '/r/lib/ai/a.ts': `import { c } from '@/lib/c';`,
        '/r/lib/b.ts': `export const b = 1;`,
        '/r/lib/c/index.ts': `export const c = 1;`,
      })
    );
    expect(modules.sort()).toEqual([
      'lib/ai/a.ts',
      'lib/ai/entry.ts',
      'lib/b.ts',
      'lib/c/index.ts',
    ]);
  });

  it('reads import() and require(), a side-effect import, and skips what the bundle erases', () => {
    const { packages } = importGraph(
      '/r/lib/entry.ts',
      '/r',
      tree({
        '/r/lib/entry.ts': [
          `import 'side-effect/register';`,
          `const lazy = () => import('lazy-pkg');`,
          `const old = require('@scope/old/sub');`,
          `import type { T } from 'types-only';`,
          `import { type U } from 'named-types-only';`,
          `export type { V } from 'reexported-types';`,
        ].join('\n'),
      })
    );
    expect([...packages.keys()].sort()).toEqual(['@scope/old', 'lazy-pkg', 'side-effect']);
  });

  it('refuses a package outside the list, with the chain of imports that reached it', () => {
    const { packages } = importGraph(
      '/r/lib/ai/entry.ts',
      '/r',
      tree({
        '/r/lib/ai/entry.ts': `import { a } from './a';\nimport ok from 'allowed-pkg';`,
        '/r/lib/ai/a.ts': `import { b } from '../b';`,
        '/r/lib/b.ts': `import { useState } from 'react';`,
      })
    );
    expect(refusedPackages(packages, { 'allowed-pkg': 'a reason for the test' })).toEqual([
      'react (lib/ai/entry.ts → lib/ai/a.ts → lib/b.ts)',
    ]);
  });

  it('refuses a dynamic import it cannot read, and a specifier it cannot resolve', () => {
    expect(() =>
      importGraph(
        '/r/lib/entry.ts',
        '/r',
        tree({ '/r/lib/entry.ts': 'const m = (name: string) => import(name);' })
      )
    ).toThrow('lib/entry.ts:1: a dynamic import the walk cannot read (import(name))');
    expect(() =>
      importGraph('/r/lib/entry.ts', '/r', tree({ '/r/lib/entry.ts': `import './gone';` }))
    ).toThrow('lib/entry.ts: cannot resolve ./gone');
  });
});

describe('the agent renderer bundles no page module and no package outside its list', () => {
  const { modules, packages } = importGraph(join(ROOT, 'lib', 'ai', 'server-entry.ts'));

  it('reaches only lib/ modules from its entry', () => {
    expect(modules.length).toBeGreaterThan(1);
    expect(modules.filter((m) => !m.startsWith('lib/'))).toEqual([]);
  });

  it('reaches the modules a relative import loads (lib/type-metrics.ts’s ./type-scale)', () => {
    expect(modules).toContain('lib/type-scale.ts');
  });

  it('imports no package outside ALLOWED_PACKAGES', () => {
    expect(refusedPackages(packages)).toEqual([]);
  });

  it('allows each package with its reason', () => {
    for (const reason of Object.values(ALLOWED_PACKAGES)) expect(reason.length).toBeGreaterThan(20);
  });

  it('reads the .cell parties’ lock words from the decision’s text half', () => {
    expect(modules).toContain('lib/lock-words.ts');
  });
});
