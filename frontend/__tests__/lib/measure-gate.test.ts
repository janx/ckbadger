import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { MEASURE_CHARS } from '@/lib/type-metrics';

/**
 * Gate for the measure (readability plan 2026-11-18, ruling R4, lane B7 `rd/prose`; the review's
 * pattern D): a paragraph of running text never stands outside the measure, the one class
 * `max-w-measure` (76 characters of the element that carries it, `MEASURE_CHARS` in
 * lib/type-metrics.ts; 638.4 px at the body step). A line of 171 characters (the cell page's out
 * point sentence across 1,232 px) is read with the eye running back across the whole page.
 *
 * What it counts as a paragraph of running text, read from the source:
 *
 * - `p`: every `<p>`;
 * - `block`: any other element that holds text as a block (a `div`, `li`, `dd`, `dt`,
 *   `figcaption`, `blockquote`, `section`, …, a `span` set `block`) whose own text is longer than
 *   110 characters (the reading probe's threshold, the review's `read.cjs`): the text the source
 *   writes in it and in its inline children (a link, a `<span>`, an `<em>`, any component's JSX
 *   children).
 *
 * It stands inside the measure when it or a JSX ancestor in its file carries `max-w-measure` at
 * rest (no variant); when it is a `<p>` inside `<Prose>` (`.prose-tier2 p` applies the class in
 * app/globals.css, checked below); or when it is handed to a slot that sets its content in the
 * measure (`MEASURED_SLOTS`: a page's lede, checked below in each component's own source). The
 * walk up goes through props and render callbacks to the element that receives them, since what a
 * component renders stands inside that element.
 *
 * Not a paragraph, so not counted:
 *
 * - a `<p>` whose whole text the source fixes and that is no longer than the measure ("Loading
 *   transactions…"): it cannot run past 76 characters;
 * - an element in a table cell (a `<td>`, a `<th>`, a `ListTable` column's `render`): `ListTable`
 *   owns every horizontal pixel of a cell, and no measure is put inside one;
 * - an element set on one line (`truncate`, `whitespace-nowrap`) or laid out as a row of items
 *   (`flex`, `grid`): a line or a row, not running text;
 * - `<pre>`, SVG text, headings, controls.
 *
 * What it cannot see, because it reads source and not layout:
 *
 * - the width a paragraph is given. A paragraph in a column narrower than the measure (a 260 px
 *   rail, a grid cell, a phone's 358 px) is already narrower, and the class does nothing there;
 *   the gate still asks for it, since the source cannot prove a column narrow at every width.
 * - text an expression builds (a variable, a function, a prop, a formatted number): it has no
 *   length here. Such a block is counted only when it is a `<p>`, or when its written text alone
 *   passes 110 characters.
 * - the step a paragraph is set at under a measured ancestor: `76ch` is the ancestor's own
 *   characters, so text a step smaller inside it runs a few more per line (a 12 px note inside a
 *   14 px measure holds 88). Where a paragraph's step is smaller, the class goes on it.
 * - a component that inks or wraps its children: the walk sees only the elements of the file; a
 *   paragraph rendered by a component is counted in that component's own file.
 * - a `max-width` set any other way (a stylesheet rule, an inline style, an arbitrary
 *   `max-w-[…]` class): only the class is the measure.
 *
 * The sites tolerated today are listed per file with their count per kind and a reason. A new site
 * fails (its file's count is exceeded, or the file is not listed) and so does a site that went
 * (its count must be lowered), so the list only shrinks; its length is pinned.
 */

type Kind = 'p' | 'block';
const KINDS: readonly Kind[] = ['p', 'block'];

interface Tolerated {
  /** The sites of each kind in the file, today. */
  sites: Partial<Record<Kind, number>>;
  /** Why they stand, in one line. */
  reason: string;
}

