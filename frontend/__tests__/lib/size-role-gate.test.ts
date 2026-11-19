import { readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { sourceFiles, unwrap } from '@/__tests__/utils/source-scan';
import { TYPE_STEPS } from '@/lib/type-scale';

/**
 * Gate for the size of a word's role (readability plan 2026-11-18, the ink and size roles; lane
 * cd-fixes-controls, the shared lane's round): a link's words and a table cell's value read at
 * 13 px or more; the meta size (`text-meta`, 12 px) is for a label, a unit, a column head, an axis
 * tick, a hint and an eyebrow. The ink-role gate's sibling: it reads the same two kinds of element
 * the ink gate reads for a link and a cell, by the same route, for their size.
 *
 * - `link`: a link renderer: `<a>`, `<Address>` or a component named `…Link`, at 12 px itself or
 *   with words in an element of 12 px inside it (a label within the link: link text all the same);
 * - `td`: a table cell: a `<td>`, the `className` of a `ListTable` column (an object with `render`
 *   and `minWidth`), or the root element its `render` returns (a link there is counted once, as a
 *   link).
 *
 * An element is at the meta size by its own class (`text-meta` at rest: with no variant, or a
 * responsive one, so `text-meta md:text-small` is 12 px below md and counts; an arbitrary size of
 * 12 px or less, `text-[11px]`), or, when it sets no size of its own, by the nearest size its JSX
 * ancestors in the same file set around it (a link in a 12 px note). A class is read through `?:`,
 * `&&`, `cn(…)`, template literals and the file's own constants, so one that is 12 px in one branch
 * counts. An element handed to a component as a prop is the component's to size, and is not
 * followed into it.
 *
 * Limits, what it cannot see: a size set by a component on its children (PhoneRow's line tones, a
 * `size` prop such as Hash's `tight`), a size from a module's constant imported into the file, a
 * size in CSS, and SVG text (`fontSize` on a `<text>`: the chart lane's own probe holds it).
 *
 * The sites tolerated today are listed per file with their family and a reason: the page lanes'
 * work list in the ink and size round, driven to 0. A new site fails, and so does a site that went
 * (its count must be lowered), so the list only shrinks; its totals by family are pinned.
 */

type Kind = 'link' | 'td';
const KINDS: readonly Kind[] = ['link', 'td'];

/** The lane whose pages a file belongs to. */
type Family = 'chain' | 'assets' | 'lists' | 'topics' | 'charts' | 'shared';

interface Tolerated {
  family: Family;
  /** The sites of each kind in the file, today. */
  sites: Partial<Record<Kind, number>>;
  /** What they are, in one line. */
  reason: string;
}

interface Site {
  kind: Kind;
  line: number;
  what: string;
}

/** Variants that hold at rest: the responsive ones. A state variant (hover:, group-hover:) does not. */
const AT_REST = /^(?:sm|md|lg|xl|2xl|max-sm|max-md|max-lg|max-xl|min-\[.+\]|max-\[.+\])$/;

/** A class's variants and its utility, split at the colons outside brackets. */
function variantsOf(token: string): { variants: string[]; utility: string } {
  const parts: string[] = [];
  let depth = 0;
  let current = '';
  for (const ch of token) {
    if (ch === '[') depth++;
    if (ch === ']') depth--;
    if (ch === ':' && depth === 0) {
      parts.push(current);
      current = '';
    } else current += ch;
  }
  return { variants: parts, utility: current };
}

/**
 * The size a class sets at rest: `meta` (the meta step, or an arbitrary size of 12 px or less),
 * `step` (another step of the type scale, or a larger arbitrary size), or none.
 */
function sizeOfClass(token: string): 'meta' | 'step' | null {
  const { variants, utility: written } = variantsOf(token);
  if (!variants.every((v) => AT_REST.test(v))) return null;
  const utility = written.replace(/^!/, '').replace(/\/(?:[\d.]+|\[[^\]]*\])$/, '');
  if (!utility.startsWith('text-')) return null;
  const key = utility.slice('text-'.length);
  if (key === 'meta') return 'meta';
  if ((TYPE_STEPS as readonly string[]).includes(key)) return 'step';
  const px = /^\[(\d+(?:\.\d+)?)px\]$/.exec(key);
  if (px) return Number(px[1]) <= 12 ? 'meta' : 'step';
  return null;
}

