import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { virtualProgram } from '@/__tests__/utils/virtual-program';

/**
 * Gate for dead exports (UI review 2026-11-14, lane E3): every symbol a module under the five
 * source directories exports has a user in the app's own code, so no function, constant,
 * component or type stays behind that nothing runs. The orphan gate (no-orphan-modules) keeps
 * every MODULE reached; this one keeps every EXPORT used.
 *
 * An export is USED when an identifier in a non-test source file resolves to it through the
 * TypeScript checker — a call, a JSX tag, a type position, a property of a namespace or of a
 * dynamically imported module, a shorthand `{ name }`, a destructured `{ name }` of a module —
 * in another file or in its own (an export only its own file uses is used; its `export` keyword
 * is not worth the churn). An import through a re-export uses the re-export too, so a re-export
 * nobody imports through is dead even when its origin is used elsewhere. An `export { … }` list,
 * an `export … from` and an `export default name` declare, they do not use. A module a dynamic
 * `import('…')` loads has its default export used (the router's `lazyPage`, `dynamic`). The
 * users are the five directories and the build configuration that imports source
 * (tailwind.config.ts, vite.config.ts); tests are not users: an export only tests import is dead,
 * and goes with its tests.
 *
 * The external entries' exports are used by what bundles them (EXTERNAL_ENTRIES). The few exports
 * kept without a user are in ALLOWLIST, each with its reason; an entry whose export gains a user
 * or goes fails the gate, so the list stays current.
 *
 * The seal library (`lib/seal/`) is outside the scope by rule (RECORD_DIRS): it is the seal
 * design's own record beside its drawing code — what each approved glyph and rim is called, in
 * English and Chinese, which glyph is drawn from a later script, the order the design names the
 * classes — pinned against the prototype by the fidelity tests so the drawn set cannot drift from
 * the approved one. Its tables are kept whether or not a page reads them yet. Its files still
 * count as users of other modules' exports.
 *
 * Limits: a use the checker cannot resolve is not seen — a module's export read by a computed key
 * (`module[name]`), or named only outside TypeScript (index.html, a stylesheet, the Rust crates);
 * the source has none today but the external entry, and such an export is listed above with the
 * file that names it.
 *
 * The pass builds one TypeScript program over the source (about 1 s) and resolves every
 * identifier in it (about 2 s): the whole file runs in about 3.5 s.
 */

/** The directories whose exports must be used (`.d.ts` files are out of scope). */
const SOURCE_DIRS = ['app', 'components', 'hooks', 'lib', 'src'];

/** Directories that record a design, kept whole whether or not a page reads them (see above). */
const RECORD_DIRS = ['lib/seal/'];

/** Whether the gate judges the exports of a file, by its path from the frontend root. */
const judged = (path: string) =>
  SOURCE_DIRS.includes(path.split('/')[0]) && !RECORD_DIRS.some((dir) => path.startsWith(dir));

/** Build configuration that imports source: its uses count. */
const CONFIG_IMPORTERS = ['tailwind.config.ts', 'vite.config.ts'];

/** Modules whose exports are an external API, each with the file that bundles it and the text that names it there. */
const EXTERNAL_ENTRIES: Record<string, { owner: string; named: string; why: string }> = {
  'lib/ai/server-entry.ts': {
    owner: 'scripts/build-agent.mjs',
    named: "entry: resolve(root, 'lib/ai/server-entry.ts')",
    why: 'bundled as the IIFE global CkbadgerAgent: the API server calls render, the build calls routeDocumentation',
  },
};

/**
 * Exports kept with no user, as `path#name` → why, and where the reason can be read, the file and
 * text that must hold it.
 */
