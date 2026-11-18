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
 * What it reads as running text, from the source:
 *
 * - `p`: every `<p>`, unless every word of it is written in the source and it is no longer than
 *   the measure ("Loading transactions…" cannot run past 76 characters);
 * - `block`: any other intrinsic element that holds text as a block (a `div`, `li`, `dd`, `dt`,
 *   `figcaption`, `blockquote`, `section`, …, a `span` set `block`) whose written text — the text
 *   the source writes in it and in its inline children (a link, a `<span>`, an `<em>`, the children
 *   of a component, but a component the file declares stands as the one element it returns) — is
 *   longer than the measure;
 * - `tag`: a tag it cannot read (a constant it cannot resolve to strings, a call that is not a
 *   component factory, a component passed as a value), refused by name. A tag held in a constant
 *   (`inline ? 'span' : 'p'`) or in a destructured prop (`as: Tag = 'div'`, its declared string
 *   union) is read as every tag it can be.
 *
 * A class is read through `?:`, `&&`, `||`, `??`, templates, `cn` and `clsx` (an object's keys each
 * there or not), the file's constants and the string constants it imports from the app's own
 * modules; any other call, a prop or a property is not read and proves nothing. A class counts only
 * when every way the expression can evaluate holds it, an exemption too.
 *
 * It stands inside the measure when an intrinsic element, it or one around it in its file, holds
 * `max-w-measure` at rest (no variant) in every branch; when it is a `<p>` inside `<Prose>`
 * (`.prose-tier2 p` applies the class in app/globals.css, checked below); or when it is handed to a
 * slot that sets its content in the measure (`MEASURED_SLOTS`: a page's lede, checked below in
 * each component's own source). The class on a component proves nothing (what the component does
 * with it is in its own file). The walk up goes through props and render callbacks to the element
 * that receives them, since what a component renders stands inside that element.
 *
 * Not running text, so not counted:
 *
 * - an element in a table cell (a `<td>`, a `<th>`, a `ListTable` column's `render`): `ListTable`
 *   owns every horizontal pixel of a cell, and no measure is put inside one;
 * - an element set on one line (`truncate`, `whitespace-nowrap`) or laid out as a row of items
 *   (`flex`, `inline-flex`, `grid`), in every branch of its class: a line or a row;
 * - `<pre>`, SVG text, headings, controls.
 *
 * What it does not read, and so cannot hold:
 *
 * - a block other than a `<p>` whose text an expression builds: a list item, a definition, a
 *   note or a `<div>` that writes `{note}` is read only by the text written beside it. A `dd` or an
 *   `li` holds a value or a row as often as a sentence, and the source cannot tell which; such a
 *   block takes the class where it holds a sentence (the reward notes, the calculation scope's
 *   items), by hand.
 * - the width a paragraph is given. A paragraph in a column narrower than the measure (a 260 px
 *   rail, a grid cell, a phone's 358 px) is already narrower, and the class does nothing there;
 *   the gate still asks for it, since the source cannot prove a column narrow at every width.
 * - the step a paragraph is set at under a measured ancestor: `76ch` is the ancestor's own
 *   characters, so text a step smaller inside it runs a few more per line (a 12 px note inside a
 *   14 px measure holds 88). Where a paragraph's step is smaller, the class goes on it.
 * - a paragraph a component renders into another file's Prose (listed below with that reason).
 * - a `max-width` set any other way (a stylesheet rule, an inline style, an arbitrary
 *   `max-w-[…]` class): only the class is the measure.
 *
 * The sites tolerated today are listed per file with their count per kind and a reason. A new site
 * fails (its file's count is exceeded, or the file is not listed) and so does a site that went
 * (its count must be lowered), so the list only shrinks; its length is pinned.
 */

type Kind = 'p' | 'block' | 'tag';
const KINDS: readonly Kind[] = ['p', 'block', 'tag'];

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

/** A part of a class the scan cannot read: a token that proves nothing. */
const UNREAD = '\u0000unread';
/** The calls whose arguments are a class list (`lib/utils.ts` `cn`, and `clsx` under it). */
const CLASS_CALLS = new Set(['cn', 'clsx']);
/** More ways than this for one class to evaluate, and it is not read. */
const MAX_ALTERNATIVES = 64;

/** What a JSX tag is: the intrinsic tags it can be, a component, or a tag the scan cannot read. */
type TagRead = { tags: readonly string[] } | 'component' | 'unread';

/** The app's own modules, parsed once: an imported class constant is read from its source. */
const MODULES = new Map<string, ts.SourceFile | null>();
const FRONTEND = join(__dirname, '..', '..');
function moduleSource(from: string | undefined, specifier: string): ts.SourceFile | null {
  const base = specifier.startsWith('@/')
    ? join(FRONTEND, specifier.slice(2))
    : specifier.startsWith('.') && from
      ? join(from, '..', specifier)
      : null;
  if (base === null) return null;
  for (const path of [`${base}.ts`, `${base}.tsx`, join(base, 'index.ts')]) {
    if (!MODULES.has(path)) {
      let text: string | null = null;
      try {
        text = readFileSync(path, 'utf8');
      } catch {
        text = null;
      }
      MODULES.set(
        path,
        text === null
          ? null
          : ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
      );
    }
    const found = MODULES.get(path);
    if (found) return found;
  }
  return null;
}

/** The calls that make a component of a function (`memo(…)`, `forwardRef(…)`, `lazy`, `dynamic`). */
const COMPONENT_FACTORIES = new Set(['memo', 'forwardRef', 'lazy', 'dynamic']);

/**
 * One file's scan: its paragraphs outside the measure, and whether a prop is set in it. `path` is
 * the file's own, to read the class constants it imports from the app's modules.
 */
function scanFile(code: string, path?: string) {
  const source = ts.createSourceFile(
    'scan.tsx',
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX
  );
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;

  const locals = new Map<string, ts.Expression[]>();
  const components = new Set<string>();
  const bindings = new Map<string, ts.BindingElement[]>();
  const types = new Map<string, ts.Node>();
  const imports = new Map<string, { specifier: string; name: string }>();
  const parameters = new Set<string>();
  /** A component the file declares, by the one element it returns, when it returns one. */
  const roots = new Map<string, ts.JsxOpeningLikeElement | null>();
  const rootOf = (fn: ts.FunctionLikeDeclaration): ts.JsxOpeningLikeElement | null => {
    const returned: ts.Expression[] = [];
    if (fn.body && !ts.isBlock(fn.body)) returned.push(fn.body);
    else if (fn.body) {
      const walk = (n: ts.Node) => {
        if (ts.isFunctionLike(n) && n !== fn) return;
        if (ts.isReturnStatement(n) && n.expression) returned.push(n.expression);
        ts.forEachChild(n, walk);
      };
      walk(fn.body);
    }
    if (returned.length !== 1) return null;
    const e = unwrap(returned[0]);
    return ts.isJsxElement(e) ? e.openingElement : ts.isJsxSelfClosingElement(e) ? e : null;
  };
  const collect = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      locals.set(node.name.text, [...(locals.get(node.name.text) ?? []), node.initializer]);
    if (ts.isImportDeclaration(node) && node.importClause) {
      const clause = node.importClause;
      const specifier = (node.moduleSpecifier as ts.StringLiteral).text;
      if (clause.name) components.add(clause.name.text);
      const named = clause.namedBindings;
      if (named && ts.isNamedImports(named))
        for (const e of named.elements) {
          components.add(e.name.text);
          imports.set(e.name.text, { specifier, name: (e.propertyName ?? e.name).text });
        }
      if (named && ts.isNamespaceImport(named)) components.add(named.name.text);
    }
    if ((ts.isFunctionDeclaration(node) || ts.isClassDeclaration(node)) && node.name)
      components.add(node.name.text);
    if (ts.isFunctionDeclaration(node) && node.name) roots.set(node.name.text, rootOf(node));
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.initializer &&
      (ts.isArrowFunction(node.initializer) || ts.isFunctionExpression(node.initializer)) &&
      /^[A-Z]/.test(node.name.text)
    )
      roots.set(node.name.text, rootOf(node.initializer));
    if (ts.isParameter(node) && ts.isIdentifier(node.name)) parameters.add(node.name.text);
    if (ts.isBindingElement(node) && ts.isIdentifier(node.name))
      bindings.set(node.name.text, [...(bindings.get(node.name.text) ?? []), node]);
    if (ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node))
      types.set(node.name.text, node);
    ts.forEachChild(node, collect);
  };
  collect(source);

  const space = (parts: readonly string[]) => parts.join(' ');
  /** Every combination of one string from each list, joined by a space; null past the cap. */
  const product = (lists: readonly (readonly string[])[]): string[] | null => {
    let out: string[] = [''];
    for (const list of lists) {
      out = out.flatMap((a) => list.map((b) => space([a, b])));
      if (out.length > MAX_ALTERNATIVES) return null;
    }
    return out;
  };

  /**
   * The ways a class expression can evaluate, each as one string of classes: a literal; each
   * branch of `?:`; `a && b` as nothing or `b`; `a || b` and `a ?? b` as either side; a template
   * with each of its parts; `cn` and `clsx` with every argument (an object's keys each there or
   * not); the file's constants. Anything else is UNREAD, which proves nothing.
   */
  const classes = (e0: ts.Expression | undefined, depth = 0): string[] => {
    if (e0 === undefined) return [''];
    if (depth > 12) return [UNREAD];
    const e = unwrap(e0);
    const next = (x: ts.Expression) => classes(x, depth + 1);
    if (ts.isStringLiteralLike(e)) return [e.text];
    if (
      e.kind === ts.SyntaxKind.NullKeyword ||
      e.kind === ts.SyntaxKind.FalseKeyword ||
      e.kind === ts.SyntaxKind.TrueKeyword ||
      (ts.isIdentifier(e) && e.text === 'undefined')
    )
      return [''];
    if (ts.isTemplateExpression(e)) {
      let out: string[] = [e.head.text];
      for (const span of e.templateSpans) {
        out = out.flatMap((a) => next(span.expression).map((b) => a + b + span.literal.text));
        if (out.length > MAX_ALTERNATIVES) return [UNREAD];
      }
      return out;
    }
    if (ts.isConditionalExpression(e)) return [...next(e.whenTrue), ...next(e.whenFalse)];
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return ['', ...next(e.right)];
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return [...next(e.left), ...next(e.right)];
      return [UNREAD];
    }
    if (
      ts.isCallExpression(e) &&
      ts.isIdentifier(e.expression) &&
      CLASS_CALLS.has(e.expression.text)
    )
      return product(e.arguments.map((a) => argClasses(a, depth + 1))) ?? [UNREAD];
    if (ts.isIdentifier(e)) {
      const values = locals.get(e.text);
      if (values) return values.flatMap(next);
      const imported = imports.get(e.text);
      return imported ? importedClasses(imported.specifier, imported.name) : [UNREAD];
    }
    return [UNREAD];
  };
  /** An imported class constant: a string the module exports as a `const`, else UNREAD. */
  const importedClasses = (specifier: string, name: string): string[] => {
    const module = moduleSource(path, specifier);
    if (!module) return [UNREAD];
    for (const statement of module.statements) {
      if (!ts.isVariableStatement(statement)) continue;
      const exported = statement.modifiers?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword);
      if (!exported || !(statement.declarationList.flags & ts.NodeFlags.Const)) continue;
      for (const d of statement.declarationList.declarations)
        if (ts.isIdentifier(d.name) && d.name.text === name && d.initializer) {
          const value = unwrap(d.initializer);
          return ts.isStringLiteralLike(value) ? [value.text] : [UNREAD];
        }
    }
    return [UNREAD];
  };
  /** A `cn` / `clsx` argument: a class expression, an array of them, or an object of conditions. */
  const argClasses = (e0: ts.Expression, depth: number): string[] => {
    const e = unwrap(e0);
    if (ts.isArrayLiteralExpression(e))
      return (
        product(
          e.elements.map((el) => (ts.isSpreadElement(el) ? [UNREAD] : argClasses(el, depth + 1)))
        ) ?? [UNREAD]
      );
    if (ts.isObjectLiteralExpression(e))
      return (
        product(
          e.properties.map((p) => {
            if (!ts.isPropertyAssignment(p)) return [UNREAD];
            const key =
              ts.isIdentifier(p.name) || ts.isStringLiteralLike(p.name) ? p.name.text : null;
            return key === null ? [UNREAD] : ['', key];
          })
        ) ?? [UNREAD]
      );
    return classes(e, depth);
  };

  type El = ts.JsxElement | ts.JsxSelfClosingElement;
  const opening = (n: El): ts.JsxOpeningLikeElement => (ts.isJsxElement(n) ? n.openingElement : n);
  const tagName = (n: ts.JsxOpeningLikeElement) => n.tagName.getText(source);
  const classAttr = (n: ts.JsxOpeningLikeElement): ts.Expression | undefined => {
    for (const a of n.attributes.properties) {
      if (!ts.isJsxAttribute(a) || a.name.getText(source) !== 'className') continue;
      if (!a.initializer) return undefined;
      return ts.isJsxExpression(a.initializer) ? a.initializer.expression : a.initializer;
    }
    return undefined;
  };
  const tokens = (classList: string) => classList.split(/\s+/).filter((t) => t && atRest(t));
  /** Whether every way the element's class can evaluate holds one of the classes. */
  const every = (n: ts.JsxOpeningLikeElement, set: ReadonlySet<string>) =>
    classes(classAttr(n)).every((c) => tokens(c).some((t) => set.has(t)));
  /** Whether some way the element's class can evaluate holds one of the classes. */
  const some = (n: ts.JsxOpeningLikeElement, set: ReadonlySet<string>) =>
    classes(classAttr(n)).some((c) => tokens(c).some((t) => set.has(t)));
  const MEASURE = new Set([MEASURE_CLASS]);

  /** The string-literal union a prop's declared type names, or null where it names anything else. */
  const declaredStrings = (
    type: ts.TypeNode | undefined,
    prop: string,
    depth = 0
  ): string[] | null => {
    if (!type || depth > 6) return null;
    const member = (members: ts.NodeArray<ts.TypeElement>) => {
      const m = members.find(
        (x) => ts.isPropertySignature(x) && x.name.getText(source) === prop
      ) as ts.PropertySignature | undefined;
      return m ? literalUnion(m.type) : undefined;
    };
    if (ts.isTypeLiteralNode(type)) return member(type.members) ?? null;
    if (ts.isIntersectionTypeNode(type)) {
      for (const t of type.types) {
        const found = declaredStrings(t, prop, depth + 1);
        if (found) return found;
      }
      return null;
    }
    if (ts.isTypeReferenceNode(type) && ts.isIdentifier(type.typeName)) {
      const decl = types.get(type.typeName.text);
      if (decl && ts.isInterfaceDeclaration(decl)) {
        const own = member(decl.members);
        if (own !== undefined) return own;
        for (const clause of decl.heritageClauses ?? [])
          for (const t of clause.types)
            if (ts.isIdentifier(t.expression)) {
              const found = declaredStrings(
                ts.factory.createTypeReferenceNode(t.expression.text),
                prop,
                depth + 1
              );
              if (found) return found;
            }
        return null;
      }
      if (decl && ts.isTypeAliasDeclaration(decl))
        return declaredStrings(decl.type, prop, depth + 1);
    }
    return null;
  };
  const literalUnion = (type: ts.TypeNode | undefined): string[] | null => {
    if (!type) return null;
    const parts = ts.isUnionTypeNode(type) ? type.types : [type];
    const out: string[] = [];
    for (const t of parts) {
      if (ts.isLiteralTypeNode(t) && ts.isStringLiteral(t.literal)) out.push(t.literal.text);
      else return null;
    }
    return out;
  };

  /**
   * What a JSX tag is. A lowercase name is that intrinsic tag. A capitalised one is a component
   * when the file imports or declares it as one; a constant is every tag its value can be (`'span'
   * or 'p'`) or a component when every value is one; a destructured prop is the string union its
   * declared type names, with its default. Anything else is a tag the scan cannot read.
   */
  const readTag = (name: ts.JsxTagNameExpression, depth = 0): TagRead => {
    if (!ts.isIdentifier(name)) return 'component'; // `motion.div`, `Foo.Bar`
    const text = name.text;
    if (/^[a-z]/.test(text)) return { tags: [text] };
    return readTagName(text, depth);
  };
  const readTagName = (text: string, depth: number): TagRead => {
    if (depth > 8) return 'unread';
    if (components.has(text)) return 'component';
    const values = locals.get(text);
    if (values) {
      const reads = values.map((v) => readTagValue(v, depth + 1));
      if (reads.every((r) => r === 'component')) return 'component';
      if (reads.some((r) => r === 'unread' || r === 'component')) return 'unread';
      return { tags: reads.flatMap((r) => (r as { tags: readonly string[] }).tags) };
    }
    const bound = bindings.get(text);
    if (!bound && !parameters.has(text)) return 'component'; // a global, as TypeScript resolves it
    if (bound && bound.length === 1) {
      const b = bound[0];
      const prop = b.propertyName
        ? b.propertyName.getText(source)
        : ts.isIdentifier(b.name)
          ? b.name.text
          : null;
      const param = b.parent.parent;
      if (prop !== null && ts.isParameter(param)) {
        const declared = declaredStrings(param.type, prop);
        const fallback =
          b.initializer && ts.isStringLiteralLike(b.initializer) ? [b.initializer.text] : [];
        if (declared) return { tags: [...new Set([...declared, ...fallback])] };
      }
    }
    return 'unread';
  };
  const readTagValue = (e0: ts.Expression, depth: number): TagRead => {
    const e = unwrap(e0);
    if (ts.isStringLiteralLike(e)) return { tags: [e.text] };
    if (ts.isArrowFunction(e) || ts.isFunctionExpression(e)) return 'component';
    if (ts.isCallExpression(e)) {
      const callee = unwrap(e.expression);
      const name = ts.isIdentifier(callee)
        ? callee.text
        : ts.isPropertyAccessExpression(callee)
          ? callee.name.text
          : null;
      return name !== null && COMPONENT_FACTORIES.has(name) ? 'component' : 'unread';
    }
    if (ts.isIdentifier(e)) return readTagName(e.text, depth);
    if (ts.isConditionalExpression(e)) {
      const a = readTagValue(e.whenTrue, depth + 1);
      const b = readTagValue(e.whenFalse, depth + 1);
      if (a === 'component' && b === 'component') return 'component';
      if (a === 'unread' || b === 'unread' || a === 'component' || b === 'component')
        return 'unread';
      return { tags: [...a.tags, ...b.tags] };
    }
    return 'unread';
  };
  const intrinsic = (read: TagRead): readonly string[] | null =>
    typeof read === 'object' ? read.tags : null;

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
          const o = opening(c);
          // a component the file declares stands as the one element it returns (its class read
          // where it is written); another component's or an unread tag's children are counted as
          // this element's text, and it builds more
          const root = ts.isIdentifier(o.tagName) ? roots.get(o.tagName.text) : undefined;
          const as = root ?? o;
          const tags = intrinsic(readTag(as.tagName));
          const inline =
            tags === null ||
            tags.every((t) => INLINE.has(t) && !(t === 'span' && some(as, SPAN_BLOCK)));
          if (tags === null || root) built = true;
          if (inline) {
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
   * Where an element stands: `measured` (an intrinsic element around it holds the class in every
   * branch, it is a Prose paragraph, or it is handed to a measured slot), `cell` (a table cell's
   * content) or `open`. The walk goes up through props and callbacks. A component given the class
   * proves nothing: what it does with a class is in its own file.
   */
  const standing = (node: El, paragraph: boolean): 'measured' | 'cell' | 'open' => {
    for (let p: ts.Node | undefined = node.parent; p; p = p.parent) {
      if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
        const o = opening(p);
        const name = tagName(o);
        const tags = intrinsic(readTag(o.tagName));
        if (tags !== null && every(o, MEASURE)) return 'measured';
        if (paragraph && name === 'Prose') return 'measured';
        if (tags !== null && tags.every((t) => t === 'td' || t === 'th')) return 'cell';
      } else if (ts.isJsxAttribute(p)) {
        const owner = p.parent.parent;
        if (MEASURED_SLOTS[tagName(owner)]?.props.includes(p.name.getText(source)))
          return 'measured';
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
      const read = readTag(o.tagName);
      const what = `<${tagName(o)}>`;
      if (read === 'unread') sites.push({ kind: 'tag', line: lineOf(node), what });
      const tags = intrinsic(read);
      if (tags !== null) {
        const paragraph = tags.includes('p');
        const block =
          paragraph ||
          tags.some((t) => TEXT_BLOCKS.has(t)) ||
          (tags.includes('span') && some(o, SPAN_BLOCK));
        if (block && !every(o, MEASURE) && !every(o, NOT_RUNNING)) {
          const { text, built } = ownText(node);
          const kind: Kind | null =
            (paragraph && built) || text.length > MEASURE_CHARS
              ? paragraph
                ? 'p'
                : 'block'
              : null;
          if (kind !== null && standing(node, paragraph) === 'open')
            sites.push({ kind, line: lineOf(node), what });
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);

  /**
   * Whether the file sets the content of a prop in the measure: every element that writes `{prop}`
   * among its children stands in an element that holds the class in every branch.
   */
  const slotMeasured = (prop: string): boolean => {
    const uses: ts.JsxExpression[] = [];
    const find = (node: ts.Node) => {
      if (
        ts.isJsxExpression(node) &&
        node.expression &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === prop &&
        ts.isJsxElement(node.parent)
      )
        uses.push(node);
      ts.forEachChild(node, find);
    };
    find(source);
    const measured = (use: ts.Node) => {
      for (let p: ts.Node | undefined = use.parent; p; p = p.parent)
        if (ts.isJsxElement(p) || ts.isJsxSelfClosingElement(p)) {
          const o = opening(p);
          if (intrinsic(readTag(o.tagName)) !== null && every(o, MEASURE)) return true;
        }
      return false;
    };
    return uses.length > 0 && uses.every(measured);
  };

  return { sites, slotMeasured };
}

/** Every element of a TSX source that is a paragraph of running text outside the measure. */
function measureSites(code: string, path?: string): Site[] {
  return scanFile(code, path).sites;
}

/** Whether a component's own source sets the content of a prop in the measure. */
function slotMeasured(code: string, prop: string): boolean {
  return scanFile(code).slotMeasured(prop);
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

  // Review of the merged rules branch (tests SHOULD-1): what the scan cannot read, it refuses.

  it('reads a tag held in a variable as every tag it can be, and refuses one it cannot read', () => {
    expect(
      sitesOf([
        "const Sentence = inline ? 'span' : 'p';",
        '<Sentence className="text-body">{x}</Sentence>;',
        "function Note({ as: Tag = 'div' }: { as?: 'div' | 'p' }) { return <Tag>{x}</Tag>; }",
        'const Pick = choose();',
        '<Pick>{x}</Pick>;',
        'function Wide() { return null; }',
        'function Narrow() { return null; }',
        'const Card = wide ? Wide : Narrow;',
        '<Card>{x}</Card>;',
        "const Group = nav ? 'nav' : 'span';",
        '<Group>{x}</Group>;',
      ])
    ).toEqual(['2 p <Sentence>', '3 p <Tag>', '5 tag <Pick>']);
  });

  it('counts a class only when every branch of its expression has it, an exemption too', () => {
    expect(
      sitesOf([
        "<p className={wide ? 'max-w-measure' : 'max-w-[900px]'}>{x}</p>;",
        "<p className={cn('text-body', compact && 'truncate')}>{x}</p>;",
        "<p className={wide ? 'mt-2 max-w-measure' : 'max-w-measure'}>{x}</p>;",
        "<p className={wide ? 'flex' : 'grid'}>{x}</p>;",
        "<div className={on ? 'max-w-measure' : undefined}><p>{x}</p></div>;",
      ])
    ).toEqual(['1 p <p>', '2 p <p>', '5 p <p>']);
  });

  it('trusts the class on an element, never on a component that may not forward it', () => {
    expect(
      sitesOf([
        '<Card className="max-w-measure"><p>{x}</p></Card>;',
        '<section className="max-w-measure"><p>{x}</p></section>;',
      ])
    ).toEqual(['1 p <p>']);
  });

  it('follows cn and clsx to their arguments, and no other call', () => {
    expect(
      sitesOf([
        "<p className={pick('max-w-measure')}>{x}</p>;",
        "<p className={clsx('max-w-measure', on && 'mt-2')}>{x}</p>;",
        "<p className={cn({ 'mt-2': on }, 'max-w-measure')}>{x}</p>;",
      ])
    ).toEqual(['1 p <p>']);
  });

  // The fix round's item 2: a list item or a definition that writes more than the measure holds
  // is running text past it; one whose text an expression builds is not read (a value or a row as
  // often as a sentence), and says so in the header.
  it('counts a block whose written text passes the measure, a list item or a definition too', () => {
    expect(
      sitesOf([
        `<li>${'w '.repeat(39)}</li>;`,
        `<dd>${'w'.repeat(MEASURE_CHARS)}</dd>;`,
        '<li>{item.description}</li>;',
      ])
    ).toEqual(['1 block <li>']);
  });

  // The second fix round (review of the fixes, tests SHOULD-1): the text written at a declared
  // component's call site counts as before, unless the component's root is itself exempt.
  it('counts the text written at a declared component’s call site unless its root is exempt', () => {
    expect(
      sitesOf([
        'function Box({ children }) { return <div>{children}</div>; }',
        `<section><Box>${'w '.repeat(60)}</Box></section>;`,
        'function Row({ children }) { return <div className="flex gap-2">{children}</div>; }',
        `<section><Row>${'w '.repeat(60)}</Row></section>;`,
      ])
    ).toEqual(['2 block <section>']);
  });

  // The second fix round (NOTE-6): `cn` is twMerge(clsx(…)), so a later class of its group wins;
  // a class the scan cannot read may be any class, and a plain list's conflict is the stylesheet's.
  it('reads a cn list as tailwind-merge does, and refuses a class another may override', () => {
    expect(
      sitesOf([
        "<p className={cn('flex gap-2', 'block')}>{x}</p>;",
        "<p className={cn('block', 'flex gap-2')}>{x}</p>;",
        '<p className="flex block">{x}</p>;',
        '<p className="flex md:block">{x}</p>;',
        "<p className={cn('max-w-measure', className)}>{x}</p>;",
        "<p className={cn(className, 'max-w-measure')}>{x}</p>;",
        "<p className={cn('max-w-measure', 'max-w-[400px]')}>{x}</p>;",
        '<p className={`max-w-measure ${extra}`}>{x}</p>;',
      ])
    ).toEqual(['1 p <p>', '3 p <p>', '4 p <p>', '5 p <p>', '7 p <p>', '8 p <p>']);
  });

  it('reads a component the file declares as the element it returns, a factory’s call as one', () => {
    expect(
      sitesOf([
        'function Grid({ children }) { return <div className="grid gap-2">{children}</div>; }',
        `<div><Grid><span>${'w'.repeat(80)}</span></Grid></div>;`,
        `<div><Row><span>${'w'.repeat(80)}</span></Row></div>;`,
        'const Badge = memo(function Badge() { return null; });',
        '<p><Badge /> new</p>;',
      ])
    ).toEqual(['3 block <div>', '5 p <p>']);
  });

  it('reads a class constant the file imports from the app’s own modules, and only those', () => {
    const at = join(__dirname, '..', '..', 'components', 'pipeline', 'probe.tsx');
    const legend = (from: string) =>
      [
        `import { FOOTER_ROW } from '${from}';`,
        `<div className={\`\${FOOTER_ROW} text-meta\`}>${'w '.repeat(50)}</div>;`,
      ].join('\n');
    // svg-text.ts: FOOTER_ROW = 'flex min-h-6 flex-wrap …', a row of items
    expect(measureSites(legend('@/components/pipeline/svg-text'), at)).toEqual([]);
    expect(measureSites(legend('./svg-text'), at)).toEqual([]);
    // a package's or an unknown module's constant is not read: it proves nothing
    expect(measureSites(legend('some-package'), at).map((s) => s.kind)).toEqual(['block']);
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
    // every branch (tests SHOULD-1 b), and an element, not a component (c)
    expect(
      slotMeasured("<div className={wide ? 'max-w-measure' : 'mt-4'}>{lede}</div>;", 'lede')
    ).toBe(false);
    expect(slotMeasured('<Box className="max-w-measure">{lede}</Box>;', 'lede')).toBe(false);
  });
});

/** The tolerated list's sites, summed. */
function toleratedSites(list: Readonly<Record<string, Tolerated>>): number {
  return Object.values(list).reduce((n, t) => n + (t.sites.p ?? 0) + (t.sites.block ?? 0), 0);
}

describe('the tolerated list keys a file by its count per kind, so it only shrinks', () => {
  // The second fix round (NOTE-1): the pinned total counts every kind, a refused tag too.
  it('totals the tolerated sites over every kind', () => {
    expect(
      toleratedSites({ 'app/a.tsx': { sites: { p: 1, block: 2, tag: 1 }, reason: 'three kinds' } })
    ).toBe(4);
  });

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
    const sites = measureSites(readFileSync(file, 'utf8'), file);
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
    expect(toleratedSites(TOLERATED)).toBe(9);
  });
});