const TOLERATED: Readonly<Record<string, Tolerated>> = {
  // ---- a paragraph a component returns into a Prose the walk cannot reach (`.prose-tier2 p`
  // sets it on the measure; Prose's rule: its paragraphs carry no class of their own) ----
  'app/network/network-map.tsx': {
    sites: { p: 1 },
    reason: 'NetworkMapNotes: set in the network page’s tier-2 Prose (HowTheCrawlerSees)',
  },
  'app/network/trends.tsx': {
    sites: { p: 1 },
    reason: 'NetworkTrendNotes: set in the network page’s tier-2 Prose (HowTheCrawlerSees)',
  },
  'components/asset/genetics.tsx': {
    sites: { p: 1 },
    reason: 'DnaParagraph: set in the object page’s “This object, in words” Prose',
  },
  'components/fiber/fiber-channel-board.tsx': {
    sites: { p: 1 },
    reason: 'Summary, the channel’s story: set in the same file’s tier-2 Prose, a call away',
  },
  'components/fiber/fiber-network-graph.tsx': {
    sites: { p: 1 },
    reason: 'FiberPartiesNote: set in the Fiber page’s “What the chain sees of Fiber” Prose',
  },
  // ---- not running text ----
  'app/tx/[hash]/client-page.tsx': {
    sites: { p: 2 },
    reason:
      'raw data: a witness’s bytes and a header dep’s hash in hex, broken anywhere, read in chunks',
  },
  'components/command-palette.tsx': {
    sites: { p: 1 },
    reason:
      'chrome: the shortcuts panel’s release and clock line, its rule spanning the 672 px dialog',
  },
  // ---- another lane's file ----
  'components/ui/list-table.tsx': {
    sites: { p: 1 },
    reason: 'the tables lane’s ListTable: its empty-list sentence, under a table’s full width',
  },
};

/**
 * The slots that set what they are handed in the measure: a component's prop whose content its
 * own source wraps in `max-w-measure` (checked below, file by file).
 */
const MEASURED_SLOTS: Readonly<Record<string, { file: string; props: readonly string[] }>> = {
  DetailLayout: { file: 'components/ui/detail-layout.tsx', props: ['lede'] },
  AssetDetailShell: { file: 'components/asset/detail-shell.tsx', props: ['lede'] },
  TopicHero: { file: 'components/ui/topic-hero.tsx', props: ['lede'] },
};

// ---- the scan ----

const SCAN_DIRS = ['app', 'components'];
const MEASURE_CLASS = 'max-w-measure';

/** Tags whose text runs inside the line of their parent. */
const INLINE = new Set([
  'a',
  'abbr',
  'b',
  'bdi',
  'bdo',
  'br',
  'cite',
  'code',
  'data',
  'dfn',
  'em',
  'i',
  'kbd',
  'mark',
  'q',
  's',
  'samp',
  'small',
  'span',
  'strong',
  'sub',
  'sup',
  'time',
  'u',
  'var',
  'wbr',
]);
/** Tags that hold text as a block, other than `p`. */
const TEXT_BLOCKS = new Set([
  'address',
  'article',
  'aside',
  'blockquote',
  'caption',
  'dd',
  'details',
  'div',
  'dt',
  'figcaption',
  'footer',
  'header',
  'legend',
  'li',
  'main',
  'nav',
  'section',
]);
/** Classes that set an element on one line or lay it out as a row of items (at rest). */
const NOT_RUNNING = new Set(['truncate', 'whitespace-nowrap', 'flex', 'inline-flex', 'grid']);
/** Classes that make a `span` a block. */
const SPAN_BLOCK = new Set(['block', 'flex', 'grid', 'inline-block']);

interface Site {
  kind: Kind;
  line: number;
  what: string;
}

function unwrap(e: ts.Expression): ts.Expression {
  while (
    ts.isParenthesizedExpression(e) ||
    ts.isAsExpression(e) ||
    ts.isSatisfiesExpression(e) ||
    ts.isNonNullExpression(e)
  )
    e = e.expression;
  return e;
}

/** A class token with no variant (`max-w-measure`, not `md:max-w-measure` or `hover:…`). */
const atRest = (token: string) => !token.includes(':');