const ALLOWLIST: Record<string, { why: string; anchor?: { file: string; text: string } }> = {
  'lib/pipeline/layout.ts#POOL_PENDING_CAPACITY': {
    why: 'the frontend side of the pipeline truncation contract: the API serves this many pending transactions',
    anchor: { file: '../crates/api/src/routes/pipeline.rs', text: '`POOL_PENDING_CAPACITY`' },
  },
  'lib/pipeline/layout.ts#POOL_PROPOSED_CAPACITY': {
    why: 'the frontend side of the pipeline truncation contract: the API serves this many proposed transactions',
    anchor: { file: '../crates/api/src/routes/pipeline.rs', text: '`POOL_PROPOSED_CAPACITY`' },
  },
  'lib/pipeline/layout.ts#BLOCK_FIELD_CAPACITY': {
    why: "the frontend side of the pipeline truncation contract: a block card's field, the API serving one fewer (the cellbase)",
    anchor: { file: '../crates/api/src/routes/pipeline.rs', text: '`BLOCK_FIELD_CAPACITY`' },
  },
  'lib/palette.ts#apcaLc': {
    why: 'an instrument (readability plan 2026-11-18, ruling R2): the perceptual grade palette.test.ts pins the night text by, kept beside contrastRatio so every check of a pairing reads one implementation; no page grades an ink at run time',
    anchor: { file: 'lib/palette.ts', text: 'palette.test pins the floors' },
  },
  'lib/theme.ts#resetThemeForTests': {
    why: 'a test seam, by its name: the theme store keeps module state (the snapshot, its listeners, the window watchers) that each test must drop; the app never resets it',
    anchor: { file: 'lib/theme.ts', text: 'Test seam:' },
  },
  // The controls library (UI review 2026-11-14, phase C, lane C1), built before any page uses it:
  // the C2 lanes place each component, from the controls gate's work list; an entry goes when a
  // page first draws its component (this test then fails until it does).
  'components/ui/tabs.tsx#Tabs': {
    why: 'the controls library’s tab row (lane C1): placed by the C2 lanes; mounted meanwhile by the dev proof page proof/controls.tsx',
    anchor: { file: '__tests__/lib/controls-gate.test.ts', text: "'components/ui/tabs.tsx'" },
  },
  'components/ui/filter-chips.tsx#FilterChips': {
    why: 'the controls library’s filter chips (lane C1): placed by the C2 lanes; mounted meanwhile by the dev proof page proof/controls.tsx',
    anchor: {
      file: '__tests__/lib/controls-gate.test.ts',
      text: "'components/ui/filter-chips.tsx'",
    },
  },
  'components/ui/range-select.tsx#RangeSelect': {
    why: 'the controls library’s range (lane C1): placed by the C2 lanes; mounted meanwhile by the dev proof page proof/controls.tsx',
    anchor: {
      file: '__tests__/lib/controls-gate.test.ts',
      text: "'components/ui/range-select.tsx'",
    },
  },
  'components/ui/copy-button.tsx#CopyButton': {
    why: 'the controls library’s copy button (lane C1): placed by the C2 lanes; mounted meanwhile by the dev proof page proof/controls.tsx',
    anchor: {
      file: '__tests__/lib/controls-gate.test.ts',
      text: "'components/ui/copy-button.tsx'",
    },
  },
  'components/ui/text-field.tsx#TextField': {
    why: 'the controls library’s text field (lane C1): placed by the C2 lanes; mounted meanwhile by the dev proof page proof/controls.tsx',
    anchor: {
      file: '__tests__/lib/controls-gate.test.ts',
      text: "'components/ui/text-field.tsx'",
    },
  },
};

const isSource = (path: string) => /\.tsx?$/.test(path) && !path.endsWith('.d.ts');

/**
 * The exports of `scope` that no file of `users` uses, as `path:line name`. `rel` names a file
 * for the report; `external` lists the files whose exports are used from outside.
 */
