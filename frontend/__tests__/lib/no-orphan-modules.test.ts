import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, posix, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate for orphan modules (UI review 2026-11-14, plan §1 "Dead code in the old language", lane E1):
 * every module under the five source directories is reached from the build's entry points through
 * imports, so no file the app never runs stays behind for later lanes to convert.
 *
 * A module is LIVE when an import chain leads to it from an entry: a static `import` (a type-only
 * one too: type-check needs the file), `export … from`, `import x = require(…)`, a type position's
 * `import('…')`, or a dynamic `import('…')` (the router's `lazyPage`, `lazy`, `dynamic` from
 * lib/dynamic-client), with `@/` and relative specifiers resolved to `.ts` / `.tsx` files and to a
 * directory's `index`. The entries are the module script index.html loads, the agent renderer's
 * library entry that `pnpm build` bundles (scripts/build-agent.mjs), and the build configuration
 * that imports source (tailwind.config.ts reads the palette and the type scale). Tests are not
 * importers: a module only tests import is dead, and goes with its tests.
 *
 * A dynamic import whose argument is not a string literal, an `import.meta.glob`, or an `@/` or
 * relative specifier that resolves to no file fails the gate: the graph would miss what it loads,
 * and a live module would read as dead (or the reverse). Package specifiers are not followed.
 */

/** The directories whose modules must be reached (`.d.ts` files and `public/` are out of scope). */
const SOURCE_DIRS = ['app', 'components', 'hooks', 'lib', 'src'];

/** The build's entry points, each with the file that names it and the text that names it there. */
const ENTRIES: Record<string, { owner: string; named: string }> = {
  'src/main.tsx': { owner: 'index.html', named: '<script type="module" src="/src/main.tsx">' },
  'lib/ai/server-entry.ts': {
    owner: 'scripts/build-agent.mjs',
    named: "entry: resolve(root, 'lib/ai/server-entry.ts')",
  },
};

/** Build configuration whose imports of source the build itself needs. */
const CONFIG_IMPORTERS = ['tailwind.config.ts', 'vite.config.ts'];

/** Unreachable modules the gate tolerates, as path → why the module stays without an importer. */
const ALLOWLIST: Record<string, string> = {
  // The controls library (UI review 2026-11-14, phase C, lane C1): built before any page uses it,
  // placed by the four C2 lanes, whose work list is __tests__/lib/controls-gate.test.ts; the dev
  // proof page (proof/controls.tsx, outside the build) mounts it meanwhile. An entry goes when its
  // module is first imported by a page (this test then fails until it does).
  'components/ui/tabs.tsx':
    'the controls library’s tab row (lane C1): placed by the C2 lanes, mounted meanwhile by proof/controls.tsx',
  'components/ui/range-select.tsx':
    'the controls library’s range (lane C1): placed by the C2 lanes, mounted meanwhile by proof/controls.tsx',
  'components/ui/copy-button.tsx':
    'the controls library’s copy button (lane C1): placed by the C2 lanes, mounted meanwhile by proof/controls.tsx',
};

const isModule = (path: string) => /\.tsx?$/.test(path) && !path.endsWith('.d.ts');
const inScope = (path: string) => isModule(path) && SOURCE_DIRS.includes(path.split('/')[0]);

interface Specifier {
  spec: string;
  line: number;
}