/** Every element of a TSX source that is a paragraph of running text outside the measure. */
function measureSites(code: string): Site[] {
  const source = ts.createSourceFile(
    'scan.tsx',
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX
  );
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;

  const locals = new Map<string, ts.Expression[]>();
  const collect = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      locals.set(node.name.text, [...(locals.get(node.name.text) ?? []), node.initializer]);
    ts.forEachChild(node, collect);
  };
  collect(source);

  /** The strings a class expression can reach: literals, branches, `cn(…)`, the file's constants. */
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
    if (ts.isIdentifier(e)) return (locals.get(e.text) ?? []).flatMap(next);
    return [];
  };

  type El = ts.JsxElement | ts.JsxSelfClosingElement;
  const opening = (n: El): ts.JsxOpeningLikeElement => (ts.isJsxElement(n) ? n.openingElement : n);
  const tagOf = (n: ts.JsxOpeningLikeElement) => n.tagName.getText(source);
  const classTokens = (n: ts.JsxOpeningLikeElement): string[] => {
    for (const a of n.attributes.properties) {
      if (!ts.isJsxAttribute(a) || a.name.getText(source) !== 'className' || !a.initializer)
        continue;
      const init = a.initializer;
      const e = ts.isJsxExpression(init) ? init.expression : init;
      return e ? leaves(e).flatMap((s) => s.split(/\s+/).filter(Boolean)) : [];
    }
    return [];
  };
  /** A class that holds in some branch of the expression: a conditional measure counts too. */
  const has = (n: ts.JsxOpeningLikeElement, set: ReadonlySet<string>) =>
    classTokens(n).some((t) => atRest(t) && set.has(t));
  const MEASURE = new Set([MEASURE_CLASS]);

  /** The text an element writes as its own, and whether any of it is built by an expression. */
  const ownText = (n: El): { text: string; built: boolean } => {
    if (!ts.isJsxElement(n)) return { text: '', built: false };
    let text = '';
    let built = false;
    const add = (children: ts.NodeArray<ts.JsxChild>) => {
      for (const c of children) {
        if (ts.isJsxText(c)) text += c.text;
        else if (ts.isJsxExpression(c)) {
          if (!c.expression) continue;
          const e = unwrap(c.expression);
          if (ts.isStringLiteralLike(e)) text += e.text;
          else built = true;
        } else if (ts.isJsxFragment(c)) add(c.children);
        else if (ts.isJsxElement(c) || ts.isJsxSelfClosingElement(c)) {
          const tag = tagOf(opening(c));
          const component = /^[A-Z]/.test(tag) || tag.includes('.');
          if (component) built = true;
          if (component || (INLINE.has(tag) && !(tag === 'span' && has(opening(c), SPAN_BLOCK)))) {
            const inner = ownText(c);
            text += inner.text;
            built ||= inner.built;
          }
        }
      }
    };
    add(n.children);
    return { text: text.replace(/\s+/g, ' ').trim(), built };
  };

  /**
   * Where an element stands: `measured` (the class, a Prose paragraph, a measured slot), `cell`
   * (a table cell's content) or `open`. The walk goes up through props and callbacks.
   */
  const standing = (node: El, isP: boolean): 'measured' | 'cell' | 'open' => {
    for (let p: ts.Node | undefined = node.parent; p; p = p.parent) {
      if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
        const o = opening(p);
        const tag = tagOf(o);
        if (has(o, MEASURE)) return 'measured';
        if (isP && tag === 'Prose') return 'measured';
        if (tag === 'td' || tag === 'th') return 'cell';
      } else if (ts.isJsxAttribute(p)) {
        const owner = p.parent.parent;
        if (MEASURED_SLOTS[tagOf(owner)]?.props.includes(p.name.getText(source))) return 'measured';
      } else if (ts.isObjectLiteralExpression(p)) {
        const names = new Set(
          p.properties.map((x) => {
            const name = (x as { name?: ts.PropertyName }).name;
            return name && (ts.isIdentifier(name) || ts.isStringLiteralLike(name)) ? name.text : '';
          })
        );
        if (names.has('render') && names.has('minWidth')) return 'cell';
      }
    }
    return 'open';
  };

  const sites: Site[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const o = opening(node);
      const tag = tagOf(o);
      const candidate =
        tag === 'p' || TEXT_BLOCKS.has(tag) || (tag === 'span' && has(o, SPAN_BLOCK));
      if (candidate && !has(o, MEASURE) && !has(o, NOT_RUNNING)) {
        const { text, built } = ownText(node);
        const kind: Kind | null =
          tag === 'p'
            ? built || text.length > MEASURE_CHARS
              ? 'p'
              : null
            : text.length > 110
              ? 'block'
              : null;
        if (kind !== null && standing(node, tag === 'p') === 'open')
          sites.push({ kind, line: lineOf(node), what: `<${tag}>` });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return sites;
}

/**
 * Whether a component's own source sets the content of a prop in the measure: the element that
 * writes `{prop}` among its children, or one of its ancestors in the file, carries the class.
 */
function slotMeasured(code: string, prop: string): boolean {
  const source = ts.createSourceFile(
    'slot.tsx',
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX
  );
  const uses: ts.JsxExpression[] = [];
  const visit = (node: ts.Node) => {
    if (
      ts.isJsxExpression(node) &&
      node.expression &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === prop &&
      ts.isJsxElement(node.parent)
    )
      uses.push(node);
    ts.forEachChild(node, visit);
  };
  visit(source);
  const measured = (use: ts.Node) => {
    for (let p: ts.Node | undefined = use.parent; p; p = p.parent)
      if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
        const o = ts.isJsxElement(p) ? p.openingElement : p;
        const cls = o.attributes.properties.find(
          (a) => ts.isJsxAttribute(a) && a.name.getText(source) === 'className'
        );
        if (
          cls &&
          new RegExp(`(^|[\\s'"\`])${MEASURE_CLASS}($|[\\s'"\`])`).test(cls.getText(source))
        )
          return true;
      }
    return false;
  };
  return uses.length > 0 && uses.every(measured);
}

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith('.tsx')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