function deadExports(
  program: ts.Program,
  users: readonly string[],
  scope: readonly string[],
  rel: (file: string) => string,
  external: ReadonlySet<string> = new Set()
): string[] {
  const checker = program.getTypeChecker();
  const used = new Set<ts.Symbol>();
  const mark = (symbol: ts.Symbol | undefined) => {
    // every step of an alias chain is used: an import through a re-export uses the re-export
    let s = symbol;
    for (let step = 0; s && step < 64; step++) {
      used.add(s);
      if (!(s.flags & ts.SymbolFlags.Alias)) return;
      const next = checker.getImmediateAliasedSymbol(s);
      if (!next || next === s) break;
      s = next;
    }
    if (symbol && symbol.flags & ts.SymbolFlags.Alias) used.add(checker.getAliasedSymbol(symbol));
  };
  const moduleExport = (specifier: ts.Expression, name: string) => {
    const module = checker.getSymbolAtLocation(specifier);
    return module ? checker.getExportsOfModule(module).find((e) => e.name === name) : undefined;
  };
  for (const file of users) {
    const source = program.getSourceFile(file);
    if (!source) throw new Error(`${rel(file)} is not in the program`);
    const visit = (node: ts.Node) => {
      if (
        ts.isCallExpression(node) &&
        node.expression.kind === ts.SyntaxKind.ImportKeyword &&
        node.arguments[0] &&
        ts.isStringLiteralLike(node.arguments[0])
      )
        mark(moduleExport(node.arguments[0], 'default'));
      if (ts.isIdentifier(node)) {
        const parent = node.parent;
        if (ts.isShorthandPropertyAssignment(parent) && parent.name === node) {
          mark(checker.getShorthandAssignmentValueSymbol(parent));
        } else if (
          ts.isBindingElement(parent) &&
          parent.name === node &&
          !parent.propertyName &&
          ts.isObjectBindingPattern(parent.parent)
        ) {
          // `const { name } = module`: the property it reads
          const type = checker.getTypeAtLocation(parent.parent);
          mark(type.getProperty(node.text));
        } else if (!ts.isExportSpecifier(parent) && !ts.isExportAssignment(parent)) {
          const symbol = checker.getSymbolAtLocation(node);
          const declares =
            symbol?.declarations?.some(
              (d) =>
                (d as ts.NamedDeclaration).name === node && !ts.isShorthandPropertyAssignment(d)
            ) ?? false;
          if (symbol && !declares) mark(symbol);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  const dead: string[] = [];
  for (const file of scope) {
    const source = program.getSourceFile(file);
    const module = source && checker.getSymbolAtLocation(source);
    if (!source || !module || external.has(file)) continue;
    for (const exported of checker.getExportsOfModule(module)) {
      const alias = (exported.flags & ts.SymbolFlags.Alias) !== 0;
      const target = alias ? checker.getAliasedSymbol(exported) : exported;
      const declaration = (alias ? exported : target).declarations?.[0];
      if (!declaration) continue;
      // a star re-export is judged where its symbol is declared
      if (!alias && declaration.getSourceFile() !== source) continue;
      const origin = target.declarations?.[0]?.getSourceFile();
      const reexport = alias && origin !== undefined && origin !== source;
      if (used.has(exported) || (!reexport && used.has(target))) continue;
      const line = source.getLineAndCharacterOfPosition(declaration.getStart(source)).line + 1;
      dead.push(`${rel(file)}:${line} ${exported.name}`);
    }
  }
  return dead.sort();
}

function virtualDead(files: Record<string, string>, external: string[] = []): string[] {
  const program = virtualProgram(files);
  const paths = Object.keys(files).map((f) => `/v/${f}`);
  const users = paths.filter((p) => !p.includes('__tests__/'));
  const scope = users.filter((p) => judged(p.slice(3)));
  return deadExports(
    program,
    users,
    scope,
    (f) => f.slice(3),
    new Set(external.map((f) => `/v/${f}`))
  );
}

describe('the export gate finds what it must, and only that', () => {
  it('flags an unused function, constant, component, type and interface, and a test-only export', () => {
    expect(
      virtualDead({
        'lib/a.ts': [
          'export function unused(): number { return 1; }',
          'export const UNUSED = 2;',
          'export type Unused = { a: number };',
          'export interface UnusedShape { a: number }',
          'export const testedOnly = 3;',
          'export const used = 4;',
        ].join('\n'),
        'components/b.tsx': [
          "import { used } from '@/lib/a';",
          'export function Card() { return <i>{used}</i>; }',
        ].join('\n'),
        '__tests__/a.test.ts': [
          "import { testedOnly } from '@/lib/a';",
          "import { Card } from '@/components/b';",
          'testedOnly; Card;',
        ].join('\n'),
      })
    ).toEqual([
      'components/b.tsx:2 Card',
      'lib/a.ts:1 unused',
      'lib/a.ts:2 UNUSED',
      'lib/a.ts:3 Unused',
      'lib/a.ts:4 UnusedShape',
      'lib/a.ts:5 testedOnly',
    ]);
  });

  it('counts a call, a JSX tag, a type, a shorthand, its own file, a namespace and a destructure', () => {
    expect(
      virtualDead({
        'lib/a.ts': [
          'export function call(): number { return 1; }',
          'export function Tag() { return null; }',
          'export interface Shape { a: number }',
          'export const short = 2;',
          'export const ownFile = 3;',
          'export const viaNamespace = 4;',
          'export const viaDestructure = 5;',
          'export const viaRename = 6;',
          'export const local = ownFile + 1;',
        ].join('\n'),
        'src/main.tsx': [
          "import { call, Tag, type Shape, short, local } from '@/lib/a';",
          "import * as ns from '@/lib/a';",
          'const s: Shape = { a: call() };',
          'const o = { short };',
          'const v = ns.viaNamespace;',
          'const { viaRename: renamed } = ns;',
          'const load = () => { const { viaDestructure } = ns; return viaDestructure + renamed; };',
          'export const App = () => <Tag />;',
          's; o; v; load; local; App;',
        ].join('\n'),
      })
    ).toEqual([]);
  });

  it('uses the default export of a dynamically imported module, and nothing else of it', () => {
    expect(
      virtualDead({
        'app/page.tsx':
          'export const revalidate = 0;\nexport default function Page() { return null; }',
        'src/router.ts': "const page = () => import('@/app/page');\npage;",
      })
    ).toEqual(['app/page.tsx:1 revalidate']);
  });

  it('flags a re-export nobody imports through, and follows one that is', () => {
    expect(
      virtualDead({
        'lib/origin.ts':
          'export const direct = 1;\nexport const through = 2;\nexport const never = 3;',
        'lib/barrel.ts': [
          "export { direct } from '@/lib/origin';",
          "export { through } from '@/lib/origin';",
          "export { never } from '@/lib/origin';",
        ].join('\n'),
        'src/main.ts': [
          "import { direct } from '@/lib/origin';",
          "import { through } from '@/lib/barrel';",
          'direct; through;',
        ].join('\n'),
      })
    ).toEqual(['lib/barrel.ts:1 direct', 'lib/barrel.ts:3 never', 'lib/origin.ts:3 never']);
  });

  it('reads an export list as a declaration, not a use, and keeps an external entry whole', () => {
    expect(
      virtualDead(
        {
          'lib/list.ts': 'const listed = 1;\nconst kept = 2;\nexport { listed, kept };\nkept;',
          'lib/entry.ts': 'export function render() { return 1; }',
        },
        ['lib/entry.ts']
      )
    ).toEqual(['lib/list.ts:3 listed']);
  });

  it('keeps the seal library’s record out of scope, and flags the same shape anywhere else', () => {
    const table = [
      'export const LABEL = { sun: ["sun", "日"] } as const;',
      "export const ORDER = ['ckb', 'dao'] as const;",
    ].join('\n');
    expect(
      virtualDead({
        'lib/seal/glyphs.ts': table,
        'lib/sealed.ts': table,
        'lib/other/seal/window.ts': table,
      })
    ).toEqual([
      'lib/other/seal/window.ts:1 LABEL',
      'lib/other/seal/window.ts:2 ORDER',
      'lib/sealed.ts:1 LABEL',
      'lib/sealed.ts:2 ORDER',
    ]);
    // a module in the record still counts as a user of what it imports
    expect(
      virtualDead({
        'lib/seal/core.ts': "import { brush } from '@/lib/brush';\nexport const RIM = brush;",
        'lib/brush.ts': 'export const brush = 1;',
      })
    ).toEqual([]);
  });
});

describe('no dead exports (lane E3)', () => {
  const root = join(__dirname, '..', '..');
  const rel = (file: string) => relative(root, file).split(sep).join('/');
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (isSource(name)) files.push(path);
    }
  };
  for (const dir of SOURCE_DIRS) walk(join(root, dir));
  const users = [...files, ...CONFIG_IMPORTERS.map((config) => join(root, config))];
  const parsed = ts.getParsedCommandLineOfConfigFile(join(root, 'tsconfig.json'), undefined, {
    ...ts.sys,
    onUnRecoverableConfigFileDiagnostic: (diagnostic) => {
      throw new Error(ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'));
    },
  });
  if (!parsed) throw new Error('tsconfig.json did not parse');
  const program = ts.createProgram({
    rootNames: users,
    options: { ...parsed.options, incremental: false, noEmit: true },
  });
  const external = new Set(Object.keys(EXTERNAL_ENTRIES).map((entry) => join(root, entry)));
  const dead = deadExports(
    program,
    users,
    files.filter((file) => judged(rel(file))),
    rel,
    external
  );
  const key = (site: string) => {
    const [path, name] = site.split(' ');
    return `${path.split(':')[0]}#${name}`;
  };

  it('takes its external entries from where the build names them', () => {
    for (const [entry, { owner, named }] of Object.entries(EXTERNAL_ENTRIES)) {
      expect(readFileSync(join(root, owner), 'utf8'), entry).toContain(named);
    }
  });

  it('scans the source', () => {
    expect(files.length).toBeGreaterThan(300);
  });

  it('leaves no export without a user', () => {
    expect(
      dead.filter((site) => !(key(site) in ALLOWLIST)),
      `${dead.filter((site) => !(key(site) in ALLOWLIST)).length} dead exports`
    ).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [site, { why, anchor }] of Object.entries(ALLOWLIST)) {
      expect(why.trim().length, site).toBeGreaterThan(10);
      expect(dead.map(key), site).toContain(site);
      if (anchor)
        expect(readFileSync(join(root, anchor.file), 'utf8'), site).toContain(anchor.text);
    }
  });
});