/** The module specifiers a source text imports, and the imports the gate cannot follow. */
function importsOf(path: string, code: string): { specs: Specifier[]; unfollowable: string[] } {
  const source = ts.createSourceFile(
    path,
    code,
    ts.ScriptTarget.Latest,
    true,
    path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const lineOf = (node: ts.Node) =>
    source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  const specs: Specifier[] = [];
  const unfollowable: string[] = [];
  const literal = (node: ts.Node | undefined) =>
    node && (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node))
      ? node.text
      : null;
  const visit = (node: ts.Node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      const spec = literal(node.moduleSpecifier);
      if (spec !== null) specs.push({ spec, line: lineOf(node) });
    } else if (
      ts.isImportEqualsDeclaration(node) &&
      ts.isExternalModuleReference(node.moduleReference)
    ) {
      const spec = literal(node.moduleReference.expression);
      if (spec !== null) specs.push({ spec, line: lineOf(node) });
    } else if (ts.isImportTypeNode(node)) {
      const arg = ts.isLiteralTypeNode(node.argument) ? literal(node.argument.literal) : null;
      if (arg !== null) specs.push({ spec: arg, line: lineOf(node) });
      else unfollowable.push(`${path}:${lineOf(node)} a type import of a non-literal`);
    } else if (ts.isCallExpression(node)) {
      const callee = node.expression;
      if (callee.kind === ts.SyntaxKind.ImportKeyword) {
        const spec = literal(node.arguments[0]);
        if (spec !== null) specs.push({ spec, line: lineOf(node) });
        else unfollowable.push(`${path}:${lineOf(node)} a dynamic import of a non-literal`);
      } else if (
        ts.isPropertyAccessExpression(callee) &&
        ts.isMetaProperty(callee.expression) &&
        callee.expression.keywordToken === ts.SyntaxKind.ImportKeyword &&
        callee.name.text.startsWith('glob')
      ) {
        unfollowable.push(`${path}:${lineOf(node)} an import.meta.${callee.name.text}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { specs, unfollowable };
}

/**
 * The file an `@/` or relative specifier names (the alias is the frontend root, as in vite.config.ts
 * and tsconfig.json), `undefined` when it names none, `null` for a package (not followed).
 */
function resolveSpecifier(
  from: string,
  spec: string,
  files: Record<string, string>
): string | null | undefined {
  let base: string;
  if (spec.startsWith('@/')) base = spec.slice(2);
  else if (spec.startsWith('./') || spec.startsWith('../'))
    base = posix.normalize(posix.join(posix.dirname(from), spec));
  else return null;
  const candidates = [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`];
  return candidates.find((candidate) => candidate in files && !candidate.endsWith('.d.ts'));
}

/**
 * Walks the import graph from `roots` over `files` (path relative to the frontend root → source
 * text): the in-scope modules no root reaches, and every import met on the way that cannot be
 * followed (a non-literal dynamic import, a glob, a local specifier that resolves to no file).
 */
function orphanModules(
  files: Record<string, string>,
  roots: string[]
): { orphans: string[]; unfollowable: string[] } {
  const reached = new Set<string>();
  const unfollowable: string[] = [];
  const queue = [...roots];
  while (queue.length > 0) {
    const path = queue.pop()!;
    if (reached.has(path)) continue;
    if (!(path in files)) {
      unfollowable.push(`${path} is an entry that is not a file`);
      continue;
    }
    reached.add(path);
    if (!isModule(path)) continue;
    const found = importsOf(path, files[path]);
    unfollowable.push(...found.unfollowable);
    for (const { spec, line } of found.specs) {
      const target = resolveSpecifier(path, spec, files);
      if (target === undefined) unfollowable.push(`${path}:${line} '${spec}' resolves to no file`);
      else if (target !== null) queue.push(target);
    }
  }
  const orphans = Object.keys(files)
    .filter((path) => inScope(path) && !reached.has(path))
    .sort();
  return { orphans, unfollowable };
}

describe('the orphan gate finds what it must, and only that', () => {
  const ROOTS = ['src/main.tsx', 'tailwind.config.ts'];

  it('reaches static, type-only, re-exported, dynamic, index and relative imports', () => {
    const files: Record<string, string> = {
      'src/main.tsx': [
        "import { App } from '@/src/app';",
        "import '@/app/globals.css';",
        "import { useThing } from 'react';",
      ].join('\n'),
      'app/globals.css': '.a { color: red; }',
      'src/app.tsx': [
        "import type { Row } from '@/lib/types';",
        "export { Wave } from '@/components/wave';",
        "export * from '../lib/format';",
        "const Page = lazyPage(() => import('@/app/blocks/page'));",
        'const Graph = dynamic(() => import(`@/components/graph`), { ssr: false });',
        "type Mod = typeof import('@/lib/only-a-type-position');",
        'export const App = () => null;',
      ].join('\n'),
      'lib/types.ts': 'export interface Row { id: string }',
      'components/wave/index.tsx': "import { tick } from './tick';\nexport const Wave = tick;",
      'components/wave/tick.ts': 'export const tick = 1;',
      'lib/format.ts': 'export const format = 1;',
      'app/blocks/page.tsx': 'export default function Page() { return null; }',
      'components/graph.tsx': 'export default function Graph() { return null; }',
      'lib/only-a-type-position.ts': 'export const x = 1;',
      'tailwind.config.ts': "import { palette } from './lib/palette';\nexport default palette;",
      'lib/palette.ts': 'export const palette = {};',
    };
    expect(orphanModules(files, ROOTS)).toEqual({ orphans: [], unfollowable: [] });
  });

  it('finds an orphan, one only a test imports, and what only orphans import, cycles included', () => {
    const files: Record<string, string> = {
      'src/main.tsx': "import { live } from '@/lib/live';",
      'tailwind.config.ts': 'export default {};',
      'lib/live.ts': [
        'export const live = 1;',
        "// import { gone } from '@/components/commented-out';",
        'const words = \'import("@/components/in-a-string")\';',
      ].join('\n'),
      'components/old-card.tsx': "import { helper } from '@/lib/helper';\nexport const Old = 1;",
      'lib/helper.ts': "import { back } from '@/components/old-card';\nexport const helper = 1;",
      'components/tested-only.tsx': 'export const TestedOnly = 1;',
      '__tests__/components/tested-only.test.tsx':
        "import { TestedOnly } from '@/components/tested-only';",
      'components/commented-out.tsx': 'export const gone = 1;',
      'components/in-a-string.tsx': 'export const s = 1;',
      'lib/types.d.ts': 'declare const x: number;',
      'scripts/tool.ts': 'export const outside = 1;',
    };
    expect(orphanModules(files, ROOTS)).toEqual({
      orphans: [
        'components/commented-out.tsx',
        'components/in-a-string.tsx',
        'components/old-card.tsx',
        'components/tested-only.tsx',
        'lib/helper.ts',
      ],
      unfollowable: [],
    });
  });

  it('fails on a dynamic import of a non-literal, a glob, and a local specifier with no file', () => {
    const files: Record<string, string> = {
      'src/main.tsx': [
        'const page = (name: string) => import(`@/app/${name}/page`);',
        "const pages = import.meta.glob('@/app/**/page.tsx');",
        "import { gone } from '@/lib/deleted';",
        "import { thing } from './missing';",
        "import { ok } from 'some-package';",
      ].join('\n'),
      'tailwind.config.ts': 'export default {};',
    };
    expect(orphanModules(files, ROOTS)).toEqual({
      orphans: [],
      unfollowable: [
        'src/main.tsx:1 a dynamic import of a non-literal',
        'src/main.tsx:2 an import.meta.glob',
        "src/main.tsx:3 '@/lib/deleted' resolves to no file",
        "src/main.tsx:4 './missing' resolves to no file",
      ],
    });
  });

  it('fails on an entry that is not a file', () => {
    expect(orphanModules({ 'tailwind.config.ts': '' }, ROOTS).unfollowable).toEqual([
      'src/main.tsx is an entry that is not a file',
    ]);
  });
});

describe('no orphan modules (lane E1)', () => {
  const root = join(__dirname, '..', '..');
  const files: Record<string, string> = {};
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else files[relative(root, path).split(sep).join('/')] = readFileSync(path, 'utf8');
    }
  };
  for (const dir of SOURCE_DIRS) walk(join(root, dir));
  for (const config of CONFIG_IMPORTERS) files[config] = readFileSync(join(root, config), 'utf8');
  const { orphans, unfollowable } = orphanModules(files, [
    ...Object.keys(ENTRIES),
    ...CONFIG_IMPORTERS,
  ]);

  it('takes its entries from where the build names them', () => {
    for (const [entry, { owner, named }] of Object.entries(ENTRIES)) {
      expect(readFileSync(join(root, owner), 'utf8'), entry).toContain(named);
    }
    const html = readFileSync(join(root, 'index.html'), 'utf8');
    const moduleScripts = [...html.matchAll(/<script type="module" src="\/([^"]+)">/g)].map(
      (m) => m[1]
    );
    const fromHtml = Object.entries(ENTRIES)
      .filter(([, { owner }]) => owner === 'index.html')
      .map(([entry]) => entry);
    expect(moduleScripts).toEqual(fromHtml);
    expect(readFileSync(join(root, 'package.json'), 'utf8')).toContain(
      'vite build && node scripts/build-agent.mjs'
    );
  });

  it('scans the source', () => {
    expect(Object.keys(files).filter(inScope).length).toBeGreaterThan(300);
  });

  it('follows every import it meets', () => {
    expect(unfollowable).toEqual([]);
  });

  it('leaves no module unreachable', () => {
    expect(orphans.filter((path) => !(path in ALLOWLIST))).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [path, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, path).toBeGreaterThan(10);
      expect(orphans, path).toContain(path);
    }
  });
});