/** The scan's count against the list: every file's count per kind must equal its tolerated one. */
function toleratedFailures(
  found: ReadonlyMap<string, readonly Site[]>,
  tolerated: Readonly<Record<string, Tolerated>>
): string[] {
  const out: string[] = [];
  const paths = new Set([...found.keys(), ...Object.keys(tolerated)]);
  for (const path of [...paths].sort()) {
    const sites = found.get(path) ?? [];
    const entry = tolerated[path];
    for (const kind of KINDS) {
      const of = sites.filter((s) => s.kind === kind);
      const allowed = entry?.sites[kind] ?? 0;
      if (of.length > allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s) outside the measure, ${allowed} tolerated — a new one: ${of
            .map((s) => `${s.line} ${s.what}`)
            .join(', ')} (running text takes max-w-measure, on itself or a block around it)`
        );
      if (of.length < allowed)
        out.push(
          `${path}: ${of.length} ${kind} site(s) outside the measure, ${allowed} tolerated — lower its count to ${of.length}`
        );
    }
    if (entry && entry.reason.trim().length <= 10)
      out.push(`${path}: a reason too short to say why: "${entry.reason}"`);
  }
  return out;
}

describe('the measure scan finds what it must, and only that', () => {
  const sitesOf = (lines: string[]) =>
    measureSites(lines.join('\n')).map((s) => `${s.line} ${s.kind} ${s.what}`);
  const long = 'x'.repeat(111);

  it('finds a paragraph built by an expression, a long written one and a long block', () => {
    expect(
      sitesOf([
        '<p className="text-body">Loading {label}…</p>;',
        `<p>${'a '.repeat(39)}</p>;`,
        `<div className="text-small">${long}</div>;`,
        `<li>${long}</li>;`,
        '<p>The block <BlockLink number={n} /> spent it.</p>;',
        `<span className="block text-meta">${long}</span>;`,
      ])
    ).toEqual(['1 p <p>', '2 p <p>', '3 block <div>', '4 block <li>', '5 p <p>', '6 block <span>']);
  });

  it('counts a block’s inline children and component children as its own text', () => {
    expect(
      sitesOf([
        `<div>${'w'.repeat(60)} <a href="/x">${'l'.repeat(30)}</a> <Link href="/y">${'k'.repeat(30)}</Link></div>;`,
        `<div>${'w'.repeat(60)} <div>${'d'.repeat(60)}</div></div>;`,
      ])
    ).toEqual(['1 block <div>']);
  });

  it('passes the measure on the element, on an ancestor, through a callback or a prop', () => {
    expect(
      sitesOf([
        '<p className="text-body max-w-measure">Loading {label}…</p>;',
        '<div className="max-w-measure"><p>{a}</p><div>{rows.map((r) => <p key={r}>{r}</p>)}</div></div>;',
        "<p className={cn('text-small', wide && 'mt-2', 'max-w-measure')}>{x}</p>;",
        "const PROSE = 'text-body max-w-measure';",
        '<p className={PROSE}>{x}</p>;',
        '<div className="max-w-measure"><Section body={<p>{x}</p>} /></div>;',
      ])
    ).toEqual([]);
  });

  it('passes a Prose paragraph and a measured slot, and only those', () => {
    expect(
      sitesOf([
        '<Prose><p>{x}</p></Prose>;',
        '<DetailLayout title="t" lede={<p>{x}</p>} />;',
        '<TopicHero title="t" lede={<>{x}</>} figures={[]} />;',
        '<DetailLayout title="t" tier2={<p>{x}</p>} />;',
        `<Prose><div>${long}</div></Prose>;`,
      ])
    ).toEqual(['4 p <p>', '5 block <div>']);
  });

  it('refuses a measure behind a variant or written any other way', () => {
    expect(
      sitesOf([
        '<p className="md:max-w-measure">{x}</p>;',
        '<p className="max-w-[640px]">{x}</p>;',
        "<p style={{ maxWidth: '76ch' }}>{x}</p>;",
      ])
    ).toEqual(['1 p <p>', '2 p <p>', '3 p <p>']);
  });

  it('passes what is not running text: a short written line, a cell, a line, a row, a heading', () => {
    expect(
      sitesOf([
        '<p className="text-text-dim">Loading transactions…</p>;',
        `<p>${'a'.repeat(MEASURE_CHARS)}</p>;`,
        '<td><p>{x}</p></td>;',
        "const columns = [{ key: 'a', minWidth: 80, render: (r) => <p>{r.a}</p> }];",
        '<p className="truncate">{x}</p>;',
        '<p className="flex flex-wrap gap-2">{x}</p>;',
        `<h2>${long}</h2>;`,
        `<pre>${long}</pre>;`,
        `<span className="text-small">${long}</span>;`,
      ])
    ).toEqual([]);
  });
});

describe('a slot is measured where its component writes it', () => {
  it('finds the class on the element that writes the prop, or on one around it', () => {
    expect(slotMeasured('<div className="mt-4 max-w-measure">{lede}</div>;', 'lede')).toBe(true);
    expect(
      slotMeasured(
        '<section className="max-w-measure"><p className="x">{lede}</p></section>;',
        'lede'
      )
    ).toBe(true);
    expect(slotMeasured('<div className="mt-4">{lede}</div>;', 'lede')).toBe(false);
    expect(
      slotMeasured(
        '<div className="max-w-measure">{lede}</div>; <div className="mt-4">{lede}</div>;',
        'lede'
      )
    ).toBe(false);
    expect(slotMeasured('<div className="max-w-measure">{title}</div>;', 'lede')).toBe(false);
  });
});

describe('the tolerated list keys a file by its count per kind, so it only shrinks', () => {
  const site = (kind: Kind, line: number): Site => ({ kind, line, what: '<p>' });
  const list: Record<string, Tolerated> = {
    'app/a.tsx': { sites: { p: 2 }, reason: 'two banners whose frame spans the column' },
  };

  it('passes the same count on other lines', () => {
    expect(toleratedFailures(new Map([['app/a.tsx', [site('p', 3), site('p', 9)]]]), list)).toEqual(
      []
    );
  });

  it('fails a new site in a listed file or an unlisted one', () => {
    expect(
      toleratedFailures(
        new Map([
          ['app/a.tsx', [site('p', 3), site('p', 9), site('block', 12)]],
          ['app/b.tsx', [site('p', 4)]],
        ]),
        list
      )
    ).toEqual([
      expect.stringMatching(
        /^app\/a\.tsx: 1 block site\(s\) outside the measure, 0 tolerated — a new one: 12 <p>/
      ),
      expect.stringMatching(
        /^app\/b\.tsx: 1 p site\(s\) outside the measure, 0 tolerated — a new one: 4 <p>/
      ),
    ]);
  });

  it('fails a site that went until its count is lowered', () => {
    expect(toleratedFailures(new Map([['app/a.tsx', [site('p', 3)]]]), list)).toEqual([
      'app/a.tsx: 1 p site(s) outside the measure, 2 tolerated — lower its count to 1',
    ]);
  });
});

describe('running text stands inside the measure (R4, lane B7)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = new Map<string, Site[]>();
  for (const file of files) {
    const sites = measureSites(readFileSync(file, 'utf8'));
    if (sites.length) found.set(relative(root, file), sites);
  }

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  it('sets a Prose paragraph in the measure: .prose-tier2 p applies the class', () => {
    const css = readFileSync(join(root, 'app', 'globals.css'), 'utf8');
    const p = css.match(/\.prose-tier2 p\s*\{([^}]*)\}/)![1];
    expect(p).toContain(`@apply ${MEASURE_CLASS};`);
    expect(p).not.toMatch(/max-width/);
  });

  it('sets every measured slot in the measure in its component’s own source', () => {
    for (const [component, { file, props }] of Object.entries(MEASURED_SLOTS))
      for (const prop of props)
        expect(
          slotMeasured(readFileSync(join(root, file), 'utf8'), prop),
          `${component}.${prop}`
        ).toBe(true);
  });

  it('sets no paragraph of running text outside the measure but the tolerated list’s', () => {
    expect(toleratedFailures(found, TOLERATED)).toEqual([]);
  });

  it('keeps the tolerated list short and reasoned: its length is pinned', () => {
    // pinned: a file more is a decision, not a line added here (9 sites in 8 files)
    expect(Object.keys(TOLERATED)).toHaveLength(8);
    expect(
      Object.values(TOLERATED).reduce((n, t) => n + (t.sites.p ?? 0) + (t.sites.block ?? 0), 0)
    ).toBe(9);
  });
});