const LINK = (tag: string) => tag === 'a' || tag === 'Address' || /^(?:[A-Z]\w*)?Link$/.test(tag);

/** Every site of the two kinds at the meta size in a TSX source. */
export function sizeRoleSites(code: string): Site[] {
  const source = ts.createSourceFile(
    'scan.tsx',
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX
  );
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const text = (n: ts.Node) => n.getText(source).replace(/\s+/g, ' ');

  const locals = new Map<string, ts.Expression[]>();
  const collect = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      locals.set(node.name.text, [...(locals.get(node.name.text) ?? []), node.initializer]);
    ts.forEachChild(node, collect);
  };
  collect(source);

  /** The strings an expression can reach: literals, template parts, branches, call arguments, constants. */
  const leaves = (e0: ts.Expression, depth = 0): string[] => {
    if (depth > 12) return [];
    const e = unwrap(e0);
    const next = (x: ts.Expression) => leaves(x, depth + 1);
    if (ts.isStringLiteralLike(e)) return [e.text];
    if (ts.isTemplateExpression(e))
      return [
        e.head.text,
        ...e.templateSpans.flatMap((s) => [s.literal.text, ...next(s.expression)]),
      ];
    if (ts.isConditionalExpression(e)) return [...next(e.whenTrue), ...next(e.whenFalse)];
    if (ts.isBinaryExpression(e)) return [...next(e.left), ...next(e.right)];
    if (ts.isCallExpression(e)) return e.arguments.flatMap(next);
    if (ts.isArrayLiteralExpression(e))
      return e.elements.flatMap((el) => next(ts.isSpreadElement(el) ? el.expression : el));
    if (ts.isObjectLiteralExpression(e))
      return e.properties.flatMap((p) => (ts.isPropertyAssignment(p) ? next(p.initializer) : []));
    if (ts.isIdentifier(e)) return (locals.get(e.text) ?? []).flatMap(next);
    if (ts.isPropertyAccessExpression(e)) {
      const base = unwrap(e.expression);
      if (!ts.isIdentifier(base)) return [];
      return (locals.get(base.text) ?? []).flatMap((t0) => {
        const t = unwrap(t0);
        if (!ts.isObjectLiteralExpression(t)) return [];
        return t.properties.flatMap((p) =>
          ts.isPropertyAssignment(p) &&
          (ts.isIdentifier(p.name) || ts.isStringLiteralLike(p.name)) &&
          p.name.text === e.name.text
            ? next(p.initializer)
            : []
        );
      });
    }
    return [];
  };
  const tokensOf = (e: ts.Expression) => leaves(e).flatMap((s) => s.split(/\s+/).filter(Boolean));

  type El = ts.JsxElement | ts.JsxSelfClosingElement;
  const opening = (n: El) => (ts.isJsxElement(n) ? n.openingElement : n);
  const tagOf = (n: El) => text(opening(n).tagName);
  const className = (n: El): ts.Expression | undefined => {
    for (const a of opening(n).attributes.properties) {
      if (!ts.isJsxAttribute(a) || text(a.name) !== 'className' || !a.initializer) continue;
      return ts.isJsxExpression(a.initializer) ? a.initializer.expression : a.initializer;
    }
    return undefined;
  };
  /** An element's own size: whether any branch of its class is 12 px, and whether it sets one. */
  const sizes = (n: El) => {
    const value = className(n);
    const of = (value ? tokensOf(value) : []).map(sizeOfClass);
    return { meta: of.includes('meta'), sized: of.some((s) => s !== null) };
  };
  /** The size an element takes from around it in its file: the nearest JSX ancestor's. */
  const around = (node: ts.Node): 'meta' | 'step' | 'none' => {
    for (let p: ts.Node | undefined = node.parent; p; p = p.parent) {
      if (ts.isJsxAttribute(p)) return 'none';
      if (ts.isJsxElement(p)) {
        const s = sizes(p);
        if (s.meta) return 'meta';
        if (s.sized) return 'step';
      }
    }
    return 'none';
  };
  /** The first element inside an element's children that sets the meta size, if one does. */
  const metaInside = (el: ts.JsxElement): string | null => {
    let hit: string | null = null;
    const walk = (n: ts.Node) => {
      if (hit !== null || ts.isJsxAttribute(n)) return;
      if ((ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n)) && n !== el && sizes(n).meta) {
        hit = tagOf(n);
        return;
      }
      ts.forEachChild(n, walk);
    };
    el.children.forEach(walk);
    return hit;
  };
  /** The root elements a render function returns. */
  const roots = (e0: ts.Expression | undefined): El[] => {
    if (!e0) return [];
    const e = unwrap(e0);
    if (ts.isJsxElement(e) || ts.isJsxSelfClosingElement(e)) return [e];
    if (ts.isConditionalExpression(e)) return [...roots(e.whenTrue), ...roots(e.whenFalse)];
    if (ts.isBinaryExpression(e)) return [...roots(e.left), ...roots(e.right)];
    return [];
  };
  const returned = (fn: ts.Expression): El[] => {
    const f = unwrap(fn);
    if (!ts.isArrowFunction(f) && !ts.isFunctionExpression(f)) return [];
    if (!ts.isBlock(f.body)) return roots(f.body);
    const out: El[] = [];
    const walk = (n: ts.Node) => {
      if (ts.isFunctionLike(n) && n !== f) return;
      if (ts.isReturnStatement(n)) out.push(...roots(n.expression));
      ts.forEachChild(n, walk);
    };
    walk(f.body);
    return out;
  };

  const sites: Site[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = tagOf(node);
      const kind: Kind | null = LINK(tag) ? 'link' : tag === 'td' ? 'td' : null;
      if (kind !== null) {
        const own = sizes(node);
        if (own.meta || (!own.sized && around(node) === 'meta'))
          sites.push({ kind, line: lineOf(node), what: `<${tag}>` });
        // a link's words in an element of 12 px inside it (not into a prop's elements)
        else if (kind === 'link' && ts.isJsxElement(node)) {
          const inner = metaInside(node);
          if (inner) sites.push({ kind, line: lineOf(node), what: `<${tag}> words <${inner}>` });
        }
      }
    }
    // a ListTable column: an object literal with `render` and `minWidth`
    if (ts.isObjectLiteralExpression(node)) {
      const props = new Map<string, ts.Expression>();
      for (const p of node.properties)
        if (
          ts.isPropertyAssignment(p) &&
          (ts.isIdentifier(p.name) || ts.isStringLiteralLike(p.name))
        )
          props.set(p.name.text, p.initializer);
      if (props.has('render') && props.has('minWidth')) {
        const cls = props.get('className');
        if (cls && tokensOf(cls).some((t) => sizeOfClass(t) === 'meta'))
          sites.push({ kind: 'td', line: lineOf(cls), what: 'column className' });
        for (const root of returned(props.get('render')!))
          if (!LINK(tagOf(root)) && sizes(root).meta)
            sites.push({ kind: 'td', line: lineOf(root), what: `column render <${tagOf(root)}>` });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return sites.sort((a, b) => a.line - b.line);
}

describe('the size scan finds a link or a cell at 12 px, and only that', () => {
  const found = (code: string) => sizeRoleSites(code).map((s) => `${s.kind} ${s.line} ${s.what}`);

  it('finds a link at the meta size by its own class, at rest or below a breakpoint, or by its line’s', () => {
    expect(
      found(`export const A = () => (
        <div>
          <p className="text-meta"><Link href="/a">a</Link></p>
          <Link href="/b" className="text-meta">b</Link>
          <AppLink href="/c" className="text-meta md:text-small">c</AppLink>
          <Link href="/d" className="hover:text-meta">d</Link>
          <p className="text-meta"><Link href="/e" className="text-small">e</Link></p>
          <span className="text-[11px]"><a href="/f">f</a></span>
          <span className={cn('text-text', on ? 'text-meta' : 'text-small')}><BlockLink number={1} /></span>
        </div>
      );`)
    ).toEqual([
      'link 3 <Link>',
      'link 4 <Link>',
      'link 5 <AppLink>',
      'link 8 <a>',
      'link 9 <BlockLink>',
    ]);
  });

  it('finds a cell at the meta size: a td, a column’s class or its render’s root; not a label in it', () => {
    expect(
      found(`const META = 'text-meta';
        const columns = [
          { key: 'a', minWidth: 40, className: META, render: (r) => r.a },
          { key: 'b', minWidth: 40, render: (r) => <span className="text-meta">{r.b}</span> },
          { key: 'c', minWidth: 40, render: (r) => <Link href={r.c} className="text-meta">{r.c}</Link> },
          { key: 'd', minWidth: 40, render: (r) => <span>{r.d} <span className="text-meta">ckb</span></span> },
        ];
        export const T = () => (
          <table>
            <thead><tr><th className="text-meta">name</th></tr></thead>
            <tbody><tr><td className="text-meta">1</td><td>2 <span className="text-meta">CKB</span></td></tr></tbody>
          </table>
        );`)
    ).toEqual([
      'td 3 column className',
      'td 4 column render <span>',
      'link 5 <Link>',
      'td 11 <td>',
    ]);
  });

  // a link's words can be sized inside it: a span of 12 px words within the link (the vitals
  // strip's labels) is link text at 12 px
  it('finds a link whose words stand in a 12 px element inside it', () => {
    expect(
      found(`export const A = () => (
        <div>
          <Link href="/a" className="group flex">
            <span className="text-text-dim text-meta md:text-small">Tip</span>
            <span className="text-body">21,057,546</span>
          </Link>
          <Link href="/b"><span className="text-small">b</span></Link>
        </div>
      );`)
    ).toEqual(['link 3 <Link> words <span>']);
  });

  it('leaves an element handed to a component as a prop to the component', () => {
    expect(
      found(`export const A = () => (
        <div className="text-meta">
          <Row right={<Link href="/x">x</Link>} />
        </div>
      );`)
    ).toEqual([]);
  });
});

// ---- the work list ----

/**
 * The pinned total of TOLERATED's sites by family, and of its files: each only falls, and the page
 * lanes drive each family to 0 in the ink and size round (lane cd-fixes-controls landed the gate
 * with these: 28 sites in 18 files).
 */
const TOLERATED_BY_FAMILY: Readonly<Record<Family, number>> = {
  chain: 3,
  assets: 13,
  lists: 6,
  topics: 2,
  charts: 1,
  // shared 3 → 2 (lane cd-fixes-controls): the chart's markers' legend at 13 px
  shared: 2,
};
// 18 → 17 (lane cd-fixes-controls): components/ui/chart.tsx left the list
const TOLERATED_FILES = 17;

const WORK = 'the ink and size round’s work: 13 px (text-small, or the size of the line around it)';

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- chain ----
  'components/cell/code-cell-board.tsx': {
    family: 'chain',
    sites: { link: 1 },
    reason: `a code cell's script link in its 12 px reference line; ${WORK}`,
  },
  'components/transaction/script-group-matrix.tsx': {
    family: 'chain',
    sites: { link: 1 },
    reason: `a script group's link in a 12 px line; ${WORK}`,
  },
  'components/transaction/what-moved.tsx': {
    family: 'chain',
    sites: { link: 1 },
    reason: `a party's link in a 12 px line; ${WORK}`,
  },
  // ---- assets ----
  'components/asset/asset-history.tsx': {
    family: 'assets',
    sites: { link: 2 },
    reason: `a block link in a 12 px note and a party's address at the tight size in the supply's phone rows; ${WORK}`,
  },
  'components/asset/cell-versions.tsx': {
    family: 'assets',
    sites: { link: 2 },
    reason: `a version's cell link with its id at the tight size and its transaction link at 12 px; ${WORK}`,
  },
  'components/asset/genetics.tsx': {
    family: 'assets',
    sites: { link: 3 },
    reason: `the pattern's two source links and a cluster link in 12 px lines; ${WORK}`,
  },
  'components/asset/identity-document.tsx': {
    family: 'assets',
    sites: { link: 1 },
    reason: `the document's cell link, its id at the tight size; ${WORK}`,
  },
  'components/asset/item-evidence.tsx': {
    family: 'assets',
    sites: { link: 3 },
    reason: `an item's version, transaction and cell links in 12 px lines; ${WORK}`,
  },
  'components/identity/identity-evidence.tsx': {
    family: 'assets',
    sites: { link: 1 },
    reason: `an identity's record link in a 12 px line; ${WORK}`,
  },
  'components/scripts/script-code-evidence.tsx': {
    family: 'assets',
    sites: { link: 1 },
    reason: `a code cell's link in a 12 px line; ${WORK}`,
  },
  // ---- lists ----
  'app/blocks/page.tsx': {
    family: 'lists',
    sites: { link: 3, td: 1 },
    reason: `the blocks list's miner and reward links at 12 px and a 12 px cell; ${WORK}`,
  },
  'components/asset/inventory-summary.tsx': {
    family: 'lists',
    sites: { link: 1 },
    reason: `the inventory's as-of block link in a 12 px line; ${WORK}`,
  },
  'components/pipeline/pipeline-panel.tsx': {
    family: 'lists',
    sites: { link: 1 },
    reason: `the pipeline's pinned readout link in a 12 px line; ${WORK}`,
  },
  // ---- topics ----
  'components/dao/dao-board.tsx': {
    family: 'topics',
    sites: { link: 2 },
    reason: `the block links of the capacity-use caption and the 365-day anchor line, 12 px notes; ${WORK}`,
  },
  // ---- charts ----
  'components/charts/chart-small-multiple.tsx': {
    family: 'charts',
    sites: { link: 1 },
    reason: `a small multiple's chart link at 12 px; ${WORK}`,
  },
  // ---- shared ----
  'components/stats-bar.tsx': {
    family: 'shared',
    sites: { link: 1 },
    reason: `the epoch vital's words at 12 px below md, inside its link; ${WORK}`,
  },
  // 'components/ui/chart.tsx' left the list (lane cd-fixes-controls): the markers' legend, links,
  // reads at 13 px
  'components/ui/lock-kind.tsx': {
    family: 'shared',
    sites: { link: 1 },
    reason: `an unregistered lock's code-hash link in the 12 px lock-kind label; ${WORK}`,
  },
};

const ROOT = join(__dirname, '..', '..');

/** The files whose sites differ from their entry, each with what to do. */
function toleratedFailures(
  found: ReadonlyMap<string, readonly Site[]>,
  list: Readonly<Record<string, Tolerated>>
): string[] {
  const out: string[] = [];
  for (const path of [...new Set([...found.keys(), ...Object.keys(list)])].sort()) {
    const sites = found.get(path) ?? [];
    const entry = list[path];
    for (const kind of KINDS) {
      const of = sites.filter((s) => s.kind === kind);
      const allowed = entry?.sites[kind] ?? 0;
      if (of.length > allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s) at 12 px, ${allowed} tolerated — ${of.map((s) => `${s.line} ${s.what}`).join(', ')} (13 px: text-small, or the size of the line around it)`
        );
      else if (of.length < allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s) at 12 px, ${allowed} tolerated — lower its count to ${of.length}`
        );
    }
    if (entry && entry.reason.trim().length <= 10)
      out.push(`${path}: a reason too short to say why: "${entry.reason}"`);
  }
  return out;
}

describe('no link and no table cell at 12 px but the work list’s', () => {
  const found = new Map<string, Site[]>();
  for (const file of sourceFiles(ROOT)) {
    if (!file.endsWith('.tsx')) continue;
    const sites = sizeRoleSites(readFileSync(file, 'utf8'));
    if (sites.length) found.set(relative(ROOT, file).split(sep).join('/'), sites);
  }

  it('writes no link and no cell at the meta size but the tolerated list’s', () => {
    expect(toleratedFailures(found, TOLERATED)).toEqual([]);
  });

  it('tolerates the pinned sites by family and the pinned files, which only fall', () => {
    const byFamily: Partial<Record<Family, number>> = {};
    for (const entry of Object.values(TOLERATED))
      byFamily[entry.family] =
        (byFamily[entry.family] ?? 0) +
        Object.values(entry.sites).reduce((sum, n) => sum + (n ?? 0), 0);
    const pinned = Object.fromEntries(Object.entries(TOLERATED_BY_FAMILY).filter(([, n]) => n > 0));
    expect(byFamily).toEqual(pinned);
    expect(Object.keys(TOLERATED)).toHaveLength(TOLERATED_FILES);
  });
});
